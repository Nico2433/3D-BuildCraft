//! The AI assistant: with `--mcp` the app serves its tools over stdio (see `crate::mcp`)
//! and runs each call here, on the UI thread, through the same actions as the UI. So every
//! change is an undo step and shows on screen as it happens.

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

/// Serves MCP on stdin and stdout (started with `--mcp` and no app open).
pub fn serve_stdio(call: impl Fn(&str, Value) -> Reply + Send + 'static) {
    std::thread::spawn(move || {
        // The agent disconnected; the window stays open for the user.
        if let Err(e) = mcp::serve(std::io::stdin().lock(), std::io::stdout(), call) {
            log::warn!("MCP server stopped: {e}");
        }
    });
}

impl App {
    /// Runs the agent's pending calls; call once a frame, after the UI's actions.
    pub(super) fn serve_agent(&mut self, ctx: &egui::Context) {
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
        let lang = self.settings.lang();
        for r in requests {
            if r.tool == "screenshot" {
                if self.agent_shot.is_some() {
                    let _ = r
                        .reply
                        .send(Err("a screenshot is already being taken".into()));
                } else {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
                    self.agent_shot = Some(r.reply);
                }
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
            let mut out = self.agent_call(&r.tool, &r.args, ctx);
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
    }

    fn agent_call(&mut self, tool: &str, a: &Value, ctx: &egui::Context) -> Reply {
        let selection_needed = matches!(
            tool,
            "transform" | "set_color" | "hollow" | "simplify" | "split" | "emboss_text"
        ) || (tool == "edit"
            && !matches!(a["operation"].as_str(), Some("undo" | "redo")));
        if selection_needed && self.scene.selection.is_empty() {
            return Err("nothing is selected; call select first".into());
        }
        // A tool left open by the user would catch the edit; the agent's call wins.
        if tool == "measure" {
            return Ok(vec![mcp::text(&mcp::measure(&self.scene, a)?.to_string())]);
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
                    .ok_or("ids must be a list")?
                    .iter()
                    .map(|v| v.as_u64().ok_or("ids must be integers"))
                    .collect::<Result<_, _>>()?;
                if let Some(id) = ids.iter().find(|id| self.scene.get(**id).is_none()) {
                    return Err(format!("no object with id {id}"));
                }
                self.scene.selection = ids.into_iter().collect();
            }
            "insert_shape" => {
                let name = a["shape"].as_str().unwrap_or("");
                let shape = Primitive::ALL
                    .into_iter()
                    .find(|p| p.label().eq_ignore_ascii_case(name))
                    .ok_or_else(|| format!("unknown shape: {name}"))?;
                self.apply(Action::Insert(shape), ctx);
            }
            "transform" => {
                let rotation = vec3(&a["rotation"])?;
                let size = vec3(&a["size"])?;
                let position = vec3(&a["position"])?;
                if size.is_some_and(|s| s.min_element() <= 0.0) {
                    return Err("size must be greater than 0".into());
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
                if op == "subtract" && self.scene.selection.is_empty() {
                    return Err("subtract needs a selected object".into());
                }
                let action = match op {
                    "delete" => Action::Delete,
                    "duplicate" => Action::Duplicate,
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
                    _ => return Err(format!("unknown operation: {op}")),
                };
                self.apply(action, ctx);
                self.wait_jobs();
            }
            "set_color" => {
                let c = a["color"].as_str().and_then(parse_hex);
                let c = c.ok_or("color must look like #RRGGBB")?;
                self.apply(Action::SetColor(c), ctx);
            }
            "hollow" => {
                let wall = positive(&a["wall"], "wall")?;
                self.tool = Some(Tool::Hollow { wall });
                self.apply(Action::ApplyHollow, ctx);
            }
            "simplify" => {
                let level = a["remove"].as_f64().ok_or("remove must be a number")? as f32;
                if !(0.0..=1.0).contains(&level) {
                    return Err("remove must be between 0 and 1".into());
                }
                self.tool = Some(Tool::Simplify { level });
                self.apply(Action::ApplySimplify, ctx);
            }
            "split" => {
                let z = a["z"].as_f64().filter(|z| z.is_finite());
                let z = z.ok_or("z must be a number")? as f32;
                let keep = match a["keep"].as_str().unwrap_or("top") {
                    "top" => Keep::Top,
                    "bottom" => Keep::Bottom,
                    "both" => Keep::Both,
                    k => return Err(format!("unknown keep: {k}")),
                };
                self.tool = Some(Tool::Cut { z, keep });
                self.apply(Action::ApplyCut, ctx);
            }
            "emboss_text" => {
                let mut params = Emboss {
                    text: a["text"].as_str().unwrap_or("").to_owned(),
                    ..Emboss::default()
                };
                if params.text.trim().is_empty() {
                    return Err("text is empty".into());
                }
                if !a["size"].is_null() {
                    params.size = positive(&a["size"], "size")?;
                }
                if !a["depth"].is_null() {
                    params.depth = positive(&a["depth"], "depth")?;
                }
                params.engrave = a["engrave"].as_bool().unwrap_or(false);
                if !a["offset"].is_null() {
                    let xy: Option<Vec<f32>> = a["offset"].as_array().map(|v| {
                        v.iter()
                            .filter_map(|x| x.as_f64().filter(|x| x.is_finite()))
                            .map(|x| x as f32)
                            .collect()
                    });
                    match xy.as_deref() {
                        Some(&[x, y]) => params.offset = glam::Vec2::new(x, y),
                        _ => return Err("offset must be [x, y]".into()),
                    }
                }
                self.tool = Some(Tool::Emboss {
                    params,
                    preview: Err(String::new()),
                    built: None,
                });
                self.apply(Action::ApplyEmboss, ctx);
            }
            "open" => {
                let path = PathBuf::from(a["path"].as_str().ok_or("path is missing")?);
                if !path.is_file() {
                    return Err(format!("no such file: {}", path.display()));
                }
                if self.scene.dirty && a["discard_changes"].as_bool() != Some(true) {
                    return Err(
                        "the scene has unsaved changes; save it or pass discard_changes".into(),
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
                let path = PathBuf::from(a["path"].as_str().ok_or("path is missing")?);
                if path.exists() && a["overwrite"].as_bool() != Some(true) {
                    return Err(format!(
                        "{} exists; pass overwrite to replace it",
                        path.display()
                    ));
                }
                self.save_to(path);
            }
            "print" => {
                if self.scene.objects.is_empty() {
                    return Err("the scene is empty".into());
                }
                if self.settings.slicer.is_none() {
                    self.apply(Action::DetectSlicers, ctx);
                }
                if self.settings.slicer.is_none() {
                    return Err("no slicer found; the user can choose one in 3D print".into());
                }
                self.apply(Action::SendToSlicer, ctx);
            }
            other => return Err(format!("unknown tool: {other}")),
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

/// An optional `[x, y, z]`; anything else is an error.
fn vec3(v: &Value) -> Result<Option<Vec3>, String> {
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
        _ => Err("expected [x, y, z] with three numbers".into()),
    }
}

fn positive(v: &Value, name: &str) -> Result<f32, String> {
    match v.as_f64() {
        Some(x) if x.is_finite() && x > 0.0 => Ok(x as f32),
        _ => Err(format!("{name} must be a number greater than 0")),
    }
}

fn parse_hex(s: &str) -> Option<Rgba> {
    let h = s.strip_prefix('#').unwrap_or(s);
    if h.len() != 6 || !h.is_ascii() {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?, 0xFF])
}

fn round3(v: Vec3) -> [f32; 3] {
    v.to_array().map(|x| (x * 100.0).round() / 100.0)
}

fn encode_png(img: &egui::ColorImage) -> Reply {
    let [w, h] = img.size;
    let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
    let rgba = image::RgbaImage::from_raw(w as u32, h as u32, bytes).ok_or("bad screenshot")?;
    let mut png = std::io::Cursor::new(Vec::new());
    rgba.write_to(&mut png, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(vec![mcp::png(png.get_ref())])
}
