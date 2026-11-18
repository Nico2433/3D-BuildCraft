//! The Measure tool's model, independent of the UI: diameter and radius of round geometry
//! (a circle through three points, a curved face, a whole object) and the picked points with
//! their snapping. The Measure tool in `app` and the MCP `measure` tool share it.
use crate::mesh::Mesh;
use crate::scene::{Object, Scene};
use crate::section::{section_tilted, Outline, Section, Tilt};
use crate::snapping::{align, snap_point, Alignment, FaceInfo, PointSnap};
use glam::{Mat4, Vec3};
use std::sync::Arc;

/// A fit counts as round when its RMS error is under this fraction of the radius.
const FIT_TOLERANCE: f64 = 0.005;
/// A single curved face is fitted from few, coarse triangles, so it gets twice the room.
const SURFACE_FIT_TOLERANCE: f64 = 2.0 * FIT_TOLERANCE;
/// Two unit normals closer to parallel than this (length of their cross product) do not
/// say which way a surface curves.
const MIN_AXIS_LENGTH: f32 = 0.05;
/// A cylinder's face normals are square to its axis within this (cosine of the angle).
const SQUARE_TO_AXIS: f32 = 0.05;
/// Degrees between neighbouring triangles of one curved face.
const CURVED_FACE_DEG: f32 = 30.0;
/// A sphere's face normals point away from its centre within this cosine.
const RADIAL_COS: f32 = 0.98;
/// A ring of a cone or torus: the vertices at least this fraction of the widest radius.
const RING_FRACTION: f32 = 0.998;
/// Angular sectors that vertices must fill to go "all the way round" a circle, how many of
/// them (a box's corners fill four) and the fewest vertices for that.
const SECTORS: u32 = 12;
const MIN_SECTORS: usize = 10;
const MIN_ROUND_POINTS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoundKind {
    Sphere,
    Cylinder,
    /// A circle: through three points, or the widest ring of a cone, torus or similar.
    Circle,
}

impl RoundKind {
    pub fn name(self) -> &'static str {
        match self {
            RoundKind::Sphere => "sphere",
            RoundKind::Cylinder => "cylinder",
            RoundKind::Circle => "circle",
        }
    }
}

/// A fitted circle, cylinder or sphere. `axis` is the circle's normal or the cylinder's
/// axis (a unit vector); a sphere has none.
#[derive(Clone, Copy, Debug)]
pub struct Round {
    pub kind: RoundKind,
    pub center: Vec3,
    pub axis: Option<Vec3>,
    pub radius: f32,
}

impl Round {
    pub fn diameter(&self) -> f32 {
        self.radius * 2.0
    }
}

/// The circle through three points; `None` when they are on a line or repeated.
pub fn circle_through(a: Vec3, b: Vec3, c: Vec3) -> Option<Round> {
    let (ab, ac) = (b - a, c - a);
    let n = ab.cross(ac);
    let n2 = n.length_squared();
    if n2 <= 1e-10 * ab.length_squared() * ac.length_squared() || n2 == 0.0 {
        return None;
    }
    let offset =
        (ac.cross(n) * ab.length_squared() + n.cross(ab) * ac.length_squared()) / (2.0 * n2);
    Some(Round {
        kind: RoundKind::Circle,
        center: a + offset,
        axis: Some(n.normalize()),
        radius: offset.length(),
    })
}

/// Two unit vectors that, with the unit vector `axis`, make a right-handed frame.
pub fn plane_basis(axis: Vec3) -> (Vec3, Vec3) {
    let u = axis.any_orthonormal_vector();
    (u, axis.cross(u))
}

/// Least-squares solution of `rows · x = rhs` by the normal equations.
fn least_squares<const N: usize>(rows: &[[f64; N]], rhs: &[f64]) -> Option<[f64; N]> {
    let mut m = [[0.0; N]; N];
    let mut v = [0.0; N];
    for (r, b) in rows.iter().zip(rhs) {
        for i in 0..N {
            v[i] += r[i] * b;
            for j in 0..N {
                m[i][j] += r[i] * r[j];
            }
        }
    }
    let scale = (0..N).map(|i| m[i][i]).fold(0.0, f64::max);
    for i in 0..N {
        let pivot = (i..N).max_by(|&a, &b| m[a][i].abs().total_cmp(&m[b][i].abs()))?;
        if m[pivot][i].abs() <= 1e-12 * scale {
            return None;
        }
        m.swap(i, pivot);
        v.swap(i, pivot);
        for r in i + 1..N {
            let f = m[r][i] / m[i][i];
            for c in i..N {
                m[r][c] -= f * m[i][c];
            }
            v[r] -= f * v[i];
        }
    }
    let mut x = [0.0; N];
    for i in (0..N).rev() {
        let tail: f64 = (i + 1..N).map(|j| m[i][j] * x[j]).sum();
        x[i] = (v[i] - tail) / m[i][i];
    }
    Some(x)
}

pub(crate) fn centroid(points: &[Vec3]) -> Vec3 {
    points.iter().sum::<Vec3>() / points.len().max(1) as f32
}

