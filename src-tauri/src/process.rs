use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use base64::Engine;
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::{CompressionType, FilterType as PngFilter, PngEncoder};
use image::imageops::FilterType;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader};
use serde::{Deserialize, Serialize};

const NO_WINDOW: u32 = 0x0800_0000;

#[derive(Deserialize, Clone)]
pub struct Req {
    pub files: Vec<String>,
    pub verb: String,
    pub option: String,
    pub value: Option<f64>,
    pub value2: Option<f64>,
    pub text: Option<String>,
    #[serde(default)]
    pub preset: Option<crate::presets::Preset>,
}

#[derive(Serialize, Clone)]
pub struct Outcome {
    pub name: String,
    pub ok: bool,
    pub message: String,
    pub out_path: Option<String>,
    pub before: u64,
    pub after: u64,
}

#[derive(Serialize)]
pub struct Thumb {
    pub data: String,
    pub w: u32,
    pub h: u32,
}

#[derive(Serialize, Clone)]
pub struct MediaInfo { pub w: u32, pub h: u32, pub has_audio: bool }

pub fn video_info(path: &Path) -> Option<MediaInfo> {
    let output=Command::new(crate::system::ffmpeg_path()).args(["-hide_banner","-i"]).arg(path)
        .stdin(Stdio::null()).creation_flags(NO_WINDOW).output().ok()?;
    let text=String::from_utf8_lossy(&output.stderr);
    let dimensions=text.lines().filter(|line|line.contains("Video:")).flat_map(|line|line.split(|c:char|c.is_whitespace()||c==','))
        .find_map(|token|{let (w,h)=token.split_once('x')?;Some((w.parse::<u32>().ok()?,h.parse::<u32>().ok()?))})?;
    Some(MediaInfo { w: dimensions.0,h:dimensions.1,has_audio:text.lines().any(|line|line.contains("Audio:")) })
}

pub struct Ctx<'a> {
    pub cancel: &'a AtomicBool,
    pub pid: &'a Mutex<Option<u32>>,
    pub report: &'a (dyn Fn(f64, u64) + Send + Sync),
}

pub(crate) struct Done {
    pub(crate) out: Option<PathBuf>,
    pub(crate) message: String,
}

pub use crate::capabilities::Kind;

pub(crate) fn ext_lower(p: &Path) -> String {
    p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

pub fn kind_of(p: &Path) -> Kind {
    crate::capabilities::kind_of(p)
}

pub fn size_of(p: &Path) -> u64 {
    if p.is_dir() {
        fs::read_dir(p)
            .map(|rd| rd.flatten().map(|e| size_of(&e.path())).sum())
            .unwrap_or(0)
    } else {
        fs::metadata(p).map(|m| m.len()).unwrap_or(0)
    }
}

pub(crate) fn unique(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let mut p = dir.join(format!("{stem}.{ext}"));
    let mut n = 1;
    while p.exists() {
        p = dir.join(format!("{stem} ({n}).{ext}"));
        n += 1;
    }
    p
}

pub(crate) fn stem_dir(p: &Path) -> (String, PathBuf) {
    (
        p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "file".into()),
        p.parent().map(|d| d.to_path_buf()).unwrap_or_default(),
    )
}

// ---------------------------------------------------------------- images

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Fmt {
    Jpg,
    Png,
    Webp,
    Bmp,
    Gif,
    Tiff,
    Ico,
    Avif,
}

fn fmt_of(p: &Path) -> Option<Fmt> {
    match ext_lower(p).as_str() {
        "jpg" | "jpeg" | "jfif" => Some(Fmt::Jpg),
        "png" => Some(Fmt::Png),
        "webp" => Some(Fmt::Webp),
        "bmp" => Some(Fmt::Bmp),
        "gif" => Some(Fmt::Gif),
        "tif" | "tiff" => Some(Fmt::Tiff),
        "ico" => Some(Fmt::Ico),
        "avif" => Some(Fmt::Avif),
        _ => None,
    }
}

fn ext_of(f: Fmt) -> &'static str {
    match f {
        Fmt::Jpg => "jpg",
        Fmt::Png => "png",
        Fmt::Webp => "webp",
        Fmt::Bmp => "bmp",
        Fmt::Gif => "gif",
        Fmt::Tiff => "tiff",
        Fmt::Ico => "ico",
        Fmt::Avif => "avif",
    }
}

pub(crate) fn err<E: ToString>(e: E) -> String {
    e.to_string()
}

pub(crate) fn load(path: &Path) -> Result<DynamicImage, String> {
    if crate::capabilities::external_image(path) { return crate::image_engine::load(path); }
    if ext_lower(path) == "avif" {
        let output = Command::new(crate::system::ffmpeg_path())
            .args(["-hide_banner", "-loglevel", "error", "-i"]).arg(path)
            .args(["-frames:v", "1", "-f", "image2pipe", "-c:v", "png", "-pix_fmt", "rgba", "pipe:1"])
            .stdin(Stdio::null()).creation_flags(NO_WINDOW).output().map_err(err)?;
        if !output.status.success() { return Err(format!("This AVIF can't be read: {}",String::from_utf8_lossy(&output.stderr).trim())); }
        return image::load_from_memory_with_format(&output.stdout, ImageFormat::Png).map_err(err);
    }
    let mut decoder = ImageReader::open(path)
        .map_err(err)?
        .with_guessed_format()
        .map_err(err)?
        .into_decoder()
        .map_err(err)?;
    let orientation = decoder.orientation().ok();
    let mut img = DynamicImage::from_decoder(decoder).map_err(err)?;
    if let Some(o) = orientation {
        img.apply_orientation(o);
    }
    Ok(img)
}

/// Composites transparency over white so formats without alpha don't turn it black.
fn flatten(img: &DynamicImage) -> image::RgbImage {
    if !img.color().has_alpha() {
        return img.to_rgb8();
    }
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let mut out = image::RgbImage::new(w, h);
    for (x, y, p) in rgba.enumerate_pixels() {
        let a = p[3] as u32;
        let f = |c: u8| ((c as u32 * a + 255 * (255 - a)) / 255) as u8;
        out.put_pixel(x, y, image::Rgb([f(p[0]), f(p[1]), f(p[2])]));
    }
    out
}

pub(crate) fn jpeg_bytes(img: &DynamicImage, quality: u8) -> Result<Vec<u8>, String> {
    let rgb = DynamicImage::ImageRgb8(flatten(img));
    let mut buf = Vec::new();
    rgb.write_with_encoder(JpegEncoder::new_with_quality(&mut buf, quality)).map_err(err)?;
    Ok(buf)
}

pub(crate) fn png_bytes(img: &DynamicImage) -> Result<Vec<u8>, String> {
    let prepared = if img.color().has_alpha() {
        DynamicImage::ImageRgba8(img.to_rgba8())
    } else {
        DynamicImage::ImageRgb8(img.to_rgb8())
    };
    let mut buf = Vec::new();
    prepared
        .write_with_encoder(PngEncoder::new_with_quality(&mut buf, CompressionType::Best, PngFilter::Adaptive))
        .map_err(err)?;
    Ok(buf)
}

