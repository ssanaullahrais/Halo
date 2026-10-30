//! Recorder exports outlive their widget and have independent cancellation.
use super::*;
use std::collections::VecDeque;

#[derive(Clone,Serialize)]pub struct View{pub id:String,pub label:String,pub destination:String,pub phase:String,pub stage:String,pub percent:f64,pub original_output:String,pub output:Option<String>,pub project_path:Option<String>,pub error:Option<String>}
struct Entry{view:Mutex<View>,control:export::Control,op:Arc<crate::OpEntry>}
struct Work{entry:Arc<Entry>,session:Session,app:tauri::AppHandle,_update:crate::updates::Job}
struct Queue{pending:VecDeque<Work>,entries:VecDeque<Arc<Entry>>,running:bool}
static QUEUE:Mutex<Queue>=Mutex::new(Queue{pending:VecDeque::new(),entries:VecDeque::new(),running:false});

fn snapshot(entry:&Entry)->View{
 let mut view=entry.view.lock().unwrap().clone();
 if view.phase=="exporting"{let (stage,percent)=entry.control.snapshot();view.stage=match stage{1|2=>"preparing",3=>"encoding",_=>"finalizing"}.into();view.percent=percent;if entry.control.cancelled(){view.stage="cancelling".into();}}
 view
}
pub(super) fn views()->Vec<View>{let entries=QUEUE.lock().unwrap().entries.iter().cloned().collect::<Vec<_>>();entries.iter().map(|e|snapshot(e)).collect()}
fn terminal(phase:&str)->bool{matches!(phase,"completed"|"cancelled"|"failed")}
fn title(view:&View)->String{match view.stage.as_str(){"queued"=>"Queued for export".into(),"preparing"=>format!("Preparing {:.0}%",view.percent),"encoding"=>format!("Exporting {}%",if view.percent<1.{format!("{:.2}",view.percent)}else{format!("{:.0}",view.percent.floor())}),"cancelling"=>"Cancelling export…".into(),_=>"Finalizing video…".into()}}
fn publish(app:&tauri::AppHandle,entry:&Entry){
 let view=snapshot(entry);if terminal(&view.phase){return;}
 let title=title(&view);let notice=serde_json::json!({"kind":"busy","opId":view.id,"title":title,"file":view.label,"total":"Original video and project saved","pct":view.percent/100.,"indeterminate":matches!(view.stage.as_str(),"queued"|"finalizing"|"cancelling"),"cancelling":view.stage=="cancelling"});
 let label=format!("{} — {}",title,view.label);let changed={let mut current=entry.op.label.lock().unwrap();if *current==label{false}else{*current=label;true}};
 *entry.op.last_notice.lock().unwrap()=Some(notice.clone());let _=app.emit("halo://operation-state",&notice);
 if changed{let shared=app.state::<Arc<crate::Shared>>().inner().clone();crate::rebuild_tray_menu(app,&shared);}
}
fn complete(app:&tauri::AppHandle,entry:&Entry,phase:&str,error:Option<String>,output:Option<String>){
 {let mut view=entry.view.lock().unwrap();view.phase=phase.into();view.stage=phase.into();view.percent=if phase=="completed"{100.}else{0.};view.error=error;view.output=output;}
 let view=snapshot(entry);let notice=if phase=="completed"{serde_json::json!({"kind":"done","opId":view.id,"title":"Recording export complete","detail":view.label,"paths":[view.output]})}else{serde_json::json!({"kind":"toast","opId":view.id,"tone":if phase=="failed"{"error"}else{"info"},"text":view.error})};
 if phase=="completed"{let _=app.emit("halo://recording-saved",&view.output);}
 *entry.op.last_notice.lock().unwrap()=Some(notice.clone());
 let notifications=app.state::<Arc<crate::Shared>>().notifications.load(Ordering::Relaxed);
 if phase=="cancelled"||(!notifications&&phase!="failed"){let _=app.emit_to("notice","halo://operation-finished",notice);}else{crate::show_notice(app.clone(),notice);}
 let shared=app.state::<Arc<crate::Shared>>().inner().clone();shared.operations.lock().unwrap().remove(&view.id);crate::rebuild_tray_menu(app,&shared);
}
pub(super) fn enqueue(app:tauri::AppHandle,mut session:Session,original:String)->Res<String>{
 let mut queue=QUEUE.lock().unwrap();let base=session.output.clone();let stem=base.file_stem().unwrap_or_default().to_string_lossy();let mut suffix=2;
 let key=|path:&Path|path.to_string_lossy().replace('\\',"/").trim_start_matches("//?/").to_lowercase();
 while session.output.exists()||queue.entries.iter().any(|e|{let v=e.view.lock().unwrap();!terminal(&v.phase)&&key(Path::new(&v.destination))==key(&session.output)}){session.output=base.with_file_name(format!("{stem} ({suffix}).mp4"));suffix+=1;}
 let update=crate::updates::job()?;let id=format!("recording-{}",chrono::Local::now().timestamp_nanos_opt().unwrap_or_default());let cancel=Arc::new(AtomicBool::new(false));
 let op=Arc::new(crate::OpEntry{cancel:cancel.clone(),pid:Mutex::new(None),label:Mutex::new(String::new()),last_notice:Mutex::new(None)});
 let entry=Arc::new(Entry{control:export::Control::background(cancel),op:op.clone(),view:Mutex::new(View{id:id.clone(),label:session.output.file_name().unwrap_or_default().to_string_lossy().into_owned(),destination:session.output.to_string_lossy().into_owned(),phase:"queued".into(),stage:"queued".into(),percent:0.,original_output:original,output:None,project_path:session.project.as_ref().map(|p|p.to_string_lossy().into_owned()),error:None})});
 if let Err(error)=project::set_status(&session,"queued",None){log_error(&format!("Project queue status: {error}"));}
 let shared=app.state::<Arc<crate::Shared>>().inner().clone();shared.operations.lock().unwrap().insert(id.clone(),op);
 queue.entries.push_back(entry.clone());queue.pending.push_back(Work{entry:entry.clone(),session,app:app.clone(),_update:update});while queue.entries.len()>100{if queue.entries.front().map(|e|terminal(&e.view.lock().unwrap().phase)).unwrap_or(false){queue.entries.pop_front();}else{break;}}let start=!queue.running;queue.running=true;drop(queue);
 publish(&app,&entry);if start{tauri::async_runtime::spawn_blocking(worker);}
 Ok(id)
}
fn worker(){loop{
 let work={let mut queue=QUEUE.lock().unwrap();match queue.pending.pop_front(){Some(work)=>work,None=>{queue.running=false;return;}}};
 let Work{entry,session,app,_update}=work;
 entry.view.lock().unwrap().phase="exporting".into();entry.control.set_stage(1);publish(&app,&entry);
 let done=Arc::new(AtomicBool::new(false));let watch_done=done.clone();let watch_entry=entry.clone();let watch_app=app.clone();let watcher=thread::spawn(move||{while !watch_done.load(Ordering::Relaxed){publish(&watch_app,&watch_entry);thread::sleep(Duration::from_millis(400));}});
 let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||{if entry.control.cancelled(){return Err("Recording export cancelled.".into());}project::set_status(&session,"exporting",None)?;finish_controlled(&session,entry.control.clone())}));
 done.store(true,Ordering::Relaxed);let _=watcher.join();
 match result{
  Ok(Ok((path,_)))=>{if let Err(error)=project::set_status(&session,"completed",None){log_error(&error);}complete(&app,&entry,"completed",None,Some(path.to_string_lossy().into_owned()));}
  other=>{entry.control.terminate_children();let cancelled=entry.control.cancelled();let error=match other{Ok(Err(e))=>e,_=>"The export worker stopped unexpectedly.".into()};log_error(&format!("Background export {}: {error}",entry.view.lock().unwrap().id));let phase=if cancelled{"cancelled"}else{"failed"};let message=if cancelled{"Export cancelled. Your original video and project are saved."}else{"Export could not finish. Your original video and project are saved."};if let Err(error)=project::set_status(&session,phase,Some(message)){log_error(&error);}complete(&app,&entry,phase,Some(message.into()),None);}
 }
 drop(_update);
}}
pub(super) fn foreground(state:&mut State){
 let Some(id)=state.export_id.as_ref()else{return;};let entry=QUEUE.lock().unwrap().entries.iter().find(|e|e.view.lock().unwrap().id==*id).cloned();let Some(entry)=entry else{return;};let view=snapshot(&entry);
 state.save_stage=view.stage;state.save_percent=view.percent;if view.error.is_some(){state.error=view.error;}
 if terminal(&view.phase){state.phase="idle".into();state.output=Some(view.output.unwrap_or(view.original_output));}
}
pub(super) fn cancel(app:&tauri::AppHandle,id:&str)->bool{
 let (entry,waiting)={let mut queue=QUEUE.lock().unwrap();let entry=queue.entries.iter().find(|e|e.view.lock().unwrap().id==id).cloned();let waiting=queue.pending.iter().position(|w|w.entry.view.lock().unwrap().id==id).and_then(|i|queue.pending.remove(i));(entry,waiting)};
 let Some(entry)=entry else{return false;};if terminal(&entry.view.lock().unwrap().phase){return true;}entry.op.cancel.store(true,Ordering::Relaxed);
 if let Some(work)=waiting{let message="Export cancelled. Your original video and project are saved.";if let Err(error)=project::set_status(&work.session,"cancelled",Some(message)){log_error(&error);}complete(app,&entry,"cancelled",Some(message.into()),None);}else{publish(app,&entry);}
 true
}
pub(super) fn shutdown(app:&tauri::AppHandle){for view in views(){if !terminal(&view.phase){cancel(app,&view.id);}}let start=Instant::now();while QUEUE.lock().unwrap().running&&start.elapsed()<Duration::from_secs(10){thread::sleep(Duration::from_millis(30));}let entries=QUEUE.lock().unwrap().entries.iter().cloned().collect::<Vec<_>>();for entry in entries{entry.control.terminate_children();}}
