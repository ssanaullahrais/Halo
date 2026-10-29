//! Independent native screen recording via FFmpeg's Windows Graphics Capture input.
use std::{fs,io::{BufRead,BufReader,Write},os::windows::process::CommandExt,path::{Path,PathBuf},process::{Child,Stdio},sync::{Arc,Mutex,atomic::{AtomicBool,AtomicU32,Ordering}},thread,time::{Duration,Instant}};
use base64::Engine;
use serde::{Deserialize,Serialize};
use tauri::{Emitter,Manager,PhysicalPosition,PhysicalSize};
use windows::core::BOOL;
use windows::Win32::{Foundation::{HWND,POINT,RECT},Graphics::Gdi::{MonitorFromPoint,MONITOR_DEFAULTTONEAREST,ClientToScreen},Storage::Xps::{PrintWindow,PRINT_WINDOW_FLAGS},System::Threading::{GetCurrentProcessId,GetCurrentThreadId,AttachThreadInput},UI::{Input::KeyboardAndMouse::GetAsyncKeyState,WindowsAndMessaging::*}};
use crate::recorder_audio::AudioTrack;
const HIDDEN:u32=0x0800_0000;
const CLICK_SLOTS:usize=4;
#[cfg(test)]#[path="recorder_media_tests.rs"]mod media_tests;
type Res<T>=Result<T,String>;

#[derive(Clone,Serialize,Deserialize)]
#[serde(default)]
pub struct Config {pub shortcut:String,pub output_dir:String,pub fps:u32,pub quality:String,pub resolution:String,pub encoder:String,pub microphone:bool,pub system_audio:bool,pub microphone_id:String,pub mic_gain:f64,pub system_gain:f64,pub voice_enhance:bool,pub cursor:String,pub cursor_size:u32,pub cursor_color:String,pub click_effect:bool,pub cursor_effect:String,pub click_style:String,pub effect_strength:f64,pub frame:String,pub background:String,pub frame_padding:u32,pub countdown:u32}
impl Default for Config {fn default()->Self{Self{shortcut:"Ctrl+Alt+R".into(),output_dir:std::env::var("USERPROFILE").map(|p|PathBuf::from(p).join("Videos").join("Halo").to_string_lossy().into_owned()).unwrap_or_else(|_|".".into()),fps:60,quality:"high".into(),resolution:"original".into(),encoder:"auto".into(),microphone:true,system_audio:false,microphone_id:String::new(),mic_gain:1.,system_gain:1.,voice_enhance:true,cursor:"normal".into(),cursor_size:unsafe{GetSystemMetrics(SM_CXCURSOR).clamp(16,96)as u32},cursor_color:"#FFFFFF".into(),click_effect:false,cursor_effect:"none".into(),click_style:"moderate".into(),effect_strength:0.65,frame:"none".into(),background:"preset:aurora".into(),frame_padding:5,countdown:3}}}
fn config_path()->PathBuf {crate::system::tools_dir().parent().unwrap().join("recorder.json")}
pub fn config()->Config{let mut c:Config=fs::read(config_path()).ok().and_then(|d|serde_json::from_slice(&d).ok()).unwrap_or_default();if !(2..=30).contains(&c.frame_padding){c.frame_padding=Config::default().frame_padding;}if !["weak","moderate","strong"].contains(&c.click_style.as_str()){c.click_style=Config::default().click_style;}c}
fn validate(c:&Config)->Res<()> {
 if ![30,60].contains(&c.fps)||!["high","balanced","archive"].contains(&c.quality.as_str())||!["original","1080p","720p"].contains(&c.resolution.as_str())||!["auto","cpu"].contains(&c.encoder.as_str())||!["normal","hidden","highlight","large"].contains(&c.cursor.as_str())||!["none","mac","windows"].contains(&c.frame.as_str())||!(16..=96).contains(&c.cursor_size)||!(2..=30).contains(&c.frame_padding)||c.countdown>10||!["none","spotlight","trail"].contains(&c.cursor_effect.as_str())||!["weak","moderate","strong"].contains(&c.click_style.as_str())||!c.effect_strength.is_finite()||!(0.1..=1.).contains(&c.effect_strength)||![c.mic_gain,c.system_gain].iter().all(|v|v.is_finite()&&(0.0..=2.0).contains(v)){return Err("One of the recording preferences is outside its supported range.".into());}
 let hex=|color:&str|color.len()==7&&color.starts_with('#')&&color[1..].chars().all(|v|v.is_ascii_hexdigit());
 if !hex(&c.cursor_color)||!(hex(&c.background)||background_preset(c).is_some()){return Err("Choose a valid RGB colour.".into());}
 if !Path::new(&c.output_dir).is_absolute(){return Err("Choose an absolute save folder.".into());}Ok(())
}
#[derive(Clone,Copy,Serialize,Deserialize)]pub struct Rect {pub x:i32,pub y:i32,pub w:u32,pub h:u32}
#[derive(Clone,Serialize,Deserialize)]pub struct Target {pub mode:String,pub monitor:u64,pub window:u64,pub rect:Option<Rect>}
#[derive(Clone,Serialize)]pub struct Monitor {pub id:u64,pub name:String,pub x:i32,pub y:i32,pub w:u32,pub h:u32,pub scale:f64}
#[derive(Serialize)]pub struct Window {pub id:u64,pub name:String,pub w:u32,pub h:u32,pub minimized:bool}
#[derive(Clone,Serialize,Default)]pub struct State {pub phase:String,pub elapsed:f64,pub output:Option<String>,pub original_output:Option<String>,pub error:Option<String>,pub mic_peak:f32,pub system_peak:f32,pub preview_active:bool,pub save_percent:f64,pub target:Option<Rect>,pub microphone:Option<bool>,pub system_audio:Option<bool>}
static STATE:Mutex<Option<State>>=Mutex::new(None);
static SESSION:Mutex<Option<Session>>=Mutex::new(None);
static OP:Mutex<()>=Mutex::new(());
static CANCEL:AtomicBool=AtomicBool::new(false);
static SAVE_PROGRESS:AtomicU32=AtomicU32::new(0);
fn save_progress(value:f64){SAVE_PROGRESS.store((value.clamp(0.,100.)*100.)as u32,Ordering::Relaxed);}
static PREVIEW:Mutex<Option<AudioPreview>>=Mutex::new(None);
struct AudioPreview {stop:Arc<AtomicBool>,tracks:Vec<AudioTrack>,mic:Arc<AtomicU32>,system:Arc<AtomicU32>}
fn stop_preview(){if let Some(p)=PREVIEW.lock().unwrap().take(){p.stop.store(true,Ordering::Relaxed);for track in p.tracks{let _=track.join.join();}}}
#[tauri::command]pub async fn recorder_preview_stop()->Res<()> {tauri::async_runtime::spawn_blocking(||{let _op=OP.lock().unwrap();stop_preview();}).await.map_err(|e|e.to_string())?;Ok(())}
#[tauri::command]pub async fn recorder_preview_start(config:Config)->Res<()> {tauri::async_runtime::spawn_blocking(move||{let _op=OP.lock().unwrap();stop_preview();if SESSION.lock().unwrap().is_some(){return Ok(());}let stop=Arc::new(AtomicBool::new(false));let mic=Arc::new(AtomicU32::new(0));let system=Arc::new(AtomicU32::new(0));let mut tracks=vec![];for (enabled,is_mic) in [(config.microphone,true),(config.system_audio,false)]{if enabled{match crate::recorder_audio::meter(is_mic,config.microphone_id.clone(),stop.clone(),if is_mic{mic.clone()}else{system.clone()}){Ok(track)=>tracks.push(track),Err(error)=>{stop.store(true,Ordering::Relaxed);for track in tracks{let _=track.join.join();}return Err(error);}}}}*PREVIEW.lock().unwrap()=Some(AudioPreview{stop,tracks,mic,system});Ok(())}).await.map_err(|e|e.to_string())?}

static SELECTOR:Mutex<Option<Selection>>=Mutex::new(None);
struct Selection {monitor:Monitor,data:String}
struct Running {child:Child,stop:Arc<AtomicBool>,gate:Arc<Mutex<Option<Instant>>>,audio:Vec<AudioTrack>,pointer:Option<thread::JoinHandle<()>>,raw:PathBuf,commands:PathBuf,mic_failed:bool,system_failed:bool}
struct Segment {raw:PathBuf,audio:Vec<(PathBuf,bool)>,commands:PathBuf,duration:f64}
struct Session {config:Config,target:Target,bounds:Rect,dir:PathBuf,output:PathBuf,original_output:Option<PathBuf>,segments:Vec<Segment>,running:Option<Running>,gpu:bool,mic_peak:Arc<AtomicU32>,sys_peak:Arc<AtomicU32>,mic_muted:Arc<AtomicBool>,sys_muted:Arc<AtomicBool>,annotations:Vec<crate::recorder_annotations::Annotation>}
fn phase(value:&str,error:Option<String>){let mut state=STATE.lock().unwrap();let s=state.get_or_insert_with(State::default);s.phase=value.into();s.error=error;}
fn ffmpeg()->std::process::Command{let mut c=std::process::Command::new(crate::system::ffmpeg_path());c.args(["-hide_banner","-loglevel","error","-y"]);c.creation_flags(HIDDEN);c}
fn run(command:std::process::Command)->Res<()> {run_command(command,None,None)}
fn run_progress(mut command:std::process::Command,duration:f64,from:f64,to:f64)->Res<()> {command.args(["-progress","pipe:1","-stats_period","0.1"]);run_command(command,None,Some((duration.max(0.1),from,to)))}
fn run_command(mut command:std::process::Command,timeout:Option<Duration>,progress:Option<(f64,f64,f64)>)->Res<()> {
 let started=Instant::now();if CANCEL.load(Ordering::Relaxed){return Err("Recording cancelled.".into());}
 let mut child=command.stdin(Stdio::null()).stdout(if progress.is_some(){Stdio::piped()}else{Stdio::null()}).stderr(Stdio::piped()).creation_flags(HIDDEN).spawn().map_err(|e|e.to_string())?;
 let progress_reader=progress.map(|(duration,from,to)|{let stdout=child.stdout.take().unwrap();thread::spawn(move||{for line in BufReader::new(stdout).lines().map_while(Result::ok){if let Some(value)=line.strip_prefix("out_time_us="){if let Ok(time)=value.parse::<f64>(){save_progress(from+(to-from)*(time/1_000_000./duration).clamp(0.,1.));}}}})});
 let mut stderr=child.stderr.take().unwrap();let reader=thread::spawn(move||{use std::io::Read;let mut bytes=Vec::new();let _=stderr.read_to_end(&mut bytes);bytes});
 let result=loop{if timeout.map(|limit|started.elapsed()>limit).unwrap_or(false){let _=child.kill();let _=child.wait();break Err("The window did not provide a capture frame in time.".into());}if CANCEL.load(Ordering::Relaxed){let _=child.kill();let _=child.wait();break Err("Recording cancelled.".into());}match child.try_wait(){Ok(Some(status))=>break Ok(status),Ok(None)=>thread::sleep(Duration::from_millis(30)),Err(error)=>{let _=child.kill();let _=child.wait();break Err(error.to_string());}}};
 let error=reader.join().unwrap_or_default();if let Some(reader)=progress_reader{let _=reader.join();}let status=result?;if !status.success(){return Err(String::from_utf8_lossy(&error).chars().take(1000).collect());}if let Some((_,_,to))=progress{save_progress(to);}Ok(())
}

