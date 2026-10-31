use super::*;

// Explicitly run these tests with --ignored on a workstation with FFmpeg.
// They exercise the real export graph and inspect decoded pixels, not metadata.
pub(super) fn fixture(fps:u32,style:&str)->Session{
 let root=std::env::var_os("HALO_RECORDER_TEST_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
 let dir=root.join(format!("click-parity-{fps}-{}",chrono::Local::now().timestamp_nanos_opt().unwrap()));fs::create_dir_all(&dir).unwrap();
 let mut config=Config::default();config.auto_fade=false;config.fps=fps;config.click_style=style.into();config.microphone=false;config.encoder="cpu".into();config.cursor="hidden".into();config.frame="none".into();
 let raw=dir.join("source.mp4");let mut cmd=ffmpeg();cmd.args(["-f","lavfi","-i",&format!("color=c=0x102030:s=384x256:r={fps}"),"-t","1.2"]);intermediate_video_settings(&mut cmd,false);cmd.arg(&raw);run(cmd).unwrap();
 let commands=dir.join("clicks.txt");let mut text=String::new();
 for tick in 0..fps*2{let t=tick as f64/fps as f64;for slot in 0..CLICK_SLOTS{let age=t-(0.2+slot as f64*0.12);let alpha=if (0.0..0.5).contains(&age){1}else{0};let cell=((age/0.5*60.).floor()as i32).clamp(0,60);text+=&format!("{t:.6} overlay@click{slot} x {};\n{t:.6} overlay@click{slot} y 96;\n{t:.6} colorchannelmixer@clickalpha{slot} aa {alpha};\n{t:.6} crop@clickframe{slot} x {};\n",32+slot*72,cell*64);}}
 fs::write(&commands,text).unwrap();
 Session{config,target:Target{mode:"area".into(),monitor:0,window:0,rect:None},bounds:Rect{x:0,y:0,w:384,h:256},output:dir.join("delivery.mp4"),dir:dir.clone(),project:None,original_output:None,segments:vec![Segment{raw,audio:vec![],commands,duration:1.2}],running:None,encoder:Encoder::Cpu,mic_peak:Arc::new(AtomicU32::new(0)),sys_peak:Arc::new(AtomicU32::new(0)),mic_muted:Arc::new(AtomicBool::new(false)),sys_muted:Arc::new(AtomicBool::new(false)),annotations:vec![]}
}

fn check_click_parity(fps:u32,style:&str){
 let mut job=fixture(fps,style);job.config.cursor_color="#ffffff".into();finish_partitioned(&job,3,(fps as f64*0.4).round()as u64).unwrap();
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

// Saturated amber incurs RGB chroma-subsampling error even with lossless H.264.
// Check geometry using luma, and the opaque core's RGB independently.
#[test]#[ignore="requires local FFmpeg"]fn amber_click_keeps_colour_and_source_position(){
 for fps in [30,60]{let job=fixture(fps,"strong");finish_partitioned(&job,3,(fps as f64*0.4).round()as u64).unwrap();let sprite=image::open(job.dir.join("click.png")).unwrap().to_rgba8();let decoded=decoded_video(&job.output);let bytes=384*256*3;let tick=(fps as f64*11./30.).round()as usize;let age=tick as f64/fps as f64-0.2;let cell=(age/0.5*60.).floor()as u32;let frame=&decoded[tick*bytes..(tick+1)*bytes];let mut error=0.;
  for y in 0..64{for x in 0..64{let p=sprite.get_pixel(cell*64+x,y);let alpha=p[3]as f64/255.;let index=((96+y as usize)*384+32+x as usize)*3;let mut expected=0.;let mut actual=0.;for(ch,weight)in [0.299,0.587,0.114].into_iter().enumerate(){expected+=(p[ch]as f64*alpha+decoded[index+ch]as f64*(1.-alpha))*weight;actual+=frame[index+ch]as f64*weight;}error+=(actual-expected).abs();}}
  assert!(error/(64.*64.)<4.,"Amber ring luma/position changed: {}",error/(64.*64.));let center=(128*384+64)*3;
  // Rendered sections no longer use B-frames, so splice-concatenating them never
  // duplicates a decoded frame; that costs a few compression levels here.
  for(ch,expected)in [255i32,176,32].into_iter().enumerate(){assert!((frame[center+ch]as i32-expected).abs()<16,"Amber core colour changed");}
 }
}

fn decoded_video(path:&Path)->Vec<u8>{
 let mut command=ffmpeg();command.arg("-i").arg(path).args(["-map","0:v:0","-pix_fmt","rgb24","-f","rawvideo","pipe:1"]);
 let output=command.output().unwrap();assert!(output.status.success(),"{}",String::from_utf8_lossy(&output.stderr));output.stdout
}

#[test]#[ignore="benchmark"]fn bench_cursor_planning_speed(){
 let mut job=fixture(30,"moderate");job.config.smart_zoom=true;job.config.smooth_cursor=true;job.config.cursor="normal".into();
 let duration=120.0;job.segments[0].duration=duration;
 let rate=300.;let n=(duration*rate)as usize;let mut text=String::new();
 for i in 0..n{let t=i as f64/rate;let x=(190.+100.*(t*0.7).sin())as i32;let y=(128.+80.*(t*0.5).cos())as i32;let buttons=if i%900==0{1}else{0};text+=&format!("{{\"time\":{t:.4},\"x\":{x},\"y\":{y},\"inside\":true,\"buttons\":{buttons}}}\n");}
 fs::write(job.dir.join("cursor-0.jsonl"),text).unwrap();
 let control=export::Control::default();
 let start=Instant::now();
 cinematic::prepare_cursor(&job,&control).unwrap();
 let _camera=cinematic::plan(&job,&control).unwrap();
 let _scroll=cinematic::ScrollTrack::plan(&job,&control).unwrap();
 println!("BENCH cursor planning ({n} samples, {duration}s): {:?}",start.elapsed());
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
 let mut job=fixture(30,"moderate");job.config.click_effect=false;fs::remove_file(&job.segments[0].commands).unwrap();
 // Use the exact live-capture codec rather than the lossless effects fixture.
 let live=job.dir.join("live.mp4");let mut command=ffmpeg();command.arg("-i").arg(&job.segments[0].raw);capture_video_settings(&mut command,&job.config,Encoder::Cpu);command.arg(&live);run(command).unwrap();job.segments[0].raw=live;
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
 let mut job=fixture(30,"moderate");job.config.click_effect=false;
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

#[test]#[ignore="requires local FFmpeg and HALO_TEST_ENCODER hardware"]fn hardware_capture_and_styled_export_roundtrip(){
 let selected=std::env::var("HALO_TEST_ENCODER").expect("Set HALO_TEST_ENCODER to nvenc, qsv or amf");let encoder=match selected.as_str(){"nvenc"=>Encoder::Nvenc,"qsv"=>Encoder::Qsv,"amf"=>Encoder::Amf,_=>panic!("Unknown test encoder")};
 for fps in [30,60]{
 let mut job=fixture(fps,"moderate");job.encoder=encoder;job.config.encoder="auto".into();job.config.frame="mac".into();
 let control=export::Control::background(Arc::new(AtomicBool::new(false)));
 assert!(encoder::probe(encoder,&job.config,384,256,true,&control),"{} capture probe failed",encoder.name());
 let live=job.dir.join("live.mp4");let mut cmd=ffmpeg();cmd.arg("-i").arg(&job.segments[0].raw);capture_video_settings(&mut cmd,&job.config,encoder);cmd.args(["-movflags","+frag_keyframe+empty_moov+default_base_moof"]).arg(&live);run_command(cmd,Some(Duration::from_secs(15)),None).unwrap();job.segments[0].raw=live;
 // Simulate a resumed segment using CPU after hardware becomes unavailable.
 // Packet-copy saving must preserve both segments across different H.264 encoders.
 let cpu=job.dir.join("resumed-cpu.mp4");let mut cmd=ffmpeg();cmd.arg("-i").arg(&job.segments[0].raw);capture_video_settings(&mut cmd,&job.config,Encoder::Cpu);cmd.arg(&cpu);run(cmd).unwrap();
 let commands=job.dir.join("resumed-cursor.txt");fs::copy(&job.segments[0].commands,&commands).unwrap();
 job.segments.push(Segment{raw:cpu,audio:vec![],commands,duration:1.2});
 let original=job.dir.join("original.mp4");export::save_original(&job,&original).unwrap();job.original_output=Some(original.clone());let saved=fs::read(&original).unwrap();
 assert_eq!(decoded_video(&original).len(),384*256*3*(fps as usize*24/10));
 let (w,h)=encoder::output_size(&job.config,384,256);assert!(encoder::probe(encoder,&job.config,w,h,false,&control));
 let started=Instant::now();finish_partitioned_controlled(&job,2,fps as u64*4/10,control,encoder).unwrap();
 assert_eq!(decoded_video(&job.output).len(),w as usize*h as usize*3*(fps as usize*24/10));assert_eq!(fs::read(&original).unwrap(),saved);
 println!("{fps}fps {} live codec + original + six styled sections: {:?}, {}",encoder.name(),started.elapsed(),job.output.display());
}
}

