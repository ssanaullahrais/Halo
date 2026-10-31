//! A pinned, app-private recording runtime. Conversion keeps its existing FFmpeg.
use super::*;
use sha2::{Digest,Sha256};
use std::io::Read;

const BUILD:&str="ffmpeg-2026-10-29-git-31ba826726-essentials_build";
const ARCHIVE_BYTES:u64=36_325_390;
const ARCHIVE_SHA:&str="94c9d4eae0817f4c2cc8c78fe2d001db347ce4a433a52f1d9312ff6a886451f8";
const URL:&str="https://github.com/GyanD/codexffmpeg/releases/download/2026-10-29-git-31ba826726/ffmpeg-2026-10-29-git-31ba826726-essentials_build.7z";
static READY:Mutex<Option<PathBuf>>=Mutex::new(None);
fn directory()->PathBuf{crate::system::tools_dir().join("recorder-runtime")}
fn private_path()->PathBuf{directory().join(BUILD).join("bin/ffmpeg.exe")}
fn supports_idle_frames(help:&str)->bool{help.contains("dup_frames")&&help.contains("capture_cursor")&&help.contains("resize_mode")}
fn supported(path:&Path)->bool{
 let Ok(mut child)=std::process::Command::new(path).args(["-hide_banner","-h","filter=gfxcapture"]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).creation_flags(HIDDEN).spawn()else{return false;};
 let output=child.stdout.take().unwrap();let reader=thread::spawn(move||{let mut text=String::new();let _=output.take(64*1024).read_to_string(&mut text);text});
 let start=Instant::now();loop{match child.try_wait(){Ok(Some(status))=>return status.success()&&supports_idle_frames(&reader.join().unwrap_or_default()),Ok(None)if start.elapsed()<Duration::from_secs(5)=>thread::sleep(Duration::from_millis(20)),_=>{let _=child.kill();let _=child.wait();let _=reader.join();return false;}}}
}
pub(crate) fn ready()->bool{
 let mut cached=READY.lock().unwrap();if cached.as_ref().map(|p|p.is_file()).unwrap_or(false){return true;}
 for candidate in [private_path(),crate::system::ffmpeg_path()]{if candidate.is_file()&&supported(&candidate){*cached=Some(candidate);return true;}}false
}
pub(super) fn path()->PathBuf{if ready(){READY.lock().unwrap().as_ref().unwrap().clone()}else{crate::system::ffmpeg_path()}}
pub(super) fn require()->Res<()>{if ready(){Ok(())}else{Err("The recording engine is being prepared. Open System to see setup progress or retry its download.".into())}}
fn verified(path:&Path)->Res<bool>{
 let Ok(mut file)=fs::File::open(path)else{return Ok(false);};if file.metadata().map_err(|e|e.to_string())?.len()!=ARCHIVE_BYTES{return Ok(false);}
 let mut digest=Sha256::new();let mut buffer=[0u8;64*1024];loop{let n=file.read(&mut buffer).map_err(|e|e.to_string())?;if n==0{break;}digest.update(&buffer[..n]);}Ok(format!("{:x}",digest.finalize())==ARCHIVE_SHA)
}
pub(crate) fn install()->Res<String>{
 if ready(){return Ok("Recording engine is ready.".into());}
 let root=directory();fs::create_dir_all(&root).map_err(|e|e.to_string())?;let archive=root.join(format!("{BUILD}.7z"));
 if !verified(&archive)?{
  let agent=ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(15)).timeout_read(Duration::from_secs(30)).build();
  let response=agent.get(URL).call().map_err(|e|format!("Could not download the recording engine: {e}"))?;
  let partial=archive.with_extension("7z.partial");let mut reader=response.into_reader().take(ARCHIVE_BYTES+1);let mut file=fs::File::create(&partial).map_err(|e|e.to_string())?;let start=Instant::now();let mut buffer=[0u8;64*1024];
  loop{let n=reader.read(&mut buffer).map_err(|e|format!("Recording engine download interrupted: {e}"))?;if n==0{break;}file.write_all(&buffer[..n]).map_err(|e|e.to_string())?;if start.elapsed()>Duration::from_secs(900){return Err("The recording engine download timed out. Retry from System.".into());}}
  file.sync_all().map_err(|e|e.to_string())?;drop(file);if !verified(&partial)?{let _=fs::remove_file(&partial);return Err("The recording engine download failed its integrity check.".into());}if archive.exists(){fs::rename(&archive,archive.with_extension(format!("invalid-{}",chrono::Local::now().timestamp_millis()))).map_err(|e|e.to_string())?;}fs::rename(&partial,&archive).map_err(|e|e.to_string())?;
 }
 let staging=root.join(format!("unpack-{}",chrono::Local::now().timestamp_nanos_opt().unwrap_or_default()));fs::create_dir(&staging).map_err(|e|e.to_string())?;
 let mut child=std::process::Command::new("tar").arg("-xf").arg(&archive).arg("-C").arg(&staging).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).creation_flags(HIDDEN).spawn().map_err(|e|format!("Could not unpack the recording engine: {e}"))?;
 let start=Instant::now();loop{if let Some(status)=child.try_wait().map_err(|e|e.to_string())?{if !status.success(){return Err("The recording engine archive could not be unpacked.".into());}break;}if start.elapsed()>Duration::from_secs(60){let _=child.kill();let _=child.wait();return Err("Unpacking the recording engine timed out.".into());}thread::sleep(Duration::from_millis(30));}
 let unpacked=staging.join(BUILD);if !supported(&unpacked.join("bin/ffmpeg.exe")){return Err("The downloaded recording engine does not support reliable idle capture.".into());}
 // Publish only a verified, complete directory. Keep its bundled license/docs.
 let destination=root.join(BUILD);if destination.exists(){fs::rename(&destination,root.join(format!("previous-{}",chrono::Local::now().timestamp_millis()))).map_err(|e|e.to_string())?;}
 fs::rename(&unpacked,&destination).map_err(|e|e.to_string())?;let _=fs::remove_dir(&staging);
 *READY.lock().unwrap()=Some(destination.join("bin/ffmpeg.exe"));Ok("Recording engine installed.".into())
}

#[cfg(test)]mod tests{
 use super::*;
 #[test]fn capture_capability_is_required_instead_of_guessing_from_the_version(){assert!(!supports_idle_frames("gfxcapture capture_cursor resize_mode"));assert!(supports_idle_frames("gfxcapture dup_frames capture_cursor resize_mode"));}
 #[test]fn an_incomplete_archive_is_never_accepted(){let p=std::env::temp_dir().join(format!("halo-runtime-check-{}",chrono::Local::now().timestamp_nanos_opt().unwrap()));fs::write(&p,b"incomplete").unwrap();assert!(!verified(&p).unwrap());fs::remove_file(p).unwrap();}
}
