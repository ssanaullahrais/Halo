#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod convert;
mod image_engine;
mod presets;
mod recorder;
mod recorder_audio;
mod recorder_annotations;
#[path = "../../shared/capabilities.rs"]
mod capabilities;
mod folders;
mod hook;
mod qr;
mod process;
mod selection;
mod share;
mod system;
mod updates;

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState, Shortcut};
use windows::Win32::Foundation::POINT;
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

use hook::Ev;

/// Logical size of the widget window and where the wheel centre sits inside it.
const WIN_W: f64 = 600.0;
const WIN_H: f64 = 432.0;
const CENTER_X: f64 = 204.0;
const CENTER_Y: f64 = 216.0;
/// In the menu style the pointer sits this far from the window's top-left corner.
const MENU_ANCHOR: f64 = 28.0;

#[derive(Debug)]
struct WidgetPlacement { x: i32, y: i32, w: i32, h: i32, flip: bool }

// Changing the options' side must preserve the position chosen by the user.
fn options_side(window_x:i32,scale:f64,menu:bool,flipped:bool,area:(i32,i32,u32,u32))->(i32,bool) {
    let center=if menu {120.0}else{CENTER_X};
    let previous_offset=((if flipped {WIN_W-center}else{center})*scale).round() as i32;
    let center_x=window_x+previous_offset;
    let needed=((if menu {350.0}else{384.0})*scale).round() as i32;
    let left=center_x-area.0;
    let right=area.0+area.2 as i32-center_x;
    let flip=right<needed&&left>right;
    let next_offset=((if flip {WIN_W-center}else{center})*scale).round() as i32;
    (center_x-next_offset,flip)
}

#[derive(Serialize)]
struct FitResult { flipped: bool, dragging: bool }

fn widget_placement(x:i32,y:i32,scale:f64,menu:bool,area:(i32,i32,u32,u32))->WidgetPlacement {
    let (left,top,width,height)=area;
    let (w,h)=((WIN_W*scale).round() as i32,(WIN_H*scale).round() as i32);
    let (anchor_x,anchor_y)=if menu {(MENU_ANCHOR,MENU_ANCHOR)}else{(CENTER_X,CENTER_Y)};
    let flip=x+((WIN_W-anchor_x)*scale).round() as i32>left+width as i32;
    let center_x=if flip {WIN_W-anchor_x}else{anchor_x};
    WidgetPlacement {
        x:(x-(center_x*scale).round() as i32).clamp(left,(left+width as i32-w).max(left)),
        y:(y-(anchor_y*scale).round() as i32).clamp(top,(top+height as i32-h).max(top)),
        w,h,flip,
    }
}

