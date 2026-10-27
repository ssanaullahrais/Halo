use std::{fs,path::Path};
use base64::Engine;
use minisign_verify::{PublicKey,Signature};
fn main(){
 let args:Vec<_>=std::env::args().collect();let version=args[1].trim_start_matches('v');let dir=Path::new(&args[2]);
 let config:serde_json::Value=serde_json::from_slice(&fs::read("src-tauri/tauri.conf.json").unwrap()).unwrap();
 let key=base64::engine::general_purpose::STANDARD.decode(config["plugins"]["updater"]["pubkey"].as_str().unwrap()).unwrap();let key=PublicKey::decode(std::str::from_utf8(&key).unwrap()).unwrap();
 let manifest:serde_json::Value=serde_json::from_slice(&fs::read(dir.join("latest.json")).unwrap()).unwrap();assert_eq!(manifest["version"].as_str(),Some(version));
 let platforms=manifest["platforms"].as_object().unwrap();assert!(platforms.keys().any(|p|p.starts_with("windows-x86_64")));
 let prefix=format!("https://github.com/ssanaullahrais/Halo/releases/download/v{version}/");
 for platform in platforms.values(){let url=platform["url"].as_str().unwrap();assert!(url.starts_with(&prefix));let file=url.rsplit('/').next().unwrap();assert!(!file.contains(['\\','%']));
  let encoded=fs::read_to_string(dir.join(format!("{file}.sig"))).unwrap();assert_eq!(encoded.trim(),platform["signature"].as_str().unwrap().trim());
  let decoded=base64::engine::general_purpose::STANDARD.decode(encoded.trim()).unwrap();let signature=Signature::decode(std::str::from_utf8(&decoded).unwrap()).unwrap();let mut data=fs::read(dir.join(file)).unwrap();key.verify(&data,&signature,false).expect("Signature rejected");let at=data.len()/2;data[at]^=1;assert!(key.verify(&data,&signature,false).is_err(),"Tampered artifact accepted");println!("{file}: valid signature accepted, tampering rejected");
 }
 assert!(dir.read_dir().unwrap().filter_map(Result::ok).any(|f|f.path().extension().map(|x|x=="exe").unwrap_or(false)));
 assert!(dir.read_dir().unwrap().filter_map(Result::ok).any(|f|f.path().extension().map(|x|x=="msi").unwrap_or(false)));
}