#[test]#[ignore="requires local FFmpeg"]fn new_drawing_fades_and_mockup_pixels(){
 for frame in ["mac","windows"]{
 let mut job=fixture(30,"moderate");job.config.frame=frame.into();job.config.background="preset:ocean".into();job.config.auto_fade=true;fs::remove_file(&job.segments[0].commands).unwrap();
 let tone=job.dir.join("tone.wav");let mut command=ffmpeg();command.args(["-f","lavfi","-i","sine=frequency=440:sample_rate=48000","-t","1.2"]).arg(&tone);run(command).unwrap();job.segments[0].audio.push((tone,true));
 for (id,kind)in [(1,"pencil"),(2,"ellipse")]{job.annotations.push(crate::recorder_annotations::Annotation{id,kind:kind.into(),x:80.,y:60.,w:180.,h:100.,text:String::new(),color:"#ff375f".into(),stroke:6.,font_size:24.,start:0.,end:None,keyframes:vec![],points:if kind=="pencil"{vec![crate::recorder_annotations::CurvePoint{x:110.,y:160.},crate::recorder_annotations::CurvePoint{x:180.,y:80.}]}else{vec![]}});}
 let saved_frame=job.config.frame.clone();job.config.frame="none".into();let saved_annotations=std::mem::take(&mut job.annotations);job.config.click_effect=false;assert!(!export::needs_render(&job),"short clips must not render just for fades");job.config.click_effect=true;job.annotations=saved_annotations;job.config.frame=saved_frame;
 let original=job.dir.join("original.mp4");export::save_original(&job,&original).unwrap();job.original_output=Some(original);
 finish_partitioned(&job,3,3).unwrap();let (w,h)=encoder::output_size(&job.config,384,256);let bytes=w as usize*h as usize*3;let decoded=decoded_video(&job.output);assert_eq!(decoded.len()/bytes,36);
 for tick in [0,35]{let error=decoded[tick*bytes..(tick+1)*bytes].iter().zip(&decoded[18*bytes..19*bytes]).map(|(a,b)|a.abs_diff(*b)as f64).sum::<f64>()/bytes as f64;assert!(error<2.,"short clips must have no fade at frame {tick}");}
 let middle=&decoded[18*bytes..19*bytes];let bg=image::open(job.dir.join("background-0.png")).unwrap().to_rgb8();
 for (x,y)in [(0,0),(10,10),(w-1,h-1)]{let i=((y*w+x)*3)as usize;for c in 0..3{assert!(middle[i+c].abs_diff(bg.get_pixel(x,y)[c])<9,"background color mismatch {frame} at {x},{y}");}}
 let pad=frame_pad(&job.config,384);for x in [pad,pad+383]{let y=pad+40+255;let i=((y*w+x)*3)as usize;for c in 0..3{assert!(middle[i+c].abs_diff(bg.get_pixel(x,y)[c])<12,"bottom corner must show background");}}
 let pink=middle.chunks_exact(3).filter(|p|p[0]>150&&p[1]<115&&p[2]>45).count();assert!(pink>400,"new drawing tools must be visible");
 let mut c=ffmpeg();c.arg("-i").arg(&job.output).args(["-map","0:a:0","-f","f32le","pipe:1"]);let audio=c.output().unwrap();assert!(audio.status.success());let samples:Vec<f32>=audio.stdout.chunks_exact(4).map(|b|f32::from_le_bytes(b.try_into().unwrap())).collect();
 let rms=|start:usize,end:usize|(samples[start..end].iter().map(|x|(*x as f64).powi(2)).sum::<f64>()/(end-start)as f64).sqrt();assert!(rms(4800,5760)>rms(48000,48960)*0.8,"short audio must not fade in");assert!(rms(100800,101760)>rms(48000,48960)*0.8,"short audio must not fade out");
 println!("Validated {frame}: {}",job.output.display());
 }
}

