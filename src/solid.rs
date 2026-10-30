//! Solid edits through manifold-rust: smooth, hollow and extrude down. Inputs must be
//! closed and manifold, like for booleans.

use crate::boolean::{from_manifold, to_manifold};
use crate::mesh::{Dsu, Mesh};
use anyhow::{bail, Result};
use glam::Vec3;
use manifold_rust::linalg::Vec3 as MVec3;
use manifold_rust::manifold::Manifold;
use manifold_rust::types::Box as MBox;
use std::collections::HashMap;

/// Above this, smoothing (which makes 4× the triangles) takes too long.
pub const SMOOTH_MAX_TRIANGLES: usize = 250_000;

/// Rounds the surface: edges sharper than 60° stay sharp, every face is split in four.
pub fn smooth(mesh: &Mesh) -> Result<Mesh> {
    if mesh.triangles.len() > SMOOTH_MAX_TRIANGLES {
        bail!(
            "Too many triangles to smooth ({}); simplify the model first",
            mesh.triangles.len()
        );
    }
    Ok(from_manifold(
        &to_manifold(mesh)?.smooth_out(60.0, 0.0).refine(2),
    ))
}

/// Rounds every edge by `amount` (0 sharp .. 1 fully round), for Custom's "Roundness".
/// The result fits the original's bounding box.
pub fn round(mesh: &Mesh, amount: f32) -> Result<Mesh> {
    if amount <= 0.0 {
        return Ok(mesh.clone());
    }
    // Rounding makes 16× the triangles; past this it takes too long to preview.
    if mesh.triangles.len() > 60_000 {
        bail!(
            "Too many faces to round ({}); use fewer subdivisions",
            mesh.triangles.len()
        );
    }
    let smooth = to_manifold(mesh)?.smooth_out(0.0, amount.clamp(0.0, 1.0) as f64);
    let rounded = from_manifold(&smooth.refine(4));
    // Smoothing bulges the faces outwards; fit the result back into the original box so
    // the shape keeps its size and stays on the floor.
    let (from, to) = (rounded.aabb(), mesh.aabb());
    let k = (to.max - to.min) / (from.max - from.min).max(Vec3::splat(1e-6));
    Ok(rounded.transformed(
        &(glam::Mat4::from_translation(to.min)
            * glam::Mat4::from_scale(k)
            * glam::Mat4::from_translation(-from.min)),
    ))
}

/// Removes the inside of `mesh`, leaving a closed wall at least `wall` thick.
pub fn hollow(mesh: &Mesh, wall: f32) -> Result<Mesh> {
    let solid = to_manifold(mesh)?;
    let inside = inner(mesh, wall);
    if inside.is_empty() {
        bail!("The object is too thin to hollow with a {wall} mm wall");
    }
    Ok(from_manifold(&solid.difference(&inside)))
}

/// Fills the space under every downward face down to `floor` (or the object's lowest
/// point when it reaches below `floor`).
pub fn extrude_down(mesh: &Mesh, floor: f32) -> Result<Mesh> {
    let solid = to_manifold(mesh)?;
    let floor = floor.min(mesh.aabb().min.z);
    // Skirts reach below the floor and are trimmed there, so none is flat.
    let mut parts = skirts(mesh, floor - 1.0)
        .iter()
        .map(to_manifold)
        .collect::<Result<Vec<_>>>()?;
    // Pairwise, so each union stays small.
    while parts.len() > 1 {
        parts = parts
            .chunks(2)
            .map(|c| match c {
                [a, b] => a.union(b),
                [a] => a.clone(),
                _ => unreachable!(),
            })
            .collect();
    }
    let out = match parts.pop() {
        Some(skirt) => solid.union(&skirt),
        None => solid,
    };
    Ok(from_manifold(
        &out.trim_by_plane(MVec3::new(0.0, 0.0, 1.0), floor as f64),
    ))
}

/// Most voxels along the longest side; keeps memory and time bounded on big models.
const MAX_CELLS: f32 = 160.0;

/// Voxels the inner surface is pulled in by: the distance to the nearest outside voxel
/// centre overshoots the true depth by about √3 voxels, the trilinear sample by up to
/// √3/2 more, and the line jitter by a little.
const MARGIN: f32 = 2.7;

const JITTER: (f32, f32) = (0.013_7, 0.029_1);

