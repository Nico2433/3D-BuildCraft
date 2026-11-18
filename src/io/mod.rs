//! Model file formats. Conventions follow what the original app writes
//! (vault: Projects/3d-builder/docs/data-model, findings H32–H33 and H43–H45).

pub mod gltf_io;
pub mod obj;
pub mod ply;
pub mod stl;
pub mod svg;
pub mod threemf;

use crate::mesh::Mesh;
use anyhow::{bail, Result};
use glam::Mat4;
use std::path::Path;

pub type Rgba = [u8; 4];

/// One object as stored in a file: mesh in object space plus its placement.
#[derive(Clone, Debug)]
pub struct ModelObject {
    pub name: String,
    pub mesh: Mesh,
    pub transform: Mat4,
    pub color: Rgba,
}

/// What a file says about the model ("About the model"); 3MF keeps it as metadata.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModelInfo {
    pub title: String,
    pub designer: String,
    pub copyright: String,
    pub license_terms: String,
    pub description: String,
}

impl ModelInfo {
    /// The 3MF metadata names, each with its field.
    pub fn fields_mut(&mut self) -> [(&'static str, &mut String); 5] {
        [
            ("Title", &mut self.title),
            ("Designer", &mut self.designer),
            ("Copyright", &mut self.copyright),
            ("LicenseTerms", &mut self.license_terms),
            ("Description", &mut self.description),
        ]
    }

    pub fn fields(&self) -> [(&'static str, &str); 5] {
        [
            ("Title", &self.title),
            ("Designer", &self.designer),
            ("Copyright", &self.copyright),
            ("LicenseTerms", &self.license_terms),
            ("Description", &self.description),
        ]
    }
}

pub struct LoadedModel {
    pub objects: Vec<ModelObject>,
    /// Empty for formats without it.
    pub info: ModelInfo,
    /// False for formats without units (STL, OBJ, PLY): the import screen asks the user.
    pub units_known: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    ThreeMf,
    Stl,
    Obj,
    Ply,
    Gltf,
    Glb,
    /// Import only: a flat drawing extruded.
    Svg,
}

impl Format {
    pub const SAVE: [Format; 6] = [
        Format::ThreeMf,
        Format::Stl,
        Format::Obj,
        Format::Ply,
        Format::Glb,
        Format::Gltf,
    ];

    pub fn from_path(path: &Path) -> Option<Format> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        Some(match ext.as_str() {
            "3mf" => Format::ThreeMf,
            "stl" => Format::Stl,
            "obj" => Format::Obj,
            "ply" => Format::Ply,
            "gltf" => Format::Gltf,
            "glb" => Format::Glb,
            "svg" => Format::Svg,
            _ => return None,
        })
    }

    pub fn extension(self) -> &'static str {
        match self {
            Format::ThreeMf => "3mf",
            Format::Stl => "stl",
            Format::Obj => "obj",
            Format::Ply => "ply",
            Format::Gltf => "gltf",
            Format::Glb => "glb",
            Format::Svg => "svg",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Format::ThreeMf => "3D Manufacturing Format (3MF)",
            Format::Stl => "STL",
            Format::Obj => "OBJ",
            Format::Ply => "PLY",
            Format::Gltf => "glTF",
            Format::Glb => "GLB",
            Format::Svg => "SVG",
        }
    }
}

pub const OPEN_EXTENSIONS: [&str; 7] = ["3mf", "stl", "obj", "ply", "gltf", "glb", "svg"];
/// The model formats that open, for messages.
pub const OPEN_FORMATS: &str = "3MF, STL, OBJ, PLY, glTF, GLB or SVG";
/// The formats a scene saves as ([`Format::SAVE`]), for messages.
pub const SAVE_FORMATS: &str = "3MF, STL, OBJ, PLY, GLB or glTF";

/// The file name without extension, "Object" when it has none.
pub fn file_stem(path: &Path) -> &str {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Object")
}

/// The largest an object may be along one axis, in mm: the limit of the tools and of what a
/// file that carries its own units may hold.
pub const MAX_SIZE_MM: f32 = 10_000.0;

