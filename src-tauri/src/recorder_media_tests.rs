use super::*;

// Explicitly run these tests with --ignored on a workstation with FFmpeg.
// They exercise the real export graph and inspect decoded pixels, not metadata.
pub(super) fn fixture(fps:u32,style:&str)->Session{
 let root=std::env::var_os("HALO_RECORDER_TEST_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
 let dir=root.join(format!("click-parity-{fps}-{}",chrono::Local::now().timestamp_nanos_opt().unwrap()));fs::create_dir_all(&dir).unwrap();
 let mut config=Config::default();config.fps=fps;config.click_style=style.into();config.microphone=false;config.encoder="cpu".into();config.cursor="hidden".into();config.frame="none".into();
 let raw=dir.join("source.mp4");let mut cmd=ffmpeg();cmd.args(["-f","lavfi","-i",&format!("color=c=0x102030:s=384x256:r={fps}"),"-t","1.2"]);intermediate_video_settings(&mut cmd,false);cmd.arg(&raw);run(cmd).unwrap();
 let commands=dir.join("clicks.txt");let mut text=String::new();
 for tick in 0..fps*2{let t=tick as f64/fps as f64;for slot in 0..CLICK_SLOTS{let age=t-(0.2+slot as f64*0.12);let alpha=if (0.0..0.5).contains(&age){1}else{0};let cell=((age/0.5*60.).floor()as i32).clamp(0,60);text+=&format!("{t:.6} overlay@click{slot} x {};\n{t:.6} overlay@click{slot} y 96;\n{t:.6} colorchannelmixer@clickalpha{slot} aa {alpha};\n{t:.6} crop@clickframe{slot} x {};\n",32+slot*72,cell*64);}}
 fs::write(&commands,text).unwrap();
 Session{config,target:Target{mode:"area".into(),monitor:0,window:0,rect:None},bounds:Rect{x:0,y:0,w:384,h:256},output:dir.join("delivery.mp4"),dir:dir.clone(),project:None,original_output:None,segments:vec![Segment{raw,audio:vec![],commands,duration:1.2}],running:None,gpu:false,mic_peak:Arc::new(AtomicU32::new(0)),sys_peak:Arc::new(AtomicU32::new(0)),mic_muted:Arc::new(AtomicBool::new(false)),sys_muted:Arc::new(AtomicBool::new(false)),annotations:vec![]}
}

fn check_click_parity(fps:u32,style:&str){
 let job=fixture(fps,style);finish_partitioned(&job,3,(fps as f64*0.4).round()as u64).unwrap();
 let sprite=image::open(job.dir.join("click.png")).unwrap().to_rgba8();
 // Inspect the joined delivery so assertions cover every section boundary.
 let decoded=decoded_video(&job.output);let frame_bytes=384*256*3;assert_eq!(decoded.len()/frame_bytes,(fps as f64*1.2).round()as usize);
 let mut errors=Vec::new();let mut visible=[0usize;CLICK_SLOTS];
 for (tick,frame) in decoded.chunks_exact(frame_bytes).enumerate(){
  let t=tick as f64/fps as f64;
  for slot in 0..CLICK_SLOTS{
   let age=t-(0.2+slot as f64*0.12);let active=(0.0..0.5).contains(&age);let cell=((age/0.5*60.).floor()as i32).clamp(0,60)as u32;let mut error=0u64;let mut change=0u64;
   for y in 0..64{for x in 0..64{let pixel=sprite.get_pixel(cell*64+x,y);let alpha=if active{pixel[3]as f64/255.}else{0.};let offset=((96+y as usize)*384+32+slot*72+x as usize)*3;for channel in 0..3{let base=decoded[offset+channel]as f64;let expected=pixel[channel]as f64*alpha+base*(1.-alpha);let actual=frame[offset+channel]as f64;error+=(actual-expected).abs().round()as u64;change+=(actual-base).abs().round()as u64;}}}
   let mean=error as f64/(64.*64.*3.);errors.push(mean);if change>500{visible[slot]+=1;}
  }
 }
 let max_error=errors.iter().copied().fold(0.,f64::max);let mean_error=errors.iter().sum::<f64>()/errors.len()as f64;
 println!("{fps} fps {style} click parity: max MAE={max_error:.3}, mean MAE={mean_error:.3}, visible frames={visible:?}, output={}",job.dir.display());
 // This now inspects the delivery encode (H.264 4:2:0), not a lossless RGB intermediate.
 // Allow chroma subsampling error while keeping timing/shape regressions detectable.
 assert!(max_error<4.,"Recorded sprites must match the preview at the same time: {max_error}");
 for frames in visible{assert!(frames>=fps as usize/3,"Click animation must continue across subsequent clicks");}
}

#[test]#[ignore="requires local FFmpeg"]fn click_preview_matches_export_at_60_fps(){for style in ["weak","moderate","strong"]{check_click_parity(60,style);}}
#[test]#[ignore="requires local FFmpeg"]fn click_preview_matches_export_at_30_fps(){for style in ["weak","moderate","strong"]{check_click_parity(30,style);}}

fn decoded_video(path:&Path)->Vec<u8>{
 let mut command=ffmpeg();command.arg("-i").arg(path).args(["-map","0:v:0","-pix_fmt","rgb24","-f","rawvideo","pipe:1"]);
 let output=command.output().unwrap();assert!(output.status.success(),"{}",String::from_utf8_lossy(&output.stderr));output.stdout
}

#[test]#[ignore="requires local FFmpeg"]fn dense_cursor_capture_starts_without_a_long_parser_stall(){
 let mut job=fixture(30,"moderate");let mut text=String::new();
 // A long high-rate capture previously made sendcmd spend tens of seconds
 // parsing intervals before output, even when only a short range was rendered.
 for sample in 0..34_320{let time=sample as f64/240.;text+=&format!("{time:.3} overlay@pointer x {};\n{time:.3} overlay@pointer y 80;\n",sample%300);}
 fs::write(&job.segments[0].commands,text).unwrap();
 job.config.cursor="large".into();let started=Instant::now();finish(&job).unwrap();
 assert!(started.elapsed()<Duration::from_secs(15),"dense cursor startup regressed: {:?}",started.elapsed());
 let commands=fs::read_to_string(job.dir.join("cursor-main-0.txt")).unwrap();assert!(commands.lines().count()<=4300);
 assert_eq!(decoded_video(&job.output).len()/(384*256*3),36);
 println!("Dense 143-second cursor log rendered in {:?}",started.elapsed());
}

