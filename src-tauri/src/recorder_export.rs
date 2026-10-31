//! Original-first delivery and supervised FFmpeg exports.
//! Final names become visible only after FFmpeg successfully closes the MP4.
use super::*;
use std::sync::atomic::{AtomicU64,AtomicUsize};

const STALL_LIMIT:Duration=Duration::from_secs(120);
const STARTUP_LIMIT:Duration=Duration::from_secs(30);
const AUDIO_STOP_LIMIT:Duration=Duration::from_secs(5);

pub(super) struct JobControl{pub cancel:Arc<AtomicBool>,progress:AtomicU32,stage:AtomicU32,children:Mutex<std::collections::HashMap<u32,std::os::windows::io::OwnedHandle>>}
#[derive(Clone,Default)]pub(super) struct Control(Option<Arc<JobControl>>);
impl Control{
 pub(super) fn background(cancel:Arc<AtomicBool>)->Self{Self(Some(Arc::new(JobControl{cancel,progress:AtomicU32::new(0),stage:AtomicU32::new(1),children:Mutex::new(std::collections::HashMap::new())})))}
 pub(super) fn cancelled(&self)->bool{self.0.as_ref().map(|c|c.cancel.load(Ordering::Relaxed)).unwrap_or_else(||CANCEL.load(Ordering::Relaxed))}
 fn progress(&self)->&AtomicU32{self.0.as_ref().map(|c|&c.progress).unwrap_or(&SAVE_PROGRESS)}
 fn stage(&self)->&AtomicU32{self.0.as_ref().map(|c|&c.stage).unwrap_or(&SAVE_STAGE)}
 pub(super) fn set_stage(&self,stage:u32){self.stage().store(stage,Ordering::Relaxed);if stage!=2{self.set_progress(0.);}}
 pub(super) fn set_progress(&self,value:f64){self.progress().store((value.clamp(0.,100.)*100.)as u32,Ordering::Relaxed);}
 pub(super) fn encoding(&self,value:f64){self.set_progress(value);let _=self.stage().compare_exchange(2,3,Ordering::Relaxed,Ordering::Relaxed);}
 pub(super) fn snapshot(&self)->(u32,f64){(self.stage().load(Ordering::Relaxed),self.progress().load(Ordering::Relaxed)as f64/100.)}
 fn priority(&self,progress:bool)->u32{
  use windows::Win32::System::Threading::{NORMAL_PRIORITY_CLASS,BELOW_NORMAL_PRIORITY_CLASS};
  if !progress{return 0;}
  let capture=self.0.is_some()&&STATE.try_lock().map(|s|s.as_ref().map(|s|matches!(s.phase.as_str(),"preparing"|"recording"|"resuming"|"pausing"|"saving")).unwrap_or(false)).unwrap_or(true);
  if capture{BELOW_NORMAL_PRIORITY_CLASS.0}else{NORMAL_PRIORITY_CLASS.0}
 }
 fn track(&self,child:&Child,add:bool){if let Some(c)=&self.0{use std::os::windows::io::AsHandle;let mut children=c.children.lock().unwrap();if add{if let Ok(handle)=child.as_handle().try_clone_to_owned(){children.insert(child.id(),handle);}}else{children.remove(&child.id());}}}
 pub(super) fn terminate_children(&self){if let Some(c)=&self.0{use std::os::windows::io::AsRawHandle;for handle in c.children.lock().unwrap().values(){unsafe{let _=windows::Win32::System::Threading::TerminateProcess(windows::Win32::Foundation::HANDLE(handle.as_raw_handle()),1);}}}}
}