/// The RMS distance of `points` from the round shape, given the distance of each.
fn rms(distances: impl Iterator<Item = f64>, radius: f64) -> f64 {
    let (n, sum) = distances.fold((0.0, 0.0), |(n, s), d| (n + 1.0, s + (d - radius).powi(2)));
    (sum / f64::max(n, 1.0)).sqrt()
}

/// The circle that best fits `points` seen along `axis`, with its relative RMS error.
pub(crate) fn fit_circle_around(points: &[Vec3], axis: Vec3) -> Option<(Round, f64)> {
    if points.len() < 3 {
        return None;
    }
    let axis = axis.try_normalize()?;
    let (u, v) = plane_basis(axis);
    let origin = centroid(points);
    let flat: Vec<[f64; 2]> = points
        .iter()
        .map(|p| [(*p - origin).dot(u) as f64, (*p - origin).dot(v) as f64])
        .collect();
    let rows: Vec<[f64; 3]> = flat.iter().map(|p| [p[0], p[1], 1.0]).collect();
    let rhs: Vec<f64> = flat.iter().map(|p| -(p[0] * p[0] + p[1] * p[1])).collect();
    let [d, e, f] = least_squares(&rows, &rhs)?;
    let (cx, cy) = (-d / 2.0, -e / 2.0);
    let r2 = cx * cx + cy * cy - f;
    if r2 <= 0.0 {
        return None;
    }
    let r = r2.sqrt();
    let error = rms(flat.iter().map(|p| (p[0] - cx).hypot(p[1] - cy)), r) / r;
    // The centre stays at the points' height along the axis.
    let center = origin + u * cx as f32 + v * cy as f32;
    let round = Round {
        kind: RoundKind::Circle,
        center,
        axis: Some(axis),
        radius: r as f32,
    };
    Some((round, error))
}

/// The sphere that best fits `points`, with its relative RMS error.
fn fit_sphere(points: &[Vec3]) -> Option<(Round, f64)> {
    if points.len() < 4 {
        return None;
    }
    let origin = centroid(points);
    let local: Vec<[f64; 3]> = points
        .iter()
        .map(|p| (*p - origin).to_array().map(f64::from))
        .collect();
    let rows: Vec<[f64; 4]> = local.iter().map(|p| [p[0], p[1], p[2], 1.0]).collect();
    let rhs: Vec<f64> = local
        .iter()
        .map(|p| -(p[0] * p[0] + p[1] * p[1] + p[2] * p[2]))
        .collect();
    let [d, e, f, g] = least_squares(&rows, &rhs)?;
    let c = [-d / 2.0, -e / 2.0, -f / 2.0];
    let r2 = c.iter().map(|x| x * x).sum::<f64>() - g;
    if r2 <= 0.0 {
        return None;
    }
    let r = r2.sqrt();
    let error = rms(
        local.iter().map(|p| {
            ((p[0] - c[0]).powi(2) + (p[1] - c[1]).powi(2) + (p[2] - c[2]).powi(2)).sqrt()
        }),
        r,
    ) / r;
    let round = Round {
        kind: RoundKind::Sphere,
        center: origin + Vec3::new(c[0] as f32, c[1] as f32, c[2] as f32),
        axis: None,
        radius: r as f32,
    };
    Some((round, error))
}

/// Fit a curved surface from its vertices and the unit normals of its triangles: a
/// cylinder when every normal is square to one axis, else a sphere. `None` for a flat
/// surface or one that is neither.
pub fn fit_surface(points: &[Vec3], normals: &[Vec3]) -> Option<Round> {
    let first = *normals.first()?;
    let far = normals
        .iter()
        .min_by(|a, b| first.dot(**a).abs().total_cmp(&first.dot(**b).abs()))?;
    let axis = first.cross(*far);
    if axis.length() < MIN_AXIS_LENGTH {
        return None;
    }
    let axis = axis.normalize();
    let fit = if normals.iter().all(|n| n.dot(axis).abs() < SQUARE_TO_AXIS) {
        fit_circle_around(points, axis).map(|(r, e)| {
            (
                Round {
                    kind: RoundKind::Cylinder,
                    ..r
                },
                e,
            )
        })
    } else {
        fit_sphere(points)
    };
    fit.filter(|(_, e)| *e < SURFACE_FIT_TOLERANCE)
        .map(|(r, _)| r)
}

/// The curved face around triangle `tri` of `mesh` placed by `model`, fitted (see
/// [`fit_surface`]). Neighbouring triangles up to [`CURVED_FACE_DEG`] apart belong to it.
pub fn surface_round(mesh: &Mesh, model: &Mat4, tri: usize) -> Option<Round> {
    let region = mesh.face_region(tri, CURVED_FACE_DEG);
    let mut points = Vec::new();
    let mut normals = Vec::new();
    for t in region {
        let v = mesh.triangles[t].map(|i| model.transform_point3(mesh.positions[i as usize]));
        normals.push(Mesh::face_normal(v[0], v[1], v[2]));
        points.extend(v);
    }
    fit_surface(&points, &normals)
}

/// Whether `points` go all the way round `round`: a box's corners fit a circle but do not
/// cover it.
fn goes_round(points: &[Vec3], round: &Round) -> bool {
    let (u, v) = plane_basis(round.axis.unwrap_or(Vec3::Z));
    let sectors: std::collections::HashSet<u32> = points
        .iter()
        .map(|p| {
            let d = *p - round.center;
            let angle = d.dot(v).atan2(d.dot(u));
            ((angle + std::f32::consts::PI) / std::f32::consts::TAU * SECTORS as f32) as u32
                % SECTORS
        })
        .collect();
    points.len() >= MIN_ROUND_POINTS && sectors.len() >= MIN_SECTORS
}

