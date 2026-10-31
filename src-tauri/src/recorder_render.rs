//! Frame-aligned render sections and bounded parallel FFmpeg workers.
use super::*;
use std::collections::VecDeque;

fn available_memory()->Option<u64>{
 use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx,MEMORYSTATUSEX};
 let mut memory=MEMORYSTATUSEX{dwLength:std::mem::size_of::<MEMORYSTATUSEX>()as u32,..Default::default()};
 unsafe{GlobalMemoryStatusEx(&mut memory)}.ok().map(|_|memory.ullAvailPhys)
}
fn asset_bytes(svg:&str)->u64{
 let root=svg.split('>').next().unwrap_or(svg);
 let dimension=|name:&str|root.split_once(&format!(" {name}=\""))?.1.split('"').next()?.parse::<u64>().ok();
 dimension("width").and_then(|w|dimension("height").and_then(|h|w.checked_mul(h))).and_then(|pixels|pixels.checked_mul(16)).unwrap_or(512*1024*1024)
}
fn asset_workers(requested:usize,cpus:usize,memory:Option<u64>,bytes:u64)->usize{
 let fits=memory.map(|n|(n.saturating_sub(2*1024*1024*1024)/bytes.max(1)).max(1)as usize).unwrap_or(1);
 requested.max(1).min(cpus.max(1)).min(fits)
}

pub(super) struct Assets{pending:Vec<(String,PathBuf)>,copies:Vec<(PathBuf,PathBuf)>,known:std::collections::HashMap<String,PathBuf>}
impl Assets{
 pub(super) fn new()->Self{Self{pending:Vec::new(),copies:Vec::new(),known:std::collections::HashMap::new()}}
 pub(super) fn push(&mut self,svg:String,path:PathBuf){if let Some(source)=self.known.get(&svg){self.copies.push((source.clone(),path));}else{self.known.insert(svg.clone(),path.clone());self.pending.push((svg,path));}}
 pub(super) fn prepare(self,progress:&export::PreparationProgress,workers:usize)->Res<()>{
  let bytes=self.pending.iter().map(|(svg,_)|asset_bytes(svg)).max().unwrap_or(1);
  let workers=asset_workers(workers,thread::available_parallelism().map(|n|n.get()).unwrap_or(1),available_memory(),bytes);
  let count=self.pending.len();let queue=Mutex::new(VecDeque::from(self.pending));let error=Mutex::new(None);let abort=AtomicBool::new(false);
  thread::scope(|scope|{for _ in 0..workers.max(1).min(count){let queue=&queue;let error=&error;let abort=&abort;
   scope.spawn(move||loop{
    if progress.cancelled()||abort.load(Ordering::Relaxed){break;}
    let Some((svg,path))=queue.lock().unwrap().pop_front()else{break;};
    match render_png(&svg,&path){Ok(())=>progress.advance(),Err(e)=>{*error.lock().unwrap()=Some(e);abort.store(true,Ordering::Relaxed);break;}}
   });
  }});
  if progress.cancelled(){return Err("Recording cancelled.".into());}
  if let Some(error)=error.into_inner().unwrap(){return Err(error);}
  for (source,destination)in self.copies{if progress.cancelled(){return Err("Recording cancelled.".into());}fs::copy(source,destination).map_err(|e|e.to_string())?;progress.advance();}
  Ok(())
 }
}

pub(super) struct Part<'a>{pub segment:&'a Segment,pub start:f64,pub duration:f64,pub offset:f64,pub cost:f64}
fn section_cost(job:&Session,offset:f64,duration:f64)->f64{
 let mut cost=job.bounds.w as f64*job.bounds.h as f64*duration;
 for annotation in &job.annotations{
  let active=(annotation.end.unwrap_or(f64::MAX).min(offset+duration)-annotation.start.max(offset)).max(0.);if active==0.{continue;}
  let (_,_,w,h)=crate::recorder_annotations::canvas_bounds(annotation);let pixels=w as f64*h as f64;
  // Cropped blur/mask frames also traverse framesync outside their visible
  // interval; active blur itself costs several passes over the same pixels.
  cost+=pixels*if annotation.kind=="blur"{duration*2.+active*8.}else{active};
 }
 cost
}
pub(super) fn parts(job:&Session,frames_per_part:u64)->Vec<Part<'_>>{
 let fps=job.config.fps as f64;let mut result=Vec::new();let mut offset=0.;
 for segment in &job.segments{
  let frames=(segment.duration*fps).round().max(1.)as u64;let mut first=0;
  while first<frames{let count=(frames-first).min(frames_per_part.max(1));let start=first as f64/fps;let duration=count as f64/fps;result.push(Part{segment,start,duration,offset:offset+start,cost:section_cost(job,offset+start,duration)});first+=count;}
  offset+=segment.duration;
 }
 result
}

