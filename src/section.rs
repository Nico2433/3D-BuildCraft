//! Cross-sections: the outline a plane leaves on a mesh, with a circle fitted to each
//! closed contour. For a flower pot sliced near its top this gives the mouth's diameter even
//! when the rim is smooth (no crease for `edge_loops` to find), and for a hollow pot the
//! outer and inner contours, so the wall thickness is the difference of their radii.
//! The Measure tool's "Section" mode and the MCP `measure` tool (`section`) share it.
use crate::edge_loops::EdgeLoop;
use crate::mesh::Mesh;
use glam::{Mat4, Vec2, Vec3};
use rustc_hash::FxHashMap as HashMap;

/// One contour of a section: a closed loop (or an open chain where the mesh is open) with
/// its circle fit and where it sits among the others.
#[derive(Clone, Debug)]
pub struct Contour {
    /// The points in order and the circle fitted to them (`chain` is the contour's index).
    pub outline: EdgeLoop,
    /// How many closed contours enclose this one: 0 is an outer wall, 1 a cavity or hole.
    pub depth: usize,
    /// The smallest closed contour that encloses it.
    pub parent: Option<usize>,
    /// The area inside a closed contour, mm².
    pub area: f32,
}

/// A wall: an outer contour, the contour inside it, and how thick the material between is.
#[derive(Clone, Copy, Debug)]
pub struct Wall {
    pub outer: usize,
    pub inner: usize,
    /// Outer radius minus inner radius, mm (the least thickness when they are off centre).
    pub thickness: f32,
    /// Distance between the two centres, mm.
    pub offset: f32,
}

/// The contours a plane cuts out of a mesh.
#[derive(Clone, Debug)]
pub struct Section {
    pub point: Vec3,
    pub normal: Vec3,
    pub contours: Vec<Contour>,
}

impl Section {
    /// The contour whose outline passes closest to `p`.
    pub fn nearest(&self, p: Vec3) -> Option<usize> {
        self.contours
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let d = c.outline.points.iter().map(|q| q.distance_squared(p));
                (i, d.fold(f32::INFINITY, f32::min))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }

    /// Every cavity inside a round outer contour that is round itself.
    pub fn walls(&self) -> Vec<Wall> {
        self.contours
            .iter()
            .enumerate()
            .filter_map(|(inner, c)| {
                let outer = c.parent?;
                let (a, b) = (self.contours[outer].outline.round()?, c.outline.round()?);
                let offset = a.center.distance(b.center);
                Some(Wall {
                    outer,
                    inner,
                    thickness: a.radius - b.radius - offset,
                    offset,
                })
            })
            .collect()
    }

    /// The wall the contour `i` is part of, as its outer or inner side.
    pub fn wall_of(&self, i: usize) -> Option<Wall> {
        self.walls()
            .into_iter()
            .find(|w| w.outer == i || w.inner == i)
    }
}