/// The round shape of a whole object, given its mesh in world space and the world
/// directions of its own axes: a cylinder if the vertices form full circles about an axis,
/// a sphere if they fit one and its faces face away from the centre, else the widest ring
/// about an axis (a cone's base, a torus's outer circle). `None` if none fit.
pub fn fit_object(world: &Mesh, axes: [Vec3; 3]) -> Option<Round> {
    let points = &world.positions;
    let best = |fits: Vec<(Round, f64)>| {
        fits.into_iter()
            .filter(|(_, e)| *e < FIT_TOLERANCE)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(r, _)| r)
    };
    // An axis with no length (an object scaled flat) says nothing.
    let axes: Vec<Vec3> = axes.iter().filter_map(|a| a.try_normalize()).collect();
    let full =
        |pts: &[Vec3], axis: Vec3| fit_circle_around(pts, axis).filter(|(r, _)| goes_round(pts, r));
    if let Some(r) = best(axes.iter().filter_map(|a| full(points, *a)).collect()) {
        return Some(Round {
            kind: RoundKind::Cylinder,
            ..r
        });
    }
    if let Some((s, e)) = fit_sphere(points) {
        let radial = world.triangle_positions().all(|[a, b, c]| {
            let n = Mesh::face_normal(a, b, c);
            n.dot(((a + b + c) / 3.0 - s.center).normalize_or_zero())
                .abs()
                > RADIAL_COS
        });
        if e < FIT_TOLERANCE && radial {
            return Some(s);
        }
    }
    let centre = world.aabb().center();
    let rings = axes.iter().filter_map(|&axis| {
        let radial = |p: &Vec3| {
            let d = *p - centre;
            (d - axis * d.dot(axis)).length()
        };
        let widest = points.iter().map(radial).fold(0.0, f32::max);
        let ring: Vec<Vec3> = points
            .iter()
            .filter(|p| radial(p) >= widest * RING_FRACTION)
            .copied()
            .collect();
        full(&ring, axis)
    });
    best(rings.collect())
}

/// The object nearest to `p`, with the triangle of its surface and the point on it that is
/// closest to `p`.
pub fn nearest_surface(scene: &Scene, p: Vec3) -> Option<(&Object, usize, Vec3)> {
    scene
        .objects
        .iter()
        .filter_map(|o| {
            let (tri, q) = o.mesh.closest_triangle(&o.matrix(), p)?;
            Some((o, tri, q))
        })
        .min_by(|a, b| a.2.distance(p).total_cmp(&b.2.distance(p)))
}

/// Snap `p` to the nearest face feature (vertex, edge middle, edge, face centre) of any
/// object within `tolerance` mm of it, with that object's id; `None` when no surface is
/// that near, or when `tolerance` is 0 (no snapping).
pub fn snap_to_scene(scene: &Scene, p: Vec3, tolerance: f32) -> Option<(u64, PointSnap)> {
    if tolerance <= 0.0 {
        return None;
    }
    scene
        .objects
        .iter()
        .filter_map(|o| {
            let model = o.matrix();
            let (tri, q) = o.mesh.closest_triangle(&model, p)?;
            let face = FaceInfo::new(&o.mesh, &model, tri)?;
            (q.distance(p) <= tolerance)
                .then(|| (q.distance(p), (o.id, snap_point(&face, q, tolerance))))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, s)| s)
}

/// What the Measure tool measures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// The distance between two points.
    Distance,
    /// The diameter of the curved face under a click.
    Surface,
    /// The diameter of the circle through three points.
    Circle,
    /// The diameter, radius and centre of the cross-section at a click (the contour nearest
    /// to it, horizontal or tilted to where it is round), and the wall thickness of a hollow
    /// object there.
    Section,
}

impl Mode {
    /// The diameter variants, in the order the toolbar lists them.
    pub const ROUND: [Mode; 3] = [Mode::Section, Mode::Surface, Mode::Circle];
    /// The diameter variant a fresh tool starts on.
    pub const DEFAULT_ROUND: Mode = Mode::Section;

    /// `mode` when it is a diameter variant, otherwise `fallback`.
    fn round_or(mode: Mode, fallback: Mode) -> Mode {
        if mode == Mode::Distance {
            fallback
        } else {
            mode
        }
    }

    /// The toolbar's English label for a diameter variant (translated by the UI).
    pub fn label(self) -> &'static str {
        match self {
            Mode::Distance => "Distance",
            Mode::Surface => "Curved surface",
            Mode::Circle => "3 points",
            Mode::Section => "Section at height",
        }
    }
}

/// A picked point: where, the surface normal there and how it was lined up with the first
/// point.
#[derive(Clone, Copy, Debug)]
pub struct Spot {
    pub point: Vec3,
    pub normal: Option<Vec3>,
    pub aligned: Option<Alignment>,
}

impl Spot {
    pub fn free(point: Vec3) -> Self {
        Spot {
            point,
            normal: None,
            aligned: None,
        }
    }
}

