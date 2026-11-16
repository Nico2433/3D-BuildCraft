//! Solid edits through manifold-rust: smooth, hollow and extrude down. Inputs must be
//! closed and manifold, like for booleans.

use crate::boolean::{from_manifold, to_manifold};
use crate::mesh::{Dsu, Mesh};
use crate::texture::FaceTex;
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

/// Most subdivision steps the Smooth slider reaches.
pub const SMOOTH_LEVELS: u32 = 5;
/// Past this many triangles the Smooth tool stops subdividing further.
const SMOOTH_BUDGET: usize = 400_000;

/// The Smooth tool's surface, as the original's: `amount` (0 to 1) sets how many times
/// every face is split in four (up to [`SMOOTH_LEVELS`], fewer for big meshes), so the
/// slider changes how fine the surface is, not its shape. Without `edges` the split is
/// interpolating (butterfly): the original points stay and the shape keeps its size, its
/// edges rounded between them. With `edges` (the original's "UseSubdivision") it is
/// approximating (Loop): every point moves to its average, so edges and corners melt
/// into a rounder, smaller shape. The bottom stays at its height. Points on open or non-manifold edges stay put. Face
/// colours and textures follow their faces.
pub fn smooth_by(mesh: &Mesh, amount: f32, edges: bool) -> Result<Mesh> {
    if mesh.triangles.len() > SMOOTH_MAX_TRIANGLES {
        bail!(
            "Too many triangles to smooth ({}); simplify the model first",
            mesh.triangles.len()
        );
    }
    let mut levels = (amount.clamp(0.0, 1.0) * SMOOTH_LEVELS as f32).round() as u32;
    while levels > 1 && mesh.triangles.len() * 4usize.pow(levels) > SMOOTH_BUDGET {
        levels -= 1;
    }
    if levels == 0 {
        return Ok(mesh.clone());
    }
    // Weld points at the same place, so seams do not open.
    let mut ids: HashMap<[u32; 3], u32> = HashMap::new();
    let mut pos: Vec<Vec3> = Vec::new();
    let remap: Vec<u32> = mesh
        .positions
        .iter()
        .map(|p| {
            *ids.entry(p.to_array().map(f32::to_bits))
                .or_insert_with(|| {
                    pos.push(*p);
                    pos.len() as u32 - 1
                })
        })
        .collect();
    let mut tris: Vec<[u32; 3]> = Vec::new();
    let mut colors: Vec<[u8; 4]> = Vec::new();
    // Textures follow their faces too: a face split in four has the same picture, with
    // the UVs of the new corners halfway between the old ones.
    let mut texs: Vec<FaceTex> = Vec::new();
    for (i, t) in mesh.triangles.iter().enumerate() {
        let t = t.map(|v| remap[v as usize]);
        if t[0] != t[1] && t[1] != t[2] && t[0] != t[2] {
            tris.push(t);
            colors.push(mesh.colors.get(i).copied().unwrap_or([0; 4]));
            texs.push(mesh.face_tex(i).unwrap_or_default());
        }
    }
    center_fans(&mut pos, &mut tris, &mut colors, &mut texs);
    let bottom = pos.iter().map(|p| p.z).fold(f32::INFINITY, f32::min);
    for _ in 0..levels {
        (pos, tris, colors, texs) = subdivide(&pos, &tris, &colors, &texs, edges);
    }
    // Rounding lifts the bottom (or bulges it below); keep it where it was, so a
    // model on the floor stays there.
    let lift = bottom - pos.iter().map(|p| p.z).fold(f32::INFINITY, f32::min);
    pos.iter_mut().for_each(|p| p.z += lift);
    let mut out = Mesh::new(pos, tris);
    if colors.iter().any(|c| c[3] != 0) {
        out.colors = colors;
    }
    if texs.iter().any(|t| t.layer > 0) {
        out.tex = texs;
    }
    Ok(out)
}

