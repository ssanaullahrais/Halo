//! Loads the built DLL like Explorer would and drives the IExplorerCommand list.
//! Run: cargo build --release && cargo test --release -- --nocapture --test-threads=1
//! HALO_DLL overrides the DLL (invoke needs halo.exe beside it, e.g. src-tauri\target\debug\halo_menu.dll).
//! HALO_TEST_INVOKE=<title> also invokes that item on test-files\sample.jpg (e.g. "Compress").

use std::ffi::c_void;
use windows::core::{s, Interface, GUID, HRESULT, HSTRING};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, IClassFactory, CLSCTX_LOCAL_SERVER, COINIT_APARTMENTTHREADED};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    IExplorerCommand, IShellItemArray, SHCreateShellItemArrayFromIDLists, SHParseDisplayName,
};

const CLSID: GUID = GUID::from_u128(0x7c3a1d52_9b64_4e0a_8f2d_5a1b6c9e4d31);

fn text(p: windows::core::PWSTR) -> String {
    let s = unsafe { p.to_string().unwrap() };
    unsafe { CoTaskMemFree(Some(p.0 as *const c_void)) };
    s
}

fn full(path: &str) -> String {
    let p = std::fs::canonicalize(path).unwrap();
    p.to_string_lossy().trim_start_matches(r"\\?\").to_string()
}

/// Selection built from a PIDL, the way Explorer builds it.
fn items(path: &str) -> IShellItemArray {
    unsafe {
        let mut pidl: *mut ITEMIDLIST = std::ptr::null_mut();
        SHParseDisplayName(&HSTRING::from(full(path)), None, &mut pidl, 0, None).unwrap();
        SHCreateShellItemArrayFromIDLists(&[pidl as *const ITEMIDLIST]).unwrap()
    }
}

fn root() -> IExplorerCommand {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let dll = std::env::var("HALO_DLL")
            .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), r"\target\release\halo_menu.dll").to_string());
        let module = LoadLibraryW(&HSTRING::from(dll.as_str())).expect("load dll");
        let get: unsafe extern "system" fn(*const GUID, *const GUID, *mut *mut c_void) -> HRESULT =
            std::mem::transmute(GetProcAddress(module, s!("DllGetClassObject")).expect("export"));
        let mut raw = std::ptr::null_mut();
        get(&CLSID, &IClassFactory::IID, &mut raw).ok().expect("class object");
        IClassFactory::from_raw(raw).CreateInstance(None).expect("create command")
    }
}

/// Visible sub-commands for a selection: (title, command).
fn visible(root: &IExplorerCommand, arr: &IShellItemArray) -> Vec<(String, IExplorerCommand)> {
    unsafe {
        assert_eq!(root.GetState(arr, false).unwrap(), 0, "parent must be enabled");
        assert_eq!(root.GetFlags().unwrap() & 1, 1, "parent must have sub-commands");
        let en = root.EnumSubCommands().unwrap();
        let mut out = vec![];
        loop {
            let mut one: [Option<IExplorerCommand>; 1] = [None];
            let mut got = 0u32;
            let _ = en.Next(&mut one, Some(&mut got));
            if got == 0 {
                break;
            }
            let c = one[0].take().unwrap();
            if c.GetState(arr, false).unwrap() != 2 {
                out.push((text(c.GetTitle(arr).unwrap()), c));
            }
        }
        out
    }
}

fn titles(root: &IExplorerCommand, path: &str) -> Vec<String> {
    visible(root, &items(path)).into_iter().map(|(t, _)| t).collect()
}

