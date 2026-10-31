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

// Artboard bounds are descriptors in each layer's artb/artd/abdd tagged block.
// Crop the saved Photoshop composite, so effects/text use Photoshop's own rendering.
pub fn artboards(path:&Path)->Result<Vec<DynamicImage>,String>{
 let rects=artboard_rects(path)?;if rects.is_empty(){return Ok(vec![]);}
 let image=load(path)?;let mut images=Vec::new();
 for [left,top,right,bottom] in rects{
  if left<0.||top<0.||right>image.width()as f64||bottom>image.height()as f64||right<=left||bottom<=top{return Err("An artboard extends outside this PSD's saved composite. Save it with Maximize Compatibility in Photoshop before exporting.".into());}
  images.push(image.crop_imm(left as u32,top as u32,(right-left)as u32,(bottom-top)as u32));
 }
 Ok(images)
}
fn artboard_rects(path:&Path)->Result<Vec<[f64;4]>,String>{
 use std::io::{Read,Seek,SeekFrom};
 let mut file=fs::File::open(path).map_err(|e|e.to_string())?;
 let mut header=[0;26];file.read_exact(&mut header).map_err(|e|e.to_string())?;
 if &header[..4]!=b"8BPS"{return Err("Invalid Photoshop header.".into());}
 let version=u16::from_be_bytes([header[4],header[5]]);if ![1,2].contains(&version){return Err("Unsupported Photoshop version.".into());}let large=version==2;
 fn length(f:&mut fs::File,large:bool)->Result<u64,String>{let n=if large{8}else{4};let mut b=[0;8];f.read_exact(&mut b[8-n..]).map_err(|e|e.to_string())?;Ok(u64::from_be_bytes(b))}
 for _ in 0..2{let n=length(&mut file,false)?;file.seek(SeekFrom::Current(i64::try_from(n).map_err(|_|"PSD section too large")?)).map_err(|e|e.to_string())?;}
 let section=length(&mut file,large)?;if section==0{return Ok(vec![]);}
 let info=length(&mut file,large)?;if info==0{return Ok(vec![]);}
 let mut count=[0;2];file.read_exact(&mut count).map_err(|e|e.to_string())?;let count=i16::from_be_bytes(count).unsigned_abs();let mut result=Vec::new();
 let position=file.stream_position().map_err(|e|e.to_string())?;let end=position.checked_add(info.saturating_sub(2)).filter(|end|*end<=file.metadata().map(|m|m.len()).unwrap_or(0)).ok_or("Invalid PSD layer section length")?;
 for _ in 0..count{
  if file.stream_position().map_err(|e|e.to_string())?+18>end{return Err("Truncated PSD layer records.".into());}
  file.seek(SeekFrom::Current(16)).map_err(|e|e.to_string())?;
  let mut channels=[0;2];file.read_exact(&mut channels).map_err(|e|e.to_string())?;
  file.seek(SeekFrom::Current(u16::from_be_bytes(channels)as i64*if large{10}else{6}+12)).map_err(|e|e.to_string())?;
  let extra=length(&mut file,false)?;let position=file.stream_position().map_err(|e|e.to_string())?;
  if extra>64*1024*1024||position+extra>end{return Err("Invalid PSD layer metadata size.".into());}
  let mut bytes=vec![0;extra as usize];file.read_exact(&mut bytes).map_err(|e|e.to_string())?;
  let mut r=PsdReader{bytes:&bytes,pos:0};let mask=r.u32()?as usize;r.take(mask)?;let blend=r.u32()?as usize;r.take(blend)?;
  let name=r.take(1)?[0]as usize;r.take((name+1).div_ceil(4)*4-1)?;
  while r.bytes.len().saturating_sub(r.pos)>=12{
   let signature=r.take(4)?;if signature!=b"8BIM"&&signature!=b"8B64"{break;}
   let key=r.take(4)?;let long=signature==b"8B64"||(large&&[b"LMsk",b"Lr16",b"Lr32",b"Layr",b"Mt16",b"Mt32",b"Mtrn",b"Alph",b"FMsk",b"lnk2",b"FEid",b"FXid",b"PxSD"].contains(&key.try_into().unwrap()));
   let len=if long{u64::from_be_bytes(r.take(8)?.try_into().unwrap())}else{r.u32()?as u64};let len=usize::try_from(len).map_err(|_|"PSD block too large")?;
   let block=r.take(len)?;
   if [b"artb".as_slice(),b"artd",b"abdd"].contains(&key){let mut d=PsdReader{bytes:block,pos:0};if d.u32()?!=16{return Err("Unsupported artboard descriptor version.".into());}let values=d.descriptor(0)?;let rect=values.into_iter().find(|(key,_)|key==b"artboardRect").ok_or("Missing artboard rectangle")?.1;
    if let PsdValue::Object(rect)=rect{let side=|key:&[u8]|->Result<f64,String>{match rect.iter().find(|(k,_)|k==key).map(|(_,v)|v){Some(PsdValue::Number(n))if n.is_finite()=>Ok(*n),_=>Err("Invalid artboard bounds.".into())}};result.push([side(b"Left")?,side(b"Top ")?,side(b"Rght")?,side(b"Btom")?]);}
   }
   if len%2!=0&&r.pos<r.bytes.len(){r.take(1)?;}
  }
 }
 Ok(result)
}
enum PsdValue{Number(f64),Object(Vec<(Vec<u8>,PsdValue)>),Other}
struct PsdReader<'a>{bytes:&'a[u8],pos:usize}
impl<'a>PsdReader<'a>{
 fn take(&mut self,n:usize)->Result<&'a[u8],String>{let end=self.pos.checked_add(n).ok_or("PSD metadata overflow")?;let value=self.bytes.get(self.pos..end).ok_or("Truncated PSD metadata")?;self.pos=end;Ok(value)}
 fn u32(&mut self)->Result<u32,String>{Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))}
 fn id(&mut self)->Result<Vec<u8>,String>{let n=self.u32()?as usize;Ok(self.take(if n==0{4}else{n})?.to_vec())}
 fn unicode(&mut self)->Result<(),String>{let n=self.u32()?as usize;self.take(n.checked_mul(2).ok_or("PSD string too long")?)?;Ok(())}
 fn descriptor(&mut self,depth:usize)->Result<Vec<(Vec<u8>,PsdValue)>,String>{if depth>16{return Err("PSD descriptor nesting too deep.".into());}self.unicode()?;self.id()?;let n=self.u32()?;if n>10000{return Err("Too many PSD descriptor values.".into());}let mut values=Vec::new();for _ in 0..n{let key=self.id()?;let value=self.value(depth+1)?;values.push((key,value));}Ok(values)}
 fn value(&mut self,depth:usize)->Result<PsdValue,String>{if depth>16{return Err("PSD descriptor nesting too deep.".into());}let kind=self.take(4)?;Ok(match kind{
  b"Objc"|b"GlbO"=>PsdValue::Object(self.descriptor(depth)?),
  b"doub"=>PsdValue::Number(f64::from_be_bytes(self.take(8)?.try_into().unwrap())),
  b"UntF"=>{self.take(4)?;PsdValue::Number(f64::from_be_bytes(self.take(8)?.try_into().unwrap()))},
  b"long"=>PsdValue::Number(i32::from_be_bytes(self.take(4)?.try_into().unwrap())as f64),
  b"bool"=>{self.take(1)?;PsdValue::Other},b"TEXT"=>{self.unicode()?;PsdValue::Other},
  b"enum"=>{self.id()?;self.id()?;PsdValue::Other},
  b"VlLs"=>{let n=self.u32()?;if n>10000{return Err("PSD list too long.".into());}for _ in 0..n{self.value(depth+1)?;}PsdValue::Other},
  b"tdta"|b"alis"=>{let n=self.u32()?as usize;self.take(n)?;PsdValue::Other},
  _=>return Err(format!("Unsupported artboard descriptor type: {}",String::from_utf8_lossy(kind)))
 })}
}

