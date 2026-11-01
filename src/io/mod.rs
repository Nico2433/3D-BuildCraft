//! Model file formats. Conventions follow what the original app writes
//! (vault: Projects/3d-builder/docs/data-model, findings H32–H33 and H43–H45).

pub mod gltf_io;
pub mod obj;
pub mod ply;
pub mod stl;
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
        }
    }
}

pub const OPEN_EXTENSIONS: [&str; 6] = ["3mf", "stl", "obj", "ply", "gltf", "glb"];

/// The file name without extension, "Object" when it has none.
pub fn file_stem(path: &Path) -> &str {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Object")
}

pub fn load(path: &Path) -> Result<LoadedModel> {
    let Some(format) = Format::from_path(path) else {
        bail!("Unsupported format: {}", path.display());
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
    };
    let units_known = matches!(format, Format::ThreeMf | Format::Gltf | Format::Glb);
    Ok(LoadedModel {
        objects,
        info,
        units_known,
    })
}

pub fn save(path: &Path, objects: &[ModelObject], info: &ModelInfo) -> Result<()> {
    let Some(format) = Format::from_path(path) else {
        bail!("Unsupported format: {}", path.display());
    };
    match format {
        Format::ThreeMf => threemf::write(path, objects, info),
        Format::Stl => stl::write(path, objects),
        Format::Obj => obj::write(path, objects),
        Format::Ply => ply::write(path, objects),
        Format::Gltf => gltf_io::write(path, objects, false),
        Format::Glb => gltf_io::write(path, objects, true),
    }
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
        None => format!("Color_{:02X}{:02X}{:02X}", c[0], c[1], c[2]),
    }
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
