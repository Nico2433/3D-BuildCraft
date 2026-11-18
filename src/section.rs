//! Cross-sections: the outline a plane leaves on a mesh, with a circle fitted to each
//! contour. This is the one way to measure a diameter: a cylinder, a cone at any height, a
//! pot's mouth (outer and inner diameter, centre, wall thickness), a hole, a rim or a base.
//! The Measure tool's "Section at height" mode and the MCP `measure` tool (`section`) share
//! it.
//!
//! A contour is *circular* when a circle fits its points (see [`Outline::why_not`]): RMS
//! distance from the circle under [`CIRCLE_RMS`] of the radius, no point further than
//! [`CIRCLE_MAX`], at least [`MIN_CLOSED`] points round a closed contour that fills
//! [`MIN_FILL`] of its circle (a hexagon or a square also has all its corners on a circle but
//! is a polygon), or for an open one [`MIN_ARC`] points spanning [`MIN_SWEEP_DEG`] degrees.
//!
//! A click on a tilted round feature (a pot whose head is tilted) is cut by a tilted plane:
//! [`section_tilted`] with [`Tilt::Auto`] turns the plane through the point to the feature's
//! axis, so the diameter is the true one and not an ellipse's.
//!
//! When the plane lies in a flat face (the top or bottom of a cylinder, the base of a cone,
//! the rim of a hollow cylinder) it cuts no material, so [`section`] slices a hair inside
//! the solid instead ([`SHIFT`] of the object's size) on the side that has material.
use crate::measure::{centroid, fit_circle_around, plane_basis, Round};
use crate::mesh::Mesh;
use glam::{Mat4, Vec2, Vec3};
use rustc_hash::FxHashMap as HashMap;

/// RMS distance of a contour's points from its circle, as a fraction of the radius.
pub const CIRCLE_RMS: f64 = 0.01;
/// The largest distance of any point from the circle, as a fraction of the radius.
pub const CIRCLE_MAX: f64 = 0.025;
/// The area a closed contour must enclose, as a fraction of its circle's: a thin sliver
/// along a rim's crest has all its points on a circle but is not a section of a round body
/// (an inscribed octagon fills 0.90).
pub const MIN_FILL: f32 = 0.85;
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
    /// Whether [`Cutter::cut_round`] turned the plane away from horizontal to find a round
    /// contour.
    pub auto_tilted: bool,
    pub contours: Vec<Contour>,
}

impl Section {
    /// How far the plane is tilted from horizontal, degrees (0 to 90).
    pub fn tilt_degrees(&self) -> f32 {
        self.normal.z.abs().clamp(0.0, 1.0).acos().to_degrees()
    }

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
    Cutter::new(mesh, model).cut(point, normal, near)
}

/// Whether [`section_tilted`] may turn the plane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tilt {
    /// Cut with a horizontal plane.
    None,
    /// Cut horizontally unless that contour is not round, then look for the plane through
    /// the point that cuts the roundest contour (see [`Cutter::cut_round`]).
    Auto,
}

/// The section a click gives: horizontal through `point`, or with [`Tilt::Auto`] the tilted
/// plane that cuts a round contour near `near` when the horizontal one is not round.
pub fn section_tilted(
    mesh: &Mesh,
    model: &Mat4,
    point: Vec3,
    near: Option<Vec3>,
    tilt: Tilt,
) -> Section {
    let cutter = Cutter::new(mesh, model);
    match tilt {
        Tilt::None => cutter.cut(point, Vec3::Z, near),
        Tilt::Auto => cutter.cut_round(point, near),
    }
}

/// How many slabs of height the triangles are sorted into by their lowest point.
const Z_BINS: usize = 256;
/// A triangle taller than this many slabs is tested against every plane.
const TALL_BINS: f32 = 4.0;

/// A mesh placed in the world once, so that many planes can cut it. Triangles are sorted
/// into slabs of height, so a plane that is nearly horizontal only looks at the few slabs
/// it passes through.
pub struct Cutter<'a> {
    mesh: &'a Mesh,
    world: Vec<Vec3>,
    /// The corners of the box round the object.
    bounds: (Vec3, Vec3),
    /// Triangle numbers by the slab their lowest vertex is in (slab `i` starts at
    /// `bounds.0.z + i * slab`), and those taller than [`TALL_BINS`] slabs.
    bins: Vec<Vec<u32>>,
    tall: Vec<u32>,
    slab: f32,
    /// Positions closer than this are one point (a millionth of the object's size).
    weld: f32,
    /// The object's largest size, mm.
    size: f32,
    /// How far a plane lying in a flat face is moved into the material (see [`SHIFT`]).
    shift: f32,
}

