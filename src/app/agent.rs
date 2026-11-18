//! The AI assistant: with `--mcp` the app serves its tools over stdio (see `crate::mcp`)
//! and runs each call here, on the UI thread, through the same actions as the UI. So every
//! change is an undo step and shows on screen as it happens.

use super::agent_shot::Shot;
use super::*;
use crate::mcp::{self, Reply};
use serde_json::{json, Value};
use std::sync::mpsc::{channel, Sender};

pub struct Request {
    tool: String,
    args: Value,
    reply: Sender<Reply>,
}

/// A tool call made from any thread: it is sent to the UI thread and waited for.
pub fn caller(
    tx: Sender<Request>,
    ctx: egui::Context,
) -> impl Fn(&str, Value) -> Reply + Clone + Send + 'static {
    move |tool, args| {
        let (reply, answer) = channel();
        let request = Request {
            tool: tool.to_owned(),
            args,
            reply,
        };
        if tx.send(request).is_err() {
            return Err("3D BuildCraft was closed".into());
        }
        ctx.request_repaint();
        answer
            .recv()
            .unwrap_or_else(|_| Err("3D BuildCraft was closed".into()))
    }
}

impl App {
    /// Runs the agent's pending calls; call once a frame, after the UI's actions.
    pub(super) fn serve_agent(&mut self, ctx: &egui::Context) -> bool {
        if self.power.sleeping() {
            if let Some(reply) = self.agent_shot.take() {
                let _ = reply.send(Err("screenshots are unavailable while minimized".into()));
            }
        }
        if let Some(reply) = self.agent_shot.take() {
            let shot = ctx.input(|i| {
                i.raw.events.iter().find_map(|e| match e {
                    egui::Event::Screenshot { image, .. } => Some(image.clone()),
                    _ => None,
                })
            });
            match shot {
                Some(img) => {
                    let _ = reply.send(encode_png(&img));
                }
                None => {
                    self.agent_shot = Some(reply);
                    ctx.request_repaint();
                }
            }
        }
        let requests: Vec<Request> = self.agent.try_iter().collect();
        let processed = !requests.is_empty();
        let lang = self.settings.lang();
        for r in requests {
            if let Err(e) = mcp::check_args(&r.tool, &r.args) {
                let _ = r.reply.send(Err(e));
                continue;
            }
            if r.tool == "screenshot" {
                self.screenshot(r, ctx);
                continue;
            }
            if r.tool != "get_scene" {
                // One notice that follows the assistant, not one per call.
                let prefix = tr(lang, "AI assistant:");
                self.toasts.retain(|t| !t.text.starts_with(prefix));
                self.toast(format!("{prefix} {}", r.tool), false);
            }
            // Errors the actions report as notices go back to the agent too.
            let before = self.toasts.len();
            // A panic in a tool fails that call, not the app.
            let mut out = crate::no_panic(|| Ok(self.agent_call(&r.tool, &r.args, ctx)))
                .unwrap_or_else(|e| Err(format!("{e:#}; the scene may need undo")));
            let errors: Vec<String> = self.toasts[before.min(self.toasts.len())..]
                .iter()
                .filter(|t| t.error)
                .map(|t| t.text.clone())
                .collect();
            if out.is_ok() && !errors.is_empty() {
                out = Err(errors.join("\n"));
            }
            let _ = r.reply.send(out);
        }
        processed
    }

