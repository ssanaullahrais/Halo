//! Checks that the programs Halo relies on are present, and installs the missing ones with one click.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;

use serde::Serialize;

const NO_WINDOW: u32 = 0x0800_0000;

#[derive(Serialize, Clone)]
pub struct Check {
    pub id: &'static str,
    pub name: &'static str,
    pub purpose: &'static str,
    pub ok: bool,
    pub detail: String,
    /// Label of the one-click fix, when there is one.
    pub fix: Option<&'static str>,
}

fn run(program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program).args(args).stdin(Stdio::null()).stderr(Stdio::null()).creation_flags(NO_WINDOW).output().ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        None
    }
}

fn expand_vars(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(start) = rest.find('%') {
        out += &rest[..start];
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) => {
                out += &std::env::var(&after[..end]).unwrap_or_default();
                rest = &after[end + 1..];
            }
            None => {
                out += &rest[start..];
                rest = "";
            }
        }
    }
    out + rest
}

/// The PATH as Windows would give it to a new program (a running app keeps the PATH it started with).
fn fresh_path_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    for key in [r"HKCU\Environment", r"HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Environment"] {
        if let Some(text) = run("reg", &["query", key, "/v", "Path"]) {
            if let Some(line) = text.lines().find(|l| l.trim_start().to_lowercase().starts_with("path")) {
                if let Some(value) = line.splitn(3, "    ").nth(2).or_else(|| line.split("REG_EXPAND_SZ").nth(1)).or_else(|| line.split("REG_SZ").nth(1)) {
                    dirs.extend(expand_vars(value.trim()).split(';').filter(|d| !d.is_empty()).map(PathBuf::from));
                }
            }
        }
    }
    dirs
}

pub fn tools_dir() -> PathBuf {
    let base = std::env::var("APPDATA").map(PathBuf::from).unwrap_or_else(|_| std::env::temp_dir());
    base.join("com.halo.wheel").join("tools")
}

fn winget_packages(prefix: &str, exe: &str) -> Option<PathBuf> {
    let root = PathBuf::from(std::env::var("LOCALAPPDATA").ok()?).join(r"Microsoft\WinGet\Packages");
    for entry in std::fs::read_dir(root).ok()?.flatten() {
        if entry.file_name().to_string_lossy().starts_with(prefix) {
            for sub in walk(&entry.path(), 3) {
                if sub.file_name().map(|n| n.to_string_lossy().eq_ignore_ascii_case(exe)).unwrap_or(false) {
                    return Some(sub);
                }
            }
        }
    }
    None
}

fn walk(dir: &Path, depth: u32) -> Vec<PathBuf> {
    let mut found = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() && depth > 0 {
                found.extend(walk(&p, depth - 1));
            } else {
                found.push(p);
            }
        }
    }
    found
}

static FFMPEG: Mutex<Option<PathBuf>> = Mutex::new(None);

pub fn find_ffmpeg() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = vec![tools_dir().join("ffmpeg.exe")];
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        candidates.push(PathBuf::from(local).join(r"Microsoft\WinGet\Links\ffmpeg.exe"));
    }
    candidates.extend(fresh_path_dirs().into_iter().map(|d| d.join("ffmpeg.exe")));
    candidates.extend(winget_packages("Gyan.FFmpeg", "ffmpeg.exe"));
    candidates.push(PathBuf::from(r"C:\ProgramData\chocolatey\bin\ffmpeg.exe"));
    candidates.into_iter().find(|p| p.exists())
}

/// The ffmpeg to run: a found copy, or plain `ffmpeg` so the error message stays meaningful.
pub fn ffmpeg_path() -> PathBuf {
    let mut cached = FFMPEG.lock().unwrap();
    if cached.as_ref().map(|p| !p.exists()).unwrap_or(true) {
        *cached = find_ffmpeg();
    }
    cached.clone().unwrap_or_else(|| PathBuf::from("ffmpeg"))
}

fn forget_ffmpeg() {
    *FFMPEG.lock().unwrap() = None;
}

