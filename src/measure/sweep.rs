//! Sweeps a synthetic cursor across a model after a first point, as the viewport does on
//! hover, and checks the second point stays put and on the face under the cursor.

use super::*;
use crate::camera::OrbitCamera;
use crate::mesh::ray_triangle;
use crate::primitives::{self, Primitive};
use crate::scene::{Object, DEFAULT_COLOR};
use glam::Vec2;

const W: f32 = 1920.0;
const H: f32 = 1080.0;
/// The snap reach on screen, as in the viewport.
const REACH_PX: f32 = 14.0;

/// Object and quantised face normal.
type FaceKey = (u64, [i32; 3]);

/// One cursor position: what the ray hit and the spot the tool would take.
#[derive(Clone, Debug)]
pub struct Sample {
    pub x: usize,
    pub y: usize,
    /// Object and quantised face normal of the hit.
    pub face: Option<FaceKey>,
    pub hit: Vec3,
    pub spot: Option<Spot>,
    pub tolerance: f32,
}

fn pick(scene: &Scene, origin: Vec3, dir: Vec3) -> Option<(u64, usize, f32)> {
    let mut best: Option<(u64, usize, f32)> = None;
    for o in &scene.objects {
        let model = o.matrix();
        for (i, t) in o.mesh.triangles.iter().enumerate() {
            let [a, b, c] = t.map(|v| model.transform_point3(o.mesh.positions[v as usize]));
            if let Some(d) = ray_triangle(origin, dir, a, b, c) {
                if best.is_none_or(|(_, _, bd)| d < bd) {
                    best = Some((o.id, i, d));
                }
            }
        }
    }
    best
}

fn face_key(scene: &Scene, id: u64, tri: usize) -> FaceKey {
    let o = scene.get(id).unwrap();
    let model = o.matrix();
    let [a, b, c] =
        o.mesh.triangles[tri].map(|v| model.transform_point3(o.mesh.positions[v as usize]));
    let n = Mesh::face_normal(a, b, c);
    (id, (n * 50.0).round().as_ivec3().to_array())
}

/// The cursor at pixel `(x, y)`, like `App::measure_ray`.
pub fn cursor(m: &mut Measure, scene: &Scene, cam: &OrbitCamera, x: usize, y: usize) -> Sample {
    let ndc = |px: f32, py: f32| Vec2::new(px / W * 2.0 - 1.0, 1.0 - py / H * 2.0);
    let aspect = W / H;
    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
    let (origin, dir) = cam.ray(ndc(px, py), aspect);
    let beside = cam.ray(ndc(px + REACH_PX, py), aspect);
    let hit = pick(scene, origin, dir);
    let floor_t = -origin.z / dir.z;
    let floor =
        (hit.is_none() && floor_t.is_finite() && floor_t > 0.0).then(|| origin + dir * floor_t);
    let depth = hit.map_or(floor_t, |h| h.2);
    let tolerance = ((beside.0 + beside.1 * depth) - (origin + dir * depth)).length();
    let spot = m.spot(
        scene,
        hit.map(|(id, tri, t)| (id, tri, origin + dir * t)),
        floor,
        tolerance,
        true,
    );
    Sample {
        x,
        y,
        face: hit.map(|(id, tri, _)| face_key(scene, id, tri)),
        hit: hit.map_or(Vec3::ZERO, |h| origin + dir * h.2),
        spot,
        tolerance,
    }
}

/// Scanlines every `rows` pixels, left to right in one pixel steps; the first point is
/// placed through `first` (a world point on the model).
pub fn sweep(scene: &Scene, cam: &OrbitCamera, m: &mut Measure, rows: usize) -> Vec<Vec<Sample>> {
    (rows / 2..H as usize)
        .step_by(rows)
        .map(|y| {
            (0..W as usize)
                .map(|x| cursor(m, scene, cam, x, y))
                .collect()
        })
        .collect()
}

/// Places the first point on the vertex nearest `corner`, on a triangle there whose normal
/// is `face_normal` (any when `None`), so the point carries that face's normal.
pub fn first_on_vertex(scene: &Scene, corner: Vec3, face_normal: Option<Vec3>) -> Measure {
    let o = &scene.objects[0];
    let model = o.matrix();
    let tri = (0..o.mesh.triangles.len())
        .find(|&t| {
            let v =
                o.mesh.triangles[t].map(|i| model.transform_point3(o.mesh.positions[i as usize]));
            face_normal.is_none_or(|n| Mesh::face_normal(v[0], v[1], v[2]).dot(n) > 0.99)
                && v.iter().any(|p| p.distance(corner) < 1e-3)
        })
        .expect("a triangle of that face at the corner");
    let mut m = Measure::new(Mode::Distance);
    let spot = m
        .spot(scene, Some((o.id, tri, corner)), None, 1.0, true)
        .unwrap();
    assert_eq!(spot.kind, Some(SnapKind::Vertex));
    m.click(scene, spot, None);
    m
}

/// What went wrong between two neighbouring samples on the same face.
#[derive(Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Clone, Copy)]
pub enum Fault {
    /// The point moved far more than the cursor did.
    Jump,
    /// The point left the plane of the face under the cursor.
    OffFace,
    /// The point is further from the cursor than the snap reach: it went to a far corner.
    Far,
}

pub struct Report {
    /// Fault, face, row y and x of the later sample.
    pub faults: Vec<(Fault, FaceKey, usize, usize)>,
    pub samples: usize,
}

