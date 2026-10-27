//! File capabilities shared by the app and Explorer extension. No UI or external processes.
use std::path::Path;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Image,
    Video,
    Audio,
    Archive,
    Pdf,
    Doc,
    Svg,
    Folder,
    Other,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Video => "video",
            Self::Audio => "audio",
            Self::Archive => "archive",
            Self::Pdf => "pdf",
            Self::Doc => "doc",
            Self::Svg => "svg",
            Self::Folder => "folder",
            Self::Other => "other",
        }
    }
}

pub const IMAGE_EXT: &[&str] = &[
    "jpg", "jpeg", "jfif", "png", "webp", "bmp", "gif", "tif", "tiff", "ico", "avif", "psd", "psb", "tga", "dds", "exr",
];

pub fn external_image(path: &Path) -> bool {
    path.extension().and_then(|e|e.to_str()).map(|e|["psd","psb","tga","dds","exr"].contains(&e.to_ascii_lowercase().as_str())).unwrap_or(false)
}

pub fn image_engine_path() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let mut candidates=vec![exe.join("resources/magick/magick.exe"),exe.join("magick/magick.exe")];
    if let Some(base)=std::env::var_os("APPDATA") { candidates.push(std::path::PathBuf::from(base).join("com.halo.wheel/tools/magick/magick.exe")); }
    candidates.into_iter().find(|p|p.is_file())
}
pub const VIDEO_EXT: &[&str] = &["mp4", "mov", "mkv", "avi", "webm", "m4v", "wmv", "flv", "3gp", "3gpp", "mpeg", "mpg", "ogv", "ts", "vob", "rm"];
pub const AUDIO_EXT: &[&str] = &["mp3", "wav", "flac", "ogg", "m4a", "aac", "wma", "opus", "aiff", "m4b", "oga"];
pub const DOC_EXT: &[&str] = &[
    "txt", "md", "markdown", "html", "htm", "docx", "csv", "log", "json", "xml",
];
pub const VERBS: &[&str] = &["compress", "convert", "resize", "edit", "files", "more"];
pub const IMAGE_TARGETS: &[&str] = &["jpg", "png", "webp", "avif", "bmp", "tiff", "gif", "ico"];
pub const BLOCKED: &[&str] = &[
    "exe", "bat", "cmd", "com", "msi", "ps1", "vbs", "vbe", "js", "jse", "jar", "scr", "wsf",
    "reg", "lnk", "hta",
];

