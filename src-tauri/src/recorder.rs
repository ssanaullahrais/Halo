//! Independent native screen recording via FFmpeg's Windows Graphics Capture input.
use std::{fs,io::{BufRead,BufReader,Write},os::windows::process::CommandExt,path::{Path,PathBuf},process::{Child,Stdio},sync::{Arc,Mutex,atomic::{AtomicBool,AtomicU32,Ordering}},thread,time::{Duration,Instant}};
use base64::Engine;
use serde::{Deserialize,Serialize};
use tauri::{Emitter,Manager,PhysicalPosition,PhysicalSize};
use windows::core::BOOL;
use windows::Win32::{Foundation::{HWND,POINT,RECT},Graphics::Gdi::{MonitorFromPoint,MONITOR_DEFAULTTONEAREST,ClientToScreen},Storage::Xps::{PrintWindow,PRINT_WINDOW_FLAGS},System::Threading::{GetCurrentProcessId,GetCurrentThreadId,AttachThreadInput},UI::{Input::KeyboardAndMouse::{GetAsyncKeyState,SendInput,INPUT,INPUT_0,INPUT_MOUSE,MOUSEINPUT,MOUSEEVENTF_WHEEL},WindowsAndMessaging::*}};
use crate::recorder_audio::AudioTrack;
const HIDDEN:u32=0x0800_0000;
const CLICK_SLOTS:usize=4;
#[path="recorder_export.rs"]mod export;
#[path="recorder_render.rs"]mod render;
#[path="recorder_encoder.rs"]mod encoder;
use encoder::Encoder;
#[path="recorder_jobs.rs"]mod jobs;
#[path="recorder_project.rs"]mod project;
#[path="recorder_cinematic.rs"]mod cinematic;
#[path="recorder_runtime.rs"]pub(crate) mod runtime;
#[path="recorder_cursor_motion.rs"]mod cursor_motion;
#[path="recorder_context.rs"]mod context;
#[path="recorder_zoom_preview.rs"]mod zoom_preview;
#[path="recorder_soundtrack.rs"]mod soundtrack;
#[path="recorder_keystrokes.rs"]mod keystrokes;
#[tauri::command]pub fn recorder_zoom_preview_frame(after:u64)->Option<serde_json::Value>{zoom_preview::frame(after)}
#[tauri::command]pub async fn recorder_zoom_preview_hide(app:tauri::AppHandle)->Res<()>{zoom_preview::hide(app).await}
#[cfg(test)]#[path="recorder_media_tests.rs"]mod media_tests;
type Res<T>=Result<T,String>;

#[derive(Clone,Serialize,Deserialize)]
#[serde(default)]
pub struct Config {pub shortcut:String,pub output_dir:String,pub fps:u32,pub quality:String,pub resolution:String,pub encoder:String,pub microphone:bool,pub system_audio:bool,pub microphone_id:String,pub mic_gain:f64,pub system_gain:f64,pub voice_enhance:bool,pub cursor:String,pub cursor_size:u32,pub cursor_color:String,pub click_effect:bool,pub cursor_effect:String,pub click_style:String,pub effect_strength:f64,pub frame:String,pub background:String,pub frame_padding:u32,pub countdown:u32,pub auto_fade:bool,pub smooth_cursor:bool,pub cinematic_cursor:bool,pub kenney_cursor:bool,pub show_keystrokes:bool,pub keystroke_theme:String,pub interaction_sounds:bool,pub background_music:bool,pub zoom_preview:bool,pub smart_zoom:bool,pub smart_zoom_amount:f64}
impl Default for Config {fn default()->Self{Self{shortcut:"Ctrl+Alt+R".into(),output_dir:std::env::var("USERPROFILE").map(|p|PathBuf::from(p).join("Videos").join("Halo").to_string_lossy().into_owned()).unwrap_or_else(|_|".".into()),fps:30,quality:"high".into(),resolution:"original".into(),encoder:"auto".into(),microphone:true,system_audio:false,microphone_id:String::new(),mic_gain:1.,system_gain:1.,voice_enhance:true,cursor:"normal".into(),cursor_size:unsafe{GetSystemMetrics(SM_CXCURSOR).clamp(16,96)as u32},cursor_color:"#FFB020".into(),click_effect:false,cursor_effect:"none".into(),click_style:"moderate".into(),effect_strength:0.65,frame:"none".into(),background:"preset:aurora".into(),frame_padding:5,countdown:3,auto_fade:true,smooth_cursor:false,cinematic_cursor:false,kenney_cursor:false,show_keystrokes:false,keystroke_theme:"dark".into(),interaction_sounds:false,background_music:false,zoom_preview:false,smart_zoom:false,smart_zoom_amount:2.}}}
fn config_path()->PathBuf {crate::system::tools_dir().parent().unwrap().join("recorder.json")}
pub fn config()->Config{let mut c:Config=fs::read(config_path()).ok().and_then(|d|serde_json::from_slice(&d).ok()).unwrap_or_default();if c.cinematic_cursor{c.smooth_cursor=false;}if !(2..=30).contains(&c.frame_padding){c.frame_padding=Config::default().frame_padding;}if !["weak","moderate","strong"].contains(&c.click_style.as_str()){c.click_style=Config::default().click_style;}c}
fn validate(c:&Config)->Res<()> {
 if c.smooth_cursor&&c.cinematic_cursor{return Err("Choose either Smooth Cursor or Cinematic Cursor.".into());}
 if !["light","dark"].contains(&c.keystroke_theme.as_str())||!c.smart_zoom_amount.is_finite()||!(1.1..=2.).contains(&c.smart_zoom_amount)||![30,60].contains(&c.fps)||!["high","balanced","archive"].contains(&c.quality.as_str())||!["original","1080p","720p"].contains(&c.resolution.as_str())||!["auto","cpu"].contains(&c.encoder.as_str())||!["normal","hidden","highlight","large"].contains(&c.cursor.as_str())||!["none","mac","windows"].contains(&c.frame.as_str())||!(16..=96).contains(&c.cursor_size)||!(2..=30).contains(&c.frame_padding)||c.countdown>10||!["none","spotlight","trail"].contains(&c.cursor_effect.as_str())||!["weak","moderate","strong"].contains(&c.click_style.as_str())||!c.effect_strength.is_finite()||!(0.1..=1.).contains(&c.effect_strength)||![c.mic_gain,c.system_gain].iter().all(|v|v.is_finite()&&(0.0..=2.0).contains(v)){return Err("One of the recording preferences is outside its supported range.".into());}
 let hex=|color:&str|color.len()==7&&color.starts_with('#')&&color[1..].chars().all(|v|v.is_ascii_hexdigit());
 if !hex(&c.cursor_color)||!(hex(&c.background)||background_preset(c).is_some()){return Err("Choose a valid RGB colour.".into());}
 if !Path::new(&c.output_dir).is_absolute(){return Err("Choose an absolute save folder.".into());}Ok(())
}
#[derive(Clone,Copy,Serialize,Deserialize)]pub struct Rect {pub x:i32,pub y:i32,pub w:u32,pub h:u32}
#[derive(Clone,Serialize,Deserialize)]pub struct Target {pub mode:String,pub monitor:u64,pub window:u64,pub rect:Option<Rect>}
#[derive(Clone,Serialize)]pub struct Monitor {pub id:u64,pub name:String,pub x:i32,pub y:i32,pub w:u32,pub h:u32,pub scale:f64}
#[derive(Serialize)]pub struct Window {pub id:u64,pub name:String,pub w:u32,pub h:u32,pub minimized:bool}
#[derive(Clone,Serialize,Default)]pub struct State {pub phase:String,pub export_id:Option<String>,pub elapsed:f64,pub output:Option<String>,pub original_output:Option<String>,pub project_path:Option<String>,pub error:Option<String>,pub mic_peak:f32,pub system_peak:f32,pub preview_active:bool,pub save_percent:f64,pub save_stage:String,pub target:Option<Rect>,pub microphone:Option<bool>,pub system_audio:Option<bool>}
static STATE:Mutex<Option<State>>=Mutex::new(None);
static SESSION:Mutex<Option<Session>>=Mutex::new(None);
static OP:Mutex<()>=Mutex::new(());
static CANCEL:AtomicBool=AtomicBool::new(false);
static SAVE_PROGRESS:AtomicU32=AtomicU32::new(0);
// 0 original, 1 visual preparation, 2 encoder startup, 3 encoding,
// 4 container finalization, 5 publishing the completed file.
static SAVE_STAGE:AtomicU32=AtomicU32::new(0);
fn save_stage(stage:u32){SAVE_STAGE.store(stage,Ordering::Relaxed);if stage!=2{save_progress(0.);}}
fn save_stage_name()->&'static str{match SAVE_STAGE.load(Ordering::Relaxed){1|2=>"preparing",3=>"encoding",4|5=>"finalizing",_=>"saving"}}
fn save_progress(value:f64){SAVE_PROGRESS.store((value.clamp(0.,100.)*100.)as u32,Ordering::Relaxed);}
static PREVIEW:Mutex<Option<AudioPreview>>=Mutex::new(None);
struct AudioPreview {microphone:bool,system_audio:bool,stop:Arc<AtomicBool>,tracks:Vec<AudioTrack>,mic:Arc<AtomicU32>,system:Arc<AtomicU32>}
fn stop_preview(){if let Some(p)=PREVIEW.lock().unwrap().take(){p.stop.store(true,Ordering::Relaxed);for track in p.tracks{let _=export::join_audio(track);}}}
#[tauri::command]pub async fn recorder_preview_stop()->Res<()> {tauri::async_runtime::spawn_blocking(||{let _op=OP.lock().unwrap();stop_preview();}).await.map_err(|e|e.to_string())?;Ok(())}
#[tauri::command]pub async fn recorder_preview_start(config:Config)->Res<()> {tauri::async_runtime::spawn_blocking(move||{let _op=OP.lock().unwrap();stop_preview();if SESSION.lock().unwrap().is_some(){return Ok(());}let stop=Arc::new(AtomicBool::new(false));let mic=Arc::new(AtomicU32::new(0));let system=Arc::new(AtomicU32::new(0));let mut tracks=vec![];for (enabled,is_mic) in [(config.microphone,true),(config.system_audio,false)]{if enabled{match crate::recorder_audio::meter(is_mic,config.microphone_id.clone(),stop.clone(),if is_mic{mic.clone()}else{system.clone()}){Ok(track)=>tracks.push(track),Err(error)=>{log_error(&format!("Audio preview unavailable: {error}"));}}}}*PREVIEW.lock().unwrap()=Some(AudioPreview{microphone:config.microphone,system_audio:config.system_audio,stop,tracks,mic,system});Ok(())}).await.map_err(|e|e.to_string())?}

