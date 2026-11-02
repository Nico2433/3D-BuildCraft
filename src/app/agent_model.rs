//! The agent's modelling tools beyond the basics in `agent.rs`: painting and texturing by
//! area, materials, the full Split and Emboss, custom shapes, adding files and Load image.
//! Each one sets up the same tool or action the UI uses and applies it, so it is one undo
//! step and fails with the message the UI would show. Schemas: `crate::mcp::model_tools`.

use super::agent::{parse_hex, positive, vec3};
use super::tool_bar::{custom_mesh, CUSTOM_SHAPES, SEGMENTS};
use super::*;
use crate::texture_gen::Kind;
use serde_json::Value;
use std::ops::RangeInclusive;
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
            .ok_or_else(|| format!("unknown shape: {name}"))?;
        let size = vec3(&a["size"])?;
        if size.is_some_and(|s| s.min_element() <= 0.0) {
            return Err("size must be greater than 0".into());
        }
        let c = &a["custom"];
        if c.is_null() {
            self.apply(Action::Insert(shape), ctx);
        } else {
            if !c.is_object() {
                return Err("custom must be an object".into());
            }
            if !CUSTOM_SHAPES.contains(&shape) {
                return Err("custom works with cube, sphere, cone, torus and cylinder".into());
            }
            let steps = SEGMENTS[0] as u32..=SEGMENTS[SEGMENTS.len() - 1] as u32;
            let key = (
                shape,
                integer(c, "latitude", 10, steps.clone())? as usize,
                integer(c, "longitude", 10, steps)? as usize,
                number(c, "width", 40.0, 1.0..=1000.0)?,
                number(c, "height", 40.0, 1.0..=1000.0)?,
                integer(c, "roundness", 0, 0..=10)? as u8,
            );
            let (shape, lat, lon, width, height, roundness) = key;
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
        let color = color(a, "color")?.ok_or("color must look like #RRGGBB")?;
        let coverage = number(a, "coverage", 0.0, 0.0..=1.0)?;
        let ids = match object_id(a)? {
            Some(id) => vec![id],
            None => self.scene.objects.iter().map(|o| o.id).collect(),
        };
        let (id, tri, _) = self.surface_at(&ids, point(a)?)?;
        self.apply(Action::PaintArea(id, tri, color, coverage), ctx);
        Ok(())
    }

    /// `set_material`: Paint → Material's type and colour on the selection.
    pub(super) fn agent_material(&mut self, a: &Value, ctx: &egui::Context) -> Result<(), String> {
        let kind = choice(
            a,
            "type",
            None,
            &[
                ("none", Material::None),
                ("simple", Material::Simple),
                ("realistic", Material::Realistic),
                ("translucent", Material::Translucent),
            ],
        )?;
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
            Some(id) => return Err(format!("object {id} is not selected")),
            None => self.scene.selected_ids(),
        };
        let at = vec3(&a["point"])?;
        match choice(
            a,
            "mode",
            Some("brush"),
            &[("brush", "brush"), ("sticker", "sticker")],
        )? {
            "brush" => {
                let coverage = number(a, "coverage", 1.0, 0.0..=1.0)?;
                match at {
                    Some(p) => {
                        let (id, tri, _) = self.surface_at(&ids, p)?;
                        self.texture_box(id, Some((tri, coverage)), layer, true);
                    }
                    // The whole of every object, as one undo step.
                    None => {
                        for (i, id) in ids.into_iter().enumerate() {
                            self.texture_box(id, None, layer, i == 0);
                        }
                    }
                }
            }
            _ => {
                let p = at.ok_or("a sticker needs point")?;
                let hit = self.surface_at(&ids, p)?;
                // No viewing direction: the sticker faces out of the triangle.
                let mut decal = self
                    .texture_decal(hit, layer, Vec3::ZERO)
                    .ok_or("the object is gone")?;
                if !a["size"].is_null() {
                    decal.width = positive(&a["size"], "size")?;
                    decal.height = decal.width / crate::texture::aspect(layer).max(0.05);
                }
                decal.angle = number(a, "angle", 0.0, -360.0..=360.0)?.to_radians();
                self.bake_decal(hit.0, decal, layer);
            }
        }
        Ok(())
    }

    /// `split`: the Split tool's plane, level at `z` or through a point and turned.
    pub(super) fn agent_split(&mut self, a: &Value, ctx: &egui::Context) -> Result<(), String> {
        let z = match &a["z"] {
            Value::Null => None,
            v => Some(
                v.as_f64()
                    .filter(|z| z.is_finite())
                    .ok_or("z must be a number")? as f32,
            ),
        };
        let at = vec3(&a["point"])?;
        let (normal, rotation) = (vec3(&a["normal"])?, vec3(&a["rotation"])?);
        if normal.is_some() && rotation.is_some() {
            return Err("give normal or rotation, not both".into());
        }
        if z.is_none() && at.is_none() {
            return Err("give z, or point (with normal or rotation for a slanted plane)".into());
        }
        let mut centre = at.unwrap_or(self.scene.selection_aabb().center());
        if let Some(z) = z {
            centre.z = z;
        }
        let plane = match normal {
            Some(n) => CutPlane::through(centre, n).ok_or("normal must not be zero")?,
            None => CutPlane {
                center: centre,
                offset: 0.0,
                rotation: rotation.unwrap_or(Vec3::ZERO),
            },
        };
        let keep = choice(
            a,
            "keep",
            Some(Keep::Top),
            &[
                ("top", Keep::Top),
                ("bottom", Keep::Bottom),
                ("both", Keep::Both),
            ],
        )?;
        self.tool = Some(Tool::Cut { plane, keep });
        self.apply(Action::ApplyCut, ctx);
        Ok(())
    }

    /// `emboss_text`: the Emboss tool with every setting it has.
    pub(super) fn agent_emboss(&mut self, a: &Value, ctx: &egui::Context) -> Result<(), String> {
        let mut params = Emboss {
            text: a["text"].as_str().unwrap_or("").to_owned(),
            ..Emboss::default()
        };
        params.pattern = match a["pattern"].as_str().unwrap_or("text") {
            "text" => Pattern::Text,
            "star" => Pattern::Shape(Shape::Star),
            "heart" => Pattern::Shape(Shape::Heart),
            "circle" => Pattern::Shape(Shape::Circle),
            "image" => Pattern::Image(file(a, "image")?),
            p => return Err(format!("unknown pattern: {p}")),
        };
        if params.pattern == Pattern::Text && params.text.trim().is_empty() {
            return Err("text is empty".into());
        }
        for (key, value) in [("size", &mut params.size), ("depth", &mut params.depth)] {
            if !a[key].is_null() {
                *value = positive(&a[key], key)?;
            }
        }
        params.engrave = flag(a, "engrave", false)?;
        params.bevel = flag(a, "bevel", false)?;
        params.angle = number(a, "angle", 0.0, -360.0..=360.0)?;
        params.projection = choice(
            a,
            "projection",
            Some(Projection::Planar),
            &[
                ("planar", Projection::Planar),
                ("cylindrical", Projection::Cylindrical),
                ("spherical", Projection::Spherical),
            ],
        )?;
        if let Some(f) = a["font"].as_str() {
            params.font = Some(
                crate::emboss::font_families()
                    .into_iter()
                    .find(|n| n.eq_ignore_ascii_case(f.trim()))
                    .ok_or_else(|| format!("font {f} is not installed"))?,
            );
        }
        params.top_color = color(a, "top_color")?;
        params.side_color = color(a, "side_color")?;
        if !a["offset"].is_null() {
            let xy: Option<Vec<f32>> = a["offset"].as_array().map(|v| {
                v.iter()
                    .filter_map(|x| x.as_f64().filter(|x| x.is_finite()))
                    .map(|x| x as f32)
                    .collect()
            });
            match xy.as_deref() {
                Some(&[x, y]) => params.offset = GVec2::new(x, y),
                _ => return Err("offset must be [x, y]".into()),
            }
        }
        if let Some(p) = vec3(&a["at"])? {
            let target = match self.scene.selection.len() {
                1 => self.scene.selected().next(),
                _ => None,
            };
            let target = target.ok_or("emboss needs exactly one selected object")?;
            params.offset = crate::emboss::offset_at(&target.world_mesh(), params.projection, p);
        }
        self.tool = Some(Tool::Emboss {
            params,
            preview: Err(String::new()),
            built: None,
        });
        self.apply(Action::ApplyEmboss, ctx);
        Ok(())
    }

    /// `import`: adds a model file to the scene, as Add → Import model with its units.
    pub(super) fn agent_import(&mut self, a: &Value, ctx: &egui::Context) -> Result<(), String> {
        let path = file(a, "path")?;
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
            Some(Format::ThreeMf | Format::Gltf | Format::Glb)
        ) {
            units = Units::Millimeter;
        }
        let queued = self.pending_import.len();
        self.apply(Action::AddPath(path), ctx);
        self.wait_jobs();
        // A file that fails to load leaves a notice, which `serve_agent` returns.
        let added = self
            .pending_import
            .split_off(queued.min(self.pending_import.len()));
        let count = added.len();
        for (i, mut p) in added.into_iter().enumerate() {
            p.units = units;
            self.pending_import.insert(i, p);
        }
        for _ in 0..count {
            self.finish_import(true);
        }
        Ok(())
    }

    /// `load_image`: Load image's solid from a picture, added on the floor.
    pub(super) fn agent_load_image(
        &mut self,
        a: &Value,
        ctx: &egui::Context,
    ) -> Result<(), String> {
        let path = file(a, "path")?;
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !crate::image3d::EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()) {
            return Err("the picture must be PNG or JPEG".into());
        }
        use crate::image3d::Mode;
        let mut opts = crate::image3d::Options::new(path);
        opts.mode = choice(
            a,
            "method",
            Some(Mode::Contour),
            &[
                ("contour", Mode::Contour),
                ("height_map", Mode::HeightMap),
                ("edge", Mode::Edge),
                ("stamp", Mode::Stamp),
            ],
        )?;
        for (key, value) in [("size", &mut opts.size), ("depth", &mut opts.depth)] {
            if !a[key].is_null() {
                *value = positive(&a[key], key)?;
            }
        }
        opts.levels = integer(a, "levels", opts.levels, 2..=8)?;
        opts.smooth = number(a, "smooth", opts.smooth, 0.0..=1.0)?;
        opts.invert = flag(a, "invert", opts.invert)?;
        opts.textures = flag(a, "textures", opts.textures)?;
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
            .ok_or("there is no object there")?;
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
            let kind = Kind::from_name(name).ok_or_else(|| format!("unknown texture: {name}"))?;
            crate::texture::layer_of(kind)
        }
        (None, false) => {
            let path = file(a, "image")?;
            let img = image::open(&path)
                .map_err(|e| format!("could not open {}: {e}", path.display()))?;
            crate::texture::register_image(&img.into_rgba8())
        }
        _ => return Err("give either texture or image".into()),
    };
    layer.ok_or_else(|| "there are too many pictures loaded".into())
}

