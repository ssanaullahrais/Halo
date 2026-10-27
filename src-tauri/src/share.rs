//! Upload a copy of a file to free services (no account, no API key) and get a link back, that need no account and no API key.
//!
//! Every service here was tried with a real upload before being added. A file is tried on each
//! turned-on service in order until one accepts it, so a service being down or refusing a file
//! type does not stop the upload.

use std::fs;
use std::path::Path;
use std::time::Duration;

use serde::Serialize;

#[derive(Serialize, Clone)]
pub struct ShareLink {
    pub name: String,
    pub url: String,
    pub provider: String,
    pub expires: String,
}

#[derive(Serialize, Clone)]
pub struct Provider {
    pub id: &'static str,
    pub name: &'static str,
    pub note: &'static str,
}

/// In the order they are tried.
pub const PROVIDERS: [Provider; 4] = [
    Provider { id: "img402", name: "img402", note: "Pictures up to 10 MB. Small ones are kept permanently, larger ones for 30 days." },
    Provider { id: "catbox", name: "Catbox", note: "Any file up to 200 MB. Links are kept long-term." },
    Provider { id: "tmpfiles", name: "tmpfiles.org", note: "Any file up to 100 MB. Links are temporary." },
    Provider { id: "uguu", name: "Uguu", note: "Any file up to 100 MB. Links are temporary, a few hours." },
];

pub fn default_enabled() -> Vec<String> {
    PROVIDERS.iter().map(|p| p.id.to_string()).collect()
}

fn mime_for(ext: &str) -> &'static str {
    match ext {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "pdf" => "application/pdf",
        "mp4" => "video/mp4",
        "mp3" => "audio/mpeg",
        "zip" => "application/zip",
        "txt" => "text/plain",
        _ => "application/octet-stream",
    }
}

fn multipart(fields: &[(&str, &str)], file_field: &str, filename: &str, mime: &str, data: &[u8]) -> (String, Vec<u8>) {
    let boundary = format!(
        "----HaloBoundary{}",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
    );
    let mut body = Vec::new();
    for (k, v) in fields {
        body.extend(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{k}\"\r\n\r\n{v}\r\n").as_bytes());
    }
    let safe_name = filename.replace('"', "_").replace(['\r', '\n'], "");
    body.extend(
        format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{file_field}\"; filename=\"{safe_name}\"\r\nContent-Type: {mime}\r\n\r\n").as_bytes(),
    );
    body.extend(data);
    body.extend(format!("\r\n--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

fn post(url: &str, fields: &[(&str, &str)], file_field: &str, filename: &str, mime: &str, data: &[u8]) -> Result<String, String> {
    let (content_type, body) = multipart(fields, file_field, filename, mime, data);
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(12))
        .timeout(Duration::from_secs(600))
        .user_agent("Halo/0.1 (file sharing)")
        .build();
    match agent.post(url).set("Content-Type", &content_type).send_bytes(&body) {
        Ok(resp) => resp.into_string().map_err(|e| e.to_string()),
        Err(ureq::Error::Status(code, resp)) => {
            let text = resp.into_string().unwrap_or_default();
            let snippet: String = text.chars().take(120).collect();
            Err(format!("the service answered {code} {snippet}").trim().to_string())
        }
        Err(e) => Err(format!("couldn't reach the service ({e})")),
    }
}

fn try_provider(id: &str, name: &str, ext: &str, data: &[u8]) -> Result<(String, String), String> {
    let mime = mime_for(ext);
    match id {
        "img402" => {
            let text = post("https://img402.dev/api/free", &[], "file", name, mime, data)?;
            let v: serde_json::Value = serde_json::from_str(&text).map_err(|_| "unexpected answer".to_string())?;
            let url = v["url"].as_str().ok_or_else(|| v["error"].as_str().unwrap_or("no link returned").to_string())?;
            let expires = if v["expiresAt"].is_null() { "Kept permanently" } else { "Kept for 30 days" };
            Ok((url.to_string(), expires.to_string()))
        }
        "catbox" => {
            let text = post("https://catbox.moe/user/api.php", &[("reqtype", "fileupload")], "fileToUpload", name, mime, data)?;
            let url = text.trim();
            if url.starts_with("https://") {
                Ok((url.to_string(), "Kept long-term".to_string()))
            } else {
                Err(url.chars().take(120).collect())
            }
        }
        "tmpfiles" => {
            let text = post("https://tmpfiles.org/api/v1/upload", &[], "file", name, mime, data)?;
            let v: serde_json::Value = serde_json::from_str(&text).map_err(|_| "unexpected answer".to_string())?;
            let page = v["data"]["url"].as_str().ok_or("no link returned")?;
            // the page link shows a preview; the /dl/ link downloads the file itself
            Ok((page.replacen("tmpfiles.org/", "tmpfiles.org/dl/", 1), "Temporary link".to_string()))
        }
        "uguu" => {
            let text = post("https://uguu.se/upload", &[], "files[]", name, mime, data)?;
            let v: serde_json::Value = serde_json::from_str(&text).map_err(|_| "unexpected answer".to_string())?;
            let url = v["files"][0]["url"].as_str().ok_or("no link returned")?;
            Ok((url.to_string(), "Temporary link, a few hours".to_string()))
        }
        _ => Err("unknown service".into()),
    }
}

fn suitable(id: &str, ext: &str, size: u64) -> bool {
    let mb = 1024 * 1024;
    match id {
        "img402" => matches!(ext, "png" | "jpg" | "jpeg" | "gif" | "webp") && size <= 10 * mb,
        "catbox" => size <= 200 * mb,
        "tmpfiles" | "uguu" => size <= 100 * mb,
        _ => false,
    }
}

pub fn upload(path: &Path, enabled: &[String]) -> Result<ShareLink, String> {
    if path.is_dir() {
        return Err("Folders can't be shared as a link. Make a zip first.".into());
    }
    let data = fs::read(path).map_err(|e| e.to_string())?;
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "file".into());
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let size = data.len() as u64;

    let mut tried = false;
    let mut last_error = String::new();
    for provider in PROVIDERS.iter() {
        if !enabled.iter().any(|e| e == provider.id) || !suitable(provider.id, &ext, size) {
            continue;
        }
        tried = true;
        match try_provider(provider.id, &name, &ext, &data) {
            Ok((url, expires)) => return Ok(ShareLink { name, url, provider: provider.name.to_string(), expires }),
            Err(e) => last_error = format!("{} said: {e}", provider.name),
        }
    }
    if !tried {
        return Err("No turned-on upload service can take this file. Check Uploads in Halo's settings.".into());
    }
    Err(format!("The upload didn't work. {last_error}"))
}