/// Rebuilds flat fans from their centre. A cap triangulated as a fan from one rim point
/// (a cylinder's) gives that point a hundred neighbours, and subdivision would pinch a
/// crease from it; triangulated from its centre it rounds evenly, as the original's
/// shapes do. Only flat, single-colour, untextured patches shaped like a disc with a
/// point touching more than eight of their faces change, and only when every new face
/// still faces the same way, so the shape and the paint stay as they are.
fn center_fans(
    pos: &mut Vec<Vec3>,
    tris: &mut Vec<[u32; 3]>,
    colors: &mut Vec<[u8; 4]>,
    texs: &mut Vec<FaceTex>,
) {
    let key = |a: u32, b: u32| (a.min(b), a.max(b));
    let normal = |pos: &[Vec3], t: &[u32; 3]| {
        let [a, b, c] = t.map(|v| pos[v as usize]);
        (b - a).cross(c - a).normalize_or_zero()
    };
    let mut faces: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
    for (i, t) in tris.iter().enumerate() {
        for k in 0..3 {
            faces.entry(key(t[k], t[(k + 1) % 3])).or_default().push(i);
        }
    }
    // Flat patches: faces joined across edges with the same normal and paint.
    let mut patch = Dsu::new(tris.len());
    for f in faces.values() {
        if let &[f1, f2] = f.as_slice() {
            if colors[f1] == colors[f2]
                && texs[f1].layer == 0
                && texs[f2].layer == 0
                && normal(pos, &tris[f1]).dot(normal(pos, &tris[f2])) > 0.99999
            {
                patch.union(f1, f2);
            }
        }
    }
    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..tris.len() {
        groups.entry(patch.find(i)).or_default().push(i);
    }
    let mut drop = vec![false; tris.len()];
    let mut added: Vec<([u32; 3], [u8; 4])> = Vec::new();
    for members in groups.values().filter(|m| m.len() > 8) {
        let mut uses: HashMap<u32, usize> = HashMap::new();
        for &f in members {
            tris[f]
                .iter()
                .for_each(|&v| *uses.entry(v).or_default() += 1);
        }
        if uses.values().all(|&n| n <= 8) {
            continue;
        }
        // The patch's rim, as directed edges a → b of its faces.
        let inside: std::collections::HashSet<usize> = members.iter().copied().collect();
        let mut next: HashMap<u32, u32> = HashMap::new();
        let mut simple = true;
        for &f in members {
            let t = tris[f];
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                let shared = faces[&key(a, b)]
                    .iter()
                    .any(|&g| g != f && inside.contains(&g));
                if !shared && next.insert(a, b).is_some() {
                    simple = false;
                }
            }
        }
        // One loop through every rim point, and a disc: V - E + F = 1.
        let Some(&start) = next.keys().next() else {
            continue;
        };
        let mut ring = vec![start];
        while let Some(&n) = next.get(ring.last().unwrap()) {
            if n == start || ring.len() > next.len() {
                break;
            }
            ring.push(n);
        }
        let edges = (members.len() * 3 + ring.len()) / 2;
        if !simple || ring.len() != next.len() || uses.len() + members.len() != edges + 1 {
            continue;
        }
        let up = normal(pos, &tris[members[0]]);
        let center = ring.iter().map(|&v| pos[v as usize]).sum::<Vec3>() / ring.len() as f32;
        let fan: Vec<[u32; 3]> = (0..ring.len())
            .map(|k| [ring[k], ring[(k + 1) % ring.len()], pos.len() as u32])
            .collect();
        let mut probe = pos.clone();
        probe.push(center);
        if fan.iter().any(|t| normal(&probe, t).dot(up) < 0.999) {
            continue;
        }
        pos.push(center);
        members.iter().for_each(|&f| drop[f] = true);
        added.extend(fan.into_iter().map(|t| (t, colors[members[0]])));
    }
    if added.is_empty() {
        return;
    }
    let mut k = drop.iter();
    tris.retain(|_| !*k.next().unwrap());
    let mut k = drop.iter();
    colors.retain(|_| !*k.next().unwrap());
    let mut k = drop.iter();
    texs.retain(|_| !*k.next().unwrap());
    for (t, c) in added {
        tris.push(t);
        colors.push(c);
        texs.push(FaceTex::default());
    }
}

/// Points, triangles and their colours and textures.
type Split = (Vec<Vec3>, Vec<[u32; 3]>, Vec<[u8; 4]>, Vec<FaceTex>);

