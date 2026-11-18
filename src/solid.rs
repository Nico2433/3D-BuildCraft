//! Solid edits: smooth (subdivision and HC smoothing of the mesh itself), and through
//! manifold-rust round, hollow and extrude down, whose inputs must be closed and
//! manifold, like for booleans.

use crate::boolean::{from_manifold, to_manifold};
use crate::mesh::{Dsu, Mesh};
use crate::texture::FaceTex;
use anyhow::{bail, Result};
use glam::{DVec3, Vec3};
use manifold_rust::linalg::Vec3 as MVec3;
use manifold_rust::manifold::Manifold;
use manifold_rust::types::Box as MBox;
use std::collections::HashMap;

/// Most passes the Smooth slider reaches, as the original's (its default is 3, half).
pub const SMOOTH_STEPS: u32 = 6;
/// The original splits faces without Smooth edges only below this many points.
const SPLIT_BELOW_POINTS: usize = 4096;
/// Past this many triangles Loop subdivision stops (the original has no limit and runs
/// out of memory on scans).
const SMOOTH_BUDGET: usize = 400_000;

/// The Smooth tool's surface, as the original's (read from its code): `amount` (0 to 1)
/// is a number of passes, 0 to [`SMOOTH_STEPS`]. With `edges` each pass is one step of
/// Loop subdivision, so edges and corners melt into a rounder, smaller shape. Without
/// it each pass splits every face in four while the mesh has fewer than 4096 points,
/// then relaxes it with ten rounds of HC Laplacian smoothing, which keeps the volume:
/// a box turns into a pillow of about its size, and a detailed scan keeps its triangle
/// count while its mesh evens out. Colours and textures follow their faces. Beyond the
/// original: points on open or non-manifold edges stay put, flat fans are rebuilt from
/// their centre first, and the bottom keeps its height so a model stays on the floor.
pub fn smooth_by(mesh: &Mesh, amount: f32, edges: bool) -> Result<Mesh> {
    let steps = (amount.clamp(0.0, 1.0) * SMOOTH_STEPS as f32).round() as u32;
    if steps == 0 || mesh.triangles.is_empty() {
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
    for _ in 0..steps {
        if edges {
            if tris.len() * 4 > SMOOTH_BUDGET {
                break;
            }
            (pos, tris, colors, texs) = subdivide(&pos, &tris, &colors, &texs, true);
        } else {
            if pos.len() < SPLIT_BELOW_POINTS {
                (pos, tris, colors, texs) = subdivide(&pos, &tris, &colors, &texs, false);
            }
            hc_smooth(&mut pos, &tris, 10);
        }
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

/// HC Laplacian smoothing (Vollmer, Mencl and Müller), as the original's: `rounds`
/// rounds of moving every point to the weighted mean of its neighbours, then pushing it
/// back towards where it started (α 0.2, β 0.8), so the shape does not shrink. An edge
/// weighs the mean angle facing it in its triangles, in radians; angles under 0.03 or
/// over 1.5408 count as nothing. Points on open or non-manifold edges stay put.
fn hc_smooth(pos: &mut [Vec3], tris: &[[u32; 3]], rounds: usize) {
    let key = |a: u32, b: u32| (a.min(b), a.max(b));
    let mut index: HashMap<(u32, u32), usize> = HashMap::new();
    let mut edges: Vec<(u32, u32)> = Vec::new();
    let mut faces: Vec<u32> = Vec::new();
    // For every triangle, the edge facing each corner.
    let facing: Vec<[usize; 3]> = tris
        .iter()
        .map(|t| {
            [0, 1, 2].map(|k| {
                let e = key(t[(k + 1) % 3], t[(k + 2) % 3]);
                *index.entry(e).or_insert_with(|| {
                    edges.push(e);
                    faces.push(0);
                    edges.len() - 1
                })
            })
        })
        .collect();
    facing.iter().flatten().for_each(|&e| faces[e] += 1);
    let mut fixed = vec![false; pos.len()];
    for (e, &(a, b)) in edges.iter().enumerate() {
        if faces[e] != 2 {
            fixed[a as usize] = true;
            fixed[b as usize] = true;
        }
    }
    let start = pos.to_vec();
    let mut weight = vec![0f32; edges.len()];
    let mean = |values: &[Vec3], weight: &[f32]| {
        let mut sum = vec![Vec3::ZERO; values.len()];
        let mut total = vec![0f32; values.len()];
        for (&(a, b), &w) in edges.iter().zip(weight) {
            sum[a as usize] += values[b as usize] * w;
            sum[b as usize] += values[a as usize] * w;
            total[a as usize] += w;
            total[b as usize] += w;
        }
        (sum, total)
    };
    for _ in 0..rounds {
        weight.iter_mut().for_each(|w| *w = 0.0);
        for (t, e) in tris.iter().zip(&facing) {
            for k in 0..3 {
                let p = pos[t[k] as usize];
                let u = (pos[t[(k + 1) % 3] as usize] - p).normalize_or_zero();
                let v = (pos[t[(k + 2) % 3] as usize] - p).normalize_or_zero();
                let angle = u.dot(v).clamp(-1.0, 1.0).acos();
                if (0.03..=1.5408).contains(&angle) {
                    weight[e[k]] += angle;
                }
            }
        }
        for (w, &n) in weight.iter_mut().zip(&faces) {
            *w /= n.max(1) as f32;
        }
        let (sum, total) = mean(pos, &weight);
        let q: Vec<Vec3> = (0..pos.len())
            .map(|i| {
                if total[i] > 0.0 && !fixed[i] {
                    sum[i] / total[i]
                } else {
                    pos[i]
                }
            })
            .collect();
        let pull: Vec<Vec3> = (0..pos.len())
            .map(|i| q[i] - (start[i] * 0.2 + pos[i] * 0.8))
            .collect();
        let (near, total) = mean(&pull, &weight);
        for i in 0..pos.len() {
            if fixed[i] || total[i] <= 0.0 {
                continue;
            }
            pos[i] = q[i] - (pull[i] * 0.8 + near[i] / total[i] * 0.2);
        }
    }
}

/// Points, triangles and their colours and textures.
type Split = (Vec<Vec3>, Vec<[u32; 3]>, Vec<[u8; 4]>, Vec<FaceTex>);

/// One step of [`smooth_by`]: every face split in four, the new points placed by the
/// Loop rules (`approximate`) or halfway along their edge.
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
                        half
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
    // No ball of radius `wall` fits inside an object thinner than twice the wall.
    if mesh.aabb().size().min_element() <= 2.0 * wall {
        bail!("The object is too thin to hollow with a {wall} mm wall");
    }
    let solid = to_manifold(mesh)?;
    let inside = inner(mesh, wall)?;
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

/// Rounding slack, in voxels, added to the curvature margin of the inner surface.
const SLACK: f32 = 0.02;

const JITTER: (f32, f32) = (0.013_7, 0.029_1);

/// The region deeper than `wall` inside `mesh`, as a solid.
fn inner(mesh: &Mesh, wall: f32) -> Result<Manifold> {
    let b = mesh.aabb();
    let size = b.max - b.min;
    // Depths are exact distances to the surface near the wall, so the wall is within
    // the margin of the request; big models get coarse voxels and so a coarser surface.
    let h = (wall / 4.0).max(size.max_element() / MAX_CELLS);
    // One outside voxel on every side, so every line starts and ends outside.
    let cells = (size / h).ceil().as_uvec3() + 2;
    let n = cells.to_array().map(|v| v as usize);
    // Centred on the model, so the first outside voxel centre lies as far beyond the
    // surface on both ends of every axis and the cavity is not shifted towards one side.
    let origin = (b.min + b.max - cells.as_vec3() * h) * 0.5;
    let grid = Grid { origin, h, n };
    let center = |i: usize, axis: usize| grid.center(i, axis);
    let idx = |x: usize, y: usize, z: usize| (z * n[1] + y) * n[0] + x;

    // Crossings of each vertical line through voxel centres with the surface.
    let mut cross: Vec<Vec<f32>> = vec![Vec::new(); n[0] * n[1]];
    for t in &mesh.triangles {
        let [a, b2, c] = t.map(|i| mesh.positions[i as usize]);
        let lo = a.min(b2).min(c);
        let hi = a.max(b2).max(c);
        // Lines sit a jitter past the voxel centres, so the range shifts back by it.
        let x0 = (((lo.x - origin.x) / h - 0.5 - JITTER.0).ceil().max(0.0)) as usize;
        let y0 = (((lo.y - origin.y) / h - 0.5 - JITTER.1).ceil().max(0.0)) as usize;
        let x1 = ((((hi.x - origin.x) / h - 0.5 - JITTER.0).floor()) as usize).min(n[0] - 1);
        let y1 = ((((hi.y - origin.y) / h - 0.5 - JITTER.1).floor()) as usize).min(n[1] - 1);
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

    // Signed distance (mm, > 0 inside) to the surface, exact within `reach` of it and
    // clamped beyond: only the level `wall` matters, and its trilinear cell lies within
    // √3 voxels of it.
    let reach = wall + 2.0 * h;
    let mut sdist = vec![reach; n[0] * n[1] * n[2]];
    let mut inside = vec![false; sdist.len()];
    for y in 0..n[1] {
        for x in 0..n[0] {
            let zs = &mut cross[y * n[0] + x];
            zs.sort_by(f32::total_cmp);
            for pair in zs.chunks_exact(2) {
                for z in 0..n[2] {
                    let pz = center(z, 2);
                    inside[idx(x, y, z)] |= pz > pair[0] && pz < pair[1];
                }
            }
        }
    }
    carve_distances(mesh, &grid, reach, &mut sdist);
    for (d, &i) in sdist.iter_mut().zip(&inside) {
        if !i {
            *d = -*d;
        }
    }

    // Concave parts of the surface (holes, the inside of a ring) make the trilinear sample
    // overestimate the depth, and the level set's chords cut into the cavity, each by up
    // to h²/(8R) with R the radius of the inner surface there. R is at least `wall`
    // (the surface is offset that far from a concave one), so the inner surface is
    // pulled in by their sum, which vanishes as the voxels shrink.
    let margin = h * h / (4.0 * wall) + SLACK * h;
    let sdf = |p: MVec3| -> f64 {
        // Trilinear sample of the signed distance (mm), minus the wall.
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
            acc += wgt * sdist[idx(i[0], i[1], i[2])];
        }
        (acc - wall - margin) as f64
    };
    level_set(&sdf, b.min, b.max, h)
}

/// A regular grid of voxels of side `h`, `n` of them along each axis from `origin`.
struct Grid {
    origin: Vec3,
    h: f32,
    n: [usize; 3],
}

impl Grid {
    /// Coordinate of the centre of voxel `i` along `axis`.
    fn center(&self, i: usize, axis: usize) -> f32 {
        self.origin[axis] + (i as f32 + 0.5) * self.h
    }

    /// Voxels whose centres lie in [lo, hi] along `axis`.
    fn span(&self, lo: f32, hi: f32, axis: usize) -> std::ops::Range<usize> {
        let cell = |v: f32| (v - self.origin[axis]) / self.h - 0.5;
        let first = cell(lo).ceil().max(0.0) as usize;
        let last = (cell(hi).floor().max(-1.0) + 1.0) as usize;
        first..last.min(self.n[axis])
    }
}

/// Layers of voxels one thread takes at a time in [`carve_distances`].
const SLAB: usize = 4;

/// Lowers every voxel of `dist` to its distance to the surface of `mesh` where that is
/// below its current value (callers start it at `reach`, so farther voxels keep it).
/// Threads take slabs of `SLAB` layers; per triangle and voxel, cheap lower bounds (the
/// plane, the bounding box) skip the exact distance, and a row only spans the voxels
/// within `reach` of the box.
fn carve_distances(mesh: &Mesh, grid: &Grid, reach: f32, dist: &mut [f32]) {
    let [n0, n1, _] = grid.n;
    let slabs = std::sync::Mutex::new(dist.chunks_mut(SLAB * n0 * n1).enumerate());
    let worker = || {
        loop {
            // The lock is released before the slab is worked on.
            let next = slabs.lock().unwrap().next();
            let Some((k, slab)) = next else { break };
            carve_slab(mesh, grid, reach, k * SLAB, slab);
        }
    };
    let threads = std::thread::available_parallelism().map_or(1, |t| t.get());
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(worker);
        }
    });
}

