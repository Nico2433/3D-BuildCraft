//! The agent's modelling tools beyond the basics in `agent.rs`: painting and texturing by
//! area, materials, the full Split and Emboss, custom shapes, adding files and Load image.
//! Each one sets up the same tool or action the UI uses and applies it, so it is one undo
//! step and fails with the message the UI would show. Schemas: `crate::mcp::model_tools`.

use super::tool_bar::{custom_mesh, CUSTOM_SHAPES};
use super::*;
use crate::mcp::args::{self, limits::*, *};
use crate::texture_gen::Kind;
use serde_json::Value;
use std::sync::Arc;

/// How far a point given by the agent may be from the surface it means, in mm.
const SURFACE_REACH: f32 = 5.0;

impl App {
    /// `insert_shape`: a basic or Custom shape on the floor, optionally resized.
    pub(super) fn agent_insert_shape(
        &mut self,
        a: &Value,
        ctx: &egui::Context,
    ) -> Result<(), String> {
        let name = a["shape"].as_str().unwrap_or("");
        let shape = Primitive::ALL
            .into_iter()
            .find(|p| p.label().eq_ignore_ascii_case(name))
            .ok_or_else(|| {
                let names: Vec<String> = Primitive::ALL
                    .iter()
                    .map(|p| p.label().to_lowercase())
                    .collect();
                format!("unknown shape: {name}; one of {}", names.join(", "))
            })?;
        let size = object_size(a)?;
        let c = &a["custom"];
        if c.is_null() {
            self.apply(Action::Insert(shape), ctx);
        } else {
            if !CUSTOM_SHAPES.contains(&shape) {
                return Err(format!(
                    "custom works with cube, sphere, cone, torus and cylinder, not {name}; leave custom out for a plain {name}"
                ));
            }
            let (lat, lon, width, height, roundness) = args::custom(c)?;
            let key = (shape, lat, lon, width, height, roundness);
            self.tool = Some(Tool::Custom {
                shape,
                lat,
                lon,
                width,
                height,
                roundness,
                wire: true,
                preview: Ok(custom_mesh(key)?),
                built: Some(key),
            });
            self.apply(Action::ApplyCustom, ctx);
        }
        if let Some(s) = size {
            // Part of the insert's undo step; it stays on the floor.
            let bottom = self.scene.selection_aabb().min.z;
            self.apply(Action::SetSize(s), ctx);
            let b = self.scene.selection_aabb();
            self.apply(
                Action::MoveSelectionTo(b.center() + Vec3::Z * (bottom - b.min.z)),
                ctx,
            );
        }
        Ok(())
    }

    /// `paint_area`: Paint's click with a coverage, at the surface point nearest `point`.
    pub(super) fn agent_paint_area(
        &mut self,
        a: &Value,
        ctx: &egui::Context,
    ) -> Result<(), String> {
        let color = color(a, "color")?.ok_or("color is missing: #RRGGBB, e.g. #FF8000")?;
        let coverage = number(a, "coverage", 0.0, FRACTION)?;
        let ids = match object_id(a)? {
            Some(id) => vec![id],
            None => self.scene.objects.iter().map(|o| o.id).collect(),
        };
        let at = point(a)?;
        let (id, tri, _) = self.surface_at(&ids, at)?;
        let near = radius(a)?.map(|r| (at, r));
        self.apply(Action::PaintArea(id, tri, color, coverage, near), ctx);
        Ok(())
    }

    /// `set_material`: Paint → Material's type and colour on the selection.
    pub(super) fn agent_material(&mut self, a: &Value, ctx: &egui::Context) -> Result<(), String> {
        let kinds = Material::ALL.map(|m| (m.name(), m));
        let kind = choice(a, "type", None, &kinds)?;
        let new_color = color(a, "color")?;
        self.apply(Action::StartMaterial, ctx);
        if let Some(Tool::Material {
            kind: k, color: c, ..
        }) = &mut self.tool
        {
            *k = kind;
            *c = new_color.unwrap_or(*c);
        }
        self.material_step();
        self.apply(Action::ApplyMaterial, ctx);
        Ok(())
    }

