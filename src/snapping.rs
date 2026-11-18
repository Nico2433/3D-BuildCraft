//! Snapping, independent of the UI and camera: bounding-box alignment for moving objects,
//! and point snapping (vertices, edge middles and edges, face centres and planes) for the
//! Measure tool and the MCP `measure` tool.
use crate::mesh::{Aabb, Mesh};
use glam::{Mat4, Vec3};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug)]
pub struct Snap {
    pub offset: f32,
    pub value: f32,
    pub target: Aabb,
}

/// Find the nearest pair of minimum, centre or maximum features on one axis.
pub fn snap_axis(moving: &Aabb, targets: &[Aabb], axis: usize, threshold: f32) -> Option<Snap> {
    let features = |b: &Aabb| [b.min[axis], b.center()[axis], b.max[axis]];
    let mut best: Option<Snap> = None;
    for target in targets.iter().filter(|b| b.is_valid()) {
        for source in features(moving) {
            for value in features(target) {
                let offset = value - source;
                if offset.abs() <= threshold && best.is_none_or(|b| offset.abs() < b.offset.abs()) {
                    best = Some(Snap {
                        offset,
                        value,
                        target: *target,
                    });
                }
            }
        }
    }
    best
}

/// What a measured point snapped to, nearest kind first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapKind {
    Vertex,
    Midpoint,
    Edge,
    Face,
    /// Nothing nearer than the face itself: the point stays on its plane.
    Plane,
}

impl SnapKind {
    /// English name, shown in the UI through `tr` and returned by the MCP tool.
    pub fn name(self) -> &'static str {
        match self {
            SnapKind::Vertex => "Vertex",
            SnapKind::Midpoint => "Midpoint",
            SnapKind::Edge => "Edge",
            SnapKind::Face => "Face centre",
            SnapKind::Plane => "Plane",
        }
    }
}

/// A point on a surface after snapping, with the surface's normal there.
#[derive(Clone, Copy, Debug)]
pub struct PointSnap {
    pub point: Vec3,
    pub normal: Vec3,
    pub kind: SnapKind,
}

/// The flat face (triangles within 1 degree of each other) around a triangle, in world
/// space: its centre, normal and outline edges.
#[derive(Clone, Debug)]
pub struct FaceInfo {
    pub centre: Vec3,
    pub normal: Vec3,
    pub edges: Vec<[Vec3; 2]>,
}

impl FaceInfo {
    /// `None` for a missing or degenerate triangle.
    pub fn new(mesh: &Mesh, model: &Mat4, tri: usize) -> Option<Self> {
        let world = |t: usize| {
            mesh.triangles[t].map(|v| model.transform_point3(mesh.positions[v as usize]))
        };
        let region = mesh.face_region(tri, 1.0);
        let [a, b, c] = world(*region.first()?);
        let normal = Mesh::face_normal(a, b, c);
        if normal == Vec3::ZERO {
            return None;
        }
        let key = |p: Vec3| (p * 1000.0).round().as_ivec3().to_array();
        type Key = [i32; 3];
        let mut edges: HashMap<(Key, Key), (u32, [Vec3; 2])> = HashMap::new();
        let (mut sum, mut area) = (Vec3::ZERO, 0.0);
        for t in region {
            let v = world(t);
            let twice = (v[1] - v[0]).cross(v[2] - v[0]).length();
            sum += (v[0] + v[1] + v[2]) / 3.0 * twice;
            area += twice;
            for k in 0..3 {
                let (p, q) = (v[k], v[(k + 1) % 3]);
                let (kp, kq) = (key(p), key(q));
                let e = edges.entry((kp.min(kq), kp.max(kq))).or_insert((0, [p, q]));
                e.0 += 1;
            }
        }
        (area > 0.0).then(|| FaceInfo {
            centre: sum / area,
            normal,
            edges: edges
                .into_values()
                .filter(|e| e.0 == 1)
                .map(|e| e.1)
                .collect(),
        })
    }
}

fn closest_on_segment(p: Vec3, [a, b]: [Vec3; 2]) -> Vec3 {
    let ab = b - a;
    let t = if ab.length_squared() > 0.0 {
        ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0)
    } else {
        0.0
    };
    a + ab * t
}

/// Snap `hit`, a point on the face, to its nearest vertex, edge middle, edge or the face
/// centre within `threshold` (in that order of preference), else keep it on the plane.
pub fn snap_point(face: &FaceInfo, hit: Vec3, threshold: f32) -> PointSnap {
    let nearest = |candidates: Vec<Vec3>| {
        candidates
            .into_iter()
            .map(|p| (p.distance(hit), p))
            .filter(|(d, _)| *d <= threshold)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, p)| p)
    };
    let found = nearest(face.edges.iter().flatten().copied().collect())
        .map(|p| (p, SnapKind::Vertex))
        .or_else(|| {
            nearest(face.edges.iter().map(|[a, b]| (*a + *b) / 2.0).collect())
                .map(|p| (p, SnapKind::Midpoint))
        })
        .or_else(|| {
            nearest(
                face.edges
                    .iter()
                    .map(|e| closest_on_segment(hit, *e))
                    .collect(),
            )
            .map(|p| (p, SnapKind::Edge))
        })
        .or_else(|| nearest(vec![face.centre]).map(|p| (p, SnapKind::Face)));
    let (point, kind) = found.unwrap_or((hit, SnapKind::Plane));
    PointSnap {
        point,
        normal: face.normal,
        kind,
    }
}