/// A face's snapping info with what it was built from: object id, triangle, mesh, placement.
type CachedFace = (u64, usize, Arc<Mesh>, Mat4, Option<FaceInfo>);

/// Section mode's result: the object that was cut, its contours and the one that was clicked.
#[derive(Clone, Debug)]
pub struct SectionPick {
    pub object: u64,
    pub section: Section,
    pub chosen: Option<usize>,
}

/// The Measure tool's state.
pub struct Measure {
    pub mode: Mode,
    pub points: Vec<Spot>,
    /// The point under the cursor.
    pub hover: Option<Spot>,
    /// Surface mode's result.
    pub round: Option<Round>,
    /// Section mode's result.
    pub section: Option<SectionPick>,
    /// Whether Section mode may tilt its plane to where the contour is round.
    pub tilt: Tilt,
    /// The diameter variant last used, restored when switching back from Distance.
    pub round_mode: Mode,
    /// The face last snapped on, so a moving cursor does not rebuild it.
    face: Option<CachedFace>,
}

impl Measure {
    pub fn new(mode: Mode) -> Self {
        Measure {
            mode,
            points: vec![],
            hover: None,
            round: None,
            section: None,
            tilt: Tilt::Auto,
            round_mode: Mode::round_or(mode, Mode::DEFAULT_ROUND),
            face: None,
        }
    }

    /// A measurement with its points preset, for screenshots: `[mode:]point;point;…`, mode
    /// `distance`, `surface`, `circle` or `section`; a point is `x,y,z` (mm) with `~` in front for
    /// the cursor point. Surface mode fits the face nearest its point; section mode cuts the object nearest to its point at that point's height.
    pub fn preset(spec: &str, scene: &Scene) -> Self {
        let (mode, rest) = match spec.split_once(':') {
            Some(("surface", rest)) => (Mode::Surface, rest),
            Some(("circle", rest)) => (Mode::Circle, rest),
            Some(("section", rest)) => (Mode::Section, rest),
            Some((_, rest)) => (Mode::Distance, rest),
            None => (Mode::Distance, spec),
        };
        let mut m = Measure::new(mode);
        for entry in rest.split(';') {
            let v: Vec<f32> = entry
                .trim_start_matches('~')
                .split(',')
                .filter_map(|n| n.trim().parse().ok())
                .collect();
            let Ok::<[f32; 3], _>([x, y, z]) = v.try_into() else {
                continue;
            };
            let spot = Spot::free(Vec3::new(x, y, z));
            if entry.starts_with('~') {
                m.hover = Some(spot);
            } else {
                m.points.push(spot);
            }
        }
        if let (Mode::Section, Some(first)) = (mode, m.points.first()) {
            m.section = nearest_surface(scene, first.point).map(|(o, _, _)| m.cut(o, first.point));
        }
        if let (Mode::Surface, Some(first)) = (mode, m.points.first()) {
            m.round = nearest_surface(scene, first.point)
                .and_then(|(o, tri, _)| surface_round(&o.mesh, &o.matrix(), tri));
        }
        m
    }

    /// Drops the results of a measurement whose objects may have changed (after an undo or
    /// redo); the placed points are positions in space and stay.
    pub fn forget_results(&mut self) {
        self.round = None;
        self.section = None;
    }

    /// Right click: drops the last placed point and the results that came from it; with no
    /// point left, drops the section or fitted circle.
    pub fn cancel(&mut self) {
        if self.points.pop().is_some() {
            self.round = None;
        } else {
            self.forget_results();
        }
    }

    /// Whether a diameter variant is active rather than the distance.
    pub fn is_round(&self) -> bool {
        self.mode != Mode::Distance
    }

    /// Switches between the distance and the last diameter variant used.
    pub fn set_round(&mut self, round: bool) {
        self.set_mode(if round {
            self.round_mode
        } else {
            Mode::Distance
        });
    }

    pub fn set_mode(&mut self, mode: Mode) {
        let round_mode = Mode::round_or(mode, self.round_mode);
        *self = Measure {
            tilt: self.tilt,
            round_mode,
            ..Measure::new(mode)
        };
    }

    /// The section of `o` through `p`, with the contour passing closest to `p`: horizontal,
    /// or with [`Tilt::Auto`] tilted where the horizontal contour is not round. When `p` is
    /// on a flat face (a rim, a base) the cut goes a hair inside the material.
    fn cut(&self, o: &Object, p: Vec3) -> SectionPick {
        let section = section_tilted(&o.mesh, &o.matrix(), p, Some(p), self.tilt);
        let chosen = section.nearest(p);
        SectionPick {
            object: o.id,
            section,
            chosen,
        }
    }

    /// The section contour that was clicked.
    pub fn outline(&self) -> Option<&Outline> {
        let s = self
            .section
            .as_ref()
            .filter(|_| self.mode == Mode::Section)?;
        Some(&s.section.contours.get(s.chosen?)?.outline)
    }

    /// Section mode: the wall (outer and inner contour) the chosen contour belongs to.
    pub fn section_wall(&self) -> Option<crate::section::Wall> {
        let s = self.section.as_ref()?;
        s.section.wall_of(s.chosen?)
    }

