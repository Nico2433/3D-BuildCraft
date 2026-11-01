//! Finding the installed slicers, opening a model in one, and where files sent to it go.

use crate::host::{host_command, in_flatpak};
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
pub(super) const KNOWN: &[Known] = &[
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
    // The Flatpak exports are hidden in our own sandbox: list them on the host.
    let host: Vec<PathBuf> = if in_flatpak() && !roots.is_empty() {
        host_command("find")
            .args(&roots)
            .args(["-maxdepth", "1"])
            .output()
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .map(PathBuf::from)
                    .collect()
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    detect_where(&path, &roots, &|p| {
        p.exists() || host.iter().any(|h| h == p)
    })
}

/// [`detect`] over the given PATH folders and install roots.
pub fn detect_in(path: &[PathBuf], roots: &[PathBuf]) -> Vec<Slicer> {
    detect_where(path, roots, &|p| p.exists())
}

fn detect_where(
    path: &[PathBuf],
    roots: &[PathBuf],
    exists: &dyn Fn(&Path) -> bool,
) -> Vec<Slicer> {
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
                .find(|p| exists(p))
                .map(|path| Slicer {
                    name: (*name).to_owned(),
                    path,
                })
        })
        .collect()
}

/// A slicer picked by hand, named after its file (or its Flatpak export).
pub fn from_path(path: &Path) -> Slicer {
    let known = KNOWN.iter().find(|(name, programs, flatpaks, ..)| {
        let file = path.file_stem().map(|s| s.to_string_lossy().to_lowercase());
        let file = file.as_deref().unwrap_or("");
        flatpaks
            .iter()
            .any(|f| path.file_name() == Some(f.as_ref()))
            || programs.iter().any(|p| p.to_lowercase() == file)
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

/// Opens `model` in the slicer without waiting for it. Put `model` in [`print_dir`] so a
/// sandboxed slicer can read it.
pub fn open(slicer: &Path, model: &Path) -> Result<()> {
    let mut cmd = if slicer.extension().is_some_and(|e| e == "app") {
        let mut c = Command::new("open");
        c.arg("-a").arg(slicer);
        c
    } else {
        host_command(slicer)
    };
    let mut child = cmd
        .arg(model)
        .spawn()
        .with_context(|| format!("could not start {}", slicer.display()))?;
    // Reaped when it closes, so it does not linger as a zombie.
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// A folder for files sent to the slicer that a Flatpak slicer can read too. Inside our
/// Flatpak the app cache is under `~/.var/app`, which other Flatpaks cannot see.
pub fn print_dir() -> PathBuf {
    match std::env::var_os("HOME") {
        Some(home) if in_flatpak() => PathBuf::from(home).join(".cache/buildcraft3d/print"),
        _ => crate::settings::dirs()
            .map(|d| d.cache_dir().join("print"))
            .unwrap_or_else(|| std::env::temp_dir().join("buildcraft3d-print")),
    }
}
