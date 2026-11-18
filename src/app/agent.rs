//! The AI assistant: with `--mcp` the app serves its tools over stdio (see `crate::mcp`)
//! and runs each call here, on the UI thread, through the same actions as the UI. So every
//! change is an undo step and shows on screen as it happens.

use super::agent_shot::Shot;
use super::*;
use crate::mcp::{self, args, Reply};
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
            let mut out = crate::i18n::in_english(|| {
                crate::no_panic(|| Ok(self.agent_call(&r.tool, &r.args, ctx)))
            })
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
        // An object with no triangles can only be deleted.
        if selection_needed && !(tool == "edit" && a["operation"] == "delete") {
            self.scene.selected().try_for_each(mcp::require_geometry)?;
        }
        let adds = matches!(tool, "insert_shape" | "import" | "load_image")
            || (tool == "edit" && a["operation"] == "duplicate");
        if adds && self.scene.objects.len() >= args::limits::MAX_OBJECTS {
            return Err(format!(
                "the scene already has {} objects, the most {tool} adds to; delete some first",
                self.scene.objects.len()
            ));
        }
        // A tool left open by the user would catch the edit; the agent's call wins.
        if tool == "measure" {
            return Ok(vec![mcp::text(&mcp::measure(&self.scene, a)?.to_string())]);
        }
        if tool == "list_fonts" {
            let limit = (!a["limit"].is_null())
                .then(|| args::integer(a, "limit", 0, args::limits::LIST_LIMIT))
                .transpose()?;
            let filter = args::text(a, "filter", args::limits::INFO_CHARS)?;
            let fonts = mcp::list_fonts(
                &crate::emboss::font_families(),
                filter.as_deref(),
                limit.map(|n| n as usize),
            );
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
        // Objects are cheap to clone (their meshes are shared): the answer says what changed.
        let before = self.scene.objects.clone();
        // What `save` wrote: the file and its size in bytes.
        let mut saved = None;
        // What to tell the agent when the call did nothing.
        let mut note = None;
        match tool {
            "get_scene" => {}
            "select" => {
                let ids = args::ids(&a["ids"], "ids")?;
                if let Some(id) = ids.iter().find(|id| self.scene.get(**id).is_none()) {
                    return Err(format!("no object with id {id}; get_scene lists the ids"));
                }
                self.scene.selection = ids.into_iter().collect();
            }
            "insert_shape" => self.agent_insert_shape(a, ctx)?,
            "transform" => {
                let t = args::transform(a)?;
                if t == args::Transform::default() {
                    note = Some("nothing changed: give position, rotation, size, bottom, left, right, front or back");
                } else {
                    self.apply(Action::BeginEdit, ctx);
                    // Rotation first: it changes the bounding box the others are about.
                    if let Some(r) = t.rotation {
                        self.apply(Action::SetRotation(r), ctx);
                    }
                    if let Some(s) = t.size {
                        // Resizing is about the centre; keep the bottom where it was, so
                        // objects on the floor stay there.
                        let bottom = self.scene.selection_aabb().min.z;
                        self.apply(Action::SetSize(s), ctx);
                        let b = self.scene.selection_aabb();
                        let lifted = b.center() + Vec3::Z * (bottom - b.min.z);
                        self.apply(Action::MoveSelectionTo(lifted), ctx);
                    }
                    if let Some(p) = t.position {
                        self.apply(Action::MoveSelectionTo(p), ctx);
                    }
                    // Side positions override the centre, so objects can meet at their bounds.
                    for &(axis, max, value) in &t.sides {
                        let b = self.scene.selection_aabb();
                        let bound = if max { b.max[axis] } else { b.min[axis] };
                        let to = b.center() + Vec3::AXES[axis] * (value - bound);
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
                let c =
                    args::color(a, "color")?.ok_or("color is missing: #RRGGBB, e.g. #FF8000")?;
                self.apply(Action::SetColor(c), ctx);
            }
            "smooth" => {
                let passes = args::passes(a)?;
                let amount = passes as f32 / crate::solid::SMOOTH_STEPS as f32;
                let edges = a["edges"].as_bool().unwrap_or(false);
                if !self.modify_selected(|m| crate::solid::smooth_by(m, amount, edges)) {
                    return Err("smoothing failed".into());
                }
            }
            "hollow" => {
                let wall = args::wall(a)?;
                self.tool = Some(Tool::Hollow { wall });
                self.apply(Action::ApplyHollow, ctx);
            }
            "simplify" => {
                let level = args::simplify_remove(a)?;
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
                let path = args::model_file(a)?;
                if self.scene.dirty && a["discard_changes"].as_bool() != Some(true) {
                    return Err(
                        "the scene has unsaved changes; save it first, pass discard_changes true to drop them, or use import to add the file to the scene".into(),
                    );
                }
                self.scene.dirty = false;
                self.apply(Action::OpenPath(path), ctx);
                self.wait_jobs();
                // Files without units are taken as millimetres instead of asking.
                while let Some(p) = self.pending_import.first() {
                    if let Err(e) = p.check_size() {
                        self.cancel_import();
                        return Err(e);
                    }
                    self.finish_import(false);
                }
            }
            "save" => {
                let path = args::save_path(a)?;
                self.save_to(path.clone());
                // A write that failed left an error notice, which `serve_agent` returns.
                saved = std::fs::metadata(&path).ok().map(|m| (path, m.len()));
            }
            "print" => self.agent_print(ctx)?,
            other => return Err(format!("unknown tool: {other}; tools/list lists the tools")),
        }
        // A tool that failed stays open in the UI; close it, the agent gets the error.
        if edits {
            self.tool = None;
        }
        let mut answer = self.scene_json();
        if let Some(note) = note {
            answer["note"] = json!(note);
        }
        if let Some((path, bytes)) = saved {
            answer["saved_to"] = json!(path.display().to_string());
            answer["bytes"] = json!(bytes);
        }
        let changes = changes(&before, &self.scene.objects);
        if changes.as_object().is_some_and(|c| !c.is_empty()) {
            answer["changes"] = changes;
        }
        Ok(vec![mcp::text(&answer.to_string())])
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

/// What a call did to the objects, by id: `created`, `removed` and `modified` (shape,
/// placement, colour, material or name); a list with nothing in it is left out. A tool that
/// replaces an object (split, a subtraction) shows as removed ids and created ones.
fn changes(before: &[Object], after: &[Object]) -> Value {
    let ids = |list: Vec<u64>| (!list.is_empty()).then(|| json!(list));
    fn find(from: &[Object], id: u64) -> Option<&Object> {
        from.iter().find(|o| o.id == id)
    }
    let created: Vec<u64> = after
        .iter()
        .filter(|o| find(before, o.id).is_none())
        .map(|o| o.id)
        .collect();
    let removed: Vec<u64> = before
        .iter()
        .filter(|o| find(after, o.id).is_none())
        .map(|o| o.id)
        .collect();
    let modified: Vec<u64> = after
        .iter()
        .filter(|n| {
            find(before, n.id).is_some_and(|o| {
                !std::sync::Arc::ptr_eq(&o.mesh, &n.mesh)
                    || (
                        o.translation,
                        o.rotation,
                        o.scale,
                        o.color,
                        o.material,
                        &o.name,
                    ) != (
                        n.translation,
                        n.rotation,
                        n.scale,
                        n.color,
                        n.material,
                        &n.name,
                    )
            })
        })
        .map(|o| o.id)
        .collect();
    let all = [
        ("created", created),
        ("removed", removed),
        ("modified", modified),
    ];
    Value::Object(
        all.into_iter()
            .filter_map(|(key, list)| ids(list).map(|v| (key.to_owned(), v)))
            .collect(),
    )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changes_lists_created_removed_and_modified_ids() {
        let mut scene = Scene::default();
        let cube = |name: &str| Object::new(name, Primitive::Cube.build(), [200, 0, 0, 255]);
        let (a, b) = (scene.add(cube("a")), scene.add(cube("b")));
        let before = scene.objects.clone();
        assert_eq!(changes(&before, &scene.objects), json!({}));
        scene.get_mut(a).unwrap().color = [0, 0, 255, 255];
        scene.selection = [b].into();
        scene.delete_selected();
        let c = scene.add(cube("c"));
        assert_eq!(
            changes(&before, &scene.objects),
            json!({"created": [c], "removed": [b], "modified": [a]})
        );
    }
}
