//! Halo's entry in the Windows 11 main context menu: an `IExplorerCommand` "Halo" submenu.
//! It only launches `halo.exe --action <verb[:option[:value]]> "<path>"`, same as the classic registry verbs.

use std::ffi::c_void;
#[path = "../../shared/capabilities.rs"]
mod capabilities;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, AtomicU8, Ordering};
use std::sync::Arc;

use windows::core::{implement, Interface, Ref, Result, BOOL, GUID, HRESULT, IUnknown, PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, E_NOINTERFACE, E_NOTIMPL, E_POINTER, HMODULE, S_FALSE, S_OK,
};
use windows::Win32::System::Com::{CoTaskMemFree, IBindCtx, IClassFactory, IClassFactory_Impl};
use windows::Win32::System::LibraryLoader::{
    GetModuleFileNameW, GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
    GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
};
use windows::Win32::UI::Shell::{
    IEnumExplorerCommand, IEnumExplorerCommand_Impl, IExplorerCommand, IExplorerCommand_Impl, IShellItemArray,
    SHStrDupW, ECF_DEFAULT, ECF_HASSUBCOMMANDS, ECS_ENABLED, ECS_HIDDEN, SIGDN_FILESYSPATH,
};

/// CLSID of the root command. Must match `packaging/AppxManifest.xml`.
const CLSID_HALO_MENU: GUID = GUID::from_u128(0x7c3a1d52_9b64_4e0a_8f2d_5a1b6c9e4d31);

const IMG: u8 = 1;
const VID: u8 = 2;
const OTH: u8 = 4;
const DIR: u8 = 8;
const ALL: u8 = IMG | VID | OTH | DIR;

const IMAGE_EXT: &[&str] = capabilities::IMAGE_EXT;
const VIDEO_EXT: &[&str] = capabilities::VIDEO_EXT;

static LOCKS: AtomicU32 = AtomicU32::new(0);
/// Kind of the last real selection seen; used when Explorer asks a sub-command for its state without items.
static LAST_KIND: AtomicU8 = AtomicU8::new(0);
static LAST_SELECTION: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// One menu entry: a leaf with an action, or a group with children.
struct Node {
    title: String,
    action: Option<String>,
    preset_target: Option<String>,
    kinds: u8,
    children: Vec<Arc<Node>>,
}

fn leaf(title: &'static str, action: &'static str, kinds: u8) -> Arc<Node> {
    Arc::new(Node { title:title.into(), action: Some(action.into()), preset_target:None, kinds, children: vec![] })
}

fn group(title: &'static str, kinds: u8, children: Vec<Arc<Node>>) -> Arc<Node> {
    Arc::new(Node { title:title.into(), action: None, preset_target:None, kinds, children })
}

/// Major actions only; the classic menu (src-tauri/src/main.rs) keeps the full list.
/// Windows 11 shows only one submenu level, so these are flat items under "Halo".
fn build_tree() -> Arc<Node> {
    let mut tree=group(
        "Halo",
        ALL,
        vec![
            leaf("Open in Halo", "wheel", ALL),
            leaf("Compress", "compress:auto", ALL),
            leaf("Convert to JPG", "convert:jpg", ALL),
            leaf("Convert to WebP", "convert:webp", ALL),
            leaf("Resize to half size", "resize:percent:50", ALL),
            leaf("Convert to MP4", "convert:mp4", ALL),
            leaf("Convert to MP3", "convert:mp3", ALL),
            leaf("Zip it", "compress:zip", ALL),
            leaf("Upload & get link", "share:link", ALL),
        ],
    );
    // Packaged COM surrogates have an isolated AppData view. Read their package
    // local metadata mirror; full settings remain in Halo's normal data folder.
    if let Some(data)=preset_metadata() {
        if data.len()<128*1024 {if let Ok(presets)=serde_json::from_str::<Vec<serde_json::Value>>(&data) {
            if let Some(root)=Arc::get_mut(&mut tree) {for p in presets.into_iter().take(32) {
                let (Some(id),Some(name),Some(target))=(p["id"].as_str(),p["name"].as_str(),p["target"].as_str()) else {continue};
                if id.is_empty()||id.len()>48||!id.chars().all(|c|c.is_ascii_alphanumeric()||c=='-'||c=='_')||name.len()>80 {continue;}
                root.children.push(Arc::new(Node{title:name.into(),action:Some(format!("preset:{id}")),preset_target:Some(target.into()),kinds:ALL,children:vec![]}));
            }}
        }}
    }
    tree
}