/// The region deeper than `wall` inside `mesh`, as a solid.
fn inner(mesh: &Mesh, wall: f32) -> Manifold {
    let b = mesh.aabb();
    let size = b.max - b.min;
    // shortcut: the inner surface is only as exact as the grid, so it is pulled in by
    // MARGIN voxels and the wall comes out up to that much thicker, never thinner; big
    // models get coarse voxels and thick walls until there is a sparse grid.
    let h = (wall / 4.0).max(size.max_element() / MAX_CELLS);
    // One outside voxel on every side, so every line starts and ends outside.
    let n = ((size / h).ceil().as_uvec3() + 2)
        .to_array()
        .map(|v| v as usize);
    let origin = b.min - Vec3::splat(h * 0.75);
    let center = |i: usize, axis: usize| origin[axis] + (i as f32 + 0.5) * h;
    let idx = |x: usize, y: usize, z: usize| (z * n[1] + y) * n[0] + x;

    // Crossings of each vertical line through voxel centres with the surface.
    let mut cross: Vec<Vec<f32>> = vec![Vec::new(); n[0] * n[1]];
    for t in &mesh.triangles {
        let [a, b2, c] = t.map(|i| mesh.positions[i as usize]);
        let lo = a.min(b2).min(c);
        let hi = a.max(b2).max(c);
        let x0 = (((lo.x - origin.x) / h - 0.5).ceil().max(0.0)) as usize;
        let y0 = (((lo.y - origin.y) / h - 0.5).ceil().max(0.0)) as usize;
        let x1 = ((((hi.x - origin.x) / h - 0.5).floor()) as usize).min(n[0] - 1);
        let y1 = ((((hi.y - origin.y) / h - 0.5).floor()) as usize).min(n[1] - 1);
        let d = (b2.x - a.x) * (c.y - a.y) - (c.x - a.x) * (b2.y - a.y);
        if d.abs() < 1e-12 {
            continue;
        }
        for y in y0..=y1 {
            for x in x0..=x1 {
                // Off-grid by an odd fraction so lines miss the vertices and edges of
                // models built on round numbers.
                let (px, py) = (center(x, 0) + h * JITTER.0, center(y, 1) + h * JITTER.1);
                let u = ((b2.x - px) * (c.y - py) - (c.x - px) * (b2.y - py)) / d;
                let v = ((c.x - px) * (a.y - py) - (a.x - px) * (c.y - py)) / d;
                let w = 1.0 - u - v;
                if u > 0.0 && v > 0.0 && w > 0.0 {
                    cross[y * n[0] + x].push(u * a.z + v * b2.z + w * c.z);
                }
            }
        }
    }

    // Squared distance in voxels to the nearest outside voxel.
    const FAR: f32 = 1e10;
    let mut dist = vec![0.0f32; n[0] * n[1] * n[2]];
    for y in 0..n[1] {
        for x in 0..n[0] {
            let zs = &mut cross[y * n[0] + x];
            zs.sort_by(f32::total_cmp);
            for pair in zs.chunks_exact(2) {
                for z in 0..n[2] {
                    let pz = center(z, 2);
                    if pz > pair[0] && pz < pair[1] {
                        dist[idx(x, y, z)] = FAR;
                    }
                }
            }
        }
    }
    for axis in 0..3 {
        let (len, stride) = match axis {
            0 => (n[0], 1),
            1 => (n[1], n[0]),
            _ => (n[2], n[0] * n[1]),
        };
        let mut line = vec![0.0f32; len];
        let mut out = vec![0.0f32; len];
        for start in 0..dist.len() {
            // Visit each line once, from its first voxel.
            if (start / stride) % len != 0 {
                continue;
            }
            for i in 0..len {
                line[i] = dist[start + i * stride];
            }
            edt_1d(&line, &mut out);
            for i in 0..len {
                dist[start + i * stride] = out[i];
            }
        }
    }

    let sdf = |p: MVec3| -> f64 {
        // Trilinear sample of the distance (mm), minus the wall.
        let g = [
            (p.x as f32 - origin.x) / h - 0.5,
            (p.y as f32 - origin.y) / h - 0.5,
            (p.z as f32 - origin.z) / h - 0.5,
        ];
        let mut acc = 0.0;
        let i0 = g.map(|v| v.floor().clamp(0.0, 1e9) as usize);
        let f = [0, 1, 2].map(|k| (g[k] - i0[k] as f32).clamp(0.0, 1.0));
        for corner in 0..8 {
            let i = [0, 1, 2].map(|k| (i0[k] + (corner >> k & 1)).min(n[k] - 1));
            let wgt: f32 = (0..3)
                .map(|k| {
                    if corner >> k & 1 == 1 {
                        f[k]
                    } else {
                        1.0 - f[k]
                    }
                })
                .product();
            acc += wgt * dist[idx(i[0], i[1], i[2])].sqrt() * h;
        }
        (acc - wall - h * MARGIN) as f64
    };
    let bounds = MBox {
        min: MVec3::new(b.min.x as f64, b.min.y as f64, b.min.z as f64),
        max: MVec3::new(b.max.x as f64, b.max.y as f64, b.max.z as f64),
    };
    Manifold::level_set(sdf, bounds, h as f64)
}

