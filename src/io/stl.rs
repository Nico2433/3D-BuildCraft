//! STL: reads binary and ASCII; writes binary with a zeroed header, no colour and
//! transforms baked in, like the original (H33).

use super::{baked, ModelObject};
use crate::mesh::Mesh;
use crate::scene::DEFAULT_COLOR;
use anyhow::{bail, Context, Result};
use glam::{Mat4, Vec3};
use std::io::{BufWriter, Write};
use std::path::Path;

pub fn read(path: &Path, name: &str) -> Result<ModelObject> {
    let data = std::fs::read(path).with_context(|| format!("Could not read {}", path.display()))?;
    let mesh = parse(&data)?;
    Ok(ModelObject {
        name: name.to_string(),
        mesh,
        transform: Mat4::IDENTITY,
        color: DEFAULT_COLOR,
    })
}

pub fn parse(data: &[u8]) -> Result<Mesh> {
    if data.len() >= 84 {
        let n = u32::from_le_bytes(data[80..84].try_into().unwrap()) as usize;
        if 84 + 50 * n == data.len() {
            return Ok(parse_binary(data, n));
        }
    }
    let text =
        std::str::from_utf8(data).context("The STL is neither valid binary nor ASCII text")?;
    parse_ascii(text)
}

fn parse_binary(data: &[u8], n: usize) -> Mesh {
    let f = |o: usize| f32::from_le_bytes(data[o..o + 4].try_into().unwrap());
    let soup: Vec<[Vec3; 3]> = (0..n)
        .map(|i| {
            let base = 84 + 50 * i + 12;
            [0, 1, 2].map(|k| {
                let o = base + 12 * k;
                Vec3::new(f(o), f(o + 4), f(o + 8))
            })
        })
        .collect();
    Mesh::from_triangle_soup(&soup)
}

fn parse_ascii(text: &str) -> Result<Mesh> {
    let mut verts = Vec::new();
    for line in text.lines() {
        let mut it = line.split_whitespace();
        if it.next() == Some("vertex") {
            let v: Vec<f32> = it.take(3).map(|s| s.parse()).collect::<Result<_, _>>()?;
            if v.len() != 3 {
                bail!("Incomplete STL vertex: {line}");
            }
            verts.push(Vec3::new(v[0], v[1], v[2]));
        }
    }
    if verts.len() % 3 != 0 {
        bail!("ASCII STL vertex count is not a multiple of 3");
    }
    let soup: Vec<[Vec3; 3]> = verts.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect();
    Ok(Mesh::from_triangle_soup(&soup))
}

pub fn write(path: &Path, objects: &[ModelObject]) -> Result<()> {
    let meshes = baked(objects);
    let count: usize = meshes.iter().map(|m| m.triangles.len()).sum();
    let mut w = BufWriter::new(std::fs::File::create(path)?);
    w.write_all(&[0u8; 80])?;
    w.write_all(&(count as u32).to_le_bytes())?;
    for m in &meshes {
        for [a, b, c] in m.triangle_positions() {
            for v in [Mesh::face_normal(a, b, c), a, b, c] {
                for x in v.to_array() {
                    w.write_all(&x.to_le_bytes())?;
                }
            }
            w.write_all(&[0, 0])?;
        }
    }
    w.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_ascii() {
        let s = "solid t\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid t\n";
        let m = parse(s.as_bytes()).unwrap();
        assert_eq!(m.triangles.len(), 1);
    }
}