/// Distance of `p` to the plane of the hit face (through `hit`, normal from the key).
fn off_plane(face: [i32; 3], hit: Vec3, p: Vec3) -> f32 {
    let n = Vec3::new(face[0] as f32, face[1] as f32, face[2] as f32).normalize();
    (p - hit).dot(n).abs()
}

pub fn check(rows: &[Vec<Sample>]) -> Report {
    let mut faults = vec![];
    let mut samples = 0;
    for row in rows {
        for w in row.windows(2) {
            samples += 1;
            let (a, b) = (&w[0], &w[1]);
            let (Some(fa), Some(fb)) = (a.face, b.face) else {
                continue;
            };
            let (Some(sa), Some(sb)) = (a.spot, b.spot) else {
                continue;
            };
            if fa != fb {
                continue;
            }
            let step = a.hit.distance(b.hit);
            // A snap may pull the point by up to the reach: that is a deliberate jump.
            if sa.point.distance(sb.point) > step + 2.0 * a.tolerance.max(b.tolerance) {
                faults.push((Fault::Jump, fb, b.y, b.x));
            }
            if off_plane(fb.1, b.hit, sb.point) > b.tolerance {
                faults.push((Fault::OffFace, fb, b.y, b.x));
            }
            if sb.point.distance(b.hit) > 2.0 * b.tolerance + 1e-3 {
                faults.push((Fault::Far, fb, b.y, b.x));
            }
        }
    }
    Report { faults, samples }
}

/// Four views, from above and below and from either side, so every face is seen.
fn cameras(bounds: &crate::mesh::Aabb) -> Vec<OrbitCamera> {
    [(-60.0, 30.0), (120.0, 30.0), (-60.0, -30.0), (120.0, -30.0)]
        .map(|(yaw, pitch): (f32, f32)| {
            let mut c = OrbitCamera {
                yaw: yaw.to_radians(),
                pitch: pitch.to_radians(),
                ..OrbitCamera::default()
            };
            c.frame(bounds, W / H);
            c
        })
        .to_vec()
}

/// Sweeps every view with the first point at `corner` and returns the faults found, after
/// checking nothing flip-flops between two kinds from one pixel to the next on a face.
fn faults_for(scene: &Scene, corner: Vec3, normal: Option<Vec3>) -> Vec<String> {
    let bounds = scene.objects[0].world_aabb();
    let mut out = vec![];
    for (c, cam) in cameras(&bounds).iter().enumerate() {
        let mut m = first_on_vertex(scene, corner, normal);
        let rows = sweep(scene, cam, &mut m, 60);
        let r = check(&rows);
        assert!(r.samples > 10_000);
        for (fault, face, y, x) in r.faults.iter().take(3) {
            out.push(format!("view {c} {fault:?} on face {face:?} at ({x}, {y})"));
        }
        for row in &rows {
            for w in row.windows(3) {
                let kind = |s: &Sample| s.spot.and_then(|s| s.kind);
                let same = w.iter().all(|s| s.face == w[0].face && s.face.is_some());
                if same && kind(&w[0]) == kind(&w[2]) && kind(&w[0]) != kind(&w[1]) {
                    out.push(format!(
                        "view {c} kind flips {:?} {:?} {:?} at ({}, {})",
                        kind(&w[0]),
                        kind(&w[1]),
                        kind(&w[2]),
                        w[1].x,
                        w[1].y
                    ));
                }
                if same
                    && w[1]
                        .spot
                        .is_some_and(|s| s.kind == Some(SnapKind::Plane) && s.aligned.is_none())
                    && w[1].spot.unwrap().point.distance(w[1].hit) > 1e-3
                {
                    out.push(format!(
                        "view {c} plane point off the cursor at ({}, {})",
                        w[1].x, w[1].y
                    ));
                }
            }
        }
    }
    out
}

#[test]
fn a_cube_sweep_is_stable_from_every_vertex_face() {
    let mut scene = Scene::default();
    scene.add(Object::new("cube", Primitive::Cube.build(), DEFAULT_COLOR));
    let max = scene.objects[0].world_aabb().max;
    for normal in [Vec3::X, Vec3::Y, Vec3::Z] {
        let faults = faults_for(&scene, max, Some(normal));
        assert!(
            faults.is_empty(),
            "first point's face {normal:?}: {faults:#?}"
        );
    }
}

#[test]
fn a_cylinder_sweep_is_stable() {
    let mut scene = Scene::default();
    scene.add(Object::new(
        "cylinder",
        Primitive::Cylinder.build(),
        DEFAULT_COLOR,
    ));
    let corner = top_vertex(&scene);
    let faults = faults_for(&scene, corner, None);
    assert!(faults.is_empty(), "{faults:#?}");
}

#[test]
fn a_pot_sweep_is_stable() {
    let mut scene = Scene::default();
    scene.add(Object::new("pot", primitives::rounded_pot(), DEFAULT_COLOR));
    let corner = top_vertex(&scene);
    let faults = faults_for(&scene, corner, None);
    assert!(faults.is_empty(), "{faults:#?}");
}

/// The vertex of the first object with the largest x + y + z.
fn top_vertex(scene: &Scene) -> Vec3 {
    let o = &scene.objects[0];
    let model = o.matrix();
    o.mesh
        .positions
        .iter()
        .map(|p| model.transform_point3(*p))
        .max_by(|a, b| (a.x + a.y + a.z).total_cmp(&(b.x + b.y + b.z)))
        .unwrap()
}