fn preset_metadata()->Option<String> {
    if let Ok(data)=windows::Storage::ApplicationData::Current() {
        if let Ok(path)=data.LocalFolder().and_then(|f|f.Path()) {
            if let Ok(text)=std::fs::read_to_string(std::path::PathBuf::from(path.to_string()).join("halo-presets.json")) {return Some(text);}
        }
    }
    let base=std::env::var_os("APPDATA")?;
    std::fs::read_to_string(std::path::PathBuf::from(base).join("com.halo.wheel").join("presets.json")).ok()
}
/// Folder holding this DLL; halo.exe sits next to it (the sparse package's external location).
fn install_dir() -> Option<PathBuf> {
    unsafe {
        let mut module = HMODULE::default();
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            PCWSTR(install_dir as *const () as *const u16),
            &mut module,
        )
        .ok()?;
        let mut buf = [0u16; 1024];
        let n = GetModuleFileNameW(Some(module), &mut buf) as usize;
        if n == 0 || n >= buf.len() {
            return None;
        }
        PathBuf::from(String::from_utf16_lossy(&buf[..n])).parent().map(|p| p.to_path_buf())
    }
}

fn halo_exe() -> Option<PathBuf> {
    install_dir().map(|d| d.join("halo.exe"))
}

/// File-system paths of the selection (items without one, e.g. Control Panel, are skipped).
fn selected_paths(items: Option<&IShellItemArray>) -> Vec<String> {
    let mut out = Vec::new();
    let Some(items) = items else { return out };
    unsafe {
        let count = items.GetCount().unwrap_or(0);
        for i in 0..count {
            let Ok(item) = items.GetItemAt(i) else { continue };
            let Ok(name) = item.GetDisplayName(SIGDN_FILESYSPATH) else { continue };
            if let Ok(s) = name.to_string() {
                out.push(s);
            }
            CoTaskMemFree(Some(name.0 as *const c_void));
        }
    }
    out
}

/// What the whole selection is: all images, all videos, or anything else (documents, folders, mixed).
fn classify(paths: &[String]) -> u8 {
    let kind = |p: &String| -> u8 {
        if std::path::Path::new(p).is_dir() {
            return DIR;
        }
        let ext = std::path::Path::new(p)
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if IMAGE_EXT.contains(&ext.as_str()) {
            IMG
        } else if VIDEO_EXT.contains(&ext.as_str()) {
            VID
        } else {
            OTH
        }
    };
    if paths.iter().any(|p| std::path::Path::new(p).is_dir()) {
        return DIR;
    }
    let mut kinds = paths.iter().map(kind);
    let first = kinds.next().unwrap_or(OTH);
    if kinds.all(|k| k == first) {
        first
    } else {
        OTH
    }
}

/// Diagnostics: appends to %TEMP%\halo_menu.log only while an empty %TEMP%\halo_menu.debug file exists.
fn log(msg: &str) {
    use std::io::Write;
    let tmp = std::env::temp_dir();
    if tmp.join("halo_menu.debug").exists() {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(tmp.join("halo_menu.log")) {
            let _ = writeln!(f, "{msg}");
        }
    }
}

fn wide(s: &str) -> Result<PWSTR> {
    let w: Vec<u16> = s.encode_utf16().chain(Some(0)).collect();
    unsafe { SHStrDupW(PCWSTR(w.as_ptr())) }
}