#[test]#[ignore="requires local FFmpeg"]fn window_resize_commands_centre_content_across_render_sections(){
 let mut job=fixture(30,"moderate");job.config.cursor="hidden".into();let raw=job.dir.join("resize-source.mp4");
 let mut c=ffmpeg();c.args(["-f","lavfi","-i","color=c=white:s=384x256:r=30:d=1.2,drawbox=x=192:y=0:w=192:h=256:color=black:t=fill:enable='gte(t,0.4)*lt(t,0.8)',drawbox=x=0:y=128:w=384:h=128:color=black:t=fill:enable='gte(t,0.8)'"]);intermediate_video_settings(&mut c,false);c.arg(&raw);run(c).unwrap();job.segments[0].raw=raw;
 fs::write(&job.segments[0].commands,"0 overlay@window x 0;\n0 overlay@window y 0;\n0.4 overlay@window x 96;\n0.4 overlay@window y 0;\n0.8 overlay@window x 0;\n0.8 overlay@window y 64;\n").unwrap();finish_partitioned(&job,3,9).unwrap();
 let decoded=decoded_video(&job.output);let pixels=384*256*3;
 for (tick,left,top,right,bottom)in [(5,0,0,384,256),(17,96,0,288,256),(29,0,64,384,192)]{let frame=&decoded[tick*pixels..(tick+1)*pixels];for y in 0..256{for x in 0..384{let value=frame[(y*384+x)*3];if x>left+3&&x+3<right&&y>top+3&&y+3<bottom{assert!(value>235,"content missing at {tick}: {x},{y}");}else if x+3<left||x>right+3||y+3<top||y>bottom+3{assert!(value<20,"bars not centred at {tick}: {x},{y}");}}}}
 println!("Centred resize export: {}",job.output.display());
}

