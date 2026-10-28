//! Signed GitHub updates. Installation is explicit and blocked by active work.
use serde::Serialize;
use std::sync::{Mutex,atomic::{AtomicBool,AtomicUsize,Ordering}};
use tauri::Emitter;
use tauri_plugin_updater::{Update,UpdaterExt};

#[derive(Clone,Default,Serialize)]
pub struct Status {pub phase:String,pub version:Option<String>,pub percent:Option<u8>,pub message:String}
struct Ready {update:Update,bytes:Vec<u8>}
static STATUS:Mutex<Option<Status>>=Mutex::new(None);
static READY:Mutex<Option<Ready>>=Mutex::new(None);
static CHECKING:AtomicBool=AtomicBool::new(false);
static INSTALLING:AtomicBool=AtomicBool::new(false);
static JOBS:AtomicUsize=AtomicUsize::new(0);
static GATE:Mutex<()>=Mutex::new(());
pub struct Job;
impl Drop for Job {fn drop(&mut self){JOBS.fetch_sub(1,Ordering::SeqCst);}}
pub fn job()->Result<Job,String>{let _gate=GATE.lock().unwrap();if INSTALLING.load(Ordering::SeqCst){return Err("Halo is updating. Please wait for it to reopen.".into());}JOBS.fetch_add(1,Ordering::SeqCst);Ok(Job)}
fn publish(app:&tauri::AppHandle,phase:&str,version:Option<String>,percent:Option<u8>,message:&str){let status=Status{phase:phase.into(),version,percent,message:message.into()};*STATUS.lock().unwrap()=Some(status.clone());let _=app.emit("halo://update",status);}
#[tauri::command]pub fn update_status()->Status {STATUS.lock().unwrap().clone().unwrap_or(Status{phase:"idle".into(),message:"Check for a newer version of Halo.".into(),..Default::default()})}
#[tauri::command]pub async fn check_updates(app:tauri::AppHandle)->Result<Status,String>{
 if READY.lock().unwrap().is_some()||CHECKING.swap(true,Ordering::SeqCst){return Ok(update_status());}
 publish(&app,"checking",None,None,"Checking for updates…");
 let result=async {
  let update=app.updater_builder().timeout(std::time::Duration::from_secs(30)).build().map_err(|e|e.to_string())?.check().await.map_err(|e|e.to_string())?;
  let Some(update)=update else{publish(&app,"current",None,None,"You have the latest version of Halo.");return Ok::<_,String>(());};
  let version=update.version.clone();publish(&app,"downloading",Some(version.clone()),None,"Downloading update…");let mut received=0u64;
  let bytes=update.download(|chunk,total|{received+=chunk as u64;let percent=total.filter(|t|*t>0).map(|t|((received.saturating_mul(100)/t).min(100))as u8);publish(&app,"downloading",Some(version.clone()),percent,"Downloading update…");},||{}).await.map_err(|e|e.to_string())?;
  *READY.lock().unwrap()=Some(Ready{update,bytes});publish(&app,"ready",Some(version.clone()),Some(100),"Update ready. Restart Halo when your work is finished.");
  // Tell the user right away, in the same corner card Halo uses for everything else, even with the app open.
  crate::show_notice(app.clone(),serde_json::json!({"kind":"update","version":version}));Ok(())
 }.await;
 CHECKING.store(false,Ordering::SeqCst);
 if let Err(error)=result{eprintln!("Halo update check: {error}");publish(&app,"unavailable",None,None,"Could not check for updates. Try again later.");}
 Ok(update_status())
}
fn can_install(jobs:usize,recording:bool)->bool{jobs==0&&!recording}
#[tauri::command]pub async fn install_update(app:tauri::AppHandle)->Result<(),String>{
 {let _gate=GATE.lock().unwrap();if !can_install(JOBS.load(Ordering::SeqCst),crate::recorder::active()){return Err("Finish your recording, conversion or upload before updating.".into());}if INSTALLING.swap(true,Ordering::SeqCst){return Err("An update is already being installed.".into());}}
 let result=tauri::async_runtime::spawn_blocking(move||{
  let mut ready=READY.lock().unwrap();let pending=ready.as_ref().ok_or("Check for updates first.")?;
  publish(&app,"installing",Some(pending.update.version.clone()),None,"Installing update…");
  match pending.update.install(&pending.bytes){Ok(())=>{ready.take();Ok(())},Err(e)=>{publish(&app,"ready",Some(pending.update.version.clone()),Some(100),"Could not install the update. Please try again.");Err(e.to_string())}}
 }).await.map_err(|e|e.to_string()).and_then(|r|r);
 INSTALLING.store(false,Ordering::SeqCst);result
}
pub fn start(app:tauri::AppHandle){if cfg!(debug_assertions){return;}std::thread::spawn(move||{std::thread::sleep(std::time::Duration::from_secs(15));loop{let handle=app.clone();tauri::async_runtime::block_on(check_updates(handle)).ok();std::thread::sleep(std::time::Duration::from_secs(4*60*60));}});}
#[cfg(test)]mod tests{use super::*;#[test]fn active_work_prevents_install(){assert!(can_install(0,false));assert!(!can_install(1,false));assert!(!can_install(0,true));assert!(!can_install(2,true));}#[test]fn guard_counts_and_releases_jobs(){let before=JOBS.load(Ordering::SeqCst);{let _job=job().unwrap();assert_eq!(JOBS.load(Ordering::SeqCst),before+1);}assert_eq!(JOBS.load(Ordering::SeqCst),before);}}