fn save(img: &DynamicImage, fmt: Fmt, quality: u8, out: &Path) -> Result<(), String> {
    match fmt {
        Fmt::Jpg => fs::write(out, jpeg_bytes(img, quality)?).map_err(err),
        Fmt::Png => fs::write(out, png_bytes(img)?).map_err(err),
        Fmt::Webp => {
            let rgba = img.to_rgba8();
            let mem = webp::Encoder::from_rgba(&rgba, rgba.width(), rgba.height()).encode(quality as f32);
            fs::write(out, &*mem).map_err(err)
        }
        Fmt::Bmp => img.save_with_format(out, ImageFormat::Bmp).map_err(err),
        Fmt::Gif => DynamicImage::ImageRgba8(img.to_rgba8()).save_with_format(out, ImageFormat::Gif).map_err(err),
        Fmt::Tiff => {
            // TIFF's encoder has no grayscale+alpha variant; preserve both as RGBA.
            let image = if matches!(img, DynamicImage::ImageLumaA8(_) | DynamicImage::ImageLumaA16(_)) {
                DynamicImage::ImageRgba8(img.to_rgba8())
            } else { img.clone() };
            image.save_with_format(out, ImageFormat::Tiff).map_err(err)
        },
        Fmt::Ico => {
            let icon=if img.width()>256 || img.height()>256 {img.thumbnail(256,256)} else {img.clone()};
            DynamicImage::ImageRgba8(icon.to_rgba8()).save_with_format(out, ImageFormat::Ico).map_err(err)
        },
        Fmt::Avif => {
            let rgba = img.to_rgba8();
            let (w, h) = rgba.dimensions();
            let pixels: Vec<rgb::RGBA8> = rgba.pixels().map(|p| rgb::RGBA8::new(p[0], p[1], p[2], p[3])).collect();
            let encoded = ravif::Encoder::new()
                .with_quality(quality as f32)
                .with_speed(7)
                .encode_rgba(imgref::Img::new(&pixels[..], w as usize, h as usize))
                .map_err(err)?;
            fs::write(out, encoded.avif_file).map_err(err)
        }
    }
}

pub(crate) fn fmt_by_name(name: &str) -> Option<Fmt> {
    match name {
        "jpg" | "jpeg" => Some(Fmt::Jpg),
        "png" => Some(Fmt::Png),
        "webp" => Some(Fmt::Webp),
        "bmp" => Some(Fmt::Bmp),
        "gif" => Some(Fmt::Gif),
        "tiff" | "tif" => Some(Fmt::Tiff),
        "ico" => Some(Fmt::Ico),
        "avif" => Some(Fmt::Avif),
        _ => None,
    }
}

/// Saves a picture in the format called `name` with sensible quality.
pub(crate) fn save_as(img: &DynamicImage, name: &str, out: &Path) -> Result<(), String> {
    let fmt = fmt_by_name(name).ok_or_else(|| format!("Can't save as {name}."))?;
    let quality = match fmt {
        Fmt::Jpg => 92,
        Fmt::Webp => 90,
        Fmt::Avif => 80,
        _ => 100,
    };
    save(img, fmt, quality, out)
}

pub(crate) fn ext_for(name: &str) -> &'static str {
    fmt_by_name(name).map(ext_of).unwrap_or("png")
}

/// Store RGB pixels and a separate soft alpha mask without JPEG re-encoding.
pub(crate) fn write_pdf(img: &DynamicImage, out: &Path) -> Result<(), String> {
    let (w, h) = (img.width(), img.height());
    let compress = |bytes: &[u8]| -> Result<Vec<u8>, String> {
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(bytes).map_err(err)?;
        encoder.finish().map_err(err)
    };
    let rgb = compress(img.to_rgb8().as_raw())?;
    let alpha = if img.color().has_alpha() {
        let values: Vec<u8> = img.to_rgba8().pixels().map(|p| p[3]).collect();
        if values.iter().any(|a| *a != 255) { Some(compress(&values)?) } else { None }
    } else { None };
    let (pw, ph) = (w as f64 * 0.75, h as f64 * 0.75);
    let mut buf: Vec<u8> = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    let mut obj = |buf: &mut Vec<u8>, body: &[u8]| {
        offsets.push(buf.len());
        buf.extend_from_slice(body);
    };
    obj(&mut buf, b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
    obj(&mut buf, b"2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n");
    obj(
        &mut buf,
        format!(
            "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {pw:.2} {ph:.2}] /Resources << /XObject << /Im0 4 0 R >> >> /Contents 5 0 R >>\nendobj\n"
        )
        .as_bytes(),
    );
    let mut image_obj = format!(
        "4 0 obj\n<< /Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode {} /Length {} >>\nstream\n",
        if alpha.is_some() { "/SMask 6 0 R" } else { "" }, rgb.len()
    )
    .into_bytes();
    image_obj.extend_from_slice(&rgb);
    image_obj.extend_from_slice(b"\nendstream\nendobj\n");
    obj(&mut buf, &image_obj);
    let content = format!("q {pw:.2} 0 0 {ph:.2} 0 0 cm /Im0 Do Q");
    obj(
        &mut buf,
        format!("5 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n", content.len()).as_bytes(),
    );
    if let Some(alpha) = alpha {
        let mut mask = format!("6 0 obj\n<< /Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /FlateDecode /Length {} >>\nstream\n", alpha.len()).into_bytes();
        mask.extend_from_slice(&alpha);
        mask.extend_from_slice(b"\nendstream\nendobj\n");
        obj(&mut buf, &mask);
    }
    let xref = buf.len();
    buf.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1).as_bytes());
    for o in &offsets {
        buf.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", offsets.len() + 1).as_bytes(),
    );
    fs::write(out, buf).map_err(err)
}

fn quality_of(req: &Req) -> u8 {
    match req.option.as_str() {
        "small" => 55,
        "medium" => 72,
        "large" => 86,
        _ => req.value.unwrap_or(70.0).clamp(10.0, 95.0) as u8,
    }
}

fn writable_fmt(path: &Path) -> Option<Fmt> {
    if crate::capabilities::external_image(path) { Some(Fmt::Png) } else { fmt_of(path) }
}

/// Finds the best quality that keeps the picture under the wanted size (in KB).
fn image_target(path: &Path, req: &Req, rep: &dyn Fn(f64, u64)) -> Result<Done, String> {
    let src = writable_fmt(path).ok_or("This image type isn't supported.")?;
    let target_fmt = match src {
        Fmt::Png => return Err("A PNG can't be squeezed to an exact size. Convert it to JPG or WebP first.".into()),
        Fmt::Gif => return Err("GIFs can't be compressed here. Try converting to WebP.".into()),
        Fmt::Webp => Fmt::Webp,
        _ => Fmt::Jpg,
    };
    let want_kb = req.value.unwrap_or(300.0).max(5.0) as usize;
    let want = want_kb * 1024;
    rep(0.1, 0);
    let img = load(path)?;
    rep(0.3, 0);
    let encode = |q: u8| -> Result<Vec<u8>, String> {
        if target_fmt == Fmt::Webp {
            let rgba = img.to_rgba8();
            let mem = webp::Encoder::from_rgba(&rgba, rgba.width(), rgba.height()).encode(q as f32);
            Ok((*mem).to_vec())
        } else {
            jpeg_bytes(&img, q)
        }
    };
    let (mut lo, mut hi) = (5u8, 95u8);
    let mut best: Option<Vec<u8>> = None;
    while lo <= hi {
        let mid = lo + (hi - lo) / 2;
        let bytes = encode(mid)?;
        if bytes.len() <= want {
            best = Some(bytes);
            lo = mid + 1;
        } else {
            hi = mid - 1;
        }
        rep(0.3 + 0.6 * (mid as f64 / 100.0), 0);
    }
    let bytes = best.ok_or("That size is too small for this picture. Try a larger target.")?;
    let (stem, dir) = stem_dir(path);
    let out = unique(&dir, &format!("{stem}-{want_kb}KB"), ext_of(target_fmt));
    fs::write(&out, bytes).map_err(err)?;
    rep(1.0, size_of(&out));
    Ok(Done { out: Some(out), message: String::new() })
}

