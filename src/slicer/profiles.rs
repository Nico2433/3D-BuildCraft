//! Reading OrcaSlicer's and Bambu Studio's configuration: printers, filaments, processes and
//! the project settings to print with.

use super::detect::{Slicer, KNOWN};
use super::host_files::{list_json, read_json};
use super::status::Printer;
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub struct Filament {
    /// Preset name as the slicer shows it, e.g. "3NMAX PLA+".
    pub name: String,
    /// RGBA, alpha 255.
    pub color: [u8; 4],
    /// Material type when known, e.g. "PLA", "PETG"; empty when unknown.
    pub kind: String,
}

/// The printer profiles the user has in the slicer, the current one first. Empty when unknown.
pub fn printers(slicer: &Slicer) -> Vec<String> {
    Config::of(slicer).map_or_else(Vec::new, |c| c.printers())
}

/// The filament slots of `printer` (None: the slicer's current printer), in slot order. Empty
/// when unknown. Read from OrcaSlicer's or Bambu Studio's configuration; other slicers give none.
pub fn filaments(slicer: &Slicer, printer: Option<&str>) -> Vec<Filament> {
    Config::of(slicer).map_or_else(Vec::new, |c| c.filaments(printer))
}

/// The print (process) profiles usable with `printer`, the one Orca would pick first. Empty when unknown.
pub fn processes(slicer: &Slicer, printer: &str) -> Vec<String> {
    Config::of(slicer).map_or_else(Vec::new, |c| c.processes(printer))
}

