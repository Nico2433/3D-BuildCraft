//! The Measure tool's model, independent of the UI: radius and diameter of round geometry
//! (a circle through three points, a curved face, a whole object) and the picked points with
//! their snapping. The Measure tool in `app` and the MCP `measure` tool share it.
use crate::edge_loops::{EdgeLoop, FeatureEdges};
use crate::mesh::Mesh;
use crate::scene::{Scene, WorkCache};
use crate::snapping::{align, snap_point, Alignment, FaceInfo, PointSnap, SnapKind};
use glam::{Mat4, Vec3};
use std::sync::Arc;

/// A fit counts as round when its RMS error is under this fraction of the radius.
const FIT_TOLERANCE: f64 = 0.005;

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
    let (u, v) = (
        axis.any_orthonormal_vector(),
        axis.cross(axis.any_orthonormal_vector()),
    );
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
    if axis.length() < 0.05 {
        return None;
    }
    let axis = axis.normalize();
    let fit = if normals.iter().all(|n| n.dot(axis).abs() < 0.05) {
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
    fit.filter(|(_, e)| *e < FIT_TOLERANCE * 2.0)
        .map(|(r, _)| r)
}

/// The curved face around triangle `tri` of `mesh` placed by `model`, fitted (see
/// [`fit_surface`]). Neighbouring triangles up to 30 degrees apart belong to it.
pub fn surface_round(mesh: &Mesh, model: &Mat4, tri: usize) -> Option<Round> {
    let region = mesh.face_region(tri, 30.0);
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
    let axis = round.axis.unwrap_or(Vec3::Z);
    let u = axis.any_orthonormal_vector();
    let v = axis.cross(u);
    let sectors: std::collections::HashSet<u32> = points
        .iter()
        .map(|p| {
            let d = *p - round.center;
            let angle = d.dot(v).atan2(d.dot(u));
            ((angle + std::f32::consts::PI) / std::f32::consts::TAU * 12.0) as u32 % 12
        })
        .collect();
    points.len() >= 8 && sectors.len() >= 10
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
    let axes = axes.map(Vec3::normalize);
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
                > 0.98
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
            .filter(|p| radial(p) >= widest * 0.98)
            .copied()
            .collect();
        full(&ring, axis)
    });
    best(rings.collect())
}

/// Snap `p` to the nearest face feature (vertex, edge middle, edge, face centre) of any
/// object within `tolerance` mm of it; `None` when no surface is that near.
pub fn snap_to_scene(scene: &Scene, p: Vec3, tolerance: f32) -> Option<PointSnap> {
    scene
        .objects
        .iter()
        .filter_map(|o| {
            let model = o.matrix();
            let (tri, q) = o.mesh.closest_triangle(&model, p)?;
            let face = FaceInfo::new(&o.mesh, &model, tri)?;
            (q.distance(p) <= tolerance).then(|| (q.distance(p), snap_point(&face, q, tolerance)))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, s)| s)
}

/// The edge loop nearest to `p` over all objects (object id and loop) within `tolerance`
/// mm, waiting for the feature edges when they are not known yet.
pub fn nearest_edge_loop(scene: &Scene, p: Vec3, tolerance: f32) -> Option<(u64, EdgeLoop)> {
    scene
        .objects
        .iter()
        .filter_map(|o| {
            let model = o.matrix();
            let feats = crate::edge_loops::feature_edges(&o.mesh);
            let (chain, d) = feats.nearest_chain(&model, p, tolerance)?;
            Some((d, o.id, feats.edge_loop(chain, &model)))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, id, l)| (id, l))
}

/// What the Measure tool measures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// The distance between two points.
    Distance,
    /// The radius of the curved face under a click.
    Surface,
    /// The radius of the circle through three points.
    Circle,
    /// The diameter, radius and centre of the circular edge under a click.
    Edge,
}

/// A picked point: where, the surface normal there, what it snapped to and how it was
/// lined up with the first point.
#[derive(Clone, Copy, Debug)]
pub struct Spot {
    pub point: Vec3,
    pub normal: Option<Vec3>,
    pub kind: Option<SnapKind>,
    pub aligned: Option<Alignment>,
}

impl Spot {
    pub fn free(point: Vec3) -> Self {
        Spot {
            point,
            normal: None,
            kind: None,
            aligned: None,
        }
    }
}

/// A face's snapping info with what it was built from: object id, triangle, mesh, placement.
type CachedFace = (u64, usize, Arc<Mesh>, Mat4, Option<FaceInfo>);

/// The edge loop under the cursor and what it was found from: object id, chain, mesh and
/// placement, so a moving cursor on the same edge does not fit it again.
type HoverKey = (u64, usize, Arc<Mesh>, Mat4);