/// Felzenszwalb–Huttenlocher squared distance transform of one line.
fn edt_1d(f: &[f32], d: &mut [f32]) {
    let n = f.len();
    let mut v = vec![0usize; n];
    let mut z = vec![0.0f64; n + 1];
    let (mut k, f) = (0, |i: usize| f[i] as f64);
    z[0] = f64::NEG_INFINITY;
    z[1] = f64::INFINITY;
    let s = |q: usize, p: usize| {
        ((f(q) + (q * q) as f64) - (f(p) + (p * p) as f64)) / (2 * (q - p)) as f64
    };
    for q in 1..n {
        while s(q, v[k]) <= z[k] {
            k -= 1;
        }
        k += 1;
        v[k] = q;
        z[k] = s(q, v[k - 1]);
        z[k + 1] = f64::INFINITY;
    }
    k = 0;
    for (q, out) in d.iter_mut().enumerate() {
        while z[k + 1] < q as f64 {
            k += 1;
        }
        let p = v[k];
        *out = ((q as f64 - p as f64).powi(2) + f(p)) as f32;
    }
}

/// One closed solid per connected patch of downward faces: the patch on top, its
/// projection at `bottom`, and walls along the patch border.
fn skirts(mesh: &Mesh, bottom: f32) -> Vec<Mesh> {
    let p = &mesh.positions;
    let down: Vec<bool> = mesh
        .triangles
        .iter()
        .map(|t| {
            let [a, b, c] = t.map(|i| p[i as usize]);
            (b - a).cross(c - a).z < -1e-9
        })
        .collect();
    let mut edge_tri: HashMap<(u32, u32), usize> = HashMap::new();
    for (i, t) in mesh.triangles.iter().enumerate() {
        if down[i] {
            for k in 0..3 {
                edge_tri.insert((t[k], t[(k + 1) % 3]), i);
            }
        }
    }
    let mut seen = vec![false; mesh.triangles.len()];
    let mut out = Vec::new();
    for start in 0..mesh.triangles.len() {
        if !down[start] || seen[start] {
            continue;
        }
        let mut patch = vec![start];
        seen[start] = true;
        let mut i = 0;
        while i < patch.len() {
            let t = mesh.triangles[patch[i]];
            for k in 0..3 {
                if let Some(&n) = edge_tri.get(&(t[(k + 1) % 3], t[k])) {
                    if !seen[n] {
                        seen[n] = true;
                        patch.push(n);
                    }
                }
            }
            i += 1;
        }
        // One vertex per corner group: corners of a vertex join only across patch edges,
        // so a patch that touches itself at a vertex gets one copy per side.
        let tris: Vec<[u32; 3]> = patch.iter().map(|&t| mesh.triangles[t]).collect();
        let at: HashMap<(u32, u32), usize> = tris
            .iter()
            .enumerate()
            .flat_map(|(i, t)| (0..3).map(move |k| ((t[k], t[(k + 1) % 3]), 3 * i + k)))
            .collect();
        let mut dsu = Dsu::new(3 * tris.len());
        for (&(a, b), &c) in &at {
            if let Some(&r) = at.get(&(b, a)) {
                // `c` starts at `a`; the reverse edge's corner at `a` is the one after `r`.
                dsu.union(c, r - r % 3 + (r + 1) % 3);
            }
        }
        let mut local: HashMap<usize, u32> = HashMap::new();
        let mut positions = Vec::new();
        let mut id = |corner: usize| {
            let v = p[tris[corner / 3][corner % 3] as usize];
            *local.entry(dsu.find(corner)).or_insert_with(|| {
                positions.push(v);
                positions.push(Vec3::new(v.x, v.y, bottom));
                positions.len() as u32 - 2
            })
        };
        let mut triangles = Vec::new();
        for i in 0..tris.len() {
            let [a, b, c] = [0, 1, 2].map(|k| id(3 * i + k));
            triangles.push([a, c, b]);
            triangles.push([a + 1, b + 1, c + 1]);
        }
        for (&(a, b), &c) in &at {
            if !at.contains_key(&(b, a)) {
                let (a, b) = (id(c), id(c - c % 3 + (c + 1) % 3));
                triangles.push([a, b, b + 1]);
                triangles.push([a, b + 1, a + 1]);
            }
        }
        drop(id);
        out.push(Mesh::new(positions, triangles));
    }
    out
}