/// The optional object `id`; it must exist.
fn object_id(a: &Value) -> Result<Option<u64>, String> {
    match &a["id"] {
        Value::Null => Ok(None),
        v => v
            .as_u64()
            .map(Some)
            .ok_or_else(|| "id must be an integer".into()),
    }
}

/// The required `point`.
fn point(a: &Value) -> Result<Vec3, String> {
    vec3(&a["point"])?.ok_or_else(|| "point is missing".into())
}

/// A path to an existing file under `key`.
fn file(a: &Value, key: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(a[key].as_str().ok_or_else(|| format!("{key} is missing"))?);
    if !path.is_file() {
        return Err(format!("no such file: {}", path.display()));
    }
    Ok(path)
}

/// An optional `#RRGGBB` colour.
fn color(a: &Value, key: &str) -> Result<Option<Rgba>, String> {
    match &a[key] {
        Value::Null => Ok(None),
        v => v
            .as_str()
            .and_then(parse_hex)
            .map(Some)
            .ok_or_else(|| format!("{key} must look like #RRGGBB")),
    }
}

/// A number in `range`, or `default` when it is missing.
fn number(a: &Value, key: &str, default: f32, range: RangeInclusive<f32>) -> Result<f32, String> {
    match &a[key] {
        Value::Null => Ok(default),
        v => v
            .as_f64()
            .map(|x| x as f32)
            .filter(|x| range.contains(x))
            .ok_or_else(|| {
                format!(
                    "{key} must be a number from {} to {}",
                    range.start(),
                    range.end()
                )
            }),
    }
}