#[test]
fn menu_per_file_type() {
    let tf = format!(r"{}\..\output\capability-audit\shell-fixtures-{}",env!("CARGO_MANIFEST_DIR"),std::process::id());
    std::fs::create_dir_all(&tf).unwrap();
    for name in ["sample.jpg", "t.mp4", "t.txt", "t.ico", "t.gif", "t.wav", "t.webp", "t.avif"] {
        std::fs::write(format!(r"{tf}\{name}"), "x").unwrap();
    }
    let r = root();
    let img = ["Open in Halo", "Compress", "Convert to JPG", "Convert to WebP", "Resize to half size", "Zip it", "Upload & get link"];
    let jpg = ["Open in Halo", "Compress", "Convert to WebP", "Resize to half size", "Zip it", "Upload & get link"];
    let vid = ["Open in Halo", "Compress", "Resize to half size", "Convert to MP3", "Zip it", "Upload & get link"];
    let doc = ["Open in Halo", "Convert to JPG", "Convert to WebP", "Zip it", "Upload & get link"];
    let gif = ["Open in Halo", "Convert to JPG", "Convert to WebP", "Convert to MP4", "Zip it", "Upload & get link"];
    let audio = ["Open in Halo", "Compress", "Convert to MP3", "Zip it", "Upload & get link"];
    let webp = ["Open in Halo", "Compress", "Convert to JPG", "Resize to half size", "Zip it", "Upload & get link"];
    let dir = ["Open in Halo", "Zip it"];
    let cases: [(&str, &[&str]); 9] = [
        (r"\sample.jpg", &jpg),
        (r"\t.ico", &img),
        (r"\t.mp4", &vid),
        (r"\t.txt", &doc),
        (r"\t.gif", &gif),
        (r"\t.wav", &audio),
        (r"\t.webp", &webp),
        (r"\t.avif", &img),
        ("", &dir), // the test-files folder itself
    ];
    for (rel, want) in cases {
        let got = titles(&r, &format!("{tf}{rel}"));
        println!("{:<14} -> {:?}", if rel.is_empty() { "<folder>" } else { rel }, got);
        assert_eq!(got, want, "{rel}");
    }
    // A sub-command asked without any selection is not hidden.
    unsafe {
        let en = r.EnumSubCommands().unwrap();
        let mut one: [Option<IExplorerCommand>; 1] = [None];
        let mut got = 0u32;
        let _ = en.Next(&mut one, Some(&mut got));
        assert_ne!(one[0].as_ref().unwrap().GetState(None::<&IShellItemArray>, false).unwrap(), 2);
    }
}

#[test]
fn invoke_item() {
    let Ok(title) = std::env::var("HALO_TEST_INVOKE") else { return };
    let r = root();
    let path=std::env::var("HALO_TEST_IMAGE").unwrap_or_else(|_|concat!(env!("CARGO_MANIFEST_DIR"), r"\..\test-files\sample.jpg").to_string());
    let arr = items(&path);
    let (t, cmd) = visible(&r, &arr).into_iter().find(|(t, _)| *t == title).expect("item not found");
    println!("invoking: {t}");
    unsafe { cmd.Invoke(&arr, None).expect("invoke") };
}

#[test]
fn named_preset_visibility() {
    let Ok(title)=std::env::var("HALO_TEST_PRESET_TITLE") else {return};
    let image=std::env::var("HALO_TEST_IMAGE").unwrap();
    let other=std::env::var("HALO_TEST_OTHER").unwrap();
    let r=root();
    assert!(titles(&r,&image).contains(&title));
    assert!(!titles(&r,&other).contains(&title));
}

#[test]
fn registered_extension_activates() {
    if std::env::var("HALO_TEST_REGISTERED").as_deref()!=Ok("1") {return;}
    unsafe {
        let _=CoInitializeEx(None,COINIT_APARTMENTTHREADED);
        let command:IExplorerCommand=CoCreateInstance(&CLSID,None,CLSCTX_LOCAL_SERVER).expect("activate registered Windows 11 extension");
        assert_eq!(text(command.GetTitle(None::<&IShellItemArray>).unwrap()),"Halo");
        let path=std::env::var("HALO_TEST_IMAGE").unwrap();
        let entries=visible(&command,&items(&path));
        if std::env::var("HALO_TEST_HOLD_COM").as_deref()==Ok("1") {std::thread::sleep(std::time::Duration::from_secs(20));}
        assert!(entries.iter().any(|(title,_)|title=="Open in Halo"));
        if let Ok(expected)=std::env::var("HALO_TEST_REGISTERED_EXPECT") {
            assert!(entries.iter().any(|(title,_)|title==&expected),"Expected {expected}, got {:?}",entries.iter().map(|(t,_)|t).collect::<Vec<_>>());
            if std::env::var("HALO_TEST_REGISTERED_INVOKE").as_deref()==Ok("1") {
                entries.iter().find(|(title,_)|title==&expected).unwrap().1.Invoke(&items(&path),None).expect("invoke registered action");
            }
        }
        println!("Registered extension activated with {} eligible actions",entries.len());
    }
}
