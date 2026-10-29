use super::*;

// Explicitly run these tests with --ignored on a workstation with FFmpeg.
// They exercise the real export graph and inspect decoded pixels, not metadata.
fn fixture(fps:u32,style:&str)->Session{
 let root=std::env::var_os("HALO_RECORDER_TEST_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
 let dir=root.join(format!("click-parity-{fps}-{}",chrono::Local::now().timestamp_nanos_opt().unwrap()));fs::create_dir_all(&dir).unwrap();
 let mut config=Config::default();config.fps=fps;config.click_style=style.into();config.microphone=false;config.encoder="cpu".into();config.cursor="hidden".into();config.frame="none".into();
 let raw=dir.join("source.mp4");let mut cmd=ffmpeg();cmd.args(["-f","lavfi","-i",&format!("color=c=0x102030:s=384x256:r={fps}"),"-t","1.2"]);intermediate_video_settings(&mut cmd,false);cmd.arg(&raw);run(cmd).unwrap();
 let commands=dir.join("clicks.txt");let mut text=String::new();
 for tick in 0..fps*2{let t=tick as f64/fps as f64;for slot in 0..CLICK_SLOTS{let age=t-(0.2+slot as f64*0.12);let alpha=if (0.0..0.5).contains(&age){1}else{0};let cell=((age/0.5*60.).floor()as i32).clamp(0,60);text+=&format!("{t:.6} overlay@click{slot} x {};\n{t:.6} overlay@click{slot} y 96;\n{t:.6} colorchannelmixer@clickalpha{slot} aa {alpha};\n{t:.6} crop@clickframe{slot} x {};\n",32+slot*72,cell*64);}}
 fs::write(&commands,text).unwrap();
 Session{config,target:Target{mode:"area".into(),monitor:0,window:0,rect:None},bounds:Rect{x:0,y:0,w:384,h:256},output:dir.join("delivery.mp4"),dir:dir.clone(),original_output:None,segments:vec![Segment{raw,audio:vec![],commands,duration:1.2}],running:None,gpu:false,mic_peak:Arc::new(AtomicU32::new(0)),sys_peak:Arc::new(AtomicU32::new(0)),mic_muted:Arc::new(AtomicBool::new(false)),sys_muted:Arc::new(AtomicBool::new(false)),annotations:vec![]}
}

fn check_click_parity(fps:u32,style:&str){
 let job=fixture(fps,style);finish(&job).unwrap();
 let sprite=image::open(job.dir.join("click.png")).unwrap().to_rgba8();
 let mut decoder=ffmpeg();decoder.arg("-i").arg(job.dir.join("finished-0.mp4")).args(["-pix_fmt","rgb24","-f","rawvideo","pipe:1"]);
 let decoded=decoder.output().unwrap();assert!(decoded.status.success());let frame_bytes=384*256*3;assert_eq!(decoded.stdout.len()/frame_bytes,(fps as f64*1.2).round()as usize);
 let mut errors=Vec::new();let mut visible=[0usize;CLICK_SLOTS];
 for (tick,frame) in decoded.stdout.chunks_exact(frame_bytes).enumerate(){
  let t=tick as f64/fps as f64;
  for slot in 0..CLICK_SLOTS{
   let age=t-(0.2+slot as f64*0.12);let active=(0.0..0.5).contains(&age);let cell=((age/0.5*60.).floor()as i32).clamp(0,60)as u32;let mut error=0u64;let mut change=0u64;
   for y in 0..64{for x in 0..64{let pixel=sprite.get_pixel(cell*64+x,y);let alpha=if active{pixel[3]as f64/255.}else{0.};let offset=((96+y as usize)*384+32+slot*72+x as usize)*3;for channel in 0..3{let base=decoded.stdout[offset+channel]as f64;let expected=pixel[channel]as f64*alpha+base*(1.-alpha);let actual=frame[offset+channel]as f64;error+=(actual-expected).abs().round()as u64;change+=(actual-base).abs().round()as u64;}}}
   let mean=error as f64/(64.*64.*3.);errors.push(mean);if change>500{visible[slot]+=1;}
  }
 }
 let max_error=errors.iter().copied().fold(0.,f64::max);let mean_error=errors.iter().sum::<f64>()/errors.len()as f64;
 println!("{fps} fps {style} click parity: max MAE={max_error:.3}, mean MAE={mean_error:.3}, visible frames={visible:?}, output={}",job.dir.display());
 assert!(max_error<3.,"Recorded sprites must match the preview at the same time: {max_error}");
 for frames in visible{assert!(frames>=fps as usize/3,"Click animation must continue across subsequent clicks");}
}

#[test]#[ignore="requires local FFmpeg"]fn click_preview_matches_export_at_60_fps(){for style in ["weak","moderate","strong"]{check_click_parity(60,style);}}
#[test]#[ignore="requires local FFmpeg"]fn click_preview_matches_export_at_30_fps(){for style in ["weak","moderate","strong"]{check_click_parity(30,style);}}

#[test]#[ignore="requires local FFmpeg"]fn arrow_edits_replay_visually_in_export(){
 use crate::recorder_annotations::{Annotation,CurvePoint,record_frame,asset_on_canvas};
 let mut job=fixture(30,"moderate");
 let mut a=Annotation{id:1,kind:"arrow".into(),x:30.,y:80.,w:40.,h:0.,text:String::new(),color:"#FF375F".into(),stroke:3.,font_size:24.,start:0.,end:None,keyframes:vec![],points:vec![]};
 let mut expected=vec![(0.,a.clone())];record_frame(&mut a,0.);
 a.w=260.;record_frame(&mut a,0.2);expected.push((0.2,a.clone()));
 a.points=vec![CurvePoint{x:160.,y:180.}];record_frame(&mut a,0.4);expected.push((0.4,a.clone()));
 a.points[0].y=220.;record_frame(&mut a,0.6);expected.push((0.6,a.clone()));
 a.color="#00FF00".into();a.stroke=6.;record_frame(&mut a,0.8);expected.push((0.8,a.clone()));
 a.points.clear();record_frame(&mut a,0.95);expected.push((0.95,a.clone()));
 a.end=Some(1.05);job.annotations.push(a);finish(&job).unwrap();
 let mut decoder=ffmpeg();decoder.arg("-i").arg(&job.output).args(["-pix_fmt","rgb24","-f","rawvideo","pipe:1"]);let decoded=decoder.output().unwrap();assert!(decoded.status.success());let bytes=384*256*3;
 let mut checked=0;
 for tick in [3usize,9,15,21,27,30,34]{
  let t=tick as f64/30.;let actual=&decoded.stdout[tick*bytes..(tick+1)*bytes];
  if t>=1.05{assert!(actual.iter().all(|v|*v<75),"cleared arrow remains in export at {t}");continue;}
  let shape=&expected.iter().rev().find(|(time,_)|*time<=t).unwrap().1;
  let reference=job.dir.join(format!("expected-{tick}.png"));render_png(&asset_on_canvas(shape,(0,0,384,256)).0,&reference).unwrap();let reference=image::open(reference).unwrap().to_rgba8();
  let mut error=0f64;let mut count=0;
  for (i,p)in reference.pixels().enumerate(){if p[3]>180{let alpha=p[3]as f64/255.;for channel in 0..3{let base=[16.,32.,48.][channel];let expected=p[channel]as f64*alpha+base*(1.-alpha);error+=(actual[i*3+channel]as f64-expected).abs();count+=1;}}}
  assert!(count>100);let mean=error/count as f64;assert!(mean<8.,"arrow appearance was skipped or applied at the wrong time: t={t}, pixel MAE={mean}");checked+=1;
 }
 println!("Verified {checked} distinct drawing stages and the clear event in {}",job.output.display());
}