pub(super) fn worker_limit(gpu:bool)->usize{
 let memory_workers=available_memory().map(|n|(n.saturating_sub(2*1024*1024*1024)/(2*1024*1024*1024)).max(1)as usize).unwrap_or(1);
 thread::available_parallelism().map(|n|(n.get()/4).max(1)).unwrap_or(1).min(memory_workers).min(if gpu{8}else{4})
}

fn clone_command(source:&std::process::Command)->std::process::Command{
 let mut command=std::process::Command::new(source.get_program());command.args(source.get_args());command
}

// Hardware drivers can allow fewer concurrent encoding sessions. A worker
// that encounters that limit retires; the remaining workers take its section.
fn session_limit(error:&str)->bool{
 let e=error.to_ascii_lowercase();
 (e.contains("nvenc")&&(e.contains("session")||e.contains("memory")||e.contains("resource")||e.contains("initializeencoder")))
 ||(e.contains("qsv")&&["memory_alloc","device busy","resource","session limit"].iter().any(|s|e.contains(s)))
 ||(e.contains("amf")&&["out_of_memory","no_free_surfaces","resource","session limit"].iter().any(|s|e.contains(s)))
}

pub(super) fn parallel(commands:Vec<(std::process::Command,f64,f64)>,workers:usize,total:f64,control:export::Control)->Res<()>{
 let originals=commands.iter().map(|(c,d,_)|(clone_command(c),*d)).collect::<Vec<_>>();
 let mut pending=commands.into_iter().enumerate().map(|(i,(c,d,cost))|(i,c,d,cost)).collect::<Vec<_>>();
 // Longest estimated work first avoids a late expensive section leaving
 // every other encoder idle. Indices and final delivery order stay unchanged.
 pending.sort_by(|a,b|b.3.total_cmp(&a.3));
 let queue=Mutex::new(pending.into_iter().collect::<VecDeque<_>>());
 let completed=Arc::new(Mutex::new(vec![0f64;originals.len()]));let abort=Arc::new(AtomicBool::new(false));let errors=Mutex::new(Vec::new());
 let progress=|index:usize,duration:f64|{let completed=completed.clone();let control=control.clone();Arc::new(move|fraction:f64|{let mut done=completed.lock().unwrap();done[index]=done[index].max(fraction*duration);let sum=done.iter().sum::<f64>();if sum>0.{control.encoding(100.*sum/total);}})as Arc<dyn Fn(f64)+Send+Sync>};
 thread::scope(|scope|{
  for _ in 0..workers.max(1).min(originals.len()){
   let queue=&queue;let errors=&errors;let originals=&originals;let abort=abort.clone();let progress=&progress;let control=control.clone();
   scope.spawn(move||loop{
    if control.cancelled()||abort.load(Ordering::Relaxed){break;}
    let Some((index,command,duration,cost))=queue.lock().unwrap().pop_front()else{break;};
    if let Err(error)=export::run_render_command(command,duration,progress(index,duration),abort.clone(),control.clone()){
     if session_limit(&error)&&!control.cancelled(){queue.lock().unwrap().push_front((index,clone_command(&originals[index].0),duration,cost));log_error(&format!("GPU session limit: reducing render concurrency and requeuing section {index}."));break;}
     else{errors.lock().unwrap().push(error);abort.store(true,Ordering::Relaxed);break;}
    }
   });
  }
 });
 if control.cancelled(){return Err("Recording cancelled.".into());}
 let failures=errors.into_inner().unwrap();if !failures.is_empty(){return Err(failures.into_iter().find(|e|!e.contains("Another recording section failed")).unwrap_or_else(||"Recording section failed.".into()));}
 // If every worker hit a driver limit, make one bounded sequential attempt.
 // An unavailable encoder is reported instead of repeatedly restarting workers.
 for (index,command,duration,_)in queue.into_inner().unwrap(){export::run_render_command(command,duration,progress(index,duration),abort.clone(),control.clone())?;}
 control.set_progress(100.);Ok(())
}