/// [`carve_distances`] for the layers from `z_lo` that `slab` holds.
fn carve_slab(mesh: &Mesh, grid: &Grid, reach: f32, z_lo: usize, slab: &mut [f32]) {
    let [n0, n1, _] = grid.n;
    let layers = slab.len() / (n0 * n1);
    // Distance from `v` to the interval [lo, hi].
    let gap = |v: f32, lo: f32, hi: f32| (lo - v).max(v - hi).max(0.0);
    let (z_min, z_max) = (grid.center(z_lo, 2), grid.center(z_lo + layers - 1, 2));
    for t in &mesh.triangles {
        let [a, b, c] = t.map(|i| mesh.positions[i as usize]);
        let (lo, hi) = (a.min(b).min(c), a.max(b).max(c));
        if lo.z - reach > z_max || hi.z + reach < z_min {
            continue;
        }
        let normal = (b - a).cross(c - a).normalize_or_zero();
        let zs = grid.span(lo.z - reach, hi.z + reach, 2);
        for z in zs.start.max(z_lo)..zs.end.min(z_lo + layers) {
            let pz = grid.center(z, 2);
            let dz = gap(pz, lo.z, hi.z);
            for y in grid.span(lo.y - reach, hi.y + reach, 1) {
                let py = grid.center(y, 1);
                let dyz2 = dz * dz + gap(py, lo.y, hi.y).powi(2);
                let rest = reach * reach - dyz2;
                if rest <= 0.0 {
                    continue;
                }
                let rx = rest.sqrt();
                for x in grid.span(lo.x - rx, hi.x + rx, 0) {
                    let p = Vec3::new(grid.center(x, 0), py, pz);
                    let cell = &mut slab[((z - z_lo) * n1 + y) * n0 + x];
                    // Cheap lower bounds first: the plane and the bounding box.
                    if normal.dot(p - a).abs() >= *cell
                        || dyz2 + gap(p.x, lo.x, hi.x).powi(2) >= *cell * *cell
                    {
                        continue;
                    }
                    *cell = cell.min(dist_to_triangle(p, a, b, c));
                }
            }
        }
    }
}