/// One step of [`smooth_by`]: every face split in four, the new points placed by the
/// butterfly rule (`approximate` off) or the Loop rules (on).
fn subdivide(
    pos: &[Vec3],
    tris: &[[u32; 3]],
    colors: &[[u8; 4]],
    texs: &[FaceTex],
    approximate: bool,
) -> Split {
    let key = |a: u32, b: u32| (a.min(b), a.max(b));
    // The corners facing each edge.
    let mut facing: HashMap<(u32, u32), Vec<u32>> = HashMap::new();
    for t in tris {
        for k in 0..3 {
            facing
                .entry(key(t[k], t[(k + 1) % 3]))
                .or_default()
                .push(t[(k + 2) % 3]);
        }
    }
    // The corner across edge (a, b) from `not`, when the edge has exactly two faces.
    let across = |a: u32, b: u32, not: u32| -> Option<u32> {
        let f = facing.get(&key(a, b))?;
        (f.len() == 2).then(|| if f[0] == not { f[1] } else { f[0] })
    };
    let mut near: Vec<Vec<u32>> = vec![Vec::new(); pos.len()];
    let mut pinned = vec![false; pos.len()];
    for (&(a, b), f) in &facing {
        near[a as usize].push(b);
        near[b as usize].push(a);
        if f.len() != 2 {
            pinned[a as usize] = true;
            pinned[b as usize] = true;
        }
    }
    let mut out: Vec<Vec3> = pos
        .iter()
        .enumerate()
        .map(|(i, &p)| {
            let n = near[i].len();
            if !approximate || pinned[i] || n < 3 {
                return p;
            }
            // Loop's vertex rule.
            let beta = if n == 3 {
                3.0 / 16.0
            } else {
                3.0 / (8.0 * n as f32)
            };
            p * (1.0 - n as f32 * beta)
                + near[i].iter().map(|&j| pos[j as usize]).sum::<Vec3>() * beta
        })
        .collect();
    let mut mids: HashMap<(u32, u32), u32> = HashMap::new();
    let mut mid = |a: u32, b: u32, out: &mut Vec<Vec3>| -> u32 {
        *mids.entry(key(a, b)).or_insert_with(|| {
            let (pa, pb) = (pos[a as usize], pos[b as usize]);
            let half = (pa + pb) * 0.5;
            let p = match facing.get(&key(a, b)).map(Vec::as_slice) {
                Some(&[c, d]) => {
                    let (pc, pd) = (pos[c as usize], pos[d as usize]);
                    if approximate {
                        (pa + pb) * 0.375 + (pc + pd) * 0.125
                    } else {
                        // The eight-point butterfly; a missing wing falls back to the
                        // plain midpoint.
                        let wings = [
                            across(a, c, b),
                            across(b, c, a),
                            across(a, d, b),
                            across(b, d, a),
                        ];
                        match wings {
                            [Some(e), Some(f), Some(g), Some(h)] => {
                                half + (pc + pd) * 0.125
                                    - (pos[e as usize]
                                        + pos[f as usize]
                                        + pos[g as usize]
                                        + pos[h as usize])
                                        * 0.0625
                            }
                            _ => half,
                        }
                    }
                }
                _ => half,
            };
            out.push(p);
            out.len() as u32 - 1
        })
    };
    let mut next = Vec::with_capacity(tris.len() * 4);
    let mut next_colors = Vec::with_capacity(tris.len() * 4);
    let mut next_texs = Vec::with_capacity(tris.len() * 4);
    for ((t, c), tex) in tris.iter().zip(colors).zip(texs) {
        let [a, b, c3] = *t;
        let (ab, bc, ca) = (
            mid(a, b, &mut out),
            mid(b, c3, &mut out),
            mid(c3, a, &mut out),
        );
        next.extend([[a, ab, ca], [ab, b, bc], [ca, bc, c3], [ab, bc, ca]]);
        next_colors.extend([*c; 4]);
        let [ua, ub, uc] = tex.uv;
        let half = |p: [f32; 2], q: [f32; 2]| [(p[0] + q[0]) / 2.0, (p[1] + q[1]) / 2.0];
        let (uab, ubc, uca) = (half(ua, ub), half(ub, uc), half(uc, ua));
        next_texs.extend(
            [
                [ua, uab, uca],
                [uab, ub, ubc],
                [uca, ubc, uc],
                [uab, ubc, uca],
            ]
            .map(|uv| FaceTex { uv, ..*tex }),
        );
    }
    (out, next, next_colors, next_texs)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::Primitive;

    #[test]
    fn smooth_subdivides_and_edges_round_off() {
        let cube = Primitive::Cube.build();
        let volume = |m: &Mesh| m.stats().volume;
        let size = |m: &Mesh| m.aabb().max - m.aabb().min;
        assert_eq!(smooth_by(&cube, 0.0, true).unwrap(), cube);
        for edges in [false, true] {
            let mid = smooth_by(&cube, 0.5, edges).unwrap();
            let full = smooth_by(&cube, 1.0, edges).unwrap();
            // The slider sets how fine the surface is...
            assert_eq!(mid.triangles.len(), cube.triangles.len() * 4usize.pow(3));
            assert_eq!(full.triangles.len(), cube.triangles.len() * 4usize.pow(5));
            assert!(full.stats().is_closed_manifold());
            // ...not its shape.
            assert!((volume(&mid) - volume(&full)).abs() < 0.05 * volume(&cube));
        }
        // Butterfly goes through the corners, so the size stays; Loop pulls them in.
        let kept = smooth_by(&cube, 0.5, false).unwrap();
        assert!(size(&kept).cmpge(size(&cube) - 1e-3).all());
        let round = smooth_by(&cube, 0.5, true).unwrap();
        assert!(size(&round).max_element() < 0.95 * size(&cube).max_element());
        assert!(volume(&round) < volume(&kept));
        assert_eq!(round.aabb().min.z, cube.aabb().min.z);
        assert_eq!(kept.aabb().min.z, cube.aabb().min.z);
    }

    #[test]
    fn flat_fans_are_rebuilt_from_their_centre() {
        let cylinder = Primitive::Cylinder.build().welded();
        let (mut pos, mut tris) = (cylinder.positions.clone(), cylinder.triangles.clone());
        let n = tris.len();
        let (mut colors, mut texs) = (vec![[0; 4]; n], vec![FaceTex::default(); n]);
        let most = |tris: &[[u32; 3]], points: usize| {
            let mut count = vec![0; points];
            tris.iter().flatten().for_each(|&v| count[v as usize] += 1);
            count
        };
        assert!(most(&tris, pos.len()).into_iter().max().unwrap() > 60);
        center_fans(&mut pos, &mut tris, &mut colors, &mut texs);
        // Two new centre points carry the caps; no rim point keeps a fan.
        assert_eq!(pos.len(), cylinder.positions.len() + 2);
        let count = most(&tris, pos.len());
        assert!(count[..cylinder.positions.len()].iter().all(|&c| c <= 8));
        let rebuilt = Mesh::new(pos, tris);
        assert!(rebuilt.stats().is_closed_manifold());
        let volume = cylinder.stats().volume;
        assert!((rebuilt.stats().volume - volume).abs() < 1e-3 * volume);
        // A cube's faces are two triangles each: left alone.
        let cube = Primitive::Cube.build().welded();
        let (mut p, mut t) = (cube.positions.clone(), cube.triangles.clone());
        let mut c = vec![[0; 4]; t.len()];
        let mut x = vec![FaceTex::default(); t.len()];
        center_fans(&mut p, &mut t, &mut c, &mut x);
        assert_eq!(t, cube.triangles);
    }

    #[test]
    fn smooth_carries_face_colors() {
        let mut ball = Primitive::Sphere.build();
        let palette = [[255, 0, 0, 255], [0, 255, 0, 255]];
        ball.colors = (0..ball.triangles.len()).map(|i| palette[i % 2]).collect();
        let smooth = smooth_by(&ball, 0.2, true).unwrap();
        assert_eq!(smooth.colors.len(), smooth.triangles.len());
        assert!(smooth.colors.iter().all(|c| palette.contains(c)));
        assert!(palette.iter().all(|p| smooth.colors.contains(p)));
    }
}