    /// Section mode: the diameter of every round contour of the cut, outer ones (true) and
    /// inner ones (holes, cavities), largest first.
    pub fn section_diameters(&self) -> Vec<(bool, f32)> {
        let Some(s) = self.section.as_ref().filter(|_| self.mode == Mode::Section) else {
            return vec![];
        };
        s.section
            .contours
            .iter()
            .filter_map(|c| Some((c.depth % 2 == 0, c.outline.round()?.diameter())))
            .collect()
    }

    /// Points the mode takes before it starts over.
    pub fn capacity(&self) -> usize {
        match self.mode {
            Mode::Distance => 2,
            Mode::Surface => 1,
            Mode::Circle => 3,
            // Section mode places no points: the result is what the click found.
            Mode::Section => 0,
        }
    }

    /// Whether the next click places something, so the arrow marker follows the pointer:
    /// every mode but a finished one; a section is picked by each click.
    pub fn picking(&self) -> bool {
        self.mode == Mode::Section || self.points.len() < self.capacity()
    }

    /// The distance between the placed points, or the last one and the cursor.
    pub fn distance(&self) -> f32 {
        let at = |s: &Spot| s.point;
        match (&self.points[..], &self.hover) {
            ([a, b], _) => at(a).distance(at(b)),
            ([a], Some(h)) => at(a).distance(at(h)),
            _ => 0.0,
        }
    }

    /// The circle or sphere being measured: the surface's, or the one through the
    /// placed points and the cursor.
    pub fn current_round(&self) -> Option<Round> {
        match self.mode {
            Mode::Distance => None,
            Mode::Surface => self.round,
            Mode::Section => self.outline().and_then(Outline::round),
            Mode::Circle => {
                let live = if self.points.len() < 3 {
                    self.hover
                } else {
                    None
                };
                let all: Vec<Vec3> = self
                    .points
                    .iter()
                    .chain(live.as_ref())
                    .map(|s| s.point)
                    .collect();
                match all[..] {
                    [a, b, c] => circle_through(a, b, c),
                    _ => None,
                }
            }
        }
    }

    fn face_info(&mut self, scene: &Scene, id: u64, tri: usize) -> Option<&FaceInfo> {
        let o = scene.get(id)?;
        let model = o.matrix();
        let stale = !matches!(&self.face, Some((i, t, m, mat, _))
            if *i == id && *t == tri && Arc::ptr_eq(m, &o.mesh) && *mat == model);
        if stale {
            let info = FaceInfo::new(&o.mesh, &model, tri);
            self.face = Some((id, tri, o.mesh.clone(), model, info));
        }
        self.face.as_ref().and_then(|f| f.4.as_ref())
    }

    /// The point a click would take. `hit` is the picked object, triangle and surface
    /// point, `floor` the point on the floor when nothing is hit, `tolerance` how far (mm)
    /// a feature may be to catch the point. With `snap` the point goes to the nearest
    /// feature of its face, else stays on the face, and the second point of a distance
    /// is lined up with the first (see [`align`]).
    pub fn spot(
        &mut self,
        scene: &Scene,
        hit: Option<(u64, usize, Vec3)>,
        floor: Option<Vec3>,
        tolerance: f32,
        snap: bool,
    ) -> Option<Spot> {
        let mut spot = match hit {
            Some((id, tri, p)) => match snap.then(|| self.face_info(scene, id, tri)).flatten() {
                Some(face) => {
                    let s = snap_point(face, p, tolerance);
                    Spot {
                        point: s.point,
                        // A facet of a curved surface has no normal to line up along.
                        normal: (!s.curved).then_some(s.normal),
                        aligned: None,
                    }
                }
                None => Spot::free(p),
            },
            None if snap => Spot {
                point: (floor? * 10.0).round() / 10.0,
                normal: Some(Vec3::Z),
                aligned: None,
            },
            None => Spot::free(floor?),
        };
        if let (true, Mode::Distance, [first]) = (snap, self.mode, &self.points[..]) {
            if let Some((p, how)) = align(
                first.point,
                first.normal,
                spot.point,
                spot.normal,
                tolerance,
            ) {
                spot.point = p;
                spot.aligned = Some(how);
            }
        }
        Some(spot)
    }

    /// A click: place `spot`, starting over when the mode's points are all placed; in
    /// Surface mode, fit the curved face `face` (object, triangle) that was clicked.
    pub fn click(&mut self, scene: &Scene, spot: Spot, face: Option<(u64, usize)>) {
        if self.mode == Mode::Section {
            self.section = face.and_then(|(id, _)| Some(self.cut(scene.get(id)?, spot.point)));
            return;
        }
        if !self.picking() {
            self.points.clear();
        }
        self.points.push(spot);
        if self.mode == Mode::Surface {
            self.round = face.and_then(|(id, tri)| {
                let o = scene.get(id)?;
                surface_round(&o.mesh, &o.matrix(), tri)
            });
        }
    }
}

#[cfg(test)]
mod sweep;

#[cfg(test)]
mod cancel_tests {
    use super::*;