#[cfg(test)]
mod placement_tests {
    use super::*;
    #[test]
    fn both_layouts_flip_and_fit_scaled_monitor_work_areas() {
        for scale in [1.0,1.25,2.0] {
            for menu in [false,true] {
                for left in [-3840,0,1920] {
                    let area=(left,40,(1920.0*scale) as u32,(1040.0*scale) as u32);
                    for (x,expected_flip) in [(left+10,false),(left+area.2 as i32-10,true)] {
                        for y in [area.1,area.1+area.3 as i32-1] {
                            let layout=widget_placement(x,y,scale,menu,area);
                            assert_eq!(layout.flip,expected_flip,"{layout:?}");
                            assert!(layout.x>=area.0&&layout.x+layout.w<=area.0+area.2 as i32);
                            assert!(layout.y>=area.1&&layout.y+layout.h<=area.1+area.3 as i32);
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn changing_options_side_never_pushes_the_dragged_widget_back() {
        for scale in [1.0,1.25,2.0] {for menu in [false,true] {for flipped in [false,true] {for x in [-500,0,800,1700,2100] {
            let center=if menu {120.0}else{CENTER_X};
            let before=x+((if flipped {WIN_W-center}else{center})*scale).round() as i32;
            let (next,flip)=options_side(x,scale,menu,flipped,(0,0,1920,1040));
            let after=next+((if flip {WIN_W-center}else{center})*scale).round() as i32;
            assert_eq!(before,after);
            let (again,same_flip)=options_side(next,scale,menu,flip,(0,0,1920,1040));
            assert_eq!((again,same_flip),(next,flip),"layout must settle without repeated movement");
        }}}}
    }
}
/// Default time the pointer must rest on a file before the wheel opens (changeable in Settings).
const DEFAULT_HOLD_MS: u64 = 250;
/// How far (physical pixels) the pointer may drift during the hold.
const SLOP: i32 = 8;
/// A held touchpad press drifts more than a mouse button does, so a hold tolerates a wider wobble.
const HOLD_SLOP: i32 = 24;

pub struct Shared {
    visible: AtomicBool,
    rect: Mutex<(i32, i32, i32, i32)>,
    selector: Mutex<Option<selection::Selector>>,
    cancel: AtomicBool,
    pid: Mutex<Option<u32>>,
    hold_ms: AtomicU64,
    shortcut: Mutex<String>,
    /// "wheel" or "menu": how the widget looks when a file is held.
    style: Mutex<String>,
    share: Mutex<Vec<String>>,
    notifications: AtomicBool,
    /// The drop zone stays open when you click elsewhere (you have to drag files from other windows onto it).
    pinned: AtomicBool,
    hit_regions:Mutex<Vec<HitRegion>>,
    ctrl_click: AtomicBool,
    tray_on_close: AtomicBool,
    save_mode: Mutex<String>,
    /// (action, path) pairs from the right-click menu. Explorer starts one process per file, so they are gathered briefly.
    cli_queue: Mutex<Vec<(String, String)>>,
    cli_timer: AtomicBool,
    /// True while the native "choose files" dialog is open, so a second click doesn't open another one.
    browsing: AtomicBool,
}

const DEFAULT_SHORTCUT: &str = "ctrl+alt+KeyH";

#[derive(Serialize, Deserialize, Clone)]
struct Settings {
    hold_ms: u64,
    startup: bool,
    shortcut: String,
    context_menu: bool,
    #[serde(default)]
    native_context_menu: bool,
    style: String,
    /// Turned-on sharing services (ids).
    #[serde(default = "share::default_enabled")]
    share: Vec<String>,
    /// Show a message when a task finishes.
    #[serde(default = "yes")]
    notifications: bool,
    /// Closing the settings window keeps Halo running in the tray.
    #[serde(default = "yes")]
    tray_on_close: bool,
    /// "beside" saves next to the original; "ask" lets the user pick a folder each time.
    #[serde(default = "beside")]
    save_mode: String,
    /// Ctrl + click on a file also opens Halo.
    #[serde(default = "yes")]
    ctrl_click: bool,
}

fn yes() -> bool {
    true
}
fn beside() -> String {
    "beside".to_string()
}

#[derive(Serialize, Clone)]
struct FileInfo {
    path: String,
    name: String,
    ext: String,
    kind: &'static str,
    size: u64,
    capabilities: std::collections::BTreeMap<&'static str, Vec<&'static str>>,
    folder_scan: Option<folders::FolderScan>,
    media: Option<process::MediaInfo>,
}

/// An action that runs straight away when the widget opens (used by the Explorer right-click menu).
#[derive(serde::Deserialize, Serialize, Clone)]
struct AutoAction {
    verb: String,
    option: String,
    value: Option<f64>,
    #[serde(default)]
    extra: Option<serde_json::Value>,
}

#[derive(Serialize, Clone)]
struct ShowPayload {
    files: Vec<FileInfo>,
    flip: bool,
    auto: Option<AutoAction>,
    style: String,
    /// Opened empty (from the tray or by launching Halo): wait for files to be dropped on it.
    dropzone: bool,
    tray_corner: bool,
    requested_verb:Option<String>,
}

fn file_info(path: &str) -> FileInfo {
    let p = Path::new(path);
    let folder_scan = p.is_dir().then(|| folders::scan(p));
    let media = if process::kind_of(p)==process::Kind::Video {process::video_info(p)} else {None};
    let mut capabilities: std::collections::BTreeMap<_,_> = capabilities::VERBS.iter().map(|verb| (*verb, capabilities::options(p, verb))).collect();
    if let Some(scan)=&folder_scan {
        for (verb,options) in &scan.capabilities {capabilities.insert(*verb,options.clone());}
    }
    if let Some(info)=&media {
        if !info.has_audio {
            capabilities.get_mut("convert").unwrap().retain(|option| !["mp3","wav","m4a","aac","ogg","flac","opus"].contains(option));
            capabilities.get_mut("edit").unwrap().retain(|option| *option!="extract_audio");
        }
    }
    FileInfo {
        path: path.to_string(),
        name: p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
        ext: p.extension().map(|e| e.to_string_lossy().to_uppercase()).unwrap_or_default(),
        kind: process::kind_of(p).name(),
        size: process::size_of(p),
        capabilities,
        folder_scan,
        media,
    }
}

fn show_widget(app: &AppHandle, shared: &Arc<Shared>, paths: Vec<String>, x: i32, y: i32, auto: Option<AutoAction>) {show_widget_from(app,shared,paths,x,y,auto,false,None);}
fn show_widget_from(app: &AppHandle, shared: &Arc<Shared>, paths: Vec<String>, x: i32, y: i32, auto: Option<AutoAction>, tray_corner:bool, requested_verb:Option<String>) {
    let Some(win) = app.get_webview_window("main") else { return };
    let monitors = app.available_monitors().unwrap_or_default();
    let Some(mon) = monitors
        .iter()
        .find(|m| {
            let (p, s) = (m.position(), m.size());
            x >= p.x && x < p.x + s.width as i32 && y >= p.y && y < p.y + s.height as i32
        })
        .or(monitors.first())
    else {
        return;
    };
    let scale = mon.scale_factor();
    let style = shared.style.lock().unwrap().clone();
    let area=mon.work_area();
    let WidgetPlacement{x:mut wx,y:mut wy,w,h,mut flip}=widget_placement(x,y,scale,style=="menu",(area.position.x,area.position.y,area.size.width,area.size.height));

    let tray_corner=tray_corner&&style=="menu";if tray_corner{wx=area.position.x+area.size.width as i32-w;wy=area.position.y+area.size.height as i32-h;flip=true;}
    let _ = win.set_size(PhysicalSize::new(w as u32, h as u32));
    let _ = win.set_position(PhysicalPosition::new(wx, wy));
    *shared.rect.lock().unwrap() = (wx, wy, wx + w, wy + h);
    shared.hit_regions.lock().unwrap().clear();
    shared.visible.store(true, Ordering::SeqCst);

    let dropzone = paths.is_empty();
    shared.pinned.store(dropzone, Ordering::SeqCst);
    let payload = ShowPayload { files: paths.iter().map(|p| file_info(p)).collect(), flip, auto, style, dropzone, tray_corner, requested_verb };
    let _ = app.emit("halo://show", payload);
    std::thread::sleep(Duration::from_millis(40));
    let _ = win.show();
}

fn hide_widget_inner(app: &AppHandle, shared: &Arc<Shared>) {
    shared.visible.store(false, Ordering::SeqCst);
    shared.pinned.store(false, Ordering::SeqCst);
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.hide();
        let _ = win.set_focusable(false);
    }
    let _ = app.emit("halo://hide", ());
}

fn fire(app: &AppHandle, shared: &Arc<Shared>, x: i32, y: i32, strict: bool) {
    let paths = {
        let mut guard = shared.selector.lock().unwrap();
        match guard.as_mut() {
            Some(sel) if strict => sel.query(x, y),
            Some(sel) => sel.query(-1, -1),
            None => vec![],
        }
    };
    if !paths.is_empty() {
        show_widget(app, shared, paths, x, y, None);
    }
}

fn worker(app: AppHandle, shared: Arc<Shared>, rx: Receiver<Ev>) {
    let mut pending: Option<(i32, i32, Instant)> = None;
    // A Ctrl + click: opens Halo when the button is released (if the pointer stayed put).
    let mut ctrl_pending: Option<(i32, i32, Instant)> = None;
    loop {
        let ev = match pending {
            Some((_, _, t0)) => {
                let hold = Duration::from_millis(shared.hold_ms.load(Ordering::Relaxed));
                rx.recv_timeout(hold.saturating_sub(t0.elapsed()))
            }
            None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        match ev {
            Ok(Ev::Down(x, y, ctrl)) => {
                if shared.visible.load(Ordering::SeqCst) {
                    let (l, t, r, b) = *shared.rect.lock().unwrap();
                    let inside_window=x>=l&&x<r&&y>=t&&y<b;
                    let regions=shared.hit_regions.lock().unwrap();let inside=inside_window&&(regions.is_empty()||regions.iter().any(|region|region.contains((x-l)as f64,(y-t)as f64)));drop(regions);
                    if inside||shared.browsing.load(Ordering::SeqCst){continue;}
                    // A wheel opened from the tray (no file yet) stays until closed with a right-click, Esc or its X.
                    if !shared.pinned.load(Ordering::SeqCst){hide_widget_inner(&app,&shared);}
                }
                if ctrl && shared.ctrl_click.load(Ordering::Relaxed) {
                    ctrl_pending = Some((x, y, Instant::now()));
                } else {
                    pending = Some((x, y, Instant::now()));
                }
            }
            Ok(Ev::Move(x, y)) => {
                if let Some((px, py, _)) = pending {
                    if (x - px).abs() > HOLD_SLOP || (y - py).abs() > HOLD_SLOP {
                        pending = None;
                    }
                }
                if let Some((px, py, _)) = ctrl_pending {
                    if (x - px).abs() > SLOP || (y - py).abs() > SLOP {
                        ctrl_pending = None;
                    }
                }
            }
            Ok(Ev::Up) => {
                pending = None;
                if let Some((x, y, t0)) = ctrl_pending.take() {
                    if t0.elapsed() < Duration::from_millis(900) {
                        // let Explorer finish updating its selection first
                        std::thread::sleep(Duration::from_millis(160));
                        fire(&app, &shared, x, y, true);
                    }
                }
            }
            Ok(Ev::Esc) => {
                if shared.visible.load(Ordering::SeqCst) {
                    hide_widget_inner(&app, &shared);
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                if let Some((x, y, _)) = pending.take() {
                    fire(&app, &shared, x, y, true);
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}

#[derive(Clone,serde::Deserialize)]struct HitRegion{x:f64,y:f64,w:f64,h:f64,round:bool}
impl HitRegion{fn contains(&self,x:f64,y:f64)->bool{if self.round{let rx=self.w/2.;let ry=self.h/2.;rx>0.&&ry>0.&&((x-self.x-rx)/rx).powi(2)+((y-self.y-ry)/ry).powi(2)<=1.}else{x>=self.x&&x<=self.x+self.w&&y>=self.y&&y<=self.y+self.h}}}
#[tauri::command]fn set_hit_regions(app:AppHandle,shared:State<'_,Arc<Shared>>,regions:Vec<HitRegion>)->Result<(),String>{let win=app.get_webview_window("main").ok_or("Halo window is unavailable.")?;let scale=win.scale_factor().map_err(|e|e.to_string())?;*shared.hit_regions.lock().unwrap()=regions.into_iter().filter(|r|[r.x,r.y,r.w,r.h].iter().all(|v|v.is_finite())&&r.w>0.&&r.h>0.).take(30).map(|r|HitRegion{x:r.x*scale,y:r.y*scale,w:r.w*scale,h:r.h*scale,round:r.round}).collect();Ok(())}
// ------------------------------------------------------------------ commands

#[tauri::command]
fn fit_widget(app:AppHandle,shared:State<'_,Arc<Shared>>,style:String,flipped:bool)->Result<FitResult,String> {
    // Windows owns the drag loop. Do not change the HWND's position until release.
    if unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(0x01) as u16 & 0x8000 != 0 } {
        return Ok(FitResult{flipped,dragging:true});
    }
    let win=app.get_webview_window("main").ok_or("Halo's window is unavailable.")?;
    let position=win.outer_position().map_err(|e|e.to_string())?;
    let scale=win.scale_factor().map_err(|e|e.to_string())?;
    let menu=style=="menu";
    let anchor_x=if menu {120.0}else{CENTER_X};
    let anchor_y=if menu {MENU_ANCHOR}else{CENTER_Y};
    let x=position.x+((if flipped {WIN_W-anchor_x}else{anchor_x})*scale).round() as i32;
    let y=position.y+(anchor_y*scale).round() as i32;
    let monitors=app.available_monitors().map_err(|e|e.to_string())?;
    let mon=monitors.iter().find(|m| {let(p,s)=(m.position(),m.size());x>=p.x&&x<p.x+s.width as i32&&y>=p.y&&y<p.y+s.height as i32}).or(monitors.first()).ok_or("No monitor is available.")?;
    let area=mon.work_area();
    let (next_x,flip)=options_side(position.x,scale,menu,flipped,(area.position.x,area.position.y,area.size.width,area.size.height));
    let size=win.inner_size().map_err(|e|e.to_string())?;
    if position.x!=next_x {win.set_position(PhysicalPosition::new(next_x,position.y)).map_err(|e|e.to_string())?;}
    *shared.rect.lock().unwrap()=(next_x,position.y,next_x+size.width as i32,position.y+size.height as i32);
    Ok(FitResult{flipped:flip,dragging:false})
}

#[tauri::command]
fn hide_widget(app: AppHandle, shared: State<'_, Arc<Shared>>) {
    hide_widget_inner(&app, shared.inner());
}

#[tauri::command]
fn set_typing(app: AppHandle, on: bool) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.set_focusable(on);
        if on {
            let _ = win.set_focus();
        }
    }
}

#[tauri::command]
async fn thumbnail(path: String) -> Option<process::Thumb> {
    tauri::async_runtime::spawn_blocking(move || process::thumbnail(&path)).await.ok().flatten()
}

#[tauri::command]
async fn run_action(app: AppHandle, shared: State<'_, Arc<Shared>>, req: process::Req) -> Result<Vec<process::Outcome>, String> {
    let _job=updates::job()?;
    let shared = shared.inner().clone();
    if req.option == "move_to" {
        hide_widget_inner(&app, &shared);
    }
    tauri::async_runtime::spawn_blocking(move || {
        shared.cancel.store(false, Ordering::SeqCst);
        let progress_app = app.clone();
        let report = move |pct: f64, bytes: u64| {
            let _ = progress_app.emit("halo://progress", serde_json::json!({ "pct": pct, "bytes": bytes }));
        };
        let ctx = process::Ctx { cancel: &shared.cancel, pid: &shared.pid, report: &report };
        let mut outcomes = process::run(&req, &ctx);
        if shared.save_mode.lock().unwrap().as_str() == "ask" {
            process::relocate(&mut outcomes);
        }
        outcomes
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn cancel_action(shared: State<'_, Arc<Shared>>) {
    shared.cancel.store(true, Ordering::SeqCst);
    if let Some(pid) = *shared.pid.lock().unwrap() {
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .creation_flags_hidden()
            .status();
    }
}

#[tauri::command]
fn get_presets() -> Result<Vec<presets::Preset>,String> { let list=presets::list()?;let _=presets::publish_metadata(&list);Ok(list) }
#[tauri::command]
fn save_presets(presets: Vec<presets::Preset>) -> Result<(),String> { presets::save(&presets) }

#[tauri::command]
fn open_path(path: String) -> Result<(), String> {
    process::open_path(&path)
}

#[tauri::command]
fn reveal_path(path: String) -> Result<(), String> {
    process::reveal(&path)
}

#[tauri::command]
async fn copy_files(paths: Vec<String>) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || process::copy_files(&paths)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn copy_text(text: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || process::copy_text(&text)).await.map_err(|e| e.to_string())?
}

trait HiddenExt {
    fn creation_flags_hidden(&mut self) -> &mut Self;
}
impl HiddenExt for std::process::Command {
    fn creation_flags_hidden(&mut self) -> &mut Self {
        use std::os::windows::process::CommandExt;
        self.creation_flags(0x0800_0000)
    }
}

fn cursor() -> (i32, i32) {
    let mut pt = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut pt);
    }
    (pt.x, pt.y)
}

/// The widget with nothing in it yet: drag files from anywhere onto it, then pick an action.
fn open_dropzone(app: &AppHandle) {
    // Callers run on the main (UI) thread, and looking up monitors waits for that same thread, so do the work on another one.
    let app = app.clone();
    std::thread::spawn(move || open_dropzone_now(&app));
}

fn open_dropzone_now(app: &AppHandle) {
    let shared = app.state::<Arc<Shared>>().inner().clone();
    let (x, y) = app
        .primary_monitor()
        .ok()
        .flatten()
        .map(|m| (m.position().x + m.size().width as i32 / 2, m.position().y + m.size().height as i32 / 2))
        .unwrap_or((900, 500));
    show_widget_from(app, &shared, vec![], x, y, None,true,None);
}

#[tauri::command]
fn file_infos(paths: Vec<String>) -> Vec<FileInfo> {
    paths.iter().map(|p| file_info(p)).collect()
}

/// Opens the native "Open File" dialog (single or multi-select) as a fallback to dragging files in.
#[tauri::command]
fn browse_files(app: AppHandle, shared: State<'_, Arc<Shared>>,verb:Option<String>,action:Option<AutoAction>) {
    let shared = shared.inner().clone();
    if shared.browsing.swap(true, Ordering::SeqCst) {
        return; // a dialog is already open
    }
    let mut dlg = app.dialog().file();
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.set_focusable(true);
        let _ = win.set_focus();
        dlg = dlg.set_parent(&win);
    }
    dlg.pick_files(move |picked| {
        shared.browsing.store(false, Ordering::SeqCst);
        let Some(picked) = picked else { return };
        let paths: Vec<String> = picked.into_iter().filter_map(|p| p.into_path().ok()).map(|p| p.to_string_lossy().to_string()).collect();
        if paths.is_empty() {
            return;
        }
        let (x, y) = {
            let (l, t, r, b) = *shared.rect.lock().unwrap();
            ((l + r) / 2, (t + b) / 2)
        };
        show_widget_from(&app, &shared, paths, x, y, action,false,verb);
    });
}

#[tauri::command]
async fn open_settings(app: AppHandle) {
    open_home(&app);
}

/// Opens (or brings forward) the Halo home window. Used by the tray icon and by a second launch.
fn open_home(app: &AppHandle) {
    let app=app.clone();
    std::thread::spawn(move || {
    if let Some(win) = app.get_webview_window("home") {
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
        return;
    }
    // Cap height to a share of the screen's work area so Settings doesn't overflow shorter/scaled displays.
    let height = app
        .primary_monitor()
        .ok()
        .flatten()
        .map(|m| {
            let area = m.work_area();
            (area.size.height as f64 / m.scale_factor() * 0.7).clamp(600.0, 820.0)
        })
        .unwrap_or(820.0);
    let _ = tauri::WebviewWindowBuilder::new(&app, "home", tauri::WebviewUrl::App("home.html".into()))
        .title("Halo")
        .inner_size(1000.0, height)
        .resizable(false)
        .maximizable(false)
        .center()
        .build();
    });
}

/// The small window that shows results ("Compression Complete!", "Moved to the Recycle Bin") bottom-right.
/// It is created hidden at start-up so its page is already loaded when the first result arrives.
fn ensure_notice(app: &AppHandle) -> Option<tauri::WebviewWindow> {
    if let Some(win) = app.get_webview_window("notice") {
        return Some(win);
    }
    tauri::WebviewWindowBuilder::new(app, "notice", tauri::WebviewUrl::App("notice.html".into()))
        .title("Halo notice")
        .inner_size(380.0, 260.0)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .shadow(false)
        .visible(false)
        .focused(false)
        .focusable(false)
        .build()
        .ok()
}

#[tauri::command]
fn show_notice(app: AppHandle, notice: serde_json::Value) {
    {
        let shared = app.state::<Arc<Shared>>();
        let quiet = !shared.notifications.load(Ordering::Relaxed);
        let kind = notice["kind"].as_str().unwrap_or("");
        let is_error = notice["tone"] == "error";
        if quiet && !is_error && kind != "link" && kind != "update" {
            return;
        }
    }
    let Some(win) = ensure_notice(&app) else { return };
    let (x, y) = cursor();
    let monitors = app.available_monitors().unwrap_or_default();
    let Some(mon) = monitors
        .iter()
        .find(|m| {
            let (p, s) = (m.position(), m.size());
            x >= p.x && x < p.x + s.width as i32 && y >= p.y && y < p.y + s.height as i32
        })
        .or(monitors.first())
    else {
        return;
    };
    // Bottom-right of the work area, so it sits just above the taskbar like a Windows notification.
    let scale = mon.scale_factor();
    let area = mon.work_area();
    let (logical_w, logical_h) = if notice["kind"] != "toast" { (290.0, 330.0) } else { (290.0, 180.0) };
    let (w, h) = ((logical_w * scale) as i32, (logical_h * scale) as i32);
    let margin = (0.0 * scale) as i32;
    let nx = area.position.x + area.size.width as i32 - w - margin;
    let ny = area.position.y + area.size.height as i32 - h - margin;
    let _ = win.set_size(PhysicalSize::new(w as u32, h as u32));
    let _ = win.set_position(PhysicalPosition::new(nx, ny));
    let _ = win.emit("halo://notice", notice);
    std::thread::sleep(Duration::from_millis(30));
    let _ = win.show();
}

#[tauri::command]
fn hide_notice(app: AppHandle) {
    if let Some(win) = app.get_webview_window("notice") {
        let _ = win.hide();
    }
}

fn settings_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join("settings.json"))
}

const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";

fn startup_enabled() -> bool {
    use std::os::windows::process::CommandExt;
    std::process::Command::new("reg")
        .args(["query", RUN_KEY, "/v", "Halo"])
        .creation_flags(0x0800_0000)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn set_startup(on: bool) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let mut cmd = std::process::Command::new("reg");
    if on {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        cmd.args(["add", RUN_KEY, "/v", "Halo", "/t", "REG_SZ", "/d"])
            .arg(format!("\"{}\" --hidden", exe.display()))
            .arg("/f");
    } else {
        cmd.args(["delete", RUN_KEY, "/v", "Halo", "/f"]);
    }
    cmd.creation_flags(0x0800_0000);
    // Deleting a value that isn't there reports failure; that's fine.
    let status = cmd.output().map_err(|e| e.to_string())?.status;
    if on && !status.success() {
        return Err("Windows would not let Halo start with your computer.".into());
    }
    Ok(())
}

#[tauri::command]
fn get_settings(shared: State<'_, Arc<Shared>>) -> Settings {
    Settings {
        hold_ms: shared.hold_ms.load(Ordering::Relaxed),
        startup: startup_enabled(),
        shortcut: shared.shortcut.lock().unwrap().clone(),
        context_menu: context_menu_enabled(),
        native_context_menu: sparse_installed(),
        style: shared.style.lock().unwrap().clone(),
        share: shared.share.lock().unwrap().clone(),
        notifications: shared.notifications.load(Ordering::Relaxed),
        ctrl_click: shared.ctrl_click.load(Ordering::Relaxed),
        tray_on_close: shared.tray_on_close.load(Ordering::Relaxed),
        save_mode: shared.save_mode.lock().unwrap().clone(),
    }
}

#[tauri::command]
fn set_settings(app: AppHandle, shared: State<'_, Arc<Shared>>, settings: Settings) -> Result<(), String> {
    let hold = settings.hold_ms.clamp(200, 1500);
    shared.hold_ms.store(hold, Ordering::Relaxed);

    // Swap the global shortcut; if Windows refuses the new one, keep the old one working.
    let wanted = settings.shortcut.trim().to_string();
    let current = shared.shortcut.lock().unwrap().clone();
    if !wanted.is_empty() && wanted != current {
        let keys = app.global_shortcut();
        let _ = keys.unregister(current.as_str());
        if keys.register(wanted.as_str()).is_err() {
            let _ = keys.register(current.as_str());
            return Err("That shortcut is taken or not allowed. Please try another one.".into());
        }
        *shared.shortcut.lock().unwrap() = wanted;
    }

    *shared.style.lock().unwrap() = if settings.style == "menu" { "menu".to_string() } else { "wheel".to_string() };
    shared.notifications.store(settings.notifications, Ordering::Relaxed);
    shared.ctrl_click.store(settings.ctrl_click, Ordering::Relaxed);
    shared.tray_on_close.store(settings.tray_on_close, Ordering::Relaxed);
    *shared.save_mode.lock().unwrap() = if settings.save_mode == "ask" { "ask".to_string() } else { "beside".to_string() };
    *shared.share.lock().unwrap() = settings.share.iter().filter(|id| share::PROVIDERS.iter().any(|p| p.id == id.as_str())).cloned().collect();

    if let Some(path) = settings_path(&app) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let shortcut = shared.shortcut.lock().unwrap().clone();
        let style = shared.style.lock().unwrap().clone();
        let _ = std::fs::write(path, serde_json::json!({ "hold_ms": hold, "shortcut": shortcut, "style": style, "share": shared.share.lock().unwrap().clone(), "notifications": settings.notifications, "ctrl_click": settings.ctrl_click, "tray_on_close": settings.tray_on_close, "save_mode": shared.save_mode.lock().unwrap().clone() }).to_string());
    }
    if settings.context_menu != context_menu_enabled() {
        if !settings.context_menu && sparse_installed() {
            use std::os::windows::process::CommandExt;
            let _ = std::process::Command::new("powershell")
                .args(["-NoProfile", "-Command", "Get-AppxPackage Halo.Wheel.ContextMenu | Remove-AppxPackage"])
                .creation_flags(0x0800_0000)
                .output();
        }
        set_context_menu(settings.context_menu)?;
    }
    set_startup(settings.startup)
}

// ------------------------------------------------------------------ sharing and system checks

#[derive(Serialize)]
struct AppInfo {
    version: &'static str,
    settings_file: String,
}

#[tauri::command]
fn app_info(app: AppHandle) -> AppInfo {
    AppInfo { version: env!("CARGO_PKG_VERSION"), settings_file: settings_path(&app).map(|p| p.display().to_string()).unwrap_or_default() }
}

#[tauri::command]
async fn scan_folder(path: String) -> Option<folders::FolderScan> {
    tauri::async_runtime::spawn_blocking(move || folders::scan(Path::new(&path))).await.ok()
}

#[derive(Serialize)]
struct QrResult {
    out_path: String,
    note: String,
}

/// Makes a QR picture beside the file: of its text when it is a short text file, otherwise of an upload link.
#[tauri::command]
async fn make_qr(shared: State<'_, Arc<Shared>>, path: String) -> Result<QrResult, String> {
    let _job=updates::job()?;
    let enabled = shared.share.lock().unwrap().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let p = Path::new(&path);
        if p.is_dir() {
            return Err("Folders can't be turned into a QR code.".to_string());
        }
        let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        let small_text = matches!(ext.as_str(), "txt" | "md" | "csv" | "log") && process::size_of(p) <= 1500;
        let (data, note) = match std::fs::read_to_string(p).ok().filter(|_| small_text) {
            Some(text) => (text.trim().to_string(), "A QR code of the text in the file".to_string()),
            None => {
                let link = share::upload(p, &enabled)?;
                (link.url.clone(), format!("A QR code for the {} link", link.provider))
            }
        };
        let out = qr::write(p, &data)?;
        Ok(QrResult { out_path: out.to_string_lossy().to_string(), note })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn list_providers() -> Vec<share::Provider> {
    share::PROVIDERS.to_vec()
}

/// Uploads a copy of each file to the turned-on services and returns the links (also copied to the clipboard).
#[tauri::command]
async fn share_files(shared: State<'_, Arc<Shared>>, paths: Vec<String>) -> Result<Vec<share::ShareLink>, String> {
    let _job=updates::job()?;
    let enabled = shared.share.lock().unwrap().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut links = Vec::new();
        for p in &paths {
            links.push(share::upload(Path::new(p), &enabled)?);
        }
        let _ = process::copy_text(&links.iter().map(|l| l.url.as_str()).collect::<Vec<_>>().join("\n"));
        Ok(links)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn get_requirements() -> Vec<system::Check> {
    tauri::async_runtime::spawn_blocking(system::checks).await.unwrap_or_default()
}

#[tauri::command]
async fn install_requirement(id: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || system::install(&id)).await.map_err(|e| e.to_string())?
}

// ------------------------------------------------------------------ Explorer right-click menu
const MENU_ROOTS: [&str; 4] = [
    r"HKCU\Software\Classes\*\shell\Halo",
    r"HKCU\Software\Classes\Directory\shell\Halo",
    r"HKCU\Software\Classes\SystemFileAssociations\image\shell\Halo",
    r"HKCU\Software\Classes\SystemFileAssociations\video\shell\Halo",
];
const MENU_SETS: [&str; 3] = ["HaloMenu.Any", "HaloMenu.Image", "HaloMenu.Video"];

fn context_menu_enabled() -> bool {
    use std::os::windows::process::CommandExt;
    std::process::Command::new("reg")
        .args(["query", MENU_ROOTS[0]])
        .creation_flags(0x0800_0000)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
        || sparse_installed()
}

/// True while the sparse package (packaging/install-sparse.ps1) provides the native Windows 11 menu.
/// Then the classic entries stay off so Explorer does not show Halo twice.
fn sparse_installed() -> bool {
    use std::os::windows::process::CommandExt;
    std::process::Command::new("reg")
        .args(["query", r"HKCU\Software\Classes\PackagedCom\Package", "/f", "Halo.Wheel.ContextMenu", "/k"])
        .creation_flags(0x0800_0000)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn reg_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// The menu entries: (label, action). An action is `verb:option` or `verb:option:value`, or `wheel`.
fn menu_items(set: &str) -> Vec<(&'static str, &'static str)> {
    let mut items = vec![("Open Halo wheel", "wheel")];
    match set {
        "HaloMenu.Image" => items.extend([
            ("Compress: Small", "compress:small"),
            ("Compress: Medium", "compress:medium"),
            ("Compress: Large", "compress:large"),
            ("Convert to JPG", "convert:jpg"),
            ("Convert to PNG", "convert:png"),
            ("Convert to WebP", "convert:webp"),
            ("Convert to PDF", "convert:pdf"),
            ("Resize: Half size", "resize:percent:50"),
            ("Resize: Quarter size", "resize:percent:25"),
            ("Resize: For email", "resize:email"),
        ]),
        "HaloMenu.Video" => items.extend([
            ("Compress: Small", "compress:small"),
            ("Compress: Medium", "compress:medium"),
            ("Compress: Large", "compress:large"),
            ("Convert to MP4", "convert:mp4"),
            ("Convert to WebM", "convert:webm"),
            ("Convert to GIF", "convert:gif"),
            ("Convert to MP3", "convert:mp3"),
            ("Resize: Half size", "resize:percent:50"),
        ]),
        _ => items.push(("Zip it", "compress:zip")),
    }
    items
}

fn menu_reg(exe: &str) -> String {
    let icon = reg_escape(&format!("\"{exe}\",0"));
    let mut s = String::from("Windows Registry Editor Version 5.00\r\n\r\n");
    let roots = [(MENU_ROOTS[0], MENU_SETS[0]), (MENU_ROOTS[1], MENU_SETS[0]), (MENU_ROOTS[2], MENU_SETS[1]), (MENU_ROOTS[3], MENU_SETS[2])];
    for (root, set) in roots {
        s += &format!("[{root}]\r\n\"MUIVerb\"=\"Halo\"\r\n\"Icon\"=\"{icon}\"\r\n\"ExtendedSubCommandsKey\"=\"{set}\"\r\n\r\n");
    }
    for set in MENU_SETS {
        for (i, (label, action)) in menu_items(set).iter().enumerate() {
            let command = reg_escape(&format!("\"{exe}\" --action {action} \"%1\""));
            s += &format!(
                "[HKEY_CURRENT_USER\\Software\\Classes\\{set}\\shell\\{i:02}]\r\n\"MUIVerb\"=\"{label}\"\r\n\r\n[HKEY_CURRENT_USER\\Software\\Classes\\{set}\\shell\\{i:02}\\command]\r\n@=\"{command}\"\r\n\r\n"
            );
        }
    }
    s.replace("[HKCU\\", "[HKEY_CURRENT_USER\\")
}

fn set_context_menu(on: bool) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let run = |args: &[&str]| {
        std::process::Command::new("reg").args(args).creation_flags(0x0800_0000).output().map_err(|e| e.to_string())
    };
    if on && !sparse_installed() {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let configured = std::env::var_os("HALO_NATIVE_MENU_DIR").map(std::path::PathBuf::from)
            .filter(|p|p.join("halo.exe").is_file()&&p.join("halo_menu.dll").is_file()&&p.join("HaloContextMenu.msix").is_file());
        let directory = configured.as_deref().or_else(||exe.parent()).ok_or("Halo's installation folder is missing.")?;
        let package = directory.join("HaloContextMenu.msix");
        if package.is_file() {
            let result = std::process::Command::new("powershell")
                .args(["-NoProfile","-NonInteractive","-Command","Add-AppxPackage -Path $env:HALO_MSIX -ExternalLocation $env:HALO_INSTALL -ErrorAction Stop"])
                .env("HALO_MSIX", &package).env("HALO_INSTALL",directory).creation_flags(0x0800_0000)
                .output().map_err(|e| e.to_string())?;
            if !result.status.success() {return Err(format!("Windows could not register Halo's main context menu: {}", String::from_utf8_lossy(&result.stderr).trim()));}
        }
    }
    if on && !sparse_installed() {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let text = menu_reg(&exe.display().to_string());
        let mut bytes = vec![0xFF, 0xFE];
        bytes.extend(text.encode_utf16().flat_map(|u| u.to_le_bytes()));
        let file = std::env::temp_dir().join("halo-menu.reg");
        std::fs::write(&file, bytes).map_err(|e| e.to_string())?;
        let out = run(&["import", &file.display().to_string()])?;
        let _ = std::fs::remove_file(&file);
        if !out.status.success() {
            return Err("Windows would not add Halo to the right-click menu.".into());
        }
    } else {
        for root in MENU_ROOTS {
            let _ = run(&["delete", root, "/f"]);
        }
        for set in MENU_SETS {
            let _ = run(&["delete", &format!(r"HKCU\Software\Classes\{set}"), "/f"]);
        }
    }
    Ok(())
}

/// Handles `halo.exe --action <verb[:option[:value]]> <path>` from the right-click menu.
/// Returns false when the arguments are not a menu action.
fn handle_cli(app: &AppHandle, args: &[String]) -> bool {
    let Some(i) = args.iter().position(|a| a == "--action") else { return false };
    let (Some(action), Some(path)) = (args.get(i + 1), args.get(i + 2)) else { return true };
    let shared = app.state::<Arc<Shared>>().inner().clone();
    shared.cli_queue.lock().unwrap().push((action.clone(), path.clone()));
    if !shared.cli_timer.swap(true, Ordering::SeqCst) {
        let app = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(450));
            shared.cli_timer.store(false, Ordering::SeqCst);
            let queued = std::mem::take(&mut *shared.cli_queue.lock().unwrap());
            let Some((first_action, _)) = queued.first().cloned() else { return };
            let paths: Vec<String> = queued.into_iter().filter(|(a, _)| *a == first_action).map(|(_, p)| p).collect();
            let mut parts = first_action.split(':');
            let verb = parts.next().unwrap_or("wheel").to_string();
            let mut option = parts.next().unwrap_or("").to_string();
            let value = parts.next().and_then(|v| v.parse::<f64>().ok());
            // Several files zipped from the menu go into one archive.
            if verb == "compress" && option == "zip" && paths.len() > 1 {
                option = "zip_all".to_string();
            }
            let auto = if verb == "wheel" { None } else { Some(AutoAction { verb, option, value, extra:None }) };
            let (x, y) = cursor();
            show_widget(&app, &shared, paths, x, y, auto);
        });
    }
    true
}

#[tauri::command]
fn quit_app(app: AppHandle) {
    if recorder::active(){tauri::async_runtime::spawn(async move {let _=recorder::recorder_stop(app.clone()).await;app.exit(0);});}else{app.exit(0);}
}

fn main() {
    // `halo.exe --check` prints the system check; `halo.exe --share <file>` uploads a file and prints the link.
    {
        let args: Vec<String> = std::env::args().collect();
        if args.iter().any(|a| a == "--check") {
            println!("{}", serde_json::to_string_pretty(&system::checks()).unwrap_or_default());
            return;
        }
        if let Some(i) = args.iter().position(|a| a == "--install") {
            match args.get(i + 1).map(|id| system::install(id)) {
                Some(Ok(msg)) => println!("{msg}"),
                Some(Err(e)) => {
                    eprintln!("{e}");
                    std::process::exit(1);
                }
                None => eprintln!("usage: halo --install ffmpeg|edge|pdfium"),
            }
            return;
        }
        if let Some(i) = args.iter().position(|a| a == "--share") {
            let enabled = share::default_enabled();
            match args.get(i + 1).map(|p| share::upload(Path::new(p), &enabled)) {
                Some(Ok(link)) => println!("{}", serde_json::to_string(&link).unwrap_or_default()),
                Some(Err(e)) => {
                    eprintln!("{e}");
                    std::process::exit(1);
                }
                None => eprintln!("usage: halo --share <file>"),
            }
            return;
        }
    }

    // `halo.exe --context-menu on|off` adds or removes the Explorer right-click entries without opening anything.
    let cli: Vec<String> = std::env::args().collect();
    if let Some(i) = cli.iter().position(|a| a == "--context-menu") {
        let on = cli.get(i + 1).map(|v| v == "on").unwrap_or(false);
        if let Err(e) = set_context_menu(on) {
            eprintln!("{e}");
            std::process::exit(1);
        }
        return;
    }

    tauri::Builder::default()
        // Must be registered first: a second launch hands over to this running copy instead of starting again.
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            if !handle_cli(app, &argv) {
                open_dropzone(app);
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state == ShortcutState::Pressed {
                        if recorder::config().shortcut.parse::<Shortcut>().ok().as_ref()==Some(shortcut) {recorder::open(app);return;}
                        let app = app.clone();
                        std::thread::spawn(move || {
                            let shared = app.state::<Arc<Shared>>().inner().clone();
                            let (x, y) = cursor();
                            fire(&app, &shared, x, y, false);
                        });
                    }
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            updates::update_status, updates::check_updates, updates::install_update,
            set_hit_regions, fit_widget,
            recorder::recorder_debug_preview, recorder::recorder_test_scene, recorder::open_recorder,
            recorder::recorder_options,
            recorder::recorder_save_config,
            recorder::recorder_pick_folder,
            recorder::recorder_annotation_update,recorder::recorder_annotations_flushed,recorder::recorder_log_error,recorder::recorder_countdown_open,recorder::recorder_countdown_data,recorder::recorder_close_countdown,recorder::recorder_cancel_countdown,recorder::recorder_window_thumbnail,recorder::recorder_annotation_data,recorder::recorder_annotation_add,recorder::recorder_annotation_end,recorder::recorder_annotations_edit,recorder::recorder_annotation_snapshot,recorder::recorder_output_info,recorder::recorder_recycle_output,recorder::recorder_cursor_preview, recorder::recorder_toggle_audio, recorder::recorder_preview_start, recorder::recorder_preview_stop, recorder::recorder_state,
            recorder::recorder_select_area,
            recorder::recorder_selection_data,
            recorder::recorder_cancel, recorder::recorder_cancel_area, recorder::recorder_commit_area,
            recorder::recorder_start,
            recorder::recorder_pause,
            recorder::recorder_resume,
            recorder::recorder_stop,
            hide_widget,
            set_typing,
            thumbnail,
            run_action,
            get_presets,
            save_presets,
            cancel_action,
            open_path,
            reveal_path,
            copy_files,
            copy_text,
            get_settings,
            set_settings,
            quit_app,
            show_notice,
            hide_notice,
            list_providers,
            file_infos,
            open_settings,
            browse_files,
            scan_folder,
            make_qr,
            app_info,
            share_files,
            get_requirements,
            install_requirement
        ])
        .setup(|app| {
            // Starting visible (then hiding at once) works around a WRY/WebView2 bug where a window
            // created with visible:false never initializes OLE drag-and-drop (tauri#14643).
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.hide();
            }
            let shared = Arc::new(Shared {
                visible: AtomicBool::new(false),
                rect: Mutex::new((0, 0, 0, 0)),
                selector: Mutex::new(None),
                cancel: AtomicBool::new(false),
                pid: Mutex::new(None),
                hold_ms: AtomicU64::new(DEFAULT_HOLD_MS),
                shortcut: Mutex::new(DEFAULT_SHORTCUT.to_string()),
                style: Mutex::new("wheel".to_string()),
                share: Mutex::new(share::default_enabled()),
                notifications: AtomicBool::new(true),
                pinned: AtomicBool::new(false),
                hit_regions:Mutex::new(vec![]),
                ctrl_click: AtomicBool::new(true),
                tray_on_close: AtomicBool::new(true),
                save_mode: Mutex::new("beside".to_string()),
                cli_queue: Mutex::new(Vec::new()),
                cli_timer: AtomicBool::new(false),
                browsing: AtomicBool::new(false),
            });
            if let Some(saved) = settings_path(app.handle())
                .and_then(|p| std::fs::read_to_string(p).ok())
                .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            {
                if let Some(ms) = saved["hold_ms"].as_u64() {
                    shared.hold_ms.store(ms.clamp(200, 1500), Ordering::Relaxed);
                }
                if let Some(sc) = saved["shortcut"].as_str().filter(|s| !s.is_empty()) {
                    *shared.shortcut.lock().unwrap() = sc.to_string();
                }
                if let Some(v) = saved["ctrl_click"].as_bool() {
                    shared.ctrl_click.store(v, Ordering::Relaxed);
                }
                if let Some(v) = saved["notifications"].as_bool() {
                    shared.notifications.store(v, Ordering::Relaxed);
                }
                if let Some(v) = saved["tray_on_close"].as_bool() {
                    shared.tray_on_close.store(v, Ordering::Relaxed);
                }
                if saved["save_mode"].as_str() == Some("ask") {
                    *shared.save_mode.lock().unwrap() = "ask".to_string();
                }
                if let Some(list) = saved["share"].as_array() {
                    *shared.share.lock().unwrap() = list.iter().filter_map(|v| v.as_str().map(String::from)).collect();
                }
                if saved["style"].as_str() == Some("menu") {
                    *shared.style.lock().unwrap() = "menu".to_string();
                }
            }
            app.manage(shared.clone());
            updates::start(app.handle().clone());

            ensure_notice(app.handle());

            // Launching Halo by hand opens its window; starting with Windows (--hidden) stays in the tray.
            let args: Vec<String> = std::env::args().collect();
            if args.iter().any(|a| a == "--action") {
                handle_cli(app.handle(), &args);
            } else if !args.iter().any(|a| a == "--hidden") {
                open_dropzone(app.handle());
            }

            // Start the selection helper in the background so launch stays instant.
            let helper_shared = shared.clone();
            std::thread::spawn(move || {
                if let Ok(sel) = selection::Selector::start() {
                    *helper_shared.selector.lock().unwrap() = Some(sel);
                }
            });

            let (tx, rx) = channel();
            hook::start(tx);
            let handle = app.handle().clone();
            let worker_shared = shared.clone();
            std::thread::spawn(move || worker(handle, worker_shared, rx));

            let saved_shortcut = shared.shortcut.lock().unwrap().clone();
            if app.global_shortcut().register(saved_shortcut.as_str()).is_err() {
                // The saved shortcut is no longer available: fall back to the default.
                *shared.shortcut.lock().unwrap() = DEFAULT_SHORTCUT.to_string();
                let _ = app.global_shortcut().register(DEFAULT_SHORTCUT);
            }

            let hint = MenuItem::with_id(app, "hint", "Hold a file, or use your shortcut", false, None::<&str>)?;
            let _=app.global_shortcut().register(recorder::config().shortcut.as_str());
            let record=MenuItem::with_id(app,"record","Record screen",true,None::<&str>)?;
            let open = MenuItem::with_id(app, "open", "Open Halo", true, None::<&str>)?;
            let settings = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit Halo", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &record, &settings, &hint, &quit])?;
            TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("Halo: hold any file to open the wheel")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "open" => open_dropzone(app),
                    "record" => recorder::open(app),
                    "settings" => open_home(app),
                    "quit" => quit_app(app.clone()),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                        open_dropzone(tray.app_handle());
                    }
                })
                .build(app)?;

            // `HALO_DEMO=<file>` opens the wheel on that file in the middle of the primary screen.
            if let Ok(file) = std::env::var("HALO_DEMO") {
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(7000));
                    let shared = handle.state::<Arc<Shared>>().inner().clone();
                    let (x, y) = handle
                        .primary_monitor()
                        .ok()
                        .flatten()
                        .map(|m| (m.position().x + m.size().width as i32 / 2, m.position().y + m.size().height as i32 / 2))
                        .unwrap_or((900, 500));
                    show_widget(&handle, &shared, vec![file], x, y, None);
                });
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the widget only hides it; Halo keeps running in the tray.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label()=="recorder"{api.prevent_close();let app=window.app_handle().clone();tauri::async_runtime::spawn(async move{let _=recorder::cancel_recorder(app).await;});return;}
                if window.label()=="recorder-countdown"{api.prevent_close();recorder::cancel_countdown(window.app_handle());return;}
                if window.label()=="recorder-selector"{api.prevent_close();recorder::cancel_area(window.app_handle());return;}
                if window.label()=="recorder-test"{return;}
                let keep_running = window.app_handle().state::<Arc<Shared>>().tray_on_close.load(Ordering::Relaxed);
                if window.label() == "home" && !keep_running {
                    api.prevent_close();
                    quit_app(window.app_handle().clone());
                    return;
                }
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building Halo")
        .run(|_app, event| {
            // Stay alive when no window is open. An explicit Quit (exit code set) still ends the app.
            if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
                if code.is_none() {
                    api.prevent_exit();
                }
            }
        });
}



#[cfg(test)]mod hit_region_tests{use super::*;#[test]fn wheel_corner_dismisses_but_centre_does_not(){let r=HitRegion{x:10.,y:20.,w:320.,h:320.,round:true};assert!(r.contains(170.,180.));assert!(!r.contains(12.,22.));assert!(!r.contains(5.,180.));}#[test]fn menu_rect_keeps_actions_interactive(){let r=HitRegion{x:28.,y:28.,w:184.,h:310.,round:false};assert!(r.contains(28.,28.));assert!(r.contains(200.,320.));assert!(!r.contains(220.,320.));}}
