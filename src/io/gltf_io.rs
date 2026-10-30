//! glTF 2.0 / GLB. Positions stay in object space (mm); each node matrix carries the
//! object transform, mm→m and Z-up→Y-up. Colours are linear PBR base colours with
//! metallic 0 and roughness 1, as the original writes them (H45).

use super::{linear_to_srgb, srgb_to_linear, ModelObject};
use crate::mesh::Mesh;
use crate::scene::DEFAULT_COLOR;
use anyhow::{bail, Context, Result};
use glam::{Mat4, Vec3};
use gltf::mesh::Mode;
use serde_json::{json, Value};
use std::path::Path;

/// From the app's frame (mm, Z-up) to glTF's (m, Y-up).
fn to_gltf() -> Mat4 {
    Mat4::from_rotation_x(-std::f32::consts::FRAC_PI_2) * Mat4::from_scale(Vec3::splat(0.001))
}

pub fn write(path: &Path, objects: &[ModelObject], binary: bool) -> Result<()> {
    let mut bin: Vec<u8> = Vec::new();
    let mut accessors = Vec::new();
    let mut views = Vec::new();
    let mut meshes = Vec::new();
    let mut materials = Vec::new();
    let mut nodes = vec![json!({ "children": (1..=objects.len()).collect::<Vec<_>>() })];

    for (i, o) in objects.iter().enumerate() {
        let b = o.mesh.aabb();
        let pos_off = bin.len();
        for p in &o.mesh.positions {
            for x in p.to_array() {
                bin.extend_from_slice(&x.to_le_bytes());
            }
        }
        let idx_off = bin.len();
        for t in &o.mesh.triangles {
            for v in t {
                bin.extend_from_slice(&v.to_le_bytes());
            }
        }
        views.push(json!({ "buffer": 0, "byteOffset": pos_off, "byteLength": idx_off - pos_off, "byteStride": 12, "target": 34962 }));
        views.push(json!({ "buffer": 0, "byteOffset": idx_off, "byteLength": bin.len() - idx_off, "target": 34963 }));
        accessors.push(json!({
            "bufferView": 2 * i, "componentType": 5126, "count": o.mesh.positions.len(), "type": "VEC3",
            "min": b.min.to_array(), "max": b.max.to_array()
        }));
        accessors.push(json!({ "bufferView": 2 * i + 1, "componentType": 5125, "count": o.mesh.triangles.len() * 3, "type": "SCALAR" }));
        meshes.push(json!({ "primitives": [{ "attributes": { "POSITION": 2 * i }, "indices": 2 * i + 1, "mode": 4, "material": i }] }));
        let c = o.color;
        materials.push(json!({
            "name": super::color_name(c),
            "pbrMetallicRoughness": {
                "baseColorFactor": [srgb_to_linear(c[0]), srgb_to_linear(c[1]), srgb_to_linear(c[2]), c[3] as f32 / 255.0],
                "metallicFactor": 0.0, "roughnessFactor": 1.0
            },
            "alphaMode": "OPAQUE"
        }));
        let m = to_gltf() * o.transform;
        nodes.push(json!({ "name": o.name, "mesh": i, "matrix": m.to_cols_array() }));
    }

    let mut buffer = json!({ "byteLength": bin.len() });
    let bin_name = path.with_extension("bin");
    if !binary {
        buffer["uri"] = Value::String(
            bin_name
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("model.bin")
                .to_string(),
        );
    }
    let doc = json!({
        "asset": { "version": "2.0", "generator": "3D BuildCraft" },
        "scene": 0, "scenes": [{ "nodes": [0] }],
        "nodes": nodes, "meshes": meshes, "materials": materials,
        "accessors": accessors, "bufferViews": views, "buffers": [buffer]
    });

    if binary {
        let mut js = serde_json::to_vec(&doc)?;
        while js.len() % 4 != 0 {
            js.push(b' ');
        }
        while bin.len() % 4 != 0 {
            bin.push(0);
        }
        let total = 12 + 8 + js.len() + 8 + bin.len();
        let mut out = Vec::with_capacity(total);
        out.extend_from_slice(b"glTF");
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&(total as u32).to_le_bytes());
        out.extend_from_slice(&(js.len() as u32).to_le_bytes());
        out.extend_from_slice(b"JSON");
        out.extend_from_slice(&js);
        out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        out.extend_from_slice(b"BIN\0");
        out.extend_from_slice(&bin);
        std::fs::write(path, out)?;
    } else {
        std::fs::write(path, serde_json::to_vec_pretty(&doc)?)?;
        std::fs::write(bin_name, bin)?;
    }
    Ok(())
}