/// The size along each axis of the box round all `objects` placed, in mm.
pub fn extent(objects: &[ModelObject]) -> glam::Vec3 {
    let mut bounds = crate::mesh::Aabb::empty();
    for o in objects.iter().filter(|o| !o.mesh.is_empty()) {
        let b = o.mesh.aabb();
        for i in 0..8 {
            let corner = glam::Vec3::select(
                glam::BVec3::new(i & 1 != 0, i & 2 != 0, i & 4 != 0),
                b.max,
                b.min,
            );
            bounds.grow(o.transform.transform_point3(corner));
        }
    }
    // Not `is_valid`: an infinite box is a size too, and the one to refuse.
    if bounds.min.cmple(bounds.max).all() {
        bounds.size()
    } else {
        glam::Vec3::ZERO
    }
}

/// An error naming the limit when a model of this `size` in mm is larger than
/// [`MAX_SIZE_MM`] along any side (or has no size).
pub fn check_size(size: glam::Vec3) -> Result<()> {
    if size.is_nan() || size.max_element() > MAX_SIZE_MM {
        bail!(
            "The model is {:.0} × {:.0} × {:.0} mm; the most is {MAX_SIZE_MM} mm along each side",
            size.x,
            size.y,
            size.z
        );
    }
    Ok(())
}

/// What a file read must hold: objects with triangles (empty ones are dropped), and, when
/// the format carries its units, none larger than [`MAX_SIZE_MM`].
fn checked(
    mut objects: Vec<ModelObject>,
    format: Format,
    units_known: bool,
) -> Result<Vec<ModelObject>> {
    objects.retain(|o| !o.mesh.is_empty());
    if objects.is_empty() {
        bail!(
            "The file has no triangles: it is empty, or not a valid {} model",
            format.label()
        );
    }
    if units_known {
        check_size(extent(&objects))?;
    }
    Ok(objects)
}

pub fn load(path: &Path) -> Result<LoadedModel> {
    let Some(format) = Format::from_path(path) else {
        bail!(
            "Unsupported format: {}. Models open as {OPEN_FORMATS}",
            path.display()
        );
    };
    let stem = file_stem(path).to_string();
    let mut info = ModelInfo::default();
    let objects = match format {
        Format::ThreeMf => {
            let (objects, read) = threemf::read(path)?;
            info = read;
            objects
        }
        Format::Stl => vec![stl::read(path, &stem)?],
        Format::Obj => obj::read(path)?,
        Format::Ply => vec![ply::read(path, &stem)?],
        Format::Gltf | Format::Glb => gltf_io::read(path)?,
        Format::Svg => svg::read(path)?,
    };
    let units_known = matches!(
        format,
        Format::ThreeMf | Format::Gltf | Format::Glb | Format::Svg
    );
    let objects = checked(objects, format, units_known)?;
    Ok(LoadedModel {
        objects,
        info,
        units_known,
    })
}

pub fn save(path: &Path, objects: &[ModelObject], info: &ModelInfo) -> Result<()> {
    let Some(format) = Format::from_path(path) else {
        bail!(
            "Unsupported format: {}. Scenes save as {SAVE_FORMATS}",
            path.display()
        );
    };
    let written = match format {
        Format::ThreeMf => threemf::write(path, objects, info),
        Format::Stl => stl::write(path, objects),
        Format::Obj => obj::write(path, objects),
        Format::Ply => ply::write(path, objects),
        Format::Gltf => gltf_io::write(path, objects, false),
        Format::Glb => gltf_io::write(path, objects, true),
        Format::Svg => bail!("SVG files can only be imported; save as {SAVE_FORMATS}"),
    };
    written.map_err(|e| {
        let denied = e.root_cause().downcast_ref::<std::io::Error>().is_some_and(|io| {
            matches!(
                io.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::PermissionDenied
            )
        });
        if !denied {
            return e;
        }
        let folder = path.parent().unwrap_or(Path::new("."));
        // A folder the app cannot see is not the same as one it may not write in.
        let why = if folder.is_dir() {
            "is not writable for the app"
        } else {
            "does not exist for the app (a Flatpak app sees the real home folder but has its own private /tmp)"
        };
        anyhow::anyhow!(
            "{e:#}. The folder {} {why}; save in an existing folder that is writable, e.g. the home folder",
            folder.display()
        )
    })
}

/// Objects with their transforms applied, for formats that store world coordinates.
pub fn baked(objects: &[ModelObject]) -> Vec<Mesh> {
    objects
        .iter()
        .map(|o| o.mesh.transformed(&o.transform))
        .collect()
}