#[test]#[ignore="requires local FFmpeg"]fn one_second_video_and_audio_fades(){
 let mut job=fixture(30,"moderate");job.config.auto_fade=true;fs::remove_file(&job.segments[0].commands).unwrap();let source=job.dir.join("ten-seconds.mp4");let mut c=ffmpeg();c.args(["-f","lavfi","-i","color=c=white:s=384x256:r=30:d=10"]);intermediate_video_settings(&mut c,false);c.arg(&source);run(c).unwrap();job.segments[0].raw=source;job.segments[0].duration=10.;
 let tone=job.dir.join("tone.wav");let mut c=ffmpeg();c.args(["-f","lavfi","-i","sine=frequency=440:sample_rate=48000:duration=10"]).arg(&tone);run(c).unwrap();job.segments[0].audio.push((tone,true));let original=job.dir.join("original.mp4");export::save_original(&job,&original).unwrap();job.original_output=Some(original);finish_partitioned(&job,3,9).unwrap();
 let decoded=decoded_video(&job.output);let bytes=384*256*3;assert_eq!(decoded.len()/bytes,300);let pixel=|tick:usize|decoded[tick*bytes+100]as i32;
 assert!(pixel(0)<5&&pixel(299)<5);assert!((pixel(15)-128).abs()<12,"fade in must last one second");assert!(pixel(30)>245&&pixel(45)>245);assert!((pixel(284)-128).abs()<12,"fade out must last one second");
 let mut c=ffmpeg();c.arg("-i").arg(&job.output).args(["-map","0:a:0","-f","f32le","pipe:1"]);let output=c.output().unwrap();assert!(output.status.success());let samples:Vec<f32>=output.stdout.chunks_exact(4).map(|b|f32::from_le_bytes(b.try_into().unwrap())).collect();let rms=|time:f64|{let from=(time*96000.)as usize;let values=&samples[from..from+960];(values.iter().map(|v|(*v as f64).powi(2)).sum::<f64>()/values.len()as f64).sqrt()};let peak=rms(1.4);assert!((rms(0.5)/peak-0.5).abs()<0.12);assert!((rms(9.5)/peak-0.5).abs()<0.12);
 println!("One-second fades verified: {}",job.output.display());
}

