//! Printing the scene on paper: a picture of the model, a one-page PDF with it centred on
//! A4, and the system's own print dialog for it (the XDG print portal on Linux, the
//! picture's "print" verb on Windows).

use anyhow::{Context, Result};
use image::RgbImage;
use std::path::PathBuf;

/// Side of the square model picture drawn by [`crate::thumbnail::render_parts`].
pub const MODEL: u32 = 1200;
/// An A4 page in PDF points, portrait.
pub const A4: (u32, u32) = (595, 842);
/// Blank border around the picture on the page, in points (half an inch).
const MARGIN: f32 = 36.0;
/// JPEG quality of the picture.
const QUALITY: u8 = 92;

/// How the print went.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The print dialog took the job.
    Printed,
    /// The user closed the print dialog.
    Cancelled,
    /// The print dialog is open on its own (Windows), so how it goes is not known.
    Shown,
    /// There is no print dialog: the page opened in the default viewer to print from there.
    Opened,
}

/// Saves the page for the model picture (`MODEL`² RGBA) in [`crate::slicer::print_dir`]
/// and shows the system's print dialog for it; returns once the dialog closed. `window`
/// is the app window's X11 id, for the dialog to sit on it.
pub fn print(model: &[u8], title: &str, window: Option<u64>) -> Result<Outcome> {
    let picture = picture(model, MODEL);
    let jpeg = jpeg(&picture)?;
    show_dialog(&picture, &jpeg, title, window)
}

#[cfg(not(windows))]
fn show_dialog(
    picture: &RgbImage,
    jpeg: &[u8],
    title: &str,
    window: Option<u64>,
) -> Result<Outcome> {
    let path = save("page.pdf", &pdf(jpeg, picture.width(), picture.height()))?;
    #[cfg(target_os = "linux")]
    if let Some(done) = portal::print(&path, title, window)? {
        return Ok(done);
    }
    #[cfg(not(target_os = "linux"))]
    let _ = (title, window);
    open(&path)?;
    Ok(Outcome::Opened)
}

/// Windows' own print dialog for a picture (Print Pictures), through its "print" verb.
// unverified: not built or run on Windows yet.
#[cfg(windows)]
fn show_dialog(_: &RgbImage, jpeg: &[u8], _: &str, _: Option<u64>) -> Result<Outcome> {
    use std::os::windows::process::CommandExt;
    let path = save("page.jpg", jpeg)?;
    let path = path.to_string_lossy().replace('\'', "''");
    let out = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command"])
        .arg(format!("Start-Process -FilePath '{path}' -Verb Print"))
        // No console window flashes up.
        .creation_flags(0x0800_0000)
        .output()
        .context("could not run the print system")?;
    if !out.status.success() {
        anyhow::bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(Outcome::Shown)
}

/// Writes `bytes` to the file `name` in [`crate::slicer::print_dir`], which a sandboxed
/// viewer can read too.
fn save(name: &str, bytes: &[u8]) -> Result<PathBuf> {
    let dir = crate::slicer::print_dir();
    std::fs::create_dir_all(&dir).with_context(|| format!("could not create {}", dir.display()))?;
    let path = dir.join(name);
    std::fs::write(&path, bytes).with_context(|| format!("could not write {}", path.display()))?;
    Ok(path)
}

/// Opens `path` in the default app without waiting for it.
#[cfg(not(windows))]
fn open(path: &std::path::Path) -> Result<()> {
    let program = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let mut child = crate::host::host_command(program)
        .arg(path)
        .spawn()
        .with_context(|| format!("could not start {program}"))?;
    // Reaped when it closes, so it does not linger as a zombie.
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// The model picture (`size`² RGBA with a transparent background) cut to the model and
/// laid on white.
pub fn picture(rgba: &[u8], size: u32) -> RgbImage {
    let alpha = |x: u32, y: u32| rgba[((y * size + x) * 4 + 3) as usize];
    let (mut x0, mut y0, mut x1, mut y1) = (size, size, 0, 0);
    for y in 0..size {
        for x in 0..size {
            if alpha(x, y) > 0 {
                (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1));
            }
        }
    }
    if x0 >= x1 {
        (x0, y0, x1, y1) = (0, 0, size, size);
    }
    RgbImage::from_fn(x1 - x0, y1 - y0, |x, y| {
        let i = (((y + y0) * size + x + x0) * 4) as usize;
        let a = rgba[i + 3] as u32;
        image::Rgb([0, 1, 2].map(|k| ((rgba[i + k] as u32 * a + 255 * (255 - a)) / 255) as u8))
    })
}

pub fn jpeg(picture: &RgbImage) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, QUALITY)
        .encode_image(picture)
        .context("could not encode the picture")?;
    Ok(out)
}