    #[test]
    fn cancel_drops_points_one_by_one() {
        let mut m = Measure::new(Mode::Distance);
        m.points.push(Spot::free(Vec3::ZERO));
        m.points.push(Spot::free(Vec3::X));
        m.cancel();
        assert_eq!(m.points.len(), 1);
        m.cancel();
        m.cancel();
        assert!(m.points.is_empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::Primitive;
    use crate::scene::{Object, DEFAULT_COLOR};

    fn near(a: f32, b: f32, tolerance: f32) -> bool {
        (a - b).abs() <= tolerance
    }

    #[test]
    fn three_points_give_their_circle() {
        let c = circle_through(
            Vec3::new(5., 0., 0.),
            Vec3::new(0., 5., 0.),
            Vec3::new(-5., 0., 0.),
        )
        .unwrap();
        assert!(near(c.radius, 5.0, 1e-4) && near(c.diameter(), 10.0, 2e-4));
        assert!(c.center.length() < 1e-4);
        assert!(near(c.axis.unwrap().z.abs(), 1.0, 1e-5));
        assert!(circle_through(Vec3::ZERO, Vec3::X, Vec3::X * 2.0).is_none());
    }

    #[test]
    fn a_sphere_is_fitted_from_its_vertices() {
        let points: Vec<Vec3> = (0..40)
            .map(|i| {
                let (a, b) = (i as f32 * 0.7, i as f32 * 1.9);
                Vec3::new(1., 2., 3.)
                    + 4.0 * Vec3::new(a.cos() * b.sin(), a.sin() * b.sin(), b.cos())
            })
            .collect();
        let (s, e) = fit_sphere(&points).unwrap();
        assert!(near(s.radius, 4.0, 1e-3) && e < 1e-4);
        assert!(s.center.distance(Vec3::new(1., 2., 3.)) < 1e-3);
    }

    #[test]
    fn flat_and_square_shapes_are_not_round() {
        let flat = vec![Vec3::Z; 5];
        assert!(fit_surface(&[Vec3::ZERO, Vec3::X, Vec3::Y], &flat).is_none());
        // A cube's corners lie on a sphere but its normals do not say "curved".
        let cube = Primitive::Cube.build();
        assert!(fit_object(&cube, [Vec3::X, Vec3::Y, Vec3::Z])
            .is_none_or(|r| r.kind == RoundKind::Sphere));
    }

    #[test]
    fn a_cylinder_object_gives_its_radius_and_a_curved_face_too() {
        let mesh = Primitive::Cylinder.build();
        let size = mesh.aabb().size();
        let r = fit_object(&mesh, [Vec3::X, Vec3::Y, Vec3::Z]).unwrap();
        assert_eq!(r.kind, RoundKind::Cylinder);
        assert!(near(r.radius, size.x / 2.0, size.x * 0.01));
        assert!(r.axis.unwrap().z.abs() > 0.99);
        // The first side triangle: find one whose normal is horizontal.
        let tri = (0..mesh.triangles.len())
            .find(|&t| {
                let [a, b, c] = mesh.triangles[t].map(|i| mesh.positions[i as usize]);
                Mesh::face_normal(a, b, c).z.abs() < 0.01
            })
            .unwrap();
        let face = surface_round(&mesh, &Mat4::IDENTITY, tri).unwrap();
        assert!(near(face.radius, r.radius, r.radius * 0.01));
    }

    #[test]
    fn a_sphere_object_and_a_cone_base_are_found() {
        let sphere = Primitive::Sphere.build();
        let s = fit_object(&sphere, [Vec3::X, Vec3::Y, Vec3::Z]).unwrap();
        assert_eq!(s.kind, RoundKind::Sphere);
        assert!(near(
            s.diameter(),
            sphere.aabb().size().x,
            sphere.aabb().size().x * 0.02
        ));
        // A torus gives its outer equator, whatever its rows.
        for torus in [
            Primitive::Torus.build(),
            crate::primitives::torus(15.0, 5.0, 24, 10),
        ] {
            let t = fit_object(&torus, [Vec3::X, Vec3::Y, Vec3::Z]).expect("torus");
            assert_eq!(t.kind, RoundKind::Circle);
            assert!(near(t.diameter(), torus.aabb().size().x, 0.05), "{t:?}");
        }
        let cone = Primitive::Cone.build();
        let c = fit_object(&cone, [Vec3::X, Vec3::Y, Vec3::Z]).unwrap();
        assert_eq!(c.kind, RoundKind::Circle);
        assert!(near(
            c.diameter(),
            cone.aabb().size().x,
            cone.aabb().size().x * 0.02
        ));
    }

    #[test]
    fn the_spot_snaps_aligns_and_the_tool_collects_points() {
        let mut scene = Scene::default();
        let cube = Primitive::Cube.build();
        let id = scene.add(Object::new("cube", cube, DEFAULT_COLOR));
        let o = scene.get(id).unwrap();
        let top = o.world_aabb().max.z;
        let model = o.matrix();
        // A triangle on the top face.
        let tri = (0..o.mesh.triangles.len())
            .find(|&t| {
                let v = o.mesh.triangles[t]
                    .map(|i| model.transform_point3(o.mesh.positions[i as usize]));
                v.iter().all(|p| near(p.z, top, 1e-3))
            })
            .unwrap();
        let corner = o.world_aabb().max;
        let mut m = Measure::new(Mode::Distance);
        let near_corner = corner - Vec3::new(0.2, 0.2, 0.0);
        let s = m
            .spot(&scene, Some((id, tri, near_corner)), None, 1.0, true)
            .unwrap();
        assert!(s.point.distance(corner) < 1e-3);
        // Snapping off keeps the exact point.
        let free = m
            .spot(&scene, Some((id, tri, near_corner)), None, 1.0, false)
            .unwrap();
        assert_eq!(free.point, near_corner);
        // The floor under the cube is a plane, so a top-to-floor distance is perpendicular when
        // the cursor is near the line down from the point; further away it keeps its own point.
        m.click(&scene, s, None);
        let floor = m
            .spot(
                &scene,
                None,
                Some(Vec3::new(corner.x + 0.3, corner.y + 0.4, 0.0)),
                1.0,
                true,
            )
            .unwrap();
        assert_eq!(floor.aligned, Some(Alignment::Perpendicular));
        assert!(near(floor.point.x, corner.x, 1e-4));
        let away = Some(Vec3::new(corner.x + 3.0, corner.y + 4.0, 0.0));
        assert_eq!(m.spot(&scene, None, away, 1.0, true).unwrap().aligned, None);
        m.hover = Some(floor);
        assert!(near(m.distance(), top, 1e-3));
        m.click(&scene, floor, None);
        assert_eq!(m.points.len(), 2);
        m.click(&scene, floor, None);
        assert_eq!(m.points.len(), 1);
    }

    #[test]
    fn circle_mode_uses_the_cursor_as_the_last_point() {
        let mut m = Measure::new(Mode::Circle);
        let scene = Scene::default();
        m.click(&scene, Spot::free(Vec3::new(5., 0., 0.)), None);
        m.click(&scene, Spot::free(Vec3::new(0., 5., 0.)), None);
        assert!(m.current_round().is_none());
        m.hover = Some(Spot::free(Vec3::new(-5., 0., 0.)));
        assert!(near(m.current_round().unwrap().radius, 5.0, 1e-4));
        m.click(&scene, Spot::free(Vec3::new(-5., 0., 0.)), None);
        m.hover = None;
        assert!(near(m.current_round().unwrap().radius, 5.0, 1e-4));
    }

    #[test]
    fn snap_to_scene_finds_the_nearest_feature() {
        let mut scene = Scene::default();
        let id = scene.add(Object::new("cube", Primitive::Cube.build(), DEFAULT_COLOR));
        let max = scene.get(id).unwrap().world_aabb().max;
        let (found, s) = snap_to_scene(&scene, max + Vec3::new(0.3, 0.0, 0.2), 1.0).unwrap();
        assert_eq!((found, s.kind), (id, crate::snapping::SnapKind::Vertex));
        assert!(s.point.distance(max) < 1e-3);
        assert!(snap_to_scene(&scene, max + Vec3::splat(50.0), 1.0).is_none());
        // A tolerance of 0 snaps nothing, not even a point on the surface.
        assert!(snap_to_scene(&scene, max, 0.0).is_none());
    }

    #[test]
    fn a_preset_spec_sets_mode_points_and_the_cursor_point() {
        let scene = Scene::default();
        let m = Measure::preset("circle:1,2,3;4,5,6;~7,8,9", &scene);
        assert_eq!((m.mode, m.points.len()), (Mode::Circle, 2));
        assert_eq!(m.hover.unwrap().point, Vec3::new(7., 8., 9.));
        let d = Measure::preset("0,0,0;3,4,0", &scene);
        assert_eq!((d.mode, d.distance()), (Mode::Distance, 5.0));
    }

    #[test]
    fn a_section_click_picks_the_nearest_contour_and_lists_them_all() {
        let mut scene = Scene::default();
        let id = scene.add(Object::new(
            "pot",
            crate::primitives::rounded_pot(),
            DEFAULT_COLOR,
        ));
        let mut s = Measure::new(Mode::Section);
        assert_eq!(s.capacity(), 0);
        // A click on the outer wall cuts at its height and picks the outer contour; the
        // flared wall is 2.4 mm thick there.
        s.click(
            &scene,
            Spot::free(Vec3::new(40.0, 0.0, 85.0)),
            Some((id, 0)),
        );
        let r = s.current_round().unwrap();
        assert!(near(r.center.z, 85.0, 1e-3) && near(r.diameter(), 78.81, 0.1));
        assert!(near(s.section_wall().unwrap().thickness, 2.4, 0.1));
        let all = s.section_diameters();
        assert_eq!(all.len(), 2);
        assert!(all[0].0 && !all[1].0 && all[0].1 > all[1].1);
        // A click nearer the inner wall picks the inner contour.
        s.click(
            &scene,
            Spot::free(Vec3::new(r.radius - 1.9, 0.0, 85.0)),
            Some((id, 0)),
        );
        assert!(s.current_round().unwrap().diameter() < all[0].1 - 4.0);
        let p = Measure::preset("section:40,0,85", &scene);
        assert_eq!(p.mode, Mode::Section);
        assert!(near(p.current_round().unwrap().diameter(), all[0].1, 1e-3));
        // Clicking on nothing clears it, and so does changing the mode.
        s.click(&scene, Spot::free(Vec3::ZERO), None);
        assert!(s.section.is_none() && s.current_round().is_none());
        s.click(
            &scene,
            Spot::free(Vec3::new(40.0, 0.0, 85.0)),
            Some((id, 0)),
        );
        s.set_mode(Mode::Distance);
        assert!(s.section.is_none());
    }

    #[test]
    fn a_click_on_a_cylinders_top_rim_gives_its_diameter() {
        let mut scene = Scene::default();
        let id = scene.add(Object::new(
            "can",
            Primitive::Cylinder.build(),
            DEFAULT_COLOR,
        ));
        let b = scene.get(id).unwrap().world_aabb();
        let r = b.size().x / 2.0;
        for z in [b.min.z, b.max.z] {
            let mut m = Measure::new(Mode::Section);
            let rim = Vec3::new(b.center().x + r, b.center().y, z);
            m.click(&scene, Spot::free(rim), Some((id, 0)));
            assert!(near(m.current_round().unwrap().diameter(), 2.0 * r, 0.05));
            assert!(m.section.as_ref().unwrap().section.shift != 0.0);
        }
    }

    #[test]
    fn a_click_on_a_tilted_cylinder_tilts_the_section_unless_tilt_is_off() {
        let mut scene = Scene::default();
        let mut can = Object::new("can", Primitive::Cylinder.build(), DEFAULT_COLOR);
        can.rotation.x = 20.0;
        let id = scene.add(can);
        let o = scene.get(id).unwrap();
        let local = o.mesh.aabb();
        let r = local.size().x / 2.0;
        let wall = o.matrix().transform_point3(local.center() + Vec3::X * r);
        let mut m = Measure::new(Mode::Section);
        assert_eq!(m.tilt, Tilt::Auto);
        m.click(&scene, Spot::free(wall), Some((id, 0)));
        let tilted = m.section.as_ref().unwrap();
        assert!(tilted.section.auto_tilted && near(tilted.section.tilt_degrees(), 20.0, 0.3));
        assert!(near(m.current_round().unwrap().diameter(), 2.0 * r, 0.05));
        // The setting survives a change of mode and turns the search off.
        m.tilt = Tilt::None;
        m.set_mode(Mode::Section);
        m.click(&scene, Spot::free(wall), Some((id, 0)));
        assert!(m.current_round().is_none() && m.section.is_some());
    }

    #[test]
    fn a_curved_face_click_fits_a_sphere() {
        let mut scene = Scene::default();
        let id = scene.add(Object::new(
            "ball",
            Primitive::Sphere.build(),
            DEFAULT_COLOR,
        ));
        let o = scene.get(id).unwrap();
        let tri = o.mesh.triangles.len() / 2;
        let mut m = Measure::new(Mode::Surface);
        m.click(&scene, Spot::free(Vec3::ZERO), Some((id, tri)));
        let r = m.current_round().unwrap();
        assert_eq!(r.kind, RoundKind::Sphere);
        let size = o.world_aabb().size().x;
        assert!(near(r.diameter(), size, size * 0.02));
        // A flat face is not round: the result is cleared.
        let cube = scene.add(Object::new("box", Primitive::Cube.build(), DEFAULT_COLOR));
        m.click(&scene, Spot::free(Vec3::ZERO), Some((cube, 0)));
        assert!(m.current_round().is_none());
    }

    #[test]
    fn degenerate_input_gives_none_not_a_panic() {
        let cyl = Primitive::Cylinder.build();
        // An object scaled flat has axes with no length.
        assert!(fit_object(&cyl, [Vec3::ZERO, Vec3::Y, Vec3::Z]).is_some());
        assert!(fit_object(&Mesh::default(), [Vec3::X, Vec3::Y, Vec3::Z]).is_none());
        assert!(fit_circle_around(&[Vec3::ZERO, Vec3::X, Vec3::Y], Vec3::ZERO).is_none());
        let (u, v) = plane_basis(Vec3::Z);
        assert!(u.dot(v).abs() < 1e-6 && near(u.cross(v).z, 1.0, 1e-6));
    }

    #[test]
    fn a_surface_preset_takes_the_nearest_object_not_the_first() {
        let mut scene = Scene::default();
        let mut ball = Object::new("ball", Primitive::Sphere.build(), DEFAULT_COLOR);
        ball.translation.x = 500.0;
        scene.add(ball);
        let can = scene.add(Object::new(
            "can",
            Primitive::Cylinder.build(),
            DEFAULT_COLOR,
        ));
        let b = scene.get(can).unwrap().world_aabb();
        let wall = Vec3::new(b.max.x, b.center().y, b.center().z);
        let m = Measure::preset(&format!("surface:{},{},{}", wall.x, wall.y, wall.z), &scene);
        assert_eq!(m.round.map(|r| r.kind), Some(RoundKind::Cylinder));
        assert_eq!(nearest_surface(&scene, wall).unwrap().0.id, can);
        assert!(nearest_surface(&Scene::default(), wall).is_none());
    }

    #[test]
    fn forgetting_results_keeps_the_points() {
        let mut m = Measure::new(Mode::Circle);
        m.click(&Scene::default(), Spot::free(Vec3::X), None);
        m.round = circle_through(Vec3::X, Vec3::Y, -Vec3::X);
        m.forget_results();
        assert!(m.round.is_none() && m.points.len() == 1);
    }
}