pub fn normalized(ext: &str) -> &str {
    match ext {
        "jpeg" | "jfif" => "jpg",
        "tif" => "tiff",
        "htm" => "html",
        "markdown" => "md",
        other => other,
    }
}
pub fn kind_of(path: &Path) -> Kind {
    if path.is_dir() {
        return Kind::Folder;
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if IMAGE_EXT.contains(&ext.as_str()) {
        Kind::Image
    } else if VIDEO_EXT.contains(&ext.as_str()) {
        Kind::Video
    } else if AUDIO_EXT.contains(&ext.as_str()) {
        Kind::Audio
    } else if DOC_EXT.contains(&ext.as_str()) {
        Kind::Doc
    } else {
        match ext.as_str() {
            "pdf" => Kind::Pdf,
            "svg" => Kind::Svg,
            "zip" => Kind::Archive,
            _ => Kind::Other,
        }
    }
}

pub fn format_options(path: &Path, verb: &str) -> Vec<&'static str> {
    let kind = kind_of(path);
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let gif = ext == "gif";
    let raster = kind == Kind::Image && !gif;
    let mut out = Vec::new();
    match verb {
        "compress" => {
            if raster || matches!(kind, Kind::Video | Kind::Audio | Kind::Folder) {
                out.extend(["auto", "small", "medium", "large", "custom"]);
            }
            if raster && ext != "png" && !external_image(path) {
                out.push("target");
            }
            out.extend(["zip", "zip_all"]);
        }
        "convert" => {
            match kind {
                Kind::Image | Kind::Svg => {
                    out.extend_from_slice(IMAGE_TARGETS);
                    if gif { out.extend(["mp4","webm","mkv","mov","avi","ogv"]); }
                    out.push("pdf");
                    if kind == Kind::Image {
                        out.push("svg");
                    }
                }
                Kind::Video => out.extend([
                    "mp4", "webm", "mkv", "mov", "avi", "ogv", "gif", "mp3", "wav", "m4a", "aac", "ogg", "flac", "opus",
                ]),
                Kind::Audio => out.extend(["mp3", "wav", "ogg", "flac", "m4a", "aac", "opus"]),
                Kind::Pdf | Kind::Doc => {
                    out.extend_from_slice(IMAGE_TARGETS);
                    out.extend(["txt", "docx", "html", "md"]);
                    if kind == Kind::Doc {
                        out.push("pdf");
                    }
                }
                Kind::Folder => {
                    out.extend_from_slice(IMAGE_TARGETS);
                    out.extend([
                        "pdf", "txt", "docx", "html", "md", "svg", "mp4", "webm", "mkv", "mov",
                        "avi", "ogv", "mp3", "wav", "ogg", "flac", "m4a", "aac", "opus",
                    ]);
                }
                _ => {}
            }
            out.retain(|target| *target != normalized(&ext));
        }
        "resize" => {
            if raster || matches!(kind, Kind::Video | Kind::Folder) {
                out.extend(["custom", "percent"]);
                if ext != "ico" { out.extend(["social", "wallpaper", "email"]); }
            }
        }
        "edit" => {
            if kind == Kind::Pdf { out.push("extract_images"); }
            if !BLOCKED.contains(&ext.as_str()) {
                out.push("preview");
            }
            if raster {
                out.extend([
                    "rotate_left",
                    "rotate_right",
                    "flip_h",
                    "flip_v",
                    "set_wallpaper",
                ]);
            }
            if kind == Kind::Video {
                out.push("extract_audio");
            }
            if raster || (matches!(kind, Kind::Video | Kind::Audio) && ext != "rm") {
                out.push("strip_meta");
            }
            if !matches!(kind, Kind::Image | Kind::Video | Kind::Audio | Kind::Folder) {
                out.push("open_with");
            }
        }
        "files" => {
            out.extend([
                "copy",
                "duplicate",
                "copy_path",
                "move_to",
                "rename",
                "open_location",
                "show_new_window",
                "check_type",
            ]);
            if kind == Kind::Folder {
                out.push("open");
            }
            if kind != Kind::Folder
                && path
                    .metadata()
                    .map(|m| m.len() <= 8 * 1024 * 1024)
                    .unwrap_or(false)
            {
                out.push("copy_base64");
            }
            if kind == Kind::Image {
                out.push("copy_ocr");
            }
        }
        "more" => {
            out.extend(["properties", "delete", "open_with"]);
            if kind != Kind::Folder {
                out.extend(["share_link", "qr"]);
            }
            if kind != Kind::Archive {
                out.push("zip_it");
            }
            if kind == Kind::Archive {
                out.push("extract");
            }
        }
        _ => {}
    }
    out
}

pub fn options(path: &Path, verb: &str) -> Vec<&'static str> {
    let mut out=format_options(path,verb);
    if external_image(path) && image_engine_path().is_none() {
        match verb {
            "compress" => out.retain(|o| matches!(*o,"zip"|"zip_all")),
            "convert"|"resize" => out.clear(),
            "edit" => out.retain(|o|*o=="preview"),
            "files" => out.retain(|o|*o!="copy_ocr"),
            _=>{}
        }
    }
    out
}
pub fn supports(path: &Path, verb: &str, option: &str) -> bool {
    options(path, verb).contains(&option)
}
pub fn format_supports(path: &Path, verb: &str, option: &str) -> bool {
    format_options(path,verb).contains(&option)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_declared_extensions_have_the_expected_kind() {
        for (extensions,kind) in [(IMAGE_EXT,Kind::Image),(VIDEO_EXT,Kind::Video),(AUDIO_EXT,Kind::Audio),(DOC_EXT,Kind::Doc)] {
            for ext in extensions {assert_eq!(kind_of(Path::new(&format!("sample.{}",ext.to_ascii_uppercase()))),kind);}
        }
    }
    #[test]
    fn animated_gifs_only_offer_safe_actions() {
        let file=Path::new("sample.gif");
        assert!(!supports(file,"compress","small"));
        assert!(supports(file,"compress","zip"));
        assert!(options(file,"resize").is_empty());
        for action in ["rotate_left","rotate_right","flip_h","flip_v","strip_meta"] {assert!(!supports(file,"edit",action));}
    }
    #[test]
    fn current_formats_and_png_targets_are_excluded() {
        for ext in ["jpg","jpeg","png","tif","tiff","markdown","htm","avif"] {
            assert!(!supports(Path::new(&format!("sample.{ext}")),"convert",normalized(ext)));
        }
        assert!(!supports(Path::new("sample.png"),"compress","target"));
        assert!(supports(Path::new("sample.webp"),"compress","target"));
    }
    #[test]
    fn every_kind_can_be_archived_and_program_preview_stays_blocked() {
        for ext in ["jpg","png","gif","avif","pdf","docx","mp4","mp3","zip","bin"] {assert!(supports(Path::new(&format!("sample.{ext}")),"compress","zip"));}
        assert!(!supports(Path::new("program.exe"),"edit","preview"));
    }
}