#[test]#[ignore="requires local FFmpeg"]fn blur_is_visible_only_during_its_active_interval(){
 let mut job=fixture(30,"moderate");fs::remove_file(&job.segments[0].commands).unwrap();
 let source=job.dir.join("pattern.mp4");let mut command=ffmpeg();command.args(["-f","lavfi","-i","testsrc2=s=384x256:r=30:d=1.2"]);intermediate_video_settings(&mut command,false);command.arg(&source);run(command).unwrap();job.segments[0].raw=source.clone();
 job.annotations.push(crate::recorder_annotations::Annotation{id:1,kind:"blur".into(),x:80.,y:60.,w:180.,h:120.,text:String::new(),color:"#ffffff".into(),stroke:3.,font_size:24.,start:0.2,end:Some(0.8),keyframes:vec![],points:vec![]});
 finish(&job).unwrap();let original=decoded_video(&source);let rendered=decoded_video(&job.output);assert_eq!(original.len(),rendered.len());let frame_bytes=384*256*3;
 let difference=|tick:usize,rect:(usize,usize,usize,usize)|{let (x,y,w,h)=rect;let mut sum=0u64;for row in y..y+h{for col in x..x+w{let index=tick*frame_bytes+(row*384+col)*3;for c in 0..3{sum+=original[index+c].abs_diff(rendered[index+c])as u64;}}}sum as f64/(w*h*3)as f64};
 for tick in [3,27]{assert!(difference(tick,(0,0,384,256))<6.,"inactive blur modified frame {tick}");}
 assert!(difference(15,(85,65,170,110))>8.,"active blur was not applied");
 assert!(difference(15,(280,30,80,180))<6.,"blur modified pixels outside its mask");
}

#[test]#[ignore="requires local FFmpeg"]fn parallel_sections_preserve_pauses_and_continuous_audio(){
 let mut job=fixture(30,"moderate");let tone=job.dir.join("tone.wav");let mut command=ffmpeg();command.args(["-f","lavfi","-i","sine=frequency=440:sample_rate=48000","-t","1.2"]).arg(&tone);run(command).unwrap();job.segments[0].audio.push((tone,true));
 let second=job.dir.join("segment-1.mp4");fs::copy(&job.segments[0].raw,&second).unwrap();job.segments.push(Segment{raw:second,audio:vec![],commands:job.dir.join("no-effects"),duration:1.2});
 let original=job.dir.join("original.mp4");export::save_original(&job,&original).unwrap();job.original_output=Some(original.clone());finish_partitioned(&job,3,12).unwrap();
 assert_eq!(decoded_video(&job.output).len()/(384*256*3),72);
 let audio=|path:&Path|{let mut c=ffmpeg();c.arg("-i").arg(path).args(["-map","0:a:0","-f","f32le","pipe:1"]);let o=c.output().unwrap();assert!(o.status.success());o.stdout};
 assert_eq!(audio(&original),audio(&job.output),"section joins must not re-encode or shift audio");
}