#[test]#[ignore="requires local FFmpeg"]fn cinematic_zoom_and_smooth_cursor_preserve_frames_in_all_styles(){
 for fps in [30,60]{for frame in ["none","mac","windows"]{
  let mut job=fixture(fps,"moderate");job.config.smooth_cursor=true;job.config.smart_zoom=true;job.config.cursor="normal".into();job.config.click_effect=true;job.config.frame=frame.into();let source=job.dir.join("cinematic-source.mp4");let mut c=ffmpeg();c.args(["-f","lavfi","-i",&format!("testsrc2=s=384x256:r={fps}:d=8")]);intermediate_video_settings(&mut c,false);c.arg(&source);run(c).unwrap();job.segments[0].raw=source;job.segments[0].duration=8.;
  let events=job.dir.join("cursor-0.jsonl");let mut text=String::new();for tick in 0..8*fps{let t=tick as f64/fps as f64;let (x,y)=if t<2.{(60,70)}else if t<3.5{(340,210)}else{(120,120)};let sample=project::CursorSample{context:None,wheel:0,shape:0,zoom:0,time:t,x,y,inside:true,buttons:if [fps,2*fps,4*fps].contains(&tick){1}else{0},window_offset:None};text+=&format!("{}\n",serde_json::to_string(&sample).unwrap());}fs::write(&events,text).unwrap();project::restore_cursor(&events,&job.segments[0].commands,&job.config,384,256,8.).unwrap();
  let control=export::Control::background(Arc::new(AtomicBool::new(false)));let track=cinematic::plan(&job,&control).unwrap().unwrap();assert!(track.frames[fps as usize*2..fps as usize*3].iter().any(|s|s.zoom>1.3));assert_eq!(track.frames.last().unwrap().zoom,1.);
  if fps==30&&frame=="windows"{let tone=job.dir.join("camera-tone.wav");let mut c=ffmpeg();c.args(["-f","lavfi","-i","sine=frequency=440:sample_rate=48000:duration=8"]).arg(&tone);run(c).unwrap();job.segments[0].audio.push((tone,true));let original=job.dir.join("original.mp4");export::save_original(&job,&original).unwrap();job.original_output=Some(original);}
  finish_partitioned(&job,3,fps as u64*2).unwrap();if let Some(original)=&job.original_output{let audio=|path:&Path|{let mut c=ffmpeg();c.arg("-i").arg(path).args(["-map","0:a:0","-f","f32le","pipe:1"]);let out=c.output().unwrap();assert!(out.status.success());out.stdout};assert_eq!(audio(original),audio(&job.output),"camera motion must not shift audio");}let (w,h)=encoder::output_size(&job.config,384,256);let decoded=decoded_video(&job.output);assert_eq!(decoded.len()/(w as usize*h as usize*3),8*fps as usize);
  println!("Cinematic {fps}fps {frame}: {}",job.output.display());
 }}
}