/// Grid shifts (in cells) [`level_set`] tries; between them every face offset lies
/// within a quarter cell of a grid point in one of them.
const GRID_SHIFTS: [f32; 3] = [0.0, 0.5, 0.25];

/// The solid where `sdf` > 0 between `min` and `max`, meshed on a grid of `h` cells.
/// manifold-rust's level set panics ("table full before any vert") when its first hash
/// table fills before any grid point lies within a quarter cell of the surface and snaps
/// to it. That happens when big flat faces parallel to the grid sit between grid planes
/// (a 30 mm cube hollowed with a 2 mm wall): their crossings fill the table and none
/// snaps. Shifting the grid by a fraction of a cell moves those faces onto grid points,
/// so the next shift succeeds.
fn level_set(
    sdf: &(impl Fn(MVec3) -> f64 + Sync),
    min: Vec3,
    max: Vec3,
    h: f32,
) -> Result<Manifold> {
    for shift in GRID_SHIFTS {
        // Whole cells from a shifted corner, one spare cell outside on every side.
        let lo = min - Vec3::splat(h * (1.0 + shift));
        let hi = lo + ((max - lo) / h + 1.0).ceil() * h;
        let bounds = MBox {
            min: MVec3::new(lo.x as f64, lo.y as f64, lo.z as f64),
            max: MVec3::new(hi.x as f64, hi.y as f64, hi.z as f64),
        };
        // A hair under h, so rounding never drops the last grid plane.
        let edge = h as f64 * (1.0 - 1e-6);
        if let Ok(solid) = crate::no_panic(|| Ok(Manifold::level_set(sdf, bounds, edge))) {
            return Ok(solid);
        }
    }
    bail!("Could not build the inner surface; try another wall thickness")
}