#[test]#[ignore="requires local FFmpeg"]fn parallel_failure_stops_sibling_encoders_promptly(){
 let mut live=ffmpeg();live.args(["-re","-f","lavfi","-i","color=s=64x64:r=30","-t","60","-f","null","-"]);
 let mut invalid=ffmpeg();invalid.args(["-i","halo-missing-source-for-test.mp4","-f","null","-"]);
 let started=Instant::now();let error=render::parallel(vec![(live,60.,0.),(invalid,60.,60.)],2,120.,export::Control::default()).unwrap_err();
 assert!(error.contains("No such file")||error.contains("Error opening input"),"{error}");assert!(started.elapsed()<Duration::from_secs(5));
}

#[test]#[ignore="requires local FFmpeg"]fn original_is_packet_copied_without_rendering(){
 let mut job=fixture(30,"moderate");fs::remove_file(&job.segments[0].commands).unwrap();
 // Use the exact live-capture codec rather than the lossless effects fixture.
 let live=job.dir.join("live.mp4");let mut command=ffmpeg();command.arg("-i").arg(&job.segments[0].raw);capture_video_settings(&mut command,&job.config,false);command.arg(&live);run(command).unwrap();job.segments[0].raw=live;
 assert!(!export::needs_render(&job));let start=Instant::now();export::save_original(&job,&job.output).unwrap();
 let elapsed=start.elapsed();assert!(!job.dir.join("finished-0.mp4").exists());
 assert_eq!(decoded_video(&job.segments[0].raw),decoded_video(&job.output));
 println!("Original remux completed in {elapsed:?}, no render intermediates: {}",job.output.display());
}

#[test]#[ignore="requires local FFmpeg"]fn original_survives_mac_windows_and_failed_styled_exports(){
 for frame in ["mac","windows"]{
  let mut job=fixture(30,"moderate");job.config.frame=frame.into();let original=job.dir.join("original.mp4");job.original_output=Some(original.clone());
  export::save_original(&job,&original).unwrap();let saved=fs::read(&original).unwrap();assert!(!job.output.exists());
  finish(&job).unwrap();assert_eq!(fs::read(&original).unwrap(),saved);
  let size=export::styled_size(&job.config,job.bounds.w,job.bounds.h);let pad=frame_pad(&job.config,size.0);let info=crate::process::video_info(&job.output).unwrap();assert_eq!((info.w,info.h),(size.0+2*pad,size.1+2*pad+40));assert!(!decoded_video(&job.output).is_empty());
  fs::remove_file(&job.segments[0].raw).unwrap();assert!(finish(&job).is_err());assert_eq!(fs::read(&original).unwrap(),saved);assert!(!decoded_video(&original).is_empty());
 }
}

#[test]#[ignore="requires local FFmpeg"]fn paused_original_preserves_audio_and_silent_segment_timing(){
 let mut job=fixture(30,"moderate");let audio=job.dir.join("tone.wav");let mut command=ffmpeg();command.args(["-f","lavfi","-i","sine=frequency=440:sample_rate=48000","-t","1.2"]).arg(&audio);run(command).unwrap();
 job.segments[0].audio.push((audio,true));let second=job.dir.join("segment-1.mp4");fs::copy(&job.segments[0].raw,&second).unwrap();job.segments.push(Segment{raw:second,audio:vec![],commands:job.dir.join("no-effects"),duration:1.2});
 export::save_original(&job,&job.output).unwrap();let video=decoded_video(&job.output);assert_eq!(video.len()/(384*256*3),72);
 let mut command=ffmpeg();command.arg("-i").arg(&job.output).args(["-map","0:a:0","-ac","1","-ar","48000","-f","f32le","pipe:1"]);let result=command.output().unwrap();assert!(result.status.success());let samples=result.stdout.chunks_exact(4).map(|b|f32::from_le_bytes(b.try_into().unwrap())).collect::<Vec<_>>();
 assert!((samples.len()as f64/48000.-2.4).abs()<0.1);let rms=|slice:&[f32]|(slice.iter().map(|s|(*s as f64).powi(2)).sum::<f64>()/slice.len()as f64).sqrt();assert!(rms(&samples[4800..48000])>0.01);assert!(rms(&samples[67200..105600])<0.001);
 println!("Pause/resume original: 72 video frames, {:.3}s audio, tone followed by silence",samples.len()as f64/48000.);
}

#[test]#[ignore="requires local FFmpeg"]fn pause_joins_discard_capture_shutdown_tail_frames(){
 let mut job=fixture(30,"moderate");job.segments[0].duration=1.;
 let second=job.dir.join("segment-1.mp4");fs::copy(&job.segments[0].raw,&second).unwrap();
 job.segments.push(Segment{raw:second,audio:vec![],commands:job.dir.join("no-effects"),duration:1.});
 export::save_original(&job,&job.output).unwrap();assert_eq!(decoded_video(&job.output).len()/(384*256*3),60);
}

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
 a.end=Some(1.05);job.annotations.push(a);finish_partitioned(&job,3,12).unwrap();
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
