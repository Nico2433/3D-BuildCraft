//! Printing the scene on paper: the system's printers, a page picture of the model and
//! the print command (CUPS `lp` on Linux and macOS, Paint on Windows).

use crate::host::host_command;
use anyhow::{bail, Context, Result};
use image::RgbaImage;
use std::path::Path;
use std::process::Command;

/// An A4 page at 150 dpi, portrait.
pub const PAGE: (u32, u32) = (1240, 1754);
/// Side of the square model picture centred on the page.
pub const MODEL: u32 = 1000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColorMode {
    Auto,
    Color,
    Gray,
}

impl ColorMode {
    pub const ALL: [ColorMode; 3] = [ColorMode::Auto, ColorMode::Color, ColorMode::Gray];
    pub fn label(self) -> &'static str {
        match self {
            ColorMode::Auto => "Automatic",
            ColorMode::Color => "Color",
            ColorMode::Gray => "Grayscale",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Job {
    pub printer: String,
    pub copies: u32,
    pub landscape: bool,
    pub color: ColorMode,
    /// Title the print queue shows.
    pub title: String,
}

/// The printers and the default one, if any.
pub fn printers() -> Result<(Vec<String>, Option<String>)> {
    let (list, default) = if cfg!(windows) {
        (
            powershell("Get-Printer | ForEach-Object Name")?,
            powershell("(Get-CimInstance Win32_Printer -Filter 'Default=true').Name")?,
        )
    } else {
        (
            output(host_command("lpstat").arg("-e"))?,
            output(host_command("lpstat").arg("-d"))?,
        )
    };
    let names = list
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect();
    // lpstat says "system default destination: NAME" in the user's language, or no colon
    // when there is none; PowerShell gives the bare name.
    let default = if cfg!(windows) {
        Some(default.trim())
    } else {
        default.split_once(": ").map(|(_, n)| n.trim())
    };
    Ok((names, default.filter(|n| !n.is_empty()).map(String::from)))
}

/// The page: the model picture (`MODEL`² RGBA, see [`crate::thumbnail::render_parts`])
/// centred on white, in grey for [`ColorMode::Gray`].
pub fn page(model: &[u8], landscape: bool, gray: bool) -> RgbaImage {
    let (w, h) = if landscape { (PAGE.1, PAGE.0) } else { PAGE };
    let mut page = RgbaImage::from_pixel(w, h, image::Rgba([255; 4]));
    let (x0, y0) = ((w - MODEL) / 2, (h - MODEL) / 2);
    for (i, px) in model.chunks_exact(4).enumerate() {
        let a = px[3] as u32;
        let mut c = [0, 1, 2].map(|k| ((px[k] as u32 * a + 255 * (255 - a)) / 255) as u8);
        if gray {
            let l = (c[0] as u32 * 299 + c[1] as u32 * 587 + c[2] as u32 * 114) / 1000;
            c = [l as u8; 3];
        }
        let (x, y) = (i as u32 % MODEL, i as u32 / MODEL);
        page.put_pixel(x0 + x, y0 + y, image::Rgba([c[0], c[1], c[2], 255]));
    }
    page
}

/// Sends the picture at `path` to the printer; returns once the system took the job.
// shortcut: Windows prints through Paint (`mspaint /pt`, unverified), one run per copy and
// without the orientation or colour mode; upgrade to the Win32 print API if it falls short.
pub fn print(path: &Path, job: &Job) -> Result<()> {
    if cfg!(windows) {
        for _ in 0..job.copies.max(1) {
            run(Command::new("mspaint")
                .arg("/pt")
                .arg(path)
                .arg(&job.printer))?;
        }
        return Ok(());
    }
    run(host_command("lp").args(lp_args(job)).arg(path))
}

/// Options for CUPS `lp`, before the file.
fn lp_args(job: &Job) -> Vec<String> {
    let mut args = vec![
        "-d".into(),
        job.printer.clone(),
        "-n".into(),
        job.copies.max(1).to_string(),
        "-t".into(),
        job.title.clone(),
        "-o".into(),
        "fit-to-page".into(),
        "-o".into(),
        format!(
            "orientation-requested={}",
            if job.landscape { 4 } else { 3 }
        ),
    ];
    let color: &[&str] = match job.color {
        ColorMode::Auto => &[],
        ColorMode::Color => &["print-color-mode=color"],
        ColorMode::Gray => &["print-color-mode=monochrome", "ColorModel=Gray"],
    };
    for o in color {
        args.extend(["-o".into(), o.to_string()]);
    }
    args
}

fn powershell(script: &str) -> Result<String> {
    output(Command::new("powershell").args(["-NoProfile", "-Command", script]))
}

fn output(cmd: &mut Command) -> Result<String> {
    no_window(cmd);
    let out = cmd.output().context("could not run the print system")?;
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn run(cmd: &mut Command) -> Result<()> {
    no_window(cmd);
    let out = cmd.output().context("could not run the print system")?;
    if !out.status.success() {
        bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
}

/// No console window flashes up on Windows.
fn no_window(_cmd: &mut Command) {
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(_cmd, 0x0800_0000);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lp_options_follow_the_job() {
        let mut job = Job {
            printer: "Office".into(),
            copies: 2,
            landscape: true,
            color: ColorMode::Gray,
            title: "cube".into(),
        };
        assert_eq!(
            lp_args(&job).join(" "),
            "-d Office -n 2 -t cube -o fit-to-page -o orientation-requested=4 \
             -o print-color-mode=monochrome -o ColorModel=Gray"
        );
        (job.copies, job.landscape, job.color) = (0, false, ColorMode::Auto);
        assert_eq!(
            lp_args(&job).join(" "),
            "-d Office -n 1 -t cube -o fit-to-page -o orientation-requested=3"
        );
    }

    #[test]
    fn the_model_sits_in_the_middle_of_a_white_page() {
        let model = [200, 40, 40, 255].repeat((MODEL * MODEL) as usize);
        let p = page(&model, true, true);
        assert_eq!(p.dimensions(), (PAGE.1, PAGE.0));
        assert_eq!(p.get_pixel(0, 0).0, [255; 4]);
        let mid = p.get_pixel(PAGE.1 / 2, PAGE.0 / 2).0;
        assert!(
            mid[0] == mid[1] && mid[1] == mid[2] && mid[0] < 200,
            "{mid:?}"
        );
    }
}