/// How a second measured point was moved to line up with the first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Alignment {
    /// Both points lie on parallel faces: the distance is the one between the planes.
    Perpendicular,
    /// The line is within 3 degrees of this world axis (0 X, 1 Y, 2 Z): made exact.
    Axis(usize),
}

/// Line the second point up with the first so the measurement is exact: onto the normal
/// through the first point when both lie on parallel faces, else onto a world axis the
/// line is nearly along. `None` when neither applies.
pub fn align(
    first: Vec3,
    first_normal: Option<Vec3>,
    second: Vec3,
    second_normal: Option<Vec3>,
) -> Option<(Vec3, Alignment)> {
    let d = second - first;
    if let Some((n, m)) = first_normal.zip(second_normal) {
        let along = d.dot(n);
        if n.dot(m).abs() >= 0.9995 && along.abs() > 1e-6 {
            return Some((first + n * along, Alignment::Perpendicular));
        }
    }
    let len = d.length();
    (0..3)
        .find(|&i| len > 1e-6 && d[i].abs() / len >= 0.9986)
        .map(|i| {
            let mut p = first;
            p[i] = second[i];
            (p, Alignment::Axis(i))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;
    fn bounds(min: f32, max: f32) -> Aabb {
        Aabb {
            min: Vec3::splat(min),
            max: Vec3::splat(max),
        }
    }
    #[test]
    fn centre_to_centre() {
        let hit = snap_axis(&bounds(0., 10.), &[bounds(3., 8.)], 0, 0.6).unwrap();
        assert_eq!(hit.offset, 0.5);
        assert_eq!(hit.value, 5.5);
    }
    #[test]
    fn edge_to_edge() {
        assert_eq!(
            snap_axis(&bounds(0., 10.), &[bounds(0.4, 30.)], 0, 0.5)
                .unwrap()
                .offset,
            0.4
        );
    }
    #[test]
    fn edge_to_opposite_edge() {
        assert!(
            (snap_axis(&bounds(0., 10.), &[bounds(10.3, 20.)], 0, 0.5)
                .unwrap()
                .offset
                - 0.3)
                .abs()
                < 1e-5
        );
    }
    #[test]
    fn beyond_threshold() {
        assert!(snap_axis(&bounds(0., 10.), &[bounds(12., 22.)], 0, 0.5).is_none());
    }

    fn square() -> FaceInfo {
        // A 10 x 10 square in the XY plane made of two triangles.
        let mesh = Mesh::new(
            vec![
                Vec3::ZERO,
                Vec3::new(10., 0., 0.),
                Vec3::new(10., 10., 0.),
                Vec3::new(0., 10., 0.),
            ],
            vec![[0, 1, 2], [0, 2, 3]],
        );
        FaceInfo::new(&mesh, &Mat4::IDENTITY, 0).unwrap()
    }
    #[test]
    fn face_info_skips_the_inner_diagonal() {
        let f = square();
        assert_eq!(f.edges.len(), 4);
        assert!(f.centre.distance(Vec3::new(5., 5., 0.)) < 1e-4);
        assert!((f.normal - Vec3::Z).length() < 1e-5);
    }
    #[test]
    fn snaps_in_order_of_preference() {
        let f = square();
        let kind = |p: Vec3| snap_point(&f, p, 0.5).kind;
        assert_eq!(kind(Vec3::new(0.2, 0.1, 0.)), SnapKind::Vertex);
        assert_eq!(kind(Vec3::new(5.2, 0.1, 0.)), SnapKind::Midpoint);
        assert_eq!(kind(Vec3::new(2.0, 0.3, 0.)), SnapKind::Edge);
        assert_eq!(kind(Vec3::new(5.1, 4.9, 0.)), SnapKind::Face);
        let p = snap_point(&f, Vec3::new(3., 4., 0.), 0.5);
        assert_eq!((p.kind, p.point), (SnapKind::Plane, Vec3::new(3., 4., 0.)));
        assert_eq!(
            snap_point(&f, Vec3::new(0.2, 0.1, 0.), 0.5).point,
            Vec3::ZERO
        );
    }
    #[test]
    fn parallel_faces_give_the_perpendicular_distance() {
        let n = Some(Vec3::Z);
        let (p, how) = align(Vec3::new(1., 1., 0.), n, Vec3::new(4., 6., 7.), n).unwrap();
        assert_eq!((p, how), (Vec3::new(1., 1., 7.), Alignment::Perpendicular));
    }
    #[test]
    fn nearly_axis_aligned_lines_become_exact() {
        let (p, how) = align(Vec3::ZERO, None, Vec3::new(10., 0.1, -0.1), None).unwrap();
        assert_eq!((p, how), (Vec3::new(10., 0., 0.), Alignment::Axis(0)));
        assert!(align(Vec3::ZERO, None, Vec3::new(10., 5., 0.), None).is_none());
    }
}