    fn agent_call(&mut self, tool: &str, a: &Value, ctx: &egui::Context) -> Reply {
        let key = match tool {
            "emboss_text" | "apply_texture" => "image",
            _ => "path",
        };
        let inline = if matches!(
            tool,
            "open" | "import" | "load_image" | "emboss_text" | "apply_texture"
        ) {
            mcp::inline_file(a, key, &mcp::inline_dir())?
        } else {
            None
        };
        let a = inline.as_ref().unwrap_or(a);
        let selection_needed = matches!(
            tool,
            "transform"
                | "set_color"
                | "hollow"
                | "smooth"
                | "simplify"
                | "extrude_outline"
                | "split"
                | "emboss_text"
                | "set_material"
                | "apply_texture"
        ) || (tool == "edit"
            && !matches!(a["operation"].as_str(), Some("undo" | "redo")));
        if selection_needed && self.scene.selection.is_empty() {
            return Err(mcp::nothing_selected(tool));
        }
        // A tool left open by the user would catch the edit; the agent's call wins.
        if tool == "measure" {
            return Ok(vec![mcp::text(&mcp::measure(&self.scene, a)?.to_string())]);
        }
        if tool == "list_fonts" {
            let fonts = mcp::list_fonts(&crate::emboss::font_families(), a["filter"].as_str());
            return Ok(vec![mcp::text(&fonts.to_string())]);
        }
        // These leave the user's open tool alone.
        if matches!(
            tool,
            "model_info" | "print_setup" | "printer_status" | "print_on_paper"
        ) {
            return self.agent_print_tool(tool, a, ctx);
        }
        let edits = !matches!(tool, "get_scene" | "select");
        if edits {
            self.tool = None;
        }
        match tool {
            "get_scene" => {}
            "select" => {
                let ids: Vec<u64> = a["ids"]
                    .as_array()
                    .ok_or("ids must be a list of object ids, e.g. [1, 2]")?
                    .iter()
                    .map(|v| {
                        v.as_u64()
                            .ok_or("ids must be whole numbers, the ids from get_scene")
                    })
                    .collect::<Result<_, _>>()?;
                if let Some(id) = ids.iter().find(|id| self.scene.get(**id).is_none()) {
                    return Err(format!("no object with id {id}; get_scene lists the ids"));
                }
                self.scene.selection = ids.into_iter().collect();
            }
            "insert_shape" => self.agent_insert_shape(a, ctx)?,
            "transform" => {
                let rotation = vec3(a, "rotation")?;
                let size = vec3(a, "size")?;
                let position = vec3(a, "position")?;
                if size.is_some_and(|s| s.min_element() <= 0.0) {
                    return Err("size must be [x, y, z] in mm, all greater than 0".into());
                }
                self.apply(Action::BeginEdit, ctx);
                // Rotation first: it changes the bounding box the others are about.
                if let Some(r) = rotation {
                    self.apply(Action::SetRotation(r), ctx);
                }
                if let Some(s) = size {
                    // Resizing is about the centre; keep the bottom where it was, so
                    // objects on the floor stay there.
                    let bottom = self.scene.selection_aabb().min.z;
                    self.apply(Action::SetSize(s), ctx);
                    let b = self.scene.selection_aabb();
                    let lifted = b.center() + Vec3::Z * (bottom - b.min.z);
                    self.apply(Action::MoveSelectionTo(lifted), ctx);
                }
                if let Some(p) = position {
                    self.apply(Action::MoveSelectionTo(p), ctx);
                }
                // Side positions override the centre, so objects can meet at their bounds.
                for (side, axis, max) in [
                    ("bottom", 2, false),
                    ("left", 0, false),
                    ("right", 0, true),
                    ("front", 1, false),
                    ("back", 1, true),
                ] {
                    if let Some(value) = a[side].as_f64() {
                        let b = self.scene.selection_aabb();
                        let bound = if max { b.max[axis] } else { b.min[axis] };
                        let to = b.center() + Vec3::AXES[axis] * (value as f32 - bound);
                        self.apply(Action::MoveSelectionTo(to), ctx);
                    }
                }
            }
            "edit" => {
                let op = a["operation"].as_str().unwrap_or("");
                let boolean = matches!(op, "merge" | "intersect");
                if boolean && self.scene.selection.len() < 2 {
                    return Err(format!("{op} needs two or more selected objects"));
                }
                if op == "group" && self.scene.selection.len() < 2 {
                    return Err("group needs two or more selected objects".into());
                }
                if op == "ungroup" && !self.scene.selected().any(|o| o.mesh.solids().len() > 1) {
                    return Err(
                        "ungroup needs a selected object made of separate solids, such as a group"
                            .into(),
                    );
                }
                let action = match op {
                    "delete" => Action::Delete,
                    "duplicate" => Action::Duplicate,
                    "group" => Action::Group,
                    "ungroup" => Action::Ungroup,
                    "merge" => Action::Boolean(Op::Union),
                    "subtract" => Action::Boolean(Op::Subtract),
                    "intersect" => Action::Boolean(Op::Intersect),
                    "repair" => Action::Repair,
                    "smooth" => Action::Smooth,
                    "extrude_down" => Action::ExtrudeDown,
                    "settle" => Action::SettleInstant,
                    "mirror_x" => Action::Mirror(Axis::X),
                    "mirror_y" => Action::Mirror(Axis::Y),
                    "mirror_z" => Action::Mirror(Axis::Z),
                    "undo" => Action::Undo,
                    "redo" => Action::Redo,
                    _ => {
                        return Err(format!(
                            "unknown operation: {op}; one of {}",
                            mcp::EDIT_OPERATIONS.join(", ")
                        ))
                    }
                };
                self.apply(action, ctx);
                self.wait_jobs();
            }
            "set_color" => {
                let c = a["color"].as_str().and_then(parse_hex);
                let c = c.ok_or("color must be #RRGGBB, e.g. #FF8000")?;
                self.apply(Action::SetColor(c), ctx);
            }
            "smooth" => {
                let passes = match &a["passes"] {
                    Value::Null => 3,
                    v => v
                        .as_u64()
                        .filter(|p| *p <= crate::solid::SMOOTH_STEPS as u64)
                        .ok_or("passes must be a whole number from 0 to 6")?,
                };
                let amount = passes as f32 / crate::solid::SMOOTH_STEPS as f32;
                let edges = a["edges"].as_bool().unwrap_or(false);
                if !self.modify_selected(|m| crate::solid::smooth_by(m, amount, edges)) {
                    return Err("smoothing failed".into());
                }
            }
            "hollow" => {
                let wall = positive(&a["wall"], "wall")?;
                self.tool = Some(Tool::Hollow { wall });
                self.apply(Action::ApplyHollow, ctx);
            }
            "simplify" => {
                let level = a["remove"].as_f64().unwrap_or(-1.0) as f32;
                if !(0.0..=1.0).contains(&level) {
                    return Err(
                        "remove must be a number from 0 to 1, the fraction of triangles to remove"
                            .into(),
                    );
                }
                self.start_simplify(1.0 - level, ctx);
                self.wait_jobs();
            }
            "split" => self.agent_split(a, ctx)?,
            "emboss_text" => self.agent_emboss(a, ctx)?,
            "paint_area" => self.agent_paint_area(a, ctx)?,
            "set_material" => self.agent_material(a, ctx)?,
            "apply_texture" => self.agent_texture(a)?,
            "import" => self.agent_import(a, ctx)?,
            "load_image" => self.agent_load_image(a, ctx)?,
            "extrude_outline" => self.agent_extrude_outline(a)?,
            "open" => {
                let path = super::agent_model::model_file(a)?;
                if self.scene.dirty && a["discard_changes"].as_bool() != Some(true) {
                    return Err(
                        "the scene has unsaved changes; save it first, pass discard_changes true to drop them, or use import to add the file to the scene".into(),
                    );
                }
                self.scene.dirty = false;
                self.apply(Action::OpenPath(path), ctx);
                self.wait_jobs();
                // Files without units are taken as millimetres instead of asking.
                while !self.pending_import.is_empty() {
                    self.finish_import(false);
                }
            }
            "save" => {
                let mut path = PathBuf::from(
                    a["path"]
                        .as_str()
                        .ok_or("path is missing: the file to write, e.g. /home/me/box.3mf")?,
                );
                if path.extension().is_none() {
                    path.set_extension("3mf");
                }
                if !Format::from_path(&path).is_some_and(|f| Format::SAVE.contains(&f)) {
                    return Err(format!(
                        "save writes {}, chosen by the path's extension; SVG and pictures cannot be saved",
                        io::SAVE_FORMATS
                    ));
                }
                if path.exists() && a["overwrite"].as_bool() != Some(true) {
                    return Err(format!(
                        "{} exists; pass overwrite to replace it",
                        path.display()
                    ));
                }
                self.save_to(path);
            }
            "print" => self.agent_print(ctx)?,
            other => return Err(format!("unknown tool: {other}; tools/list lists the tools")),
        }
        // A tool that failed stays open in the UI; close it, the agent gets the error.
        if edits {
            self.tool = None;
        }
        Ok(vec![mcp::text(&self.scene_json().to_string())])
    }