/// A one-page A4 PDF with the `width`×`height` JPEG centred on it, as large as fits
/// inside the margins; the page is landscape when the picture is wider than tall.
pub fn pdf(jpeg: &[u8], width: u32, height: u32) -> Vec<u8> {
    let (pw, ph) = if width > height { (A4.1, A4.0) } else { A4 };
    let scale =
        ((pw as f32 - 2.0 * MARGIN) / width as f32).min((ph as f32 - 2.0 * MARGIN) / height as f32);
    let (w, h) = (width as f32 * scale, height as f32 * scale);
    let (x, y) = ((pw as f32 - w) / 2.0, (ph as f32 - h) / 2.0);
    let content = format!("q {w:.2} 0 0 {h:.2} {x:.2} {y:.2} cm /Im0 Do Q");
    let mut image = format!(
        "<< /Type /XObject /Subtype /Image /Width {width} /Height {height} \
         /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n",
        jpeg.len()
    )
    .into_bytes();
    image.extend_from_slice(jpeg);
    image.extend_from_slice(b"\nendstream");
    let objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {pw} {ph}] \
             /Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>"
        )
        .into_bytes(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        )
        .into_bytes(),
        image,
    ];
    // The comment of high bytes tells readers the file holds binary data.
    let mut out = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    let size = objects.len() + 1;
    let mut table = format!("xref\n0 {size}\n0000000000 65535 f \n");
    for o in offsets {
        table.push_str(&format!("{o:010} 00000 n \n"));
    }
    table.push_str(&format!(
        "trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n"
    ));
    out.extend_from_slice(table.as_bytes());
    out
}

/// The XDG desktop portal's print dialog (`org.freedesktop.portal.Print`), which also
/// works inside the Flatpak sandbox.
#[cfg(target_os = "linux")]
mod portal {
    use super::Outcome;
    use anyhow::Result;
    use ashpd::desktop::print::PrintProxy;
    use ashpd::desktop::ResponseError;
    use ashpd::WindowIdentifier;
    use std::os::fd::AsFd;
    use std::path::Path;

    /// Shows the print dialog, then hands it the PDF at `path` if the user accepts.
    /// None when no portal can show the dialog.
    pub fn print(path: &Path, title: &str, window: Option<u64>) -> Result<Option<Outcome>> {
        let file = std::fs::File::open(path)?;
        let parent = window.map(WindowIdentifier::from_xid);
        pollster::block_on(async {
            let request = match PrintProxy::new().await {
                Ok(proxy) => proxy
                    .prepare_print(
                        parent.as_ref(),
                        title,
                        Default::default(),
                        Default::default(),
                        None,
                        true,
                    )
                    .await
                    .map(|r| (proxy, r)),
                Err(e) => Err(e),
            };
            let (proxy, request) = match request {
                Ok(r) => r,
                Err(e) => {
                    log::warn!("no print portal: {e}");
                    return Ok(None);
                }
            };
            let token = match request.response() {
                Ok(prepared) => prepared.token,
                Err(ashpd::Error::Response(ResponseError::Cancelled)) => {
                    return Ok(Some(Outcome::Cancelled))
                }
                Err(e) => return Err(e.into()),
            };
            proxy
                .print(parent.as_ref(), title, &file.as_fd(), Some(token), true)
                .await?
                .response()?;
            Ok(Some(Outcome::Printed))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count(hay: &[u8], needle: &str) -> usize {
        hay.windows(needle.len())
            .filter(|w| *w == needle.as_bytes())
            .count()
    }

    #[test]
    fn the_picture_is_cut_to_the_model_on_white() {
        let size = 10;
        let mut rgba = vec![0u8; (size * size * 4) as usize];
        for (x, y, a) in [(2, 3, 255), (6, 4, 128)] {
            let i = ((y * size + x) * 4) as usize;
            rgba[i..i + 4].copy_from_slice(&[0, 0, 0, a]);
        }
        let p = picture(&rgba, size);
        assert_eq!(p.dimensions(), (5, 2));
        assert_eq!(p.get_pixel(0, 0).0, [0; 3]);
        assert_eq!(p.get_pixel(1, 0).0, [255; 3]);
        assert_eq!(p.get_pixel(4, 1).0, [127; 3]);
        // Nothing drawn: the whole square.
        assert_eq!(picture(&[0; 16], 2).dimensions(), (2, 2));
    }

    #[test]
    fn the_pdf_has_one_a4_page_in_the_pictures_orientation() {
        let jpeg = jpeg(&RgbImage::from_pixel(40, 20, image::Rgb([200, 40, 40]))).unwrap();
        let wide = pdf(&jpeg, 40, 20);
        assert!(wide.starts_with(b"%PDF-1.4\n"));
        assert!(wide.ends_with(b"%%EOF\n"));
        assert_eq!(count(&wide, "/Type /Page "), 1);
        assert_eq!(count(&wide, "/Count 1"), 1);
        assert_eq!(count(&wide, "/Filter /DCTDecode"), 1);
        assert_eq!(count(&wide, "/MediaBox [0 0 842 595]"), 1);
        // As large as the margins let it be, centred.
        assert_eq!(count(&wide, "q 770.00 0 0 385.00 36.00 105.00 cm"), 1);
        assert_eq!(count(&pdf(&jpeg, 20, 40), "/MediaBox [0 0 595 842]"), 1);
    }

    #[test]
    fn the_cross_reference_table_points_at_each_object() {
        let doc = pdf(&jpeg(&RgbImage::new(8, 8)).unwrap(), 8, 8);
        let end = &doc[doc.len() - 40..];
        let end = String::from_utf8_lossy(end);
        let tail = end.rsplit("startxref\n").next().unwrap();
        let start: usize = tail.lines().next().unwrap().parse().unwrap();
        let table = std::str::from_utf8(&doc[start..]).unwrap();
        assert!(table.starts_with("xref\n0 6\n0000000000 65535 f \n"));
        let rows = table.lines().skip(3).take(5);
        for (i, row) in rows.enumerate() {
            let at: usize = row[..10].parse().unwrap();
            let obj = format!("{} 0 obj\n", i + 1);
            assert!(doc[at..].starts_with(obj.as_bytes()), "{row}");
        }
    }
}