    /// `apply_texture`: Texture's brush (box mapping) or a sticker, with a built-in
    /// texture or a picture file.
    pub(super) fn agent_texture(&mut self, a: &Value) -> Result<(), String> {
        let layer = texture_layer(a)?;
        let ids = match object_id(a)? {
            Some(id) if self.scene.selection.contains(&id) => vec![id],
            Some(id) => {
                return Err(format!(
                    "object {id} is not selected; select it first, or leave id out"
                ))
            }
            None => self.scene.selected_ids(),
        };
        let at = vec3(a, "point", COORD)?;
        match choice(
            a,
            "mode",
            Some("brush"),
            &[("brush", "brush"), ("sticker", "sticker")],
        )? {
            "brush" => {
                let coverage = number(a, "coverage", 1.0, FRACTION)?;
                let radius = radius(a)?;
                match at {
                    Some(p) => {
                        let (id, tri, _) = self.surface_at(&ids, p)?;
                        let near = radius.map(|r| (p, r));
                        self.texture_box(id, Some((tri, coverage)), near, layer, true);
                    }
                    None if radius.is_some() => {
                        return Err("radius needs point, the brush's centre [x, y, z] in mm".into())
                    }
                    // The whole of every object, as one undo step.
                    None => {
                        for (i, id) in ids.into_iter().enumerate() {
                            self.texture_box(id, None, None, layer, i == 0);
                        }
                    }
                }
            }
            _ => {
                let p = at.ok_or("a sticker needs point, a surface point [x, y, z] in mm")?;
                let hit = self.surface_at(&ids, p)?;
                // No viewing direction: the sticker faces out of the triangle.
                let mut decal = self
                    .texture_decal(hit, layer, Vec3::ZERO)
                    .ok_or("the object is gone")?;
                if let Some(width) = optional(a, "size", STICKER_SIZE)? {
                    decal.width = width;
                    decal.height = decal.width / crate::texture::aspect(layer).max(0.05);
                }
                decal.angle = number(a, "angle", 0.0, TURN)?.to_radians();
                self.bake_decal(hit.0, decal, layer);
            }
        }
        Ok(())
    }

    /// `split`: the Split tool's plane, level at `z` or through a point and turned.
    pub(super) fn agent_split(&mut self, a: &Value, ctx: &egui::Context) -> Result<(), String> {
        let (plane, keep) = args::split(a, self.scene.selection_aabb().center())?;
        self.tool = Some(Tool::Cut { plane, keep });
        self.apply(Action::ApplyCut, ctx);
        Ok(())
    }

    /// `emboss_text`: the Emboss tool with every setting it has.
    pub(super) fn agent_emboss(&mut self, a: &Value, ctx: &egui::Context) -> Result<(), String> {
        let selected = self.scene.selection.len();
        if selected != 1 {
            return Err(format!(
                "emboss_text needs exactly one selected object; {selected} are selected"
            ));
        }
        let mut params = args::emboss(a)?;
        if let Some(p) = vec3(a, "at", COORD)? {
            let target = self.scene.selected().next().ok_or("the object is gone")?;
            crate::emboss::place_at(&target.world_mesh(), &mut params, p);
        }
        self.tool = Some(Tool::emboss(params));
        self.apply(Action::ApplyEmboss, ctx);
        Ok(())
    }

    /// `import`: adds a model file to the scene, as Add → Import model with its units.
    pub(super) fn agent_import(&mut self, a: &Value, ctx: &egui::Context) -> Result<(), String> {
        let path = model_file(a)?;
        let mut units = choice(
            a,
            "units",
            Some(Units::Millimeter),
            &[
                ("micron", Units::Micron),
                ("mm", Units::Millimeter),
                ("cm", Units::Centimeter),
                ("inch", Units::Inch),
                ("foot", Units::Foot),
                ("m", Units::Meter),
            ],
        )?;
        // These formats carry their units and load in millimetres already.
        if matches!(
            Format::from_path(&path),
            Some(Format::ThreeMf | Format::Gltf | Format::Glb | Format::Svg)
        ) {
            units = Units::Millimeter;
        }
        let queued = self.pending_import.len();
        self.apply(Action::AddPath(path), ctx);
        self.wait_jobs();
        // A file that fails to load leaves a notice, which `serve_agent` returns.
        // Files dropped onto the window meanwhile stay queued for the user.
        let (mut added, dropped): (Vec<_>, Vec<_>) = self
            .pending_import
            .split_off(queued.min(self.pending_import.len()))
            .into_iter()
            .partition(|p| p.batch.is_none());
        self.pending_import.extend(dropped);
        let count = added.len();
        // Each file, at its units, is checked before any is added.
        for p in &mut added {
            p.units = units;
            p.check_size()?;
        }
        for (i, p) in added.into_iter().enumerate() {
            self.pending_import.insert(i, p);
        }
        for _ in 0..count {
            self.finish_import(true);
        }
        Ok(())
    }

