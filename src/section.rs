//! Cross-sections: the outline a plane leaves on a mesh, with a circle fitted to each
//! contour. This is the one way to measure a diameter: a cylinder, a cone at any height, a
//! pot's mouth (outer and inner diameter, centre, wall thickness), a hole, a rim or a base.
//! The Measure tool's "Section at height" mode and the MCP `measure` tool (`section`) share
//! it.
//!
//! A contour is *circular* when a circle fits its points (see [`Outline::why_not`]): RMS
//! distance from the circle under [`CIRCLE_RMS`] of the radius, no point further than
//! [`CIRCLE_MAX`], at least [`MIN_CLOSED`] points round a closed contour (a hexagon or a
//! square also has all its corners on a circle but is a polygon), or for an open one
//! [`MIN_ARC`] points spanning [`MIN_SWEEP_DEG`] degrees.
//!
//! When the plane lies in a flat face (the top or bottom of a cylinder, the base of a cone,
//! the rim of a hollow cylinder) it cuts no material, so [`section`] slices a hair inside
//! the solid instead ([`SHIFT`] of the object's size) on the side that has material.
use crate::measure::{centroid, fit_circle_around, Round};
use crate::mesh::Mesh;
use glam::{Mat4, Vec2, Vec3};
use rustc_hash::FxHashMap as HashMap;

/// RMS distance of a contour's points from its circle, as a fraction of the radius.
pub const CIRCLE_RMS: f64 = 0.01;
/// The largest distance of any point from the circle, as a fraction of the radius.
pub const CIRCLE_MAX: f64 = 0.025;
/// Points a closed contour needs to count as round.
pub const MIN_CLOSED: usize = 8;
/// Points an open contour needs to count as round.
pub const MIN_ARC: usize = 5;
/// Degrees an open contour must span to count as round.
pub const MIN_SWEEP_DEG: f32 = 40.0;
/// How far inside the material a plane lying in a flat face is moved, as a fraction of the
/// object's largest size (0.02 mm on a 100 mm pot, less than the error of a picked point).
pub const SHIFT: f32 = 2e-4;

/// One contour of a section: a closed loop (or an open chain where the mesh is open) with
/// its circle fit and where it sits among the others.
#[derive(Clone, Debug)]
pub struct Contour {
    /// The points in order and the circle fitted to them.
    pub outline: Outline,
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
    /// How far (mm, along the normal) the plane was moved into the material because it lay in
    /// a flat face; 0 when it cut as asked. The contours are projected back onto the plane.
    pub shift: f32,
    pub contours: Vec<Contour>,
}

