//! Folders: apply an action to the media inside a folder.
//!
//! Every matching file inside the folder (and its sub-folders) is processed, and the results are gathered in
//! a new folder beside it ("Holiday - small"), keeping the same sub-folders. The originals are never touched.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use serde::Serialize;

use crate::process::{self, Ctx, Kind, Outcome, Req};

#[derive(Serialize, Clone)]
pub struct FolderScan {
    pub images: u32,
    pub videos: u32,
    pub audio: u32,
    pub docs: u32,
    pub other: u32,
    pub capabilities: std::collections::BTreeMap<&'static str, Vec<&'static str>>,
}

fn walk_files(dir: &Path, depth: u32, out: &mut Vec<PathBuf>, cap: usize) {
    if depth == 0 || out.len() >= cap {
        return;
    }
    if let Ok(rd) = fs::read_dir(dir) {
        let mut entries: Vec<_> = rd.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let p = e.path();
            if p.is_dir() {
                walk_files(&p, depth - 1, out, cap);
            } else {
                out.push(p);
            }
            if out.len() >= cap {
                return;
            }
        }
    }
}

/// Counts what is inside a folder, so the wheel can offer the right actions.
pub fn scan(path: &Path) -> FolderScan {
    let mut files = Vec::new();
    walk_files(path, 6, &mut files, 5000);
    let mut scan = FolderScan { images: 0, videos: 0, audio: 0, docs: 0, other: 0, capabilities: ["compress", "convert", "resize"].into_iter().map(|v|(v,Vec::new())).collect() };
    for f in files {
        for verb in ["compress", "convert", "resize"] {
            for option in crate::capabilities::options(&f,verb) {
                if applies(&f,verb,option) && !scan.capabilities[verb].contains(&option) {scan.capabilities.get_mut(verb).unwrap().push(option);}
            }
        }
        match process::kind_of(&f) {
            Kind::Image | Kind::Svg => scan.images += 1,
            Kind::Video => scan.videos += 1,
            Kind::Audio => scan.audio += 1,
            Kind::Doc | Kind::Pdf => scan.docs += 1,
            _ => scan.other += 1,
        }
    }
    scan.capabilities.get_mut("compress").unwrap().extend(["zip", "zip_all"]);
    scan
}

/// Which files inside a folder an action applies to.
fn applies(path: &Path, verb: &str, option: &str) -> bool {
    matches!(verb,"compress"|"convert"|"resize") && !matches!(option,"zip"|"zip_all") && crate::capabilities::supports(path,verb,option)
}

fn move_into(produced: &Path, dest_dir: &Path) -> Option<PathBuf> {
    fs::create_dir_all(dest_dir).ok()?;
    let name = produced.file_name()?.to_string_lossy().to_string();
    let target = if produced.is_dir() {
        let mut t = dest_dir.join(&name);
        let mut n = 1;
        while t.exists() {
            t = dest_dir.join(format!("{name} ({n})"));
            n += 1;
        }
        t
    } else {
        match name.rsplit_once('.') {
            Some((stem, ext)) => process::unique(dest_dir, stem, ext),
            None => dest_dir.join(&name),
        }
    };
    if fs::rename(produced, &target).is_err() {
        if produced.is_file() && fs::copy(produced, &target).is_ok() {
            let _ = fs::remove_file(produced);
        } else {
            return None;
        }
    }
    Some(target)
}

fn run_folder(root: &Path, req: &Req, ctx: &Ctx) -> Outcome {
    let name = root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "folder".into());
    let mut all = Vec::new();
    walk_files(root, 6, &mut all, 5000);
    let files: Vec<PathBuf> = all.into_iter().filter(|f| applies(f, &req.verb, &req.option)).collect();
    if files.is_empty() {
        return Outcome { name, ok: false, message: "There is nothing inside this folder that this action can change.".into(), out_path: None, before: 0, after: 0 };
    }
    let label = match (req.verb.as_str(), req.option.as_str()) {
        ("resize", "percent") => format!("{}pct", req.value.unwrap_or(50.0) as u32),
        (_, o) => o.to_string(),
    };
    let parent = root.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let mut out_root = parent.join(format!("{name} - {label}"));
    let mut n = 1;
    while out_root.exists() {
        out_root = parent.join(format!("{name} - {label} ({n})"));
        n += 1;
    }
    let total = files.len();
    let (mut done, mut skipped, mut before, mut after) = (0usize, 0usize, 0u64, 0u64);
    for (i, f) in files.iter().enumerate() {
        if ctx.cancel.load(Ordering::SeqCst) {
            break;
        }
        let rep = |p: f64, _b: u64| (ctx.report)((i as f64 + p) / total as f64, 0);
        match process::run_one(f, req, ctx, &rep, None) {
            Ok(d) => match d.out {
                Some(produced) => {
                    let rel_dir = f.parent().and_then(|p| p.strip_prefix(root).ok()).map(|p| p.to_path_buf()).unwrap_or_default();
                    match move_into(&produced, &out_root.join(rel_dir)) {
                        Some(moved) => {
                            done += 1;
                            before += process::size_of(f);
                            after += process::size_of(&moved);
                        }
                        None => skipped += 1,
                    }
                }
                None => skipped += 1,
            },
            Err(_) => skipped += 1,
        }
    }
    if ctx.cancel.load(Ordering::SeqCst) {
        return Outcome { name, ok: false, message: "Cancelled".into(), out_path: None, before, after };
    }
    if done == 0 {
        let _ = fs::remove_dir_all(&out_root);
        return Outcome { name, ok: false, message: "None of the files in this folder could be processed.".into(), out_path: None, before: 0, after: 0 };
    }
    let summary = if skipped > 0 { format!("{name} ({done} files, {skipped} skipped)") } else { format!("{name} ({done} files)") };
    Outcome { name: summary, ok: true, message: String::new(), out_path: Some(out_root.to_string_lossy().to_string()), before, after }
}

/// Handles the folders in a request. Returns None when no folder is involved.
pub fn run(req: &Req, ctx: &Ctx) -> Option<Vec<Outcome>> {
    let wants = matches!(req.verb.as_str(), "compress" | "convert" | "resize") && !matches!(req.option.as_str(), "zip" | "zip_all");
    if !wants || !req.files.iter().any(|f| Path::new(f).is_dir()) {
        return None;
    }
    let mut outcomes: Vec<Outcome> = req.files.iter().filter(|f| Path::new(f).is_dir()).map(|d| run_folder(Path::new(d), req, ctx)).collect();
    let rest: Vec<String> = req.files.iter().filter(|f| !Path::new(f).is_dir()).cloned().collect();
    if !rest.is_empty() {
        let mut other = req.clone();
        other.files = rest;
        outcomes.extend(process::run(&other, ctx));
    }
    Some(outcomes)
}
