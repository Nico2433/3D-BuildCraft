//! Wavefront OBJ + MTL. Writes `o Object.N`, `v`/`f` only, mm, Z-up, baked transforms,
//! and an MTL with `Kd` in sRGB (H43).

use super::{baked, color_name, ModelObject, Rgba};
use crate::mesh::Mesh;
use crate::scene::DEFAULT_COLOR;
use anyhow::{bail, Context, Result};
use glam::{Mat4, Vec3};
use std::collections::HashMap;
use std::io::{BufWriter, Write};
use std::path::Path;

pub fn read(path: &Path) -> Result<Vec<ModelObject>> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("Could not read {}", path.display()))?;
    let dir = path.parent().unwrap_or(Path::new("."));
    let mut materials: HashMap<String, Rgba> = HashMap::new();
    let mut positions: Vec<Vec3> = Vec::new();
    // (name, colour, triangles with global indices)
    let mut groups: Vec<(String, Rgba, Vec<[u32; 3]>)> = Vec::new();
    let mut color = DEFAULT_COLOR;
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Object")
        .to_string();

    for line in text.lines() {
        let line = line.trim();
        let mut it = line.split_whitespace();
        match it.next() {
            Some("v") => {
                let v: Vec<f32> = it.take(3).map(|s| s.parse()).collect::<Result<_, _>>()?;
                if v.len() < 3 {
                    bail!("Incomplete OBJ vertex: {line}");
                }
                positions.push(Vec3::new(v[0], v[1], v[2]));
            }
            Some("o") | Some("g") => {
                let name = it.collect::<Vec<_>>().join(" ");
                groups.push((
                    if name.is_empty() { stem.clone() } else { name },
                    color,
                    Vec::new(),
                ));
            }
            Some("mtllib") => {
                // One name with spaces ("my model.mtl"), else several names.
                let whole = line["mtllib".len()..].trim();
                for lib in std::iter::once(whole).chain(it) {
                    if let Ok(mtl) = std::fs::read_to_string(dir.join(lib)) {
                        materials.extend(parse_mtl(&mtl));
                        break;
                    }
                }
            }
            Some("usemtl") => {
                color = it
                    .next()
                    .and_then(|n| materials.get(n))
                    .copied()
                    .unwrap_or(DEFAULT_COLOR);
                if let Some(g) = groups.last_mut() {
                    if g.2.is_empty() {
                        g.1 = color;
                    }
                }
            }
            Some("f") => {
                let n = positions.len() as i64;
                let idx: Vec<u32> = it
                    .map(|tok| {
                        let i: i64 = tok.split('/').next().unwrap_or("").parse()?;
                        Ok::<u32, std::num::ParseIntError>(if i < 0 {
                            (n + i) as u32
                        } else {
                            (i - 1) as u32
                        })
                    })
                    .collect::<Result<_, _>>()?;
                if groups.is_empty() {
                    groups.push((stem.clone(), color, Vec::new()));
                }
                let tris = &mut groups.last_mut().unwrap().2;
                super::fan(&idx, tris);
            }
            _ => {}
        }
    }

    let mut out = Vec::new();
    for (name, color, tris) in groups.into_iter().filter(|g| !g.2.is_empty()) {
        if tris
            .iter()
            .flatten()
            .any(|i| *i as usize >= positions.len())
        {
            bail!("Face index out of range in {name}");
        }
        // Keep only the vertices this object uses.
        let mut remap: HashMap<u32, u32> = HashMap::new();
        let mut local = Vec::new();
        let triangles = tris
            .iter()
            .map(|t| {
                t.map(|i| {
                    *remap.entry(i).or_insert_with(|| {
                        local.push(positions[i as usize]);
                        (local.len() - 1) as u32
                    })
                })
            })
            .collect();
        out.push(ModelObject {
            name,
            mesh: Mesh::new(local, triangles),
            transform: Mat4::IDENTITY,
            color,
        });
    }
    if out.is_empty() {
        bail!("The OBJ file has no faces");
    }
    Ok(out)
}

fn parse_mtl(text: &str) -> HashMap<String, Rgba> {
    let mut map = HashMap::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        let mut it = line.split_whitespace();
        match it.next() {
            Some("newmtl") => current = it.next().map(str::to_string),
            Some("Kd") => {
                let v: Vec<f32> = it.filter_map(|s| s.parse().ok()).collect();
                if let (Some(name), [r, g, b, ..]) = (&current, v.as_slice()) {
                    let c = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
                    map.insert(name.clone(), [c(*r), c(*g), c(*b), 255]);
                }
            }
            _ => {}
        }
    }
    map
}

pub fn write(path: &Path, objects: &[ModelObject]) -> Result<()> {
    let mtl_path = path.with_extension("mtl");
    let mtl_name = mtl_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("model.mtl")
        .to_string();
    let colors = super::unique_colors(objects);
    let mat_name = |c: Rgba| {
        format!(
            "{}_{}",
            color_name(c),
            colors.iter().position(|x| *x == c).unwrap()
        )
    };

    let mut m = BufWriter::new(std::fs::File::create(&mtl_path)?);
    writeln!(m, "# Exported from 3D BuildCraft\n")?;
    for c in &colors {
        let f = |x: u8| x as f32 / 255.0;
        writeln!(
            m,
            "newmtl {}\nKd {:.6} {:.6} {:.6}\n",
            mat_name(*c),
            f(c[0]),
            f(c[1]),
            f(c[2])
        )?;
    }
    m.flush()?;

    let mut w = BufWriter::new(std::fs::File::create(path)?);
    writeln!(w, "# Exported from 3D BuildCraft\nmtllib {mtl_name}\n")?;
    let mut base = 1u32;
    for (i, (o, mesh)) in objects.iter().zip(baked(objects)).enumerate() {
        writeln!(w, "o Object.{}", i + 1)?;
        for p in &mesh.positions {
            writeln!(w, "v {:.6} {:.6} {:.6}", p.x, p.y, p.z)?;
        }
        writeln!(w, "\nusemtl {}", mat_name(o.color))?;
        for t in &mesh.triangles {
            writeln!(w, "f {} {} {}", t[0] + base, t[1] + base, t[2] + base)?;
        }
        writeln!(w)?;
        base += mesh.positions.len() as u32;
    }
    w.flush()?;
    Ok(())
}