/// Distinct object colours in first-use order; formats index materials into this list.
pub(crate) fn unique_colors(objects: &[ModelObject]) -> Vec<Rgba> {
    let mut colors = Vec::new();
    let mut add = |c: Rgba| {
        if !colors.contains(&c) {
            colors.push(c);
        }
    };
    for o in objects {
        add(o.color);
        // Painted areas too.
        for i in 0..o.mesh.triangles.len() {
            if let Some(c) = o.mesh.face_color(i) {
                add(c);
            }
        }
    }
    colors
}

/// Splits a convex polygon into triangles around its first vertex.
pub(crate) fn fan(idx: &[u32], out: &mut Vec<[u32; 3]>) {
    for k in 1..idx.len().saturating_sub(1) {
        out.push([idx[0], idx[k], idx[k + 1]]);
    }
}

/// Named colours the original uses in 3MF `basematerials`.
pub fn color_name(c: Rgba) -> String {
    match crate::scene::PALETTE.iter().find(|(_, rgba)| *rgba == c) {
        Some((name, _)) => (*name).to_string(),
        None => format!("Color_{}", hex6(c)),
    }
}

/// "DC4E62": a colour's red, green and blue as hex digits.
pub fn hex6(c: Rgba) -> String {
    format!("{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

/// "DC4E62" or "#dc4e62" as an opaque colour.
pub fn parse_hex(s: &str) -> Option<Rgba> {
    let s = s.trim();
    let s = s.strip_prefix('#').unwrap_or(s);
    if s.len() != 6 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let v = u32::from_str_radix(s, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8, 0xFF])
}

pub fn srgb_to_linear(c: u8) -> f32 {
    let c = c as f32 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

pub fn linear_to_srgb(v: f32) -> u8 {
    let v = v.clamp(0.0, 1.0);
    let s = if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::Primitive;
    use glam::Vec3;

    #[test]
    fn hex_round_trip() {
        assert_eq!(parse_hex("#DC4E62"), Some([0xDC, 0x4E, 0x62, 0xFF]));
        assert_eq!(parse_hex(" dc4e62 "), Some([0xDC, 0x4E, 0x62, 0xFF]));
        assert_eq!(parse_hex("dc4e6"), None);
        assert_eq!(parse_hex("+c4e62"), None);
        assert_eq!(hex6([0xDC, 0x4E, 0x62, 0xFF]), "DC4E62");
    }

    fn sample() -> Vec<ModelObject> {
        vec![
            ModelObject {
                name: "Cube".into(),
                mesh: Primitive::Cube.build(),
                transform: Mat4::from_translation(Vec3::new(10.0, -5.0, 0.0)),
                color: crate::scene::DEFAULT_COLOR,
            },
            ModelObject {
                name: "Cilindro".into(),
                mesh: Primitive::Cylinder.build(),
                transform: Mat4::from_scale(Vec3::splat(0.5)),
                color: [0xFE, 0xF1, 0x00, 0xFF],
            },
        ]
    }

    fn world_volume(objs: &[ModelObject]) -> f64 {
        baked(objs).iter().map(|m| m.stats().volume).sum()
    }

    #[test]
    fn every_format_round_trips_geometry() {
        let dir = tempfile::tempdir().unwrap();
        let objs = sample();
        let expected = world_volume(&objs);
        for f in Format::SAVE {
            let path = dir.path().join(format!("roundtrip.{}", f.extension()));
            save(&path, &objs, &ModelInfo::default()).unwrap();
            let loaded = load(&path).unwrap();
            let got = world_volume(&loaded.objects);
            assert!(
                (got - expected).abs() / expected < 1e-4,
                "{f:?}: {got} vs {expected}"
            );
        }
    }

    #[test]
    fn srgb_conversion_matches_the_original_gltf_export() {
        // Turquoise #00A0E8 was written as baseColorFactor [0, 0.3515, 0.8070].
        assert!((srgb_to_linear(0xA0) - 0.3515).abs() < 1e-3);
        assert!((srgb_to_linear(0xE8) - 0.8070).abs() < 1e-3);
        assert_eq!(linear_to_srgb(srgb_to_linear(0xA0)), 0xA0);
    }
}