static SELECTOR:Mutex<Option<Selection>>=Mutex::new(None);
struct Selection {monitor:Monitor,data:String}
struct Running {child:Child,stop:Arc<AtomicBool>,gate:Arc<Mutex<Option<Instant>>>,audio:Vec<AudioTrack>,pointer:Option<thread::JoinHandle<()>>,preview:Option<thread::JoinHandle<()>>,raw:PathBuf,commands:PathBuf,mic_failed:bool,system_failed:bool}
struct Segment {raw:PathBuf,audio:Vec<(PathBuf,bool)>,commands:PathBuf,duration:f64}
struct Session {config:Config,target:Target,bounds:Rect,dir:PathBuf,output:PathBuf,project:Option<PathBuf>,original_output:Option<PathBuf>,segments:Vec<Segment>,running:Option<Running>,encoder:Encoder,mic_peak:Arc<AtomicU32>,sys_peak:Arc<AtomicU32>,mic_muted:Arc<AtomicBool>,sys_muted:Arc<AtomicBool>,annotations:Vec<crate::recorder_annotations::Annotation>}
fn phase(value:&str,error:Option<String>){let mut state=STATE.lock().unwrap();let s=state.get_or_insert_with(State::default);s.phase=value.into();s.error=error;}
fn ffmpeg()->std::process::Command{let mut c=std::process::Command::new(runtime::path());c.args(["-hide_banner","-loglevel","error","-y","-protocol_whitelist","file,pipe"]);c.creation_flags(HIDDEN);c}
fn run(command:std::process::Command)->Res<()> {run_command(command,None,None)}
fn run_progress(mut command:std::process::Command,duration:f64,from:f64,to:f64)->Res<()> {command.args(["-progress","pipe:1","-stats_period","0.1"]);run_command(command,None,Some((duration.max(0.1),from,to)))}
fn run_command(command:std::process::Command,timeout:Option<Duration>,progress:Option<(f64,f64,f64)>)->Res<()> {
 export::run_command(command,timeout,progress)
}

fn escaped(path:&Path)->String {path.to_string_lossy().replace('\\',"/").replace(':',"\\:").replace('\'',"\\'")}
pub fn monitors(app:&tauri::AppHandle)->Res<Vec<Monitor>> {
 app.available_monitors().map_err(|e|e.to_string()).map(|list|list.into_iter().enumerate().map(|(i,m)|{let p=m.position();let s=m.size();let id=unsafe{MonitorFromPoint(POINT{x:p.x+s.width as i32/2,y:p.y+s.height as i32/2},MONITOR_DEFAULTTONEAREST).0 as u64};Monitor{id,name:m.name().cloned().unwrap_or_else(||format!("Display {}",i+1)),x:p.x,y:p.y,w:s.width,h:s.height,scale:m.scale_factor()}}).collect())
}
/// Centre native pixels without resampling; larger windows are clipped to the fixed canvas.
fn window_offset(width:u32,height:u32,source_width:u32,source_height:u32)->(i32,i32){
 ((width.saturating_sub(source_width)/2)as i32,(height.saturating_sub(source_height)/2)as i32)
}
fn annotation_viewport(job:&Session,r:Rect)->serde_json::Value{
 if job.target.mode!="window"{return serde_json::json!({"x":0,"y":0,"w":job.bounds.w,"h":job.bounds.h});}
 let (x,y)=window_offset(job.bounds.w,job.bounds.h,r.w,r.h);
 serde_json::json!({"x":x,"y":y,"w":r.w,"h":r.h})
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
static RECORDER_READY:AtomicBool=AtomicBool::new(false);
#[tauri::command]pub fn recorder_widget_ready(app:tauri::AppHandle)->Res<()> {
 if let Some(w)=app.get_webview_window("recorder"){w.show().map_err(|e|e.to_string())?;RECORDER_READY.store(true,Ordering::Relaxed);}
 Ok(())
}
pub fn open(app:&tauri::AppHandle){let app=app.clone();thread::spawn(move||{let _=countdown_window(&app);if let Some(w)=app.get_webview_window("recorder"){if RECORDER_READY.load(Ordering::Relaxed){let _=w.show();let _=w.set_focus();}let _=app.emit_to("recorder","halo://recorder-opened",());return;}RECORDER_READY.store(false,Ordering::Relaxed);if let Ok(w)=tauri::WebviewWindowBuilder::new(&app,"recorder",tauri::WebviewUrl::App("recorder.html".into())).title("Halo · Screen Recording").inner_size(615.,84.).visible(false).transparent(true).decorations(false).always_on_top(true).skip_taskbar(true).resizable(false).shadow(false).build(){exclude(&w);if let Ok(Some(m))=app.primary_monitor(){let scale=m.scale_factor();let p=m.position();let _=w.set_position(PhysicalPosition::new(p.x+(m.size().width as i32-(615.*scale) as i32)/2,p.y+(24.*scale) as i32));}}});}
#[tauri::command]pub async fn open_recorder(app:tauri::AppHandle){open(&app);}
#[tauri::command]pub async fn recorder_options(app:tauri::AppHandle)->Res<serde_json::Value>{let displays=monitors(&app)?;tauri::async_runtime::spawn_blocking(move||{let mut windows=Vec::<Window>::new();unsafe{let _=EnumWindows(Some(enum_window),windows::Win32::Foundation::LPARAM(&mut windows as *mut _ as isize));}Ok(serde_json::json!({"config":config(),"native_cursor_size":unsafe{GetSystemMetrics(SM_CXCURSOR).clamp(16,96)},"monitors":displays,"windows":windows,"microphones":crate::recorder_audio::devices()}))}).await.map_err(|e|e.to_string())?}
#[tauri::command]pub async fn recorder_save_config(app:tauri::AppHandle,config:Config)->Res<()> {
 validate(&config)?;use tauri_plugin_global_shortcut::GlobalShortcutExt;let old=self::config();if old.shortcut!=config.shortcut {let wanted:tauri_plugin_global_shortcut::Shortcut=config.shortcut.parse().map_err(|_|"Choose a valid recorder shortcut.")?;app.global_shortcut().register(wanted).map_err(|_|"That recorder shortcut is already in use.")?;let _=app.global_shortcut().unregister(old.shortcut.as_str());}
 let path=config_path();fs::create_dir_all(path.parent().unwrap()).map_err(|e|e.to_string())?;fs::write(path,serde_json::to_vec_pretty(&config).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;let _=app.emit("halo://recorder-config",&config);Ok(())
}
#[tauri::command]pub async fn recorder_pick_folder(app:tauri::AppHandle)->Option<String>{use tauri_plugin_dialog::DialogExt;tauri::async_runtime::spawn_blocking(move||app.dialog().file().blocking_pick_folder().and_then(|p|p.into_path().ok()).map(|p|p.to_string_lossy().into_owned())).await.ok().flatten()}
#[tauri::command]pub fn recorder_state()->State {
 let mut s=STATE.lock().unwrap().clone().unwrap_or_else(||State{phase:"idle".into(),..Default::default()});
 if let Ok(guard)=SESSION.try_lock(){if let Some(job)=guard.as_ref(){s.elapsed=job.segments.iter().map(|s|s.duration).sum();if let Some(r)=&job.running {if let Some(start)=*r.gate.lock().unwrap(){s.elapsed+=start.elapsed().as_secs_f64();}}s.mic_peak=f32::from_bits(job.mic_peak.load(Ordering::Relaxed));s.system_peak=f32::from_bits(job.sys_peak.load(Ordering::Relaxed));s.microphone=Some(job.config.microphone);s.system_audio=Some(job.config.system_audio);}}if matches!(s.phase.as_str(),"saving"|"exporting"){s.save_percent=SAVE_PROGRESS.load(Ordering::Relaxed)as f64/100.;s.save_stage=save_stage_name().into();}jobs::foreground(&mut s);if matches!(s.phase.as_str(),"idle"|"exporting"){if let Ok(p)=PREVIEW.try_lock(){if let Some(p)=p.as_ref(){s.mic_peak=f32::from_bits(p.mic.load(Ordering::Relaxed));s.system_peak=f32::from_bits(p.system.load(Ordering::Relaxed));s.preview_active=true;s.microphone=if p.microphone{Some(p.tracks.iter().any(|t|t.mic))}else{None};s.system_audio=if p.system_audio{Some(p.tracks.iter().any(|t|!t.mic))}else{None};}}}s
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