fn escaped(path:&Path)->String {path.to_string_lossy().replace('\\',"/").replace(':',"\\:").replace('\'',"\\'")}
pub fn monitors(app:&tauri::AppHandle)->Res<Vec<Monitor>> {
 app.available_monitors().map_err(|e|e.to_string()).map(|list|list.into_iter().enumerate().map(|(i,m)|{let p=m.position();let s=m.size();let id=unsafe{MonitorFromPoint(POINT{x:p.x+s.width as i32/2,y:p.y+s.height as i32/2},MONITOR_DEFAULTTONEAREST).0 as u64};Monitor{id,name:m.name().cloned().unwrap_or_else(||format!("Display {}",i+1)),x:p.x,y:p.y,w:s.width,h:s.height,scale:m.scale_factor()}}).collect())
}
fn window_rect(id:u64)->Res<Rect>{unsafe{let h=HWND(id as usize as *mut _);if !IsWindow(Some(h)).as_bool(){return Err("That window has closed. Choose a window again.".into());}let mut r=RECT::default();GetClientRect(h,&mut r).map_err(|e|e.to_string())?;let mut p=POINT{x:0,y:0};let _=ClientToScreen(h,&mut p);Ok(Rect{x:p.x,y:p.y,w:(r.right-r.left).max(0) as u32,h:(r.bottom-r.top).max(0) as u32})}}
// FFmpeg's gfxcapture (Windows Graphics Capture) input can hang indefinitely opening a
// capture session for a window that isn't the foreground window, so we bring it forward
// first. A plain SetForegroundWindow often gets ignored by Windows' focus-stealing
// prevention, so attach our input queue to the current foreground thread first, which is
// the standard workaround.
fn activate_window(h:HWND){
 unsafe{
  if GetForegroundWindow()==h{return;}
  let foreground=GetForegroundWindow();
  let current=GetCurrentThreadId();
  let foreground_thread=GetWindowThreadProcessId(foreground,None);
  let attached=foreground_thread!=0&&foreground_thread!=current&&AttachThreadInput(current,foreground_thread,true).as_bool();
  let _=BringWindowToTop(h);
  let _=SetForegroundWindow(h);
  if attached{let _=AttachThreadInput(current,foreground_thread,false);}
 }
}
fn bounds(app:&tauri::AppHandle,t:&Target)->Res<Rect>{
 if t.mode=="window"{let h=HWND(t.window as usize as *mut _);if unsafe{IsIconic(h)}.as_bool(){return Err("Restore the selected window before recording.".into());}activate_window(h);return window_rect(t.window);}
 let m=monitors(app)?.into_iter().find(|m|m.id==t.monitor).ok_or("Choose a connected display.")?;
 if t.mode=="area" {let r=t.rect.ok_or("Select an area first.")?;if r.x<0||r.y<0||r.w<2||r.h<2||r.x as u64+r.w as u64>m.w as u64||r.y as u64+r.h as u64>m.h as u64{return Err("The selected area is outside the display.".into());}Ok(Rect{x:m.x+r.x,y:m.y+r.y,w:r.w,h:r.h})}else if t.mode=="screen"{Ok(Rect{x:m.x,y:m.y,w:m.w,h:m.h})}else{Err("Choose Screen, Window or Area.".into())}
}
unsafe extern "system" fn enum_window(h:HWND,param:windows::Win32::Foundation::LPARAM)->BOOL {
 let list=unsafe{&mut *(param.0 as *mut Vec<Window>)};
 if !unsafe{IsWindowVisible(h)}.as_bool()||h==unsafe{GetShellWindow()}{return BOOL(1);}let style=unsafe{GetWindowLongPtrW(h,GWL_EXSTYLE)}as u32;if style&(WS_EX_TOOLWINDOW.0|WS_EX_NOACTIVATE.0)!=0{return BOOL(1);}let mut pid=0;unsafe{GetWindowThreadProcessId(h,Some(&mut pid));}if pid==unsafe{GetCurrentProcessId()}{let mut title=[0u16;128];let n=unsafe{GetWindowTextW(h,&mut title)};if !cfg!(debug_assertions)||String::from_utf16_lossy(&title[..n.max(0) as usize])!="Halo Recorder Test"{return BOOL(1);}}
 let mut text=[0u16;512];let n=unsafe{GetWindowTextW(h,&mut text)};if n>0 {if let Ok(r)=window_rect(h.0 as u64){if r.w>1&&r.h>1{list.push(Window{id:h.0 as u64,name:String::from_utf16_lossy(&text[..n as usize]),w:r.w,h:r.h,minimized:unsafe{IsIconic(h)}.as_bool()});}}}BOOL(1)
}
fn exclude(window:&tauri::WebviewWindow){if let Ok(hwnd)=window.hwnd(){unsafe{let _=SetWindowDisplayAffinity(HWND(hwnd.0),WDA_EXCLUDEFROMCAPTURE);}}}
pub fn open(app:&tauri::AppHandle){let app=app.clone();thread::spawn(move||{let _=countdown_window(&app);if let Some(w)=app.get_webview_window("recorder"){let _=w.show();let _=w.set_focus();let _=app.emit_to("recorder","halo://recorder-opened",());return;}if let Ok(w)=tauri::WebviewWindowBuilder::new(&app,"recorder",tauri::WebviewUrl::App("recorder.html".into())).title("Halo · Screen Recording").inner_size(615.,84.).visible(false).transparent(true).decorations(false).always_on_top(true).skip_taskbar(true).resizable(false).shadow(false).build(){exclude(&w);{let late=w.clone();thread::spawn(move||{thread::sleep(Duration::from_millis(3000));let _=late.show();});}if let Ok(Some(m))=app.primary_monitor(){let scale=m.scale_factor();let p=m.position();let _=w.set_position(PhysicalPosition::new(p.x+(m.size().width as i32-(615.*scale) as i32)/2,p.y+(24.*scale) as i32));}}});}
#[tauri::command]pub async fn open_recorder(app:tauri::AppHandle){open(&app);}
#[tauri::command]pub async fn recorder_options(app:tauri::AppHandle)->Res<serde_json::Value>{let displays=monitors(&app)?;tauri::async_runtime::spawn_blocking(move||{let mut windows=Vec::<Window>::new();unsafe{let _=EnumWindows(Some(enum_window),windows::Win32::Foundation::LPARAM(&mut windows as *mut _ as isize));}Ok(serde_json::json!({"config":config(),"native_cursor_size":unsafe{GetSystemMetrics(SM_CXCURSOR).clamp(16,96)},"monitors":displays,"windows":windows,"microphones":crate::recorder_audio::devices()}))}).await.map_err(|e|e.to_string())?}
#[tauri::command]pub async fn recorder_save_config(app:tauri::AppHandle,config:Config)->Res<()> {
 validate(&config)?;use tauri_plugin_global_shortcut::GlobalShortcutExt;let old=self::config();if old.shortcut!=config.shortcut {let wanted:tauri_plugin_global_shortcut::Shortcut=config.shortcut.parse().map_err(|_|"Choose a valid recorder shortcut.")?;app.global_shortcut().register(wanted).map_err(|_|"That recorder shortcut is already in use.")?;let _=app.global_shortcut().unregister(old.shortcut.as_str());}
 let path=config_path();fs::create_dir_all(path.parent().unwrap()).map_err(|e|e.to_string())?;fs::write(path,serde_json::to_vec_pretty(&config).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;let _=app.emit("halo://recorder-config",&config);Ok(())
}
#[tauri::command]pub async fn recorder_pick_folder(app:tauri::AppHandle)->Option<String>{use tauri_plugin_dialog::DialogExt;tauri::async_runtime::spawn_blocking(move||app.dialog().file().blocking_pick_folder().and_then(|p|p.into_path().ok()).map(|p|p.to_string_lossy().into_owned())).await.ok().flatten()}
#[tauri::command]pub fn recorder_state()->State {
 let mut s=STATE.lock().unwrap().clone().unwrap_or_else(||State{phase:"idle".into(),..Default::default()});
 if let Ok(guard)=SESSION.try_lock(){if let Some(job)=guard.as_ref(){s.elapsed=job.segments.iter().map(|s|s.duration).sum();if let Some(r)=&job.running {if let Some(start)=*r.gate.lock().unwrap(){s.elapsed+=start.elapsed().as_secs_f64();}}s.mic_peak=f32::from_bits(job.mic_peak.load(Ordering::Relaxed));s.system_peak=f32::from_bits(job.sys_peak.load(Ordering::Relaxed));s.microphone=Some(job.config.microphone);s.system_audio=Some(job.config.system_audio);}}if s.phase=="saving"{s.save_percent=SAVE_PROGRESS.load(Ordering::Relaxed)as f64/100.;}if s.phase=="idle"{if let Ok(p)=PREVIEW.try_lock(){if let Some(p)=p.as_ref(){s.mic_peak=f32::from_bits(p.mic.load(Ordering::Relaxed));s.system_peak=f32::from_bits(p.system.load(Ordering::Relaxed));s.preview_active=true;}}}s
}
#[tauri::command]pub async fn recorder_select_area(app:tauri::AppHandle,monitor:u64)->Res<()> {
 let m=monitors(&app)?.into_iter().find(|m|m.id==monitor).ok_or("Choose a display.")?;let request=m.clone();
 let data=tauri::async_runtime::spawn_blocking(move||{let _op=OP.lock().unwrap();CANCEL.store(false,Ordering::Relaxed);let dir=crate::system::tools_dir().join("recorder-cache");fs::create_dir_all(&dir).map_err(|e|e.to_string())?;let path=dir.join(format!("selection-{}.png",chrono::Local::now().timestamp_millis()));let mut c=ffmpeg();c.args(["-f","lavfi","-i",&format!("gfxcapture=hmonitor={}:capture_cursor=0",request.id),"-frames:v","1","-vf","hwdownload,format=bgra"]).arg(&path);run(c)?;Ok::<_,String>(format!("data:image/png;base64,{}",base64::engine::general_purpose::STANDARD.encode(fs::read(path).map_err(|e|e.to_string())?)))}).await.map_err(|e|e.to_string())??;
 if CANCEL.load(Ordering::Relaxed){return Err("Selection cancelled.".into());}
 *SELECTOR.lock().unwrap()=Some(Selection{monitor:m.clone(),data});if let Some(w)=app.get_webview_window("recorder-selector"){let _=w.destroy();}
 let w=tauri::WebviewWindowBuilder::new(&app,"recorder-selector",tauri::WebviewUrl::App("recorder-select.html".into())).title("Halo · Select recording area").decorations(false).transparent(true).always_on_top(true).skip_taskbar(true).resizable(false).shadow(false).build().map_err(|e|e.to_string())?;
 w.set_size(PhysicalSize::new(m.w,m.h)).map_err(|e|e.to_string())?;w.set_position(PhysicalPosition::new(m.x,m.y)).map_err(|e|e.to_string())?;exclude(&w);let _=w.set_focus();Ok(())
}
#[tauri::command]pub fn recorder_selection_data()->Res<serde_json::Value>{let s=SELECTOR.lock().unwrap();let s=s.as_ref().ok_or("No display was selected.")?;Ok(serde_json::json!({"monitor":s.monitor,"data":s.data}))}
pub fn cancel_area(app:&tauri::AppHandle){
 let active=SELECTOR.lock().unwrap().take().is_some();
 if let Some(w)=app.get_webview_window("recorder-selector"){let _=w.destroy();}
 if active{let _=app.emit("halo://recording-area-cancelled",());}
}
#[tauri::command]pub async fn recorder_cancel_area(app:tauri::AppHandle){cancel_area(&app);}
#[tauri::command]pub async fn recorder_commit_area(app:tauri::AppHandle,rect:Rect)->Res<()> {
 let monitor={let mut selection=SELECTOR.lock().unwrap();let current=selection.as_ref().ok_or("No area selection is active.")?;
 if rect.x<0||rect.y<0||rect.w<2||rect.h<2||rect.x as u64+rect.w as u64>current.monitor.w as u64||rect.y as u64+rect.h as u64>current.monitor.h as u64{return Err("Choose an area inside the display.".into());}
 let monitor=current.monitor.id;selection.take();monitor};
 if let Some(w)=app.get_webview_window("recorder-selector"){w.destroy().map_err(|e|e.to_string())?;}
 app.emit("halo://recording-area",serde_json::json!({"monitor":monitor,"rect":rect})).map_err(|e|e.to_string())?;Ok(())
}