#[implement(IExplorerCommand)]
struct Command {
    node: Arc<Node>,
}

impl IExplorerCommand_Impl for Command_Impl {
    fn GetTitle(&self, _items: Ref<'_, IShellItemArray>) -> Result<PWSTR> {
        wide(&self.node.title)
    }

    fn GetIcon(&self, _items: Ref<'_, IShellItemArray>) -> Result<PWSTR> {
        match halo_exe() {
            Some(exe) => wide(&format!("{},0", exe.display())),
            None => Err(E_NOTIMPL.into()),
        }
    }

    fn GetToolTip(&self, _items: Ref<'_, IShellItemArray>) -> Result<PWSTR> {
        Err(E_NOTIMPL.into())
    }

    fn GetCanonicalName(&self) -> Result<GUID> {
        Ok(GUID::zeroed())
    }

    fn GetState(&self, items: Ref<'_, IShellItemArray>, _slow: BOOL) -> Result<u32> {
        let class = match items.as_ref() {
            Some(arr) => {
                let paths = selected_paths(Some(arr));
                if paths.is_empty() {
                    log("GetState: selection has no file-system paths -> hidden");
                    return Ok(ECS_HIDDEN.0 as u32);
                }
                *LAST_SELECTION.lock().unwrap()=paths.clone();
                let k = classify(&paths);
                LAST_KIND.store(k, Ordering::Relaxed);
                k
            }
            // Explorer may query sub-commands without the selection: reuse the last one, else show.
            None => match LAST_KIND.load(Ordering::Relaxed) {
                0 => ALL,
                k => k,
            },
        };
        if let Some(action) = self.node.action.as_deref() {
            if action != "wheel" {
                let paths=LAST_SELECTION.lock().unwrap();
                let mut parts=action.split(':');
                let verb=parts.next().unwrap_or("");
                let option=parts.next().unwrap_or("");
                let option=if verb=="share" {"share_link"} else {option};
                let verb=if verb=="share" {"more"} else {verb};
                // Folder media needs a scan in Halo; keep the Explorer folder action to ZIP.
                if paths.iter().any(|p| {
                    let path=std::path::Path::new(p);
                    if let Some(target)=&self.node.preset_target {
                        let media=matches!(capabilities::kind_of(path),capabilities::Kind::Image|capabilities::Kind::Audio|capabilities::Kind::Video);
                        let ext=path.extension().and_then(|s|s.to_str()).unwrap_or("").to_ascii_lowercase();
                        !media || !(capabilities::format_supports(path,"convert",target) || (capabilities::normalized(&ext)==target && ext!="gif"))
                    } else {(path.is_dir() && action!="compress:zip") || !capabilities::format_supports(path,verb,option)}
                }) {return Ok(ECS_HIDDEN.0 as u32);}
            }
        }
        if self.node.kinds & class == 0 {
            log(&format!("GetState {}: hidden (class {class})", self.node.title));
            return Ok(ECS_HIDDEN.0 as u32);
        }
        Ok(ECS_ENABLED.0 as u32)
    }

    fn Invoke(&self, items: Ref<'_, IShellItemArray>, _bind: Ref<'_, IBindCtx>) -> Result<()> {
        let Some(action) = self.node.action.as_deref() else { return Err(E_NOTIMPL.into()) };
        let Some(exe) = halo_exe() else { return Err(E_NOTIMPL.into()) };
        // One launch per file: Halo's single-instance handler batches them.
        for path in selected_paths(items.as_ref()) {
            let _ = std::process::Command::new(&exe)
                .arg("--action")
                .arg(action)
                .arg(&path)
                .current_dir(exe.parent().unwrap_or(std::path::Path::new(".")))
                .creation_flags(0x0800_0000)
                .spawn();
        }
        Ok(())
    }

