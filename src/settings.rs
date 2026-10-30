//! Preferences, recent files and the autosave, stored in the platform's config and data
//! folders (the original keeps the same things in a UWP settings hive and LocalState).

use anyhow::Result;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const RECENT_LIMIT: usize = 24;
pub const AUTOSAVE_INTERVAL: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Units {
    Micron,
    #[default]
    Millimeter,
    Centimeter,
    Inch,
    Foot,
    Meter,
}

impl Units {
    pub const ALL: [Units; 6] = [
        Units::Micron,
        Units::Millimeter,
        Units::Centimeter,
        Units::Inch,
        Units::Foot,
        Units::Meter,
    ];

    pub fn to_mm(self) -> f32 {
        match self {
            Units::Micron => 0.001,
            Units::Millimeter => 1.0,
            Units::Centimeter => 10.0,
            Units::Inch => 25.4,
            Units::Foot => 304.8,
            Units::Meter => 1000.0,
        }
    }

    /// Full name, for the print page's unit list.
    pub fn name(self) -> &'static str {
        match self {
            Units::Micron => "Micrometer",
            Units::Millimeter => "Millimeter",
            Units::Centimeter => "Centimeter",
            Units::Inch => "Inch",
            Units::Foot => "Foot",
            Units::Meter => "Meter",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Units::Micron => "µm",
            Units::Millimeter => "mm",
            Units::Centimeter => "cm",
            Units::Inch => "in",
            Units::Foot => "ft",
            Units::Meter => "m",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ColorMode {
    #[default]
    System,
    Light,
    Dark,
}

/// The "View" tab toggles.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RenderFlags {
    pub shading: bool,
    pub shadows: bool,
    pub colors: bool,
    pub reflections: bool,
    pub smoothing: bool,
    pub wireframe: bool,
    pub grid: bool,
    /// Grid properties: 0 default checker, 1 light, 2 dark; and the distance labels.
    pub grid_tone: u8,
    pub grid_labels: bool,
    pub xray: bool,
}

impl Default for RenderFlags {
    fn default() -> Self {
        Self {
            shading: true,
            shadows: true,
            colors: true,
            reflections: true,
            smoothing: true,
            wireframe: false,
            grid: true,
            grid_tone: 0,
            grid_labels: true,
            xray: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RecentItem {
    pub path: PathBuf,
    pub title: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub lang: Option<crate::i18n::Lang>,
    pub units: Units,
    pub color_mode: ColorMode,
    pub snap: bool,
    pub collision: bool,
    pub render: RenderFlags,
    pub skip_intro: bool,
    pub selection_pane_open: bool,
    pub recent: Vec<RecentItem>,
    /// Paint's recent colours, newest first.
    pub recent_colors: Vec<[u8; 4]>,
    /// Folders shown as local 3D libraries in the Add dialog.
    pub libraries: Vec<PathBuf>,
    /// The slicer "Print" opens the scene in.
    pub slicer: Option<PathBuf>,
    /// The AI assistant; its API key is in the system keychain, not here.
    pub ai: crate::assistant::Config,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            lang: None,
            units: Units::Millimeter,
            color_mode: ColorMode::System,
            snap: true,
            collision: false,
            render: RenderFlags::default(),
            skip_intro: false,
            selection_pane_open: true,
            recent: Vec::new(),
            recent_colors: Vec::new(),
            slicer: None,
            ai: crate::assistant::Config::default(),
            libraries: Vec::new(),
        }
    }
}

pub fn dirs() -> Option<ProjectDirs> {
    ProjectDirs::from("org", "BuildCraft", "3D BuildCraft")
}

impl Settings {
    pub fn lang(&self) -> crate::i18n::Lang {
        self.lang.unwrap_or_else(crate::i18n::Lang::system)
    }

    pub fn load() -> Self {
        dirs()
            .and_then(|d| std::fs::read_to_string(d.config_dir().join("settings.toml")).ok())
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<()> {
        if let Some(d) = dirs() {
            std::fs::create_dir_all(d.config_dir())?;
            std::fs::write(
                d.config_dir().join("settings.toml"),
                toml::to_string_pretty(self)?,
            )?;
        }
        Ok(())
    }

    pub fn add_recent(&mut self, path: &Path) {
        self.recent.retain(|r| r.path != path);
        let title = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        self.recent.insert(
            0,
            RecentItem {
                path: path.to_path_buf(),
                title,
            },
        );
        self.recent.truncate(RECENT_LIMIT);
    }
}

/// Writes the open scene to `<data>/AutoSaveFiles/{uuid}.buildcraft.3mf` while there are
/// unsaved changes, and removes the file on a clean exit, like the original (H37–H39).
pub struct Autosave {
    path: Option<PathBuf>,
    last: Instant,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Default for Autosave {
    fn default() -> Self {
        Self::new()
    }
}

impl Autosave {
    pub fn folder() -> Option<PathBuf> {
        dirs().map(|d| d.data_dir().join("AutoSaveFiles"))
    }

