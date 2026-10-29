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
static AUTO_ARMED:AtomicBool=AtomicBool::new(false);
pub struct Job;
impl Drop for Job {fn drop(&mut self){JOBS.fetch_sub(1,Ordering::SeqCst);}}
pub fn job()->Result<Job,String>{let _gate=GATE.lock().unwrap();if INSTALLING.load(Ordering::SeqCst){return Err("Halo is updating. Please wait for it to reopen.".into());}JOBS.fetch_add(1,Ordering::SeqCst);Ok(Job)}
fn publish(app:&tauri::AppHandle,phase:&str,version:Option<String>,percent:Option<u8>,message:&str){let status=Status{phase:phase.into(),version,percent,message:message.into()};*STATUS.lock().unwrap()=Some(status.clone());let _=app.emit("halo://update",status);}
#[tauri::command]pub fn update_status()->Status {STATUS.lock().unwrap().clone().unwrap_or(Status{phase:"idle".into(),message:"Updates are checked and installed automatically.".into(),..Default::default()})}
#[tauri::command]pub async fn check_updates(app:tauri::AppHandle)->Result<Status,String>{check_for_updates(app,true).await}
async fn check_for_updates(app:tauri::AppHandle,refresh:bool)->Result<Status,String>{
 {let _gate=GATE.lock().unwrap();
  if INSTALLING.load(Ordering::SeqCst)||CHECKING.swap(true,Ordering::SeqCst){return Ok(update_status());}
  if !refresh&&READY.lock().unwrap().is_some(){CHECKING.store(false,Ordering::SeqCst);return Ok(update_status());}
 }
 publish(&app,"checking",None,None,"Checking for updates…");
 let result=async {
  let update=app.updater_builder().timeout(std::time::Duration::from_secs(30)).build().map_err(|e|e.to_string())?.check().await.map_err(|e|e.to_string())?;
  let Some(update)=update else{*READY.lock().unwrap()=None;publish(&app,"current",None,None,"You have the latest version of Halo. Automatic updates are on.");return Ok::<_,String>(());};
  let version=update.version.clone();publish(&app,"downloading",Some(version.clone()),None,"Downloading update…");let mut received=0u64;
  let bytes=update.download(|chunk,total|{received+=chunk as u64;let percent=total.filter(|t|*t>0).map(|t|((received.saturating_mul(100)/t).min(100))as u8);publish(&app,"downloading",Some(version.clone()),percent,"Downloading update…");},||{}).await.map_err(|e|e.to_string())?;
  *READY.lock().unwrap()=Some(Ready{update,bytes});publish(&app,"ready",Some(version.clone()),Some(100),"Update ready. Halo will update and restart automatically when your work is finished.");
  // Tell the user right away, in the same corner card Halo uses for everything else, even with the app open.
  crate::show_notice(app.clone(),serde_json::json!({"kind":"update","version":version}));
  arm_auto_install(app.clone());
  Ok(())
 }.await;
 CHECKING.store(false,Ordering::SeqCst);
 if let Err(error)=result{eprintln!("Halo update check: {error}");publish(&app,"unavailable",None,None,&format!("Could not check for updates: {error}. Halo will retry automatically."));}
 Ok(update_status())
}
fn can_install(jobs:usize,recording:bool)->bool{jobs==0&&!recording}
#[tauri::command]pub async fn install_update(app:tauri::AppHandle)->Result<(),String>{
 {let _gate=GATE.lock().unwrap();if CHECKING.load(Ordering::SeqCst){return Err("Wait for the update check to finish.".into());}if !can_install(JOBS.load(Ordering::SeqCst),crate::recorder::active()){return Err("Finish your recording, conversion or upload before updating.".into());}if INSTALLING.swap(true,Ordering::SeqCst){return Err("An update is already being installed.".into());}}
 let result=tauri::async_runtime::spawn_blocking(move||{
  let mut ready=READY.lock().unwrap();let pending=ready.as_ref().ok_or("Check for updates first.")?;
  publish(&app,"installing",Some(pending.update.version.clone()),None,"Installing update…");
  match pending.update.install(&pending.bytes){Ok(())=>{ready.take();Ok(())},Err(e)=>{let message=format!("Could not install the update: {e}. Halo will retry automatically.");eprintln!("Halo update installation: {e}");publish(&app,"ready",Some(pending.update.version.clone()),Some(100),&message);Err(message)}}
 }).await.map_err(|e|e.to_string()).and_then(|r|r);
 INSTALLING.store(false,Ordering::SeqCst);result
}
/// Installs the downloaded update as soon as nothing would be interrupted (no running job, no
/// active recording) — so updating never needs a click in Settings. One watcher runs at a time.
fn arm_auto_install(app:tauri::AppHandle){
 if AUTO_ARMED.swap(true,Ordering::SeqCst){return;}
 std::thread::spawn(move||{
  let mut retry_at=std::time::Instant::now();let mut failures=0u32;
  loop{
   std::thread::sleep(std::time::Duration::from_secs(10));
   if READY.lock().unwrap().is_none(){break;}
   if INSTALLING.load(Ordering::SeqCst)||CHECKING.load(Ordering::SeqCst)||std::time::Instant::now()<retry_at{continue;}
   let safe={let _gate=GATE.lock().unwrap();can_install(JOBS.load(Ordering::SeqCst),crate::recorder::active())};
   if safe{
    if tauri::async_runtime::block_on(install_update(app.clone())).is_ok(){break;}
    failures=failures.saturating_add(1);
    retry_at=std::time::Instant::now()+std::time::Duration::from_secs((60u64.saturating_mul(1u64<<failures.min(3).saturating_sub(1))).min(300));
   }
  }
  AUTO_ARMED.store(false,Ordering::SeqCst);
 });
}
pub fn start(app:tauri::AppHandle){std::thread::spawn(move||{
 std::thread::sleep(std::time::Duration::from_secs(15));
 loop{
  let status=tauri::async_runtime::block_on(check_for_updates(app.clone(),false));
  let seconds=if status.as_ref().map(|s|s.phase=="unavailable").unwrap_or(true){5*60}else{4*60*60};
  std::thread::sleep(std::time::Duration::from_secs(seconds));
 }
});}
#[cfg(test)]mod tests{use super::*;#[test]fn active_work_prevents_install(){assert!(can_install(0,false));assert!(!can_install(1,false));assert!(!can_install(0,true));assert!(!can_install(2,true));}#[test]fn guard_counts_and_releases_jobs(){let before=JOBS.load(Ordering::SeqCst);{let _job=job().unwrap();assert_eq!(JOBS.load(Ordering::SeqCst),before+1);}assert_eq!(JOBS.load(Ordering::SeqCst),before);}}