#[test]#[ignore="requires local FFmpeg"]fn optional_soundtrack_preserves_video_and_voice(){
 let mut job=fixture(30,"moderate");let control=export::Control::default();
 assert!(soundtrack::render(&job,None,&control).unwrap().is_none());
 let events=[project::CursorSample{context:None,wheel:0,shape:0,zoom:0,window_offset:None,time:0.2,x:100,y:100,inside:true,buttons:1},project::CursorSample{context:Some(context::Context{activity:context::TYPE,..Default::default()}),wheel:0,shape:0,zoom:0,window_offset:None,time:0.4,x:100,y:100,inside:true,buttons:0}];
 fs::write(job.dir.join("cursor-0.jsonl"),events.iter().map(|s|format!("{}\n",serde_json::to_string(s).unwrap())).collect::<String>()).unwrap();
 job.config.smart_zoom=false;
 for(sounds,music)in [(true,false),(false,true),(true,true)]{job.config.interaction_sounds=sounds;job.config.background_music=music;let started=Instant::now();let wav=soundtrack::render(&job,None,&control).unwrap().unwrap();let mut reader=hound::WavReader::open(&wav).unwrap();assert_eq!(reader.duration(),57600);assert!(reader.samples::<i16>().any(|s|s.unwrap().abs()>40));
 let output=job.dir.join(format!("soundtrack-{sounds}-{music}.mp4"));export::concat_with_soundtrack(&[job.segments[0].raw.clone()],&output,&job.dir.join("fx-list.txt"),1.2,None,false,&wav,false,None,&control).unwrap();assert_eq!(decoded_video(&output),decoded_video(&job.segments[0].raw),"audio must packet-copy the video");println!("Soundtrack sounds={sounds} music={music}: {:?}",started.elapsed());}
 let tone=job.dir.join("voice.wav");let mut cmd=ffmpeg();cmd.args(["-f","lavfi","-i","sine=frequency=440:sample_rate=48000","-t","1.2"]).arg(&tone);run(cmd).unwrap();job.segments[0].audio.push((tone,true));let original=job.dir.join("original-voice.mp4");export::save_original(&job,&original).unwrap();let before=fs::read(&original).unwrap();let fx=soundtrack::render(&job,None,&control).unwrap().unwrap();let output=job.dir.join("voice-and-soundtrack.mp4");export::concat_with_soundtrack(&[job.segments[0].raw.clone()],&output,&job.dir.join("voice-list.txt"),1.2,Some(&original),true,&fx,false,None,&control).unwrap();assert_eq!(before,fs::read(&original).unwrap());assert_eq!(decoded_video(&output),decoded_video(&job.segments[0].raw));let mut cmd=ffmpeg();cmd.arg("-i").arg(&output).args(["-map","0:a:0","-ar","48000","-ac","1","-f","f32le","pipe:1"]);let decoded=cmd.output().unwrap();assert!(decoded.status.success());let samples=decoded.stdout.chunks_exact(4).map(|b|f32::from_le_bytes(b.try_into().unwrap())).collect::<Vec<_>>();let rms=(samples.iter().map(|s|s*s).sum::<f32>()/samples.len()as f32).sqrt();assert!(rms>0.05&&rms<0.3,"voice level changed: {rms}");
}
#[test]#[ignore="requires local FFmpeg"]fn scroll_blur_is_gated_and_keeps_frame_count(){
 for fps in [30,60]{let mut job=fixture(fps,"moderate");job.config.smart_zoom=true;let events=[project::CursorSample{context:None,wheel:120,shape:0,zoom:0,window_offset:None,time:0.3,x:100,y:100,inside:true,buttons:0},project::CursorSample{context:None,wheel:120,shape:0,zoom:0,window_offset:None,time:0.6,x:100,y:100,inside:true,buttons:0}];fs::write(job.dir.join("cursor-0.jsonl"),events.iter().map(|s|format!("{}\n",serde_json::to_string(s).unwrap())).collect::<String>()).unwrap();let scroll=cinematic::ScrollTrack::plan(&job,&export::Control::default()).unwrap();assert!(scroll.filter(0.8,0.3).is_none());let filter=scroll.filter(0.,1.2).unwrap();let source=job.dir.join("pattern.mp4");let mut cmd=ffmpeg();cmd.args(["-f","lavfi","-i",&format!("testsrc2=s=384x256:r={fps}:d=1.2")]);intermediate_video_settings(&mut cmd,false);cmd.arg(&source);run(cmd).unwrap();let out=job.dir.join("scroll.mp4");let mut cmd=ffmpeg();cmd.arg("-i").arg(&source).args(["-vf",&filter]);intermediate_video_settings(&mut cmd,false);cmd.arg(&out);run(cmd).unwrap();let a=decoded_video(&source);let b=decoded_video(&out);assert_eq!(a.len(),b.len());let bytes=384*256*3;assert_eq!(&a[bytes..2*bytes],&b[bytes..2*bytes]);let tick=(fps as f64*0.4)as usize;assert_ne!(&a[tick*bytes..(tick+1)*bytes],&b[tick*bytes..(tick+1)*bytes]);}
}