    fn GetFlags(&self) -> Result<u32> {
        Ok(if self.node.children.is_empty() { ECF_DEFAULT.0 as u32 } else { ECF_HASSUBCOMMANDS.0 as u32 })
    }

    fn EnumSubCommands(&self) -> Result<IEnumExplorerCommand> {
        log(&format!("EnumSubCommands {}", self.node.title));
        if self.node.children.is_empty() {
            return Err(E_NOTIMPL.into());
        }
        Ok(Enumerator { nodes: self.node.children.clone(), pos: std::cell::Cell::new(0) }.into())
    }
}

#[implement(IEnumExplorerCommand)]
struct Enumerator {
    nodes: Vec<Arc<Node>>,
    pos: std::cell::Cell<usize>,
}

impl IEnumExplorerCommand_Impl for Enumerator_Impl {
    fn Next(&self, celt: u32, out: *mut Option<IExplorerCommand>, fetched: *mut u32) -> HRESULT {
        // `out` is the first slot of a caller-allocated array of `celt` interface pointers.
        let slot = out;
        log(&format!("Next celt={celt} pos={}", self.pos.get()));
        let mut got = 0u32;
        while got < celt && self.pos.get() < self.nodes.len() {
            let cmd: IExplorerCommand = Command { node: self.nodes[self.pos.get()].clone() }.into();
            unsafe { slot.add(got as usize).write(Some(cmd)) };
            self.pos.set(self.pos.get() + 1);
            got += 1;
        }
        if !fetched.is_null() {
            unsafe { *fetched = got };
        }
        if got == celt {
            S_OK
        } else {
            S_FALSE
        }
    }

    fn Skip(&self, celt: u32) -> Result<()> {
        self.pos.set((self.pos.get() + celt as usize).min(self.nodes.len()));
        Ok(())
    }

    fn Reset(&self) -> Result<()> {
        self.pos.set(0);
        Ok(())
    }

    fn Clone(&self) -> Result<IEnumExplorerCommand> {
        Ok(Enumerator { nodes: self.nodes.clone(), pos: std::cell::Cell::new(self.pos.get()) }.into())
    }
}

#[implement(IClassFactory)]
struct Factory;

impl IClassFactory_Impl for Factory_Impl {
    fn CreateInstance(&self, outer: Ref<'_, IUnknown>, riid: *const GUID, ppv: *mut *mut c_void) -> Result<()> {
        if ppv.is_null() || riid.is_null() {
            return Err(E_POINTER.into());
        }
        unsafe { *ppv = std::ptr::null_mut() };
        if outer.is_some() {
            return Err(CLASS_E_NOAGGREGATION.into());
        }
        let cmd: IExplorerCommand = Command { node: build_tree() }.into();
        let unk: IUnknown = cmd.cast()?;
        let hr = unsafe { unk.query(riid, ppv) };
        if hr.is_ok() {
            Ok(())
        } else {
            Err(E_NOINTERFACE.into())
        }
    }

    fn LockServer(&self, lock: BOOL) -> Result<()> {
        if lock.as_bool() {
            LOCKS.fetch_add(1, Ordering::SeqCst);
        } else {
            LOCKS.fetch_sub(1, Ordering::SeqCst);
        }
        Ok(())
    }
}

#[no_mangle]
pub extern "system" fn DllGetClassObject(rclsid: *const GUID, riid: *const GUID, ppv: *mut *mut c_void) -> HRESULT {
    if rclsid.is_null() || riid.is_null() || ppv.is_null() {
        return E_POINTER;
    }
    unsafe {
        *ppv = std::ptr::null_mut();
        if *rclsid != CLSID_HALO_MENU {
            return CLASS_E_CLASSNOTAVAILABLE;
        }
        let factory: IClassFactory = Factory.into();
        factory.query(riid, ppv)
    }
}

#[no_mangle]
pub extern "system" fn DllCanUnloadNow() -> HRESULT {
    // Live COM objects are not counted; keep the DLL loaded for the Explorer session.
    S_FALSE
}