    /// The scene as the agent sees it, in millimetres.
    fn scene_json(&self) -> Value {
        let objects: Vec<Value> = self
            .scene
            .objects
            .iter()
            .map(|o| {
                let b = o.world_aabb();
                let [r, g, bl, _] = o.color;
                json!({
                    "id": o.id,
                    "name": o.name,
                    "color": format!("#{r:02X}{g:02X}{bl:02X}"),
                    "center": round3(b.center()),
                    "center_of_mass": round3(o.world_mesh().centroid().unwrap_or(b.center())),
                    "size": round3(b.size()),
                    "bottom": (b.min.z * 1000.0).round() / 1000.0,
                    "top": (b.max.z * 1000.0).round() / 1000.0,
                    "left": (b.min.x * 1000.0).round() / 1000.0,
                    "right": (b.max.x * 1000.0).round() / 1000.0,
                    "front": (b.min.y * 1000.0).round() / 1000.0,
                    "back": (b.max.y * 1000.0).round() / 1000.0,
                    "rotation": round3(o.rotation),
                    "triangles": o.mesh.triangles.len(),
                    "valid": !self.invalid.contains(&o.id),
                    "selected": self.scene.selection.contains(&o.id),
                    "material": o.material.name(),
                    "painted": o.mesh.colors.iter().any(|c| c[3] > 0),
                    "textured": o.mesh.tex.iter().any(|t| t.layer != 0 && !t.decal),
                    "stickers": o.mesh.tex.iter().any(|t| t.layer != 0 && t.decal),
                })
            })
            .collect();
        json!({
            "units": "mm",
            "objects": objects,
            "slicer": self.settings.slicer.as_deref().map(|p| crate::slicer::from_path(p).name),
            "can_undo": self.history.can_undo(),
            "can_redo": self.history.can_redo(),
        })
    }
}