fn image_compress(path: &Path, req: &Req, rep: &dyn Fn(f64, u64)) -> Result<Done, String> {
    if req.option == "target" {
        return image_target(path, req, rep);
    }
    let src = writable_fmt(path).ok_or("This image type isn't supported.")?;
    if src == Fmt::Gif {
        return Err("GIFs can't be compressed here. Try converting to WebP.".into());
    }
    let q = quality_of(req);
    let target = match src {
        Fmt::Jpg | Fmt::Png | Fmt::Webp => src,
        _ => Fmt::Jpg,
    };
    rep(0.1, 0);
    let img = load(path)?;
    rep(0.45, 0);
    let (stem, dir) = stem_dir(path);
    let label = match req.option.as_str() {
        "small" | "medium" | "large" | "auto" => req.option.clone(),
        _ => format!("q{q}"),
    };
    let out = unique(&dir, &format!("{stem}-{label}"), ext_of(target));
    if target == Fmt::Png {
        // A PNG can't be shrunk by a quality setting, so the presets reduce its colours (keeping the smaller result).
        let colors = match req.option.as_str() {
            "small" => 64,
            "medium" | "auto" => 128,
            "large" => 256,
            _ => (32.0 + (q as f64 - 10.0) / 85.0 * 224.0) as usize,
        };
        let lossless = png_bytes(&img)?;
        let best = match png_palette_bytes(&img, colors) {
            Ok(reduced) if reduced.len() < lossless.len() => reduced,
            _ => lossless,
        };
        fs::write(&out, best).map_err(err)?;
    } else {
        save(&img, target, q, &out)?;
    }
    rep(0.9, 0);
    let (before, after) = (size_of(path), size_of(&out));
    rep(1.0, after);
    // The result is always kept as a new file beside the original; the original is never touched.
    let message = if after >= before {
        format!("Couldn't get it smaller ({} → {}). A copy was saved.", kb(before), kb(after))
    } else {
        String::new()
    };
    Ok(Done { out: Some(out), message })
}

fn kb(n: u64) -> String {
    format!("{:.1} KB", n as f64 / 1024.0)
}

/// A PNG with a reduced colour palette (NeuQuant), much smaller for photos and gradients.
fn png_palette_bytes(img: &DynamicImage, colors: usize) -> Result<Vec<u8>, String> {
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let quant = color_quant::NeuQuant::new(10, colors.clamp(2, 256), rgba.as_raw());
    let indices: Vec<u8> = rgba.pixels().map(|p| quant.index_of(&p.0) as u8).collect();
    let (mut palette, mut alpha) = (Vec::new(), Vec::new());
    for c in quant.color_map_rgba().chunks(4) {
        palette.extend_from_slice(&c[..3]);
        alpha.push(c[3]);
    }
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Indexed);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_palette(palette);
        if alpha.iter().any(|a| *a < 255) {
            enc.set_trns(alpha);
        }
        enc.set_compression(png::Compression::Best);
        let mut writer = enc.write_header().map_err(err)?;
        writer.write_image_data(&indices).map_err(err)?;
    }
    Ok(out)
}

fn image_convert(path: &Path, req: &Req, rep: &dyn Fn(f64, u64)) -> Result<Done, String> {
    let src = fmt_of(path);
    let (stem, dir) = stem_dir(path);
    let target = req.option.as_str();
    if req.preset.is_none() && fmt_by_name(target).is_some() && fmt_by_name(target) == src {
        return Err(format!("It's already a {}.", target.to_uppercase()));
    }
    rep(0.1, 0);
    let mut img = load(path)?;
    if let Some(preset)=&req.preset {
        if preset.scale!=100 {
            let w=(img.width() as u64*preset.scale as u64/100).max(1);
            let h=(img.height() as u64*preset.scale as u64/100).max(1);
            if w>20000 || h>20000 || w*h>100_000_000 {return Err("The preset would make this image too large.".into());}
            img=img.resize_exact(w as u32,h as u32,FilterType::Lanczos3);
        }
        img=match preset.rotation {90=>img.rotate90(),180=>img.rotate180(),270=>img.rotate270(),_=>img};
    }
    let quality=req.preset.as_ref().map(|p|p.quality).unwrap_or(92);
    rep(0.5, 0);
    let out = match target {
        "jpg" => {
            let o = unique(&dir, &stem, "jpg");
            save(&img, Fmt::Jpg, quality, &o)?;
            o
        }
        "png" => {
            let o = unique(&dir, &stem, "png");
            save(&img, Fmt::Png, 100, &o)?;
            o
        }
        "webp" => {
            let o = unique(&dir, &stem, "webp");
            save(&img, Fmt::Webp, req.preset.as_ref().map(|p|p.quality).unwrap_or(90), &o)?;
            o
        }
        "pdf" => {
            let o = unique(&dir, &stem, "pdf");
            write_pdf(&img, &o)?;
            o
        }
        "svg" => {
            let o = unique(&dir, &stem, "svg");
            let b64 = base64::engine::general_purpose::STANDARD.encode(png_bytes(&img)?);
            let (w, h) = (img.width(), img.height());
            let svg = format!(
                "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\"><image width=\"{w}\" height=\"{h}\" xlink:href=\"data:image/png;base64,{b64}\"/></svg>"
            );
            fs::write(&o, svg).map_err(err)?;
            o
        }
        name if fmt_by_name(name).is_some() => {
            let o = unique(&dir, &stem, ext_for(name));
            if req.preset.is_some() { save(&img, fmt_by_name(name).unwrap(), quality, &o)?; } else { save_as(&img, name, &o)?; }
            o
        }
        other => return Err(format!("Can't convert to {other}.")),
    };
    rep(1.0, size_of(&out));
    Ok(Done { out: Some(out), message: String::new() })
}

fn image_resize(path: &Path, req: &Req, rep: &dyn Fn(f64, u64)) -> Result<Done, String> {
    let src = writable_fmt(path).ok_or("This image type isn't supported.")?;
    if src == Fmt::Gif {
        return Err("GIFs can't be resized here. Try converting to WebP first.".into());
    }
    rep(0.1, 0);
    let img = load(path)?;
    rep(0.4, 0);
    let (w, h) = (img.width(), img.height());
    let resized = match req.option.as_str() {
        "percent" => {
            let p = req.value.unwrap_or(50.0).clamp(1.0, 400.0) / 100.0;
            img.resize_exact(((w as f64 * p) as u32).max(1), ((h as f64 * p) as u32).max(1), FilterType::Lanczos3)
        }
        "custom" => {
            let nw = req.value.unwrap_or(w as f64).clamp(1.0, 20000.0) as u32;
            let nh = req.value2.unwrap_or(h as f64).clamp(1.0, 20000.0) as u32;
            img.resize_exact(nw, nh, FilterType::Lanczos3)
        }
        preset => {
            let (bw, bh) = match preset {
                "social" => (1080, 1080),
                "wallpaper" => (1920, 1080),
                "email" => (800, 600),
                _ => return Err("Unknown size.".into()),
            };
            img.resize(bw, bh, FilterType::Lanczos3)
        }
    };
    if src == Fmt::Ico && (resized.width() > 256 || resized.height() > 256) {
        return Err("ICO images are limited to 256 × 256 pixels. Convert to PNG for larger sizes.".into());
    }
    rep(0.75, 0);
    let (stem, dir) = stem_dir(path);
    let out = unique(&dir, &format!("{stem}-{}x{}", resized.width(), resized.height()), ext_of(src));
    save(&resized, src, 92, &out)?;
    rep(1.0, size_of(&out));
    Ok(Done { out: Some(out), message: String::new() })
}

fn image_flip(path: &Path, horizontal: bool) -> Result<Done, String> {
    let src = writable_fmt(path).ok_or("Flip works on pictures only.")?;
    if src == Fmt::Gif {
        return Err("GIFs can't be flipped here.".into());
    }
    let img = load(path)?;
    let flipped = if horizontal { img.fliph() } else { img.flipv() };
    let (stem, dir) = stem_dir(path);
    let out = unique(&dir, &format!("{stem}-{}", if horizontal { "flipped-h" } else { "flipped-v" }), ext_of(src));
    save(&flipped, src, 95, &out)?;
    let name = out.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    Ok(Done { out: Some(out), message: format!("Flipped copy saved as {name}") })
}

fn image_rotate(path: &Path, left: bool) -> Result<Done, String> {
    let src = writable_fmt(path).ok_or("Rotate works on images only.")?;
    if src == Fmt::Gif {
        return Err("GIFs can't be rotated here.".into());
    }
    let img = load(path)?;
    let rotated = if left { img.rotate270() } else { img.rotate90() };
    let (stem, dir) = stem_dir(path);
    let out = unique(&dir, &format!("{stem}-{}", if left { "rotated-left" } else { "rotated-right" }), ext_of(src));
    save(&rotated, src, 95, &out)?;
    let name = out.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    Ok(Done { out: Some(out), message: format!("Rotated copy saved as {name}") })
}

