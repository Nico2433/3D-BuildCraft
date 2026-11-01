//! Boolean operations ("Merge", "Subtract", "Intersect") through manifold-rust, a pure
//! Rust port of Manifold, so they build on every platform without a C++ toolchain.

use crate::mesh::Mesh;
use anyhow::{bail, Result};
use glam::Vec3;
use manifold_rust::manifold::Manifold;
use manifold_rust::types::{Error, MeshGL};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Union,
    Subtract,
    Intersect,
}

/// Applies `op` left to right: `first op second op third…`. Inputs must be closed and
/// manifold (repair them first). Each result triangle keeps the painted colour and
/// texture of the input triangle it lies on (manifold's face ids carry which one), with
/// UVs found barycentrically there.
pub fn apply(op: Op, meshes: &[Mesh]) -> Result<Mesh> {
    let mut base = 0u32;
    let mut parts = Vec::new();
    for m in meshes {
        parts.push(to_manifold_with_ids(m, base));
        base += m.triangles.len() as u32;
    }
    let mut parts = parts.into_iter();
    let Some(first) = parts.next() else {
        bail!("No objects")
    };
    let mut acc = first?;
    for m in parts {
        let m = m?;
        acc = match op {
            Op::Union => acc.union(&m),
            Op::Subtract => acc.difference(&m),
            Op::Intersect => acc.intersection(&m),
        };
    }
    Ok(from_manifold_of(&acc, meshes))
}

/// Like `to_manifold`, numbering the triangles from `base` so the result can say which
/// input triangle each of its own lies on.
fn to_manifold_with_ids(m: &Mesh, base: u32) -> Result<Manifold> {
    let gl = MeshGL {
        num_prop: 3,
        vert_properties: m.positions.iter().flat_map(|p| p.to_array()).collect(),
        tri_verts: m.triangles.iter().flatten().copied().collect(),
        face_id: (base..base + m.triangles.len() as u32).collect(),
        ..Default::default()
    };
    checked(Manifold::from_mesh_gl(&gl))
}

fn checked(man: Manifold) -> Result<Manifold> {
    match man.status() {
        Error::NoError => Ok(man),
        e => bail!("The mesh is not valid for boolean operations ({e:?}); repair it first"),
    }
}

/// The mesh of a boolean result, with colours and textures taken from `sources`, the
/// operands in order.
fn from_manifold_of(m: &Manifold, sources: &[Mesh]) -> Mesh {
    let gl = m.get_mesh_gl(-1);
    let mut out = from_manifold(m);
    let any_colors = sources.iter().any(|s| s.has_colors());
    let any_tex = sources.iter().any(|s| s.has_tex());
    if gl.face_id.len() != out.triangles.len() || !(any_colors || any_tex) {
        return out;
    }
    // First global triangle number of each operand.
    let starts: Vec<usize> = sources
        .iter()
        .scan(0, |n, s| {
            let at = *n;
            *n += s.triangles.len();
            Some(at)
        })
        .collect();
    let (mut colors, mut tex) = (Vec::new(), Vec::new());
    for (i, id) in gl.face_id.iter().enumerate() {
        let id = *id as usize;
        let k = starts.partition_point(|s| *s <= id).saturating_sub(1);
        let (src, local) = (&sources[k], id - starts[k]);
        colors.push(src.face_color(local).unwrap_or([0; 4]));
        let own = src.face_tex(local);
        tex.push(own.map_or_else(Default::default, |t| {
            let corners = |m: &Mesh, tri: [u32; 3]| tri.map(|v| m.positions[v as usize]);
            t.within(
                corners(src, src.triangles[local]),
                corners(&out, out.triangles[i]),
            )
        }));
    }
    if any_colors && colors.iter().any(|c| c[3] > 0) {
        out.colors = colors;
    }
    if any_tex && tex.iter().any(|t| t.layer > 0) {
        out.tex = tex;
    }
    out
}

pub(crate) fn to_manifold(m: &Mesh) -> Result<Manifold> {
    let gl = MeshGL {
        num_prop: 3,
        vert_properties: m.positions.iter().flat_map(|p| p.to_array()).collect(),
        tri_verts: m.triangles.iter().flatten().copied().collect(),
        ..Default::default()
    };
    checked(Manifold::from_mesh_gl(&gl))
}

pub(crate) fn from_manifold(m: &Manifold) -> Mesh {
    let gl = m.get_mesh_gl(-1);
    let n = gl.num_prop as usize;
    Mesh::new(
        gl.vert_properties
            .chunks_exact(n)
            .map(|p| Vec3::new(p[0], p[1], p[2]))
            .collect(),
        gl.tri_verts
            .chunks_exact(3)
            .map(|t| [t[0], t[1], t[2]])
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::cube;
    use glam::Mat4;

    fn shifted(x: f32) -> Mesh {
        cube(10.0).transformed(&Mat4::from_translation(Vec3::new(x, 0.0, 0.0)))
    }

    fn closed_volume(m: &Mesh) -> f64 {
        let s = m.stats();
        assert!(s.is_closed_manifold(), "{s:?}");
        s.volume
    }

    #[test]
    fn overlapping_cubes() {
        let (a, b) = (cube(10.0), shifted(5.0));
        let v = |op| closed_volume(&apply(op, &[a.clone(), b.clone()]).unwrap());
        assert!((v(Op::Union) - 1500.0).abs() < 1e-2);
        assert!((v(Op::Subtract) - 500.0).abs() < 1e-2);
        assert!((v(Op::Intersect) - 500.0).abs() < 1e-2);
    }

    #[test]
    fn subtract_keeps_textured_outer_faces_and_colors() {
        use crate::texture::apply_box;
        let cube = cube(10.0);
        let all: Vec<usize> = (0..cube.triangles.len()).collect();
        let mut textured = apply_box(&cube, &Mat4::IDENTITY, &all, 1, 10.0);
        textured.colors = vec![[9, 8, 7, 255]; cube.triangles.len()];
        let hole = cube_at(Vec3::new(2.0, 2.0, 7.0), 4.0);
        let out = apply(Op::Subtract, &[textured, hole]).unwrap();
        closed_volume(&out);
        let n = out.triangles.len();
        let with = (0..n).filter(|&i| out.face_tex(i).is_some()).count();
        // The hole's walls have none; the rest of the cube keeps its picture and colour.
        assert!(with > 0 && with < n, "{with} of {n}");
        assert!(out.face_color(0).is_some() || out.colors.iter().any(|c| c[3] > 0));
        for t in out.tex.iter().filter(|t| t.layer > 0) {
            assert_eq!(t.layer, 1);
            assert!(t.uv.iter().flatten().all(|u| (-1.01..=1.01).contains(u)));
        }
    }

    fn cube_at(at: Vec3, side: f32) -> Mesh {
        cube(side).transformed(&Mat4::from_translation(at))
    }

    #[test]
    fn open_mesh_is_rejected() {
        let mut open = cube(10.0);
        open.triangles.pop();
        assert!(apply(Op::Union, &[open, shifted(5.0)]).is_err());
    }
}
