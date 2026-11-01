//! Slicers: finding the ones installed, reading their filaments and opening a model in the
//! chosen one. Slicing and sending to the printer stay in the slicer for now.

use crate::host::{host_command, in_flatpak};
use anyhow::{Context, Result};
use serde_json::Value;
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

#[derive(Clone, Debug, PartialEq)]
pub struct Filament {
    /// Preset name as the slicer shows it, e.g. "3NMAX PLA+".
    pub name: String,
    /// RGBA, alpha 255.
    pub color: [u8; 4],
    /// Material type when known, e.g. "PLA", "PETG"; empty when unknown.
    pub kind: String,
}

/// The filament slots of the slicer's current printer, in slot order. Empty when unknown.
/// Read from OrcaSlicer's or Bambu Studio's configuration; other slicers give none.
pub fn filaments(slicer: &Slicer) -> Vec<Filament> {
    config_dir(slicer).map_or_else(Vec::new, |(dir, app)| read_filaments(&dir, app))
}

/// The configuration folder of OrcaSlicer or Bambu Studio and its app name.
fn config_dir(slicer: &Slicer) -> Option<(PathBuf, &'static str)> {
    let app = match slicer.name.as_str() {
        "OrcaSlicer" => "OrcaSlicer",
        "Bambu Studio" => "BambuStudio",
        _ => return None,
    };
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let flatpak = KNOWN
        .iter()
        .flat_map(|k| k.2.iter())
        .find(|f| slicer.path.file_name() == Some(f.as_ref()));
    let base = if let Some(id) = flatpak {
        home.join(".var/app").join(id).join("config")
    } else if cfg!(target_os = "linux") {
        // Not XDG_CONFIG_HOME: our Flatpak points it into its own sandbox.
        home.join(".config")
    } else {
        directories::BaseDirs::new()?.config_dir().to_path_buf()
    };
    Some((base.join(app), app))
}

fn read_filaments(dir: &Path, app: &str) -> Vec<Filament> {
    let Some(conf) = read_json(&dir.join(format!("{app}.conf"))) else {
        return Vec::new();
    };
    let folder = conf["app"]["preset_folder"]
        .as_str()
        .filter(|f| !f.is_empty())
        .unwrap_or("default");
    let presets = conf["orca_presets"].as_array();
    // Orca keeps one entry per printer; Bambu Studio keeps the slots in `presets`.
    let slots = presets
        .and_then(|p| {
            p.iter()
                .find(|e| e["machine"] == conf["presets"]["machine"])
                .or(p.first())
        })
        .unwrap_or(&conf["presets"]);
    let colors: Vec<&str> = slots["filament_colors"]
        .as_str()
        .unwrap_or("")
        .split(',')
        .collect();
    std::iter::once("filament".to_owned())
        .chain((1..).map(|i| format!("filament_{i:02}")))
        .map_while(|key| slots[&key].as_str().map(str::to_owned))
        .enumerate()
        .map(|(i, name)| {
            let user = dir.join(format!("user/{folder}/filament/{name}.json"));
            let kind = read_json(&user)
                .and_then(|p| p["filament_type"][0].as_str().map(str::to_owned))
                .unwrap_or_else(|| kind_from_name(&name));
            Filament {
                color: colors
                    .get(i)
                    .and_then(|c| crate::io::threemf::parse_color(c))
                    .unwrap_or([255; 4]),
                name,
                kind,
            }
        })
        .collect()
}

/// Reads a JSON file, through the host when our sandbox hides it (another app's `~/.var/app`).
fn read_json(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok().or_else(|| {
        let out = in_flatpak().then(|| host_command("cat").arg(path).output().ok())??;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    })?;
    serde_json::from_str(&text).ok()
}

/// The material named in a preset name such as "Generic PETG @System"; empty when none.
fn kind_from_name(name: &str) -> String {
    const KINDS: &[&str] = &[
        "PLA", "PETG", "PET", "ABS", "ASA", "TPU", "PA", "PC", "PVA", "HIPS", "PP",
    ];
    let upper = name.to_uppercase();
    upper
        .split(|c: char| c.is_whitespace() || "@()_".contains(c))
        .find_map(|word| {
            KINDS.iter().find(|k| {
                word.strip_prefix(**k)
                    .is_some_and(|rest| !rest.starts_with(|c: char| c.is_alphanumeric()))
            })
        })
        .map_or_else(String::new, |k| (*k).to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_current_printers_filaments() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("OrcaSlicer.conf"),
            r##"{"app": {"preset_folder": "42"}, "presets": {"machine": "K1M 0.4"},
            "orca_presets": [
                {"machine": "Other", "filament": "Generic PLA", "filament_colors": "#26A69A"},
                {"machine": "K1M 0.4", "filament": "Generic ASA @Creality", "filament_01": "3NMAX PLA+",
                 "filament_02": "Mystery", "filament_colors": "#FFFFFF,#FFC0CB"}
            ]}"##,
        )
        .unwrap();
        let presets = dir.path().join("user/42/filament");
        std::fs::create_dir_all(&presets).unwrap();
        std::fs::write(
            presets.join("3NMAX PLA+.json"),
            r#"{"filament_type": ["PLA+"]}"#,
        )
        .unwrap();
        let f = read_filaments(dir.path(), "OrcaSlicer");
        let got: Vec<(&str, [u8; 4], &str)> = f
            .iter()
            .map(|f| (f.name.as_str(), f.color, f.kind.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                ("Generic ASA @Creality", [255, 255, 255, 255], "ASA"),
                ("3NMAX PLA+", [255, 192, 203, 255], "PLA+"),
                ("Mystery", [255, 255, 255, 255], ""),
            ]
        );
        assert_eq!(kind_from_name("Generic PETG-CF @System"), "PETG");
        assert_eq!(kind_from_name("PPS Pro"), "");
    }

    #[test]
    fn a_flatpak_export_reads_the_flatpak_config() {
        let orca = from_path(Path::new(
            "/var/lib/flatpak/exports/bin/com.orcaslicer.OrcaSlicer",
        ));
        assert_eq!(orca.name, "OrcaSlicer");
        let (dir, app) = config_dir(&orca).unwrap();
        assert_eq!(app, "OrcaSlicer");
        assert!(dir.ends_with(".var/app/com.orcaslicer.OrcaSlicer/config/OrcaSlicer"));
        assert!(config_dir(&from_path(Path::new("/usr/bin/prusa-slicer"))).is_none());
    }
}