fn video_settings(command:&mut std::process::Command,c:&Config,gpu:bool){let q=match c.quality.as_str(){"archive"=>12,"balanced"=>23,_=>16};if gpu{command.args(["-c:v","h264_nvenc","-preset","p5","-rc","vbr","-cq",&q.to_string(),"-b:v","0"]);}else{command.args(["-c:v","libx264","-preset","medium","-crf",&q.to_string()]);}command.args(["-pix_fmt","yuv420p"]);}
// Capture and working segments retain full chroma and avoid cumulative compression.
// Only the delivery MP4 is encoded with the user's quality/compatibility settings.
fn intermediate_video_settings(command:&mut std::process::Command,gpu:bool){
 if gpu{command.args(["-c:v","h264_nvenc","-preset","p1","-tune","lossless","-pix_fmt","yuv444p"]);}else{command.args(["-c:v","libx264rgb","-preset","ultrafast","-crf","0","-pix_fmt","rgb24"]);}
}
fn capture_video_settings(command:&mut std::process::Command,gpu:bool){
 intermediate_video_settings(command,gpu);
 // Keep live encoding from queuing old pictures behind the source clock.
 if gpu{command.args(["-zerolatency","1","-delay","0","-bf","0"]);}else{command.args(["-tune","zerolatency"]);}
}
fn gpu_available(dir:&Path,width:u32,height:u32,fps:u32)->bool {let mut c=ffmpeg();c.args(["-f","lavfi","-i",&format!("color=size={}x{}:rate={fps}",(width+1)/2*2,(height+1)/2*2),"-frames:v","1"]);capture_video_settings(&mut c,true);c.arg(dir.join("encoder-probe.mp4"));run(c).is_ok()}
fn start_segment(job:&Session)->Res<Running>{
 if CANCEL.load(Ordering::Relaxed){return Err("Recording cancelled.".into());}
 let i=job.segments.len();let raw=job.dir.join(format!("segment-{i}.mp4"));let commands=job.dir.join(format!("cursor-{i}.txt"));let gate=Arc::new(Mutex::new(None::<Instant>));let stop=Arc::new(AtomicBool::new(false));let mut audio=Vec::new();
 // A failed audio device (disconnected mic, no capture device on this machine, etc.) shouldn't
 // block video recording entirely; skip that one track, keep going without it, and report it so
 // the UI can turn the toggle off instead of showing it as enabled while silently not recording.
 let (mut mic_failed,mut system_failed)=(false,false);
 for (enabled,mic) in [(job.config.microphone,true),(job.config.system_audio,false)] {if enabled {let path=job.dir.join(format!("{}-{i}.wav",if mic{"mic"}else{"system"}));match crate::recorder_audio::start(path,mic,job.config.microphone_id.clone(),stop.clone(),gate.clone(),if mic{job.mic_peak.clone()}else{job.sys_peak.clone()},if mic{job.mic_muted.clone()}else{job.sys_muted.clone()}){Ok(track)=>audio.push(track),Err(e)=>{log_error(&format!("{} capture unavailable, recording without it: {e}",if mic{"Microphone"}else{"System audio"}));if mic{mic_failed=true}else{system_failed=true}}}}}
 let native=job.config.frame!="mac"&&(job.config.cursor=="normal"||job.config.cursor=="highlight");let target=&job.target;
 let mut source=if target.mode=="window"{format!("gfxcapture=hwnd={}:width={}:height={}:resize_mode=crop",target.window,job.bounds.w,job.bounds.h)}else{format!("gfxcapture=hmonitor={}",target.monitor)};
 if target.mode=="area" {let r=target.rect.unwrap();let monitors=unsafe{MonitorFromPoint(POINT{x:job.bounds.x,y:job.bounds.y},MONITOR_DEFAULTTONEAREST)};let mut info=windows::Win32::Graphics::Gdi::MONITORINFO{cbSize:std::mem::size_of::<windows::Win32::Graphics::Gdi::MONITORINFO>() as u32,..Default::default()};unsafe{let _=windows::Win32::Graphics::Gdi::GetMonitorInfoW(monitors,&mut info);}let w=(info.rcMonitor.right-info.rcMonitor.left) as u32;let h=(info.rcMonitor.bottom-info.rcMonitor.top) as u32;source+=&format!(":crop_left={}:crop_top={}:crop_right={}:crop_bottom={}",r.x,r.y,w-r.x as u32-r.w,h-r.y as u32-r.h);}
 // Acquire above the output rate: a 60Hz minimum capture interval on a
 // 144Hz desktop otherwise supplies about 48 unique pictures per second.
 // Sample to the requested rate before downloading or encoding pixels.
 source+=&format!(":capture_cursor={}:max_framerate={}",u8::from(native),job.config.fps*4);
 if CANCEL.load(Ordering::Relaxed){stop.store(true,Ordering::Relaxed);for track in audio{let _=track.join.join();}return Err("Recording cancelled.".into());}
 // Start the shared clock at the first source picture, before resampling or
 // either CPU or GPU encoding can queue it. Reset PTS to that same picture.
 let mut c=ffmpeg();c.args(["-loglevel","info","-nostats","-f","lavfi","-i",&source,"-vf",&format!("setpts=PTS-STARTPTS,showinfo=checksum=0,fps={}:eof_action=pass,hwdownload,format=bgra,pad=ceil(iw/2)*2:ceil(ih/2)*2:0:0:black",job.config.fps),"-an","-r",&job.config.fps.to_string()]);capture_video_settings(&mut c,job.gpu);c.args(["-movflags","+frag_keyframe+empty_moov+default_base_moof"]).arg(&raw).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped());
 let mut child=match c.creation_flags(HIDDEN).spawn(){Ok(child)=>child,Err(e)=>{stop.store(true,Ordering::Relaxed);for track in audio{let _=track.join.join();}return Err(e.to_string());}};let stderr=child.stderr.take().unwrap();let signal=gate.clone();let diagnostic=job.dir.join(format!("capture-{i}.log"));
 thread::spawn(move||{let mut log=fs::File::create(diagnostic).ok();for line in BufReader::new(stderr).lines().map_while(Result::ok){
  if line.contains("showinfo"){
   if line.contains(" n:"){let mut g=signal.lock().unwrap();if g.is_none(){*g=Some(Instant::now());if let Some(log)=log.as_mut(){let _=writeln!(log,"{line}");}}}
  }else if let Some(log)=log.as_mut(){let _=writeln!(log,"{line}");}
 }});
 let start=Instant::now();while gate.lock().unwrap().is_none(){if CANCEL.load(Ordering::Relaxed)||child.try_wait().map_err(|e|e.to_string())?.is_some()||start.elapsed()>Duration::from_secs(12){stop.store(true,Ordering::Relaxed);let _=child.kill();let _=child.wait();for t in audio{let _=t.join.join();}return Err("The capture source did not produce video. Check the selected window and graphics driver.".into());}thread::sleep(Duration::from_millis(30));}
 let pointer=if job.config.cursor=="highlight"||job.config.cursor=="large"||(job.config.frame=="mac"&&job.config.cursor!="hidden")||job.config.click_effect||job.config.cursor_effect!="none" {
 let stopped=stop.clone();let clock=gate.clone();let target=target.clone();let bounds=job.bounds;let file=commands.clone();let size=job.config.cursor_size;let arrow=job.config.cursor=="large"||(job.config.frame=="mac"&&job.config.cursor=="normal");let clicks=job.config.click_effect;let effect=job.config.cursor_effect.clone();let strength=job.config.effect_strength;let click_size=size*2;let spot_size=(size*6).min(bounds.w.min(bounds.h)).max(2);
 Some(thread::spawn(move||{unsafe{windows::Win32::Media::timeBeginPeriod(1);}let Ok(f)=fs::File::create(file)else{unsafe{windows::Win32::Media::timeEndPeriod(1);}return};let mut f=std::io::BufWriter::new(f);let mut was_down=false;let mut last_write=-1.0f64;let mut click_events=[None::<(f64,i32,i32)>;CLICK_SLOTS];let mut click_slot=0;let mut history=std::collections::VecDeque::<(f64,i32,i32)>::new();
 while !stopped.load(Ordering::Relaxed){let t=clock.lock().unwrap().map(|g|g.elapsed().as_secs_f64()).unwrap_or(0.);let r=if target.mode=="window"{window_rect(target.window).unwrap_or(bounds)}else{bounds};let mut p=POINT::default();let down=unsafe{let _=GetCursorPos(&mut p);(GetAsyncKeyState(0x01) as u16|GetAsyncKeyState(0x02) as u16)&0x8000!=0};let inside=p.x>=r.x&&p.y>=r.y&&p.x<r.x+bounds.w as i32&&p.y<r.y+bounds.h as i32;
 if t-last_write<0.005{thread::sleep(Duration::from_millis(1));continue;}last_write=t;
 let (x,y)=if inside{(p.x-r.x-if arrow{(size as f32*5./64.).round()as i32}else{size as i32/2},p.y-r.y-if arrow{(size as f32*3./64.).round()as i32}else{size as i32/2})}else{(-200,-200)};
 if clicks&&down&&!was_down&&inside{click_events[click_slot]=Some((t,p.x-r.x-click_size as i32/2,p.y-r.y-click_size as i32/2));click_slot=(click_slot+1)%CLICK_SLOTS;}was_down=down;
 let _=writeln!(f,"{t:.3} overlay@pointer x {x};\n{t:.3} overlay@pointer y {y};");
 for (slot,event) in click_events.iter_mut().enumerate(){if let Some((at,cx,cy))=*event{let age=t-at;if age<0.5{let frame=((age/0.5*60.).floor()as i32).clamp(0,60);let _=writeln!(f,"{t:.3} overlay@click{slot} x {cx};\n{t:.3} overlay@click{slot} y {cy};\n{t:.3} colorchannelmixer@clickalpha{slot} aa 1;\n{t:.3} crop@clickframe{slot} x {};",frame*click_size as i32);}else{let _=writeln!(f,"{t:.3} colorchannelmixer@clickalpha{slot} aa 0;");*event=None;}}}
 if effect=="spotlight"{let sx=(p.x-r.x-spot_size as i32/2).clamp(0,(bounds.w-spot_size)as i32);let sy=(p.y-r.y-spot_size as i32/2).clamp(0,(bounds.h-spot_size)as i32);let dim=if inside{-0.16*strength}else{0.};let alpha=if inside{1.}else{0.};let _=writeln!(f,"{t:.3} eq@spotdim brightness {dim:.3};\n{t:.3} crop@spotcrop x {sx};\n{t:.3} crop@spotcrop y {sy};\n{t:.3} overlay@spotlight x {sx};\n{t:.3} overlay@spotlight y {sy};\n{t:.3} colorchannelmixer@spotalpha aa {alpha};");}
 if effect=="trail"{history.push_back((t,p.x-r.x,p.y-r.y));while history.front().map(|v|t-v.0>0.3).unwrap_or(false){history.pop_front();}for (i,delay)in [0.025,0.065,0.12].iter().enumerate(){let past=history.iter().rev().find(|v|v.0<=t-delay).or(history.front());if let Some((_,px,py))=past{let moved=(*px-(p.x-r.x)).abs()+(*py-(p.y-r.y)).abs();let alpha=if inside{(moved as f64/35.).min(1.)*strength*(1.-i as f64*0.25)}else{0.};let tx=px-size as i32/2;let ty=py-size as i32/2;let _=writeln!(f,"{t:.3} overlay@trail{i} x {tx};\n{t:.3} overlay@trail{i} y {ty};\n{t:.3} colorchannelmixer@trailalpha{i} aa {alpha:.3};");}}}
 thread::sleep(Duration::from_millis(2));}
 unsafe{windows::Win32::Media::timeEndPeriod(1);}
 }))}else{None};

 Ok(Running{child,stop,gate,audio,pointer,raw,commands,mic_failed,system_failed})
}
fn finish_segment(mut running:Running)->Res<Segment>{let duration=running.gate.lock().unwrap().map(|t|t.elapsed().as_secs_f64()).unwrap_or(0.).max(0.1);running.stop.store(true,Ordering::Relaxed);if let Some(input)=running.child.stdin.as_mut(){let _=input.write_all(b"q\n");}let start=Instant::now();loop{if running.child.try_wait().map_err(|e|e.to_string())?.is_some(){break;}if start.elapsed()>Duration::from_secs(12){let _=running.child.kill();let _=running.child.wait();break;}thread::sleep(Duration::from_millis(40));}if let Some(p)=running.pointer{let _=p.join();}let mut paths=Vec::new();for track in running.audio {track.join.join().map_err(|_|"An audio capture thread stopped unexpectedly.")??;paths.push((track.path,track.mic));}Ok(Segment{raw:running.raw,audio:paths,commands:running.commands,duration})}
fn render_png(svg:&str,path:&Path)->Res<()> {let mut options=resvg::usvg::Options::default();if svg.contains("<text"){static FONTS:std::sync::OnceLock<Arc<resvg::usvg::fontdb::Database>>=std::sync::OnceLock::new();options.fontdb=FONTS.get_or_init(||{let mut database=resvg::usvg::fontdb::Database::new();database.load_system_fonts();Arc::new(database)}).clone();}let tree=resvg::usvg::Tree::from_str(svg,&options).map_err(|e|e.to_string())?;let size=tree.size().to_int_size();let mut pix=resvg::tiny_skia::Pixmap::new(size.width(),size.height()).ok_or("The recording frame is too large.")?;resvg::render(&tree,resvg::tiny_skia::Transform::identity(),&mut pix.as_mut());pix.save_png(path).map_err(|e|e.to_string())}
fn cursor_svg(c:&Config)->String {
 let mac=c.frame=="mac";let arrow=if mac{"M5 3V52L17 40L27 60L36 56L27 36H45Z"}else{"M5 3V50L17 38L27 59L36 55L27 35H45Z"};
 let original=mac&&c.cursor=="normal";let pointer=|fill:&str|{let path=format!(r##"<path d="{arrow}" fill="{fill}" stroke="{}" stroke-width="2.4" stroke-linejoin="round"/>"##,if mac{"#ffffff"}else{"#14243c"});if original{format!(r#"<g transform="translate(5 3) scale(.62) translate(-5 -3)">{path}</g>"#)}else{path}};
 let graphic=match c.cursor.as_str(){"large"=>pointer(&c.cursor_color),"normal" if mac=>pointer("#15171c"),"highlight"=>format!(r#"<circle cx="32" cy="32" r="29" fill="{}" fill-opacity=".18" stroke="{}" stroke-opacity=".65" stroke-width="1.5"/>{}"#,c.cursor_color,c.cursor_color,if mac{format!("<g transform=\"translate(29.5 30.5) scale(.5)\">{}</g>",pointer("#15171c"))}else{String::new()}),_=>String::new()};
 format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" viewBox="0 0 64 64">{graphic}</svg>"#,c.cursor_size,c.cursor_size)
}
/// A single glowing "orb" click animation: gradient fill, outer ring and bright core, growing
/// then fading out. Visual design adapted from the MIT-licensed CursorFly project
/// (github.com/anugotta/cursorfly-screen-recorder, see its `drawOrbClick`).
fn click_sprite(c:&Config)->String{
 let (size_mul,alpha_mul)=match c.click_style.as_str(){"weak"=>(0.7,0.7),"strong"=>(1.4,1.2),_=>(1.,1.)};
 let size=c.cursor_size*2;let cell=size as f64;let base=cell*0.27*size_mul;let frames=61;
 let mut shapes=String::new();
 for i in 0..frames{
  let t=i as f64/(frames-1) as f64;let eased=1.-(1.-t).powi(3);
  let radius=base*(0.5+eased*0.75);
  let alpha=(if t<0.3{0.95*(1.-(1.-t/0.3).powi(3))}else{0.95*(1.-(t-0.3)/0.7).powi(2)}*alpha_mul).clamp(0.,1.);
  let cx=i as f64*cell+cell/2.;let cy=cell/2.;let core_r=radius*0.3;let color=&c.cursor_color;
  shapes+=&format!(r##"<g opacity="{alpha:.3}"><circle cx="{cx}" cy="{cy}" r="{radius:.2}" fill="url(#orb)"/><circle cx="{cx}" cy="{cy}" r="{radius:.2}" fill="none" stroke="{color}" stroke-width="2.4"/><circle cx="{cx}" cy="{cy}" r="{core_r:.2}" fill="{color}"/></g>"##);
 }
 let color=&c.cursor_color;let width=size*frames;
 format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{size}"><defs><radialGradient id="orb"><stop offset="0%" stop-color="{color}"/><stop offset="40%" stop-color="{color}" stop-opacity=".7"/><stop offset="100%" stop-color="{color}" stop-opacity="0"/></radialGradient></defs>{shapes}</svg>"#)
}
#[tauri::command]pub fn recorder_click_preview(config:Config)->Res<String>{validate(&config)?;Ok(click_sprite(&config))}
#[tauri::command]pub fn recorder_default_config()->Config{Config::default()}
#[tauri::command]pub fn recorder_cursor_preview(config:Config)->Res<String>{validate(&config)?;let mut preview=config.clone();if preview.cursor=="normal"{preview.cursor="large".into();preview.cursor_color="#202633".into();}Ok(cursor_svg(&preview))}
fn finish(job:&Session)->Res<(PathBuf,Option<PathBuf>)>{
 let total_duration=job.segments.iter().map(|s|s.duration).sum::<f64>().max(0.1);let mut completed=Vec::new();let mut segment_offset=0.;let c=&job.config;let size=c.cursor_size;let color=&c.cursor_color;
 let pointer=job.dir.join("pointer.png");let click=job.dir.join("click.png");
 render_png(&cursor_svg(c),&pointer)?;
 render_png(&click_sprite(c),&click)?;
 let spot_size=(size*6).min(job.bounds.w.min(job.bounds.h)).max(2);let mask=job.dir.join("spotlight-mask.png");let trail=job.dir.join("trail.png");
 if c.cursor_effect=="spotlight"{render_png(&format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="{spot_size}" height="{spot_size}"><defs><radialGradient id="soft"><stop offset="65%" stop-color="white"/><stop offset="100%" stop-color="black"/></radialGradient></defs><rect width="100%" height="100%" fill="url(#soft)"/></svg>"#),&mask)?;}
 if c.cursor_effect=="trail"{render_png(&format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="{size}" height="{size}" viewBox="0 0 64 64"><defs><radialGradient id="soft"><stop stop-color="{color}" stop-opacity=".7"/><stop offset="100%" stop-color="{color}" stop-opacity="0"/></radialGradient></defs><circle cx="32" cy="32" r="29" fill="url(#soft)"/></svg>"#),&trail)?;}

 for (i,segment) in job.segments.iter().enumerate(){
  let result=job.dir.join(format!("finished-{i}.mp4"));let mut cmd=ffmpeg();cmd.args(["-filter_complex_threads","1"]);cmd.arg("-i").arg(&segment.raw);let effects=segment.commands.exists();let mut next=1;let mut filters=vec![format!("[0:v]fps={},tpad=stop_mode=clone:stop_duration={}[base]",c.fps,segment.duration)];let mut video="base".to_string();
  if effects {
   cmd.args(["-framerate",&c.fps.to_string(),"-loop","1","-i"]).arg(&pointer);cmd.args(["-framerate",&c.fps.to_string(),"-loop","1","-i"]).arg(&click);next=3;
   let main_commands=job.dir.join(format!("cursor-main-{i}.txt"));let mut main_text=String::new();let mut click_text=(0..CLICK_SLOTS).map(|n|format!("0 crop@clickframe{n} x 0;\n")).collect::<Vec<_>>();
   for line in BufReader::new(fs::File::open(&segment.commands).map_err(|e|e.to_string())?).lines(){let line=line.map_err(|e|e.to_string())?;if let Some(n)=(0..CLICK_SLOTS).find(|n|line.contains(&format!("crop@clickframe{n} "))||line.contains(&format!("colorchannelmixer@clickalpha{n} "))){click_text[n].push_str(&line);click_text[n].push('\n');}else{main_text.push_str(&line);main_text.push('\n');}}
   fs::write(&main_commands,main_text).map_err(|e|e.to_string())?;
   filters.push(format!("[base]sendcmd=f='{}'[commands];[commands][1:v]overlay@pointer=x=-200:y=-200:shortest=1:format=auto[pointer];[2:v]split={CLICK_SLOTS}{}",escaped(&main_commands),(0..CLICK_SLOTS).map(|n|format!("[clicksheet{n}]")).collect::<String>()));
   video="pointer".into();
   for n in 0..CLICK_SLOTS{let timeline=job.dir.join(format!("click-{i}-{n}.txt"));fs::write(&timeline,&click_text[n]).map_err(|e|e.to_string())?;filters.push(format!("[clicksheet{n}]sendcmd=f='{}',crop@clickframe{n}=w={}:h={}:x=0:y=0,format=rgba,colorchannelmixer@clickalpha{n}=aa=0[clickfx{n}];[{video}][clickfx{n}]overlay@click{n}=x=-200:y=-200:shortest=1:format=auto[clicked{n}]",escaped(&timeline),size*2,size*2));video=format!("clicked{n}");}
  }
  if effects&&c.cursor_effect=="spotlight"{cmd.args(["-framerate",&c.fps.to_string(),"-loop","1","-i"]).arg(&mask);filters.push(format!("[{video}]split[spotbase][spotpatch];[spotbase]eq@spotdim=brightness=0[dimmed];[spotpatch]crop@spotcrop=w={spot_size}:h={spot_size}:x=0:y=0,format=rgba[patch];[{next}:v]format=gray[spotmask];[patch][spotmask]alphamerge,colorchannelmixer@spotalpha=aa=0[spot];[dimmed][spot]overlay@spotlight=x=0:y=0:shortest=1:format=auto[spotted]"));video="spotted".into();next+=1;}
  if effects&&c.cursor_effect=="trail"{for n in 0..3{cmd.args(["-framerate",&c.fps.to_string(),"-loop","1","-i"]).arg(&trail);filters.push(format!("[{next}:v]format=rgba,colorchannelmixer@trailalpha{n}=aa=0[trailfx{n}];[{video}][trailfx{n}]overlay@trail{n}=x=-200:y=-200:shortest=1:format=auto[trailed{n}]"));video=format!("trailed{n}");next+=1;}}
  for annotation in &job.annotations{
   let from=(annotation.start-segment_offset).max(0.);let to=(annotation.end.unwrap_or(f64::MAX)-segment_offset).min(segment.duration);if from>=segment.duration||to<=from{continue;}
   let label=format!("ann{}_{}",i,annotation.id);let mut timeline=crate::recorder_annotations::timeline(annotation,segment_offset,segment.duration);let mut sampled=Vec::new();for (time,shape)in timeline.drain(..){let tick=(time*c.fps as f64).round()as u64;if sampled.last().map(|(t,_) : &(f64,crate::recorder_annotations::Annotation)|(*t*c.fps as f64).round()as u64==tick).unwrap_or(false){sampled.pop();}sampled.push((tick as f64/c.fps as f64,shape));}let timeline=sampled;
   let (x,y,w,h)=if annotation.kind=="blur"{let (x,y,w,h)=crate::recorder_annotations::canvas_bounds(annotation);let x=x as u32/2*2;let y=y as u32/2*2;(x as i32,y as i32,w.min(job.bounds.w-x).max(2)/2*2,h.min(job.bounds.h-y).max(2)/2*2)}else{crate::recorder_annotations::canvas_bounds(annotation)};
   let list=job.dir.join(format!("annotation-{}-{i}.ffconcat",annotation.id));let mut entries=String::from("ffconcat version 1.0\n");
   for (index,(time,shape)) in timeline.iter().enumerate(){
    if CANCEL.load(Ordering::Relaxed){return Err("Recording cancelled.".into());}
    let asset=job.dir.join(format!("annotation-{}-{i}-{index}.png",annotation.id));
    let svg=if annotation.kind=="blur"{format!(r##"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="{x} {y} {w} {h}"><rect x="{x}" y="{y}" width="{w}" height="{h}" fill="black"/><rect x="{}" y="{}" width="{}" height="{}" fill="white"/></svg>"##,shape.x.min(shape.x+shape.w),shape.y.min(shape.y+shape.h),shape.w.abs(),shape.h.abs())}else{crate::recorder_annotations::asset_on_canvas(shape,(x,y,w,h)).0};
    render_png(&svg,&asset)?;let until=timeline.get(index+1).map(|(t,_)|*t).unwrap_or(segment.duration+1.);let duration=(until-time).max(1./c.fps as f64);
    entries+=&format!("file '{}'\noption framerate {}\nduration {duration:.6}\n",asset.to_string_lossy().replace('\\',"/").replace('\'',"'\\''"),c.fps);
   }
   fs::write(&list,entries).map_err(|e|e.to_string())?;cmd.args(["-f","concat","-safe","0","-i"]).arg(list);
   if annotation.kind=="blur"{let radius=(w.min(h)/2).saturating_sub(1).min(8).max(1);filters.push(format!("[{video}]split[{label}base][{label}patch];[{label}patch]crop={w}:{h}:{x}:{y},boxblur={radius}:2,format=rgba[{label}blur];[{next}:v]format=gray[{label}mask];[{label}blur][{label}mask]alphamerge[{label}masked];[{label}base][{label}masked]overlay={x}:{y}:format=auto:enable='gte(t,{from})*lt(t,{to})'[{label}]"));}
   else{filters.push(format!("[{video}][{next}:v]overlay={x}:{y}:format=auto:enable='gte(t,{from})*lt(t,{to})'[{label}]"));}next+=1;video=label;
  }
  if c.resolution!="original"{let (w,h)=if c.resolution=="720p"{(1280,720)}else{(1920,1080)};filters.push(format!("[{video}]scale=w='min(iw,{w})':h='min(ih,{h})':force_original_aspect_ratio=decrease:force_divisible_by=2:flags=lanczos[scaled]"));video="scaled".into();}
  let mut audio_labels=Vec::new();for (j,(path,is_mic)) in segment.audio.iter().enumerate(){cmd.arg("-i").arg(path);let mic=*is_mic;let gain=if mic{c.mic_gain}else{c.system_gain};let enhance=if mic&&c.voice_enhance{"highpass=f=75,afftdn=nf=-25:tn=1,acompressor=threshold=0.125:ratio=2:attack=5:release=120:makeup=1.5,"}else{""};filters.push(format!("[{}:a]aresample=48000,aformat=sample_fmts=fltp:sample_rates=48000:channel_layouts=stereo,{}volume={},apad,atrim=duration={}[a{j}]",next+j,enhance,gain,segment.duration));audio_labels.push(format!("[a{j}]"));}
  let input_count=next+segment.audio.len();if audio_labels.is_empty()&&job.segments.iter().any(|s|!s.audio.is_empty()){cmd.args(["-f","lavfi","-i","anullsrc=r=48000:cl=stereo"]);filters.push(format!("[{input_count}:a]atrim=duration={}[silent]",segment.duration));audio_labels.push("[silent]".into());}
  if !audio_labels.is_empty(){filters.push(format!("{}amix=inputs={}:normalize=0,alimiter=limit=0.95[audio]",audio_labels.join(""),audio_labels.len()));}
  cmd.args(["-filter_complex",&filters.join(";"),"-map",&format!("[{video}]")]);if !audio_labels.is_empty(){cmd.args(["-map","[audio]","-c:a","aac","-b:a","192k","-ar","48000"]);}intermediate_video_settings(&mut cmd,false);cmd.args(["-t",&segment.duration.to_string(),"-movflags","+faststart"]).arg(&result);run_progress(cmd,segment.duration,80.*segment_offset/total_duration,80.*(segment_offset+segment.duration)/total_duration)?;completed.push(result);segment_offset+=segment.duration;
 }
 let list=job.dir.join("segments.txt");fs::write(&list,completed.iter().map(|p|format!("file '{}'\n",p.to_string_lossy().replace('\\',"/").replace('\'',"'\\''"))).collect::<String>()).map_err(|e|e.to_string())?;
 let export=job.dir.join("export-working.mp4");let mut cmd=ffmpeg();cmd.args(["-f","concat","-safe","0","-i"]).arg(list).args(["-c","copy","-movflags","+faststart"]).arg(&export);run_progress(cmd,total_duration,80.,85.)?;
 if let Some(original)=&job.original_output {
 let mut original_cmd=ffmpeg();original_cmd.arg("-i").arg(&export);video_settings(&mut original_cmd,c,job.gpu);original_cmd.args(["-c:a","copy","-movflags","+faststart"]).arg(original);run_progress(original_cmd,total_duration,85.,87.)?;let info=crate::process::video_info(&export).ok_or("Could not read the finished video dimensions.")?;let frame=job.dir.join("frame.png");render_png(&mockup_svg(c,info.w,info.h),&frame)?;let framed=job.dir.join("export-mockup.mp4");let mut command=ffmpeg();let pad=frame_pad(c,info.w);let rate=c.fps.to_string();command.arg("-i").arg(&export).args(["-framerate",&rate,"-loop","1","-i"]).arg(&frame);let graph=if let Some(id)=background_preset(c){let bg=job.dir.join("background.png");render_png(&background_svg(id,info.w+2*pad,info.h+2*pad+40),&bg)?;command.args(["-framerate",&rate,"-loop","1","-i"]).arg(&bg);format!("[0:v]setpts=PTS-STARTPTS[v0];[2:v][v0]overlay={}:{}:shortest=1:format=auto[padded];[padded][1:v]overlay=0:0:shortest=1:format=auto[framed]",pad,pad+40)}else{format!("[0:v]setpts=PTS-STARTPTS,pad=iw+{}:ih+{}:{}:{}:color={}[padded];[padded][1:v]overlay=0:0:shortest=1:format=auto[framed]",2*pad,2*pad+40,pad,pad+40,c.background)};command.args(["-filter_complex_threads","1","-filter_complex",&graph,"-map","[framed]","-map","0:a?","-c:a","copy"]);video_settings(&mut command,c,job.gpu);command.args(["-t",&total_duration.to_string(),"-movflags","+faststart"]).arg(&framed);run_progress(command,total_duration,87.,99.)?;copy_progress(&framed,&job.output,99.,100.)?;Ok((job.output.clone(),Some(original.clone())))
 }else{let delivery=job.dir.join("export-original.mp4");let mut command=ffmpeg();command.arg("-i").arg(&export);video_settings(&mut command,c,job.gpu);command.args(["-c:a","copy","-movflags","+faststart"]).arg(&delivery);run_progress(command,total_duration,85.,99.)?;copy_progress(&delivery,&job.output,99.,100.)?;Ok((job.output.clone(),None))}

}

fn preset_stops(id:&str)->Option<[&'static str;3]>{Some(match id{"aurora"=>["#5b8def","#8f6bf0","#20c4cb"],"sunset"=>["#ff9966","#ff5e8a","#8e4fd8"],"ocean"=>["#0f2a6b","#1f6fd6","#3fd0e8"],"forest"=>["#0e3b2e","#1e8a6a","#8ad9a8"],"midnight"=>["#0a0e22","#1d2a5c","#3b3f9a"],"peach"=>["#ffd6c0","#ffb3c1","#c9b6ff"],"mint"=>["#d1f7e6","#9ee6d4","#7ac6f2"],"graphite"=>["#2b2f3a","#454c5e","#6b7488"],"candy"=>["#ff8ad8","#a78bfa","#60a5fa"],"lavender"=>["#e0c3fc","#c4b5fd","#8ec5fc"],"lagoon"=>["#00c6a7","#1e9bd7","#4b5cc4"],"ember"=>["#ff512f","#dd2476","#3a1c71"],"rose"=>["#ff9a9e","#fecfef","#a18cd1"],"gold"=>["#f6d365","#fda085","#f5576c"],"sky"=>["#89f7fe","#66a6ff","#5b7cfa"],"violet"=>["#6a11cb","#8a4fff","#2575fc"],"emerald"=>["#0f9b8e","#38ef7d","#b6f3a4"],"coral"=>["#ff758c","#ff7eb3","#ffb199"],"dusk"=>["#2c3e50","#4b3f72","#fd746c"],"ice"=>["#e0f7ff","#b6e3ff","#8ab4ff"],"neon"=>["#0f0c29","#4a2a8f","#24c6dc"],"cotton"=>["#fbc2eb","#a6c1ee","#c2e9fb"],_=>return None})}
fn background_preset(c:&Config)->Option<&str>{c.background.strip_prefix("preset:").filter(|id|preset_stops(id).is_some())}
fn background_svg(id:&str,w:u32,h:u32)->String{let [a,b,m]=preset_stops(id).unwrap_or(["#15213A","#15213A","#15213A"]);format!(r##"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="{a}"/><stop offset="0.55" stop-color="{b}"/><stop offset="1" stop-color="{m}"/></linearGradient><radialGradient id="r" cx="0.2" cy="0.1" r="0.8"><stop offset="0" stop-color="#ffffff" stop-opacity="0.28"/><stop offset="1" stop-color="#ffffff" stop-opacity="0"/></radialGradient></defs><rect width="{w}" height="{h}" fill="url(#g)"/><rect width="{w}" height="{h}" fill="url(#r)"/></svg>"##)}
/// Margin around the window in pixels: `frame_padding` is a percentage of the video width, kept even for the video encoder.
fn frame_pad(c:&Config,w:u32)->u32{(((w as f64*c.frame_padding as f64/100.).round() as u32).max(8))&!1}
fn mockup_svg(c:&Config,w:u32,h:u32)->String{let p=frame_pad(c,w);let outer=if background_preset(c).is_some(){"none".to_string()}else{c.background.clone()};let right=w+p;let chrome=if c.frame=="mac"{format!(r##"<circle cx="{}" cy="{}" r="6" fill="#ff5f57"/><circle cx="{}" cy="{}" r="6" fill="#febc2e"/><circle cx="{}" cy="{}" r="6" fill="#28c840"/>"##,p+20,p+20,p+40,p+20,p+60,p+20)}else{format!(r##"<path d="M{} {}h12 M{} {}h10v10h-10z M{} {}l10 10m0-10l-10 10" stroke="#253252" fill="none" stroke-width="1.3"/>"##,right.saturating_sub(78),p+19,right.saturating_sub(48),p+14,right.saturating_sub(20),p+14)};format!(r##"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}"><path fill="{}" fill-rule="evenodd" d="M0 0H{}V{}H0Z M{} {}H{}V{}H{}Z"/><rect x="{}" y="{}" width="{w}" height="40" rx="10" fill="#e8ecf4"/><rect x="{}" y="{}" width="{w}" height="20" fill="#e8ecf4"/>{chrome}</svg>"##,w+2*p,h+2*p+40,outer,w+2*p,h+2*p+40,p,p+40,w+p,h+p+40,p,p,p,p,p+20)}
fn copy_progress(from:&Path,to:&Path,start:f64,end:f64)->Res<()> {use std::io::{Read,Write};let temporary=to.with_extension(format!("mp4.partial-{}",chrono::Local::now().timestamp_millis()));let mut output=fs::OpenOptions::new().write(true).create_new(true).open(&temporary).map_err(|e|e.to_string())?;let result=(||{let mut input=fs::File::open(from).map_err(|e|e.to_string())?;let total=input.metadata().map_err(|e|e.to_string())?.len().max(1);let mut written=0u64;let mut buffer=vec![0u8;262144];loop{if CANCEL.load(Ordering::Relaxed){return Err("Recording cancelled.".into());}let n=input.read(&mut buffer).map_err(|e|e.to_string())?;if n==0{break;}output.write_all(&buffer[..n]).map_err(|e|e.to_string())?;written+=n as u64;save_progress(start+(end-start)*written as f64/total as f64);}output.flush().map_err(|e|e.to_string())?;output.sync_all().map_err(|e|e.to_string())?;Ok::<_,String>(())})();drop(output);if let Err(error)=result{let _=fs::remove_file(&temporary);return Err(error);}fs::rename(&temporary,to).map_err(|e|e.to_string())?;save_progress(end);Ok(())}

#[tauri::command]pub async fn recorder_start(app:tauri::AppHandle,target:Target,config:Config)->Res<()> {
 let _job=crate::updates::job()?;
 validate(&config)?;let bounds=bounds(&app,&target)?;if bounds.w<2||bounds.h<2||bounds.w>8192||bounds.h>8192{return Err("Select a source between 2 and 8192 pixels per side.".into());}
 tauri::async_runtime::spawn_blocking(move||{let _op=OP.lock().unwrap();if SESSION.lock().unwrap().is_some(){return Err("A recording is already active.".into());}CANCEL.store(false,Ordering::Relaxed);stop_preview();*STATE.lock().unwrap()=Some(State{phase:"preparing".into(),..Default::default()});
  let result=(||{fs::create_dir_all(&config.output_dir).map_err(|e|e.to_string())?;let name=chrono::Local::now().format("Halo Recording %Y-%m-%d %H-%M-%S").to_string();let output=crate::process::unique(Path::new(&config.output_dir),&name,"mp4");let dir=crate::system::tools_dir().join("recorder-cache").join(chrono::Local::now().timestamp_millis().to_string());fs::create_dir_all(&dir).map_err(|e|e.to_string())?;let gpu=config.encoder=="auto"&&bounds.w+2*frame_pad(&config,bounds.w)<=4096&&bounds.h+2*frame_pad(&config,bounds.w)+40<=4096&&gpu_available(&dir,bounds.w,bounds.h,config.fps);let original_output=if config.frame!="none"{Some(crate::process::unique(Path::new(&config.output_dir),&format!("{name} (original)"),"mp4"))}else{None};let mut job=Session{config,target,bounds,dir,output,original_output,segments:vec![],running:None,gpu,mic_peak:Arc::new(AtomicU32::new(0)),sys_peak:Arc::new(AtomicU32::new(0)),mic_muted:Arc::new(AtomicBool::new(false)),sys_muted:Arc::new(AtomicBool::new(false)),annotations:vec![]};let running=start_segment(&job)?;if running.mic_failed{job.config.microphone=false;}if running.system_failed{job.config.system_audio=false;}job.running=Some(running);*SESSION.lock().unwrap()=Some(job);*STATE.lock().unwrap()=Some(State{phase:"recording".into(),target:Some(bounds),..Default::default()});Ok::<_,String>(())})();if let Err(error)=&result{phase("idle",Some(error.clone()));}result
 }).await.map_err(|e|e.to_string())?
}
#[tauri::command]pub async fn recorder_pause()->Res<()> {tauri::async_runtime::spawn_blocking(||{let _op=OP.lock().unwrap();let running=SESSION.lock().unwrap().as_mut().and_then(|s|s.running.take()).ok_or("No recording is running.")?;phase("pausing",None);match finish_segment(running){Ok(segment)=>{let elapsed={let mut session=SESSION.lock().unwrap();let job=session.as_mut().unwrap();job.segments.push(segment);job.segments.iter().map(|s|s.duration).sum()};if let Some(state)=STATE.lock().unwrap().as_mut(){state.elapsed=elapsed;}phase("paused",None);Ok(())},Err(error)=>{phase("paused",Some(error.clone()));Err(error)}}}).await.map_err(|e|e.to_string())?}
#[tauri::command]pub async fn recorder_resume()->Res<()> {tauri::async_runtime::spawn_blocking(||{let _op=OP.lock().unwrap();let mut lock=SESSION.lock().unwrap();let job=lock.as_mut().ok_or("No recording is paused.")?;if job.running.is_some(){return Err("Recording is already running.".into());}if let Some(state)=STATE.lock().unwrap().as_mut(){state.elapsed=job.segments.iter().map(|s|s.duration).sum();}phase("resuming",None);match start_segment(job){Ok(running)=>{if running.mic_failed{job.config.microphone=false;}if running.system_failed{job.config.system_audio=false;}job.running=Some(running);phase("recording",None);Ok(())},Err(error)=>{phase("paused",Some(error.clone()));Err(error)}}}).await.map_err(|e|e.to_string())?}
#[tauri::command]pub async fn recorder_stop(app:tauri::AppHandle)->Res<String> {flush_annotations(&app).await?;tauri::async_runtime::spawn_blocking(move||{let _op=OP.lock().unwrap();let mut job=SESSION.lock().unwrap().take().ok_or("No recording is active.")?;if let Some(w)=app.get_webview_window("recorder-annotations"){let _=w.destroy();}save_progress(0.);phase("saving",None);let result=(||{if let Some(r)=job.running.take(){job.segments.push(finish_segment(r)?);}finish(&job)})();match result{Ok((path,original))=>{let out=path.to_string_lossy().into_owned();*STATE.lock().unwrap()=Some(State{phase:"idle".into(),output:Some(out.clone()),original_output:original.map(|p|p.to_string_lossy().into_owned()),elapsed:job.segments.iter().map(|s|s.duration).sum(),..Default::default()});let _=app.emit("halo://recording-saved",&out);Ok(out)},Err(error)=>{log_error(&format!("Export failed: {error}; retained capture: {}",job.dir.display()));let original=job.original_output.as_ref().filter(|p|p.exists()).map(|p|p.to_string_lossy().into_owned());if let Some(path)=original{*STATE.lock().unwrap()=Some(State{phase:"idle".into(),output:Some(path.clone()),original_output:Some(path),elapsed:job.segments.iter().map(|s|s.duration).sum(),error:Some("The original video was saved, but the mockup could not be completed.".into()),..Default::default()});}else{phase("idle",Some("Could not save the recording. Please try again.".into()));}Err("Could not complete the recording export. Please try again.".into())}}}).await.map_err(|e|e.to_string())?}

#[tauri::command]pub async fn recorder_test_scene(app:tauri::AppHandle,fullscreen:Option<bool>)->Res<()> {if !cfg!(debug_assertions){return Err("Available only in development.".into());}if let Some(w)=app.get_webview_window("recorder-test"){w.set_fullscreen(fullscreen.unwrap_or(false)).map_err(|e|e.to_string())?;w.show().map_err(|e|e.to_string())?;}else{tauri::WebviewWindowBuilder::new(&app,"recorder-test",tauri::WebviewUrl::App("output/recorder-research/test-scene.html".into())).title("Halo Recorder Test").fullscreen(fullscreen.unwrap_or(false)).inner_size(800.,600.).build().map_err(|e|e.to_string())?;}Ok(())}

pub fn active()->bool {SESSION.try_lock().map(|s|s.is_some()).unwrap_or(true)||matches!(recorder_state().phase.as_str(),"preparing"|"saving"|"pausing")}

#[tauri::command]pub fn recorder_debug_preview(app:tauri::AppHandle,enabled:bool)->Res<()> {if !cfg!(debug_assertions){return Err("Development only.".into());}let w=app.get_webview_window("recorder").ok_or("Recorder is closed.")?;let h=w.hwnd().map_err(|e|e.to_string())?;unsafe{SetWindowDisplayAffinity(HWND(h.0),if enabled{WDA_NONE}else{WDA_EXCLUDEFROMCAPTURE}).map_err(|e|e.to_string())?;}Ok(())}

/// Cancel only this recorder's work; conversion and upload jobs are independent.
pub async fn cancel_recorder(app:tauri::AppHandle)->Res<()> {
 CANCEL.store(true,Ordering::Relaxed);for label in ["recorder-annotations"]{if let Some(w)=app.get_webview_window(label){let _=w.destroy();}}let _=app.emit_to("recorder","halo://recorder-closed",());cancel_area(&app);if let Some(w)=app.get_webview_window("recorder"){let _=w.hide();}
 tauri::async_runtime::spawn_blocking(move||{let _op=OP.lock().unwrap();stop_preview();if let Some(mut job)=SESSION.lock().unwrap().take(){if let Some(mut r)=job.running.take(){r.stop.store(true,Ordering::Relaxed);let _=r.child.kill();let _=r.child.wait();if let Some(pointer)=r.pointer{let _=pointer.join();}for track in r.audio{let _=track.join.join();}}}*STATE.lock().unwrap()=Some(State{phase:"idle".into(),..Default::default()});}).await.map_err(|e|e.to_string())?;Ok(())
}
#[tauri::command]pub async fn recorder_cancel(app:tauri::AppHandle)->Res<()> {cancel_recorder(app).await}

#[tauri::command]pub async fn recorder_toggle_audio(kind:String,enabled:bool)->Res<()> {
 if !["microphone","system"].contains(&kind.as_str()){return Err("Choose microphone or system audio.".into());}
 tauri::async_runtime::spawn_blocking(move||{let _op=OP.lock().unwrap();let mut guard=SESSION.lock().unwrap();let job=guard.as_mut().ok_or("No recording is active.")?;let mic=kind=="microphone";let muted=if mic{job.mic_muted.clone()}else{job.sys_muted.clone()};muted.store(!enabled,Ordering::Relaxed);
 if enabled{if let Some(running)=job.running.as_mut(){if !running.audio.iter().any(|track|track.mic==mic){let index=job.segments.len();let path=job.dir.join(format!("{}-{index}.wav",if mic{"mic"}else{"system"}));let track=crate::recorder_audio::start(path,mic,job.config.microphone_id.clone(),running.stop.clone(),running.gate.clone(),if mic{job.mic_peak.clone()}else{job.sys_peak.clone()},muted.clone())?;running.audio.push(track);}}}
 if mic{job.config.microphone=enabled;}else{job.config.system_audio=enabled;}Ok(())}).await.map_err(|e|e.to_string())?
}

#[derive(Default,Clone,Serialize)]
struct CountdownState {seconds:u32,id:u64,active:bool,ready:bool,loaded:bool}
impl CountdownState {
 fn begin(&mut self,seconds:u32)->u64{self.id+=1;self.seconds=seconds;self.active=true;self.ready=false;self.id}
 fn close(&mut self,id:Option<u64>)->Option<u64>{if id.map(|id|id!=self.id).unwrap_or(false){return None;}self.active=false;self.ready=false;Some(self.id)}
 fn prepared(&mut self,id:u64)->bool{self.loaded=true;if id!=self.id{return false;}self.ready=true;self.active}
}
static COUNTDOWN:Mutex<CountdownState>=Mutex::new(CountdownState{seconds:3,id:0,active:false,ready:false,loaded:false});
static COUNTDOWN_WINDOW:Mutex<()>=Mutex::new(());
fn countdown_window(app:&tauri::AppHandle)->Res<tauri::WebviewWindow>{
 let _creation=COUNTDOWN_WINDOW.lock().unwrap();if let Some(w)=app.get_webview_window("recorder-countdown"){return Ok(w);}
 let w=tauri::WebviewWindowBuilder::new(app,"recorder-countdown",tauri::WebviewUrl::App("recorder-countdown.html".into())).title("Halo · Recording starts").visible(false).decorations(false).transparent(true).always_on_top(true).skip_taskbar(true).resizable(false).shadow(false).inner_size(260.,294.).build().map_err(|e|e.to_string())?;exclude(&w);Ok(w)
}
fn close_countdown(app:&tauri::AppHandle,id:Option<u64>)->Option<u64>{let mut state=COUNTDOWN.lock().unwrap();let id=state.close(id)?;if let Some(w)=app.get_webview_window("recorder-countdown"){let _=w.hide();}Some(id)}
fn elapsed(job:&Session)->f64{job.segments.iter().map(|s|s.duration).sum::<f64>()+job.running.as_ref().and_then(|r|r.gate.lock().unwrap().map(|t|t.elapsed().as_secs_f64())).unwrap_or(0.)}
#[tauri::command]pub async fn recorder_countdown_open(app:tauri::AppHandle,target:Target,seconds:u32)->Res<u64>{
 if !(1..=10).contains(&seconds){return Err("Invalid countdown.".into());}let r=bounds(&app,&target)?;let w=countdown_window(&app)?;
 let snapshot={let mut state=COUNTDOWN.lock().unwrap();state.begin(seconds);let _=w.hide();state.clone()};
 let scale=w.scale_factor().unwrap_or(1.);w.set_position(PhysicalPosition::new(r.x+r.w as i32/2-(130.*scale)as i32,r.y+r.h as i32/2-(147.*scale)as i32)).map_err(|e|e.to_string())?;
 app.emit_to("recorder-countdown","halo://countdown-reset",&snapshot).map_err(|e|e.to_string())?;
 tauri::async_runtime::spawn_blocking(move||{let started=Instant::now();loop{let state=COUNTDOWN.lock().unwrap().clone();if state.id!=snapshot.id||!state.active||state.ready{return Ok(snapshot.id);}if started.elapsed()>Duration::from_secs(8){close_countdown(&app,Some(snapshot.id));return Err("The countdown interface did not become ready.".into());}thread::sleep(Duration::from_millis(5));}}).await.map_err(|e|e.to_string())?
}
#[tauri::command]pub fn recorder_countdown_data()->serde_json::Value{serde_json::to_value(COUNTDOWN.lock().unwrap().clone()).unwrap()}
#[tauri::command]pub fn recorder_countdown_ready(app:tauri::AppHandle,id:u64)->Res<bool>{let mut state=COUNTDOWN.lock().unwrap();if !state.prepared(id){return Ok(false);}if let Some(w)=app.get_webview_window("recorder-countdown"){w.show().map_err(|e|e.to_string())?;let _=w.set_focus();return Ok(true);}Ok(false)}
#[tauri::command]pub async fn recorder_close_countdown(app:tauri::AppHandle,id:Option<u64>){close_countdown(&app,id);}
pub fn cancel_countdown(app:&tauri::AppHandle){if let Some(id)=close_countdown(app,None){let _=app.emit("halo://countdown-cancelled",serde_json::json!({"id":id}));}}
#[tauri::command]pub async fn recorder_cancel_countdown(app:tauri::AppHandle,id:Option<u64>){let active={let state=COUNTDOWN.lock().unwrap();state.active&&id.map(|id|id==state.id).unwrap_or(true)};if active{if let Some(id)=close_countdown(&app,id){let _=app.emit("halo://countdown-cancelled",serde_json::json!({"id":id}));}}}
#[tauri::command]pub async fn recorder_window_thumbnail(window:u64)->Res<String>{
 let h=HWND(window as usize as *mut _);
 if unsafe{IsIconic(h)}.as_bool(){return Err("Restore this window to preview it.".into());}
 if !unsafe{IsWindow(Some(h))}.as_bool(){return Err("That window has closed. Choose a window again.".into());}
 tauri::async_runtime::spawn_blocking(move||{
  // gfxcapture (Windows Graphics Capture) only delivers a frame promptly for the foreground
  // window; background/occluded windows can hang indefinitely opening the capture session. A
  // plain GDI PrintWindow grab works regardless of focus or occlusion and is near-instant.
  // HWND wraps a raw pointer, so it isn't Send; rebuild it from the u64 inside this thread.
  let h=HWND(window as usize as *mut _);
  use windows::Win32::Graphics::Gdi::{CreateCompatibleDC,CreateCompatibleBitmap,SelectObject,DeleteDC,DeleteObject,GetDIBits,GetDC,ReleaseDC,BITMAPINFO,BITMAPINFOHEADER,BI_RGB,DIB_RGB_COLORS,HGDIOBJ};
  let mut rect=RECT::default();unsafe{GetWindowRect(h,&mut rect)}.map_err(|e|e.to_string())?;
  let (w,ht)=((rect.right-rect.left).max(1),(rect.bottom-rect.top).max(1));
  let screen_dc=unsafe{GetDC(None)};
  let mem_dc=unsafe{CreateCompatibleDC(Some(screen_dc))};
  let bitmap=unsafe{CreateCompatibleBitmap(screen_dc,w,ht)};
  let old=unsafe{SelectObject(mem_dc,HGDIOBJ(bitmap.0))};
  let rendered=unsafe{PrintWindow(h,mem_dc,PRINT_WINDOW_FLAGS(2))}.as_bool();
  let mut buffer=vec![0u8;w as usize*ht as usize*4];
  let mut info=BITMAPINFO{bmiHeader:BITMAPINFOHEADER{biSize:std::mem::size_of::<BITMAPINFOHEADER>()as u32,biWidth:w,biHeight:-ht,biPlanes:1,biBitCount:32,biCompression:BI_RGB.0 as u32,..Default::default()},..Default::default()};
  let copied=unsafe{GetDIBits(mem_dc,bitmap,0,ht as u32,Some(buffer.as_mut_ptr() as *mut _),&mut info,DIB_RGB_COLORS)};
  unsafe{SelectObject(mem_dc,old);let _=DeleteObject(HGDIOBJ(bitmap.0));let _=DeleteDC(mem_dc);ReleaseDC(None,screen_dc);}
  if !rendered||copied==0{return Err("Preview unavailable".into());}
  for px in buffer.chunks_exact_mut(4){px.swap(0,2);} // BGRA -> RGBA
  let image=image::RgbaImage::from_raw(w as u32,ht as u32,buffer).ok_or("Preview unavailable")?;
  let image=image::DynamicImage::ImageRgba8(image).resize(320,180,image::imageops::FilterType::Triangle);
  let mut out=std::io::Cursor::new(Vec::new());
  image.write_to(&mut out,image::ImageFormat::Png).map_err(|e|e.to_string())?;
  Ok(format!("data:image/png;base64,{}",base64::engine::general_purpose::STANDARD.encode(out.into_inner())))
 }).await.map_err(|e|e.to_string())?
}
#[tauri::command]pub fn recorder_annotation_data()->Res<serde_json::Value>{let guard=SESSION.lock().unwrap();let job=guard.as_ref().ok_or("No recording is active.")?;Ok(serde_json::json!({"width":job.bounds.w,"height":job.bounds.h,"fps":job.config.fps,"annotations":job.annotations,"elapsed":elapsed(job)}))}
#[tauri::command]pub fn recorder_annotation_add(mut annotation:crate::recorder_annotations::Annotation)->Res<crate::recorder_annotations::Annotation>{let mut guard=SESSION.lock().unwrap();let job=guard.as_mut().ok_or("No recording is active.")?;if job.annotations.len()>=100{return Err("This recording already contains 100 annotations.".into());}crate::recorder_annotations::validate(&annotation,job.bounds.w,job.bounds.h)?;annotation.id=job.annotations.len()as u64+1;annotation.start=elapsed(job);annotation.end=None;annotation.keyframes=vec![annotation.frame(annotation.start)];job.annotations.push(annotation.clone());Ok(annotation)}
#[tauri::command]pub fn recorder_annotation_end(id:Option<u64>)->Res<()> {let mut guard=SESSION.lock().unwrap();let job=guard.as_mut().ok_or("No recording is active.")?;let time=elapsed(job);for annotation in &mut job.annotations{if annotation.end.is_none()&&id.map(|id|id==annotation.id).unwrap_or(true){annotation.end=Some(time);}}Ok(())}
#[tauri::command]pub async fn recorder_annotations_edit(app:tauri::AppHandle,editing:bool)->Res<()> {if let Some(w)=app.get_webview_window("recorder-annotations"){w.set_ignore_cursor_events(!editing).map_err(|e|e.to_string())?;let _=app.emit("halo://annotation-editing",editing);if editing{let _=w.show();let _=w.set_focus();}return Ok(());}if !editing{return Ok(());}let r={let guard=SESSION.lock().unwrap();let job=guard.as_ref().ok_or("No recording is active.")?;if job.target.mode=="window"{window_rect(job.target.window)?}else{job.bounds}};let w=tauri::WebviewWindowBuilder::new(&app,"recorder-annotations",tauri::WebviewUrl::App("recorder-annotations.html".into())).title("Halo · Recording annotations").decorations(false).transparent(true).always_on_top(false).skip_taskbar(true).resizable(false).shadow(false).build().map_err(|e|e.to_string())?;w.set_size(PhysicalSize::new(r.w,r.h)).map_err(|e|e.to_string())?;w.set_position(PhysicalPosition::new(r.x,r.y)).map_err(|e|e.to_string())?;exclude(&w);let _=w.set_focus();let handle=app.clone();thread::spawn(move||{loop{thread::sleep(Duration::from_millis(150));let Some(window)=handle.get_webview_window("recorder-annotations")else{break};let bounds={let guard=SESSION.lock().unwrap();let Some(job)=guard.as_ref()else{break};if job.target.mode=="window"{window_rect(job.target.window).ok()}else{None}};if let Some(bounds)=bounds{if window.outer_position().ok()!=Some(PhysicalPosition::new(bounds.x,bounds.y)){let _=window.set_position(PhysicalPosition::new(bounds.x,bounds.y));}}}});Ok(())}
#[tauri::command]pub async fn recorder_annotation_snapshot()->Res<String>{let(target,r)={let guard=SESSION.lock().unwrap();let job=guard.as_ref().ok_or("No recording is active.")?;(job.target.clone(),job.bounds)};tauri::async_runtime::spawn_blocking(move||{let dir=crate::system::tools_dir().join("recorder-cache");let path=dir.join(format!("annotation-source-{}.png",chrono::Local::now().timestamp_millis()));let source=if target.mode=="window"{format!("gfxcapture=hwnd={}:capture_cursor=0",target.window)}else{format!("gfxcapture=hmonitor={}:capture_cursor=0",target.monitor)};let mut c=ffmpeg();c.args(["-f","lavfi","-i",&source,"-frames:v","1","-vf"]);let crop=if target.mode=="area"{let area=target.rect.unwrap();format!("hwdownload,format=bgra,crop={}:{}:{}:{}",r.w,r.h,area.x,area.y)}else{"hwdownload,format=bgra".into()};c.arg(crop).arg(&path);run(c)?;Ok(format!("data:image/png;base64,{}",base64::engine::general_purpose::STANDARD.encode(fs::read(path).map_err(|e|e.to_string())?)))}).await.map_err(|e|e.to_string())?}
#[tauri::command]pub fn recorder_output_info()->Res<serde_json::Value>{let state=recorder_state();let path=state.output.ok_or("No recording has been saved.")?;let metadata=fs::metadata(&path).map_err(|e|e.to_string())?;Ok(serde_json::json!({"path":path,"original_path":state.original_output,"duration":state.elapsed,"bytes":metadata.len()}))}
#[tauri::command]pub async fn recorder_recycle_output()->Res<()> {let path=recorder_state().output.ok_or("No recording has been saved.")?;tauri::async_runtime::spawn_blocking(move||crate::process::recycle_recording(Path::new(&path))).await.map_err(|e|e.to_string())?}

fn log_error(message:&str){let dir=crate::system::tools_dir().join("recorder-cache");let _=fs::create_dir_all(&dir);if let Ok(mut file)=fs::OpenOptions::new().create(true).append(true).open(dir.join("diagnostics.log")){let _=writeln!(file,"{} {}",chrono::Local::now().to_rfc3339(),message);}}
#[tauri::command]pub fn recorder_log_error(message:String){log_error(&message.chars().take(8000).collect::<String>());}

static ANNOTATION_FLUSH:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(0);
async fn flush_annotations(app:&tauri::AppHandle)->Res<()> {if app.get_webview_window("recorder-annotations").is_none(){return Ok(());}let token=chrono::Local::now().timestamp_millis()as u64;app.emit_to("recorder-annotations","halo://annotation-flush",token).map_err(|e|e.to_string())?;let app=app.clone();tauri::async_runtime::spawn_blocking(move||{let started=Instant::now();while ANNOTATION_FLUSH.load(Ordering::Relaxed)!=token{if app.get_webview_window("recorder-annotations").is_none(){return Ok(());}if started.elapsed()>Duration::from_secs(2){return Err("Could not finish annotations. Please try again.".into());}thread::sleep(Duration::from_millis(10));}Ok(())}).await.map_err(|e|e.to_string())?}
#[tauri::command]pub fn recorder_annotations_flushed(token:u64){ANNOTATION_FLUSH.store(token,Ordering::Relaxed);}
#[tauri::command]pub fn recorder_annotation_update(mut annotation:crate::recorder_annotations::Annotation)->Res<()> {let mut guard=SESSION.lock().unwrap();let job=guard.as_mut().ok_or("No recording is active.")?;crate::recorder_annotations::validate(&annotation,job.bounds.w,job.bounds.h)?;let time=elapsed(job);let current=job.annotations.iter_mut().find(|a|a.id==annotation.id&&a.end.is_none()).ok_or("This annotation is no longer active.")?;annotation.start=current.start;annotation.end=None;annotation.keyframes=std::mem::take(&mut current.keyframes);if annotation.keyframes.len()>=3600{annotation.keyframes=annotation.keyframes.into_iter().enumerate().filter_map(|(i,f)|(i%2==0).then_some(f)).collect();}if annotation.keyframes.last().map(|f|f.x!=annotation.x||f.y!=annotation.y||f.w!=annotation.w||f.h!=annotation.h||f.text!=annotation.text).unwrap_or(true){annotation.keyframes.push(annotation.frame(time));}*current=annotation;Ok(())}

#[cfg(test)]mod framed_export_tests{use super::*;
 fn render_size(svg:&str)->(u32,u32){let tree=resvg::usvg::Tree::from_str(svg,&resvg::usvg::Options::default()).expect("valid svg");let size=tree.size().to_int_size();(size.width(),size.height())}
 #[test]fn mockup_and_background_match_the_padded_video(){for frame in ["mac","windows"]{for percent in [2u32,8,30]{let mut c=Config::default();c.frame=frame.into();c.frame_padding=percent;let (w,h)=(1920u32,1080u32);let pad=frame_pad(&c,w);assert_eq!(pad%2,0);let expected=(w+2*pad,h+2*pad+40);assert_eq!(render_size(&mockup_svg(&c,w,h)),expected);assert!(background_preset(&c).is_some());assert_eq!(render_size(&background_svg("aurora",expected.0,expected.1)),expected);}}}
 #[test]fn every_preset_has_gradient_stops(){for id in ["aurora","sunset","ocean","forest","midnight","peach","mint","graphite","candy","lavender","lagoon","ember","rose","gold","sky","violet","emerald","coral","dusk","ice","neon","cotton"]{assert!(preset_stops(id).is_some(),"{id}");}}
 #[test]fn default_config_is_valid(){assert!(validate(&Config::default()).is_ok());}
}
