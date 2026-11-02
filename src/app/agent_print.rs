//! The assistant's tools for the model's metadata, the 3D print setup, the printers' status
//! and printing: each goes through the same state and actions as the print view, the
//! "About the model" panel and "Print on paper".

use super::agent::parse_hex;
use super::*;
use crate::mcp::{self, Reply};
use serde_json::{json, Value};

/// The `model_info` argument for each [`io::ModelInfo::fields`] entry, in that order.
const INFO_KEYS: [&str; 5] = [
    "title",
    "designer",
    "copyright",
    "license_terms",
    "description",
];

/// What `print_setup` was asked to change; filament slots are counted from 0.
#[derive(Debug, Default, PartialEq)]
struct Setup {
    slicer: Option<String>,
    printer: Option<String>,
    process: Option<String>,
    filament: Option<usize>,
    filaments: Vec<(Rgba, usize)>,
}

impl App {
    /// Runs `model_info`, `print_setup`, `printer_status` and `print_on_paper`.
    pub(super) fn agent_print_tool(&mut self, tool: &str, a: &Value, ctx: &egui::Context) -> Reply {
        let answer = match tool {
            "model_info" => {
                let current = self.scene.file_info(&self.scene_title());
                if let Some(info) = info_args(a, current)? {
                    self.set_model_info(info);
                }
                info_json(&self.scene.file_info(&self.scene_title()))
            }
            "print_setup" => self.print_setup(a, ctx)?,
            "printer_status" => self.printer_status_json(),
            "print_on_paper" => {
                if self.scene.objects.is_empty() {
                    return Err("the scene is empty".into());
                }
                if self.paper_dialog_open() {
                    return Err("the print dialog is already open".into());
                }
                self.apply(Action::OpenPaper, ctx);
                json!({ "opened": "the system print dialog; the user picks the printer and confirms there" })
            }
            other => return Err(format!("unknown tool: {other}")),
        };
        Ok(vec![mcp::text(&answer.to_string())])
    }

    /// `print`: opens the scene in the chosen slicer, with the profiles and filaments read
    /// first when the print view has not read them yet.
    pub(super) fn agent_print(&mut self, ctx: &egui::Context) -> Result<(), String> {
        if self.scene.objects.is_empty() {
            return Err("the scene is empty".into());
        }
        if self.settings.slicer.is_none() {
            self.apply(Action::DetectSlicers, ctx);
        }
        if self.settings.slicer.is_none() {
            return Err("no slicer found; the user can choose one in 3D print".into());
        }
        self.read_profiles(ctx, false);
        self.apply(Action::SendToSlicer, ctx);
        Ok(())
    }

    /// Waits for the slicer's profiles: a reading in progress, or a new one when `again`
    /// or when none was read yet.
    fn read_profiles(&mut self, ctx: &egui::Context, again: bool) {
        let unread = self.printers.is_empty() && self.filaments.is_empty();
        if self.profiles_job.is_none() && (again || unread) {
            self.load_filaments(ctx);
        }
        self.take_profiles(true);
    }