#[test]#[ignore="requires local FFmpeg"]fn keycaps_and_kenney_cursors_export_across_styles_and_rates(){
 for fps in [30,60]{for style in ["none","mac","windows"]{let mut job=fixture(fps,"moderate");job.config.show_keystrokes=true;job.config.kenney_cursor=true;job.config.cursor="normal".into();job.config.frame=style.into();job.config.keystroke_theme=if fps==60{"light"}else{"dark"}.into();job.config.smart_zoom=style=="mac";let mut text=String::new();for tick in 0..fps*12/10{let e=project::CursorSample{context:None,wheel:0,shape:((tick as f64/fps as f64*12.).floor()as u8).min(12),zoom:0,window_offset:None,time:tick as f64/fps as f64,x:180,y:80,inside:true,buttons:0};text+=&format!("{}\n",serde_json::to_string(&e).unwrap());}fs::write(job.dir.join("cursor-0.jsonl"),text).unwrap();project::restore_cursor(&job.dir.join("cursor-0.jsonl"),&job.segments[0].commands,&job.config,384,256,1.2).unwrap();let events=[keystrokes::Event{time:0.2,keys:vec!["Win".into(),"Shift".into(),"s".into()]},keystrokes::Event{time:0.8,keys:vec!["Left".into()]}];fs::write(job.dir.join("keys-0.jsonl"),events.iter().map(|e|format!("{}\n",serde_json::to_string(e).unwrap())).collect::<String>()).unwrap();finish_partitioned(&job,2,(fps as f64*0.4)as u64).unwrap();let (w,h)=encoder::output_size(&job.config,384,256);let decoded=decoded_video(&job.output);let bytes=w as usize*h as usize*3;assert_eq!(decoded.len()/bytes,(fps as f64*1.2).round()as usize);let sheet=image::open(job.dir.join("keycaps-0.png")).unwrap().to_rgba8();assert!(sheet.pixels().any(|p|p[3]>200));let check=(fps as f64*0.5)as usize;let before=(fps as f64*0.1)as usize;let area=|tick:usize|{let mut sum=0u64;for y in h as usize-18-sheet.height()as usize/2..h as usize-18{for x in 20..(18+sheet.width()as usize).min(w as usize-18){let index=tick*bytes+(y*w as usize+x)*3;sum+=decoded[index]as u64;}}sum};assert!(area(check)>area(before)+3000,"key overlay not visible: {fps} {style}");let white=|tick:usize|{let mut count=0;for y in h as usize-18-sheet.height()as usize/2..h as usize-18{for x in 20..(18+sheet.width()as usize).min(w as usize-18){let index=tick*bytes+(y*w as usize+x)*3;if decoded[index]>190&&decoded[index+1]>190&&decoded[index+2]>190{count+=1;}}}count};assert!(white(check)>white(before)+10,"keycap glyphs missing: {fps} {style}");let root=PathBuf::from("../output/input-prompts");fs::create_dir_all(&root).unwrap();fs::copy(&job.output,root.join(format!("keycaps-{style}-{fps}.mp4"))).unwrap();println!("{fps}fps {style} keycaps/cursors: {}",job.output.display());}}
}

