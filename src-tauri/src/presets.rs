//! Halo's own typed conversion presets. No imported FileConverter code or commands.
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Preset {
    pub id: String, pub name: String, pub target: String,
    pub quality: u8, pub scale: u16, pub rotation: u16,
    pub fps: u16, pub speed: String, pub audio: bool,
    pub bitrate: u16, pub channels: u8, pub wav_bits: u8, pub rate_mode: String,
    pub template: String, pub copy_result: bool,
}
impl Default for Preset {
    fn default() -> Self { Self { id:String::new(),name:String::new(),target:"png".into(),quality:85,scale:100,rotation:0,fps:0,speed:"medium".into(),audio:true,bitrate:192,channels:0,wav_bits:16,rate_mode:"cbr".into(),template:"{name}".into(),copy_result:false } }
}
pub fn path() -> PathBuf { crate::system::tools_dir().parent().unwrap().join("presets.json") }
pub fn validate(p: &Preset) -> Result<(), String> {
    if !["cbr","vbr"].contains(&p.rate_mode.as_str()) {return Err("Choose CBR or VBR for MP3 audio.".into());}
    if p.id.is_empty() || p.id.len()>48 || !p.id.chars().all(|c|c.is_ascii_alphanumeric()||c=='-'||c=='_') { return Err("Preset IDs must contain letters, numbers, underscores or dashes.".into()); }
    if p.name.trim().is_empty() || p.name.len()>80 || p.name.chars().any(char::is_control) { return Err("Give the preset a name of up to 80 characters without control characters.".into()); }
    if !["jpg","png","webp","avif","bmp","tiff","gif","ico","pdf","mp4","webm","mkv","mov","avi","ogv","mp3","wav","ogg","flac","m4a","aac","opus"].contains(&p.target.as_str()) { return Err("Choose a supported output format.".into()); }
    if !(10..=100).contains(&p.quality) || !(1..=400).contains(&p.scale) || ![0,90,180,270].contains(&p.rotation) || p.fps>120 || !(32..=512).contains(&p.bitrate) || ![0,1,2].contains(&p.channels) || ![8,16,24,32].contains(&p.wav_bits) || !["ultrafast","fast","medium","slow"].contains(&p.speed.as_str()) { return Err("One of the conversion settings is outside its allowed range.".into()); }
    if p.template.is_empty() || p.template.len()>120 || p.template.chars().any(|c|c.is_control()||"\\/:*?\"<>|".contains(c)) { return Err("The filename template must be a name, without folders or reserved characters.".into()); }
    let remaining=p.template.replace("{name}","").replace("{format}","").replace("{index}","");
    if remaining.contains(['{','}']) { return Err("Use {name}, {format} or {index} in the filename template.".into()); }
    Ok(())
}
pub fn list() -> Result<Vec<Preset>,String> {
    match fs::read(path()) {
        Ok(data)=>{let list:Vec<Preset>=serde_json::from_slice(&data).map_err(|e|format!("Couldn't read conversion presets: {e}"))?;for p in &list {validate(p)?;}Ok(list)},
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>Ok(Vec::new()),
        Err(e)=>Err(e.to_string()),
    }
}
pub fn save(list: &[Preset]) -> Result<(),String> {
    if list.len()>32 {return Err("Keep up to 32 conversion presets.".into());}
    let mut ids=std::collections::HashSet::new();
    for p in list {validate(p)?;if !ids.insert(&p.id){return Err("Preset IDs must be unique.".into());}}
    let path=path();fs::create_dir_all(path.parent().unwrap()).map_err(|e|e.to_string())?;
    let staged=path.with_extension("json.new");
    fs::write(&staged,serde_json::to_vec_pretty(list).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
    fs::rename(staged,path).map_err(|e|e.to_string())?;
    publish_metadata(list)
}

/// Only names and destinations are shared with Explorer; encoding settings stay in Halo.
pub fn publish_metadata(list:&[Preset])->Result<(),String> {
    let metadata:Vec<_>=list.iter().map(|p|serde_json::json!({"id":p.id,"name":p.name,"target":p.target})).collect();
    let data=serde_json::to_string(&metadata).map_err(|e|e.to_string())?;
    let Some(base)=std::env::var_os("LOCALAPPDATA") else {return Ok(())};
    let packages=PathBuf::from(base).join("Packages");
    if let Ok(entries)=fs::read_dir(packages) {
        for entry in entries.flatten().filter(|e|e.file_name().to_string_lossy().starts_with("Halo.Wheel.ContextMenu_")) {
            let dir=entry.path().join("LocalState");fs::create_dir_all(&dir).map_err(|e|e.to_string())?;
            let target=dir.join("halo-presets.json");
            if fs::read_to_string(&target).ok().as_deref()!=Some(&data) {fs::write(target,&data).map_err(|e|e.to_string())?;}
        }
    }
    Ok(())
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn rejects_paths_and_unrecognised_tokens(){let mut p=Preset{id:"test".into(),name:"Test".into(),..Default::default()};assert!(validate(&p).is_ok());for t in ["../{name}","{unknown}","C:\\temp","file?"]{p.template=t.into();assert!(validate(&p).is_err());}}
    #[test] fn rejects_invalid_encoding_settings(){let mut p=Preset{id:"test".into(),name:"Test".into(),..Default::default()};p.rotation=45;assert!(validate(&p).is_err());p.rotation=90;p.wav_bits=12;assert!(validate(&p).is_err());}
}