/// The Measure tool's state.
pub struct Measure {
    pub mode: Mode,
    pub points: Vec<Spot>,
    /// The point under the cursor.
    pub hover: Option<Spot>,
    /// Surface mode's result.
    pub round: Option<Round>,
    /// Edge mode's result: the circular (or not) edge loop that was clicked.
    pub edge: Option<EdgeLoop>,
    /// The edge loop under the cursor, highlighted before a click.
    pub edge_hover: Option<EdgeLoop>,
    edge_key: Option<HoverKey>,
    /// Feature edges per mesh, found on a worker (a big mesh takes a moment).
    edges: WorkCache<Arc<FeatureEdges>>,
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
            edge: None,
            edge_hover: None,
            edge_key: None,
            edges: WorkCache::default(),
            face: None,
        }
    }

    /// A measurement with its points preset, for screenshots: `[mode:]point;point;…`, mode
    /// `distance`, `surface`, `circle` or `edge`; a point is `x,y,z` (mm) with `~` in front for
    /// the cursor point and `@vertex`, `@midpoint`, `@edge`, `@face` or `@plane` behind it
    /// for the snap marker. Surface mode fits the face nearest its point; edge mode takes the edge loop nearest
    /// to its point.
    pub fn preset(spec: &str, scene: &Scene) -> Self {
        let (mode, rest) = match spec.split_once(':') {
            Some(("surface", rest)) => (Mode::Surface, rest),
            Some(("circle", rest)) => (Mode::Circle, rest),
            Some(("edge", rest)) => (Mode::Edge, rest),
            Some((_, rest)) => (Mode::Distance, rest),
            None => (Mode::Distance, spec),
        };
        let mut m = Measure::new(mode);
        for entry in rest.split(';') {
            let (coords, kind) = entry.split_once('@').unwrap_or((entry, ""));
            let v: Vec<f32> = coords
                .trim_start_matches('~')
                .split(',')
                .filter_map(|n| n.trim().parse().ok())
                .collect();
            let Ok::<[f32; 3], _>([x, y, z]) = v.try_into() else {
                continue;
            };
            let kind = [
                SnapKind::Vertex,
                SnapKind::Midpoint,
                SnapKind::Edge,
                SnapKind::Face,
                SnapKind::Plane,
            ]
            .into_iter()
            .find(|k| k.name().to_lowercase().starts_with(kind) && !kind.is_empty());
            let spot = Spot {
                kind,
                ..Spot::free(Vec3::new(x, y, z))
            };
            if entry.starts_with('~') {
                m.hover = Some(spot);
            } else {
                m.points.push(spot);
            }
        }
        if let (Mode::Edge, Some(first)) = (mode, m.points.first()) {
            m.edge = nearest_edge_loop(scene, first.point, f32::INFINITY).map(|(_, l)| l);
        }
        if let (Mode::Surface, Some(first)) = (mode, m.points.first()) {
            m.round = scene.objects.iter().find_map(|o| {
                let model = o.matrix();
                let (tri, _) = o.mesh.closest_triangle(&model, first.point)?;
                surface_round(&o.mesh, &model, tri)
            });
        }
        m
    }

    pub fn set_mode(&mut self, mode: Mode) {
        let edges = std::mem::take(&mut self.edges);
        *self = Measure::new(mode);
        self.edges = edges;
    }

    /// Points the mode takes before it starts over.
    pub fn capacity(&self) -> usize {
        match self.mode {
            Mode::Distance => 2,
            Mode::Surface => 1,
            Mode::Circle => 3,
            // Edge mode places no points: the loop under the cursor is the result.
            Mode::Edge => 0,
        }
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
            Mode::Edge => self
                .edge
                .as_ref()
                .or(self.edge_hover.as_ref())
                .and_then(EdgeLoop::round),
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
                        normal: Some(s.normal),
                        kind: Some(s.kind),
                        aligned: None,
                    }
                }
                None => Spot::free(p),
            },
            None if snap => Spot {
                point: (floor? * 10.0).round() / 10.0,
                normal: Some(Vec3::Z),
                kind: Some(SnapKind::Plane),
                aligned: None,
            },
            None => Spot::free(floor?),
        };
        if let (true, Mode::Distance, [first]) = (snap, self.mode, &self.points[..]) {
            if let Some((p, how)) = align(first.point, first.normal, spot.point, spot.normal) {
                spot.point = p;
                spot.aligned = Some(how);
            }
        }
        Some(spot)
    }

    /// Finds the edge loop under the cursor for Edge mode: `hit` is the picked object and
    /// the surface point, `tolerance` how far (mm) an edge may be from it. The edges of a
    /// big mesh are found on a worker (`repaint` runs when they are ready); until then
    /// nothing is under the cursor.
    pub fn hover_edge(
        &mut self,
        scene: &Scene,
        hit: Option<(u64, Vec3)>,
        tolerance: f32,
        repaint: impl FnOnce() + Send + 'static,
    ) {
        let found = hit.and_then(|(id, p)| {
            let o = scene.get(id)?;
            let feats = self.edges.get(&o.mesh, FeatureEdges::compute, repaint)?;
            let model = o.matrix();
            let (chain, _) = feats.nearest_chain(&model, p, tolerance)?;
            Some((id, chain, o.mesh.clone(), model, feats))
        });
        let Some((id, chain, mesh, model, feats)) = found else {
            self.edge_hover = None;
            self.edge_key = None;
            return;
        };
        let same = matches!(&self.edge_key, Some((i, c, m, mat))
            if *i == id && *c == chain && Arc::ptr_eq(m, &mesh) && *mat == model);
        if !same {
            self.edge_hover = Some(feats.edge_loop(chain, &model));
            self.edge_key = Some((id, chain, mesh, model));
        }
    }

    /// A click: place `spot`, starting over when the mode's points are all placed; in
    /// Surface mode, fit the curved face `face` (object, triangle) that was clicked.
    pub fn click(&mut self, scene: &Scene, spot: Spot, face: Option<(u64, usize)>) {
        if self.mode == Mode::Edge {
            self.edge = self.edge_hover.clone();
            return;
        }
        if self.points.len() >= self.capacity() {
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
        assert_eq!(s.kind, Some(SnapKind::Vertex));
        assert!(s.point.distance(corner) < 1e-3);
        // Snapping off keeps the exact point.
        let free = m
            .spot(&scene, Some((id, tri, near_corner)), None, 1.0, false)
            .unwrap();
        assert_eq!((free.point, free.kind), (near_corner, None));
        // The floor under the cube is a plane, so a top-to-floor distance is perpendicular.
        m.click(&scene, s, None);
        let floor = m
            .spot(
                &scene,
                None,
                Some(Vec3::new(corner.x + 3.0, corner.y + 4.0, 0.0)),
                1.0,
                true,
            )
            .unwrap();
        assert_eq!(floor.aligned, Some(Alignment::Perpendicular));
        assert!(near(floor.point.x, corner.x, 1e-4));
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
        let s = snap_to_scene(&scene, max + Vec3::new(0.3, 0.0, 0.2), 1.0).unwrap();
        assert_eq!(s.kind, SnapKind::Vertex);
        assert!(s.point.distance(max) < 1e-3);
        assert!(snap_to_scene(&scene, max + Vec3::splat(50.0), 1.0).is_none());
    }

    #[test]
    fn a_preset_spec_sets_mode_points_and_the_cursor_marker() {
        let scene = Scene::default();
        let m = Measure::preset("circle:1,2,3;4,5,6;~7,8,9@vertex", &scene);
        assert_eq!((m.mode, m.points.len()), (Mode::Circle, 2));
        assert_eq!(m.hover.unwrap().kind, Some(SnapKind::Vertex));
        let d = Measure::preset("0,0,0;3,4,0", &scene);
        assert_eq!((d.mode, d.distance()), (Mode::Distance, 5.0));
    }

    #[test]
    fn edge_mode_selects_the_loop_under_the_cursor_and_gives_its_circle() {
        let mut scene = Scene::default();
        let id = scene.add(Object::new(
            "pot",
            Primitive::Cylinder.build(),
            DEFAULT_COLOR,
        ));
        let b = scene.get(id).unwrap().world_aabb();
        let r = b.size().x / 2.0;
        let rim = Vec3::new(b.center().x + r, b.center().y, b.max.z);
        let mut m = Measure::new(Mode::Edge);
        assert_eq!(m.capacity(), 0);
        m.hover_edge(&scene, Some((id, rim - Vec3::Z * 0.3)), 1.0, || {});
        // The worker may not be done yet; wait for it like the UI's next frame would.
        for _ in 0..200 {
            if m.edge_hover.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
            m.hover_edge(&scene, Some((id, rim - Vec3::Z * 0.3)), 1.0, || {});
        }
        let hover = m.current_round().unwrap();
        assert!(near(hover.diameter(), 2.0 * r, 0.02));
        assert!(m.edge.is_none());
        m.click(&scene, Spot::free(rim), None);
        assert!(m.edge.is_some());
        let c = m.current_round().unwrap();
        assert!(
            c.center
                .distance(Vec3::new(b.center().x, b.center().y, b.max.z))
                < 1e-2
        );
        // Far from any edge nothing is under the cursor; a click there clears the choice.
        m.hover_edge(
            &scene,
            Some((id, Vec3::new(b.center().x, b.center().y, b.max.z))),
            1.0,
            || {},
        );
        assert!(m.edge_hover.is_none());
        m.click(&scene, Spot::free(rim), None);
        assert!(m.edge.is_none() && m.current_round().is_none());
        // Changing the mode keeps nothing of the result.
        m.set_mode(Mode::Distance);
        assert!(m.edge.is_none());
        let p = Measure::preset("edge:0,0,0", &scene);
        assert_eq!(p.mode, Mode::Edge);
        assert!(p.current_round().is_some());
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
}