pub fn edge_path() -> Option<PathBuf> {
    [
        std::env::var("ProgramFiles(x86)").ok().map(|p| PathBuf::from(p).join(r"Microsoft\Edge\Application\msedge.exe")),
        std::env::var("ProgramFiles").ok().map(|p| PathBuf::from(p).join(r"Microsoft\Edge\Application\msedge.exe")),
    ]
    .into_iter()
    .flatten()
    .find(|p| p.exists())
}

/// Directories that may hold pdfium.dll, most specific first.
pub fn pdfium_dirs() -> Vec<PathBuf> {
    let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())).unwrap_or_default();
    vec![exe_dir.clone(), exe_dir.join("resources"), exe_dir.join(r"..\..\resources"), exe_dir.join(r"..\..\..\resources"), tools_dir()]
}

fn pdfium_found() -> Option<PathBuf> {
    pdfium_dirs().into_iter().map(|d| d.join("pdfium.dll")).find(|p| p.exists())
}

fn winget_ok() -> bool {
    run("winget", &["--version"]).is_some()
}

pub fn checks() -> Vec<Check> {
    let ffmpeg = find_ffmpeg();
    let ffmpeg_ver = ffmpeg
        .as_ref()
        .and_then(|p| run(&p.to_string_lossy(), &["-version"]))
        .and_then(|t| t.lines().next().map(|l| l.to_string()))
        .map(|l| l.split_whitespace().take(3).collect::<Vec<_>>().join(" "));
    let internet = std::net::TcpStream::connect_timeout(
        &std::net::ToSocketAddrs::to_socket_addrs(&("catbox.moe", 443)).ok().and_then(|mut a| a.next()).unwrap_or_else(|| "0.0.0.0:443".parse().unwrap()),
        std::time::Duration::from_secs(4),
    )
    .is_ok();
    let have_winget = winget_ok();
    let via_winget = if have_winget { Some("Install") } else { None };

    vec![
        Check {
            id: "ffmpeg",
            name: "FFmpeg",
            purpose: "Compress, convert and resize videos and audio.",
            ok: ffmpeg.is_some(),
            detail: match (&ffmpeg, &ffmpeg_ver) {
                (Some(_), Some(v)) => v.clone(),
                (Some(p), None) => p.display().to_string(),
                _ => if have_winget { "Not installed".into() } else { "Not installed, and Windows Package Manager is missing".into() },
            },
            fix: if ffmpeg.is_some() { None } else { via_winget },
        },
        Check {
            id: "edge",
            name: "Microsoft Edge",
            purpose: "Turn Word, text and web documents into PDFs.",
            ok: edge_path().is_some(),
            detail: if edge_path().is_some() { "Installed".into() } else { "Not installed".into() },
            fix: if edge_path().is_some() { None } else { via_winget },
        },
        Check {
            id: "pdfium",
            name: "PDF engine",
            purpose: "Turn PDF pages into pictures and read their text.",
            ok: pdfium_found().is_some(),
            detail: if pdfium_found().is_some() { "Included with Halo".into() } else { "Missing".into() },
            fix: if pdfium_found().is_some() { None } else { Some("Download") },
        },
        Check {
            id: "internet",
            name: "Internet connection",
            purpose: "Upload files and get links.",
            ok: internet,
            detail: if internet { "Connected".into() } else { "Can't reach the upload services".into() },
            fix: None,
        },
        Check {
            id: "imagemagick",
            name: "Extended image engine (optional)",
            purpose: "Convert PSD, PSB, TGA, DDS and EXR images. Source files stay unchanged.",
            ok: crate::capabilities::image_engine_path().is_some(),
            detail: if crate::capabilities::image_engine_path().is_some() { "Installed".into() } else { "Optional engine not installed".into() },
            fix: if crate::capabilities::image_engine_path().is_some() {None} else {Some("Install")},
        },
    ]
}

