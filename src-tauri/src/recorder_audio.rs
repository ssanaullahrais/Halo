//! WASAPI microphone and loopback capture, isolated from conversion jobs.
use std::{collections::VecDeque,path::PathBuf,sync::{Arc,Mutex,mpsc,atomic::{AtomicBool,AtomicU32,Ordering}},thread,time::{Duration,Instant}};
use wasapi::*;

pub struct AudioTrack {pub mic:bool,pub path:PathBuf,pub join:thread::JoinHandle<Result<(),String>>}
pub fn devices()->Vec<(String,String)> {
    let _=initialize_mta();
    let Ok(enumerator)=DeviceEnumerator::new() else{return vec![]};
    let Ok(collection)=enumerator.get_device_collection(&Direction::Capture) else{return vec![]};
    collection.into_iter().filter_map(|d|{let d=d.ok()?;Some((d.get_id().ok()?,d.get_friendlyname().ok()?))}).collect()
}
pub fn start(path:PathBuf,mic:bool,device_id:String,stop:Arc<AtomicBool>,gate:Arc<Mutex<Option<Instant>>>,peak:Arc<AtomicU32>,mute:Arc<AtomicBool>)->Result<AudioTrack,String> {
    let (tx,rx)=mpsc::sync_channel(1);let output=path.clone();let cancel=stop.clone();
    let join=thread::spawn(move||{
        let diagnostic=output.with_extension("log");let mark=|stage:&str|{let _=std::fs::write(&diagnostic,stage);};
        let result=(||->Result<(),Box<dyn std::error::Error>>{
            mark("COM initialization");
            initialize_sta().ok()?;
            let enumerator=DeviceEnumerator::new()?;
            mark("Selecting device");let device=if mic&&!device_id.is_empty(){enumerator.get_device(&device_id)?}else{enumerator.get_default_device(&if mic{Direction::Capture}else{Direction::Render})?};
            mark("Activating audio client");let mut client=device.get_iaudioclient()?;
            let format=client.get_mixformat()?;let rate=format.get_samplespersec();let channels=format.get_nchannels();let bytes=(format.get_bitspersample()/8) as usize;let block=format.get_blockalign() as usize;let float=matches!(format.get_subformat()?,SampleType::Float);
            let (_,minimum)=client.get_device_period()?;
            mark("Initializing audio stream");client.initialize_client(&format,&Direction::Capture,&StreamMode::EventsShared{autoconvert:false,buffer_duration_hns:minimum})?;
            mark("Getting audio event");let event=client.set_get_eventhandle()?;let capture=client.get_audiocaptureclient()?;
            let mut writer=hound::WavWriter::create(&output,hound::WavSpec{channels,sample_rate:rate,bits_per_sample:32,sample_format:hound::SampleFormat::Float})?;
            mark("Starting audio stream");client.start_stream()?;mark("Capturing");let _=tx.send(Ok(()));
            let mut queue=VecDeque::new();let mut written=0u64;
            while !stop.load(Ordering::Relaxed) {
                capture.read_from_device_to_deque(&mut queue)?;
                let elapsed=gate.lock().unwrap().map(|t|t.elapsed());
                if let Some(elapsed)=elapsed {
                    let wanted=(elapsed.as_secs_f64()*rate as f64) as u64;
                    let available=(queue.len()/block) as u64;
                    // Align quiet loopback periods to the same wall clock as video.
                    if wanted>written+available+4800 {for _ in 0..(wanted-written-available)*channels as u64 {writer.write_sample(0.0f32)?;}written=wanted-available;}
                    let mut level=0f32;
                    while queue.len()>=block && written<wanted+480 {
                        for _ in 0..channels {let mut data=[0u8;4];for v in data.iter_mut().take(bytes){*v=queue.pop_front().unwrap();}let sample=if float{f32::from_le_bytes(data)}else{match bytes{2=>i16::from_le_bytes([data[0],data[1]]) as f32/32768.,3=>((i32::from_le_bytes([0,data[0],data[1],data[2]])) as f64/2147483648.) as f32,4=>i32::from_le_bytes(data) as f32/2147483648.,_=>0.}};let sample=if mute.load(Ordering::Relaxed){0.0}else{sample};level=level.max(sample.abs());writer.write_sample(sample)?;}
                        written+=1;
                    }
                    peak.store(level.min(1.0).to_bits(),Ordering::Relaxed);
                } else {queue.clear();}
                let _=event.wait_for_event(20);
            }
            client.stop_stream()?;
            if let Some(start)=*gate.lock().unwrap(){let wanted=(start.elapsed().as_secs_f64()*rate as f64) as u64;for _ in written..wanted {for _ in 0..channels{writer.write_sample(0.0f32)?;}}}
            writer.finalize()?;peak.store(0,Ordering::Relaxed);Ok(())
        })().map_err(|e|format!("{} audio: {e}",if mic{"Microphone"}else{"System"}));
        if let Err(error)=&result{let _=tx.try_send(Err(error.clone()));}result
    });
    match rx.recv_timeout(Duration::from_secs(8)) {Ok(Ok(()))=>Ok(AudioTrack{mic,path,join}),Ok(Err(error))=>{let _=join.join();Err(error)},Err(_)=>{cancel.store(true,Ordering::Relaxed);Err("The audio device did not become ready. Check Windows microphone permissions and the selected device.".into())}}
}