/// A whole number in `range`, or `default` when it is missing.
fn integer(a: &Value, key: &str, default: u32, range: RangeInclusive<u32>) -> Result<u32, String> {
    match &a[key] {
        Value::Null => Ok(default),
        v => v
            .as_u64()
            .and_then(|x| u32::try_from(x).ok())
            .filter(|x| range.contains(x))
            .ok_or_else(|| {
                format!(
                    "{key} must be a whole number from {} to {}",
                    range.start(),
                    range.end()
                )
            }),
    }
}

/// A true or false, or `default` when it is missing.
fn flag(a: &Value, key: &str, default: bool) -> Result<bool, String> {
    match &a[key] {
        Value::Null => Ok(default),
        v => v
            .as_bool()
            .ok_or_else(|| format!("{key} must be true or false")),
    }
}

/// One of `options` by name; `default` when missing, or an error when there is none.
fn choice<T: Copy>(
    a: &Value,
    key: &str,
    default: Option<T>,
    options: &[(&str, T)],
) -> Result<T, String> {
    let names = || {
        options
            .iter()
            .map(|(n, _)| *n)
            .collect::<Vec<_>>()
            .join(", ")
    };
    match &a[key] {
        Value::Null => default.ok_or_else(|| format!("{key} is missing: one of {}", names())),
        v => {
            let name = v.as_str().unwrap_or("");
            options
                .iter()
                .find(|(n, _)| n.eq_ignore_ascii_case(name))
                .map(|(_, t)| *t)
                .ok_or_else(|| format!("unknown {key}: {name}; one of {}", names()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn arguments_take_defaults_and_reject_bad_values() {
        let a =
            json!({"coverage": 0.25, "big": 2.0, "n": 5, "on": true, "c": "#FF8000", "m": "Cm"});
        assert_eq!(number(&a, "coverage", 0.0, 0.0..=1.0), Ok(0.25));
        assert_eq!(number(&a, "missing", 0.5, 0.0..=1.0), Ok(0.5));
        assert!(number(&a, "big", 0.0, 0.0..=1.0).is_err());
        assert_eq!(integer(&a, "n", 1, 2..=8), Ok(5));
        assert!(integer(&a, "coverage", 1, 0..=8).is_err());
        assert_eq!(flag(&a, "on", false), Ok(true));
        assert!(flag(&a, "n", false).is_err());
        assert_eq!(color(&a, "c"), Ok(Some([0xFF, 0x80, 0x00, 0xFF])));
        assert_eq!(color(&a, "none"), Ok(None));
        assert!(color(&a, "n").is_err());
        let units = [("mm", 1), ("cm", 10)];
        assert_eq!(choice(&a, "m", None, &units), Ok(10));
        assert_eq!(choice(&a, "x", Some(1), &units), Ok(1));
        assert!(choice(&a, "x", None, &units)
            .unwrap_err()
            .contains("mm, cm"));
        assert!(choice(&a, "c", None, &units).is_err());
    }

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