/// Distance from `p` to the segment `ab`.
fn dist_to_segment(p: DVec3, a: DVec3, b: DVec3) -> f64 {
    let ab = b - a;
    let len2 = ab.length_squared();
    let t = if len2 > 0.0 {
        ((p - a).dot(ab) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (p - (a + ab * t)).length()
}

/// Distance from `p` to the triangle `abc` (closest point by Ericson's region tests, in
/// f64; a needle or a sliver, whose barycentric division would blow up, falls back to
/// its three edges).
pub(crate) fn dist_to_triangle(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> f32 {
    let [p, a, b, c] = [p, a, b, c].map(|v| v.as_dvec3());
    let (ab, ac, ap) = (b - a, c - a, p - a);
    let edges = || {
        dist_to_segment(p, a, b)
            .min(dist_to_segment(p, b, c))
            .min(dist_to_segment(p, c, a))
    };
    // |ab x ac|² against |ab|²|ac|²: sin² of the angle at `a`.
    if ab.cross(ac).length_squared() <= 1e-18 * ab.length_squared() * ac.length_squared() {
        return edges() as f32;
    }
    let (d1, d2) = (ab.dot(ap), ac.dot(ap));
    let q = if d1 <= 0.0 && d2 <= 0.0 {
        a
    } else {
        let bp = p - b;
        let (d3, d4) = (ab.dot(bp), ac.dot(bp));
        let cp = p - c;
        let (d5, d6) = (ab.dot(cp), ac.dot(cp));
        let (vc, vb, va) = (d1 * d4 - d3 * d2, d5 * d2 - d1 * d6, d3 * d6 - d5 * d4);
        if d3 >= 0.0 && d4 <= d3 {
            b
        } else if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
            a + ab * (d1 / (d1 - d3))
        } else if d6 >= 0.0 && d5 <= d6 {
            c
        } else if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
            a + ac * (d2 / (d2 - d6))
        } else if va <= 0.0 && d4 - d3 >= 0.0 && d5 - d6 >= 0.0 {
            b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)))
        } else {
            let sum = va + vb + vc;
            if sum > 0.0 {
                a + ab * (vb / sum) + ac * (vc / sum)
            } else {
                return edges() as f32;
            }
        }
    };
    (p - q).length() as f32
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
    fn smooth_runs_the_original_passes() {
        let cube = Primitive::Cube.build();
        let volume = |m: &Mesh| m.stats().volume;
        let size = |m: &Mesh| m.aabb().max - m.aabb().min;
        assert_eq!(smooth_by(&cube, 0.0, true).unwrap(), cube);
        // Without Smooth edges: split while under 4096 points (8, 26, 98, 386, 1538,
        // 6146), relaxed every pass; the pillow keeps about the cube's size and volume.
        let half = smooth_by(&cube, 0.5, false).unwrap();
        let full = smooth_by(&cube, 1.0, false).unwrap();
        assert_eq!(half.triangles.len(), cube.triangles.len() * 4usize.pow(3));
        assert_eq!(full.triangles.len(), cube.triangles.len() * 4usize.pow(5));
        assert!(full.stats().is_closed_manifold());
        assert!(volume(&half) > 0.75 * volume(&cube));
        assert!((volume(&half) - volume(&full)).abs() < 0.02 * volume(&cube));
        assert!(size(&half).min_element() > 0.95 * size(&cube).min_element());
        // With it: one Loop step per pass, which shrinks it into a blob.
        let round = smooth_by(&cube, 0.5, true).unwrap();
        assert_eq!(round.triangles.len(), cube.triangles.len() * 4usize.pow(3));
        assert_eq!(
            smooth_by(&cube, 1.0, true).unwrap().triangles.len(),
            cube.triangles.len() * 4usize.pow(6)
        );
        assert!(size(&round).max_element() < 0.95 * size(&cube).max_element());
        assert!(volume(&round) < volume(&half));
        assert_eq!(round.aabb().min.z, cube.aabb().min.z);
        assert_eq!(half.aabb().min.z, cube.aabb().min.z);
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
    fn big_models_are_relaxed_without_splitting() {
        let cube = Primitive::Cube.build().welded();
        let n = cube.triangles.len();
        let mut split = (
            cube.positions.clone(),
            cube.triangles.clone(),
            vec![[0; 4]; n],
            vec![FaceTex::default(); n],
        );
        for _ in 0..7 {
            split = subdivide(&split.0, &split.1, &split.2, &split.3, false);
        }
        let big = Mesh::new(split.0, split.1);
        assert!(big.positions.len() > SPLIT_BELOW_POINTS);
        let relaxed = smooth_by(&big, 0.5, false).unwrap();
        assert_eq!(relaxed.triangles.len(), big.triangles.len());
        assert_ne!(relaxed.positions, big.positions);
        assert!(relaxed.stats().is_closed_manifold());
        // Loop stops at the triangle budget instead of running out of memory.
        assert!(big.triangles.len() * 4 > SMOOTH_BUDGET);
        assert_eq!(smooth_by(&big, 1.0, true).unwrap(), big);
    }

    #[test]
    fn smooth_carries_face_colors() {
        let mut ball = Primitive::Sphere.build();
        let palette = [[255, 0, 0, 255], [0, 255, 0, 255]];
        ball.colors = (0..ball.triangles.len()).map(|i| palette[i % 2]).collect();
        let smooth = smooth_by(&ball, 1.0, true).unwrap();
        assert_eq!(smooth.colors.len(), smooth.triangles.len());
        assert!(smooth.colors.iter().all(|c| palette.contains(c)));
        assert!(palette.iter().all(|p| smooth.colors.contains(p)));
    }

    /// Wall thickness on each side (-x, +x, -y, +y, -z, +z) of a hollowed cube.
    fn walls(size: f32, wall: f32) -> [f32; 6] {
        let cube = Primitive::Cube.build();
        let k = size / (cube.aabb().max - cube.aabb().min).x;
        let cube = cube.transformed(&glam::Mat4::from_scale(Vec3::splat(k)));
        let out = hollow(&cube, wall).unwrap();
        let o = out.aabb();
        // The cavity is every vertex off the outer box.
        let mut c = crate::mesh::Aabb::empty();
        for &p in &out.positions {
            if (p - o.min).min_element() > 0.1 && (o.max - p).min_element() > 0.1 {
                c.grow(p);
            }
        }
        assert!(c.is_valid());
        let (lo, hi) = (c.min - o.min, o.max - c.max);
        [lo.x, hi.x, lo.y, hi.y, lo.z, hi.z]
    }

    #[test]
    fn hollow_cube_cavity_is_centred() {
        // The same wall, within the tolerance, on all six sides.
        for size in [30.0, 40.0, 25.3] {
            let w = walls(size, 2.0);
            for axis in 0..3 {
                let (a, b) = (w[2 * axis], w[2 * axis + 1]);
                assert!((a - b).abs() < 0.02, "{size} mm cube, axis {axis}: {w:?}");
            }
            let (min, max) = w
                .iter()
                .fold((f32::MAX, f32::MIN), |(lo, hi), &v| (lo.min(v), hi.max(v)));
            assert!(min >= 2.0 - 1e-3 && max <= 2.25, "{size} mm cube: {w:?}");
        }
    }

    #[test]
    fn distance_to_a_triangle_matches_sampling_and_known_regions() {
        let (a, b, c) = (
            Vec3::ZERO,
            Vec3::new(4.0, 0.0, 0.0),
            Vec3::new(0.0, 3.0, 0.0),
        );
        let d = |p: Vec3| dist_to_triangle(p, a, b, c);
        // Interior (above and on the plane), edges, vertices.
        assert!((d(Vec3::new(1.0, 1.0, 2.0)) - 2.0).abs() < 1e-6);
        assert!(d(Vec3::new(1.0, 1.0, 0.0)) < 1e-6);
        assert!((d(Vec3::new(2.0, -1.5, 0.0)) - 1.5).abs() < 1e-6);
        assert!((d(Vec3::new(-2.0, 1.0, 0.0)) - 2.0).abs() < 1e-6);
        // Beyond the hypotenuse (x/4 + y/3 = 1), and the vertices.
        assert!((d(Vec3::new(4.0, 3.0, 0.0)) - 12.0 / 5.0).abs() < 1e-5);
        assert!((d(Vec3::new(-3.0, -4.0, 0.0)) - 5.0).abs() < 1e-6);
        assert!((d(Vec3::new(6.0, 0.0, 0.0)) - 2.0).abs() < 1e-6);
        // Brute force: the nearest of a dense sampling of the triangle never beats it
        // (it is the minimum) and comes within the sampling step.
        let mut s = 7u32;
        let mut rnd = || {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (s >> 8) as f32 / (1 << 24) as f32
        };
        let samples: Vec<Vec3> = (0..=200)
            .flat_map(|i| (0..=200 - i).map(move |j| (i as f32 / 200.0, j as f32 / 200.0)))
            .map(|(u, v)| a + (b - a) * u + (c - a) * v)
            .collect();
        for _ in 0..300 {
            let p = Vec3::new(rnd() * 12.0 - 4.0, rnd() * 10.0 - 4.0, rnd() * 8.0 - 4.0);
            let brute = samples
                .iter()
                .map(|&q| (p - q).length())
                .fold(f32::MAX, f32::min);
            assert!(
                d(p) <= brute + 1e-5 && brute - d(p) < 0.03,
                "{p}: {} {brute}",
                d(p)
            );
        }
    }

    #[test]
    fn distance_to_a_degenerate_triangle_is_finite_and_exact() {
        let p = Vec3::new(1.0, 1.0, 0.0);
        // A needle (collinear), a point and a sliver thinner than f32 resolves.
        let needle = dist_to_triangle(
            p,
            Vec3::ZERO,
            Vec3::new(4.0, 0.0, 0.0),
            Vec3::new(2.0, 0.0, 0.0),
        );
        assert!((needle - 1.0).abs() < 1e-6);
        let dot = dist_to_triangle(p, Vec3::ZERO, Vec3::ZERO, Vec3::ZERO);
        assert!((dot - 2f32.sqrt()).abs() < 1e-6);
        let sliver = dist_to_triangle(
            p,
            Vec3::ZERO,
            Vec3::new(100.0, 1e-7, 0.0),
            Vec3::new(50.0, 1e-7, 0.0),
        );
        assert!((sliver - 1.0).abs() < 1e-5, "{sliver}");
    }

    /// Distance from `p` to the nearest triangle of `mesh` (brute force).
    fn nearest(mesh: &Mesh, p: Vec3) -> f32 {
        mesh.triangles
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|i| mesh.positions[i as usize]);
                dist_to_triangle(p, a, b, c)
            })
            .fold(f32::MAX, f32::min)
    }

    /// Smallest and largest wall of `hollow(mesh, wall)`: the distance to the original
    /// surface from the vertices, centre and edge midpoints of every `step`-th triangle
    /// of the result that lies off that surface (the cavity), so face interiors count.
    fn wall_range(mesh: &Mesh, wall: f32, step: usize) -> (f32, f32) {
        let out = hollow(mesh, wall).unwrap();
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        for t in out.triangles.iter().step_by(step) {
            let [a, b, c] = t.map(|i| out.positions[i as usize]);
            let points = [
                a,
                b,
                c,
                (a + b + c) / 3.0,
                (a + b) / 2.0,
                (b + c) / 2.0,
                (c + a) / 2.0,
            ];
            let depths = points.map(|p| nearest(mesh, p));
            // Outer faces have vertices on the original surface.
            if depths[..3].iter().all(|&d| d > 1e-3) {
                lo = depths.iter().fold(lo, |m, &d| m.min(d));
                hi = depths.iter().fold(hi, |m, &d| m.max(d));
            }
        }
        assert!(lo <= hi, "no cavity");
        (lo, hi)
    }

    /// `cargo test --offline --release -- --ignored --nocapture hollow_bench`
    #[test]
    #[ignore]
    fn hollow_bench() {
        let cases = [
            (
                "cube 30, wall 2",
                scaled(Primitive::Cube.build(), 30.0),
                2.0,
            ),
            (
                "sphere 200k, wall 2",
                crate::primitives::sphere(15.0, 316, 316),
                2.0,
            ),
            (
                "sphere 200k, wall 5",
                crate::primitives::sphere(15.0, 316, 316),
                5.0,
            ),
            (
                "sphere 500k, wall 2",
                crate::primitives::sphere(15.0, 500, 500),
                2.0,
            ),
            (
                "sphere 500k, wall 5",
                crate::primitives::sphere(15.0, 500, 500),
                5.0,
            ),
            ("torus, wall 1", scaled(Primitive::Torus.build(), 40.0), 1.0),
            ("torus, wall 2", scaled(Primitive::Torus.build(), 40.0), 2.0),
            ("cube with a hole, wall 2", cube_with_hole(0.5), 2.0),
            ("cube with a hole, wall 0.3", cube_with_hole(0.5), 0.3),
        ];
        for (name, mesh, wall) in cases {
            let t = std::time::Instant::now();
            inner(&mesh, wall).unwrap();
            let t_inner = t.elapsed();
            let hollowed = hollow(&mesh, wall).unwrap();
            let total = t.elapsed();
            let walls = if mesh.triangles.len() < 20_000 {
                let (lo, hi) = wall_range(&mesh, wall, 17);
                format!("wall {lo:.3}..{hi:.3}")
            } else {
                String::new()
            };
            println!(
                "{name}: {} tris, inner {t_inner:.2?}, with hollow {total:.2?} ({} tris) {walls}",
                mesh.triangles.len(),
                hollowed.triangles.len()
            );
        }
    }

    fn scaled(m: Mesh, size: f32) -> Mesh {
        let k = size / (m.aabb().max - m.aabb().min).max_element();
        m.transformed(&glam::Mat4::from_scale(Vec3::splat(k)))
    }

    /// A 30 mm cube with a hole of `radius` through it along z.
    fn cube_with_hole(radius: f32) -> Mesh {
        let cube = scaled(Primitive::Cube.build(), 30.0);
        let c = cube.aabb().center();
        let pin = Primitive::Cylinder.build();
        let k = Vec3::new(2.0 * radius, 2.0 * radius, 40.0) / pin.aabb().size();
        let pin = pin.transformed(
            &(glam::Mat4::from_translation(c)
                * glam::Mat4::from_scale(k)
                * glam::Mat4::from_translation(-pin.aabb().center())),
        );
        crate::boolean::apply(crate::boolean::Op::Subtract, &[cube, pin]).unwrap()
    }

    /// Checks `wall_range` of `mesh` against the request: never thinner than `wall`
    /// (1e-3 of rounding), and at most `extra` thicker.
    fn check_wall(name: &str, mesh: &Mesh, wall: f32, extra: f32, step: usize) {
        let (lo, hi) = wall_range(mesh, wall, step);
        println!("{name}, wall {wall}: {lo:.3}..{hi:.3}");
        assert!(
            lo >= wall - 1e-3 && hi <= wall + extra,
            "{name}, wall {wall}: {lo}..{hi}"
        );
    }

    #[test]
    fn hollow_wall_on_concave_shapes_is_never_thinner() {
        let torus = scaled(crate::primitives::torus(0.75, 0.25, 48, 16), 40.0);
        let pierced = cube_with_hole(0.5);
        // Voxels are wall / 4 down to 1 mm walls on these 30-40 mm models, so the wall is
        // within 0.25 mm there. 0.3 mm is the coarse case (voxels about a wall): the
        // curvature margin keeps the wall thick enough, and it comes out thicker.
        for (name, mesh, wall) in [
            ("torus", &torus, 2.0f32),
            ("cube with a hole", &pierced, 2.0),
            ("cube with a hole", &pierced, 0.3),
        ] {
            let h = (wall / 4.0).max(40.0 / MAX_CELLS);
            check_wall(name, mesh, wall, (0.5 * h).max(0.25), 17);
        }
    }

    #[test]
    fn hollow_wall_is_never_thinner_and_close_to_asked() {
        // Sphere and cylinder have curved walls, the cone and wedge slanted ones; a sharp
        // convex tip (the cone's apex) rounds the cavity there, so a bit thicker (k is the fraction of a voxel allowed).
        let shapes = [
            (Primitive::Cube, 0.4),
            (Primitive::Sphere, 0.4),
            (Primitive::Cylinder, 0.75),
            (Primitive::Cone, 0.75),
            (Primitive::Wedge, 0.75),
        ];
        for (p, k) in shapes {
            for (size, wall) in [(30.0f32, 1.0f32), (30.0, 2.0), (40.0, 5.0), (60.0, 2.0)] {
                // Allowed excess: 0.25 mm, or a fraction k of a voxel on coarse grids.
                let h = (wall / 4.0).max(size / MAX_CELLS);
                let extra = (k * h).max(0.25);
                let mesh = scaled(p.build(), size);
                check_wall(&format!("{p:?} {size} mm"), &mesh, wall, extra, 7);
            }
        }
    }
}