/// Live settings meters use the same device-native format without writing audio to disk.
pub fn meter(mic:bool,device_id:String,stop:Arc<AtomicBool>,peak:Arc<AtomicU32>)->Result<AudioTrack,String>{
 let(tx,rx)=mpsc::sync_channel(1);let cancel=stop.clone();
 let join=thread::spawn(move||{
  let result=(||->Result<(),Box<dyn std::error::Error>>{
   initialize_sta().ok()?;let enumerator=DeviceEnumerator::new()?;
   let device=if mic&&!device_id.is_empty(){enumerator.get_device(&device_id)?}else{enumerator.get_default_device(&if mic{Direction::Capture}else{Direction::Render})?};
   let mut client=device.get_iaudioclient()?;let format=client.get_mixformat()?;let bytes=(format.get_bitspersample()/8)as usize;let float=matches!(format.get_subformat()?,SampleType::Float);
   if !(1..=4).contains(&bytes){return Err("Unsupported audio device sample format.".into());}
   let(_,minimum)=client.get_device_period()?;client.initialize_client(&format,&Direction::Capture,&StreamMode::EventsShared{autoconvert:false,buffer_duration_hns:minimum})?;
   let event=client.set_get_eventhandle()?;let capture=client.get_audiocaptureclient()?;client.start_stream()?;let _=tx.send(Ok(()));let mut queue=VecDeque::new();let mut held=0f32;
   while !stop.load(Ordering::Relaxed){capture.read_from_device_to_deque(&mut queue)?;let mut level=0f32;while queue.len()>=bytes{let mut data=[0u8;4];for value in data.iter_mut().take(bytes){*value=queue.pop_front().unwrap();}let sample=if float&&bytes==4{f32::from_le_bytes(data)}else{match bytes{1=>(data[0]as f32-128.)/128.,2=>i16::from_le_bytes([data[0],data[1]])as f32/32768.,3=>i32::from_le_bytes([0,data[0],data[1],data[2]])as f32/2147483648.,4=>i32::from_le_bytes(data)as f32/2147483648.,_=>0.}};if sample.is_finite(){level=level.max(sample.abs());}}held=level.max(held*0.9);peak.store(held.min(1.).to_bits(),Ordering::Relaxed);let _=event.wait_for_event(20);}
   client.stop_stream()?;Ok(())
  })().map_err(|e|format!("{} audio preview: {e}",if mic{"Microphone"}else{"System"}));peak.store(0,Ordering::Relaxed);if let Err(e)=&result{let _=tx.try_send(Err(e.clone()));}result
 });
 match rx.recv_timeout(Duration::from_secs(8)){Ok(Ok(()))=>Ok(AudioTrack{mic,path:PathBuf::new(),join}),Ok(Err(e))=>{let _=join.join();Err(e)},Err(_)=>{cancel.store(true,Ordering::Relaxed);Err("The audio device did not become ready for testing.".into())}}
}