#[cfg(test)]mod tests{
 use super::*;
 #[test]fn hardware_resource_limits_reduce_workers_but_driver_absence_does_not(){
  for error in ["[h264_nvenc] OpenEncodeSessionEx failed: out of memory","[h264_qsv] MFX_ERR_MEMORY_ALLOC","[h264_amf] AMF_OUT_OF_MEMORY","[h264_amf] AMF_NO_FREE_SURFACES"]{assert!(session_limit(error),"{error}");}
  for error in ["[h264_qsv] Error creating a MFX session: unsupported","[h264_amf] DLL amfrt64.dll failed to open","No such filter"]{assert!(!session_limit(error),"{error}");}
 }
 #[test]fn preparation_parallelism_uses_cores_without_exceeding_image_memory_budget(){
  let image=asset_bytes(r#"<svg width="1920" height="1080"><rect width="100%"/></svg>"#);assert_eq!(image,1920*1080*16);
  assert_eq!(asset_workers(4,4,Some(4*1024*1024*1024),image),4);
  assert_eq!(asset_workers(4,2,Some(4*1024*1024*1024),image),2);
  assert_eq!(asset_workers(4,20,Some(3*1024*1024*1024),1024*1024*1024),1);
  assert_eq!(asset_workers(4,20,None,image),1);
 }
 #[test]fn driver_session_limit_reduces_workers_without_a_serial_retry_tail(){
  let dir=std::env::temp_dir().join(format!("halo-session-limit-{}",chrono::Local::now().timestamp_nanos_opt().unwrap()));fs::create_dir_all(&dir).unwrap();let script=dir.join("encoder.ps1");
  fs::write(&script,r#"param([string]$Root,[string]$Id)
$slot=$null
foreach($number in 0..1){
 try {$path=Join-Path $Root "slot-$number"; $slot=[System.IO.File]::Open($path,[System.IO.FileMode]::CreateNew,[System.IO.FileAccess]::Write,[System.IO.FileShare]::None); break} catch [System.IO.IOException] {}
}
if($null -eq $slot){[System.IO.File]::WriteAllText((Join-Path $Root "failed-$Id"),'limit');[Console]::Error.WriteLine('[h264_nvenc] OpenEncodeSessionEx failed: out of memory');exit 1}
try {Start-Sleep -Milliseconds 700;[System.IO.File]::WriteAllText((Join-Path $Root "done-$Id"),'ok');[Console]::WriteLine('out_time_us=1000000')}
finally {$slot.Dispose();[System.IO.File]::Delete($path)}
"#).unwrap();
  let mut commands=Vec::new();for index in 0..8{let mut command=std::process::Command::new("powershell.exe");command.args(["-NoProfile","-ExecutionPolicy","Bypass","-File"]).arg(&script).arg("-Root").arg(&dir).args(["-Id",&index.to_string()]);commands.push((command,1.,index as f64));}
  let started=Instant::now();parallel(commands,4,8.,export::Control::default()).unwrap();assert!(started.elapsed()<Duration::from_secs(15));
  for index in 0..8{assert!(dir.join(format!("done-{index}")).exists(),"section {index} was lost");}
  let failed=fs::read_dir(&dir).unwrap().filter(|entry|entry.as_ref().unwrap().file_name().to_string_lossy().starts_with("failed-")).count();assert!(failed>0&&failed<=2,"session limits should retire excess workers, not fail the remaining queue: {failed}");
  assert!(!dir.join("slot-0").exists()&&!dir.join("slot-1").exists());fs::remove_dir_all(dir).unwrap();
 }
}