// The capture log is intentionally higher frequency than video (and remains
// intact in the source project). FFmpeg's sendcmd parser becomes very expensive
// with one interval per mouse update. Compile only the state visible at each
// output frame, sharing one interval across its commands.
#[cfg(test)]
fn frame_commands(text:&str,fps:u32)->Res<String>{
 frame_commands_range(text,fps,0.,f64::MAX,&Control::background(Arc::new(AtomicBool::new(false))))
}
pub(super) fn frame_commands_range(text:&str,fps:u32,start:f64,duration:f64,control:&Control)->Res<String>{
 use std::collections::BTreeMap;
 let mut frames:BTreeMap<u64,BTreeMap<(&str,&str),&str>>=BTreeMap::new();
 for line in text.lines().filter(|line|!line.trim().is_empty()){
  if control.cancelled(){return Err("Recording cancelled.".into());}
  let mut fields=line.trim().trim_end_matches(';').splitn(4,' ');
  let time=fields.next().and_then(|s|s.parse::<f64>().ok()).filter(|t|t.is_finite()&&*t>=0.).ok_or("Invalid cursor command time.")?;
  let target=fields.next().ok_or("Invalid cursor target.")?;let command=fields.next().ok_or("Invalid cursor command.")?;let value=fields.next().ok_or("Invalid cursor value.")?;
  if time>=start+duration{continue;}
  let tick=((time-start).max(0.)*fps as f64-0.0001).ceil().max(0.)as u64;
  frames.entry(tick).or_default().insert((target,command),value);
 }
 let mut result=String::new();let mut previous=BTreeMap::new();
 for (tick,commands) in frames{
  let mut changes=Vec::new();
  for ((target,command),value) in commands{if previous.insert((target,command),value)!=Some(value){changes.push(format!("{target} {command} {value}"));}}
  if !changes.is_empty(){result+=&format!("{:.9} {};\n",(tick as f64/fps as f64-0.0000001).max(0.),changes.join(","));}
 }
 Ok(result)
}