pub fn read(path: &Path) -> Result<Vec<ModelObject>> {
    let (doc, buffers, _) =
        gltf::import(path).with_context(|| format!("Could not read {}", path.display()))?;
    let from_gltf = to_gltf().inverse();
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Object")
        .to_string();
    let mut out = Vec::new();
    let scene = doc.default_scene().or_else(|| doc.scenes().next());
    let Some(scene) = scene else {
        bail!("The glTF has no scenes")
    };
    // Pushed in reverse so the stack pops nodes in file order.
    let mut stack: Vec<(gltf::Node, Mat4)> = scene.nodes().map(|n| (n, Mat4::IDENTITY)).collect();
    stack.reverse();
    while let Some((node, parent)) = stack.pop() {
        let world = parent * Mat4::from_cols_array_2d(&node.transform().matrix());
        if let Some(mesh) = node.mesh() {
            let mut combined = Mesh::default();
            let mut color = None;
            for prim in mesh.primitives() {
                let reader = prim.reader(|b| buffers.get(b.index()).map(|d| &d.0[..]));
                let Some(pos) = reader.read_positions() else {
                    continue;
                };
                let positions: Vec<Vec3> = pos.map(Vec3::from).collect();
                let flat: Vec<u32> = match reader.read_indices() {
                    Some(ix) => ix.into_u32().collect(),
                    None => (0..positions.len() as u32).collect(),
                };
                if flat.iter().any(|&i| i as usize >= positions.len()) {
                    bail!("Vertex index out of range in the glTF");
                }
                let mut triangles = Vec::new();
                match prim.mode() {
                    Mode::Triangles => {
                        triangles.extend(flat.chunks_exact(3).map(|c| [c[0], c[1], c[2]]))
                    }
                    // Every other strip triangle runs backwards; swap two corners to keep winding.
                    Mode::TriangleStrip => {
                        triangles.extend(flat.windows(3).enumerate().map(|(k, w)| {
                            if k % 2 == 0 {
                                [w[0], w[1], w[2]]
                            } else {
                                [w[1], w[0], w[2]]
                            }
                        }))
                    }
                    Mode::TriangleFan => super::fan(&flat, &mut triangles),
                    _ => continue,
                }
                combined.append(&Mesh::new(positions, triangles));
                if color.is_none() {
                    let f = prim.material().pbr_metallic_roughness().base_color_factor();
                    color = Some([
                        linear_to_srgb(f[0]),
                        linear_to_srgb(f[1]),
                        linear_to_srgb(f[2]),
                        (f[3] * 255.0).round() as u8,
                    ]);
                }
            }
            // The original writes one primitive per part with its own copy of the shared
            // vertices; weld them so the object is closed again.
            if mesh.primitives().len() > 1 {
                combined = combined.welded();
            }
            if !combined.is_empty() {
                out.push(ModelObject {
                    name: node.name().or(mesh.name()).unwrap_or(&stem).to_string(),
                    mesh: combined,
                    transform: from_gltf * world,
                    color: color.unwrap_or(DEFAULT_COLOR),
                });
            }
        }
        let children: Vec<_> = node.children().map(|c| (c, world)).collect();
        stack.extend(children.into_iter().rev());
    }
    if out.is_empty() {
        bail!("The glTF has no triangle meshes");
    }
    Ok(out)
}