    /// `extrude_outline`: Extrude outline's border round each selected object, at once.
    pub(super) fn agent_extrude_outline(&mut self, a: &Value) -> Result<(), String> {
        let height = self.scene.selection_aabb().size().z;
        self.outline_now(args::extrude_outline(a, height)?)
    }

    /// `load_image`: Load image's solid from a picture, added on the floor.
    pub(super) fn agent_load_image(
        &mut self,
        a: &Value,
        ctx: &egui::Context,
    ) -> Result<(), String> {
        let opts = args::load_image(a)?;
        let mesh = crate::image3d::build(&opts).map_err(|e| format!("{e:#}"))?;
        self.tool = Some(Tool::Image {
            opts: opts.clone(),
            preview: Ok(Arc::new(mesh)),
            built: Some(opts),
        });
        self.apply(Action::ApplyCustom, ctx);
        Ok(())
    }

    /// The triangle of objects `ids` nearest to world point `p`: (object, triangle, the
    /// nearest surface point). Fails when `p` is more than `SURFACE_REACH` from it.
    fn surface_at(&self, ids: &[u64], p: Vec3) -> Result<(u64, usize, Vec3), String> {
        let (id, tri, at) = ids
            .iter()
            .filter_map(|id| self.scene.get(*id))
            .filter_map(|o| {
                let (tri, at) = o.mesh.closest_triangle(&o.matrix(), p)?;
                Some((o.id, tri, at))
            })
            .min_by(|x, y| x.2.distance_squared(p).total_cmp(&y.2.distance_squared(p)))
            .ok_or("no object with that id; get_scene lists the ids")?;
        let d = at.distance(p);
        if d > SURFACE_REACH {
            return Err(format!(
                "the point is {d:.1} mm from the nearest surface (object {id} at [{:.2}, {:.2}, {:.2}]); give a point on the surface",
                at.x, at.y, at.z
            ));
        }
        Ok((id, tri, at))
    }
}

/// The picture a texture call names: a built-in `texture` or an `image` file.
fn texture_layer(a: &Value) -> Result<u16, String> {
    let layer = match (a["texture"].as_str(), a["image"].is_null()) {
        (Some(name), true) => {
            let kind = Kind::from_name(name).ok_or_else(|| {
                let names: Vec<String> = Kind::all().map(|k| k.name()).collect();
                format!(
                    "unknown texture: {name}; one of {}, or a picture in image",
                    names.join(", ")
                )
            })?;
            crate::texture::layer_of(kind)
        }
        (None, false) => {
            let path = picture_file(a, "image")?;
            let img = crate::picture::open(&path).map_err(|e| format!("{e:#}"))?;
            crate::texture::register_image(&img.into_rgba8())
        }
        (Some(_), false) => return Err("give texture or image, not both".into()),
        (None, _) => {
            return Err(format!(
                "give texture (a built-in texture's name) or image (a picture: {})",
                crate::picture::FORMATS
            ))
        }
    };
    layer.ok_or_else(|| {
        "there are too many pictures loaded; reuse a picture already used, or a built-in texture"
            .into()
    })
}

/// The optional object `id`; it must exist.
fn object_id(a: &Value) -> Result<Option<u64>, String> {
    match &a["id"] {
        Value::Null => Ok(None),
        v => v
            .as_u64()
            .map(Some)
            .ok_or_else(|| "id must be an object id from get_scene".into()),
    }
}

/// The required `point`.
fn point(a: &Value) -> Result<Vec3, String> {
    vec3(a, "point", COORD)?
        .ok_or_else(|| "point is missing: a surface point [x, y, z] in mm".into())
}

/// The optional `radius` (mm) that keeps a brush to the triangles centred near its point.
fn radius(a: &Value) -> Result<Option<f32>, String> {
    match &a["radius"] {
        Value::Null => Ok(None),
        v => number_in(v, "radius", BRUSH_RADIUS).map(Some),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_texture_comes_from_a_name_or_a_file_but_not_both() {
        assert!(texture_layer(&json!({"texture": "walnut"})).is_ok());
        assert!(texture_layer(&json!({"texture": "plywood"})).is_err());
        assert!(texture_layer(&json!({})).is_err());
        assert!(texture_layer(&json!({"texture": "walnut", "image": "/x.png"})).is_err());
        assert!(texture_layer(&json!({"image": "/no/such/picture.png"}))
            .unwrap_err()
            .contains("no such file"));
    }
}