/// Runs the one-click fix for a check. Blocking: call it from a worker thread.
pub fn install(id: &str) -> Result<String, String> {
    match id {
        "imagemagick" => {
            use sha2::{Digest, Sha256};
            use std::io::Read;
            if crate::capabilities::image_engine_path().is_some() { return Ok("Already installed.".into()); }
            let dir=tools_dir();
            std::fs::create_dir_all(&dir).map_err(|e|e.to_string())?;
            let response=ureq::get("https://github.com/ImageMagick/ImageMagick/releases/download/7.1.2-34/ImageMagick-7.1.2-34-portable-Q16-x64.7z")
                .timeout(std::time::Duration::from_secs(180)).call().map_err(|e|format!("Couldn't download the PSD engine: {e}"))?;
            let mut data=Vec::new();
            response.into_reader().take(12_022_004).read_to_end(&mut data).map_err(|e|e.to_string())?;
            if data.len()!=12_022_003 || format!("{:x}",Sha256::digest(&data))!="52cc6a441c7884f5ff4274b410c9b90c4b7d54245dcdf4d32988bb4e4df4e5d1" {
                return Err("The PSD engine download failed its integrity check.".into());
            }
            let archive=dir.join("ImageMagick-7.1.2-34-verified.7z");
            std::fs::write(&archive,&data).map_err(|e|e.to_string())?;
            let target=dir.join("magick");
            std::fs::create_dir_all(&target).map_err(|e|e.to_string())?;
            let status=Command::new("tar").arg("-xf").arg(&archive).arg("-C").arg(&target)
                .stdin(Stdio::null()).creation_flags(NO_WINDOW).status().map_err(|e|e.to_string())?;
            if !status.success() || crate::capabilities::image_engine_path().is_none() { return Err("The PSD engine couldn't be unpacked.".into()); }
            Ok("PSD / PSB support installed. Reopen the file menu to use it.".into())
        },
        "ffmpeg" | "edge" => {
            let package = if id == "ffmpeg" { "Gyan.FFmpeg" } else { "Microsoft.Edge" };
            let out = Command::new("winget")
                .args(["install", "--id", package, "-e", "--silent", "--accept-source-agreements", "--accept-package-agreements"])
                .stdin(Stdio::null())
                .creation_flags(NO_WINDOW)
                .output()
                .map_err(|_| "Windows Package Manager (winget) isn't available on this PC.".to_string())?;
            forget_ffmpeg();
            let text = String::from_utf8_lossy(&out.stdout).to_string();
            // winget reports "already installed" with a non-zero code; that still means it is there
            if out.status.success() || text.to_lowercase().contains("already installed") || text.to_lowercase().contains("no available upgrade") {
                Ok("Installed.".into())
            } else {
                let tail: String = text.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("The installer didn't finish.").chars().take(140).collect();
                Err(format!("Installing didn't finish: {tail}"))
            }
        }
        "pdfium" => {
            let dir = tools_dir();
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let archive = dir.join("pdfium.tgz");
            let resp = ureq::get("https://github.com/bblanchon/pdfium-binaries/releases/latest/download/pdfium-win-x64.tgz")
                .timeout(std::time::Duration::from_secs(180))
                .call()
                .map_err(|e| format!("Couldn't download the PDF engine ({e})"))?;
            let mut file = std::fs::File::create(&archive).map_err(|e| e.to_string())?;
            std::io::copy(&mut resp.into_reader(), &mut file).map_err(|e| e.to_string())?;
            drop(file);
            let status = Command::new("tar").arg("-xzf").arg(&archive).arg("-C").arg(&dir).creation_flags(NO_WINDOW).status().map_err(|e| e.to_string())?;
            let _ = std::fs::remove_file(&archive);
            let dll = dir.join("bin").join("pdfium.dll");
            if status.success() && dll.exists() {
                std::fs::copy(&dll, dir.join("pdfium.dll")).map_err(|e| e.to_string())?;
                Ok("Downloaded. Restart Halo to start using it.".into())
            } else {
                Err("The download didn't unpack.".into())
            }
        }
        _ => Err("Nothing to install for this item.".into()),
    }
}