fn video_settings(command:&mut std::process::Command,c:&Config,encoder:Encoder){encoder.settings(command,c,false);}
// Lossless synthetic media used by pixel-level export tests.
#[cfg(test)]
fn intermediate_video_settings(command:&mut std::process::Command,gpu:bool){
 if gpu{command.args(["-c:v","h264_nvenc","-preset","p1","-tune","lossless","-pix_fmt","yuv444p"]);}else{command.args(["-c:v","libx264rgb","-preset","ultrafast","-crf","0","-pix_fmt","rgb24"]);}
}
fn capture_video_settings(command:&mut std::process::Command,c:&Config,encoder:Encoder){encoder.settings(command,c,true);}
fn start_segment(job:&mut Session)->Res<Running>{
 runtime::require()?;
 let control=export::Control::default();
 for candidate in encoder::candidates(&job.config,job.encoder){
  if control.cancelled(){return Err("Recording cancelled.".into());}
  if !encoder::probe(candidate,&job.config,job.bounds.w,job.bounds.h,true,&control){continue;}
  job.encoder=candidate;
  match start_segment_with_encoder(job){
   Ok(running)=>return Ok(running),
   Err(error)=>{
    let diagnostic=fs::read_to_string(job.dir.join(format!("capture-{}.log",job.segments.len()))).unwrap_or_default();
    if candidate==Encoder::Cpu||!encoder::hardware_error(&diagnostic)||control.cancelled(){return Err(error);}
    log_error(&format!("{} capture unavailable: {diagnostic}",candidate.name()));
   }
  }
 }
 Err("No video encoder could start. Check the graphics driver and FFmpeg installation.".into())
}
fn start_segment_with_encoder(job:&Session)->Res<Running>{
 if CANCEL.load(Ordering::Relaxed){return Err("Recording cancelled.".into());}
 let i=job.segments.len();let capture=project::assets_dir(job);let raw=capture.join(format!("segment-{i}.mp4"));let commands=job.dir.join(format!("cursor-{i}.txt"));let gate=Arc::new(Mutex::new(None::<Instant>));let stop=Arc::new(AtomicBool::new(false));let mut audio=Vec::new();
 // A failed audio device (disconnected mic, no capture device on this machine, etc.) shouldn't
 // block video recording entirely; skip that one track, keep going without it, and report it so
 // the UI can turn the toggle off instead of showing it as enabled while silently not recording.
 let (mut mic_failed,mut system_failed)=(false,false);
 for (enabled,mic) in [(job.config.microphone,true),(job.config.system_audio,false)] {if enabled {let path=capture.join(format!("{}-{i}.wav",if mic{"mic"}else{"system"}));match crate::recorder_audio::start(path,mic,job.config.microphone_id.clone(),stop.clone(),gate.clone(),if mic{job.mic_peak.clone()}else{job.sys_peak.clone()},if mic{job.mic_muted.clone()}else{job.sys_muted.clone()}){Ok(track)=>audio.push(track),Err(e)=>{log_error(&format!("{} capture unavailable, recording without it: {e}",if mic{"Microphone"}else{"System audio"}));if mic{mic_failed=true}else{system_failed=true}}}}}
 let native=native_cursor(&job.config);let target=&job.target;
 let mut source=if target.mode=="window"{format!("gfxcapture=hwnd={}:width={}:height={}:resize_mode=crop",target.window,job.bounds.w,job.bounds.h)}else{format!("gfxcapture=hmonitor={}",target.monitor)};
 if target.mode=="area" {let r=target.rect.unwrap();let monitors=unsafe{MonitorFromPoint(POINT{x:job.bounds.x,y:job.bounds.y},MONITOR_DEFAULTTONEAREST)};let mut info=windows::Win32::Graphics::Gdi::MONITORINFO{cbSize:std::mem::size_of::<windows::Win32::Graphics::Gdi::MONITORINFO>() as u32,..Default::default()};unsafe{let _=windows::Win32::Graphics::Gdi::GetMonitorInfoW(monitors,&mut info);}let w=(info.rcMonitor.right-info.rcMonitor.left) as u32;let h=(info.rcMonitor.bottom-info.rcMonitor.top) as u32;source+=&format!(":crop_left={}:crop_top={}:crop_right={}:crop_bottom={}",r.x,r.y,w-r.x as u32-r.w,h-r.y as u32-r.h);}
 // Acquire at twice the output rate: a 60Hz minimum capture interval on a
 // 144Hz desktop otherwise supplies about 48 unique pictures per second.
 // Sample to the requested rate before downloading or encoding pixels.
 source+=&format!(":capture_cursor={}:max_framerate={}",u8::from(native),job.config.fps*2);
 if CANCEL.load(Ordering::Relaxed){stop.store(true,Ordering::Relaxed);for track in audio{let _=track.join.join();}return Err("Recording cancelled.".into());}
 // WGC can initially repeat a synthetic black frame. Warm up before starting
 // the shared input/audio clock, then keep native pixels and exact output FPS.
 // Start the shared clock at the first saved picture, before resampling or
 // either CPU or GPU encoding can queue it. Reset PTS to that same picture.
 let preview_epoch=zoom_preview::begin(&job.config,job.bounds);let filter=format!("setpts=PTS-STARTPTS,trim=start=0.15,setpts=PTS-STARTPTS,showinfo=checksum=0,fps={}:eof_action=pass,hwdownload,format=bgra,pad=ceil(iw/2)*2:ceil(ih/2)*2:0:0:black",job.config.fps);
 let mut c=ffmpeg();c.args(["-loglevel","info","-nostats","-f","lavfi","-i",&source]);if preview_epoch.is_some(){c.args(["-filter_complex_threads","2","-filter_complex",&format!("[0:v]{filter},split=2[recording][small];[small]fps=10,scale=480:-2:flags=fast_bilinear,format=yuvj420p[preview]"),"-map","[recording]"]);}else{c.args(["-vf",&filter]);}c.args(["-an","-r",&job.config.fps.to_string()]);capture_video_settings(&mut c,&job.config,job.encoder);c.args(["-movflags","+frag_keyframe+empty_moov+default_base_moof"]).arg(&raw);if preview_epoch.is_some(){c.args(["-map","[preview]","-an","-c:v","mjpeg","-q:v","5","-threads","1","-f","image2pipe","pipe:1"]);}c.stdin(Stdio::piped()).stdout(if preview_epoch.is_some(){Stdio::piped()}else{Stdio::null()}).stderr(Stdio::piped());
 let mut child=match c.creation_flags(HIDDEN).spawn(){Ok(child)=>child,Err(e)=>{stop.store(true,Ordering::Relaxed);for track in audio{let _=track.join.join();}return Err(e.to_string());}};let preview=preview_epoch.and_then(|epoch|child.stdout.take().map(|pipe|zoom_preview::reader(pipe,epoch)));let stderr=child.stderr.take().unwrap();let signal=gate.clone();let diagnostic=job.dir.join(format!("capture-{i}.log"));
 let diagnostics=thread::spawn(move||{let mut log=fs::File::create(diagnostic).ok();for line in BufReader::new(stderr).lines().map_while(Result::ok){
  if line.contains("showinfo"){
   if line.contains(" n:"){let mut g=signal.lock().unwrap();if g.is_none(){*g=Some(Instant::now());if let Some(log)=log.as_mut(){let _=writeln!(log,"{line}");}}}
  }else if let Some(log)=log.as_mut(){let _=writeln!(log,"{line}");}
 }});
 let start=Instant::now();while gate.lock().unwrap().is_none(){if CANCEL.load(Ordering::Relaxed)||child.try_wait().map_err(|e|e.to_string())?.is_some()||start.elapsed()>Duration::from_secs(12){stop.store(true,Ordering::Relaxed);let _=child.kill();let _=child.wait();for t in audio{let _=t.join.join();}let _=diagnostics.join();return Err("The capture source did not produce video. Check the selected window and graphics driver.".into());}thread::sleep(Duration::from_millis(30));}
 // Keep semantic input events for future editing even when no cursor effect
 // is currently enabled. Only create FFmpeg commands when effects need them.
 let stopped=stop.clone();let clock=gate.clone();let target=target.clone();let bounds=job.bounds;
 let file=commands.clone();let events_path=capture.join(format!("cursor-{i}.jsonl"));let config=job.config.clone();
 let pointer=Some(thread::spawn(move||{
  unsafe{windows::Win32::Media::timeBeginPeriod(1);}
  let wheel=cursor_motion::WheelCapture::new(config.smart_zoom||config.show_keystrokes||config.interaction_sounds);
  let mut context=context::Sampler::new(config.smart_zoom||config.show_keystrokes,&target);
  let mut keys=keystrokes::Capture::new(config.show_keystrokes,&target);
  let result=(||->Res<()>{
   let mut key_file=if config.show_keystrokes{Some(std::io::BufWriter::new(fs::File::create(events_path.with_file_name(format!("keys-{i}.jsonl"))).map_err(|e|e.to_string())?))}else{None};
   let mut events=std::io::BufWriter::new(fs::File::create(events_path).map_err(|e|e.to_string())?);
   let mut commands=if project::cursor_effects(&config)||target.mode=="window"{Some(std::io::BufWriter::new(fs::File::create(&file).map_err(|e|e.to_string())?))}else{None};
   let mut program=project::CursorProgram::new(&config,bounds.w,bounds.h);let mut last=-1.;
   let initial=project::CursorSample{context:None,wheel:0,shape:0,window_offset:None,time:0.,x:-200,y:-200,inside:false,buttons:0};serde_json::to_writer(&mut events,&initial).map_err(|e|e.to_string())?;writeln!(events).map_err(|e|e.to_string())?;if let Some(commands)=commands.as_mut(){program.push(&initial,commands).map_err(|e|e.to_string())?;}
   while !stopped.load(Ordering::Relaxed){
    let time=clock.lock().unwrap().map(|g|g.elapsed().as_secs_f64()).unwrap_or(0.);
    if time-last<0.005{thread::sleep(Duration::from_millis(1));continue;}last=time;
    let r=if target.mode=="window"{window_rect(target.window).unwrap_or(bounds)}else{bounds};let mut pointer=POINT::default();
    let buttons=unsafe{let _=GetCursorPos(&mut pointer);u8::from((GetAsyncKeyState(0x01)as u16&0x8000)!=0)|(u8::from((GetAsyncKeyState(0x02)as u16&0x8000)!=0)<<1)};
    let (ox,oy)=if target.mode=="window"{window_offset(bounds.w,bounds.h,r.w,r.h)}else{(0,0)};
    let inside=pointer.x>=r.x&&pointer.y>=r.y&&pointer.x<r.x+r.w as i32&&pointer.y<r.y+r.h as i32;
    let (x,y)=(pointer.x-r.x+ox,pointer.y-r.y+oy);
    let inside=inside&&x>=0&&y>=0&&x<bounds.w as i32&&y<bounds.h as i32;
    if let Some(file)=key_file.as_mut(){keys.drain(time,file)?;}
    let sample=project::CursorSample{context:context.sample(bounds,r,(ox,oy)).or_else(||if config.interaction_sounds&&!config.smart_zoom&&!config.show_keystrokes{context::take_activity()}else{None}),wheel:wheel.take(),shape:cursor_motion::native_shape(),window_offset:if target.mode=="window"{Some([ox,oy])}else{None},time,x,y,inside,buttons};
    zoom_preview::sample(&sample);
    serde_json::to_writer(&mut events,&sample).map_err(|e|e.to_string())?;writeln!(events).map_err(|e|e.to_string())?;
    if let Some(commands)=commands.as_mut(){program.push(&sample,commands).map_err(|e|e.to_string())?;}
    thread::sleep(Duration::from_millis(2));
   }
   if let Some(file)=key_file.as_mut(){let time=clock.lock().unwrap().map(|g|g.elapsed().as_secs_f64()).unwrap_or(0.);keys.finish(time,file)?;file.flush().map_err(|e|e.to_string())?;}
   events.flush().map_err(|e|e.to_string())?;if let Some(commands)=commands.as_mut(){commands.flush().map_err(|e|e.to_string())?;}Ok(())
  })();
  if let Err(error)=result{log_error(&format!("Cursor source data could not be saved: {error}"));}
  unsafe{windows::Win32::Media::timeEndPeriod(1);}
 }));

 Ok(Running{child,stop,gate,audio,pointer,preview,raw,commands,mic_failed,system_failed})
}
fn finish_segment(mut running:Running,fps:u32)->Res<Segment>{
 let duration=running.gate.lock().unwrap().map(|t|t.elapsed().as_secs_f64()).unwrap_or(0.).max(0.1);
 let duration=(duration*fps as f64).ceil()/fps as f64;
 running.stop.store(true,Ordering::Relaxed);
 if let Some(mut input)=running.child.stdin.take(){let _=input.write_all(b"q\n");}
 let start=Instant::now();
 loop{
  if running.child.try_wait().map_err(|e|e.to_string())?.is_some(){break;}
  if start.elapsed()>Duration::from_secs(12){let _=running.child.kill();let _=running.child.wait();log_error("Capture shutdown exceeded 12 seconds; recovering fragmented capture.");break;}
  thread::sleep(Duration::from_millis(40));
 }
 if let Some(p)=running.pointer{let _=p.join();}if let Some(p)=running.preview{let _=p.join();}
 let mut paths=Vec::new();
 for track in running.audio{match export::join_audio(track){Ok(path)=>paths.push(path),Err(error)=>{
  log_error(&error);if let Some(state)=STATE.lock().unwrap().as_mut(){state.error=Some("An audio device stopped responding. The video will be saved without that audio track.".into());}
 }}}
 Ok(Segment{raw:running.raw,audio:paths,commands:running.commands,duration})
}
fn render_png(svg:&str,path:&Path)->Res<()> {let mut options=resvg::usvg::Options::default();if svg.contains("<text"){static FONTS:std::sync::OnceLock<Arc<resvg::usvg::fontdb::Database>>=std::sync::OnceLock::new();options.fontdb=FONTS.get_or_init(||{let mut database=resvg::usvg::fontdb::Database::new();database.load_system_fonts();database.load_font_data(include_bytes!("../../src/assets/fonts/Kalam-Regular.ttf").to_vec());Arc::new(database)}).clone();}let tree=resvg::usvg::Tree::from_str(svg,&options).map_err(|e|e.to_string())?;let size=tree.size().to_int_size();let mut pix=resvg::tiny_skia::Pixmap::new(size.width(),size.height()).ok_or("The recording frame is too large.")?;resvg::render(&tree,resvg::tiny_skia::Transform::identity(),&mut pix.as_mut());pix.save_png(path).map_err(|e|e.to_string())}
fn native_cursor(c:&Config)->bool{!c.kenney_cursor&&!cursor_motion::processed(c)&&c.frame!="mac"&&["normal","highlight"].contains(&c.cursor.as_str())}
fn cursor_svg(c:&Config)->String {if c.kenney_cursor&&c.cursor!="hidden"{return format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" viewBox="0 0 64 64">{}</svg>"#,c.cursor_size,c.cursor_size,cursor_motion::pack_style(c,0));}
 let mac=c.frame=="mac"||cursor_motion::processed(c);let arrow=if mac{"M5 3V52L17 40L27 60L36 56L27 36H45Z"}else{"M5 3V50L17 38L27 59L36 55L27 35H45Z"};
 let original=mac&&c.cursor=="normal"&&!cursor_motion::processed(c);let pointer=|fill:&str|{let path=format!(r##"<path d="{arrow}" fill="{fill}" stroke="{}" stroke-width="2.4" stroke-linejoin="round"/>"##,if mac{"#ffffff"}else{"#14243c"});if original{format!(r#"<g transform="translate(5 3) scale(.62) translate(-5 -3)">{path}</g>"#)}else{path}};
 let graphic=match c.cursor.as_str(){"large"=>pointer(&c.cursor_color),"normal" if mac||cursor_motion::processed(c)=>pointer(if mac{"#15171c"}else{"#ffffff"}),"highlight"=>format!(r#"<circle cx="32" cy="32" r="29" fill="{}" fill-opacity=".18" stroke="{}" stroke-opacity=".65" stroke-width="1.5"/>{}"#,c.cursor_color,c.cursor_color,if mac||cursor_motion::processed(c){format!("<g transform=\"translate(29.5 30.5) scale(.5)\">{}</g>",pointer("#15171c"))}else{String::new()}),_=>String::new()};
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
#[cfg(test)]
fn finish(job:&Session)->Res<(PathBuf,Option<PathBuf>)>{
 finish_controlled(job,export::Control::default())
}
fn finish_controlled(job:&Session,control:export::Control)->Res<(PathBuf,Option<PathBuf>)>{
 if !export::needs_video_render(job){if let Some(fx)=soundtrack::render(job,None,&control)?{let source=job.original_output.clone().unwrap_or_else(||job.dir.join("audio-delivery-source.mp4"));if !source.exists(){export::save_original(job,&source)?;}let duration=job.segments.iter().map(|s|s.duration).sum::<f64>();export::concat_with_soundtrack(&[source.clone()],&job.output,&job.dir.join("audio-delivery.txt"),duration,Some(&source),job.segments.iter().any(|s|!s.audio.is_empty()),&fx,job.config.auto_fade,&control)?;return Ok((job.output.clone(),job.original_output.clone()));}}

 let (width,height)=encoder::output_size(&job.config,job.bounds.w,job.bounds.h);
 encoder::export_with_fallback(&job.config,job.encoder,&control,
  |encoder|encoder::probe(encoder,&job.config,width,height,false,&control),
  |encoder|finish_partitioned_controlled(job,render::worker_limit(encoder.hardware()),job.config.fps as u64*15,control.clone(),encoder))
}
#[cfg(test)]
fn finish_partitioned(job:&Session,workers:usize,frames_per_part:u64)->Res<(PathBuf,Option<PathBuf>)>{finish_partitioned_controlled(job,workers,frames_per_part,export::Control::default(),job.encoder)}
fn finish_partitioned_controlled(job:&Session,workers:usize,frames_per_part:u64,control:export::Control,encoder:Encoder)->Res<(PathBuf,Option<PathBuf>)>{
 cinematic::prepare_cursor(job,&control)?;
 let camera=cinematic::plan(job,&control)?;let scroll=cinematic::ScrollTrack::plan(job,&control)?;
 let parts=render::parts(job,frames_per_part);let parts=if let Some(camera)=&camera{camera.split_parts(parts)}else{parts};let key_track=keystrokes::Track::load(job,&control)?;let preparation=export::PreparationProgress::new(job,&parts,control.clone(),if key_track.is_some(){parts.len()}else{0});let mut assets=render::Assets::new();let mut prepared=Vec::new();
 let total_duration=job.segments.iter().map(|s|s.duration).sum::<f64>().max(0.1);let mut completed=Vec::new();let c=&job.config;let size=c.cursor_size;let color=&c.cursor_color;
 let pointer=job.dir.join("pointer.png");let click=job.dir.join("click.png");
 assets.push(cursor_motion::atlas(c),pointer.clone());
 assets.push(click_sprite(c),click.clone());
 let spot_size=(size*6).min(job.bounds.w.min(job.bounds.h)).max(2);let mask=job.dir.join("spotlight-mask.png");let trail=job.dir.join("trail.png");
 if c.cursor_effect=="spotlight"{assets.push(format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="{spot_size}" height="{spot_size}"><defs><radialGradient id="soft"><stop offset="65%" stop-color="white"/><stop offset="100%" stop-color="black"/></radialGradient></defs><rect width="100%" height="100%" fill="url(#soft)"/></svg>"#),mask.clone());}
 if c.cursor_effect=="trail"{assets.push(format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="{size}" height="{size}" viewBox="0 0 64 64"><defs><radialGradient id="soft"><stop stop-color="{color}" stop-opacity=".7"/><stop offset="100%" stop-color="{color}" stop-opacity="0"/></radialGradient></defs><circle cx="32" cy="32" r="29" fill="url(#soft)"/></svg>"#),trail.clone());}

 for (i,part) in parts.iter().enumerate(){let segment=part.segment;let segment_offset=part.offset;let mut moving_camera=false;
  let result=job.dir.join(format!("finished-{i}.mp4"));let mut cmd=ffmpeg();let filter_threads=thread::available_parallelism().map(|n|(n.get()/workers.max(1)).clamp(1,4)).unwrap_or(1).to_string();cmd.args(["-filter_complex_threads",&filter_threads]);cmd.args(["-ss",&format!("{:.9}",part.start),"-t",&format!("{:.9}",part.duration+2./c.fps as f64)]).arg("-i").arg(&segment.raw);let effects=segment.commands.exists();let mut next=1;let scroll_filter=scroll.filter(part.offset,part.duration).map(|f|format!(",{f}")).unwrap_or_default();let mut filters=vec![format!("[0:v]fps={0},tpad=stop_mode=clone:stop_duration={1},trim=duration={1}{scroll_filter}[base]",c.fps,part.duration)];let mut video="base".to_string();
  if effects {
   cmd.args(["-framerate",&c.fps.to_string(),"-loop","1","-i"]).arg(&pointer);cmd.args(["-framerate",&c.fps.to_string(),"-loop","1","-i"]).arg(&click);next=3;
   let main_commands=job.dir.join(format!("cursor-main-{i}.txt"));let mut main_text=String::new();let mut click_text=(0..CLICK_SLOTS).map(|n|format!("0 crop@clickframe{n} x 0;\n")).collect::<Vec<_>>();
   for line in BufReader::new(fs::File::open(&segment.commands).map_err(|e|e.to_string())?).lines(){let line=line.map_err(|e|e.to_string())?;if let Some(n)=(0..CLICK_SLOTS).find(|n|line.contains(&format!("crop@clickframe{n} "))||line.contains(&format!("colorchannelmixer@clickalpha{n} "))){click_text[n].push_str(&line);click_text[n].push('\n');}else{main_text.push_str(&line);main_text.push('\n');}}
   fs::write(&main_commands,export::frame_commands_range(&main_text,c.fps,part.start,part.duration,&control)?).map_err(|e|e.to_string())?;
   let (fit,input)=if main_text.contains("overlay@window "){("[commands]split[windowcontent][windowcanvas];[windowcanvas]drawbox=color=black:t=fill[windowblack];[windowblack][windowcontent]overlay@window=x=0:y=0:shortest=1:format=auto[windowcenter];","windowcenter")}else{("","commands")};
   let pointer_input=if cursor_motion::enabled(c){format!("[1:v]format=rgba,crop@pointerframe=w={0}:h={0}:x=0:y={1}:exact=1[pointerinput]",cursor_motion::cell(c),12*cursor_motion::cell(c))}else{"[1:v]null[pointerinput]".into()};
   filters.push(pointer_input);
   filters.push(format!("[base]sendcmd=f='{}'[commands];{fit}[{input}][pointerinput]overlay@pointer=x=-200:y=-200:shortest=1:format=auto[pointer];[2:v]split={CLICK_SLOTS}{}",escaped(&main_commands),(0..CLICK_SLOTS).map(|n|format!("[clicksheet{n}]")).collect::<String>()));
   video="pointer".into();
   for n in 0..CLICK_SLOTS{let timeline=job.dir.join(format!("click-{i}-{n}.txt"));fs::write(&timeline,export::frame_commands_range(&click_text[n],c.fps,part.start,part.duration,&control)?).map_err(|e|e.to_string())?;filters.push(format!("[clicksheet{n}]sendcmd=f='{}',crop@clickframe{n}=w={}:h={}:x=0:y=0,format=rgba,colorchannelmixer@clickalpha{n}=aa=0[clickfx{n}];[{video}][clickfx{n}]overlay@click{n}=x=-200:y=-200:shortest=1:format=auto[clicked{n}]",escaped(&timeline),size*2,size*2));video=format!("clicked{n}");}
   preparation.advance();
  }
  if effects&&c.cursor_effect=="spotlight"{cmd.args(["-framerate",&c.fps.to_string(),"-loop","1","-i"]).arg(&mask);filters.push(format!("[{video}]split[spotbase][spotpatch];[spotbase]eq@spotdim=brightness=0[dimmed];[spotpatch]crop@spotcrop=w={spot_size}:h={spot_size}:x=0:y=0,format=rgba[patch];[{next}:v]format=gray[spotmask];[patch][spotmask]alphamerge,colorchannelmixer@spotalpha=aa=0[spot];[dimmed][spot]overlay@spotlight=x=0:y=0:shortest=1:format=auto[spotted]"));video="spotted".into();next+=1;}
  if effects&&c.cursor_effect=="trail"{for n in 0..3{cmd.args(["-framerate",&c.fps.to_string(),"-loop","1","-i"]).arg(&trail);filters.push(format!("[{next}:v]format=rgba,colorchannelmixer@trailalpha{n}=aa=0[trailfx{n}];[{video}][trailfx{n}]overlay@trail{n}=x=-200:y=-200:shortest=1:format=auto[trailed{n}]"));video=format!("trailed{n}");next+=1;}}
  for annotation in &job.annotations{
   let from=(annotation.start-segment_offset).max(0.);let to=(annotation.end.unwrap_or(f64::MAX)-segment_offset).min(part.duration);if from>=part.duration||to<=from{continue;}
   let label=format!("ann{}_{}",i,annotation.id);let mut timeline=crate::recorder_annotations::timeline(annotation,segment_offset,part.duration);let mut sampled=Vec::new();for (time,shape)in timeline.drain(..){let tick=(time*c.fps as f64).round()as u64;if sampled.last().map(|(t,_) : &(f64,crate::recorder_annotations::Annotation)|(*t*c.fps as f64).round()as u64==tick).unwrap_or(false){sampled.pop();}sampled.push((tick as f64/c.fps as f64,shape));}let timeline=sampled;
   let (x,y,w,h)=if annotation.kind=="blur"{let (x,y,w,h)=crate::recorder_annotations::canvas_bounds(annotation);let x=x as u32/2*2;let y=y as u32/2*2;(x as i32,y as i32,w.min(job.bounds.w-x).max(2)/2*2,h.min(job.bounds.h-y).max(2)/2*2)}else{crate::recorder_annotations::canvas_bounds(annotation)};
   let list=job.dir.join(format!("annotation-{}-{i}.ffconcat",annotation.id));let mut entries=String::from("ffconcat version 1.0\n");
   for (index,(time,shape)) in timeline.iter().enumerate(){
    if control.cancelled(){return Err("Recording cancelled.".into());}
    let asset=job.dir.join(format!("annotation-{}-{i}-{index}.png",annotation.id));
    let svg=if annotation.kind=="blur"{format!(r##"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="{x} {y} {w} {h}"><rect x="{x}" y="{y}" width="{w}" height="{h}" fill="black"/><rect x="{}" y="{}" width="{}" height="{}" fill="white"/></svg>"##,shape.x.min(shape.x+shape.w),shape.y.min(shape.y+shape.h),shape.w.abs(),shape.h.abs())}else{crate::recorder_annotations::asset_on_canvas(shape,(x,y,w,h)).0};
    assets.push(svg,asset.clone());let until=timeline.get(index+1).map(|(t,_)|*t).unwrap_or(part.duration+1.);let duration=(until-time).max(1./c.fps as f64);
    entries+=&format!("file '{}'\noption framerate {}\nduration {duration:.6}\n",asset.to_string_lossy().replace('\\',"/").replace('\'',"'\\''"),c.fps);
   }
   fs::write(&list,entries).map_err(|e|e.to_string())?;cmd.args(["-f","concat","-safe","0","-i"]).arg(list);preparation.advance();
   if annotation.kind=="blur"{let radius=(w.min(h)/2).saturating_sub(1).min(8).max(1);filters.push(format!("[{video}]split[{label}base][{label}patch];[{label}patch]crop={w}:{h}:{x}:{y},boxblur={radius}:2:enable='gte(t,{from})*lt(t,{to})',format=yuv444p[{label}blur];[{next}:v]format=gray[{label}mask];[{label}blur][{label}mask]alphamerge[{label}masked];[{label}base][{label}masked]overlay={x}:{y}:format=auto:enable='gte(t,{from})*lt(t,{to})'[{label}]"));}
   else{filters.push(format!("[{video}][{next}:v]overlay={x}:{y}:format=auto:enable='gte(t,{from})*lt(t,{to})'[{label}]"));}next+=1;video=label;
  }
  if c.resolution!="original"{let (w,h)=if c.resolution=="720p"{(1280,720)}else{(1920,1080)};filters.push(format!("[{video}]scale=w='min(iw,{w})':h='min(ih,{h})':force_original_aspect_ratio=decrease:force_divisible_by=2:flags=lanczos[scaled]"));video="scaled".into();}
  if c.frame!="none"{
   let (w,h)=export::styled_size(c,job.bounds.w,job.bounds.h);let pad=frame_pad(c,w);
   let frame=job.dir.join(format!("frame-{i}.png"));assets.push(mockup_svg(c,w,h),frame.clone());
   cmd.args(["-framerate",&c.fps.to_string(),"-loop","1","-i"]).arg(frame);let frame_input=next;next+=1;
   if let Some(id)=background_preset(c){
    let bg=job.dir.join(format!("background-{i}.png"));assets.push(background_svg(id,w+2*pad,h+2*pad+40),bg.clone());
    cmd.args(["-framerate",&c.fps.to_string(),"-loop","1","-i"]).arg(bg);
    filters.push(format!("[{next}:v][{video}]overlay={pad}:{}:shortest=1:format=auto[padded];[padded][{frame_input}:v]overlay=0:0:shortest=1:format=auto[framed]",pad+40));next+=1;
   }else{filters.push(format!("[{video}]pad=iw+{}:ih+{}:{pad}:{}:color={}[padded];[padded][{frame_input}:v]overlay=0:0:shortest=1:format=auto[framed]",2*pad,2*pad+40,pad+40,c.background));}
   video="framed".into();
  }
  if let Some(camera)=&camera{if let Some(filter)=camera.filter(part.offset,part.duration){moving_camera=filter.starts_with("split=");filters.push(format!("[{video}]{filter}[cinematic]"));video="cinematic".into();}}
  if let Some(track)=&key_track{let (w,h)=encoder::output_size(c,job.bounds.w,job.bounds.h);let (svg,sw,sh)=track.sheet(w,h,&c.keystroke_theme);let sheet=job.dir.join(format!("keycaps-{i}.png"));assets.push(svg,sheet.clone());cmd.args(["-framerate",&c.fps.to_string(),"-loop","1","-i"]).arg(sheet);let commands=job.dir.join(format!("keycaps-{i}.txt"));fs::write(&commands,track.commands(part.offset,part.duration,c.fps,sh)).map_err(|e|e.to_string())?;filters.push(format!("[{next}:v]sendcmd=f='{}',split=2[keysheet0][keysheet1];[keysheet0]crop@keyframe0=w={sw}:h={sh}:x=0:y=0:exact=1,format=rgba,colorchannelmixer@keyalpha0=aa=0[keycap0];[keysheet1]crop@keyframe1=w={sw}:h={sh}:x=0:y=0:exact=1,format=rgba,colorchannelmixer@keyalpha1=aa=0[keycap1];[{video}][keycap0]overlay@keys0=x=18:y=H-h-18:shortest=1:format=auto[keyed0];[keyed0][keycap1]overlay@keys1=x=18:y=H-h-18:shortest=1:format=auto[keyed]",escaped(&commands)));video="keyed".into();}
  if export::fade_enabled(c.auto_fade,total_duration){let d=1f64;let end=(total_duration-d-1./c.fps as f64).max(d);filters.push(format!("[{video}]setpts=PTS+{segment_offset}/TB,fade=t=in:st=0:d={d},fade=t=out:st={end}:d={d},setpts=PTS-{segment_offset}/TB[faded]"));video="faded".into();}
  let graph=filters.join(";").replace("format=auto","format=yuv444");if c.smart_zoom{let script=job.dir.join(format!("camera-graph-{i}.txt"));fs::write(&script,&graph).map_err(|e|e.to_string())?;cmd.arg("-/filter_complex").arg(script);}else{cmd.args(["-filter_complex",&graph]);}cmd.args(["-map",&format!("[{video}]"),"-an"]);video_settings(&mut cmd,c,encoder);if encoder==Encoder::Cpu{cmd.args(["-threads",&filter_threads]);}if encoder==Encoder::Cpu&&moving_camera{cmd.args(["-tune","zerolatency"]);}cmd.args(["-t",&part.duration.to_string(),"-movflags","+faststart"]).arg(&result);prepared.push((cmd,part.duration,part.cost));preparation.advance();completed.push(result);
 }
 assets.prepare(&preparation,4)?;preparation.complete();
 control.set_stage(2);
 render::parallel(prepared,workers,total_duration,control.clone())?;
 control.set_stage(4);
 if let Some(fx)=soundtrack::render(job,camera.as_ref(),&control)?{export::concat_with_soundtrack(&completed,&job.output,&job.dir.join("styled-segments.txt"),total_duration,job.original_output.as_deref(),job.segments.iter().any(|s|!s.audio.is_empty()),&fx,c.auto_fade,&control)?;}else{export::concat_delivery_controlled(&completed,&job.output,&job.dir.join("styled-segments.txt"),total_duration,0.,100.,job.original_output.as_deref(),None,c.auto_fade,&control)?;}
 Ok((job.output.clone(),job.original_output.clone()))
}

fn preset_stops(id:&str)->Option<[&'static str;3]>{Some(match id{"aurora"=>["#5b8def","#8f6bf0","#20c4cb"],"sunset"=>["#ff9966","#ff5e8a","#8e4fd8"],"ocean"=>["#0f2a6b","#1f6fd6","#3fd0e8"],"forest"=>["#0e3b2e","#1e8a6a","#8ad9a8"],"midnight"=>["#0a0e22","#1d2a5c","#3b3f9a"],"peach"=>["#ffd6c0","#ffb3c1","#c9b6ff"],"mint"=>["#d1f7e6","#9ee6d4","#7ac6f2"],"graphite"=>["#2b2f3a","#454c5e","#6b7488"],"candy"=>["#ff8ad8","#a78bfa","#60a5fa"],"lavender"=>["#e0c3fc","#c4b5fd","#8ec5fc"],"lagoon"=>["#00c6a7","#1e9bd7","#4b5cc4"],"ember"=>["#ff512f","#dd2476","#3a1c71"],"rose"=>["#ff9a9e","#fecfef","#a18cd1"],"gold"=>["#f6d365","#fda085","#f5576c"],"sky"=>["#89f7fe","#66a6ff","#5b7cfa"],"violet"=>["#6a11cb","#8a4fff","#2575fc"],"emerald"=>["#0f9b8e","#38ef7d","#b6f3a4"],"coral"=>["#ff758c","#ff7eb3","#ffb199"],"dusk"=>["#2c3e50","#4b3f72","#fd746c"],"ice"=>["#e0f7ff","#b6e3ff","#8ab4ff"],"neon"=>["#0f0c29","#4a2a8f","#24c6dc"],"cotton"=>["#fbc2eb","#a6c1ee","#c2e9fb"],_=>return None})}
fn background_preset(c:&Config)->Option<&str>{c.background.strip_prefix("preset:").filter(|id|preset_stops(id).is_some())}
fn background_svg(id:&str,w:u32,h:u32)->String{
 let [original_a,original_b,original_c]=preset_stops(id).unwrap_or(["#15213A","#15213A","#15213A"]);
 let darken=|hex:&str|{let channels=[1,3,5].map(|i|((u8::from_str_radix(&hex[i..i+2],16).unwrap()as f64*0.84).round())as u8);format!("#{:02x}{:02x}{:02x}",channels[0],channels[1],channels[2])};
 let (a,b,c)=(darken(original_a),darken(original_b),darken(original_c));let x=(w as f64-h as f64)/4.;let y=-x;let d=(w+h)as f64/2.;let blur=w.min(h)as f64*0.11;
 format!(r##"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}"><defs><linearGradient id="bg-base" gradientUnits="userSpaceOnUse" x1="{x}" y1="{y}" x2="{}" y2="{}"><stop stop-color="{a}"/><stop offset=".55" stop-color="{b}"/><stop offset="1" stop-color="{c}"/></linearGradient><filter id="bg-soft" x="-30%" y="-30%" width="160%" height="160%"><feGaussianBlur stdDeviation="{blur}"/></filter><radialGradient id="bg-edge" cx=".5" cy=".45" r=".8"><stop offset=".3" stop-color="#080d22" stop-opacity="0"/><stop offset="1" stop-color="#080d22" stop-opacity=".24"/></radialGradient></defs><rect width="{w}" height="{h}" fill="url(#bg-base)"/><g filter="url(#bg-soft)"><ellipse cx="{}" cy="{}" rx="{}" ry="{}" fill="{original_a}" opacity=".35"/><ellipse cx="{}" cy="{}" rx="{}" ry="{}" fill="{original_c}" opacity=".3"/></g><rect width="{w}" height="{h}" fill="url(#bg-edge)"/></svg>"##,x+d,y+d,w as f64*0.18,h as f64*0.24,w as f64*0.38,h as f64*0.27,w as f64*0.82,h as f64*0.74,w as f64*0.43,h as f64*0.3)
}
/// Margin around the window in pixels: `frame_padding` is a percentage of the video width, kept even for the video encoder.
fn frame_pad(c:&Config,w:u32)->u32{(((w as f64*c.frame_padding as f64/100.).round() as u32).max(8))&!1}
fn mockup_svg(c:&Config,w:u32,h:u32)->String{
 let p=frame_pad(c,w);let (width,height)=(w+2*p,h+2*p+40);let right=w+p;let mac=c.frame=="mac";let radius=if mac{10}else{12};
 let background=if let Some(id)=background_preset(c){background_svg(id,width,height)}else{format!(r#"<rect width="{width}" height="{height}" fill="{}"/>"#,c.background)};
 let chrome=if mac{format!(r##"<circle cx="{}" cy="{}" r="6" fill="#ff5f57"/><circle cx="{}" cy="{}" r="6" fill="#febc2e"/><circle cx="{}" cy="{}" r="6" fill="#28c840"/>"##,p+20,p+20,p+40,p+20,p+60,p+20)}else{
  let control=(w as f64/6.).min(46.);let cy=p as f64+20.;let minimize=right as f64-control*2.5;let maximize=right as f64-control*1.5;let close=right as f64-control*0.5;
  let icon=if w>=160{format!(r##"<g fill="#3274e9"><rect x="{}" y="{}" width="5" height="5" rx=".6"/><rect x="{}" y="{}" width="5" height="5" rx=".6"/><rect x="{}" y="{}" width="5" height="5" rx=".6"/><rect x="{}" y="{}" width="5" height="5" rx=".6"/></g>"##,p+15,p+14,p+22,p+14,p+15,p+21,p+22,p+21)}else{String::new()};
  format!(r##"{icon}<path d="M{} {cy}h10 M{} {}h9v9h-9z M{} {}l9 9m0-9l-9 9" stroke="#344054" fill="none" stroke-width="1.35" stroke-linecap="round" stroke-linejoin="round"/><path d="M{p} {}H{right}" stroke="#dde3ec" stroke-width="1"/><rect x="{}" y="{}" width="{}" height="{}" rx="11.5" fill="none" stroke="#cbd4e1" stroke-width="1"/>"##,minimize-5.,maximize-4.5,cy-4.5,close-4.5,cy-4.5,p as f64+39.5,p as f64+0.5,p as f64+0.5,w as f64-1.,h as f64+39.)
 };
 let title=if mac{"#e8ecf4"}else{"#f7f9fc"};
 format!(r##"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}"><defs><mask id="outside"><rect width="{width}" height="{height}" fill="white"/><rect x="{p}" y="{p}" width="{w}" height="{}" rx="{radius}" fill="black"/></mask></defs><g mask="url(#outside)">{background}</g><rect x="{p}" y="{p}" width="{w}" height="40" rx="{radius}" fill="{title}"/><rect x="{p}" y="{}" width="{w}" height="20" fill="{title}"/>{chrome}</svg>"##,h+40,p+20)
}

#[tauri::command]pub async fn recorder_start(app:tauri::AppHandle,target:Target,config:Config)->Res<()> {
 if matches!(recorder_state().phase.as_str(),"saving"){return Err("Finish the current recording export before starting another recording.".into());}
 let _job=crate::updates::job()?;
 zoom_preview::init(app.clone());validate(&config)?;let bounds=bounds(&app,&target)?;if bounds.w<2||bounds.h<2||bounds.w>8192||bounds.h>8192{return Err("Select a source between 2 and 8192 pixels per side.".into());}
 tauri::async_runtime::spawn_blocking(move||{let _op=OP.lock().unwrap();if SESSION.lock().unwrap().is_some()||matches!(recorder_state().phase.as_str(),"saving"){return Err("Finish the current recording export before starting another recording.".into());}CANCEL.store(false,Ordering::Relaxed);stop_preview();*STATE.lock().unwrap()=Some(State{phase:"preparing".into(),..Default::default()});
  let result=(||{fs::create_dir_all(&config.output_dir).map_err(|e|e.to_string())?;let name=chrono::Local::now().format("Halo Recording %Y-%m-%d %H-%M-%S").to_string();let output=crate::process::unique(Path::new(&config.output_dir),&name,"mp4");let dir=crate::system::tools_dir().join("recorder-cache").join(chrono::Local::now().timestamp_millis().to_string());fs::create_dir_all(&dir).map_err(|e|e.to_string())?;let encoder=Encoder::Nvenc;let original_output=None;let mut job=Session{config,target,bounds,dir,output,project:None,original_output,segments:vec![],running:None,encoder,mic_peak:Arc::new(AtomicU32::new(0)),sys_peak:Arc::new(AtomicU32::new(0)),mic_muted:Arc::new(AtomicBool::new(false)),sys_muted:Arc::new(AtomicBool::new(false)),annotations:vec![]};project::create(&mut job)?;let running=start_segment(&mut job)?;if running.mic_failed{job.config.microphone=false;}if running.system_failed{job.config.system_audio=false;}job.running=Some(running);let job_project=job.project.as_ref().map(|p|p.to_string_lossy().into_owned());*SESSION.lock().unwrap()=Some(job);*STATE.lock().unwrap()=Some(State{phase:"recording".into(),target:Some(bounds),project_path:job_project,..Default::default()});Ok::<_,String>(())})();if let Err(error)=&result{phase("idle",Some(error.clone()));}result
 }).await.map_err(|e|e.to_string())?
}
#[tauri::command]pub async fn recorder_pause()->Res<()> {tauri::async_runtime::spawn_blocking(||{let _op=OP.lock().unwrap();let (running,fps)={let mut session=SESSION.lock().unwrap();let job=session.as_mut().ok_or("No recording is active.")?;(job.running.take().ok_or("No recording is running.")?,job.config.fps)};phase("pausing",None);match finish_segment(running,fps){Ok(segment)=>{let elapsed={let mut session=SESSION.lock().unwrap();let job=session.as_mut().unwrap();job.segments.push(segment);if let Err(error)=project::checkpoint(job,None,"paused"){log_error(&format!("Project checkpoint: {error}"));}job.segments.iter().map(|s|s.duration).sum()};if let Some(state)=STATE.lock().unwrap().as_mut(){state.elapsed=elapsed;}phase("paused",None);Ok(())},Err(error)=>{phase("paused",Some(error.clone()));Err(error)}}}).await.map_err(|e|e.to_string())?}
#[tauri::command]pub async fn recorder_resume()->Res<()> {tauri::async_runtime::spawn_blocking(||{let _op=OP.lock().unwrap();let mut lock=SESSION.lock().unwrap();let job=lock.as_mut().ok_or("No recording is paused.")?;if job.running.is_some(){return Err("Recording is already running.".into());}if let Some(state)=STATE.lock().unwrap().as_mut(){state.elapsed=job.segments.iter().map(|s|s.duration).sum();}phase("resuming",None);match start_segment(job){Ok(running)=>{if running.mic_failed{job.config.microphone=false;}if running.system_failed{job.config.system_audio=false;}job.running=Some(running);phase("recording",None);Ok(())},Err(error)=>{phase("paused",Some(error.clone()));Err(error)}}}).await.map_err(|e|e.to_string())?}
#[tauri::command]pub async fn recorder_stop(app:tauri::AppHandle)->Res<String> {
 flush_annotations(&app).await?;
 tauri::async_runtime::spawn_blocking(move||{
  let _op=OP.lock().unwrap();let mut job=SESSION.lock().unwrap().take().ok_or("No recording is active.")?;
  if let Some(w)=app.get_webview_window("recorder-annotations"){let _=w.destroy();}
  save_stage(0);phase("saving",None);
  let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||{
   if let Some(r)=job.running.take(){job.segments.push(finish_segment(r,job.config.fps)?);}
   project::checkpoint(&job,None,"ready")?;
   let styled=export::needs_render(&job);
   if styled&&job.original_output.is_none(){let name=format!("{} (original)",job.output.file_stem().unwrap().to_string_lossy());job.original_output=Some(crate::process::unique(job.output.parent().unwrap(),&name,"mp4"));}
   let path=job.original_output.as_ref().unwrap_or(&job.output).clone();
   export::save_original(&job,&path)?;
   project::checkpoint(&job,Some(&path),if styled{"exporting"}else{"completed"})?;
   let warning=STATE.lock().unwrap().as_ref().and_then(|s|s.error.clone());
   let out=path.to_string_lossy().into_owned();
   *STATE.lock().unwrap()=Some(State{phase:if styled{"exporting"}else{"idle"}.into(),output:Some(out.clone()),original_output:if styled{Some(out.clone())}else{None},elapsed:job.segments.iter().map(|s|s.duration).sum(),error:warning,project_path:job.project.as_ref().map(|p|p.to_string_lossy().into_owned()),..Default::default()});
   let _=app.emit("halo://recording-saved",&out);
   if styled{spawn_styled_export(app.clone(),job,out.clone());}
   else if let Err(error)=project::set_status(&job,"completed",None){log_error(&format!("Project export status: {error}"));}

   Ok(out)
  })).unwrap_or_else(|_|Err("The recording save worker stopped unexpectedly.".into()));
  if let Err(error)=&result{log_error(&format!("Original save failed: {error}"));phase("idle",Some("Could not finish saving the recording. The captured video has been retained for recovery.".into()));}
  result
 }).await.map_err(|e|e.to_string())?
}

fn spawn_styled_export(app:tauri::AppHandle,job:Session,expected_original:String){
 match jobs::enqueue(app,job,expected_original){Ok(id)=>{if let Some(state)=STATE.lock().unwrap().as_mut(){state.export_id=Some(id);}},Err(error)=>phase("idle",Some(error))}
}
#[tauri::command]pub fn recorder_jobs()->Vec<jobs::View>{jobs::views()}
pub fn cancel_background(app:&tauri::AppHandle,id:&str)->bool{jobs::cancel(app,id)}
pub fn shutdown_background(app:&tauri::AppHandle){jobs::shutdown(app);}

#[tauri::command]pub async fn recorder_open_project(app:tauri::AppHandle,path:Option<String>)->Res<Option<String>>{
 if active(){return Err("Finish or cancel the current recording/export before opening a project.".into());}
 let path=if let Some(path)=path{PathBuf::from(path)}else{
  use tauri_plugin_dialog::DialogExt;let picker=app.clone();let output=PathBuf::from(config().output_dir);let sources=output.join("source-files");let directory=if sources.is_dir(){sources}else{output};
  let selected=tauri::async_runtime::spawn_blocking(move||picker.dialog().file().add_filter("Halo recording project",&["halo"]).set_directory(directory).blocking_pick_file().and_then(|p|p.into_path().ok())).await.map_err(|e|e.to_string())?;
  let Some(path)=selected else{return Ok(None);};path
 };
 tauri::async_runtime::spawn_blocking(move||{
  let _op=OP.lock().unwrap();if active(){return Err("Finish or cancel the current recording/export before opening a project.".into());}
  let _job=crate::updates::job()?;stop_preview();CANCEL.store(false,Ordering::Relaxed);save_progress(0.);*STATE.lock().unwrap()=Some(State{phase:"preparing".into(),..Default::default()});
  let result:Res<Option<String>>=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||{
   let (mut job,cached)=project::load(&path)?;let styled=export::needs_render(&job);
   let original=if styled{cached.unwrap_or_else(||crate::process::unique(job.output.parent().unwrap(),&format!("{} (original)",job.output.file_stem().unwrap().to_string_lossy()),"mp4"))}else{job.output.clone()};
   if !original.exists(){export::save_original(&job,&original).map_err(|error|{log_error(&format!("Project original preparation: {error}"));"Could not prepare the original video from this project.".to_string()})?;}
   project::set_original(&job,&original)?;let out=original.to_string_lossy().into_owned();
   if styled{job.original_output=Some(original);}
   *STATE.lock().unwrap()=Some(State{phase:if styled{"exporting"}else{"idle"}.into(),output:Some(out.clone()),original_output:if styled{Some(out.clone())}else{None},project_path:job.project.as_ref().map(|p|p.to_string_lossy().into_owned()),elapsed:job.segments.iter().map(|s|s.duration).sum(),..Default::default()});
   if styled{spawn_styled_export(app.clone(),job,out.clone());}else{project::set_status(&job,"completed",None)?;}
   Ok(Some(out))
  })).unwrap_or_else(|_|Err("This project could not be opened.".into()));
  if let Err(error)=&result{log_error(&format!("Project open failed: {error}"));phase("idle",Some(error.clone()));}
  result
 }).await.map_err(|e|e.to_string())?
}

/// Cancels only the optional styled version. The published original is immutable.
#[tauri::command]pub fn recorder_cancel_export(app:tauri::AppHandle)->Res<()> {
 let state=recorder_state();let id=state.export_id.ok_or("No styled export is running.")?;
 if state.phase!="exporting"{return Err("No styled export is running.".into());}
 jobs::cancel(&app,&id);Ok(())
}

#[tauri::command]pub async fn recorder_test_scene(app:tauri::AppHandle,fullscreen:Option<bool>)->Res<()> {if !cfg!(debug_assertions){return Err("Available only in development.".into());}if let Some(w)=app.get_webview_window("recorder-test"){w.set_fullscreen(fullscreen.unwrap_or(false)).map_err(|e|e.to_string())?;w.show().map_err(|e|e.to_string())?;}else{tauri::WebviewWindowBuilder::new(&app,"recorder-test",tauri::WebviewUrl::App("output/recorder-research/test-scene.html".into())).title("Halo Recorder Test").fullscreen(fullscreen.unwrap_or(false)).inner_size(800.,600.).build().map_err(|e|e.to_string())?;}Ok(())}

pub fn active()->bool {SESSION.try_lock().map(|s|s.is_some()).unwrap_or(true)||matches!(recorder_state().phase.as_str(),"preparing"|"saving"|"pausing"|"resuming")}

#[tauri::command]pub fn recorder_debug_preview(app:tauri::AppHandle,enabled:bool)->Res<()> {if !cfg!(debug_assertions){return Err("Development only.".into());}let w=app.get_webview_window("recorder").ok_or("Recorder is closed.")?;let h=w.hwnd().map_err(|e|e.to_string())?;unsafe{SetWindowDisplayAffinity(HWND(h.0),if enabled{WDA_NONE}else{WDA_EXCLUDEFROMCAPTURE}).map_err(|e|e.to_string())?;}Ok(())}

/// Cancel only this recorder's work; conversion and upload jobs are independent.
pub async fn cancel_recorder(app:tauri::AppHandle)->Res<()> {
 CANCEL.store(true,Ordering::Relaxed);for label in ["recorder-annotations"]{if let Some(w)=app.get_webview_window(label){let _=w.destroy();}}let _=app.emit_to("recorder","halo://recorder-closed",());cancel_area(&app);if let Some(w)=app.get_webview_window("recorder"){let _=w.hide();}
 tauri::async_runtime::spawn_blocking(move||{let _op=OP.lock().unwrap();stop_preview();if let Some(mut job)=SESSION.lock().unwrap().take(){if let Some(mut r)=job.running.take(){r.stop.store(true,Ordering::Relaxed);let _=r.child.kill();let _=r.child.wait();if let Some(pointer)=r.pointer{let _=pointer.join();}for track in r.audio{let _=export::join_audio(track);}}}*STATE.lock().unwrap()=Some(State{phase:"idle".into(),..Default::default()});}).await.map_err(|e|e.to_string())?;Ok(())
}
#[tauri::command]pub async fn recorder_cancel(app:tauri::AppHandle)->Res<()> {cancel_recorder(app).await}
#[tauri::command]pub async fn recorder_close(app:tauri::AppHandle)->Res<()> {
 let state=recorder_state();
 if !matches!(state.phase.as_str(),"idle"|"saving"|"exporting"){return cancel_recorder(app).await;}
 let _=app.emit_to("recorder","halo://recorder-closed",());cancel_area(&app);if let Some(w)=app.get_webview_window("recorder"){let _=w.hide();}
 if state.phase!="saving"{let mut current=STATE.lock().unwrap();if current.as_ref().map(|s|(&s.phase,s.project_path.as_ref()))==Some((&state.phase,state.project_path.as_ref())){*current=Some(State{phase:"idle".into(),..Default::default()});}}
 recorder_preview_stop().await?;Ok(())
}

#[tauri::command]pub async fn recorder_toggle_audio(kind:String,enabled:bool)->Res<bool> {
 if !["microphone","system"].contains(&kind.as_str()){return Err("Choose microphone or system audio.".into());}
 tauri::async_runtime::spawn_blocking(move||{let mut enabled=enabled;let _op=OP.lock().unwrap();let mut guard=SESSION.lock().unwrap();let job=guard.as_mut().ok_or("No recording is active.")?;let mic=kind=="microphone";let muted=if mic{job.mic_muted.clone()}else{job.sys_muted.clone()};muted.store(!enabled,Ordering::Relaxed);
 let capture=project::assets_dir(job);
 if enabled{if let Some(running)=job.running.as_mut(){if !running.audio.iter().any(|track|track.mic==mic){let index=job.segments.len();let path=capture.join(format!("{}-{index}.wav",if mic{"mic"}else{"system"}));match crate::recorder_audio::start(path,mic,job.config.microphone_id.clone(),running.stop.clone(),running.gate.clone(),if mic{job.mic_peak.clone()}else{job.sys_peak.clone()},muted.clone()){Ok(track)=>running.audio.push(track),Err(error)=>{log_error(&format!("Audio unavailable, keeping it disabled: {error}"));muted.store(true,Ordering::Relaxed);enabled=false;}}}}}
 if mic{job.config.microphone=enabled;}else{job.config.system_audio=enabled;}Ok(enabled)}).await.map_err(|e|e.to_string())?
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
#[tauri::command]pub fn recorder_annotation_data()->Res<serde_json::Value>{let guard=SESSION.lock().unwrap();let job=guard.as_ref().ok_or("No recording is active.")?;Ok(serde_json::json!({"width":job.bounds.w,"height":job.bounds.h,"fps":job.config.fps,"annotations":job.annotations,"elapsed":elapsed(job),"viewport":annotation_viewport(job,if job.target.mode=="window"{window_rect(job.target.window).unwrap_or(job.bounds)}else{job.bounds})}))}
#[tauri::command]pub fn recorder_annotation_add(mut annotation:crate::recorder_annotations::Annotation)->Res<crate::recorder_annotations::Annotation>{let mut guard=SESSION.lock().unwrap();let job=guard.as_mut().ok_or("No recording is active.")?;if job.annotations.len()>=100{return Err("This recording already contains 100 annotations.".into());}crate::recorder_annotations::validate(&annotation,job.bounds.w,job.bounds.h)?;annotation.id=job.annotations.len()as u64+1;annotation.start=elapsed(job);annotation.end=None;annotation.keyframes=vec![annotation.frame(annotation.start)];job.annotations.push(annotation.clone());Ok(annotation)}
#[tauri::command]pub fn recorder_annotation_end(id:Option<u64>)->Res<()> {let mut guard=SESSION.lock().unwrap();let job=guard.as_mut().ok_or("No recording is active.")?;let time=elapsed(job);for annotation in &mut job.annotations{if annotation.end.is_none()&&id.map(|id|id==annotation.id).unwrap_or(true){annotation.end=Some(time);}}Ok(())}
#[derive(Clone,Deserialize)]pub struct AnnotationHitRegion{x:f64,y:f64,w:f64,h:f64}
#[derive(Default)]struct AnnotationInput{editing:bool,cursor:bool,regions:Vec<AnnotationHitRegion>}
static ANNOTATION_READY:AtomicBool=AtomicBool::new(false);
static ANNOTATION_FORWARDING:AtomicBool=AtomicBool::new(false);
static ANNOTATION_WHEEL_LOCK:Mutex<()>=Mutex::new(());
static ANNOTATION_INPUT:Mutex<AnnotationInput>=Mutex::new(AnnotationInput{editing:false,cursor:true,regions:Vec::new()});
#[tauri::command]pub fn recorder_annotation_input(cursor:bool,regions:Vec<AnnotationHitRegion>){
 let mut input=ANNOTATION_INPUT.lock().unwrap();input.cursor=cursor;
 input.regions=regions.into_iter().filter(|r|[r.x,r.y,r.w,r.h].iter().all(|v|v.is_finite())&&r.w>0.&&r.h>0.).take(65536).collect();
}
fn annotation_ignores_pointer(input:&AnnotationInput,x:f64,y:f64,dragging:bool)->bool{
 !input.editing||(input.cursor&&!dragging&&!input.regions.iter().any(|r|x>=r.x&&x<=r.x+r.w&&y>=r.y&&y<=r.y+r.h))
}
#[tauri::command]pub fn recorder_annotation_ready(app:tauri::AppHandle)->Res<()> {
 ANNOTATION_READY.store(true,Ordering::Relaxed);
 let editing=ANNOTATION_INPUT.lock().unwrap().editing;
 if editing&&SESSION.lock().unwrap().is_some(){if let Some(w)=app.get_webview_window("recorder-annotations"){w.show().map_err(|e|e.to_string())?;}}
 Ok(())
}
#[tauri::command]pub async fn recorder_annotations_edit(app:tauri::AppHandle,editing:bool)->Res<()> {
 {let mut input=ANNOTATION_INPUT.lock().unwrap();input.editing=editing;if editing{input.cursor=true;}}
if let Some(w)=app.get_webview_window("recorder-annotations"){w.set_ignore_cursor_events(!editing).map_err(|e|e.to_string())?;let _=app.emit("halo://annotation-editing",editing);if editing&&ANNOTATION_READY.load(Ordering::Relaxed){let _=w.show();let _=w.set_focus();}return Ok(());}if !editing{return Ok(());}let r={let guard=SESSION.lock().unwrap();let job=guard.as_ref().ok_or("No recording is active.")?;if job.target.mode=="window"{window_rect(job.target.window)?}else{job.bounds}};ANNOTATION_READY.store(false,Ordering::Relaxed);let w=tauri::WebviewWindowBuilder::new(&app,"recorder-annotations",tauri::WebviewUrl::App("recorder-annotations.html".into())).title("Halo · Recording annotations").visible(false).decorations(false).transparent(true).always_on_top(true).skip_taskbar(true).resizable(false).shadow(false).build().map_err(|e|e.to_string())?;w.set_size(PhysicalSize::new(r.w,r.h)).map_err(|e|e.to_string())?;w.set_position(PhysicalPosition::new(r.x,r.y)).map_err(|e|e.to_string())?;exclude(&w);let _=w.set_focus();let handle=app.clone();thread::spawn(move||{let mut ignored=None;let mut last_size=None;let mut last_position=Instant::now();loop{
 thread::sleep(Duration::from_millis(8));let Some(window)=handle.get_webview_window("recorder-annotations")else{break};
 if let (Ok(origin),Ok(scale))=(window.outer_position(),window.scale_factor()){
  let mut p=POINT::default();if unsafe{GetCursorPos(&mut p)}.is_ok(){
   // Keep receiving a gesture that started on a drawing, even outside its old bounds.
   let down=unsafe{GetAsyncKeyState(0x01)}as u16&0x8000!=0;
   // The drawing layer stays above the recorded app, but the recorder itself
   // must still receive Finish/Pencil clicks even while a drawing tool is active.
   let over_recorder=handle.get_webview_window("recorder").filter(|w|w.is_visible().unwrap_or(false)).and_then(|w|w.outer_position().ok().zip(w.outer_size().ok())).map(|(pos,size)|p.x>=pos.x&&p.y>=pos.y&&p.x<pos.x+size.width as i32&&p.y<pos.y+size.height as i32).unwrap_or(false);
   let ignore=over_recorder||ANNOTATION_FORWARDING.load(Ordering::Relaxed)||annotation_ignores_pointer(&ANNOTATION_INPUT.lock().unwrap(),(p.x-origin.x)as f64/scale,(p.y-origin.y)as f64/scale,down&&ignored==Some(false));
   let actual=window.hwnd().ok().map(|h|unsafe{GetWindowLongPtrW(HWND(h.0),GWL_EXSTYLE)}as u32&WS_EX_TRANSPARENT.0!=0);
   if ignored!=Some(ignore)||actual!=Some(ignore){if window.set_ignore_cursor_events(ignore).is_ok(){ignored=Some(ignore);}}
  }
 }
 if last_position.elapsed()<Duration::from_millis(150){continue;}last_position=Instant::now();let bounds={let guard=SESSION.lock().unwrap();let Some(job)=guard.as_ref()else{break};if job.target.mode=="window"{window_rect(job.target.window).ok()}else{None}};if let Some(bounds)=bounds{if window.outer_position().ok()!=Some(PhysicalPosition::new(bounds.x,bounds.y)){let _=window.set_position(PhysicalPosition::new(bounds.x,bounds.y));}let size=(bounds.w,bounds.h);if bounds.w>1&&bounds.h>1&&last_size!=Some(size){let viewport={let guard=SESSION.lock().unwrap();guard.as_ref().map(|job|annotation_viewport(job,bounds))};if let Some(viewport)=viewport{let _=handle.emit_to("recorder-annotations","halo://annotation-viewport",viewport);}let _=window.set_size(PhysicalSize::new(bounds.w,bounds.h));last_size=Some(size);}}}});Ok(())}
#[tauri::command]pub async fn recorder_annotation_snapshot()->Res<String>{let(target,r)={let guard=SESSION.lock().unwrap();let job=guard.as_ref().ok_or("No recording is active.")?;(job.target.clone(),job.bounds)};tauri::async_runtime::spawn_blocking(move||{let dir=crate::system::tools_dir().join("recorder-cache");let path=dir.join(format!("annotation-source-{}.png",chrono::Local::now().timestamp_millis()));let source=if target.mode=="window"{format!("gfxcapture=hwnd={}:capture_cursor=0",target.window)}else{format!("gfxcapture=hmonitor={}:capture_cursor=0",target.monitor)};let mut c=ffmpeg();c.args(["-f","lavfi","-i",&source,"-frames:v","1","-vf"]);let crop=if target.mode=="area"{let area=target.rect.unwrap();format!("hwdownload,format=bgra,crop={}:{}:{}:{}",r.w,r.h,area.x,area.y)}else if target.mode=="window"{format!("hwdownload,format=bgra,crop=w='min(iw,{})':h='min(ih,{})':x=0:y=0,pad={}:{}:(ow-iw)/2:(oh-ih)/2:black",r.w,r.h,r.w,r.h)}else{"hwdownload,format=bgra".into()};c.arg(crop).arg(&path);run(c)?;Ok(format!("data:image/png;base64,{}",base64::engine::general_purpose::STANDARD.encode(fs::read(path).map_err(|e|e.to_string())?)))}).await.map_err(|e|e.to_string())?}
#[tauri::command]pub fn recorder_output_info()->Res<serde_json::Value>{let state=recorder_state();let path=state.output.ok_or("No recording has been saved.")?;let metadata=fs::metadata(&path).map_err(|e|e.to_string())?;Ok(serde_json::json!({"path":path,"original_path":state.original_output,"project_path":state.project_path,"duration":state.elapsed,"bytes":metadata.len()}))}
#[tauri::command]pub async fn recorder_recycle_output()->Res<()> {let path=recorder_state().output.ok_or("No recording has been saved.")?;tauri::async_runtime::spawn_blocking(move||crate::process::recycle_recording(Path::new(&path))).await.map_err(|e|e.to_string())?}

fn log_error(message:&str){let dir=crate::system::tools_dir().join("recorder-cache");let _=fs::create_dir_all(&dir);if let Ok(mut file)=fs::OpenOptions::new().create(true).append(true).open(dir.join("diagnostics.log")){let _=writeln!(file,"{} {}",chrono::Local::now().to_rfc3339(),message);}}
#[tauri::command]pub fn recorder_log_error(message:String){log_error(&message.chars().take(8000).collect::<String>());}

// Drawing mode still owns the canvas. For a wheel over it, wait until Windows
// has actually applied click-through before injecting input; Tauri's setter queues
// the change, so sending immediately can deliver the wheel straight back to us.
#[tauri::command]pub async fn recorder_annotation_forward_wheel(app:tauri::AppHandle,delta:i32)->Res<()> {
 tauri::async_runtime::spawn_blocking(move||{
  let _serial=ANNOTATION_WHEEL_LOCK.lock().unwrap();
  let Some(window)=app.get_webview_window("recorder-annotations")else{return Ok(())};
  ANNOTATION_FORWARDING.store(true,Ordering::Relaxed);
  let result=(||{
   window.set_ignore_cursor_events(true).map_err(|e|e.to_string())?;
   let hwnd=window.hwnd().map_err(|e|e.to_string())?;let started=Instant::now();
   while unsafe{GetWindowLongPtrW(HWND(hwnd.0),GWL_EXSTYLE)}as u32&WS_EX_TRANSPARENT.0==0{
    if started.elapsed()>Duration::from_millis(250){return Err("Could not forward scrolling to the window below.".into());}
    thread::sleep(Duration::from_millis(1));
   }
   let input=INPUT{r#type:INPUT_MOUSE,Anonymous:INPUT_0{mi:MOUSEINPUT{dx:0,dy:0,mouseData:delta as u32,dwFlags:MOUSEEVENTF_WHEEL,time:0,dwExtraInfo:0}}};
   if unsafe{SendInput(&[input],std::mem::size_of::<INPUT>()as i32)}!=1{return Err("Windows could not forward scrolling.".into());}
   thread::sleep(Duration::from_millis(20));Ok(())
  })();
  ANNOTATION_FORWARDING.store(false,Ordering::Relaxed);
  // The native pointer worker restores the appropriate mode after dispatch.
  result
 }).await.map_err(|e|e.to_string())?
}
static ANNOTATION_FLUSH:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(0);
async fn flush_annotations(app:&tauri::AppHandle)->Res<()> {if app.get_webview_window("recorder-annotations").is_none(){return Ok(());}let token=chrono::Local::now().timestamp_millis()as u64;app.emit_to("recorder-annotations","halo://annotation-flush",token).map_err(|e|e.to_string())?;let app=app.clone();tauri::async_runtime::spawn_blocking(move||{let started=Instant::now();while ANNOTATION_FLUSH.load(Ordering::Relaxed)!=token{if app.get_webview_window("recorder-annotations").is_none(){return Ok(());}if started.elapsed()>Duration::from_secs(2){return Err("Could not finish annotations. Please try again.".into());}thread::sleep(Duration::from_millis(10));}Ok(())}).await.map_err(|e|e.to_string())?}
#[tauri::command]pub fn recorder_annotations_flushed(token:u64){ANNOTATION_FLUSH.store(token,Ordering::Relaxed);}
#[tauri::command]pub fn recorder_annotation_update(mut annotation:crate::recorder_annotations::Annotation)->Res<()> {let mut guard=SESSION.lock().unwrap();let job=guard.as_mut().ok_or("No recording is active.")?;crate::recorder_annotations::validate(&annotation,job.bounds.w,job.bounds.h)?;let time=elapsed(job);let current=job.annotations.iter_mut().find(|a|a.id==annotation.id&&a.end.is_none()).ok_or("This annotation is no longer active.")?;annotation.start=current.start;annotation.end=None;annotation.keyframes=std::mem::take(&mut current.keyframes);if annotation.keyframes.len()>=3600{annotation.keyframes=annotation.keyframes.into_iter().enumerate().filter_map(|(i,f)|(i%2==0).then_some(f)).collect();}crate::recorder_annotations::record_frame(&mut annotation,time);*current=annotation;Ok(())}

#[cfg(test)]mod framed_export_tests{use super::*;
 fn render_size(svg:&str)->(u32,u32){let tree=resvg::usvg::Tree::from_str(svg,&resvg::usvg::Options::default()).expect("valid svg");let size=tree.size().to_int_size();(size.width(),size.height())}
 #[test]fn mockup_and_background_match_the_padded_video(){for frame in ["mac","windows"]{for percent in [2u32,8,30]{let mut c=Config::default();c.frame=frame.into();c.frame_padding=percent;let (w,h)=(1920u32,1080u32);let pad=frame_pad(&c,w);assert_eq!(pad%2,0);let expected=(w+2*pad,h+2*pad+40);assert_eq!(render_size(&mockup_svg(&c,w,h)),expected);assert!(background_preset(&c).is_some());assert_eq!(render_size(&background_svg("aurora",expected.0,expected.1)),expected);}}}
 #[test]fn every_preset_has_gradient_stops(){for id in ["aurora","sunset","ocean","forest","midnight","peach","mint","graphite","candy","lavender","lagoon","ember","rose","gold","sky","violet","emerald","coral","dusk","ice","neon","cotton"]{assert!(preset_stops(id).is_some(),"{id}");}}
 #[test]fn default_config_is_valid(){assert!(validate(&Config::default()).is_ok());}
}

#[cfg(test)]mod annotation_pointer_tests{
 use super::*;
 #[test]fn cursor_routes_desktop_input_through_but_keeps_tools_and_drags_interactive(){
  let mut input=AnnotationInput{editing:true,cursor:true,regions:vec![AnnotationHitRegion{x:100.,y:500.,w:520.,h:72.},AnnotationHitRegion{x:200.,y:100.,w:24.,h:24.}]};
  assert!(annotation_ignores_pointer(&input,400.,250.,false),"empty desktop must receive scrolling");
  assert!(!annotation_ignores_pointer(&input,300.,530.,false),"toolbar must remain clickable");
  assert!(!annotation_ignores_pointer(&input,210.,110.,false),"existing arrows must remain selectable");
  assert!(!annotation_ignores_pointer(&input,400.,250.,true),"dragging outside the starting bounds must not drop the gesture");
  input.cursor=false;assert!(!annotation_ignores_pointer(&input,400.,250.,false),"drawing tools need the canvas");
  input.editing=false;assert!(annotation_ignores_pointer(&input,300.,530.,true),"closing the widget releases all input");
 }
}

#[cfg(test)]mod window_offset_tests{
 use super::*;
 #[test]fn centres_native_pixels_without_scaling(){
  for (w,h,x,y)in [(800,600,0,0),(400,300,200,150),(400,600,200,0),(800,300,0,150),(1600,1200,0,0)]{assert_eq!(window_offset(800,600,w,h),(x,y));}
 }
}