    fn print_setup(&mut self, a: &Value, ctx: &egui::Context) -> Result<Value, String> {
        let setup = parse_setup(a)?;
        if self.slicers.is_empty() {
            // Picks the first one found while none is chosen, and reads its profiles.
            self.apply(Action::DetectSlicers, ctx);
        }
        if let Some(name) = &setup.slicer {
            let path = self.find_slicer(name)?;
            self.apply(Action::ChooseSlicer(path), ctx);
        }
        // Read again: the user may have changed the profiles in the slicer meanwhile.
        self.read_profiles(ctx, true);
        if let Some(name) = &setup.printer {
            let printer = pick(&self.printers, name, "printer profile")?;
            self.apply(Action::ChoosePrinter(printer), ctx);
            self.take_profiles(true);
        }
        if let Some(name) = &setup.process {
            let process = pick(&self.processes, name, "print profile")?;
            self.apply(Action::ChooseProcess(process), ctx);
        }
        let slots = self.filaments.len();
        let in_range = |slot: usize| {
            if slot < slots {
                Ok(())
            } else {
                Err(format!(
                    "filament slot {} does not exist; the printer has {slots}",
                    slot + 1
                ))
            }
        };
        let colors = self.scene_colors();
        if let Some(slot) = setup.filament {
            in_range(slot)?;
            for &c in &colors {
                self.apply(Action::ChooseFilament(c, slot), ctx);
            }
        }
        for &(color, slot) in &setup.filaments {
            in_range(slot)?;
            if !colors.contains(&color) {
                return Err(format!(
                    "{} is not a colour of the scene; its colours are {}",
                    hex(color),
                    colors
                        .iter()
                        .map(|&c| hex(c))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            self.apply(Action::ChooseFilament(color, slot), ctx);
        }
        Ok(self.print_setup_json())
    }

    /// A detected slicer by name, the chosen one, or a slicer program's path.
    fn find_slicer(&self, name: &str) -> Result<PathBuf, String> {
        let chosen = self.chosen_slicer();
        let known = self.slicers.iter().chain(chosen.as_ref());
        for s in known {
            if s.name.eq_ignore_ascii_case(name.trim()) || s.path == Path::new(name) {
                return Ok(s.path.clone());
            }
        }
        let path = PathBuf::from(name);
        if path.is_file() {
            return Ok(path);
        }
        let names: Vec<&str> = self.slicers.iter().map(|s| s.name.as_str()).collect();
        Err(format!(
            "unknown slicer: {name}; found: {}",
            if names.is_empty() {
                "none".to_owned()
            } else {
                names.join(", ")
            }
        ))
    }

    /// The print setup as the agent sees it: the choices and what can be chosen.
    fn print_setup_json(&mut self) -> Value {
        let slicer = |s: &crate::slicer::Slicer| json!({ "name": s.name, "path": s.path });
        let chosen = self.chosen_slicer();
        let mut slicers: Vec<Value> = self.slicers.iter().map(slicer).collect();
        // One picked by hand stays on the list, as on the Printer page.
        if let Some(c) = chosen.as_ref().filter(|c| !self.slicers.contains(c)) {
            slicers.push(slicer(c));
        }
        let colors: Vec<Value> = self
            .scene_colors()
            .into_iter()
            .map(|c| json!({ "color": hex(c), "slot": self.filament_slot(c) + 1 }))
            .collect();
        let filaments: Vec<Value> = self
            .filaments
            .iter()
            .enumerate()
            .map(|(i, f)| json!({ "slot": i + 1, "name": f.name, "type": f.kind, "color": hex(f.color) }))
            .collect();
        // Until one is chosen, the slicer's current printer and the printer's usual
        // process are used, which the lists put first.
        let current = |chosen: &Option<String>, list: &[String]| {
            chosen.clone().or_else(|| list.first().cloned())
        };
        json!({
            "slicer": chosen.as_ref().map(slicer),
            "slicers": slicers,
            "printer": current(&self.settings.printer, &self.printers),
            "printers": self.printers,
            "process": current(&self.settings.process, &self.processes),
            "processes": self.processes,
            "filaments": filaments,
            "colors": colors,
        })
    }

    /// The printers connected in the slicer and their status. Their addresses and access
    /// codes stay out: the agent needs neither.
    fn printer_status_json(&self) -> Value {
        let read = print::read_status(print::connected_printers(self.chosen_slicer()));
        let printers: Vec<Value> = read
            .iter()
            .enumerate()
            .map(|(i, (p, s))| status_json(i, p, s))
            .collect();
        json!({ "printers": printers })
    }
}

/// The metadata `a` sets on top of `info`; None when it sets nothing.
fn info_args(a: &Value, mut info: io::ModelInfo) -> Result<Option<io::ModelInfo>, String> {
    let mut changed = false;
    for (key, (_, value)) in INFO_KEYS.iter().zip(info.fields_mut()) {
        match &a[*key] {
            Value::Null => {}
            Value::String(s) => {
                *value = s.clone();
                changed = true;
            }
            _ => return Err(format!("{key} must be a string")),
        }
    }
    Ok(changed.then_some(info))
}

fn info_json(info: &io::ModelInfo) -> Value {
    let fields = INFO_KEYS
        .iter()
        .zip(info.fields())
        .map(|(key, (_, value))| (key.to_string(), Value::from(value)));
    Value::Object(fields.collect())
}

fn parse_setup(a: &Value) -> Result<Setup, String> {
    let text = |key: &str| -> Result<Option<String>, String> {
        match &a[key] {
            Value::Null => Ok(None),
            Value::String(s) if !s.trim().is_empty() => Ok(Some(s.clone())),
            _ => Err(format!("{key} must be a non-empty string")),
        }
    };
    // Slots are numbered from 1, as the slicer and the Material page show them.
    let slot = |v: &Value, what: &str| -> Result<usize, String> {
        match v.as_u64() {
            Some(n) if n >= 1 => Ok(n as usize - 1),
            _ => Err(format!("{what} must be a filament slot number, from 1")),
        }
    };
    let filament = match &a["filament"] {
        Value::Null => None,
        v => Some(slot(v, "filament")?),
    };
    let filaments = match &a["filaments"] {
        Value::Null => Vec::new(),
        Value::Array(items) => items
            .iter()
            .map(|item| {
                let color = item["color"].as_str().and_then(parse_hex);
                let color = color.ok_or("each filaments color must look like #RRGGBB")?;
                Ok((color, slot(&item["slot"], "each filaments slot")?))
            })
            .collect::<Result<_, String>>()?,
        _ => return Err("filaments must be a list of { color, slot }".into()),
    };
    Ok(Setup {
        slicer: text("slicer")?,
        printer: text("printer")?,
        process: text("process")?,
        filament,
        filaments,
    })
}

/// `name` from `options`: exactly, else the one that matches ignoring case.
fn pick(options: &[String], name: &str, what: &str) -> Result<String, String> {
    if let Some(o) = options.iter().find(|o| *o == name) {
        return Ok(o.clone());
    }
    let mut like = options
        .iter()
        .filter(|o| o.eq_ignore_ascii_case(name.trim()));
    match (like.next(), like.next()) {
        (Some(o), None) => Ok(o.clone()),
        _ if options.is_empty() => Err(format!(
            "unknown {what}: {name}; the slicer has none, choose a slicer first"
        )),
        _ => Err(format!(
            "unknown {what}: {name}; choose one of: {}",
            options.join(", ")
        )),
    }
}

/// A printer's status for the agent: never its address, which a name may be when the slicer
/// has no other.
fn status_json(i: usize, p: &crate::slicer::Printer, s: &crate::slicer::Status) -> Value {
    use crate::slicer::State;
    let bare = |h: &str| {
        let h = h
            .trim()
            .rsplit("://")
            .next()
            .unwrap_or("")
            .trim_end_matches('/');
        h.to_owned()
    };
    let host = bare(&p.host);
    let address = host.split(':').next().unwrap_or("");
    let name = if address.is_empty() || !p.name.contains(address) {
        p.name.clone()
    } else if !p.model.is_empty() {
        p.model.clone()
    } else {
        format!("Printer {}", i + 1)
    };
    let state = match s.state {
        State::Offline => "offline",
        State::Idle => "idle",
        State::Printing => "printing",
        State::Paused => "paused",
        State::Error => "error",
    };
    // In f64, so the JSON shows 25.6 and not 25.600000381.
    let tenth = |x: f32| (f64::from(x) * 10.0).round() / 10.0;
    let online = s.state != State::Offline;
    json!({
        "name": name,
        "model": p.model,
        "state": state,
        "progress_percent": s.progress.map(|p| tenth(p * 100.0)),
        "file": (online && !s.file.is_empty()).then_some(&s.file),
        "nozzle_c": s.nozzle.filter(|_| online).map(tenth),
        "bed_c": s.bed.filter(|_| online).map(tenth),
    })
}

fn hex(c: Rgba) -> String {
    format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_args_set_only_the_given_fields() {
        let base = io::ModelInfo {
            title: "Bone".into(),
            designer: "Ana".into(),
            ..Default::default()
        };
        assert_eq!(info_args(&json!({}), base.clone()), Ok(None));
        let set = info_args(
            &json!({ "designer": "", "license_terms": "CC BY 4.0" }),
            base,
        )
        .unwrap()
        .unwrap();
        assert_eq!(set.title, "Bone");
        assert_eq!(set.designer, "");
        assert_eq!(set.license_terms, "CC BY 4.0");
        assert!(info_args(&json!({ "title": 3 }), set.clone()).is_err());
        let back = info_json(&set);
        assert_eq!(back["license_terms"], "CC BY 4.0");
        assert_eq!(back.as_object().unwrap().len(), 5);
    }

    #[test]
    fn model_info_survives_a_3mf_round_trip() {
        let info = io::ModelInfo {
            title: "Bracket <v2> & \"co\"".into(),
            designer: "Ana".into(),
            copyright: "© 2026".into(),
            license_terms: "CC BY 4.0".into(),
            description: "Two lines\nof text".into(),
        };
        let object = io::ModelObject {
            name: "Cube".into(),
            mesh: crate::primitives::Primitive::Cube.build(),
            transform: glam::Mat4::IDENTITY,
            color: [255, 0, 0, 255],
        };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("info.3mf");
        io::save(&path, &[object], &info).unwrap();
        assert_eq!(io::load(&path).unwrap().info, info);
    }

    #[test]
    fn parses_the_print_setup() {
        assert_eq!(parse_setup(&json!({})), Ok(Setup::default()));
        let got = parse_setup(&json!({
            "slicer": "OrcaSlicer",
            "printer": "K1 Max",
            "filament": 2,
            "filaments": [{ "color": "#FF0000", "slot": 1 }],
        }))
        .unwrap();
        assert_eq!(got.slicer.as_deref(), Some("OrcaSlicer"));
        assert_eq!(got.printer.as_deref(), Some("K1 Max"));
        assert_eq!(got.process, None);
        assert_eq!(got.filament, Some(1));
        assert_eq!(got.filaments, [([255, 0, 0, 255], 0)]);
        for bad in [
            json!({ "printer": "" }),
            json!({ "process": 1 }),
            json!({ "filament": 0 }),
            json!({ "filament": "2" }),
            json!({ "filaments": { "color": "#FF0000", "slot": 1 } }),
            json!({ "filaments": [{ "color": "red", "slot": 1 }] }),
            json!({ "filaments": [{ "color": "#FF0000" }] }),
        ] {
            assert!(parse_setup(&bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn picks_a_profile_by_name() {
        let options = [
            "K1 Max".to_owned(),
            "k1 max".to_owned(),
            "Ender 3".to_owned(),
        ];
        assert_eq!(pick(&options, "k1 max", "printer").unwrap(), "k1 max");
        assert_eq!(pick(&options, "ENDER 3", "printer").unwrap(), "Ender 3");
        // Two match ignoring case: neither is guessed.
        assert!(pick(&options, "K1 MAX", "printer").is_err());
        assert!(pick(&[], "K1 Max", "printer").is_err());
    }

    #[test]
    fn printer_status_never_shows_the_address() {
        use crate::slicer::{Printer, State, Status};
        let printer = |name: &str, model: &str| Printer {
            name: name.into(),
            model: model.into(),
            host: "http://192.0.2.7:7125/".into(),
        };
        let printing = Status {
            state: State::Printing,
            progress: Some(0.256),
            file: "Bone.gcode".into(),
            nozzle: Some(219.96),
            bed: Some(60.0),
        };
        let v = status_json(0, &printer("K1", "Creality K1"), &printing);
        assert_eq!(v["name"], "K1");
        assert_eq!(v["state"], "printing");
        assert_eq!(v["progress_percent"], 25.6);
        assert_eq!(v["nozzle_c"], 220.0);
        assert!(!v.to_string().contains("192.0.2.7"));
        for (name, model, shown) in [
            ("192.0.2.7:7125", "Creality K1", "Creality K1"),
            ("192.0.2.7", "", "Printer 2"),
        ] {
            let v = status_json(1, &printer(name, model), &Status::default());
            assert_eq!(v["name"], shown);
            assert_eq!(v["state"], "offline");
            assert!(v["nozzle_c"].is_null());
            assert!(!v.to_string().contains("192.0.2.7"));
        }
    }
}
