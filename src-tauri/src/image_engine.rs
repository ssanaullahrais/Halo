//! Optional extended-image decoding. Read flattened composites; Halo writes outputs.
use std::{fs, path::Path, process::{Command, Stdio}, os::windows::process::CommandExt, time::{Duration, Instant}};
use image::DynamicImage;

pub fn load(path: &Path) -> Result<DynamicImage, String> {
    let exe = crate::capabilities::image_engine_path().ok_or("This file needs the optional extended image engine. Install it in Settings → System.")?;
    let dir = crate::system::tools_dir().join("image-cache");
    fs::create_dir_all(&dir).map_err(|e|e.to_string())?;
    let id = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e|e.to_string())?.as_nanos();
    let output = dir.join(format!("{}-{id}.png", std::process::id()));
    // Only the supported source decoders and PNG output; no external delegates.
    let policy = dir.join("policy.xml");
    {
        let rules=r#"<policymap>
<policy domain="resource" name="memory" value="256MiB"/>
<policy domain="resource" name="map" value="512MiB"/>
<policy domain="resource" name="disk" value="1GiB"/>
<policy domain="resource" name="width" value="20000"/>
<policy domain="resource" name="height" value="20000"/>
<policy domain="resource" name="time" value="45"/>
<policy domain="resource" name="thread" value="2"/>
<policy domain="delegate" rights="none" pattern="*"/>
<policy domain="filter" rights="none" pattern="*"/>
<policy domain="path" rights="none" pattern="@*"/>
<policy domain="path" rights="none" pattern="|*"/>
<policy domain="coder" rights="none" pattern="*"/>
<policy domain="coder" rights="read" pattern="{PSD,DDS,TGA,EXR}"/>
<policy domain="coder" rights="read|write" pattern="PNG"/>
</policymap>"#;
        if fs::read_to_string(&policy).ok().as_deref()!=Some(rules) {fs::write(&policy,rules).map_err(|e|e.to_string())?;}
    }
    let input = path.canonicalize().map_err(|e|e.to_string())?;
    let input = input.to_string_lossy().trim_start_matches(r"\\?\").to_string();
    let log = fs::File::create(dir.join(format!("{id}.log"))).map_err(|e|e.to_string())?;
    let format=match crate::process::ext_lower(path).as_str(){"tga"=>"TGA","dds"=>"DDS","exr"=>"EXR",_=>"PSD"};
    let mut child = Command::new(exe)
        .env("MAGICK_CONFIGURE_PATH", &dir)
        .args(["-limit", "memory", "256MiB", "-limit", "map", "512MiB", "-limit", "disk", "1GiB"])
        .arg(format!("{format}:{input}[0]"))
        .args(["-colorspace", "sRGB"])
        .arg(format!("PNG:{}", output.display()))
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(log).creation_flags(0x0800_0000)
        .spawn().map_err(|e|format!("Couldn't start the PSD engine: {e}"))?;
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(|e|e.to_string())? {
            if !status.success() { return Err("The PSD engine couldn't decode this file. The saved composite may be missing or damaged.".into()); }
            break;
        }
        if start.elapsed() > Duration::from_secs(50) {
            let _=child.kill();let _=child.wait();
            return Err("This PSD took too long to decode. Try a smaller file or save a compatible composite in Photoshop.".into());
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    let mut reader=image::ImageReader::open(&output).map_err(|e|e.to_string())?;
    let mut limits=image::Limits::default();
    limits.max_image_width=Some(20000);limits.max_image_height=Some(20000);limits.max_alloc=Some(512*1024*1024);
    reader.limits(limits);
    reader.decode().map_err(|e|format!("The PSD engine produced an unreadable image: {e}"))
}