// Count the exact assets/graphs that the preparation loop will produce. Frame
// quantization can merge several drawing edits into one rendered image.
fn sampled_frames(annotation:&crate::recorder_annotations::Annotation,offset:f64,duration:f64,fps:u32)->usize{
 let from=(annotation.start-offset).max(0.);let to=(annotation.end.unwrap_or(f64::MAX)-offset).min(duration);
 if from>=duration||to<=from{return 0;}
 let mut count=1;let mut previous=0;
 let first=annotation.keyframes.partition_point(|frame|frame.time<=offset);
 for frame in &annotation.keyframes[first..]{
  let time=frame.time-offset;
  if time>=duration||annotation.end.map(|end|frame.time>=end).unwrap_or(false){break;}
  let tick=(time*fps as f64).round()as u64;if tick!=previous{count+=1;previous=tick;}
 }
 count
}
fn preparation_units(job:&Session,parts:&[render::Part<'_>])->usize{
 let c=&job.config;let mut units=2+usize::from(c.cursor_effect=="spotlight")+usize::from(c.cursor_effect=="trail");
 for part in parts{let segment=part.segment;let offset=part.offset;
  units+=1+usize::from(segment.commands.exists()); // Graph and cursor command files.
  if c.frame!="none"{units+=1+usize::from(background_preset(c).is_some());}
  for annotation in &job.annotations{let frames=sampled_frames(annotation,offset,part.duration,c.fps);if frames>0{units+=frames+1;}}
 }
 units
}
pub(super) struct PreparationProgress{completed:AtomicUsize,total:usize,control:Control}
impl PreparationProgress{
 pub(super) fn new(job:&Session,parts:&[render::Part<'_>],control:Control,extra:usize)->Self{control.set_stage(1);Self{completed:AtomicUsize::new(0),total:preparation_units(job,parts)+extra,control}}
 pub(super) fn advance(&self){let completed=self.completed.fetch_add(1,Ordering::Relaxed)+1;self.control.progress().fetch_max((10000.*completed as f64/self.total.max(1)as f64).min(10000.)as u32,Ordering::Relaxed);}
 pub(super) fn complete(&self){debug_assert_eq!(self.completed.load(Ordering::Relaxed),self.total,"all visual preparation must report progress");self.control.set_progress(100.);}
 pub(super) fn cancelled(&self)->bool{self.control.cancelled()}
}

pub(super) fn fade_enabled(enabled:bool,duration:f64)->bool{enabled&&duration>=10.}

pub(super) fn needs_render(job:&Session)->bool {needs_video_render(job)||job.config.interaction_sounds||job.config.background_music}
pub(super) fn needs_video_render(job:&Session)->bool {
 let c=&job.config;
 c.show_keystrokes||c.smart_zoom||fade_enabled(c.auto_fade,job.segments.iter().map(|s|s.duration).sum())||c.frame!="none"||c.resolution!="original"||project::cursor_effects(c)||!job.annotations.is_empty()
  ||job.segments.iter().any(|s|s.commands.exists())
}

pub(super) fn styled_size(c:&Config,w:u32,h:u32)->(u32,u32){
 let (w,h)=((w+1)/2*2,(h+1)/2*2);
 if c.resolution=="original"{return (w,h);}
 let (max_w,max_h)=if c.resolution=="720p"{(1280.,720.)}else{(1920.,1080.)};
 let scale=(max_w/w as f64).min(max_h/h as f64).min(1.);
 (((w as f64*scale).floor()as u32/2*2).max(2),((h as f64*scale).floor()as u32/2*2).max(2))
}

pub(super) fn join_audio(track:AudioTrack)->Res<(PathBuf,bool)>{
 let started=Instant::now();
 while !track.join.is_finished(){
  if started.elapsed()>AUDIO_STOP_LIMIT{return Err("Audio shutdown timed out; retained its capture and continued saving video.".into());}
  thread::sleep(Duration::from_millis(20));
 }
 track.join.join().map_err(|_|"An audio capture thread stopped unexpectedly.")??;
 Ok((track.path,track.mic))
}

// A fixed wall deadline bounds initialization as well as finalization. The stall
// clock advances only when media time advances, not merely when FFmpeg prints.
struct Watchdog {wall:Duration,stall:Duration}
impl Watchdog {
 fn for_export(duration:Option<f64>,timeout:Option<Duration>)->Self{
  Self{wall:timeout.unwrap_or_else(||Duration::from_secs_f64(duration.unwrap_or(0.).mul_add(20.,120.).max(600.))),stall:STALL_LIMIT}
 }
 fn expired(&self,elapsed:Duration,last_progress:Duration)->Option<&'static str>{
  if elapsed>=self.wall{Some("Recording export exceeded its time limit.")}
  else if elapsed.saturating_sub(last_progress)>=self.stall{Some("Recording export stopped making progress.")}
  else{None}
 }
}

pub(super) fn run_command(command:std::process::Command,timeout:Option<Duration>,progress:Option<(f64,f64,f64)>)->Res<()> {
 run_supervised(command,progress,Watchdog::for_export(progress.map(|p|p.0),timeout))
}

fn run_supervised(command:std::process::Command,progress:Option<(f64,f64,f64)>,watchdog:Watchdog)->Res<()> {
 run_supervised_inner(command,progress,watchdog,None,None,Control::default())
}
pub(super) fn run_probe(command:std::process::Command,control:Control)->Res<()>{
 run_supervised_inner(command,None,Watchdog{wall:Duration::from_secs(5),stall:Duration::from_secs(5)},None,None,control)
}
pub(super) fn run_render_command(mut command:std::process::Command,duration:f64,update:Arc<dyn Fn(f64)+Send+Sync>,abort:Arc<AtomicBool>,control:Control)->Res<()>{
 command.args(["-progress","pipe:1","-stats_period","0.1"]);
 run_supervised_inner(command,Some((duration,0.,100.)),Watchdog::for_export(Some(duration),None),Some(update),Some(abort),control)
}
fn run_supervised_inner(mut command:std::process::Command,progress:Option<(f64,f64,f64)>,watchdog:Watchdog,update:Option<Arc<dyn Fn(f64)+Send+Sync>>,abort:Option<Arc<AtomicBool>>,control:Control)->Res<()> {
 let started=Instant::now();
 if control.cancelled(){return Err("Recording cancelled.".into());}
 let mut priority=control.priority(progress.is_some());
 let mut child=command.stdin(Stdio::null()).stdout(if progress.is_some(){Stdio::piped()}else{Stdio::null()}).stderr(Stdio::piped()).creation_flags(HIDDEN|priority).spawn().map_err(|e|e.to_string())?;
 control.track(&child,true);
 let last_progress=Arc::new(AtomicU64::new(0));let heartbeat=last_progress.clone();
 let encoded=Arc::new(AtomicBool::new(false));let media_started=encoded.clone();
 let reader_control=control.clone();let report=update.clone();let progress_reader=progress.map(|(duration,from,to)|{
  let stdout=child.stdout.take().unwrap();
  thread::spawn(move||{
   let mut latest=-1.;
   for line in BufReader::new(stdout).lines().map_while(Result::ok){
    if let Some(time)=line.strip_prefix("out_time_us=").and_then(|v|v.parse::<f64>().ok()){
     if time.is_finite()&&time>latest{latest=time;heartbeat.store(started.elapsed().as_millis()as u64,Ordering::Relaxed);if time>0.{media_started.store(true,Ordering::Relaxed);}let fraction=(time/1_000_000./duration).clamp(0.,1.);if let Some(report)=&report{if time>0.{report(fraction);}}else{if time>0.{let _=reader_control.stage().compare_exchange(2,3,Ordering::Relaxed,Ordering::Relaxed);}if time>0.||reader_control.stage().load(Ordering::Relaxed)!=2{reader_control.set_progress(from+(to-from)*fraction);}}}
    }
   }
  })
 });
 let stderr=child.stderr.take().unwrap();
 let reader=thread::spawn(move||{
  // Bound diagnostics even if a failing encoder emits errors continuously.
  let mut tail=std::collections::VecDeque::with_capacity(16);
  for line in BufReader::new(stderr).lines().map_while(Result::ok){if tail.len()==16{tail.pop_front();}tail.push_back(line.chars().take(2000).collect::<String>());}
  tail.into_iter().collect::<Vec<_>>().join("\n")
 });
 let result=loop{
  let current_priority=control.priority(progress.is_some());if current_priority!=priority{use std::os::windows::io::AsRawHandle;unsafe{let _=windows::Win32::System::Threading::SetPriorityClass(windows::Win32::Foundation::HANDLE(child.as_raw_handle()),windows::Win32::System::Threading::PROCESS_CREATION_FLAGS(current_priority));}priority=current_priority;}
  match child.try_wait(){Ok(Some(status))=>break Ok(status),Err(error)=>break Err(error.to_string()),Ok(None)=>{}}
  let error=if control.cancelled(){Some("Recording cancelled.")}else if abort.as_ref().map(|a|a.load(Ordering::Relaxed)).unwrap_or(false){Some("Another recording section failed.")}else if progress.is_some()&&!encoded.load(Ordering::Relaxed)&&started.elapsed()>=STARTUP_LIMIT{Some("The video encoder did not start within 30 seconds. Your original video and source project are available; try exporting again.")}else{watchdog.expired(started.elapsed(),Duration::from_millis(last_progress.load(Ordering::Relaxed)))};
  if let Some(error)=error{break Err(error.into());}
  thread::sleep(Duration::from_millis(30));
 };
 if result.is_err(){let _=child.kill();let _=child.wait();}
 control.track(&child,false);
 let error=reader.join().unwrap_or_default();if let Some(reader)=progress_reader{let _=reader.join();}
 let status=result?;
 if !status.success(){return Err(if error.is_empty(){format!("Recording encoder exited with {status}")}else{error});}
 if let Some(report)=update{report(1.);}else if let Some((_,_,to))=progress{control.set_progress(to);}
 Ok(())
}

#[cfg(test)]
fn publish_mp4(temporary:&Path,destination:&Path,limit:Duration)->Res<()> {publish_mp4_controlled(temporary,destination,limit,&Control::default())}
fn publish_mp4_controlled(temporary:&Path,destination:&Path,limit:Duration,control:&Control)->Res<()> {
 let start=Instant::now();
 loop{
  if control.cancelled(){return Err("Recording cancelled.".into());}
  match fs::rename(temporary,destination){
   Ok(())=>return Ok(()),
   // Explorer/virus scanners can briefly hold a newly closed MP4. Retry only
   // Windows sharing/lock violations, with a bounded, cancellable deadline.
   Err(error)if matches!(error.raw_os_error(),Some(32|33))&&start.elapsed()<limit=>thread::sleep(Duration::from_millis(40)),
   Err(error)=>return Err(format!("Could not publish recording from {} to {}: {error}",temporary.display(),destination.display())),
  }
 }
}

fn atomic_mp4(destination:&Path,write:impl FnOnce(&Path)->Res<()>)->Res<()> {atomic_mp4_controlled(destination,&Control::default(),write)}
fn atomic_mp4_controlled(destination:&Path,control:&Control,write:impl FnOnce(&Path)->Res<()>)->Res<()> {
 let temporary=destination.with_extension(format!("partial-{}.mp4",chrono::Local::now().timestamp_nanos_opt().unwrap_or_default()));
 let result=(||{
  write(&temporary)?;
  if control.cancelled(){return Err("Recording cancelled.".into());}
  if fs::metadata(&temporary).map_err(|e|e.to_string())?.len()==0{return Err("The encoder produced an empty recording.".into());}
  // Same-directory rename avoids a full video copy and preserves the original
  // destination if encoding, cancellation, or finalization fails.
  if control.stage().load(Ordering::Relaxed)==4{control.stage().store(5,Ordering::Relaxed);}
  publish_mp4_controlled(&temporary,destination,Duration::from_secs(3),control)
 })();
 if result.is_err(){let _=fs::remove_file(&temporary);}
 result
}

fn write_segments(paths:&[PathBuf],list:&Path,trim:Option<&[f64]>)->Res<()> {
 let mut text=String::new();
 for (i,path) in paths.iter().enumerate(){
  text+=&format!("file '{}'\n",path.to_string_lossy().replace('\\',"/").replace('\'',"'\\''"));
  if let Some(duration)=trim.and_then(|t|t.get(i)){text+=&format!("outpoint {duration:.9}\nduration {duration:.9}\n");}
 }
 fs::write(list,text).map_err(|e|e.to_string())
}

pub(super) fn concat_delivery(paths:&[PathBuf],destination:&Path,list:&Path,duration:f64,from:f64,to:f64,audio:Option<&Path>,trim:Option<&[f64]>)->Res<()> {concat_delivery_controlled(paths,destination,list,duration,from,to,audio,trim,false,&Control::default())}
pub(super) fn concat_delivery_controlled(paths:&[PathBuf],destination:&Path,list:&Path,duration:f64,from:f64,to:f64,audio:Option<&Path>,trim:Option<&[f64]>,fade:bool,control:&Control)->Res<()> {
 if paths.is_empty(){return Err("No recording segments were captured.".into());}
 write_segments(paths,list,trim)?;
 atomic_mp4_controlled(destination,control,|temporary|{
  let mut command=ffmpeg();command.args(["-f","concat","-safe","0","-i"]).arg(list);
  if let Some(audio)=audio{command.arg("-i").arg(audio);}
  command.args(["-map","0:v:0","-map",if audio.is_some(){"1:a?"}else{"0:a?"},"-c:v","copy"]);
  if fade_enabled(fade,duration){let d=1f64;command.args(["-af",&format!("afade=t=in:st=0:d={d},afade=t=out:st={}:d={d}",(duration-d).max(d)),"-c:a","aac","-b:a","192k"]);}else{command.args(["-c:a","copy"]);}
  command.args(["-t",&duration.to_string(),"-movflags","+faststart"]).arg(temporary);
  {command.args(["-progress","pipe:1","-stats_period","0.1"]);run_supervised_inner(command,Some((duration,from,to)),Watchdog::for_export(Some(duration),None),None,None,control.clone())}
 })
}

// One final audio encode mixes optional effects while copying rendered video.
pub(super) fn concat_with_soundtrack(paths:&[PathBuf],destination:&Path,list:&Path,duration:f64,source:Option<&Path>,has_audio:bool,fx:&Path,fade:bool,durations:Option<&[f64]>,control:&Control)->Res<()>{
 write_segments(paths,list,durations)?;atomic_mp4_controlled(destination,control,|temporary|{let mut cmd=ffmpeg();cmd.args(["-f","concat","-safe","0","-i"]).arg(list);let with_voice=has_audio&&source.is_some();if with_voice{cmd.arg("-i").arg(source.unwrap());}cmd.arg("-i").arg(fx);let mut filter=if with_voice{"[1:a]aresample=48000,asplit=2[voice][speech];[2:a]aresample=48000,lowpass=f=6500[fx];[fx][speech]sidechaincompress=threshold=0.025:ratio=5:attack=25:release=320:knee=4:makeup=1[ducked];[voice][ducked]amix=inputs=2:normalize=0,alimiter=limit=0.95:level=false:latency=true".to_string()}else{"[1:a]aresample=48000,lowpass=f=6500".to_string()};if fade_enabled(fade,duration){filter+=&format!(",afade=t=in:st=0:d=1,afade=t=out:st={}:d=1",(duration-1.).max(1.));}filter+="[audio]";cmd.args(["-filter_complex",&filter,"-map","0:v:0","-map","[audio]","-c:v","copy","-c:a","aac","-b:a","192k","-t",&duration.to_string(),"-movflags","+faststart"]).arg(temporary);cmd.args(["-progress","pipe:1","-stats_period","0.1"]);run_supervised_inner(cmd,Some((duration,0.,100.)),Watchdog::for_export(Some(duration),None),None,None,control.clone())})
}

pub(super) fn save_original(job:&Session,destination:&Path)->Res<()> {
 let duration=job.segments.iter().map(|s|s.duration).sum::<f64>().max(0.1);
 let paths=job.segments.iter().map(|s|s.raw.clone()).collect::<Vec<_>>();
 let has_audio=job.segments.iter().any(|s|!s.audio.is_empty());
 let list=job.dir.join("original-segments.txt");
 let trim=job.segments.iter().map(|s|s.duration).collect::<Vec<_>>();
 if !has_audio{return concat_delivery(&paths,destination,&list,duration,0.,100.,None,Some(&trim));}
 if paths.is_empty(){return Err("No recording segments were captured.".into());}
 write_segments(&paths,&list,Some(&trim))?;
 let mut cmd=ffmpeg();cmd.args(["-f","concat","-safe","0","-i"]).arg(&list);
 let mut filters=Vec::new();let mut inputs=1;let mut segments=Vec::new();
 for (i,segment) in job.segments.iter().enumerate(){
  if CANCEL.load(Ordering::Relaxed){return Err("Recording cancelled.".into());}
  let mut labels=Vec::new();
  for (j,(path,mic)) in segment.audio.iter().enumerate(){
   cmd.arg("-i").arg(path);
   let gain=if *mic{job.config.mic_gain}else{job.config.system_gain};
   let enhance=if *mic&&job.config.voice_enhance{"highpass=f=75,afftdn=nf=-25:tn=1,acompressor=threshold=0.125:ratio=2:attack=5:release=120:makeup=1.5,"}else{""};
   filters.push(format!("[{inputs}:a]aresample=48000,aformat=sample_fmts=fltp:sample_rates=48000:channel_layouts=stereo,{enhance}volume={gain},apad,atrim=duration={},asetpts=PTS-STARTPTS[a{i}_{j}]",segment.duration));inputs+=1;labels.push(format!("[a{i}_{j}]"));
  }
  if labels.is_empty(){cmd.args(["-f","lavfi","-i","anullsrc=r=48000:cl=stereo"]);filters.push(format!("[{inputs}:a]atrim=duration={},asetpts=PTS-STARTPTS[silent{i}]",segment.duration));inputs+=1;labels.push(format!("[silent{i}]"));}
  filters.push(format!("{}amix=inputs={}:normalize=0,alimiter=limit=0.95[segment{i}]",labels.join(""),labels.len()));segments.push(format!("[segment{i}]"));
 }
 // One continuous audio encode prevents AAC priming gaps at pause/resume joins.
 filters.push(format!("{}concat=n={}:v=0:a=1[audio]",segments.join(""),segments.len()));
 cmd.args(["-filter_complex_threads","1","-filter_complex",&filters.join(";"),"-map","0:v:0","-c:v","copy","-map","[audio]","-c:a","aac","-b:a","192k","-ar","48000","-t",&duration.to_string(),"-movflags","+faststart"]);
 atomic_mp4(destination,|temporary|{cmd.arg(temporary);run_progress(cmd,duration,0.,100.)})
}

#[cfg(test)]mod tests{
 use super::*;
 #[test]fn background_controls_do_not_share_capture_or_other_job_state(){
  let first=Arc::new(AtomicBool::new(false));let second=Arc::new(AtomicBool::new(false));let a=Control::background(first.clone());let b=Control::background(second);
  let capture=Control::background(Arc::new(AtomicBool::new(true)));assert!(capture.cancelled());assert!(!a.cancelled()&&!b.cancelled());a.set_stage(2);a.encoding(42.);assert_eq!(a.snapshot(),(3,42.));assert_eq!(b.snapshot(),(1,0.));first.store(true,Ordering::Relaxed);assert!(a.cancelled()&&!b.cancelled());assert!(capture.cancelled());
 }
 #[test]fn parallel_assets_reuse_exact_pixels_and_propagate_failures(){
  let dir=std::env::temp_dir().join(format!("halo-assets-{}",chrono::Local::now().timestamp_nanos_opt().unwrap()));fs::create_dir_all(&dir).unwrap();
  let svg=r##"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64"><circle cx="32" cy="32" r="24" fill="#ff1234" fill-opacity=".37"/></svg>"##;
  let progress=PreparationProgress{completed:AtomicUsize::new(0),total:3,control:Control::background(Arc::new(AtomicBool::new(false)))};let mut assets=render::Assets::new();
  for i in 0..3{assets.push(svg.into(),dir.join(format!("copy-{i}.png")));}assets.prepare(&progress,3).unwrap();progress.complete();
  let expected=fs::read(dir.join("copy-0.png")).unwrap();for i in 1..3{assert_eq!(fs::read(dir.join(format!("copy-{i}.png"))).unwrap(),expected);}
  let progress=PreparationProgress{completed:AtomicUsize::new(0),total:2,control:Control::background(Arc::new(AtomicBool::new(false)))};let mut assets=render::Assets::new();assets.push(svg.into(),dir.join("valid.png"));assets.push(svg.into(),dir.join("missing-parent/invalid.png"));assert!(assets.prepare(&progress,2).is_err());
  let progress=PreparationProgress{completed:AtomicUsize::new(0),total:1,control:Control::background(Arc::new(AtomicBool::new(true)))};let mut assets=render::Assets::new();assets.push(svg.into(),dir.join("cancelled.png"));let cancelled=assets.prepare(&progress,2);assert!(cancelled.unwrap_err().contains("cancelled"));assert!(!dir.join("cancelled.png").exists());
  fs::remove_dir_all(dir).unwrap();
 }
 #[test]fn preparation_counts_frame_collisions_and_segment_overlap(){
  let mut annotation=crate::recorder_annotations::Annotation{id:1,kind:"arrow".into(),x:10.,y:20.,w:100.,h:40.,text:String::new(),color:"#ffffff".into(),stroke:3.,font_size:24.,start:0.,end:Some(1.),keyframes:vec![],points:vec![]};
  for time in [0.,0.001,0.01,0.0334,0.05,0.9,1.01]{annotation.keyframes.push(annotation.frame(time));}
  assert_eq!(sampled_frames(&annotation,0.,2.,30),4);
  assert_eq!(sampled_frames(&annotation,0.5,0.5,30),2);
  assert_eq!(sampled_frames(&annotation,1.,0.5,30),0);
 }
 #[test]fn cursor_compilation_preserves_every_video_frame_and_bounds_intervals(){
  use std::collections::BTreeMap;
  for fps in [24,30,60]{
   let mut text=String::new();let mut events=Vec::new();
   for sample in 0..2000{let time=sample as f64/1000.;let value=sample%37;for command in ["x","y"]{text+=&format!("{time:.3} overlay@pointer {command} {value};\n");events.push((time,command,value.to_string()));}}
   text+="2.001 colorchannelmixer@clickalpha0 aa 0;\n";
   let compiled=frame_commands(&text,fps).unwrap();assert!(compiled.lines().count()<=2*fps as usize+2);
   let mut actual=BTreeMap::new();let mut lines=compiled.lines().peekable();
   for tick in 0..2*fps{let time=tick as f64/fps as f64;
    while lines.peek().map(|line|line.split_once(' ').unwrap().0.parse::<f64>().unwrap()<=time).unwrap_or(false){let (_,commands)=lines.next().unwrap().split_once(' ').unwrap();for entry in commands.trim_end_matches(';').split(','){let mut parts=entry.splitn(3,' ');let target=parts.next().unwrap();let command=parts.next().unwrap();let value=parts.next().unwrap();actual.insert((target,command),value.to_string());}}
    for command in ["x","y"]{let expected=events.iter().rev().find(|(t,c,_)|*t<=time+0.000001&&*c==command).unwrap();assert_eq!(actual.get(&("overlay@pointer",command)),Some(&expected.2),"fps={fps}, frame={tick}");}
   }
   assert!(compiled.contains("colorchannelmixer@clickalpha0 aa 0"));
  }
  assert!(frame_commands("NaN overlay@pointer x 1;",30).is_err());
 }
 #[test]fn cursor_sections_restore_state_and_shift_later_events(){
  let text="0 overlay@pointer x 10;\n0 overlay@pointer y 20;\n0.1 overlay@pointer x 30;\n0.4 overlay@pointer y 40;\n0.6 overlay@pointer x 50;\n0.8 overlay@pointer x 60;\n";
  let compiled=frame_commands_range(text,30,0.4,0.4,&Control::background(Arc::new(AtomicBool::new(false)))).unwrap();let first=compiled.lines().next().unwrap();assert!(first.contains("overlay@pointer x 30"));assert!(first.contains("overlay@pointer y 40"));
  assert!(compiled.contains("0.199999900 overlay@pointer x 50"));assert!(!compiled.contains("x 60"));
 }
 #[test]fn watchdog_bounds_initialization_stalls_and_total_time(){
  let w=Watchdog{wall:Duration::from_secs(600),stall:Duration::from_secs(120)};
  assert!(w.expired(Duration::from_secs(119),Duration::ZERO).is_none());
  assert!(w.expired(Duration::from_secs(120),Duration::ZERO).unwrap().contains("progress"));
  assert!(w.expired(Duration::from_secs(200),Duration::from_secs(190)).is_none());
  assert!(w.expired(Duration::from_secs(600),Duration::from_secs(599)).unwrap().contains("time limit"));
 }
 #[test]fn failed_delivery_never_replaces_an_existing_video(){
  let dir=std::env::temp_dir().join(format!("halo-delivery-{}",chrono::Local::now().timestamp_nanos_opt().unwrap()));fs::create_dir_all(&dir).unwrap();let path=dir.join("original.mp4");fs::write(&path,b"saved original").unwrap();
  assert!(atomic_mp4(&path,|temporary|{fs::write(temporary,b"incomplete").unwrap();Err("encoder failed".into())}).is_err());
  assert_eq!(fs::read(&path).unwrap(),b"saved original");assert_eq!(fs::read_dir(&dir).unwrap().count(),1);fs::remove_dir_all(dir).unwrap();
 }
 #[test]fn styled_dimensions_match_even_capture_and_resolution_limits(){
  let mut c=Config::default();assert_eq!(styled_size(&c,321,201),(322,202));c.resolution="720p".into();assert_eq!(styled_size(&c,1920,1080),(1280,720));assert_eq!(styled_size(&c,800,600),(800,600));
 }
 #[test]fn unresponsive_encoder_is_terminated(){
  let mut command=std::process::Command::new("powershell.exe");command.args(["-NoProfile","-Command","Start-Sleep -Seconds 30"]);
  let start=Instant::now();let error=run_supervised(command,None,Watchdog{wall:Duration::from_secs(2),stall:Duration::from_millis(300)}).unwrap_err();assert!(error.contains("progress"));assert!(start.elapsed()<Duration::from_secs(5));
 }
 #[test]fn delivery_recovers_from_a_temporary_windows_file_lock(){
  use std::os::windows::fs::OpenOptionsExt;
  let dir=std::env::temp_dir().join(format!("halo-lock-{}",chrono::Local::now().timestamp_nanos_opt().unwrap()));fs::create_dir_all(&dir).unwrap();let source=dir.join("partial.mp4");let target=dir.join("final.mp4");fs::write(&source,b"finished video").unwrap();
  let lock=fs::OpenOptions::new().read(true).share_mode(0).open(&source).unwrap();let release=thread::spawn(move||{thread::sleep(Duration::from_millis(150));drop(lock);});
  publish_mp4(&source,&target,Duration::from_secs(2)).unwrap();release.join().unwrap();assert_eq!(fs::read(&target).unwrap(),b"finished video");fs::remove_dir_all(dir).unwrap();
 }
 #[test]fn a_persistent_file_lock_has_a_bounded_failure(){
  use std::os::windows::fs::OpenOptionsExt;
  let dir=std::env::temp_dir().join(format!("halo-lock-timeout-{}",chrono::Local::now().timestamp_nanos_opt().unwrap()));fs::create_dir_all(&dir).unwrap();let source=dir.join("partial.mp4");fs::write(&source,b"finished video").unwrap();let lock=fs::OpenOptions::new().read(true).share_mode(0).open(&source).unwrap();let start=Instant::now();
  assert!(publish_mp4(&source,&dir.join("final.mp4"),Duration::from_millis(200)).is_err());assert!(start.elapsed()<Duration::from_secs(1));drop(lock);assert_eq!(fs::read(&source).unwrap(),b"finished video");fs::remove_dir_all(dir).unwrap();
 }
}

#[cfg(test)]mod fade_policy_tests{
 use super::*;
 #[test]fn fades_require_ten_seconds_and_an_enabled_setting(){for duration in [0.,1.,3.,9.99]{assert!(!fade_enabled(true,duration));}for duration in [10.,10.01,60.]{assert!(fade_enabled(true,duration));assert!(!fade_enabled(false,duration));}}
}