/// Slices `mesh` placed by `model` with the plane through `point` whose normal is
/// `normal`. Triangles with a vertex exactly on the plane count it as above, so a plane
/// through a flat top face gives the same outline as one just under it.
pub fn section(mesh: &Mesh, model: &Mat4, point: Vec3, normal: Vec3) -> Section {
    let normal = normal.try_normalize().unwrap_or(Vec3::Z);
    let world: Vec<Vec3> = mesh
        .positions
        .iter()
        .map(|p| model.transform_point3(*p))
        .collect();
    let dist: Vec<f32> = world.iter().map(|p| (*p - point).dot(normal)).collect();
    let (welded, _) = mesh.welded_indices();
    // A crossing point per mesh edge, and the segments between the two of each triangle.
    let mut index: HashMap<u64, u32> = HashMap::default();
    let mut points: Vec<Vec3> = Vec::new();
    let mut segments: Vec<[u32; 2]> = Vec::new();
    for t in &mesh.triangles {
        let above = t.map(|i| dist[i as usize] >= 0.0);
        if above[0] == above[1] && above[1] == above[2] {
            continue;
        }
        let mut ends = [0u32; 2];
        let mut n = 0;
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            if above[k] == above[(k + 1) % 3] {
                continue;
            }
            let (wa, wb) = (welded[a as usize], welded[b as usize]);
            let key = (wa.min(wb) as u64) << 32 | wa.max(wb) as u64;
            ends[n] = *index.entry(key).or_insert_with(|| {
                let (da, db) = (dist[a as usize], dist[b as usize]);
                let at = da / (da - db);
                points.push(world[a as usize].lerp(world[b as usize], at));
                (points.len() - 1) as u32
            });
            n += 1;
        }
        if n == 2 && ends[0] != ends[1] {
            segments.push(ends);
        }
    }
    let chains = join(points.len(), &segments);
    let (u, v) = (
        normal.any_orthonormal_vector(),
        normal.cross(normal.any_orthonormal_vector()),
    );
    let flat = |p: Vec3| Vec2::new((p - point).dot(u), (p - point).dot(v));
    let mut contours: Vec<(Vec<Vec3>, bool, Vec<Vec2>, f32)> = chains
        .into_iter()
        .map(|(ids, closed)| {
            let pts: Vec<Vec3> = ids.iter().map(|&i| points[i as usize]).collect();
            let poly: Vec<Vec2> = pts.iter().map(|p| flat(*p)).collect();
            let area = if closed { shoelace(&poly).abs() } else { 0.0 };
            (pts, closed, poly, area)
        })
        .collect();
    // Biggest first, so the outer contours come before what they enclose.
    contours.sort_by(|a, b| b.3.total_cmp(&a.3).then(b.0.len().cmp(&a.0.len())));
    let boxes: Vec<(Vec2, Vec2)> = contours
        .iter()
        .map(|c| {
            c.2.iter().fold(
                (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY)),
                |(lo, hi), p| (lo.min(*p), hi.max(*p)),
            )
        })
        .collect();
    let mut out = Vec::with_capacity(contours.len());
    for (i, (pts, closed, poly, area)) in contours.iter().enumerate() {
        let (mut depth, mut parent) = (0, None);
        if *closed {
            let probe = poly[0];
            for j in (0..i).filter(|&j| contours[j].1) {
                let (lo, hi) = boxes[j];
                let inside = probe.cmpge(lo).all() && probe.cmple(hi).all();
                if inside && contains(&contours[j].2, probe) {
                    depth += 1;
                    // Sorted by area, so the last enclosing one is the smallest.
                    parent = Some(j);
                }
            }
        }
        out.push(Contour {
            outline: EdgeLoop::new(i, pts.clone(), *closed),
            depth,
            parent,
            area: *area,
        });
    }
    Section {
        point,
        normal,
        contours: out,
    }
}

/// The horizontal section at height `z`.
pub fn section_at_height(mesh: &Mesh, model: &Mat4, z: f32) -> Section {
    section(mesh, model, Vec3::new(0.0, 0.0, z), Vec3::Z)
}

/// Joins segments into chains of point indices: open chains from their ends, then loops.
fn join(n: usize, segments: &[[u32; 2]]) -> Vec<(Vec<u32>, bool)> {
    let mut next: Vec<[u32; 2]> = vec![[u32::MAX; 2]; n];
    for &[a, b] in segments {
        for (p, q) in [(a, b), (b, a)] {
            let slot = &mut next[p as usize];
            if slot[0] == u32::MAX {
                slot[0] = q;
            } else if slot[1] == u32::MAX {
                slot[1] = q;
            }
        }
    }
    let mut seen = vec![false; n];
    let mut out = Vec::new();
    let degree = |p: usize| next[p].iter().filter(|q| **q != u32::MAX).count();
    let walk = |start: usize, seen: &mut Vec<bool>| {
        let mut chain = vec![start as u32];
        seen[start] = true;
        let (mut prev, mut cur) = (u32::MAX, start as u32);
        loop {
            let step = next[cur as usize]
                .into_iter()
                .find(|&q| q != u32::MAX && q != prev && !seen[q as usize]);
            let Some(q) = step else { break };
            seen[q as usize] = true;
            chain.push(q);
            (prev, cur) = (cur, q);
        }
        chain
    };
    for start in (0..n).filter(|&p| degree(p) == 1) {
        if !seen[start] {
            out.push((walk(start, &mut seen), false));
        }
    }
    for start in 0..n {
        if !seen[start] && degree(start) > 0 {
            out.push((walk(start, &mut seen), true));
        }
    }
    out
}