/// The optional `[x, y, z]` under `key`; anything else is an error.
pub(super) fn vec3(a: &Value, key: &str) -> Result<Option<Vec3>, String> {
    let v = &a[key];
    if v.is_null() {
        return Ok(None);
    }
    let xs: Option<Vec<f32>> = v.as_array().map(|a| {
        a.iter()
            .filter_map(|x| x.as_f64().map(|x| x as f32))
            .filter(|x| x.is_finite())
            .collect()
    });
    match xs.as_deref() {
        Some(&[x, y, z]) => Ok(Some(Vec3::new(x, y, z))),
        _ => Err(format!("{key} must be [x, y, z], three numbers")),
    }
}

pub(super) fn positive(v: &Value, name: &str) -> Result<f32, String> {
    match v.as_f64() {
        Some(x) if x.is_finite() && x > 0.0 => Ok(x as f32),
        _ => Err(format!("{name} must be a number greater than 0")),
    }
}

pub(super) fn parse_hex(s: &str) -> Option<Rgba> {
    let h = s.strip_prefix('#').unwrap_or(s);
    if h.len() != 6 || !h.is_ascii() {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?, 0xFF])
}

impl App {
    /// An off-screen view when the arguments ask for one, drawn on its own thread (the
    /// meshes are shared, not copied); otherwise the window as it is.
    fn screenshot(&mut self, r: Request, ctx: &egui::Context) {
        let reply = match Shot::parse(&self.scene, &r.args) {
            Err(e) => Err(e),
            Ok(Some(shot)) => {
                std::thread::spawn(move || {
                    let [w, h] = shot.size();
                    let _ = r.reply.send(png_reply(w, h, shot.render()));
                });
                return;
            }
            Ok(None) if self.power.sleeping() => {
                Err("screenshots are unavailable while minimized".into())
            }
            Ok(None) if self.agent_shot.is_some() => {
                Err("a screenshot is already being taken".into())
            }
            Ok(None) => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
                self.agent_shot = Some(r.reply);
                return;
            }
        };
        let _ = r.reply.send(reply);
    }
}

fn round3(v: Vec3) -> [f32; 3] {
    v.to_array().map(|x| (x * 100.0).round() / 100.0)
}

fn encode_png(img: &egui::ColorImage) -> Reply {
    let [w, h] = img.size;
    let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
    png_reply(w as u32, h as u32, bytes)
}

/// RGBA rows, `w`×`h`, as the PNG image a tool replies with.
fn png_reply(w: u32, h: u32, bytes: Vec<u8>) -> Reply {
    let rgba = image::RgbaImage::from_raw(w, h, bytes).ok_or("bad screenshot")?;
    let mut png = std::io::Cursor::new(Vec::new());
    rgba.write_to(&mut png, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(vec![mcp::png(png.get_ref())])
}