// ---------------------------------------------------------------- video

fn ffmpeg_duration(input: &Path) -> f64 {
    let out = Command::new(crate::system::ffmpeg_path())
        .args(["-hide_banner", "-i"])
        .arg(input)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .creation_flags(NO_WINDOW)
        .output();
    let Ok(out) = out else { return 0.0 };
    let text = String::from_utf8_lossy(&out.stderr);
    let Some(idx) = text.find("Duration: ") else { return 0.0 };
    let t = &text[idx + 10..];
    let t = t.split(',').next().unwrap_or("");
    let parts: Vec<f64> = t.trim().split(':').filter_map(|p| p.parse().ok()).collect();
    if parts.len() == 3 {
        parts[0] * 3600.0 + parts[1] * 60.0 + parts[2]
    } else {
        0.0
    }
}

fn ffmpeg_run(input: &Path, args: &[String], out: &Path, ctx: &Ctx, rep: &dyn Fn(f64, u64)) -> Result<(), String> {
    let duration = ffmpeg_duration(input);
    let mut child = Command::new(crate::system::ffmpeg_path())
        .args(["-y", "-hide_banner", "-nostats", "-loglevel", "error", "-progress", "pipe:1", "-i"])
        .arg(input)
        .args(args)
        .arg(out)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(NO_WINDOW)
        .spawn()
        .map_err(|_| "FFmpeg isn't installed, so videos can't be processed.".to_string())?;
    *ctx.pid.lock().unwrap() = Some(child.id());

    let mut stderr = child.stderr.take().unwrap();
    let stderr_thread = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stderr.read_to_string(&mut s);
        s
    });
    if let Some(stdout) = child.stdout.take() {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Some(us) = line.strip_prefix("out_time_us=") {
                if let (Ok(us), true) = (us.trim().parse::<f64>(), duration > 0.0) {
                    rep((us / 1e6 / duration).clamp(0.0, 0.99), size_of(out));
                }
            }
        }
    }
    let status = child.wait().map_err(err)?;
    *ctx.pid.lock().unwrap() = None;
    let stderr_text = stderr_thread.join().unwrap_or_default();
    if ctx.cancel.load(Ordering::SeqCst) {
        let _ = fs::remove_file(out);
        return Err("Cancelled".into());
    }
    if !status.success() {
        let _ = fs::remove_file(out);
        return Err(stderr_text.lines().last().unwrap_or("FFmpeg failed.").to_string());
    }
    rep(1.0, size_of(out));
    Ok(())
}

fn s(v: &str) -> String {
    v.to_string()
}

fn video_compress(path: &Path, req: &Req, ctx: &Ctx, rep: &dyn Fn(f64, u64)) -> Result<Done, String> {
    let crf = match req.option.as_str() {
        "small" => 30.0,
        "medium" => 26.0,
        "large" => 22.0,
        _ => (45.0 - req.value.unwrap_or(70.0).clamp(10.0, 95.0) * 0.25).round(),
    };
    let (stem, dir) = stem_dir(path);
    let label = if matches!(req.option.as_str(), "small" | "medium" | "large" | "auto") { req.option.clone() } else { "custom".into() };
    let out = unique(&dir, &format!("{stem}-{label}"), "mp4");
    let args = vec![
        s("-c:v"), s("libx264"), s("-crf"), format!("{crf}"), s("-preset"), s("medium"),
        s("-c:a"), s("aac"), s("-b:a"), s("128k"), s("-movflags"), s("+faststart"),
    ];
    ffmpeg_run(path, &args, &out, ctx, rep)?;
    Ok(Done { out: Some(out), message: String::new() })
}

fn audio_compress(path: &Path, req: &Req, ctx: &Ctx, rep: &dyn Fn(f64, u64)) -> Result<Done, String> {
    let kbps = match req.option.as_str() {
        "small" => 64,
        "medium" | "auto" => 128,
        "large" => 192,
        _ => (32.0 + (req.value.unwrap_or(70.0).clamp(10.0, 95.0) - 10.0) / 85.0 * 224.0) as u32,
    };
    let (stem, dir) = stem_dir(path);
    let out = unique(&dir, &format!("{stem}-{kbps}kbps"), "mp3");
    let args = vec![s("-vn"), s("-c:a"), s("libmp3lame"), s("-b:a"), format!("{kbps}k")];
    ffmpeg_run(path, &args, &out, ctx, rep)?;
    Ok(Done { out: Some(out), message: String::new() })
}

/// Copies the picture, video or audio without its hidden details (camera, place, dates).
fn strip_meta(path: &Path, ctx: &Ctx, rep: &dyn Fn(f64, u64)) -> Result<Done, String> {
    let (stem, dir) = stem_dir(path);
    let ext = ext_lower(path);
    let out = match kind_of(path) {
        Kind::Image => {
            let src = writable_fmt(path).ok_or("This image type isn't supported.")?;
            if src == Fmt::Gif {
                return Err("GIFs can't be cleaned here.".into());
            }
            let img = load(path)?;
            let out = unique(&dir, &format!("{stem}-clean"), ext_of(src));
            save(&img, src, 95, &out)?;
            out
        }
        Kind::Video | Kind::Audio => {
            // FFmpeg recognises the 3GP muxer by .3gp, but not the .3gpp alias.
            let out = unique(&dir, &format!("{stem}-clean"), if ext=="3gpp" {"3gp"} else {&ext});
            let args = vec![s("-map_metadata"), s("-1"), s("-c"), s("copy")];
            ffmpeg_run(path, &args, &out, ctx, rep)?;
            out
        }
        _ => return Err("Remove metadata works on pictures, videos and audio.".into()),
    };
    let name = out.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    Ok(Done { out: Some(out), message: format!("Details removed. Saved as {name}") })
}