#[cfg(test)]mod artboard_tests{
 use super::*;
 #[test]#[ignore="requires local Photoshop fixtures and optional image engine"]fn photoshop_artboards_export_separately(){
 let root=Path::new(env!("CARGO_MANIFEST_DIR")).join("../output/integration-research/psd-tools/tests/psd_files");
 assert!(root.is_dir(),"Local Photoshop fixtures are required");
 for name in ["artboard.psd","artboard-bgcolor.psd"]{let path=root.join(name);let rects=artboard_rects(&path).unwrap();assert_eq!(rects.len(),if name=="artboard.psd"{1}else{2},"{name}: {rects:?}");let images=artboards(&path).unwrap();assert_eq!(images.len(),rects.len());if name=="artboard-bgcolor.psd"{assert_eq!((images[0].width(),images[0].height()),(400,300));assert_eq!((images[1].width(),images[1].height()),(250,550));for (img,color) in images.iter().zip([[18u8,108,200],[200,95,18]]){let pixel=img.to_rgb8().get_pixel(0,0).0;for c in 0..3{assert!(pixel[c].abs_diff(color[c])<=2);}}}println!("{name}: {rects:?}");}
 }
 #[test]fn rejects_truncated_descriptors(){let mut r=PsdReader{bytes:&[0;3],pos:0};assert!(r.descriptor(0).is_err());}
}
