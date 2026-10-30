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
/// manifold (repair them first).
pub fn apply(op: Op, meshes: &[Mesh]) -> Result<Mesh> {
    let mut parts = meshes.iter().map(to_manifold);
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
    Ok(from_manifold(&acc))
}

pub(crate) fn to_manifold(m: &Mesh) -> Result<Manifold> {
    let gl = MeshGL {
        num_prop: 3,
        vert_properties: m.positions.iter().flat_map(|p| p.to_array()).collect(),
        tri_verts: m.triangles.iter().flatten().copied().collect(),
        ..Default::default()
    };
    let man = Manifold::from_mesh_gl(&gl);
    match man.status() {
        Error::NoError => Ok(man),
        e => bail!("The mesh is not valid for boolean operations ({e:?}); repair it first"),
    }
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
    fn open_mesh_is_rejected() {
        let mut open = cube(10.0);
        open.triangles.pop();
        assert!(apply(Op::Union, &[open, shifted(5.0)]).is_err());
    }
}