#[test]#[ignore="requires local FFmpeg"]fn normal_audio_only_effects_copy_video_without_rendering(){
 let mut job=fixture(30,"moderate");job.segments[0].commands=job.dir.join("no-visual-commands.txt");job.config.smart_zoom=false;job.config.cursor="hidden".into();job.config.frame="none".into();job.config.auto_fade=false;job.config.interaction_sounds=true;job.config.click_effect=false;
 let events=[project::CursorSample{context:None,wheel:0,shape:0,zoom:0,window_offset:None,time:0.2,x:100,y:100,inside:true,buttons:1},project::CursorSample{context:Some(context::Context{activity:context::TYPE,..Default::default()}),wheel:0,shape:0,zoom:0,window_offset:None,time:0.4,x:100,y:100,inside:true,buttons:0}];fs::write(job.dir.join("cursor-0.jsonl"),events.iter().map(|e|format!("{}\n",serde_json::to_string(e).unwrap())).collect::<String>()).unwrap();let original=job.dir.join("original-audio.mp4");export::save_original(&job,&original).unwrap();job.original_output=Some(original.clone());let original_bytes=fs::read(&original).unwrap();
 for(sounds,music)in [(true,false),(false,true),(true,true)]{job.config.interaction_sounds=sounds;job.config.background_music=music;job.output=job.dir.join(format!("normal-audio-{sounds}-{music}.mp4"));assert!(export::needs_render(&job));assert!(!export::needs_video_render(&job));finish(&job).unwrap();assert!(!job.dir.join("finished-0.mp4").exists(),"audio-only export must bypass visual rendering");assert_eq!(decoded_video(&original),decoded_video(&job.output));assert_eq!(fs::read(&original).unwrap(),original_bytes);let mut command=ffmpeg();command.arg("-i").arg(&job.output).args(["-map","0:a:0","-f","f32le","pipe:1"]);let audio=command.output().unwrap();assert!(audio.status.success());assert!(!audio.stdout.is_empty());}
}

#[test]#[ignore="requires local FFmpeg"]fn added_audio_ducks_during_voice_without_reducing_voice(){
 let mut job=fixture(30,"moderate");let voice=job.dir.join("gated-voice.wav");let mut cmd=ffmpeg();cmd.args(["-f","lavfi","-i","sine=frequency=440:sample_rate=48000:duration=1.2","-af","volume='if(between(t,0.3,0.9),1,0)':eval=frame"]).arg(&voice);run(cmd).unwrap();job.segments[0].audio.push((voice,true));let original=job.dir.join("voice-original.mp4");export::save_original(&job,&original).unwrap();let fx=job.dir.join("steady-fx.wav");let mut cmd=ffmpeg();cmd.args(["-f","lavfi","-i","sine=frequency=1250:sample_rate=48000:duration=1.2"]).arg(&fx);run(cmd).unwrap();let output=job.dir.join("ducked.mp4");export::concat_with_soundtrack(&[job.segments[0].raw.clone()],&output,&job.dir.join("duck-list.txt"),1.2,Some(&original),true,&fx,false,None,&export::Control::default()).unwrap();let decode=|p:&Path|{let mut cmd=ffmpeg();cmd.arg("-i").arg(p).args(["-map","0:a:0","-ac","1","-ar","48000","-f","f32le","pipe:1"]);let o=cmd.output().unwrap();assert!(o.status.success());o.stdout.chunks_exact(4).map(|b|f32::from_le_bytes(b.try_into().unwrap())as f64).collect::<Vec<_>>()};let mixed=decode(&output);let source=decode(&original);let amplitude=|s:&[f64],hz:f64,from:f64,to:f64|{let a=(from*48000.)as usize;let b=(to*48000.)as usize;let(mut re,mut im)=(0.,0.);for(i,v)in s[a..b].iter().enumerate(){let phase=2.*std::f64::consts::PI*hz*(a+i)as f64/48000.;re+=v*phase.cos();im+=v*phase.sin();}2.*re.hypot(im)/(b-a)as f64};let before=amplitude(&mixed,1250.,0.08,0.25);let during=amplitude(&mixed,1250.,0.6,0.8);let narration=amplitude(&mixed,440.,0.6,0.8);let original_voice=amplitude(&source,440.,0.6,0.8);println!("FX before={before:.5} during={during:.5}, voice original={original_voice:.5} mixed={narration:.5}");assert!(during<before*0.65,"interaction layer should duck under narration");assert!((narration/original_voice-1.).abs()<0.1,"narration itself must keep its level");assert_eq!(decoded_video(&output),decoded_video(&job.segments[0].raw));
}
