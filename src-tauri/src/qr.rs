//! QR codes: a picture of a QR code that points to a link (or holds a short text).

use std::path::{Path, PathBuf};

use qrcode::{Color, QrCode};

pub fn write(source: &Path, data: &str) -> Result<PathBuf, String> {
    let code = QrCode::new(data.as_bytes()).map_err(|_| "That is too much to fit in one QR code.".to_string())?;
    let modules = code.width();
    let border = 4usize;
    let scale = (640 / (modules + border * 2)).max(4);
    let side = (modules + border * 2) * scale;
    let mut img = image::GrayImage::from_pixel(side as u32, side as u32, image::Luma([255u8]));
    for (i, color) in code.to_colors().iter().enumerate() {
        if *color != Color::Dark {
            continue;
        }
        let (mx, my) = ((i % modules) + border, (i / modules) + border);
        for dy in 0..scale {
            for dx in 0..scale {
                img.put_pixel((mx * scale + dx) as u32, (my * scale + dy) as u32, image::Luma([0u8]));
            }
        }
    }
    let (stem, dir) = crate::process::stem_dir(source);
    let out = crate::process::unique(&dir, &format!("{stem}-qr"), "png");
    img.save(&out).map_err(|e| e.to_string())?;
    Ok(out)
}