    pub fn new() -> Self {
        let path =
            Self::folder().map(|f| f.join(format!("{{{}}}.buildcraft.3mf", uuid::Uuid::new_v4())));
        Self {
            path,
            last: Instant::now(),
            worker: None,
        }
    }

    /// Autosaves left behind by a session that did not exit cleanly.
    pub fn leftovers() -> Vec<PathBuf> {
        let Some(folder) = Self::folder() else {
            return vec![];
        };
        let Ok(rd) = std::fs::read_dir(folder) else {
            return vec![];
        };
        let mut v: Vec<PathBuf> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "3mf"))
            .collect();
        v.sort();
        v
    }

    pub fn due(&self, dirty: bool) -> bool {
        dirty
            && self.path.is_some()
            && self.worker.as_ref().is_none_or(|w| w.is_finished())
            && self.last.elapsed() >= AUTOSAVE_INTERVAL
    }

    /// Only object headers and mesh Arcs are copied by the caller. Geometry cloning and
    /// serialization happen on the worker, with at most one save in flight.
    pub fn start(&mut self, objects: Vec<crate::scene::Object>) {
        if self.worker.as_ref().is_some_and(|w| !w.is_finished()) {
            return;
        }
        let Some(path) = self.path.clone() else {
            return;
        };
        self.last = Instant::now();
        self.worker = Some(std::thread::spawn(move || {
            let result = (|| -> Result<()> {
                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir)?;
                }
                let models: Vec<_> = objects.iter().map(crate::scene::Object::to_model).collect();
                crate::io::threemf::write(&path, &models, None)
            })();
            if let Err(e) = result {
                log::warn!("autosave failed: {e:#}");
            }
        }));
    }

    pub fn clear(&mut self) {
        // Finish before removing the file so a worker cannot recreate it after exit.
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        if let Some(p) = &self.path {
            let _ = std::fs::remove_file(p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_exit_removes_in_flight_autosave() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("recovery.3mf");
        let mut save = Autosave {
            path: Some(path.clone()),
            last: Instant::now() - AUTOSAVE_INTERVAL,
            worker: None,
        };
        assert!(save.due(true));
        let object = crate::scene::Object::new(
            "cube",
            crate::primitives::Primitive::Cube.build(),
            crate::scene::DEFAULT_COLOR,
        );
        save.start(vec![object]);
        assert!(!save.due(true));
        save.clear();
        assert!(!path.exists());
        assert!(save.worker.is_none());
    }

    #[test]
    fn recent_list_is_capped_and_deduplicated() {
        let mut s = Settings::default();
        for i in 0..30 {
            s.add_recent(Path::new(&format!("/m/{i}.stl")));
        }
        s.add_recent(Path::new("/m/5.stl"));
        assert_eq!(s.recent.len(), RECENT_LIMIT);
        assert_eq!(s.recent[0].title, "5.stl");
        assert_eq!(s.recent.iter().filter(|r| r.title == "5.stl").count(), 1);
    }

    #[test]
    fn settings_from_before_the_language_option_still_load() {
        let s: Settings = toml::from_str("units = \"Inch\"\nsnap = false\n").unwrap();
        assert_eq!((s.units, s.snap), (Units::Inch, false));
        assert_eq!(s.lang, None);
    }

    #[test]
    fn saved_language_still_loads_and_wins() {
        for (value, expected) in [("Es", crate::i18n::Lang::Es), ("En", crate::i18n::Lang::En)] {
            let s: Settings = toml::from_str(&format!("lang = {value:?}\n")).unwrap();
            assert_eq!(s.lang, Some(expected));
            assert_eq!(s.lang(), expected);
            let back: Settings = toml::from_str(&toml::to_string(&s).unwrap()).unwrap();
            assert_eq!(back.lang, Some(expected));
        }
    }

    #[test]
    fn settings_survive_toml() {
        let s = Settings {
            units: Units::Inch,
            collision: true,
            ..Default::default()
        };
        let back: Settings = toml::from_str(&toml::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
    }
}
