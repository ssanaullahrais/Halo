//! Versioned, portable recording projects. Media is captured beside the .halo
//! document; generated export intermediates remain in the disposable cache.
use super::*;
use sha2::{Digest,Sha256};
use std::io::Read;

const KIND:&str="halo.recording-project";
const VERSION:u32=1;
const MAX_DOCUMENT:u64=64*1024*1024;

#[derive(Serialize,Deserialize)]
pub(super) struct Project {
 pub format:String,pub schema_version:u32,pub project_id:String,pub revision:u64,
 pub created_at:String,pub updated_at:String,pub app_version:String,
 pub capture:Capture,pub render:serde_json::Value,pub timeline:Timeline,
 pub annotations:Vec<crate::recorder_annotations::Annotation>,
 pub original:Option<Original>,pub export:ExportState,
}
#[derive(Serialize,Deserialize)]
pub(super) struct Capture {pub source_type:String,pub width:u32,pub height:u32,pub fps:u32}
#[derive(Serialize,Deserialize)]
pub(super) struct Timeline {pub time_unit:String,pub duration:f64,pub segments:Vec<SourceSegment>}
#[derive(Serialize,Deserialize)]
pub(super) struct SourceSegment {
 pub id:String,pub start:f64,pub duration:f64,pub video:String,
 pub native_cursor_baked:bool,pub audio:Vec<AudioSource>,pub cursor_events:Option<String>,
}
#[derive(Serialize,Deserialize)]
pub(super) struct AudioSource {pub id:String,pub role:String,pub file:String}
#[derive(Serialize,Deserialize)]
pub(super) struct Original {pub file:String,pub mix_signature:String}
#[derive(Serialize,Deserialize)]
pub(super) struct ExportState {pub state:String,pub output:Option<String>,#[serde(default)]pub output_location:Option<String>,pub error:Option<String>}

pub(super) fn assets_dir(job:&Session)->PathBuf {
 job.project.as_ref().map(|p|p.with_extension("assets")).unwrap_or_else(||job.dir.clone())
}
fn render_settings(config:&Config)->serde_json::Value {
 let mut value=serde_json::to_value(config).expect("serializable recorder config");
 for key in ["shortcut","output_dir","microphone_id","microphone","system_audio","countdown"]{value.as_object_mut().unwrap().remove(key);}
 value
}
fn relative(root:&Path,path:&Path)->Res<String>{
 path.strip_prefix(root).map(|p|p.to_string_lossy().replace('\\',"/")).map_err(|_|"Project media must be beside its project file.".into())
}
fn root_of(path:&Path)->Res<&Path>{path.parent().ok_or_else(||"Choose a project in a folder.".into())}
fn export_folder(project_root:&Path)->PathBuf {
 // Native layout: recordings/source-files/name/name.halo. Standalone moved
 // projects export beside their document, without depending on the old folder.
 if project_root.file_name().map(|s|s.eq_ignore_ascii_case("source-files")).unwrap_or(false){return project_root.parent().unwrap_or(project_root).to_path_buf();}
 if let Some(parent)=project_root.parent(){if parent.file_name().map(|s|s.eq_ignore_ascii_case("source-files")).unwrap_or(false){return parent.parent().unwrap_or(project_root).to_path_buf();}}
 project_root.to_path_buf()
}
static DOCUMENT_WRITE:Mutex<()>=Mutex::new(());
fn read(path:&Path)->Res<Project>{
 let file=fs::File::open(path).map_err(|_|"Could not read the Halo project file.".to_string())?;
 if file.metadata().map_err(|e|e.to_string())?.len()>MAX_DOCUMENT{return Err("This project document is too large.".into());}
 let project:Project=serde_json::from_reader(file.take(MAX_DOCUMENT+1)).map_err(|_|"This is not a valid Halo recording project.".to_string())?;
 if project.format!=KIND{return Err("This file is not a Halo recording project.".into());}
 if project.schema_version!=VERSION{return Err(format!("Project format {} is not supported by this version of Halo.",project.schema_version));}
 Ok(project)
}
fn write(path:&Path,project:&Project)->Res<()> {
 let data=serde_json::to_vec_pretty(project).map_err(|e|e.to_string())?;
 if data.len()as u64>MAX_DOCUMENT{return Err("The project contains too much editing data to save.".into());}
 let temporary=path.with_extension(format!("halo.partial-{}",chrono::Local::now().timestamp_nanos_opt().unwrap_or_default()));
 let result=(||{
  let mut file=fs::OpenOptions::new().write(true).create_new(true).open(&temporary).map_err(|e|e.to_string())?;
  file.write_all(&data).map_err(|e|e.to_string())?;file.sync_all().map_err(|e|e.to_string())?;drop(file);
  let started=Instant::now();
  loop{match fs::rename(&temporary,path){
   Ok(())=>return Ok(()),
   Err(e)if matches!(e.raw_os_error(),Some(32|33))&&started.elapsed()<Duration::from_secs(3)=>thread::sleep(Duration::from_millis(40)),
   Err(e)=>return Err(e.to_string()),
  }}
 })();
 if result.is_err(){let _=fs::remove_file(temporary);}result
}

pub(super) fn create(job:&mut Session)->Res<()> {
 let root=job.output.parent().ok_or("Choose a recording folder.")?.to_path_buf();
 let sources=root.join("source-files");fs::create_dir_all(&sources).map_err(|e|e.to_string())?;
 let base=job.output.file_stem().ok_or("Choose a recording name.")?.to_string_lossy().into_owned();
 for suffix in 0..10000 {
  let name=if suffix==0{base.clone()}else{format!("{base} ({suffix})")};
  let folder=sources.join(&name);let path=folder.join(format!("{name}.halo"));let assets=path.with_extension("assets");let output=root.join(format!("{name}.mp4"));
  if output.exists(){continue;}
  match fs::create_dir(&folder){Ok(())=>{},Err(e)if e.kind()==std::io::ErrorKind::AlreadyExists=>continue,Err(e)=>return Err(e.to_string())}
  fs::create_dir(&assets).map_err(|e|e.to_string())?;
  job.project=Some(path);job.output=output;
  return checkpoint(job,None,"recording");
 }
 Err("Could not choose an unused project name.".into())
}

pub(super) fn checkpoint(job:&Session,original:Option<&Path>,state:&str)->Res<()> {let _write=DOCUMENT_WRITE.lock().unwrap();
 let Some(path)=&job.project else{return Ok(());};let root=root_of(path)?;
 let previous=if path.exists(){Some(read(path)?)}else{None};let now=chrono::Utc::now().to_rfc3339();
 let mut offset=0.;let mut segments=Vec::new();
 for (i,segment) in job.segments.iter().enumerate(){
  let cursor=assets_dir(job).join(format!("cursor-{i}.jsonl"));
  if segment.commands.exists()&&!cursor.exists(){return Err("The recording's editable cursor events are missing.".into());}
  let audio=segment.audio.iter().enumerate().map(|(n,(file,mic))|Ok(AudioSource{id:format!("segment-{i}-audio-{n}"),role:if *mic{"microphone"}else{"system"}.into(),file:relative(root,file)?})).collect::<Res<Vec<_>>>()?;
  segments.push(SourceSegment{id:format!("segment-{i}"),start:offset,duration:segment.duration,video:relative(root,&segment.raw)?,native_cursor_baked:job.config.frame!="mac"&&["normal","highlight"].contains(&job.config.cursor.as_str()),audio,cursor_events:if cursor.exists(){Some(relative(root,&cursor)?)}else{None}});offset+=segment.duration;
 }
 let mut document=Project{format:KIND.into(),schema_version:VERSION,project_id:previous.as_ref().map(|p|p.project_id.clone()).unwrap_or_else(||format!("{}-{}",chrono::Local::now().timestamp_nanos_opt().unwrap_or_default(),std::process::id())),revision:1,created_at:previous.as_ref().map(|p|p.created_at.clone()).unwrap_or_else(||now.clone()),updated_at:now,app_version:env!("CARGO_PKG_VERSION").into(),capture:Capture{source_type:job.target.mode.clone(),width:job.bounds.w,height:job.bounds.h,fps:job.config.fps},render:render_settings(&job.config),timeline:Timeline{time_unit:"seconds".into(),duration:offset,segments},annotations:job.annotations.clone(),original:None,export:ExportState{state:state.into(),output:None,output_location:None,error:None}};
 // Published MP4s may live outside the project folder. They are optional cache
 // artifacts, never dependencies of the portable source.
 if let Some(original)=original.filter(|p|p.starts_with(root)){document.original=Some(Original{file:relative(root,original)?,mix_signature:mix_signature(&document)?});}
 write(path,&document)
}

fn mix_signature(project:&Project)->Res<String>{
 let mix=serde_json::json!({"revision":project.revision,"segments":project.timeline.segments,"mic_gain":project.render["mic_gain"],"system_gain":project.render["system_gain"],"voice_enhance":project.render["voice_enhance"]});
 Ok(format!("{:x}",Sha256::digest(serde_json::to_vec(&mix).map_err(|e|e.to_string())?)))
}
pub(super) fn set_status(job:&Session,state:&str,error:Option<&str>)->Res<()> {let _write=DOCUMENT_WRITE.lock().unwrap();
 let Some(path)=&job.project else{return Ok(());};let mut document=read(path)?;
 document.updated_at=chrono::Utc::now().to_rfc3339();document.export=ExportState{state:state.into(),output:if state=="completed"{job.output.file_name().map(|n|n.to_string_lossy().into_owned())}else{None},output_location:if state=="completed"{Some(if job.output.parent()==Some(root_of(path)?){"project_folder"}else{"recording_folder"}.into())}else{None},error:error.map(String::from)};write(path,&document)
}
pub(super) fn set_original(job:&Session,path:&Path)->Res<()> {let _write=DOCUMENT_WRITE.lock().unwrap();
 let Some(project)=&job.project else{return Ok(());};let mut document=read(project)?;
 document.original=if path.starts_with(root_of(project)?){Some(Original{file:relative(root_of(project)?,path)?,mix_signature:mix_signature(&document)?})}else{None};document.updated_at=chrono::Utc::now().to_rfc3339();write(project,&document)
}

// The import boundary accepts portable relative paths only, and checks their
// canonical destination as well, so junctions cannot escape the project folder.
fn asset(root:&Path,name:&str,required:bool)->Res<Option<PathBuf>> {
 if name.is_empty()||name.contains(['\\',':'])||Path::new(name).components().any(|p|!matches!(p,std::path::Component::Normal(_))){return Err("The project contains an unsafe media path.".into());}
 let candidate=root.join(name);
 if !candidate.exists(){return if required{Err(format!("Missing project media: {name}. Keep the .halo file and its .assets folder together."))}else{Ok(None)};}
 let canonical=candidate.canonicalize().map_err(|_|format!("Could not open project media: {name}"))?;
 if !canonical.starts_with(root)||!canonical.is_file(){return Err("Project media must be inside the project folder.".into());}
 Ok(Some(canonical))
}
fn media_header(path:&Path,video:bool)->Res<()> {
 let mut header=[0u8;12];let valid=fs::File::open(path).and_then(|mut file|file.read_exact(&mut header)).is_ok()
  &&if video{&header[4..8]==b"ftyp"}else{&header[..4]==b"RIFF"&&&header[8..12]==b"WAVE"};
 if valid{Ok(())}else{Err(format!("Damaged project {}: {}",if video{"video"}else{"audio"},path.file_name().unwrap_or_default().to_string_lossy()))}
}

pub(super) fn load(path:&Path)->Res<(Session,Option<PathBuf>)> {
 if !path.extension().map(|s|s.eq_ignore_ascii_case("halo")).unwrap_or(false){return Err("Choose a .halo project file.".into());}
 let path=path.canonicalize().map_err(|_|"Could not find that Halo project.".to_string())?;let root=root_of(&path)?;let document=read(&path)?;
 let mut config:Config=serde_json::from_value(document.render.clone()).map_err(|_|"This project has invalid export settings.".to_string())?;
 config.output_dir=root.to_string_lossy().into_owned();config.microphone=false;config.system_audio=false;config.countdown=0;config.microphone_id.clear();validate(&config)?;
 let total=document.timeline.duration;
 if !["screen","window","area"].contains(&document.capture.source_type.as_str())||!(2..=8192).contains(&document.capture.width)||!(2..=8192).contains(&document.capture.height)||document.capture.fps!=config.fps||document.timeline.time_unit!="seconds"||!total.is_finite()||total<=0.||total>7.*86400.||document.timeline.segments.is_empty()||document.timeline.segments.len()>10000{return Err("This project has an invalid recording timeline.".into());}
 let mut segments=Vec::new();let mut offset=0.;
 for source in &document.timeline.segments{
  if !source.start.is_finite()||!source.duration.is_finite()||source.duration<=0.||(source.start-offset).abs()>0.00001{return Err("This project has invalid segment timing.".into());}
  let raw=asset(root,&source.video,true)?.unwrap();media_header(&raw,true)?;let mut audio=Vec::new();
  if source.audio.len()>2{return Err("This project contains too many audio tracks in a segment.".into());}
  for track in &source.audio{if !["microphone","system"].contains(&track.role.as_str())||audio.iter().any(|(_,mic)|*mic==(track.role=="microphone")){return Err("This project has invalid audio tracks.".into());}let file=asset(root,&track.file,true)?.unwrap();media_header(&file,false)?;audio.push((file,track.role=="microphone"));}
  let cursor=source.cursor_events.as_ref().map(|name|asset(root,name,true).map(|p|p.unwrap())).transpose()?;
  segments.push((raw,audio,cursor,source.duration));offset+=source.duration;
 }
 if (offset-total).abs()>0.00001||document.annotations.len()>100{return Err("This project has an invalid recording timeline.".into());}
 for annotation in &document.annotations{
  crate::recorder_annotations::validate(annotation,document.capture.width,document.capture.height)?;
  if !annotation.start.is_finite()||annotation.start<0.||annotation.start>total||annotation.end.map(|e|!e.is_finite()||e<annotation.start||e>total+0.1).unwrap_or(false)||annotation.keyframes.len()>3600{return Err("This project has invalid drawing timing.".into());}
  let mut time=-1.;for frame in &annotation.keyframes{if !frame.time.is_finite()||frame.time<time||frame.time<0.||frame.time>total+0.1{return Err("This project has invalid drawing keyframes.".into());}crate::recorder_annotations::validate(&annotation.at(frame),document.capture.width,document.capture.height)?;time=frame.time;}
 }
 let cached=if let Some(original)=&document.original{let file=asset(root,&original.file,false)?.filter(|p|media_header(p,true).is_ok());if original.mix_signature==mix_signature(&document)?{file}else{None}}else{None};
 let dir=crate::system::tools_dir().join("recorder-cache").join(format!("project-export-{}",chrono::Local::now().timestamp_nanos_opt().unwrap_or_default()));fs::create_dir_all(&dir).map_err(|e|e.to_string())?;
 let output_root=export_folder(root);config.output_dir=output_root.to_string_lossy().into_owned();
 let name=format!("{} (export)",path.file_stem().unwrap().to_string_lossy());let output=crate::process::unique(&output_root,&name,"mp4");
 let mut restored=Vec::new();
 for (i,(raw,audio,cursor,duration)) in segments.into_iter().enumerate(){
  let commands=dir.join(format!("cursor-{i}.txt"));
  if let Some(cursor)=cursor{restore_cursor(&cursor,&commands,&config,document.capture.width,document.capture.height,duration)?;}
  else if cursor_effects(&config){return Err("This project is missing editable cursor events.".into());}
  restored.push(Segment{raw,audio,commands,duration});
 }
 let bounds=Rect{x:0,y:0,w:document.capture.width,h:document.capture.height};
 let gpu=config.encoder=="auto"&&bounds.w+2*frame_pad(&config,bounds.w)<=4096&&bounds.h+2*frame_pad(&config,bounds.w)+40<=4096&&gpu_available(&dir,bounds.w,bounds.h,config.fps);
 let job=Session{config,target:Target{mode:document.capture.source_type,monitor:0,window:0,rect:None},bounds,dir,output,project:Some(path.clone()),original_output:None,segments:restored,running:None,gpu,mic_peak:Arc::new(AtomicU32::new(0)),sys_peak:Arc::new(AtomicU32::new(0)),mic_muted:Arc::new(AtomicBool::new(false)),sys_muted:Arc::new(AtomicBool::new(false)),annotations:document.annotations};
 Ok((job,cached))
}

#[derive(Serialize,Deserialize,Clone)]
pub(super) struct CursorSample {pub time:f64,pub x:i32,pub y:i32,pub inside:bool,pub buttons:u8}
pub(super) fn cursor_effects(config:&Config)->bool {
 config.cursor=="highlight"||config.cursor=="large"||(config.frame=="mac"&&config.cursor!="hidden")||config.click_effect||config.cursor_effect!="none"
}
pub(super) struct CursorProgram {config:Config,width:u32,height:u32,was_down:bool,clicks:[Option<(f64,i32,i32)>;CLICK_SLOTS],slot:usize,history:std::collections::VecDeque<(f64,i32,i32)>}
impl CursorProgram {
 pub(super) fn new(config:&Config,width:u32,height:u32)->Self{Self{config:config.clone(),width,height,was_down:false,clicks:[None;CLICK_SLOTS],slot:0,history:std::collections::VecDeque::new()}}
 pub(super) fn push(&mut self,s:&CursorSample,out:&mut impl Write)->std::io::Result<()> {
  let c=&self.config;let size=c.cursor_size;let t=s.time;let arrow=c.cursor=="large"||(c.frame=="mac"&&c.cursor=="normal");let inside=s.inside;let down=s.buttons!=0;let click_size=size*2;let spot_size=(size*6).min(self.width.min(self.height)).max(2);
  let (x,y)=if inside{(s.x-if arrow{(size as f32*5./64.).round()as i32}else{size as i32/2},s.y-if arrow{(size as f32*3./64.).round()as i32}else{size as i32/2})}else{(-200,-200)};
  if c.click_effect&&down&&!self.was_down&&inside{self.clicks[self.slot]=Some((t,s.x-click_size as i32/2,s.y-click_size as i32/2));self.slot=(self.slot+1)%CLICK_SLOTS;}self.was_down=down;
  writeln!(out,"{t:.3} overlay@pointer x {x};\n{t:.3} overlay@pointer y {y};")?;
  for (slot,event)in self.clicks.iter_mut().enumerate(){if let Some((at,cx,cy))=*event{let age=t-at;if age<0.5{let frame=((age/0.5*60.).floor()as i32).clamp(0,60);writeln!(out,"{t:.3} overlay@click{slot} x {cx};\n{t:.3} overlay@click{slot} y {cy};\n{t:.3} colorchannelmixer@clickalpha{slot} aa 1;\n{t:.3} crop@clickframe{slot} x {};",frame*click_size as i32)?;}else{writeln!(out,"{t:.3} colorchannelmixer@clickalpha{slot} aa 0;")?;*event=None;}}}
  if c.cursor_effect=="spotlight"{let sx=(s.x-spot_size as i32/2).clamp(0,(self.width-spot_size)as i32);let sy=(s.y-spot_size as i32/2).clamp(0,(self.height-spot_size)as i32);let dim=if inside{-0.16*c.effect_strength}else{0.};let alpha=if inside{1.}else{0.};writeln!(out,"{t:.3} eq@spotdim brightness {dim:.3};\n{t:.3} crop@spotcrop x {sx};\n{t:.3} crop@spotcrop y {sy};\n{t:.3} overlay@spotlight x {sx};\n{t:.3} overlay@spotlight y {sy};\n{t:.3} colorchannelmixer@spotalpha aa {alpha};")?;}
  if c.cursor_effect=="trail"{self.history.push_back((t,s.x,s.y));while self.history.front().map(|v|t-v.0>0.3).unwrap_or(false){self.history.pop_front();}for (i,delay)in [0.025,0.065,0.12].iter().enumerate(){let past=self.history.iter().rev().find(|v|v.0<=t-delay).or(self.history.front());if let Some((_,px,py))=past{let moved=(*px-s.x).abs()+(*py-s.y).abs();let alpha=if inside{(moved as f64/35.).min(1.)*c.effect_strength*(1.-i as f64*0.25)}else{0.};let tx=px-size as i32/2;let ty=py-size as i32/2;writeln!(out,"{t:.3} overlay@trail{i} x {tx};\n{t:.3} overlay@trail{i} y {ty};\n{t:.3} colorchannelmixer@trailalpha{i} aa {alpha:.3};")?;}}}
  Ok(())
 }
}

fn restore_cursor(path:&Path,commands:&Path,config:&Config,w:u32,h:u32,duration:f64)->Res<()> {
 let mut reader=BufReader::new(fs::File::open(path).map_err(|_|"Could not read cursor events.".to_string())?);
 let mut output=if cursor_effects(config){Some(std::io::BufWriter::new(fs::File::create(commands).map_err(|e|e.to_string())?))}else{None};
 let mut program=CursorProgram::new(config,w,h);let mut previous=-1.;let mut line=String::new();
 loop{
  if CANCEL.load(Ordering::Relaxed){return Err("Project export cancelled.".into());}
  line.clear();let size=reader.by_ref().take(4097).read_line(&mut line).map_err(|_|"Could not read cursor events.".to_string())?;
  if size==0{break;}if size>4096{return Err("This project has an invalid cursor event.".into());}
  let sample:CursorSample=serde_json::from_str(&line).map_err(|_|"This project has an invalid cursor event.".to_string())?;
  if !sample.time.is_finite()||sample.time<previous||sample.time<0.||sample.time>duration+0.1||sample.buttons>3||sample.x.unsigned_abs()>1_000_000||sample.y.unsigned_abs()>1_000_000||(sample.inside&&(sample.x<0||sample.y<0||sample.x>=w as i32||sample.y>=h as i32)){return Err("This project has invalid cursor timing or coordinates.".into());}
  previous=sample.time;if let Some(out)=output.as_mut(){program.push(&sample,out).map_err(|e|e.to_string())?;}
 }
 if let Some(out)=output.as_mut(){out.flush().map_err(|e|e.to_string())?;}Ok(())
}

#[cfg(test)]mod tests {
 use super::*;
 fn minimal()->(PathBuf,Project){
  let root=std::env::temp_dir().join(format!("halo-project-{}",chrono::Local::now().timestamp_nanos_opt().unwrap()));fs::create_dir_all(root.join("source.assets")).unwrap();fs::write(root.join("source.assets/video.mp4"),b"fixture").unwrap();
  let mut config=Config::default();config.encoder="cpu".into();config.cursor="hidden".into();
  let now=chrono::Utc::now().to_rfc3339();
  let project=Project{format:KIND.into(),schema_version:VERSION,project_id:"fixture".into(),revision:1,created_at:now.clone(),updated_at:now,app_version:env!("CARGO_PKG_VERSION").into(),capture:Capture{source_type:"screen".into(),width:384,height:256,fps:config.fps},render:render_settings(&config),timeline:Timeline{time_unit:"seconds".into(),duration:1.,segments:vec![SourceSegment{id:"segment-0".into(),start:0.,duration:1.,video:"source.assets/video.mp4".into(),native_cursor_baked:false,audio:vec![],cursor_events:None}]},annotations:vec![],original:None,export:ExportState{state:"cancelled".into(),output:None,output_location:None,error:None}};
  (root,project)
 }
 #[test]fn runtime_paths_and_device_preferences_are_not_saved_in_render_settings(){
  let config=Config::default();let saved=render_settings(&config);
  for field in ["output_dir","shortcut","microphone_id","countdown"]{assert!(saved.get(field).is_none());}
  assert_eq!(saved["fps"],config.fps);
 }
 #[test]fn unsupported_versions_and_missing_media_are_actionable(){
  let (root,mut project)=minimal();let path=root.join("source.halo");project.schema_version=999;write(&path,&project).unwrap();assert!(load(&path).err().unwrap().contains("not supported"));
  project.schema_version=VERSION;project.timeline.segments[0].video="source.assets/missing.mp4".into();write(&path,&project).unwrap();assert!(load(&path).err().unwrap().contains("Missing project media"));fs::remove_dir_all(root).unwrap();
 }
 #[test]fn path_traversal_absolute_paths_and_malformed_timing_are_rejected(){
  let (root,mut project)=minimal();let path=root.join("source.halo");
  for bad in ["../private.mp4","C:/private.mp4","/private.mp4","source.assets\\video.mp4"]{project.timeline.segments[0].video=bad.into();write(&path,&project).unwrap();assert!(load(&path).err().unwrap().contains("unsafe media path"));}
  project.timeline.segments[0].video="source.assets/video.mp4".into();project.timeline.segments[0].start=2.;write(&path,&project).unwrap();assert!(load(&path).err().unwrap().contains("segment timing"));fs::remove_dir_all(root).unwrap();
 }
 #[test]fn mix_cache_is_invalidated_by_audio_edits_and_project_revision(){
  let (root,mut project)=minimal();let before=mix_signature(&project).unwrap();project.render["mic_gain"]=serde_json::json!(0.5);assert_ne!(before,mix_signature(&project).unwrap());let before=mix_signature(&project).unwrap();project.revision+=1;assert_ne!(before,mix_signature(&project).unwrap());fs::remove_dir_all(root).unwrap();
 }
 #[test]fn damaged_source_video_is_reported_before_export(){
  let (root,project)=minimal();let path=root.join("source.halo");write(&path,&project).unwrap();assert!(load(&path).err().unwrap().contains("Damaged project video"));fs::remove_dir_all(root).unwrap();
 }
 #[test]fn organized_projects_export_to_the_recording_folder(){
  let root=PathBuf::from(r"C:\Recordings");assert_eq!(export_folder(&root.join("source-files").join("Capture")),root);let moved=PathBuf::from(r"D:\Moved\Capture");assert_eq!(export_folder(&moved),moved);
 }
 #[test]fn semantic_cursor_events_rebuild_timed_effect_commands(){
  let (root,_)=minimal();let file=root.join("events.jsonl");let commands=root.join("commands.txt");let config=Config{click_effect:true,cursor:"large".into(),cursor_effect:"trail".into(),..Config::default()};
  let samples=[CursorSample{time:0.,x:40,y:60,inside:true,buttons:0},CursorSample{time:0.1,x:80,y:60,inside:true,buttons:1},CursorSample{time:0.3,x:100,y:60,inside:true,buttons:0},CursorSample{time:0.8,x:140,y:60,inside:true,buttons:0}];
  fs::write(&file,samples.iter().map(|s|format!("{}\n",serde_json::to_string(s).unwrap())).collect::<String>()).unwrap();restore_cursor(&file,&commands,&config,384,256,1.).unwrap();let text=fs::read_to_string(commands).unwrap();assert!(text.contains("0.100 colorchannelmixer@clickalpha0 aa 1"));assert!(text.contains("0.800 colorchannelmixer@clickalpha0 aa 0"));assert!(text.contains("overlay@trail0"));fs::remove_dir_all(root).unwrap();
 }
 #[test]fn corrupt_cursor_events_fail_without_being_executed(){
  let (root,_)=minimal();let file=root.join("events.jsonl");fs::write(&file,b"not cursor data\n").unwrap();let config=Config{cursor:"large".into(),..Config::default()};assert!(restore_cursor(&file,&root.join("commands.txt"),&config,384,256,1.).is_err());fs::remove_dir_all(root).unwrap();
 }
 #[test]#[ignore="requires local FFmpeg"]fn moved_project_reexports_identically_without_original_or_capture_cache(){
  let mut job=super::super::media_tests::fixture(30,"strong");job.config.frame="mac".into();job.config.click_effect=true;
  let captured=std::mem::take(&mut job.segments);create(&mut job).unwrap();job.segments=captured;let assets=assets_dir(&job);let raw=assets.join("segment-0.mp4");fs::copy(&job.segments[0].raw,&raw).unwrap();job.segments[0].raw=raw;
  job.segments[0].commands=job.dir.join("cursor-0.txt");let events=assets.join("cursor-0.jsonl");
  let mut samples=String::new();for n in 0..36{let sample=CursorSample{time:n as f64/30.,x:50+n*4,y:120,inside:true,buttons:if n==6{1}else{0}};samples+=&format!("{}\n",serde_json::to_string(&sample).unwrap());}fs::write(&events,samples).unwrap();restore_cursor(&events,&job.segments[0].commands,&job.config,384,256,1.2).unwrap();
  let audio=assets.join("mic-0.wav");let mut tone=ffmpeg();tone.args(["-f","lavfi","-i","sine=frequency=440:sample_rate=48000","-t","1.2"]).arg(&audio);run(tone).unwrap();job.segments[0].audio.push((audio,true));
  let mut annotation=crate::recorder_annotations::Annotation{id:1,kind:"arrow".into(),x:30.,y:40.,w:180.,h:60.,text:String::new(),color:"#FF375F".into(),stroke:3.,font_size:24.,start:0.1,end:None,keyframes:vec![],points:vec![]};annotation.keyframes.push(annotation.frame(0.1));annotation.w=260.;annotation.keyframes.push(annotation.frame(0.6));job.annotations.push(annotation);
  let original=job.output.with_file_name("original.mp4");export::save_original(&job,&original).unwrap();job.original_output=Some(original.clone());checkpoint(&job,Some(&original),"cancelled").unwrap();finish(&job).unwrap();
  let decode=|path:&Path|{let mut cmd=ffmpeg();cmd.arg("-i").arg(path).args(["-map","0:v:0","-pix_fmt","rgb24","-f","rawvideo","pipe:1"]);let result=cmd.output().unwrap();assert!(result.status.success());result.stdout};let expected=decode(&job.output);
  let moved=job.dir.with_file_name(format!("halo-moved-{}",chrono::Local::now().timestamp_nanos_opt().unwrap()));fs::create_dir_all(&moved).unwrap();let project=job.project.as_ref().unwrap();let new_project=moved.join(project.file_name().unwrap());fs::copy(project,&new_project).unwrap();let new_assets=new_project.with_extension("assets");fs::create_dir_all(&new_assets).unwrap();for entry in fs::read_dir(&assets).unwrap(){let entry=entry.unwrap();fs::copy(entry.path(),new_assets.join(entry.file_name())).unwrap();}
  fs::remove_file(original).unwrap();fs::remove_dir_all(&assets).unwrap();
  let (mut loaded,cached)=load(&new_project).unwrap();assert!(cached.is_none());assert_eq!(loaded.annotations[0].keyframes.len(),2);assert_eq!(loaded.segments[0].audio.len(),1);assert!(loaded.segments[0].raw.starts_with(moved.canonicalize().unwrap()));
  let rebuilt=moved.canonicalize().unwrap().join("rebuilt-original.mp4");export::save_original(&loaded,&rebuilt).unwrap();loaded.original_output=Some(rebuilt);finish(&loaded).unwrap();assert_eq!(expected,decode(&loaded.output));set_status(&loaded,"completed",None).unwrap();assert_eq!(read(&new_project).unwrap().export.state,"completed");
  println!("Moved project restored drawings, clicks and microphone audio and reproduced identical video frames: {}",new_project.display());
 }
}