fn video_convert(path: &Path, req: &Req, ctx: &Ctx, rep: &dyn Fn(f64, u64)) -> Result<Done, String> {
    let target = req.option.as_str();
    if req.preset.is_none() && ext_lower(path) == target {
        return Err(format!("It's already a {}.", target.to_uppercase()));
    }
    let mut args: Vec<String> = match target {
        "mp4" => vec![s("-c:v"), s("libx264"), s("-crf"), s("23"), s("-c:a"), s("aac"), s("-movflags"), s("+faststart")],
        "webm" => vec![s("-c:v"), s("libvpx-vp9"), s("-crf"), s("32"), s("-b:v"), s("0"), s("-c:a"), s("libopus")],
        "gif" => vec![s("-vf"), s("fps=12,scale=480:-1:flags=lanczos"), s("-loop"), s("0")],
        "mp3" => vec![s("-vn"), s("-c:a"), s("libmp3lame"), s("-q:a"), s("2")],
        "wav" => vec![s("-vn"), s("-c:a"), s("pcm_s16le")],
        "mkv" => vec![s("-c:v"), s("libx264"), s("-crf"), s("23"), s("-c:a"), s("aac")],
        "mov" => vec![s("-c:v"), s("libx264"), s("-crf"), s("23"), s("-c:a"), s("aac"), s("-movflags"), s("+faststart")],
        "avi" => vec![s("-c:v"), s("libx264"), s("-crf"), s("23"), s("-c:a"), s("libmp3lame")],
        "ogv" => vec![s("-c:v"),s("libtheora"),s("-q:v"),s("7"),s("-c:a"),s("libvorbis"),s("-q:a"),s("5")],
        "m4a" | "aac" => vec![s("-vn"), s("-c:a"), s("aac"), s("-b:a"), s("192k")],
        "opus" => vec![s("-vn"), s("-c:a"), s("libopus"), s("-b:a"), s("128k")],
        "ogg" => vec![s("-vn"), s("-c:a"), s("libvorbis"), s("-q:a"), s("5")],
        "flac" => vec![s("-vn"), s("-c:a"), s("flac")],
        other => return Err(format!("Can't convert to {other}.")),
    };
    if let Some(preset)=&req.preset {
        let video=["mp4","webm","mkv","mov","avi","ogv","gif"].contains(&target);
        if video {
            if target!="gif" {
                let crf=(51.0*(100-preset.quality) as f64/100.0).round().clamp(0.0,45.0) as u8;
                if let Some(i)=args.iter().position(|a|a=="-crf") {args[i+1]=crf.to_string();}
                if target=="ogv" {if let Some(i)=args.iter().position(|a|a=="-q:v") {args[i+1]=(preset.quality as f64/10.0).to_string();}}
                if ["mp4","mkv","mov","avi"].contains(&target) {args.extend([s("-preset"),preset.speed.clone()]);}
            } else {args=vec![s("-loop"),s("0")];}
            let mut filters=Vec::new();
            if preset.scale!=100 || target!="gif" {let scale=preset.scale as f64/100.0;filters.push(format!("scale=ceil(iw*{scale}/2)*2:ceil(ih*{scale}/2)*2"));}
            match preset.rotation {90=>filters.push("transpose=1".into()),180=>filters.push("hflip,vflip".into()),270=>filters.push("transpose=2".into()),_=>{}}
            if preset.fps>0 {filters.push(format!("fps={}",preset.fps));}
            if !filters.is_empty(){args.extend([s("-vf"),filters.join(",")]);}
            if !preset.audio {args.push(s("-an"));}
        }
        if !video || preset.audio {
            if target=="wav" {
                let codec=match preset.wav_bits {8=>"pcm_u8",24=>"pcm_s24le",32=>"pcm_s32le",_=>"pcm_s16le"};
                args=vec![s("-vn"),s("-c:a"),s(codec)];
            } else if target=="mp3" {
                args=vec![s("-vn"),s("-c:a"),s("libmp3lame")];
                if preset.rate_mode=="vbr" {args.extend([s("-q:a"),((100-preset.quality)/10).min(9).to_string()]);}
                else {args.extend([s("-b:a"),format!("{}k",preset.bitrate)]);}
            } else if target=="ogg" {
                args=vec![s("-vn"),s("-c:a"),s("libvorbis"),s("-b:a"),format!("{}k",preset.bitrate)];
            } else if !["flac","gif"].contains(&target) {
                args.extend([s("-b:a"),format!("{}k",preset.bitrate)]);
            }
            if preset.channels>0 && target!="gif" {args.extend([s("-ac"),preset.channels.to_string()]);}
        }
    }
    if ext_lower(path)=="gif" && req.preset.is_none() && ["mp4","webm","mkv","mov","avi","ogv"].contains(&target) {
        args.extend([s("-vf"),s("scale=ceil(iw/2)*2:ceil(ih/2)*2")]);
    }
    // FFmpeg 9 may choose planar RGB for GIF/alpha input. VP9 rejects that
    // default, and H.264 players expect 4:2:0. Select a supported format.
    if target=="webm" {args.extend([s("-pix_fmt"),s("yuva420p")]);}
    else if ["mp4","mkv","mov","avi","ogv"].contains(&target) {args.extend([s("-pix_fmt"),s("yuv420p")]);}
    let (stem, dir) = stem_dir(path);
    let out = unique(&dir, &stem, target);
    ffmpeg_run(path, &args, &out, ctx, rep)?;
    Ok(Done { out: Some(out), message: String::new() })
}

fn video_resize(path: &Path, req: &Req, ctx: &Ctx, rep: &dyn Fn(f64, u64)) -> Result<Done, String> {
    let (filter, label) = match req.option.as_str() {
        "percent" => {
            let p = req.value.unwrap_or(50.0).clamp(5.0, 100.0) / 100.0;
            (format!("scale=trunc(iw*{p}/2)*2:trunc(ih*{p}/2)*2"), format!("{}pct", (p * 100.0) as u32))
        }
        "custom" => {
            let w = (req.value.unwrap_or(1280.0) as u32).max(2) & !1;
            let h = (req.value2.unwrap_or(720.0) as u32).max(2) & !1;
            (format!("scale={w}:{h}"), format!("{w}x{h}"))
        }
        "social" => (s("scale=1080:1080:force_original_aspect_ratio=decrease:force_divisible_by=2"), s("1080")),
        "wallpaper" => (s("scale=1920:1080:force_original_aspect_ratio=decrease:force_divisible_by=2"), s("1080p")),
        "email" => (s("scale=800:600:force_original_aspect_ratio=decrease:force_divisible_by=2"), s("800")),
        _ => return Err("Unknown size.".into()),
    };
    let (stem, dir) = stem_dir(path);
    let out = unique(&dir, &format!("{stem}-{label}"), "mp4");
    let args = vec![
        s("-vf"), filter, s("-c:v"), s("libx264"), s("-crf"), s("23"), s("-preset"), s("medium"),
        s("-c:a"), s("aac"), s("-movflags"), s("+faststart"),
    ];
    ffmpeg_run(path, &args, &out, ctx, rep)?;
    Ok(Done { out: Some(out), message: String::new() })
}

// ---------------------------------------------------------------- other