fn shoelace(poly: &[Vec2]) -> f32 {
    let mut sum = 0.0f64;
    for (i, p) in poly.iter().enumerate() {
        let q = poly[(i + 1) % poly.len()];
        sum += p.x as f64 * q.y as f64 - q.x as f64 * p.y as f64;
    }
    (sum / 2.0) as f32
}

/// Whether `p` is inside the closed polygon (even-odd rule).
fn contains(poly: &[Vec2], p: Vec2) -> bool {
    let mut inside = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[j]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives;

    fn near(a: f32, b: f32, tolerance: f32) -> bool {
        (a - b).abs() <= tolerance
    }

    #[test]
    fn a_cylinder_section_is_one_circle() {
        let m = primitives::prism(64, 10.0, 30.0);
        let s = section_at_height(&m, &Mat4::IDENTITY, 12.3);
        assert_eq!(s.contours.len(), 1);
        let c = &s.contours[0];
        let r = c.outline.round().unwrap();
        assert!(c.outline.closed && c.depth == 0 && c.parent.is_none());
        assert!(near(r.radius, 10.0, 0.02) && near(r.center.z, 12.3, 1e-4));
        assert!(r.axis.unwrap().z.abs() > 0.999);
    }

    #[test]
    fn a_plane_that_misses_leaves_nothing_and_a_slanted_one_an_ellipse() {
        let m = primitives::prism(64, 10.0, 30.0);
        assert!(section_at_height(&m, &Mat4::IDENTITY, 31.0)
            .contours
            .is_empty());
        let s = section(
            &m,
            &Mat4::IDENTITY,
            Vec3::new(0.0, 0.0, 15.0),
            Vec3::new(1.0, 0.0, 1.0),
        );
        assert_eq!(s.contours.len(), 1);
        assert!(!s.contours[0].outline.is_circular());
    }

    #[test]
    fn a_placed_cone_gives_the_radius_at_that_height() {
        let m = primitives::cone(64, 12.0, 20.0);
        let model = Mat4::from_translation(Vec3::new(5.0, -3.0, 2.0));
        // Half way up a cone with base radius 12 and height 20: radius 6.
        let s = section_at_height(&m, &model, 12.0);
        let r = s.contours[0].outline.round().unwrap();
        assert!(near(r.radius, 6.0, 0.05));
        assert!(r.center.truncate().distance(Vec2::new(5.0, -3.0)) < 1e-3);
    }

    #[test]
    fn a_hollow_tube_has_an_outer_and_an_inner_contour_and_a_wall() {
        let outer = primitives::prism(64, 10.0, 30.0);
        let inner = primitives::prism(64, 8.0, 30.0);
        let mut m = outer.clone();
        let n = m.positions.len() as u32;
        m.positions.extend(inner.positions.iter().copied());
        m.triangles.extend(
            inner
                .triangles
                .iter()
                .map(|t| [t[0] + n, t[2] + n, t[1] + n]),
        );
        let s = section_at_height(&m, &Mat4::IDENTITY, 15.0);
        assert_eq!(s.contours.len(), 2);
        assert_eq!(
            (
                s.contours[0].depth,
                s.contours[1].depth,
                s.contours[1].parent
            ),
            (0, 1, Some(0))
        );
        let walls = s.walls();
        assert_eq!(walls.len(), 1);
        assert!(near(walls[0].thickness, 2.0, 0.05) && walls[0].offset < 1e-3);
        assert_eq!(s.wall_of(1).map(|w| w.outer), Some(0));
        let near_inner = Vec3::new(8.0, 0.0, 15.0);
        assert_eq!(s.nearest(near_inner), Some(1));
    }
}
