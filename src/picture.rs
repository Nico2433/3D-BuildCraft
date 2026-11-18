//! The pictures the picture tools open (Load image, Emboss's image pattern, textures and
//! stickers): PNG and JPEG decoded, SVG drawn by [`crate::io::svg::rasterize`].

use anyhow::{anyhow, Result};
use image::DynamicImage;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Picture files the picture tools open.
pub const EXTENSIONS: [&str; 4] = ["png", "jpg", "jpeg", "svg"];
/// The same, for messages.
pub const FORMATS: &str = "PNG, JPEG or SVG";

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase()
}

/// True for a file the picture tools open, by its extension.
pub fn is_picture(path: &Path) -> bool {
    EXTENSIONS.contains(&extension(path).as_str())
}

/// The format's name for a picture file the picture tools do not take (GIF, BMP,
/// WebP, TIFF), by its extension, so the error can say to convert it.
pub fn unsupported(path: &Path) -> Option<&'static str> {
    match extension(path).as_str() {
        "gif" => Some("GIF"),
        "bmp" => Some("BMP"),
        "webp" => Some("WebP"),
        "tif" | "tiff" => Some("TIFF"),
        _ => None,
    }
}

/// True for an SVG drawing, which is also a model format.
pub fn is_svg(path: &Path) -> bool {
    extension(path) == "svg"
}

/// The picture in `path`. An SVG is drawn on a transparent canvas; the last one drawn is
/// kept, as previews open the same file again on every change.
pub fn open(path: &Path) -> Result<DynamicImage> {
    let failed = |e: &dyn std::fmt::Display| {
        anyhow!(
            "Could not open the picture {}: {e}. Pictures can be {FORMATS}.",
            path.display()
        )
    };
    if !is_svg(path) {
        return image::open(path).map_err(|e| failed(&e));
    }
    type Cached = (PathBuf, Option<SystemTime>, DynamicImage);
    static LAST: std::sync::Mutex<Option<Cached>> = std::sync::Mutex::new(None);
    let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok();
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((p, m, img)) = &*last {
        if p == path && *m == modified {
            return Ok(img.clone());
        }
    }
    let img = DynamicImage::ImageRgba8(
        crate::io::svg::rasterize_file(path).map_err(|e| failed(&format!("{e:#}")))?,
    );
    *last = Some((path.to_path_buf(), modified, img.clone()));
    Ok(img)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn svg_and_raster_pictures_open_and_others_say_what_works() {
        let dir = tempfile::tempdir().unwrap();
        let svg = dir.path().join("Logo.SVG");
        std::fs::write(
            &svg,
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><rect width="10" height="10"/></svg>"#,
        )
        .unwrap();
        assert!(is_picture(&svg) && is_svg(&svg));
        let img = open(&svg).unwrap().to_rgba8();
        assert_eq!(img.dimensions(), (1024, 512));
        assert_eq!(img.get_pixel(100, 100).0, [0, 0, 0, 255]);
        assert_eq!(img.get_pixel(900, 100)[3], 0);
        let png = dir.path().join("a.png");
        image::RgbaImage::new(3, 2).save(&png).unwrap();
        assert_eq!(open(&png).unwrap().width(), 3);
        let bad = dir.path().join("a.gif");
        std::fs::write(&bad, b"GIF89a").unwrap();
        assert!(!is_picture(&bad));
        let e = open(&bad).unwrap_err().to_string();
        assert!(e.contains("PNG, JPEG or SVG"), "{e}");
    }
}