pub(crate) fn powershell(script: &str, envs: &[(&str, String)]) -> Result<String, String> {
    let mut cmd = Command::new("powershell.exe");
    cmd.args(["-NoLogo", "-NoProfile", "-NonInteractive", "-STA", "-ExecutionPolicy", "Bypass", "-Command", script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(NO_WINDOW);
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let out = cmd.output().map_err(err)?;
    if !out.status.success() {
        let e = String::from_utf8_lossy(&out.stderr);
        return Err(e.lines().next().unwrap_or("Something went wrong.").to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn zip_it(path: &Path, rep: &dyn Fn(f64, u64)) -> Result<Done, String> {
    let (stem, dir) = stem_dir(path);
    let out = unique(&dir, &stem, "zip");
    rep(0.2, 0);
    powershell(
        "Compress-Archive -LiteralPath $env:HALO_SRC -DestinationPath $env:HALO_DST -CompressionLevel Optimal -Force",
        &[("HALO_SRC", path.to_string_lossy().to_string()), ("HALO_DST", out.to_string_lossy().to_string())],
    )?;
    rep(1.0, size_of(&out));
    Ok(Done { out: Some(out), message: String::new() })
}

fn recycle(path: &Path) -> Result<Done, String> {
    let method = if path.is_dir() { "DeleteDirectory" } else { "DeleteFile" };
    powershell(
        &format!(
            "Add-Type -AssemblyName Microsoft.VisualBasic; [Microsoft.VisualBasic.FileIO.FileSystem]::{method}($env:HALO_SRC, 'OnlyErrorDialogs', 'SendToRecycleBin')"
        ),
        &[("HALO_SRC", path.to_string_lossy().to_string())],
    )?;
    Ok(Done { out: None, message: "Moved to the Recycle Bin.".into() })
}

fn rename(path: &Path, new_name: &str) -> Result<Done, String> {
    let name = new_name.trim();
    if name.is_empty() || name.chars().any(|c| "<>:\"/\\|?*".contains(c)) {
        return Err("Please use a name without these characters: < > : \" / \\ | ? *".into());
    }
    let target = path.with_file_name(name);
    if target.exists() {
        return Err("A file with that name already exists.".into());
    }
    fs::rename(path, &target).map_err(err)?;
    Ok(Done { out: Some(target), message: "Renamed.".into() })
}

fn move_to(path: &Path, dest: &Path) -> Result<Done, String> {
    let name = path.file_name().ok_or("Bad file name.")?;
    let target = dest.join(name);
    if target.exists() {
        return Err("A file with that name is already in that folder.".into());
    }
    if fs::rename(path, &target).is_err() {
        if path.is_dir() {
            return Err("Folders can only be moved within the same drive.".into());
        }
        fs::copy(path, &target).map_err(err)?;
        fs::remove_file(path).map_err(err)?;
    }
    Ok(Done { out: Some(target), message: format!("Moved to {}.", dest.to_string_lossy()) })
}

fn copy_dir(from: &Path, to: &Path) -> Result<(), String> {
    fs::create_dir_all(to).map_err(err)?;
    for entry in fs::read_dir(from).map_err(err)?.flatten() {
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target).map_err(err)?;
        }
    }
    Ok(())
}

/// Copies the file's contents to the clipboard as Base64 text.
fn copy_base64(path: &Path) -> Result<Done, String> {
    if path.is_dir() {
        return Err("Folders can't be copied as Base64.".into());
    }
    let size = size_of(path);
    if size > 8 * 1024 * 1024 {
        return Err(format!("This file is {:.1} MB, which is too big to copy as Base64 (the limit is 8 MB).", size as f64 / 1048576.0));
    }
    let data = fs::read(path).map_err(err)?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(&data);
    copy_text(&encoded)?;
    Ok(Done { out: None, message: format!("Copied as Base64 ({} characters).", encoded.len()) })
}

/// Reads the words in a picture with Windows' built-in text recognition and copies them.
fn copy_ocr(path: &Path) -> Result<Done, String> {
    if kind_of(path) != Kind::Image {
        return Err("Copy as text reads pictures.".into());
    }
    // The recognizer accepts PNG/JPEG well and has a size limit, so hand it a clean, reasonably sized PNG.
    let img = load(path)?;
    let img = if img.width().max(img.height()) > 4000 { img.thumbnail(4000, 4000) } else { img };
    let tmp = std::env::temp_dir().join(format!("halo-ocr-{}.png", std::process::id()));
    fs::write(&tmp, png_bytes(&img)?).map_err(err)?;
    let script = r#"
[Console]::OutputEncoding = [Text.Encoding]::UTF8
Add-Type -AssemblyName System.Runtime.WindowsRuntime
$null = [Windows.Storage.StorageFile, Windows.Storage, ContentType = WindowsRuntime]
$null = [Windows.Media.Ocr.OcrEngine, Windows.Foundation, ContentType = WindowsRuntime]
$null = [Windows.Graphics.Imaging.BitmapDecoder, Windows.Graphics, ContentType = WindowsRuntime]
function Await($op, $type) {
  $m = [System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object { $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1' } | Select-Object -First 1
  $task = $m.MakeGenericMethod($type).Invoke($null, @($op))
  $null = $task.Wait(-1)
  $task.Result
}
$file = Await ([Windows.Storage.StorageFile]::GetFileFromPathAsync($env:HALO_SRC)) ([Windows.Storage.StorageFile])
$stream = Await ($file.OpenAsync([Windows.Storage.FileAccessMode]::Read)) ([Windows.Storage.Streams.IRandomAccessStream])
$decoder = Await ([Windows.Graphics.Imaging.BitmapDecoder]::CreateAsync($stream)) ([Windows.Graphics.Imaging.BitmapDecoder])
$bitmap = Await ($decoder.GetSoftwareBitmapAsync()) ([Windows.Graphics.Imaging.SoftwareBitmap])
$engine = [Windows.Media.Ocr.OcrEngine]::TryCreateFromUserProfileLanguages()
if ($null -eq $engine) { throw 'No text recognition language is installed in Windows.' }
$result = Await ($engine.RecognizeAsync($bitmap)) ([Windows.Media.Ocr.OcrResult])
$result.Text
"#;
    let out = powershell(script, &[("HALO_SRC", tmp.to_string_lossy().to_string())]);
    let _ = fs::remove_file(&tmp);
    let text = out?.trim().to_string();
    if text.is_empty() {
        return Err("No text was found in this picture.".into());
    }
    copy_text(&text)?;
    Ok(Done { out: None, message: format!("Copied {} characters of text.", text.chars().count()) })
}

/// Tells what a file really is by its first bytes, and whether that matches its name.
fn check_type(path: &Path) -> Result<Done, String> {
    if path.is_dir() {
        return Ok(Done { out: None, message: "This is a folder.".into() });
    }
    let mut head = [0u8; 16];
    let n = {
        use std::io::Read;
        fs::File::open(path).and_then(|mut f| f.read(&mut head)).map_err(err)?
    };
    let h = &head[..n];
    let (real, exts): (&str, &[&str]) = if h.starts_with(&[0x89, b'P', b'N', b'G']) {
        ("a PNG picture", &["png"])
    } else if h.starts_with(&[0xFF, 0xD8, 0xFF]) {
        ("a JPEG picture", &["jpg", "jpeg"])
    } else if h.starts_with(b"GIF8") {
        ("a GIF picture", &["gif"])
    } else if h.len() >= 12 && &h[0..4] == b"RIFF" && &h[8..12] == b"WEBP" {
        ("a WebP picture", &["webp"])
    } else if h.starts_with(b"BM") {
        ("a BMP picture", &["bmp"])
    } else if h.starts_with(b"%PDF") {
        ("a PDF document", &["pdf"])
    } else if h.starts_with(b"PK") {
        ("a zip-based file (zip, Word, Excel, PowerPoint)", &["zip", "docx", "xlsx", "pptx", "jar", "odt", "apk"])
    } else if h.len() >= 8 && &h[4..8] == b"ftyp" {
        ("an MP4/MOV video or audio file", &["mp4", "m4v", "mov", "m4a"])
    } else if h.starts_with(b"ID3") || (h.len() > 1 && h[0] == 0xFF && (h[1] & 0xE0) == 0xE0) {
        ("an MP3 audio file", &["mp3"])
    } else if h.starts_with(b"MZ") {
        ("a Windows program", &["exe", "dll", "scr", "msi"])
    } else if h.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        ("a Matroska/WebM video", &["mkv", "webm"])
    } else if h.starts_with(b"Rar!") {
        ("a RAR archive", &["rar"])
    } else if h.starts_with(&[b'7', b'z', 0xBC, 0xAF]) {
        ("a 7-Zip archive", &["7z"])
    } else if h.iter().all(|b| *b == b'\n' || *b == b'\r' || *b == b'\t' || (0x20..0x7F).contains(b)) {
        ("a plain text file", &["txt", "md", "csv", "log", "json", "xml", "html", "htm", "svg", "css", "js"])
    } else {
        ("a file Halo doesn't recognise", &[])
    };
    let ext = ext_lower(path);
    let message = if exts.is_empty() {
        format!("It looks like {real}.")
    } else if exts.contains(&ext.as_str()) {
        format!("It is {real}, which matches its name.")
    } else {
        format!("It is really {real}, but its name says .{ext}.")
    };
    Ok(Done { out: None, message })
}

/// Sets a picture as the desktop background. A copy is kept so it still works if the original moves.
fn set_wallpaper(path: &Path) -> Result<Done, String> {
    if kind_of(path) != Kind::Image {
        return Err("Only pictures can be a desktop background.".into());
    }
    let img = load(path)?;
    let dir = crate::system::tools_dir();
    fs::create_dir_all(&dir).map_err(err)?;
    let copy = dir.join("wallpaper.png");
    fs::write(&copy, png_bytes(&img)?).map_err(err)?;
    powershell(
        "Add-Type -TypeDefinition 'using System.Runtime.InteropServices; public class HW { [DllImport(\"user32.dll\", CharSet = CharSet.Unicode)] public static extern int SystemParametersInfo(int a, int b, string c, int d); }'; [void][HW]::SystemParametersInfo(20, 0, $env:HALO_SRC, 3)",
        &[("HALO_SRC", copy.to_string_lossy().to_string())],
    )?;
    Ok(Done { out: None, message: "Desktop background changed.".into() })
}

fn show_new_window(path: &Path) -> Result<Done, String> {
    Command::new("explorer.exe").raw_arg(format!("/n,/select,\"{}\"", path.display())).creation_flags(NO_WINDOW).spawn().map_err(err)?;
    Ok(Done { out: None, message: String::new() })
}

fn duplicate(path: &Path) -> Result<Done, String> {
    let (stem, dir) = stem_dir(path);
    let ext = if path.is_dir() { None } else { path.extension().map(|e| e.to_string_lossy().to_string()) };
    let mut n = 0;
    let target = loop {
        let suffix = if n == 0 { " - Copy".to_string() } else { format!(" - Copy ({n})") };
        let name = match &ext {
            Some(e) => format!("{stem}{suffix}.{e}"),
            None => format!("{stem}{suffix}"),
        };
        let candidate = dir.join(name);
        if !candidate.exists() {
            break candidate;
        }
        n += 1;
    };
    if path.is_dir() {
        copy_dir(path, &target)?;
    } else {
        fs::copy(path, &target).map_err(err)?;
    }
    let name = target.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    Ok(Done { out: Some(target), message: format!("Duplicated as {name}") })
}

fn extract(path: &Path) -> Result<Done, String> {
    if ext_lower(path) != "zip" {
        return Err("Only .zip files can be extracted here.".into());
    }
    let (stem, dir) = stem_dir(path);
    let mut n = 0;
    let dest = loop {
        let candidate = dir.join(if n == 0 { stem.clone() } else { format!("{stem} ({n})") });
        if !candidate.exists() {
            break candidate;
        }
        n += 1;
    };
    powershell(
        "Expand-Archive -LiteralPath $env:HALO_SRC -DestinationPath $env:HALO_DST -Force",
        &[("HALO_SRC", path.to_string_lossy().to_string()), ("HALO_DST", dest.to_string_lossy().to_string())],
    )?;
    let name = dest.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    Ok(Done { out: Some(dest), message: format!("Extracted to the folder {name}") })
}

fn open_with(path: &Path) -> Result<Done, String> {
    Command::new("rundll32.exe")
        .arg("shell32.dll,OpenAs_RunDLL")
        .arg(path)
        .spawn()
        .map_err(err)?;
    Ok(Done { out: None, message: String::new() })
}

/// Shows the standard Windows Properties window for the file.
fn properties(path: &Path) -> Result<Done, String> {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
    use windows::Win32::UI::Shell::{SHObjectProperties, SHOP_FILEPATH};
    let target = HSTRING::from(path.as_os_str().to_string_lossy().as_ref());
    // The window lives on a thread of this process, so keep that thread around for a while.
    std::thread::spawn(move || unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let _ = SHObjectProperties(None, SHOP_FILEPATH, &target, PCWSTR::null());
        std::thread::sleep(std::time::Duration::from_secs(1800));
    });
    Ok(Done { out: None, message: String::new() })
}

pub fn pick_folder() -> Option<PathBuf> {
    let out = powershell(
        "$o = (New-Object -ComObject Shell.Application).BrowseForFolder(0, 'Move to...', 0x41, 0); if ($o) { $o.Self.Path }",
        &[],
    )
    .ok()?;
    if out.is_empty() { None } else { Some(PathBuf::from(out)) }
}

// ---------------------------------------------------------------- entry points

/// Packs a whole selection into one archive next to the files, named after their folder.
fn zip_all(req: &Req, ctx: &Ctx) -> Outcome {
    let first = Path::new(&req.files[0]);
    let (_, dir) = stem_dir(first);
    let stem = dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "Archive".into());
    let out = unique(&dir, &stem, "zip");
    let before: u64 = req.files.iter().map(|f| size_of(Path::new(f))).sum();
    let name = format!("{} files", req.files.len());
    (ctx.report)(0.2, 0);
    let result = powershell(
        "Compress-Archive -LiteralPath ($env:HALO_SRC -split \"`n\") -DestinationPath $env:HALO_DST -CompressionLevel Optimal -Force",
        &[("HALO_SRC", req.files.join("\n")), ("HALO_DST", out.to_string_lossy().to_string())],
    );
    match result {
        Ok(_) => {
            (ctx.report)(1.0, size_of(&out));
            Outcome { name, ok: true, message: String::new(), after: size_of(&out), out_path: Some(out.to_string_lossy().to_string()), before }
        }
        Err(message) => Outcome { name, ok: false, message, out_path: None, before, after: before },
    }
}

/// "Let me choose the location each time": asks for a folder once and moves the new files there.
/// Only files Halo just created beside the original are moved; originals are never touched.
pub fn relocate(outcomes: &mut [Outcome]) {
    let wants_move = outcomes.iter().any(|o| o.ok && o.out_path.is_some());
    if !wants_move {
        return;
    }
    let Some(folder) = pick_folder() else { return };
    for o in outcomes.iter_mut().filter(|o| o.ok) {
        let Some(current) = o.out_path.clone() else { continue };
        let from = PathBuf::from(&current);
        let Some(name) = from.file_name().map(|n| n.to_string_lossy().to_string()) else { continue };
        let (stem, ext) = match name.rsplit_once('.') {
            Some((s, e)) if from.is_file() => (s.to_string(), e.to_string()),
            _ => (name.clone(), String::new()),
        };
        let target = if ext.is_empty() { folder.join(&name) } else { unique(&folder, &stem, &ext) };
        if target.exists() {
            continue;
        }
        if fs::rename(&from, &target).is_err() && from.is_file() && fs::copy(&from, &target).is_ok() {
            let _ = fs::remove_file(&from);
        }
        if target.exists() {
            o.out_path = Some(target.to_string_lossy().to_string());
        }
    }
}

pub fn run(req: &Req, ctx: &Ctx) -> Vec<Outcome> {
    let mut resolved;
    let req = if req.verb == "preset" {
        match crate::presets::list().and_then(|list|list.into_iter().find(|p|p.id==req.option).ok_or("This conversion preset no longer exists.".into())) {
            Ok(preset)=>{resolved=req.clone();resolved.verb="convert".into();resolved.option=preset.target.clone();resolved.preset=Some(preset);&resolved},
            Err(message)=>return vec![Outcome{name:"Preset".into(),ok:false,message,out_path:None,before:0,after:0}],
        }
    } else {req};
    if let Some(preset)=&req.preset {
        if let Err(message)=crate::presets::validate(preset) {return vec![Outcome{name:"Preset".into(),ok:false,message,out_path:None,before:0,after:0}];}
    }
    if let Some(done) = crate::folders::run(req, ctx) {
        return done;
    }
    if req.verb == "compress" && req.option == "zip_all" && req.files.len() > 1 {
        return vec![zip_all(req, ctx)];
    }
    let n = req.files.len().max(1) as f64;
    let dest = if req.option == "move_to" { pick_folder() } else { None };
    let mut outcomes = Vec::new();
    for (i, file) in req.files.iter().enumerate() {
        let path = Path::new(file);
        let before = size_of(path);
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let base = i as f64 / n;
        let rep = |p: f64, bytes: u64| (ctx.report)(base + p / n, bytes);

        let result = if ctx.cancel.load(Ordering::SeqCst) {
            Err("Cancelled".to_string())
        } else {
            run_one(path, req, ctx, &rep, dest.as_deref())
        };
        let result = result.and_then(|mut done| {
            if let (Some(preset),Some(produced))=(&req.preset,&done.out) {
                let stem=path.file_stem().unwrap_or_default().to_string_lossy();
                let name=preset.template.replace("{name}",&stem).replace("{format}",&req.option).replace("{index}",&(i+1).to_string());
                let name=name.trim().trim_end_matches('.');
                if name.is_empty() {return Err("The filename template produced an empty name.".into());}
                let target=unique(produced.parent().unwrap_or(Path::new(".")),name,&ext_lower(produced));
                if preset.template!="{name}" && target!=*produced && name!=produced.file_stem().unwrap_or_default().to_string_lossy() {
                    fs::rename(produced,&target).map_err(err)?;done.out=Some(target);
                }
            }
            Ok(done)
        });
        outcomes.push(match result {
            Ok(done) => Outcome {
                name,
                ok: true,
                message: done.message,
                after: done.out.as_deref().map(size_of).unwrap_or(before),
                out_path: done.out.map(|p| p.to_string_lossy().to_string()),
                before,
            },
            Err(message) => Outcome { name, ok: false, message, out_path: None, before, after: before },
        });
    }
    if req.preset.as_ref().is_some_and(|p|p.copy_result) {
        let paths:Vec<String>=outcomes.iter().filter(|o|o.ok).filter_map(|o|o.out_path.clone()).collect();
        if !paths.is_empty() { if let Err(message)=copy_files(&paths) {
            for o in outcomes.iter_mut().filter(|o|o.ok) {o.message=format!("Saved successfully. Clipboard: {message}");}
        }}
    }
    outcomes
}

pub(crate) fn run_one(path: &Path, req: &Req, ctx: &Ctx, rep: &dyn Fn(f64, u64), dest: Option<&Path>) -> Result<Done, String> {
    if req.verb == "compress" && matches!(req.option.as_str(), "zip" | "zip_all") {
        return zip_it(path, rep);
    }
    if crate::capabilities::external_image(path) && crate::capabilities::image_engine_path().is_none()
        && crate::capabilities::format_supports(path,&req.verb,&req.option) && !crate::capabilities::supports(path,&req.verb,&req.option) {
        return Err("Install the optional extended image engine in Settings → System to use this action.".into());
    }
    let same_format_preset=req.preset.is_some() && req.verb=="convert" && crate::capabilities::normalized(&ext_lower(path))==req.option
        && matches!(kind_of(path),Kind::Image|Kind::Video|Kind::Audio) && ext_lower(path)!="gif";
    if !same_format_preset && !crate::capabilities::supports(path, &req.verb, &req.option) {
        return Err("This action isn't available for this file type.".into());
    }
    let kind = kind_of(path);
    if req.preset.is_some() && !matches!(kind,Kind::Image|Kind::Video|Kind::Audio) {return Err("Conversion presets currently apply to images, videos and audio.".into());}
    match req.verb.as_str() {
        "compress" => match kind {
            Kind::Image => image_compress(path, req, rep),
            Kind::Video => video_compress(path, req, ctx, rep),
            Kind::Audio => audio_compress(path, req, ctx, rep),
            Kind::Archive => Err("A zip file is already compressed.".into()),
            _ => zip_it(path, rep),
        },
        "convert" => match kind {
            Kind::Image if ext_lower(path)=="gif" && ["mp4","webm","mkv","mov","avi","ogv"].contains(&req.option.as_str()) => video_convert(path,req,ctx,rep),
            Kind::Image => image_convert(path, req, rep),
            Kind::Video | Kind::Audio => video_convert(path, req, ctx, rep),
            Kind::Pdf | Kind::Doc | Kind::Svg => crate::convert::convert(path, req, rep),
            _ => Err("Convert works on pictures, videos, audio, PDFs and documents.".into()),
        },
        "resize" => match kind {
            Kind::Image => image_resize(path, req, rep),
            Kind::Video => video_resize(path, req, ctx, rep),
            _ => Err("Resize works on images and videos.".into()),
        },
        // Edit, Files and More share one list of single-file actions.
        "edit" | "files" | "more" => match req.option.as_str() {
            "extract_images" => crate::convert::extract_images(path, rep),
            "rotate_left" => image_rotate(path, true),
            "rotate_right" => image_rotate(path, false),
            "flip_h" => image_flip(path, true),
            "flip_v" => image_flip(path, false),
            "strip_meta" => strip_meta(path, ctx, rep),
            "duplicate" => duplicate(path),
            "rename" => rename(path, req.text.as_deref().unwrap_or("")),
            "delete" => recycle(path),
            "move_to" => match dest {
                Some(d) => move_to(path, d),
                None => Err("Cancelled".into()),
            },
            "extract" => extract(path),
            "zip" => zip_it(path, rep),
            "copy_base64" => copy_base64(path),
            "copy_ocr" => copy_ocr(path),
            "check_type" => check_type(path),
            "set_wallpaper" => set_wallpaper(path),
            "show_new_window" => show_new_window(path),
            "open_with" => open_with(path),
            "properties" => properties(path),
            _ => Err("Unknown action.".into()),
        },
        _ => Err("Unknown action.".into()),
    }
}

const BLOCKED: &[&str] = &["exe", "bat", "cmd", "com", "msi", "ps1", "vbs", "vbe", "js", "jse", "jar", "scr", "wsf", "reg", "lnk", "hta"];

pub fn open_path(path: &str) -> Result<(), String> {
    let p = Path::new(path);
    if BLOCKED.contains(&ext_lower(p).as_str()) {
        return Err("Programs can't be opened from here.".into());
    }
    Command::new("explorer.exe").arg(p).creation_flags(NO_WINDOW).spawn().map_err(err)?;
    Ok(())
}

pub fn reveal(path: &str) -> Result<(), String> {
    Command::new("explorer.exe")
        .raw_arg(format!("/select,\"{path}\""))
        .creation_flags(NO_WINDOW)
        .spawn()
        .map_err(err)?;
    Ok(())
}

pub fn copy_files(paths: &[String]) -> Result<(), String> {
    powershell(
        "Set-Clipboard -LiteralPath ($env:HALO_PATHS -split \"`n\")",
        &[("HALO_PATHS", paths.join("\n"))],
    )
    .map(|_| ())
}

pub fn copy_text(text: &str) -> Result<(), String> {
    powershell("Set-Clipboard -Value $env:HALO_TEXT", &[("HALO_TEXT", text.to_string())]).map(|_| ())
}

pub fn thumbnail(path: &str) -> Option<Thumb> {
    let p = Path::new(path);
    let b64 = base64::engine::general_purpose::STANDARD;
    match kind_of(p) {
        Kind::Image => {
            let img = load(p).ok()?;
            let (w, h) = (img.width(), img.height());
            let bytes = jpeg_bytes(&img.thumbnail(280, 220), 82).ok()?;
            Some(Thumb { data: format!("data:image/jpeg;base64,{}", b64.encode(bytes)), w, h })
        }
        Kind::Video => {
            let out = Command::new(crate::system::ffmpeg_path())
                .args(["-hide_banner", "-loglevel", "error", "-ss", "1", "-i"])
                .arg(p)
                .args(["-frames:v", "1", "-vf", "scale=280:-2", "-f", "mjpeg", "pipe:1"])
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .creation_flags(NO_WINDOW)
                .output()
                .ok()?;
            if out.stdout.is_empty() {
                return None;
            }
            Some(Thumb { data: format!("data:image/jpeg;base64,{}", b64.encode(out.stdout)), w: 0, h: 0 })
        }
        _ => None,
    }
}

#[cfg(test)]
mod output_tests {
    use super::*;

    fn target(name: &str) -> PathBuf {
        let root=std::env::temp_dir().join(format!("halo-output-regression-{}",std::process::id()));
        fs::create_dir_all(&root).unwrap();
        root.join(name)
    }

    #[test]
    fn rgb_icons_can_be_reopened() {
        let image=DynamicImage::ImageRgb8(image::RgbImage::from_pixel(320,200,image::Rgb([40,100,160])));
        let path=target("rgb.ico");
        save(&image,Fmt::Ico,90,&path).unwrap();
        let decoded=load(&path).expect("an icon produced by Halo must be readable by Halo");
        assert_eq!(decoded.width(),256);
        assert!(decoded.height()<=256);
        assert_eq!(decoded.color(),image::ColorType::Rgba8);
    }

    #[test]
    fn high_bit_depth_images_can_be_saved_as_gif() {
        let image=DynamicImage::ImageRgb16(image::ImageBuffer::from_pixel(24,16,image::Rgb([500u16,20000,60000])));
        let path=target("rgb16.gif");
        save(&image,Fmt::Gif,90,&path).unwrap();
        let decoded=load(&path).unwrap();
        assert_eq!((decoded.width(),decoded.height()),(24,16));
    }
}

pub(crate) fn recycle_recording(path:&Path)->Result<(),String>{recycle(path).map(|_|())}