impl<'a> Cutter<'a> {
    pub fn new(mesh: &'a Mesh, model: &Mat4) -> Self {
        let world: Vec<Vec3> = mesh
            .positions
            .iter()
            .map(|p| model.transform_point3(*p))
            .collect();
        let (lo, hi) = world.iter().fold(
            (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
            |(lo, hi), p| (lo.min(*p), hi.max(*p)),
        );
        let size = (hi - lo).max_element().max(0.0);
        let slab = ((hi.z - lo.z) / Z_BINS as f32).max(f32::MIN_POSITIVE);
        let mut bins = vec![Vec::new(); Z_BINS];
        let mut tall = Vec::new();
        for (n, t) in mesh.triangles.iter().enumerate() {
            let z = t.map(|i| world[i as usize].z);
            let (low, high) = (z[0].min(z[1]).min(z[2]), z[0].max(z[1]).max(z[2]));
            if high - low > TALL_BINS * slab {
                tall.push(n as u32);
            } else {
                bins[(((low - lo.z) / slab) as usize).min(Z_BINS - 1)].push(n as u32);
            }
        }
        Cutter {
            mesh,
            bounds: (lo, hi),
            bins,
            tall,
            slab,
            weld: size.max(1.0) * 1e-6,
            size,
            shift: SHIFT * size,
            world,
        }
    }

    /// The triangles that may cross the plane through `point` with `normal`: those whose
    /// height range meets the heights the plane has inside the object's box.
    fn near_plane(&self, point: Vec3, normal: Vec3) -> Vec<[u32; 3]> {
        let (lo, hi) = self.bounds;
        let pad = 2.0 * self.shift;
        let (zlo, zhi) = if normal.z.abs() < 1e-6 {
            (f32::NEG_INFINITY, f32::INFINITY)
        } else {
            let at = |x: f32, y: f32| {
                point.z - (normal.x * (x - point.x) + normal.y * (y - point.y)) / normal.z
            };
            let z = [(lo.x, lo.y), (lo.x, hi.y), (hi.x, lo.y), (hi.x, hi.y)].map(|(x, y)| at(x, y));
            let fold = |f: fn(f32, f32) -> f32, start: f32| z.iter().fold(start, |a, b| f(a, *b));
            (
                fold(f32::min, f32::INFINITY) - pad,
                fold(f32::max, f32::NEG_INFINITY) + pad,
            )
        };
        // A triangle of at most TALL_BINS slabs that reaches `zlo` starts in the slab
        // that many below it.
        let first = ((zlo - lo.z) / self.slab - TALL_BINS)
            .floor()
            .clamp(0.0, Z_BINS as f32) as usize;
        let last = ((zhi - lo.z) / self.slab)
            .floor()
            .clamp(-1.0, (Z_BINS - 1) as f32);
        let bins = self
            .bins
            .get(first..=(last.max(0.0) as usize).max(first))
            .unwrap_or(&[]);
        bins.iter()
            .flatten()
            .chain(&self.tall)
            .map(|&n| self.mesh.triangles[n as usize])
            .filter(|t| {
                let z = t.map(|i| self.world[i as usize].z);
                z[0].min(z[1]).min(z[2]) <= zhi && z[0].max(z[1]).max(z[2]) >= zlo
            })
            .collect()
    }

    /// [`section`] for this mesh.
    pub fn cut(&self, point: Vec3, normal: Vec3, near: Option<Vec3>) -> Section {
        let normal = normal.try_normalize().unwrap_or(Vec3::Z);
        let tris = self.near_plane(point, normal);
        let shift = self.shift;
        let dist = |i: u32| (self.world[i as usize] - point).dot(normal);
        let in_a_face = shift > 0.0
            && tris
                .iter()
                .any(|t| t.iter().all(|&i| dist(i).abs() <= shift / 2.0));
        let plane = Plane {
            point,
            normal,
            shift: 0.0,
        };
        let slice = |plane| slice(&self.world, self.weld, &tris, plane);
        if !in_a_face {
            return slice(plane);
        }
        let [up, down] = [shift, -shift].map(|s| slice(Plane { shift: s, ..plane }));
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
}

/// A plane's section and how round the contour nearest to the probe is.
#[derive(Clone)]
struct Trial {
    section: Section,
    /// RMS distance of that contour from its circle as a fraction of the radius; infinite
    /// when there is no closed contour of enough points.
    cost: f32,
}

impl Trial {
    fn contour(&self, near: Vec3) -> Option<&Contour> {
        self.section.contours.get(self.section.nearest(near)?)
    }

    fn round(&self, near: Vec3) -> bool {
        self.contour(near).is_some_and(|c| c.outline.is_circular())
    }
}

/// The most the best plane may be turned from horizontal, degrees.
const MAX_TILT_DEG: f32 = 60.0;
/// Rounds of "turn the plane to where the ellipse says the axis is" before giving up.
const MAX_TURNS: usize = 8;
/// A turn smaller than this (degrees) is not worth another cut: it changes a diameter by
/// less than a hundred thousandth.
const MIN_TURN_DEG: f64 = 0.2;
/// Degrees either side of a direction round a ring in which [`Cutter::cut_round`] looks for
/// the highest point of the rim.
const RIM_SECTOR_DEG: f32 = 4.0;
/// How far under the point (as a fraction of the object's size) the final plane may go to
/// leave the crest of a rounded rim: none first, then 0.1, 0.3 and 1 mm on a 100 mm pot.
const NUDGES: [f32; 4] = [0.0, 0.001, 0.003, 0.01];
/// How far in from the point (as a fraction of the object's size) the axis is looked for when
/// the plane through the point itself shows no complete contour: a click just under a tilted
/// rim has the rim's low side above the plane.
const AXIS_DEPTHS: [f32; 3] = [0.04, 0.1, 0.2];

impl Cutter<'_> {
    fn trial(&self, point: Vec3, normal: Vec3, near: Vec3) -> Trial {
        let section = self.cut(point, normal, Some(near));
        let cost = section
            .nearest(near)
            .map(|i| &section.contours[i].outline)
            .filter(|o| o.closed && o.points.len() >= MIN_CLOSED)
            .and_then(|o| o.fit)
            .map_or(f32::INFINITY, |f| f.rms);
        Trial { section, cost }
    }

    /// The axis of the round feature the plane through `point` with `normal` cuts near
    /// `near`: the normal turned round after round to where the contour's ellipse says it
    /// is, while that makes the contour rounder.
    fn axis(&self, point: Vec3, near: Vec3, normal: Vec3) -> (Vec3, Option<Round>) {
        let mut best = self.trial(point, normal, near);
        for _ in 0..MAX_TURNS {
            let Some((turned, degrees)) = best
                .contour(near)
                .and_then(|c| turned(c, best.section.normal))
            else {
                break;
            };
            if degrees < MIN_TURN_DEG {
                break;
            }
            let next = turned
                .into_iter()
                .map(|n| self.trial(point, n, near))
                .find(|t| t.cost < best.cost);
            let Some(next) = next else { break };
            best = next;
        }
        let round = best.contour(near).and_then(|c| c.outline.round());
        (best.section.normal, round)
    }

    /// The plane of a ring's rim through `point` when the rim is slanted against its wall: a
    /// pot's mouth cut at an angle, found from the highest vertices of the ring (round
    /// `axis` through `circle`) on `point`'s side and 120 degrees either way, and the plane through them moved to `point`. `None` when the
    /// ring has no vertices there or the plane is tilted more than [`MAX_TILT_DEG`].
    fn mouth(&self, point: Vec3, axis: Vec3, circle: Round) -> Option<Vec3> {
        let (u, v) = plane_basis(axis);
        let at = |p: Vec3| {
            let d = p - circle.center;
            Vec2::new(d.dot(u), d.dot(v))
        };
        let start = at(point).normalize_or_zero();
        // The direction of each of the three points round the ring.
        let dirs = [0.0f32, 120.0, 240.0].map(|deg| {
            let (s, c) = deg.to_radians().sin_cos();
            Vec2::new(start.x * c - start.y * s, start.x * s + start.y * c)
        });
        let band = (circle.radius * 0.08).max(4.0);
        let cos = RIM_SECTOR_DEG.to_radians().cos();
        let mut top: [Option<(f32, Vec3)>; 3] = [None; 3];
        for p in &self.world {
            let d = at(*p);
            let r = d.length();
            if (r - circle.radius).abs() > band || r < f32::MIN_POSITIVE {
                continue;
            }
            let height = (*p - circle.center).dot(axis);
            for (slot, dir) in top.iter_mut().zip(dirs) {
                if d.dot(dir) >= cos * r && slot.is_none_or(|(h, _)| height > h) {
                    *slot = Some((height, *p));
                }
            }
        }
        let [Some((_, a)), Some((_, b)), Some((_, c))] = top else {
            return None;
        };
        let n = (b - a).cross(c - a).try_normalize()?;
        let n = if n.z < 0.0 { -n } else { n };
        (n.z >= MAX_TILT_DEG.to_radians().cos()).then_some(n)
    }

    /// The section through `point` that is round near `near` (`point` when none): the
    /// horizontal one when its contour is round. Else a plane through `point` whose normal
    /// is the axis of the round feature there, found by turning a plane through a point a
    /// little inwards (where a tilted rim no longer cuts the contour short) to where its
    /// contour's ellipse puts the axis; or the plane of a slanted rim on an upright wall
    /// (a pot's mouth cut at an angle), found from the ring's highest points. The plane may
    /// be taken a hair under `point`, below the crest of a rounded rim. When no such plane
    /// cuts a round contour the horizontal section is returned. A turned plane says so in
    /// [`Section::auto_tilted`].
    pub fn cut_round(&self, point: Vec3, near: Option<Vec3>) -> Section {
        let near = near.unwrap_or(point);
        let flat = self.trial(point, Vec3::Z, near);
        if flat.round(near) {
            return flat.section;
        }
        // The plane with `normal` through `point`, or a hair under it when `point` is on the
        // crest of a rounded rim that has no contour of its own.
        let found = |n: Vec3| {
            NUDGES.iter().find_map(|d| {
                let t = self.trial(point - n * (d * self.size), n, near);
                t.round(near).then(|| {
                    let mut section = t.section;
                    section.auto_tilted = section.tilt_degrees() > MIN_TURN_DEG as f32;
                    section
                })
            })
        };
        // Towards the middle of the object's height.
        let inwards = if point.z > (self.bounds.0.z + self.bounds.1.z) / 2.0 {
            -1.0
        } else {
            1.0
        };
        for depth in AXIS_DEPTHS {
            let inwards = Vec3::Z * depth * self.size * inwards;
            let (n, round) = self.axis(point + inwards, near + inwards, Vec3::Z);
            let mouth = round.and_then(|r| self.mouth(point, n, r));
            if let Some(s) = [Some(n), mouth].into_iter().flatten().find_map(found) {
                return s;
            }
        }
        flat.section
    }
}

/// Where a cylinder's axis is if the closed `contour` cut by the plane with `normal` is its
/// ellipse: tilted from the normal by the angle whose cosine is minor/major, towards the
/// major axis, on either side (a plane cannot say which): the one that puts the contour's
/// points more nearly at the same distance from the axis first. Also the angle, degrees.
/// `None` when the contour is no ellipse to speak of or an axis is tilted more than
/// [`MAX_TILT_DEG`] from vertical.
fn turned(contour: &Contour, normal: Vec3) -> Option<([Vec3; 2], f64)> {
    let points = &contour.outline.points;
    if !contour.outline.closed || points.len() < MIN_CLOSED {
        return None;
    }
    let (u, v) = plane_basis(normal);
    let flat: Vec<(f64, f64)> = points
        .iter()
        .map(|p| {
            (
                (*p - points[0]).dot(u) as f64,
                (*p - points[0]).dot(v) as f64,
            )
        })
        .collect();
    // Second moments of the polygon's area (exact for any sampling of the boundary).
    let (mut area, mut cx, mut cy) = (0.0, 0.0, 0.0);
    let (mut sxx, mut syy, mut sxy) = (0.0, 0.0, 0.0);
    for (i, &(x0, y0)) in flat.iter().enumerate() {
        let (x1, y1) = flat[(i + 1) % flat.len()];
        let w = x0 * y1 - x1 * y0;
        area += w / 2.0;
        cx += (x0 + x1) * w / 6.0;
        cy += (y0 + y1) * w / 6.0;
        sxx += (x0 * x0 + x0 * x1 + x1 * x1) * w / 12.0;
        syy += (y0 * y0 + y0 * y1 + y1 * y1) * w / 12.0;
        sxy += (x0 * y1 + 2.0 * x0 * y0 + 2.0 * x1 * y1 + x1 * y0) * w / 24.0;
    }
    if area.abs() < 1e-9 {
        return None;
    }
    let sign = area.signum();
    let (area, cx, cy) = (area * sign, cx * sign / area.abs(), cy * sign / area.abs());
    let (sxx, syy, sxy) = (
        sxx * sign - area * cx * cx,
        syy * sign - area * cy * cy,
        sxy * sign - area * cx * cy,
    );
    let mean = (sxx + syy) / 2.0;
    let spread = (((sxx - syy) / 2.0).powi(2) + sxy * sxy).sqrt();
    if mean - spread <= 0.0 {
        return None;
    }
    let cos = ((mean - spread) / (mean + spread)).sqrt();
    let phi = 0.5 * (2.0 * sxy).atan2(sxx - syy);
    let major = u * phi.cos() as f32 + v * phi.sin() as f32;
    let sin = (1.0 - cos * cos).sqrt() as f32;
    let candidates = [1.0f32, -1.0].map(|side| {
        let n = normal * cos as f32 + major * (side * sin);
        if n.z < 0.0 {
            -n
        } else {
            n
        }
    });
    // How unevenly the points sit round the line through the centre along `axis`.
    let centre = points[0] + u * cx as f32 + v * cy as f32;
    let uneven = |axis: Vec3| {
        let radii: Vec<f32> = points
            .iter()
            .map(|p| {
                let d = *p - centre;
                (d - axis * d.dot(axis)).length()
            })
            .collect();
        let mean = radii.iter().sum::<f32>() / radii.len() as f32;
        radii.iter().map(|r| (r - mean).powi(2)).sum::<f32>() / mean.max(f32::MIN_POSITIVE)
    };
    let [a, b] = candidates;
    let ordered = if uneven(a) <= uneven(b) {
        [a, b]
    } else {
        [b, a]
    };
    let limit = MAX_TILT_DEG.to_radians().cos();
    ordered
        .iter()
        .all(|n| n.z >= limit)
        .then_some((ordered, cos.acos().to_degrees()))
}

/// A plane through `point` moved `shift` along its normal.
#[derive(Clone, Copy)]
struct Plane {
    point: Vec3,
    normal: Vec3,
    shift: f32,
}

/// The outline of the triangles `tris` (vertices `world`, those within `weld` of each other
/// being one) on `plane`; the points are projected back onto the plane through
/// `plane.point`.
fn slice(world: &[Vec3], weld: f32, tris: &[[u32; 3]], plane: Plane) -> Section {
    let Plane {
        point,
        normal,
        shift,
    } = plane;
    // A crossing point per mesh edge, and the segments between the two of each triangle.
    let mut index: HashMap<([i32; 3], [i32; 3]), u32> = HashMap::default();
    let grid = |p: Vec3| (p / weld).round().as_ivec3().to_array();
    let mut points: Vec<Vec3> = Vec::new();
    let mut segments: Vec<[u32; 2]> = Vec::new();
    let dist = |i: u32| (world[i as usize] - point).dot(normal);
    for t in tris {
        let above = t.map(|i| dist(i) >= shift);
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
            let (ga, gb) = (grid(world[a as usize]), grid(world[b as usize]));
            let key = (ga.min(gb), ga.max(gb));
            ends[n] = *index.entry(key).or_insert_with(|| {
                let (da, db) = (dist(a) - shift, dist(b) - shift);
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
    let (u, v) = plane_basis(normal);
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
        auto_tilted: false,
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
    /// The area the points enclose (an open contour closed by its chord) as a fraction of
    /// the circle's.
    pub fill: f32,
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
                "a closed contour of {n} vertices is a polygon, not round (a cube gives a square); a round one needs at least {MIN_CLOSED} vertices"
            ))
        } else if self.closed && fit.fill < MIN_FILL {
            Some(format!(
                "it encloses {:.0}% of its fitted circle, a sliver; a round contour needs at least {:.0}%",
                fit.fill * 100.0,
                MIN_FILL * 100.0
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
    let area = normal.length() / 2.0;
    let mut axis = normal.try_normalize()?;
    // A stable direction for the axis: its largest component is positive.
    let big = axis.abs().max_element();
    if axis.to_array().iter().any(|x| *x == -big) {
        axis = -axis;
    }
    let (mut round, _) = fit_circle_around(points, axis)?;
    round.radius = round.radius.max(f32::MIN_POSITIVE);
    let r = round.radius as f64;
    let (u, v) = plane_basis(axis);
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
        fill: area / (std::f32::consts::PI * round.radius * round.radius),
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

    /// A cylinder tilted `deg` degrees about X, lifted to z 50.
    fn tilted(deg: f32) -> Mat4 {
        Mat4::from_translation(Vec3::Z * 50.0) * Mat4::from_rotation_x(deg.to_radians())
    }

    #[test]
    fn a_tilted_cylinder_clicked_on_its_wall_or_rim_gives_its_true_diameter() {
        let m = primitives::prism(96, 10.0, 30.0);
        let model = tilted(20.0);
        // Points on the wall half way up, at the top rim's edge, and on its tilted face.
        let rim = |angle: f32, r: f32, h: f32| {
            model.transform_point3(Vec3::new(r * angle.cos(), r * angle.sin(), h))
        };
        for (name, p) in [
            ("wall", rim(0.3, 10.0, 15.0)),
            ("rim edge", rim(2.0, 10.0, 30.0)),
            ("rim face", rim(4.0, 9.0, 30.0)),
            ("base edge", rim(1.0, 10.0, 0.0)),
        ] {
            let s = section_tilted(&m, &model, p, Some(p), Tilt::Auto);
            let c = &s.contours[s.nearest(p).unwrap()];
            let d = c
                .outline
                .round()
                .unwrap_or_else(|| panic!("{name}: not round"))
                .diameter();
            assert!(s.auto_tilted, "{name}");
            assert!(near_f(d, 20.0, 0.03), "{name}: {d}");
            assert!(
                near_f(s.tilt_degrees(), 20.0, 0.1),
                "{name}: {}",
                s.tilt_degrees()
            );
        }
    }

    #[test]
    fn an_upright_cylinder_is_not_tilted_and_tilt_none_stays_horizontal() {
        let m = primitives::prism(96, 10.0, 30.0);
        let p = Vec3::new(10.0, 0.0, 15.0);
        let s = section_tilted(&m, &Mat4::IDENTITY, p, Some(p), Tilt::Auto);
        assert!(!s.auto_tilted && s.tilt_degrees() < 1e-3);
        let model = tilted(20.0);
        let q = model.transform_point3(Vec3::new(10.0, 0.0, 15.0));
        let flat = section_tilted(&m, &model, q, Some(q), Tilt::None);
        assert!(!flat.auto_tilted && flat.tilt_degrees() < 1e-3);
        assert!(flat.contours[0].outline.round().is_none());
    }

    #[test]
    fn a_thin_sliver_along_a_circle_is_not_round() {
        // Out along a circle of radius 10 and back along one of 9.95: every point fits a
        // circle, the loop encloses almost nothing.
        let arc = |r: f32, i: usize| {
            let a = i as f32 / 24.0 * std::f32::consts::TAU;
            Vec3::new(r * a.cos(), r * a.sin(), 0.0)
        };
        let points: Vec<Vec3> = (0..24)
            .map(|i| arc(10.0, i))
            .chain((0..24).rev().map(|i| arc(9.95, i)))
            .collect();
        let sliver = Outline::new(points, true);
        assert!(sliver.why_not().unwrap().contains("sliver"));
        let disc = Outline::new((0..24).map(|i| arc(10.0, i)).collect(), true);
        assert!(disc.is_circular() && disc.fit.unwrap().fill > 0.98);
    }
}