impl Section {
    /// The contour whose outline passes closest to `p`, and how far (mm).
    pub fn closest(&self, p: Vec3) -> Option<(usize, f32)> {
        self.contours
            .iter()
            .enumerate()
            .map(|(i, c)| (i, c.outline.distance_to(p)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// The contour whose outline passes closest to `p`.
    pub fn nearest(&self, p: Vec3) -> Option<usize> {
        self.closest(p).map(|(i, _)| i)
    }

    /// The area of material the section cuts, mm²: outer contours minus what they enclose.
    fn area(&self) -> f32 {
        let sign = |c: &Contour| if c.depth % 2 == 0 { 1.0 } else { -1.0 };
        self.contours.iter().map(|c| sign(c) * c.area).sum()
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

/// Slices `mesh` placed by `model` with the plane through `point` whose normal is `normal`.
/// `near` (a point the user clicked or named) only matters when the plane lies in a flat face:
/// the slice is then taken a hair inside the material on whichever side has a contour
/// closest to it (the side with the larger cut area when there is no `near`).
///
/// Triangles with a vertex exactly on the plane count it as above, so a plane through a
/// flat top face gives the same outline as one just under it.
pub fn section(
    mesh: &Mesh,
    model: &Mat4,
    point: Vec3,
    normal: Vec3,
    near: Option<Vec3>,
) -> Section {
    let normal = normal.try_normalize().unwrap_or(Vec3::Z);
    let world: Vec<Vec3> = mesh
        .positions
        .iter()
        .map(|p| model.transform_point3(*p))
        .collect();
    let (welded, _) = mesh.welded_indices();
    let plane = |shift: f32| Plane {
        point,
        normal,
        shift,
    };
    let size = world.iter().fold(
        (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
        |(lo, hi), p| (lo.min(*p), hi.max(*p)),
    );
    let shift = SHIFT * (size.1 - size.0).max_element().max(0.0);
    let dist = |p: &Vec3| (*p - point).dot(normal);
    let in_a_face = shift > 0.0
        && mesh.triangles.iter().any(|t| {
            t.iter()
                .all(|&i| dist(&world[i as usize]).abs() <= shift / 2.0)
        });
    if !in_a_face {
        return slice(mesh, &world, &welded, plane(0.0));
    }
    let both = [shift, -shift].map(|s| slice(mesh, &world, &welded, plane(s)));
    let [up, down] = both;
    let up_wins = match (up.contours.is_empty(), down.contours.is_empty()) {
        (true, _) => false,
        (_, true) => true,
        _ => match near.and_then(|n| up.closest(n).zip(down.closest(n))) {
            Some((a, b)) => a.1 <= b.1,
            None => up.area() >= down.area(),
        },
    };
    if up_wins {
        up
    } else {
        down
    }
}

/// A plane through `point` moved `shift` along its normal.
#[derive(Clone, Copy)]
struct Plane {
    point: Vec3,
    normal: Vec3,
    shift: f32,
}

/// The outline of `mesh` (vertices `world`, welded ids `welded`) on `plane`; the points are
/// projected back onto the plane through `plane.point`.
fn slice(mesh: &Mesh, world: &[Vec3], welded: &[u32], plane: Plane) -> Section {
    let Plane {
        point,
        normal,
        shift,
    } = plane;
    let dist: Vec<f32> = world
        .iter()
        .map(|p| (*p - point).dot(normal) - shift)
        .collect();
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
                points.push(world[a as usize].lerp(world[b as usize], at) - normal * shift);
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
            outline: Outline::new(pts.clone(), *closed),
            depth,
            parent,
            area: *area,
        });
    }
    Section {
        point,
        normal,
        shift,
        contours: out,
    }
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

/// A contour's points in world space and the circle fitted to them.
#[derive(Clone, Debug)]
pub struct Outline {
    /// The points in order; a closed contour does not repeat the first.
    pub points: Vec<Vec3>,
    pub closed: bool,
    /// The best circle through the points, whether or not it is good enough.
    pub fit: Option<CircleFit>,
}

/// A circle fitted to a contour's points.
#[derive(Clone, Copy, Debug)]
pub struct CircleFit {
    pub round: Round,
    /// RMS distance of the points from the circle, as a fraction of the radius.
    pub rms: f32,
    /// The largest such distance.
    pub max: f32,
    /// Degrees the points go round the circle (360 for a closed loop).
    pub sweep: f32,
}

impl Outline {
    /// Fits a circle to `points`.
    pub fn new(points: Vec<Vec3>, closed: bool) -> Self {
        let fit = fit_loop(&points, closed);
        Outline {
            points,
            closed,
            fit,
        }
    }

    /// Why the contour is not round, or `None` when it is (see the module notes).
    pub fn why_not(&self) -> Option<String> {
        let Some(fit) = &self.fit else {
            return Some("its points lie on a line".into());
        };
        let n = self.points.len();
        let (rms, max) = (fit.rms as f64, fit.max as f64);
        if self.closed && n < MIN_CLOSED {
            Some(format!(
                "a closed contour of {n} points is a polygon, not round (needs {MIN_CLOSED})"
            ))
        } else if !self.closed && (n < MIN_ARC || fit.sweep < MIN_SWEEP_DEG) {
            Some(format!(
                "an open contour of {n} points spanning {:.0}° is too short to say it is round (needs {MIN_ARC} points and {MIN_SWEEP_DEG:.0}°)",
                fit.sweep
            ))
        } else if rms > CIRCLE_RMS || max > CIRCLE_MAX {
            Some(format!(
                "no circle fits it: {:.1}% RMS and {:.1}% worst deviation of the radius (limits {:.1}% and {:.1}%)",
                rms * 100.0,
                max * 100.0,
                CIRCLE_RMS * 100.0,
                CIRCLE_MAX * 100.0
            ))
        } else {
            None
        }
    }

    pub fn is_circular(&self) -> bool {
        self.why_not().is_none()
    }

    /// How far (mm) the contour passes from `p`: the distance to its nearest segment.
    pub fn distance_to(&self, p: Vec3) -> f32 {
        let n = self.points.len();
        let segments = if self.closed { n } else { n.saturating_sub(1) };
        let segment = |i: usize| {
            let (a, b) = (self.points[i], self.points[(i + 1) % n]);
            let ab = b - a;
            let t = ((p - a).dot(ab) / ab.length_squared().max(f32::MIN_POSITIVE)).clamp(0.0, 1.0);
            (a + ab * t).distance(p)
        };
        (0..segments).map(segment).fold(f32::INFINITY, f32::min)
    }

    /// The fitted circle when the contour is round.
    pub fn round(&self) -> Option<Round> {
        self.why_not()
            .is_none()
            .then(|| self.fit.map(|f| f.round))?
    }
}

/// The circle through `points` (in order), in the plane they roughly lie in.
fn fit_loop(points: &[Vec3], closed: bool) -> Option<CircleFit> {
    if points.len() < 3 {
        return None;
    }
    let c = centroid(points);
    // The plane's normal: the sum of the triangles round the centroid (Newell's), which
    // also works for an arc, closed by its chord.
    let mut normal = Vec3::ZERO;
    for (i, p) in points.iter().enumerate() {
        normal += (*p - c).cross(points[(i + 1) % points.len()] - c);
    }
    let mut axis = normal.try_normalize()?;
    // A stable direction for the axis: its largest component is positive.
    let big = axis.abs().max_element();
    if axis.to_array().iter().any(|x| *x == -big) {
        axis = -axis;
    }
    let (mut round, _) = fit_circle_around(points, axis)?;
    round.radius = round.radius.max(f32::MIN_POSITIVE);
    let r = round.radius as f64;
    let (u, v) = (
        axis.any_orthonormal_vector(),
        axis.cross(axis.any_orthonormal_vector()),
    );
    let (mut sum, mut max) = (0.0f64, 0.0f64);
    let (mut sweep, mut last) = (0.0f32, None::<f32>);
    for p in points.iter().chain(points.first().filter(|_| closed)) {
        let d = *p - round.center;
        let h = d.dot(axis);
        let radial = (d - axis * h).length();
        let off = ((radial - round.radius) as f64).hypot(h as f64) / r;
        sum += off * off;
        max = max.max(off);
        let angle = d.dot(v).atan2(d.dot(u));
        if let Some(prev) = last {
            let mut step = angle - prev;
            if step > std::f32::consts::PI {
                step -= std::f32::consts::TAU;
            } else if step < -std::f32::consts::PI {
                step += std::f32::consts::TAU;
            }
            sweep += step;
        }
        last = Some(angle);
    }
    Some(CircleFit {
        round,
        rms: (sum / (points.len() + usize::from(closed)) as f64).sqrt() as f32,
        max: max as f32,
        sweep: sweep.abs().to_degrees(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives;

    fn near_f(a: f32, b: f32, tolerance: f32) -> bool {
        (a - b).abs() <= tolerance
    }

    fn at_height(m: &Mesh, z: f32) -> Section {
        section(m, &Mat4::IDENTITY, Vec3::new(0.0, 0.0, z), Vec3::Z, None)
    }

    /// A hollow tube: outer radius 10, inner radius 8, 30 high.
    fn tube() -> Mesh {
        let mut m = primitives::prism(64, 10.0, 30.0);
        let inner = primitives::prism(64, 8.0, 30.0);
        let n = m.positions.len() as u32;
        m.positions.extend(inner.positions.iter().copied());
        m.triangles.extend(
            inner
                .triangles
                .iter()
                .map(|t| [t[0] + n, t[2] + n, t[1] + n]),
        );
        m
    }

    #[test]
    fn a_cylinder_section_is_one_circle() {
        let m = primitives::prism(64, 10.0, 30.0);
        let s = at_height(&m, 12.3);
        assert_eq!(s.contours.len(), 1);
        let c = &s.contours[0];
        let r = c.outline.round().unwrap();
        assert!(c.outline.closed && c.depth == 0 && c.parent.is_none());
        assert!(near_f(r.radius, 10.0, 0.02) && near_f(r.center.z, 12.3, 1e-4));
        assert!(r.axis.unwrap().z.abs() > 0.999);
    }

    #[test]
    fn a_plane_that_misses_leaves_nothing_and_a_slanted_one_an_ellipse() {
        let m = primitives::prism(64, 10.0, 30.0);
        assert!(at_height(&m, 31.0).contours.is_empty());
        let s = section(
            &m,
            &Mat4::IDENTITY,
            Vec3::new(0.0, 0.0, 15.0),
            Vec3::new(1.0, 0.0, 1.0),
            None,
        );
        assert_eq!(s.contours.len(), 1);
        assert!(!s.contours[0].outline.is_circular());
    }

    #[test]
    fn a_placed_cone_gives_the_radius_at_that_height() {
        let m = primitives::cone(64, 12.0, 20.0);
        let model = Mat4::from_translation(Vec3::new(5.0, -3.0, 2.0));
        // Half way up a cone with base radius 12 and height 20: radius 6.
        let s = section(&m, &model, Vec3::new(0.0, 0.0, 12.0), Vec3::Z, None);
        let r = s.contours[0].outline.round().unwrap();
        assert!(near_f(r.radius, 6.0, 0.05));
        assert!(r.center.truncate().distance(Vec2::new(5.0, -3.0)) < 1e-3);
    }

    #[test]
    fn a_hollow_tube_has_an_outer_and_an_inner_contour_and_a_wall() {
        let s = at_height(&tube(), 15.0);
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
        assert!(near_f(walls[0].thickness, 2.0, 0.05) && walls[0].offset < 1e-3);
        assert_eq!(s.wall_of(1).map(|w| w.outer), Some(0));
        assert_eq!(s.shift, 0.0);
        // The contour nearest to a point on each wall.
        assert_eq!(s.nearest(Vec3::new(8.0, 0.0, 15.0)), Some(1));
        assert_eq!(s.nearest(Vec3::new(10.0, 0.0, 15.0)), Some(0));
    }

    /// The diameter of every round contour, outer first.
    fn diameters(s: &Section) -> Vec<f32> {
        s.contours
            .iter()
            .filter_map(|c| Some(c.outline.round()?.diameter()))
            .collect()
    }

    #[test]
    fn a_plane_in_the_top_or_the_base_of_a_cylinder_slices_inside_the_solid() {
        let m = primitives::prism(64, 10.0, 30.0);
        for z in [0.0, 30.0] {
            for near in [Some(Vec3::new(10.0, 0.0, z)), None] {
                let s = section(&m, &Mat4::IDENTITY, Vec3::new(0.0, 0.0, z), Vec3::Z, near);
                assert_eq!(s.contours.len(), 1, "z {z}");
                let r = s.contours[0].outline.round().unwrap();
                assert!(near_f(r.diameter(), 20.0, 0.03), "z {z}: {}", r.diameter());
                // Moved 0.006 mm inside, on the side with material, and projected back.
                assert!(near_f(s.shift.abs(), SHIFT * 30.0, 1e-6));
                assert_eq!(s.shift > 0.0, z == 0.0);
                assert!(near_f(r.center.z, z, 1e-4));
            }
        }
    }

    #[test]
    fn a_plane_in_the_base_of_a_cone_slices_inside_the_solid() {
        let m = primitives::cone(64, 12.0, 20.0);
        let s = section(
            &m,
            &Mat4::IDENTITY,
            Vec3::ZERO,
            Vec3::Z,
            Some(Vec3::X * 12.0),
        );
        assert_eq!(s.contours.len(), 1);
        let r = s.contours[0].outline.round().unwrap();
        assert!(near_f(r.diameter(), 24.0, 0.03) && near_f(r.center.z, 0.0, 1e-4));
    }

    #[test]
    fn a_plane_in_the_rim_of_a_hollow_tube_gives_both_diameters_and_the_wall() {
        let m = tube();
        for z in [0.0, 30.0] {
            let at = |x: f32| Some(Vec3::new(x, 0.0, z));
            let s = section(
                &m,
                &Mat4::IDENTITY,
                Vec3::new(0.0, 0.0, z),
                Vec3::Z,
                at(10.0),
            );
            assert_eq!(diameters(&s).len(), 2, "z {z}");
            assert!(near_f(diameters(&s)[0], 20.0, 0.03) && near_f(diameters(&s)[1], 16.0, 0.03));
            assert!(near_f(s.walls()[0].thickness, 2.0, 0.05));
            // The contour nearest to a click on the outer or the inner edge of the rim.
            assert_eq!(s.nearest(at(10.0).unwrap()), Some(0));
            assert_eq!(s.nearest(at(8.0).unwrap()), Some(1));
        }
    }

    #[test]
    fn a_plane_in_a_ledge_takes_the_side_nearest_to_the_click() {
        // A wide disc (radius 10, z 0 to 10) under a thin post (radius 3, z 10 to 30): the
        // plane z = 10 is the disc's top face and the post's base.
        let mut m = primitives::prism(64, 10.0, 10.0);
        let post =
            primitives::prism(64, 3.0, 20.0).transformed(&Mat4::from_translation(Vec3::Z * 10.0));
        let n = m.positions.len() as u32;
        m.positions.extend(post.positions.iter().copied());
        m.triangles
            .extend(post.triangles.iter().map(|t| t.map(|i| i + n)));
        let cut = |near| {
            section(
                &m,
                &Mat4::IDENTITY,
                Vec3::new(0.0, 0.0, 10.0),
                Vec3::Z,
                near,
            )
        };
        let disc = cut(Some(Vec3::new(10.0, 0.0, 10.0)));
        assert!(near_f(
            disc.contours[disc.nearest(Vec3::new(10.0, 0.0, 10.0)).unwrap()]
                .outline
                .round()
                .unwrap()
                .diameter(),
            20.0,
            0.03
        ));
        let post = cut(Some(Vec3::new(3.0, 0.0, 10.0)));
        assert!(near_f(
            post.contours[post.nearest(Vec3::new(3.0, 0.0, 10.0)).unwrap()]
                .outline
                .round()
                .unwrap()
                .diameter(),
            6.0,
            0.03
        ));
    }
}