/// OrcaSlicer's `Metadata/project_settings.config` for printing with `printer`, `process` (None:
/// the printer's own or last-used one) and its filament slots: the presets with everything they
/// inherit, so the slicer opens the model with them. None when a preset cannot be read.
pub fn project_settings(slicer: &Slicer, printer: &str, process: Option<&str>) -> Option<String> {
    Config::of(slicer)?.project_settings(printer, process)
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

/// Preset keys that describe the preset rather than the print, and print host secrets.
const NOT_SETTINGS: &[&str] = &[
    "name",
    "inherits",
    "from",
    "type",
    "instantiation",
    "setting_id",
    "filament_id",
    "version",
    "is_custom_defined",
    "description",
    "renamed_from",
    "base_id",
    "compatible_printers",
    "compatible_printers_condition",
    "compatible_prints",
    "compatible_prints_condition",
    "print_host",
    "print_host_webui",
    "printhost_apikey",
    "printhost_cafile",
    "printhost_user",
    "printhost_password",
    "printhost_port",
];

/// The slicer's configuration file and where its presets are.
pub(super) struct Config {
    dir: PathBuf,
    conf: Value,
    /// The folder of the user's presets under `user/`.
    folder: String,
    /// Folders with the system profiles: `<vendor>.json` indexes beside `<vendor>/` presets.
    profiles: Vec<PathBuf>,
}

impl Config {
    pub(super) fn of(slicer: &Slicer) -> Option<Config> {
        let (dir, app) = config_dir(slicer)?;
        let mut profiles = vec![dir.join("system")];
        profiles.extend(resource_dirs(&slicer.path, app));
        Config::read(dir, app, profiles)
    }

    fn read(dir: PathBuf, app: &str, profiles: Vec<PathBuf>) -> Option<Config> {
        let conf = read_json(&dir.join(format!("{app}.conf")))?;
        let folder = conf["app"]["preset_folder"]
            .as_str()
            .filter(|f| !f.is_empty())
            .unwrap_or("default")
            .to_owned();
        Some(Config {
            dir,
            conf,
            folder,
            profiles,
        })
    }

    fn current(&self) -> Option<&str> {
        self.conf["presets"]["machine"].as_str()
    }

    /// The names the slicer's printer drop-down shows: the user's printers and the system
    /// printers of the models picked in its setup wizard.
    fn printers(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        let mut add = |name: &str| {
            if !name.is_empty() && !names.iter().any(|n| n == name) {
                names.push(name.to_owned());
            }
        };
        add(self.current().unwrap_or(""));
        for name in list_json(&self.dir.join(format!("user/{}/machine", self.folder))) {
            add(&name);
        }
        for model in self.conf["models"].as_array().into_iter().flatten() {
            let (Some(vendor), Some(name)) = (model["vendor"].as_str(), model["model"].as_str())
            else {
                continue;
            };
            let nozzles: Vec<&str> = model["nozzle_diameter"]
                .as_str()
                .unwrap_or("")
                .split(';')
                .collect();
            let Some((root, index)) = self.vendor(vendor) else {
                continue;
            };
            for entry in index["machine_list"].as_array().into_iter().flatten() {
                let (Some(preset), Some(sub)) =
                    (entry["name"].as_str(), entry["sub_path"].as_str())
                else {
                    continue;
                };
                if !preset.starts_with(name) {
                    continue;
                }
                let machine = read_json(&root.join(vendor).join(sub)).unwrap_or_default();
                if machine["printer_model"] == name
                    && nozzles.contains(&machine["printer_variant"].as_str().unwrap_or("-"))
                {
                    add(preset);
                }
            }
        }
        for entry in self.conf["orca_presets"].as_array().into_iter().flatten() {
            add(entry["machine"].as_str().unwrap_or(""));
        }
        names
    }

    /// What the slicer remembers for `printer` (None: the current one): its filament slots,
    /// colours and process.
    fn slots(&self, printer: Option<&str>) -> Option<&Value> {
        let current = self.current();
        let printer = printer.or(current);
        match self.conf["orca_presets"].as_array() {
            // Orca keeps one entry per printer.
            Some(p) => p
                .iter()
                .find(|e| e["machine"].as_str() == printer)
                .or_else(|| p.first().filter(|_| printer == current)),
            // Bambu Studio keeps the current printer's slots in `presets`.
            None => (printer == current).then_some(&self.conf["presets"]),
        }
    }

    /// The filament preset names of `printer`'s slots and their colours.
    fn slot_names(&self, printer: Option<&str>) -> (Vec<String>, Vec<String>) {
        let Some(slots) = self.slots(printer) else {
            // A printer never used: the slicer starts it with its default filaments.
            let names = printer
                .and_then(|p| self.preset("machine", p))
                .and_then(|(m, _)| {
                    serde_json::from_value(m["default_filament_profile"].clone()).ok()
                })
                .unwrap_or_default();
            return (names, Vec::new());
        };
        let names = std::iter::once("filament".to_owned())
            .chain((1..).map(|i| format!("filament_{i:02}")))
            .map_while(|key| slots[&key].as_str().map(str::to_owned))
            .collect();
        let colors = slots["filament_colors"]
            .as_str()
            .unwrap_or("")
            .split(',')
            .map(str::to_owned)
            .collect();
        (names, colors)
    }

    fn filaments(&self, printer: Option<&str>) -> Vec<Filament> {
        let (names, colors) = self.slot_names(printer);
        names
            .into_iter()
            .enumerate()
            .map(|(i, name)| {
                let user = self
                    .dir
                    .join(format!("user/{}/filament/{name}.json", self.folder));
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

    /// The index of a vendor's system profiles and the folder it is in.
    fn vendor(&self, vendor: &str) -> Option<(&Path, Value)> {
        self.profiles.iter().find_map(|root| {
            Some((
                root.as_path(),
                read_json(&root.join(format!("{vendor}.json")))?,
            ))
        })
    }

    /// Preset `name` of `kind` ("machine", "process" or "filament") with everything it
    /// inherits, and the keys a user preset changes (empty for a system preset).
    fn preset(&self, kind: &str, name: &str) -> Option<(Value, Vec<String>)> {
        let user = read_json(
            &self
                .dir
                .join(format!("user/{}/{kind}/{name}.json", self.folder)),
        );
        let changed = user.as_ref().map_or_else(Vec::new, |u| {
            u.as_object()
                .into_iter()
                .flatten()
                .map(|(k, _)| k.clone())
                .collect()
        });
        let preset = user.or_else(|| self.system_preset(kind, name))?;
        let settings = merged(preset, |parent| self.system_preset(kind, parent))?;
        Some((settings, changed))
    }

    /// A system preset, looked up in the indexes of the vendors the user installed.
    fn system_preset(&self, kind: &str, name: &str) -> Option<Value> {
        let list = format!("{kind}_list");
        let vendors = self.conf["models"].as_array().into_iter().flatten();
        let vendors = vendors
            .filter_map(|m| m["vendor"].as_str())
            .chain(["OrcaFilamentLibrary"]);
        vendors.into_iter().find_map(|vendor| {
            let (root, index) = self.vendor(vendor)?;
            let sub = index[&list]
                .as_array()?
                .iter()
                .find(|e| e["name"] == name)?["sub_path"]
                .as_str()?
                .to_owned();
            read_json(&root.join(vendor).join(sub))
        })
    }

    /// The process the slicer remembers for `printer`, else the printer's default one.
    fn default_process(&self, printer: &str, machine: &Value) -> Option<String> {
        self.slots(Some(printer))
            .and_then(|s| s["process"].as_str().or(s["print"].as_str()))
            .or(machine["default_print_profile"].as_str())
            .map(str::to_owned)
    }

    /// The process presets the slicer offers for `printer`: the user's, then the system ones of
    /// the installed vendors, with the one it would pick first at the front.
    fn processes(&self, printer: &str) -> Vec<String> {
        let Some((machine, _)) = self.preset("machine", printer) else {
            return Vec::new();
        };
        // A user printer is matched by the system printer it inherits.
        let ids = [Value::from(printer), machine["inherits"].clone()];
        let fits = |p: &Value| match p["compatible_printers"]
            .as_array()
            .filter(|l| !l.is_empty())
        {
            Some(list) => list.iter().any(|x| ids.contains(x)),
            // shortcut: a compatible_printers_condition is not evaluated and leaves the preset
            // out (a few vendors such as Prusa use them); evaluate it if their presets are missed.
            None => p["compatible_printers_condition"]
                .as_str()
                .unwrap_or("")
                .is_empty(),
        };
        let mut names: Vec<String> =
            list_json(&self.dir.join(format!("user/{}/process", self.folder)))
                .into_iter()
                .filter(|n| self.preset("process", n).is_some_and(|(p, _)| fits(&p)))
                .collect();
        let mut vendors: Vec<&str> = self.conf["models"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|m| m["vendor"].as_str())
            .collect();
        vendors.sort_unstable();
        vendors.dedup();
        for vendor in vendors {
            let Some((root, index)) = self.vendor(vendor) else {
                continue;
            };
            // Each file read once: a vendor has hundreds of processes sharing a few parents.
            let files: Vec<(&str, Value)> = index["process_list"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|e| {
                    let file = read_json(&root.join(vendor).join(e["sub_path"].as_str()?))?;
                    Some((e["name"].as_str()?, file))
                })
                .collect();
            for (name, file) in &files {
                let usable = file["instantiation"] != "false"
                    && merged(file.clone(), |p| {
                        files.iter().find(|(n, _)| *n == p).map(|(_, f)| f.clone())
                    })
                    .is_some_and(|p| fits(&p));
                if usable {
                    names.push((*name).to_owned());
                }
            }
        }
        if let Some(i) = self
            .default_process(printer, &machine)
            .and_then(|d| names.iter().position(|n| *n == d))
        {
            let first = names.remove(i);
            names.insert(0, first);
        }
        names
    }

    /// The printers the slicer can send to: printer presets with a print host, then the ones
    /// it found on the network (`local_machines`, keyed by IP), without repeating a host.
    pub(super) fn connected(&self) -> Vec<Printer> {
        fn bare(host: &str) -> &str {
            host.rsplit("://")
                .next()
                .unwrap_or("")
                .trim_end_matches('/')
        }
        let mut found: Vec<Printer> = Vec::new();
        let mut add = |name: &str, model: &str, host: &str| {
            let host = host.trim();
            if !host.is_empty() && !found.iter().any(|p| bare(&p.host) == bare(host)) {
                found.push(Printer {
                    name: if name.is_empty() { host } else { name }.to_owned(),
                    model: model.to_owned(),
                    host: host.to_owned(),
                });
            }
        };
        for name in list_json(&self.dir.join(format!("user/{}/machine", self.folder))) {
            if let Some((machine, _)) = self.preset("machine", &name) {
                let model = machine["printer_model"].as_str().unwrap_or("");
                add(&name, model, machine["print_host"].as_str().unwrap_or(""));
            }
        }
        let machines = self.conf["local_machines"]
            .as_object()
            .into_iter()
            .flatten();
        for (_, m) in machines {
            let text = |key: &str| m[key].as_str().unwrap_or("");
            add(text("dev_name"), text("printer_type"), text("dev_ip"));
        }
        found
    }

    fn project_settings(&self, printer: &str, process: Option<&str>) -> Option<String> {
        let (machine, machine_changed) = self.preset("machine", printer)?;
        let slots = self.slots(Some(printer));
        let process_name = process
            .map(str::to_owned)
            .or_else(|| self.default_process(printer, &machine))?;
        let (process, process_changed) = self.preset("process", &process_name)?;
        let (names, colors) = self.slot_names(Some(printer));
        let filaments = names
            .iter()
            .map(|n| self.preset("filament", n))
            .collect::<Option<Vec<_>>>()?;
        let extruder = machine["printer_extruder_variant"][0].clone();
        let inherits = |p: &Value, changed: &[String]| {
            let base = p["inherits"].as_str().filter(|_| !changed.is_empty());
            Value::from(base.unwrap_or(""))
        };
        let changed = |c: &[String]| {
            let keys: Vec<&str> = c
                .iter()
                .map(String::as_str)
                .filter(|k| !NOT_SETTINGS.contains(k))
                .collect();
            Value::from(keys.join(";"))
        };

        let mut out = Map::new();
        let object = |v: &Value| v.as_object().cloned().unwrap_or_default();
        for (key, value) in object(&process).into_iter().chain(object(&machine)) {
            if !NOT_SETTINGS.contains(&key.as_str()) {
                out.insert(key, value);
            }
        }
        // Filament settings hold one value per slot: the value for this printer's extruder
        // type when a preset lists one per type.
        let per_slot: Vec<Map<String, Value>> = filaments
            .iter()
            .map(|(f, _)| {
                let variants = f["filament_extruder_variant"].as_array();
                let pick = variants.and_then(|v| v.iter().position(|x| *x == extruder));
                let count = variants.map_or(0, Vec::len);
                object(f)
                    .into_iter()
                    .map(|(k, v)| {
                        let v = match v {
                            Value::Array(a) if count > 1 && a.len() == count => {
                                Value::Array(vec![a[pick.unwrap_or(0)].clone()])
                            }
                            Value::Array(a) => Value::Array(a),
                            other => Value::Array(vec![other]),
                        };
                        (k, v)
                    })
                    .collect()
            })
            .collect();
        if let Some(first) = per_slot.first() {
            for key in first.keys().filter(|k| !NOT_SETTINGS.contains(&k.as_str())) {
                // shortcut: a key some slot lacks is left to the slicer's default.
                let values: Option<Vec<Vec<Value>>> = per_slot
                    .iter()
                    .map(|f| f.get(key).and_then(Value::as_array).cloned())
                    .collect();
                if let Some(values) = values {
                    out.insert(key.clone(), Value::Array(values.concat()));
                }
            }
        }
        let filament_colour: Vec<Value> = per_slot
            .iter()
            .enumerate()
            .map(|(i, f)| match colors.get(i).filter(|c| !c.is_empty()) {
                Some(c) => Value::from(c.as_str()),
                None => f
                    .get("default_filament_colour")
                    .map_or("#FFFFFF".into(), |c| c[0].clone()),
            })
            .collect();
        let mut group = vec![inherits(&process, &process_changed)];
        let mut differ = vec![changed(&process_changed)];
        for (f, c) in &filaments {
            group.push(inherits(f, c));
            differ.push(changed(c));
        }
        group.push(inherits(&machine, &machine_changed));
        differ.push(changed(&machine_changed));
        for (key, value) in [
            ("printer_settings_id", Value::from(printer)),
            ("print_settings_id", Value::from(process_name)),
            ("filament_settings_id", Value::from(names)),
            ("filament_colour", Value::from(filament_colour)),
            ("inherits_group", Value::from(group)),
            ("different_settings_to_system", Value::from(differ)),
            ("from", Value::from("project")),
            ("name", Value::from("project_settings")),
        ] {
            out.insert(key.to_owned(), value);
        }
        // How much to purge between filaments, which the slicer keeps per printer.
        for key in ["flush_volumes_matrix", "flush_volumes_vector"] {
            if let Some(v) = slots.and_then(|s| s[key].as_str()) {
                out.insert(key.to_owned(), v.split('|').collect::<Vec<_>>().into());
            }
        }
        serde_json::to_string_pretty(&out).ok()
    }
}

/// `preset` over the presets it inherits, found with `parent`. None when one is missing.
fn merged(preset: Value, parent: impl Fn(&str) -> Option<Value>) -> Option<Value> {
    let mut chain = Vec::new();
    let mut next = Some(preset);
    // Inheritance is a few levels deep; the limit stops a loop.
    while let Some(preset) = next.take().filter(|_| chain.len() < 16) {
        let base = preset["inherits"]
            .as_str()
            .filter(|p| !p.is_empty())
            .map(str::to_owned);
        chain.push(preset);
        if let Some(base) = base {
            next = Some(parent(&base)?);
        }
    }
    let mut settings = Map::new();
    for preset in chain.into_iter().rev() {
        settings.extend(preset.as_object()?.clone());
    }
    (!settings.is_empty()).then_some(Value::Object(settings))
}

/// Where the slicer keeps the system profiles it ships with, beside its program.
fn resource_dirs(program: &Path, app: &str) -> Vec<PathBuf> {
    let parent = program.parent().unwrap_or(Path::new(""));
    let mut dirs = vec![
        // Windows
        parent.join("resources/profiles"),
        // macOS app bundle
        program.join("Contents/Resources/profiles"),
        // Linux: bin/ beside share/
        parent.join(format!("../share/{app}/profiles")),
    ];
    // A Flatpak export `<installation>/exports/bin/<id>` runs `<installation>/app/<id>`.
    if let (Some(id), Some(installation)) =
        (program.file_name(), parent.parent().and_then(Path::parent))
    {
        let app_dir = installation.join("app").join(id);
        let files = format!("active/files/share/{app}/profiles");
        dirs.push(app_dir.join("current").join(&files));
        // shortcut: newer branches may ship packed profiles; the stable one has JSON.
        dirs.push(
            app_dir
                .join(std::env::consts::ARCH)
                .join("stable")
                .join(files),
        );
    }
    dirs
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
    use crate::slicer::from_path;

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
        let config = Config::read(dir.path().into(), "OrcaSlicer", Vec::new()).unwrap();
        let f = config.filaments(None);
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

    /// An Orca configuration in `dir`: a user printer "K1M" on system printer "M 0.4", and
    /// the system profiles of vendor "V" in `dir/profiles`.
    fn orca_config(dir: &Path) -> Config {
        let files = [
            (
                "OrcaSlicer.conf",
                r##"{"app": {"preset_folder": "42"}, "presets": {"machine": "K1M"},
                "models": [{"vendor": "V", "model": "M", "nozzle_diameter": "0.4;0.6"}],
                "local_machines": {"192.168.1.10": {"dev_ip": "192.168.1.10", "dev_name": "K1",
                    "printer_type": "Creality K1"}, "10.0.0.2": {"dev_ip": "10.0.0.2", "dev_name": ""}},
                "orca_presets": [
                    {"machine": "K1M", "process": "Fine", "filament": "Mine", "filament_01": "PLA",
                     "filament_colors": "#FF0000,", "flush_volumes_matrix": "0|5|6|0"},
                    {"machine": "Gone", "filament": "PLA"}
                ]}"##,
            ),
            (
                "user/42/machine/K1M.json",
                r#"{"name": "K1M", "inherits": "M 0.4", "from": "User", "machine_start_gcode": "GO",
                "print_host": "192.168.1.10"}"#,
            ),
            (
                "user/42/filament/Mine.json",
                r#"{"name": "Mine", "inherits": "PLA", "filament_type": ["PLA+"],
                "filament_extruder_variant": ["Bowden", "Direct"], "nozzle_temperature": ["190", "215"]}"#,
            ),
            (
                "profiles/V.json",
                r#"{"machine_list": [
                    {"name": "common", "sub_path": "machine/common.json"},
                    {"name": "M 0.4", "sub_path": "machine/M 0.4.json"},
                    {"name": "M 0.8", "sub_path": "machine/M 0.8.json"},
                    {"name": "M Pro 0.4", "sub_path": "machine/M Pro 0.4.json"}],
                "process_list": [{"name": "Fine", "sub_path": "process/Fine.json"},
                    {"name": "base", "sub_path": "process/base.json"},
                    {"name": "Coarse", "sub_path": "process/Coarse.json"},
                    {"name": "Big", "sub_path": "process/Big.json"},
                    {"name": "Cond", "sub_path": "process/Cond.json"}],
                "filament_list": [{"name": "PLA", "sub_path": "filament/PLA.json"}]}"#,
            ),
            (
                "profiles/V/machine/common.json",
                r#"{"name": "common", "printer_extruder_variant": ["Direct"], "machine_start_gcode": "",
                "nozzle_diameter": ["0.4"]}"#,
            ),
            (
                "profiles/V/machine/M 0.4.json",
                r#"{"name": "M 0.4", "inherits": "common", "printer_model": "M", "printer_variant": "0.4"}"#,
            ),
            (
                "profiles/V/machine/M 0.8.json",
                r#"{"name": "M 0.8", "printer_model": "M", "printer_variant": "0.8"}"#,
            ),
            (
                "profiles/V/machine/M Pro 0.4.json",
                r#"{"name": "M Pro 0.4", "printer_model": "M Pro", "printer_variant": "0.4"}"#,
            ),
            (
                "profiles/V/process/Fine.json",
                r#"{"name": "Fine", "layer_height": "0.1"}"#,
            ),
            (
                "profiles/V/process/base.json",
                r#"{"name": "base", "instantiation": "false", "compatible_printers": ["M 0.4"]}"#,
            ),
            (
                "profiles/V/process/Coarse.json",
                r#"{"name": "Coarse", "inherits": "base", "instantiation": "true", "layer_height": "0.3"}"#,
            ),
            (
                "profiles/V/process/Big.json",
                r#"{"name": "Big", "compatible_printers": ["M 0.8"]}"#,
            ),
            (
                "profiles/V/process/Cond.json",
                r#"{"name": "Cond", "compatible_printers_condition": "nozzle_diameter[0]==0.4"}"#,
            ),
            (
                "user/42/process/Mine fast.json",
                r#"{"name": "Mine fast", "inherits": "Coarse"}"#,
            ),
            (
                "profiles/V/filament/PLA.json",
                r##"{"name": "PLA", "filament_type": ["PLA"], "nozzle_temperature": ["210"],
                "default_filament_colour": ["#00FF00"]}"##,
            ),
        ];
        for (path, text) in files {
            let path = dir.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        Config::read(dir.into(), "OrcaSlicer", vec![dir.join("profiles")]).unwrap()
    }

    #[test]
    fn lists_the_printers_the_slicer_shows() {
        let dir = tempfile::tempdir().unwrap();
        let config = orca_config(dir.path());
        assert_eq!(config.printers(), ["K1M", "M 0.4", "Gone"]);
        // The preset and the printer found on the network at its address are one printer.
        let printer = |name: &str, model: &str, host: &str| Printer {
            name: name.into(),
            model: model.into(),
            host: host.into(),
        };
        assert_eq!(
            config.connected(),
            [
                printer("K1M", "M", "192.168.1.10"),
                printer("10.0.0.2", "", "10.0.0.2"),
            ]
        );
        let names = |p| {
            config
                .filaments(p)
                .into_iter()
                .map(|f| f.name)
                .collect::<Vec<_>>()
        };
        assert_eq!(names(None), ["Mine", "PLA"]);
        assert_eq!(names(Some("Gone")), ["PLA"]);
        // The remembered process first, the user's before the system ones.
        assert_eq!(config.processes("K1M"), ["Fine", "Mine fast", "Coarse"]);
        assert_eq!(config.processes("M 0.8"), ["Fine", "Big"]);
        assert!(config.processes("Unknown").is_empty());
    }

    #[test]
    fn project_settings_resolve_the_chosen_presets() {
        let dir = tempfile::tempdir().unwrap();
        let config = orca_config(dir.path());
        let s: Value =
            serde_json::from_str(&config.project_settings("K1M", None).unwrap()).unwrap();
        assert_eq!(s["printer_settings_id"], "K1M");
        assert_eq!(s["print_settings_id"], "Fine");
        assert_eq!(s["layer_height"], "0.1");
        // The user printer over the system one it inherits.
        assert_eq!(s["machine_start_gcode"], "GO");
        assert_eq!(s["nozzle_diameter"], serde_json::json!(["0.4"]));
        assert!(s.get("print_host").is_none());
        // One value per slot, the one for the printer's extruder type.
        assert_eq!(
            s["filament_settings_id"],
            serde_json::json!(["Mine", "PLA"])
        );
        assert_eq!(s["filament_type"], serde_json::json!(["PLA+", "PLA"]));
        assert_eq!(s["nozzle_temperature"], serde_json::json!(["215", "210"]));
        assert_eq!(
            s["filament_colour"],
            serde_json::json!(["#FF0000", "#00FF00"])
        );
        assert_eq!(
            s["flush_volumes_matrix"],
            serde_json::json!(["0", "5", "6", "0"])
        );
        assert_eq!(
            s["inherits_group"],
            serde_json::json!(["", "PLA", "", "M 0.4"])
        );
        let differ = &s["different_settings_to_system"];
        assert_eq!(
            differ[1],
            "filament_extruder_variant;filament_type;nozzle_temperature"
        );
        assert_eq!(differ[3], "machine_start_gcode");
        assert!(config.project_settings("Unknown", None).is_none());
        let s: Value =
            serde_json::from_str(&config.project_settings("K1M", Some("Coarse")).unwrap()).unwrap();
        assert_eq!(s["print_settings_id"], "Coarse");
        assert_eq!(s["layer_height"], "0.3");
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
