//! Slicers: finding the ones installed and opening a model in the chosen one. Slicing and
//! sending to the printer stay in the slicer for now.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Slicer {
    pub name: String,
    /// The program, or on macOS the `.app` bundle.
    pub path: PathBuf,
}

/// Known slicers: name, program names on Linux (PATH) and Flatpak ids, Windows paths under
/// Program Files, macOS app bundles.
type Known = (
    &'static str,
    &'static [&'static str],
    &'static [&'static str],
    &'static [&'static str],
    &'static [&'static str],
);
const KNOWN: &[Known] = &[
    (
        "PrusaSlicer",
        &["prusa-slicer", "PrusaSlicer"],
        &["com.prusa3d.PrusaSlicer"],
        &["Prusa3D\\PrusaSlicer\\prusa-slicer.exe"],
        &["PrusaSlicer.app", "Original Prusa Drivers/PrusaSlicer.app"],
    ),
    (
        "OrcaSlicer",
        &["orca-slicer", "OrcaSlicer"],
        &[
            "io.github.softfever.OrcaSlicer",
            "com.orcaslicer.OrcaSlicer",
        ],
        &["OrcaSlicer\\orca-slicer.exe"],
        &["OrcaSlicer.app"],
    ),
    (
        "Bambu Studio",
        &["bambu-studio", "BambuStudio"],
        &["com.bambulab.BambuStudio"],
        &["Bambu Studio\\bambu-studio.exe"],
        &["BambuStudio.app"],
    ),
    (
        "SuperSlicer",
        &["superslicer", "SuperSlicer"],
        &["com.superslicer.SuperSlicer"],
        &["SuperSlicer\\superslicer.exe"],
        &["SuperSlicer.app"],
    ),
    (
        "UltiMaker Cura",
        &["cura", "UltiMaker-Cura"],
        &["com.ultimaker.cura"],
        &["UltiMaker Cura\\UltiMaker-Cura.exe"],
        &["UltiMaker Cura.app"],
    ),
];

/// The slicers installed on this computer.
pub fn detect() -> Vec<Slicer> {
    let path: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    let mut roots = Vec::new();
    if cfg!(target_os = "windows") {
        for var in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
            if let Some(dir) = std::env::var_os(var) {
                roots.push(PathBuf::from(dir));
            }
        }
    } else if cfg!(target_os = "macos") {
        roots.push(PathBuf::from("/Applications"));
        if let Some(home) = std::env::var_os("HOME") {
            roots.push(PathBuf::from(home).join("Applications"));
        }
    } else {
        roots.push(PathBuf::from("/var/lib/flatpak/exports/bin"));
        if let Some(home) = std::env::var_os("HOME") {
            roots.push(PathBuf::from(home).join(".local/share/flatpak/exports/bin"));
        }
    }
    detect_in(&path, &roots)
}

/// [`detect`] over the given PATH folders and install roots.
pub fn detect_in(path: &[PathBuf], roots: &[PathBuf]) -> Vec<Slicer> {
    let exe = if cfg!(target_os = "windows") {
        ".exe"
    } else {
        ""
    };
    KNOWN
        .iter()
        .filter_map(|(name, programs, flatpaks, windows, mac)| {
            let on_path = programs
                .iter()
                .flat_map(|p| path.iter().map(move |d| d.join(format!("{p}{exe}"))));
            let installed: Vec<PathBuf> = if cfg!(target_os = "windows") {
                windows
                    .iter()
                    .flat_map(|w| roots.iter().map(move |r| r.join(w)))
                    .collect()
            } else if cfg!(target_os = "macos") {
                mac.iter()
                    .flat_map(|m| roots.iter().map(move |r| r.join(m)))
                    .collect()
            } else {
                flatpaks
                    .iter()
                    .flat_map(|f| roots.iter().map(move |r| r.join(f)))
                    .collect()
            };
            on_path
                .chain(installed)
                .find(|p| p.exists())
                .map(|path| Slicer {
                    name: (*name).to_owned(),
                    path,
                })
        })
        .collect()
}

/// A slicer picked by hand, named after its file.
pub fn from_path(path: &Path) -> Slicer {
    let known = KNOWN.iter().find(|(name, programs, ..)| {
        let file = path.file_stem().map(|s| s.to_string_lossy().to_lowercase());
        let file = file.as_deref().unwrap_or("");
        programs.iter().any(|p| p.to_lowercase() == file)
            || name.to_lowercase().replace(' ', "") == file.replace([' ', '-'], "")
    });
    Slicer {
        name: known.map_or_else(
            || {
                path.file_stem()
                    .map_or_else(String::new, |s| s.to_string_lossy().into_owned())
            },
            |k| k.0.to_owned(),
        ),
        path: path.to_path_buf(),
    }
}

/// Opens `model` in the slicer without waiting for it.
pub fn open(slicer: &Path, model: &Path) -> Result<()> {
    let mut cmd = if slicer.extension().is_some_and(|e| e == "app") {
        let mut c = Command::new("open");
        c.arg("-a").arg(slicer);
        c
    } else {
        Command::new(slicer)
    };
    let mut child = cmd
        .arg(model)
        .spawn()
        .with_context(|| format!("could not start {}", slicer.display()))?;
    // Reaped when it closes, so it does not linger as a zombie.
    std::thread::spawn(move || child.wait());
    Ok(())
}
