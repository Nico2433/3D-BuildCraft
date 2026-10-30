use buildcraft3d::mesh::Mesh;
use buildcraft3d::primitives::{self, Primitive};
use buildcraft3d::scene::{Object, Scene, DEFAULT_COLOR};
use buildcraft3d::solid;
use glam::Vec3;

fn close(a: Vec3, b: Vec3) -> bool {
    (a - b).length() < 1e-3
}

fn box_tris(lat: usize, lon: usize) -> usize {
    2 * (2 * lon * lon + 4 * lon * lat)
}

fn obj(mesh: Mesh, at: Vec3) -> Object {
    let mut o = Object::new("part", mesh, DEFAULT_COLOR);
    o.translation = at;
    o
}

fn scene_with(objs: Vec<Object>) -> (Scene, Vec<u64>) {
    let mut s = Scene::default();
    let ids = objs.into_iter().map(|o| s.add(o)).collect();
    s.dirty = false;
    (s, ids)
}

#[test]
fn custom_clamps_low_counts() {
    let low = primitives::custom(Primitive::Cube, 0, 1, 20.0, 10.0);
    assert_eq!(low.triangles.len(), box_tris(1, 3));
    assert_eq!(low, primitives::custom(Primitive::Cube, 1, 3, 20.0, 10.0));
}

#[test]
fn custom_clamps_huge_counts_to_200() {
    let cyl = primitives::custom(Primitive::Cylinder, 1, usize::MAX, 20.0, 10.0);
    assert_eq!(cyl.triangles.len(), 4 * 200 - 4);
    let tall = primitives::custom(Primitive::Cube, usize::MAX, 3, 20.0, 10.0);
    assert_eq!(tall.triangles.len(), box_tris(200, 3));
}

#[test]
fn round_shape_triangle_counts_follow_lon_and_lat() {
    for (lat, lon) in [(1, 3), (5, 8), (12, 24)] {
        let cyl = primitives::custom(Primitive::Cylinder, lat, lon, 20.0, 10.0);
        assert_eq!(cyl.triangles.len(), 4 * lon - 4, "cylinder {lat}x{lon}");
        let sph = primitives::custom(Primitive::Sphere, lat, lon, 20.0, 10.0);
        assert_eq!(
            sph.triangles.len(),
            2 * lon * (lat.max(2) - 1),
            "sphere {lat}x{lon}"
        );
    }
}

#[test]
fn custom_torus_and_sphere_stay_closed_at_small_counts() {
    for (shape, lat, lon) in [
        (Primitive::Torus, 1, 3),
        (Primitive::Torus, 3, 3),
        (Primitive::Sphere, 1, 3),
        (Primitive::Sphere, 2, 3),
    ] {
        let m = primitives::custom(shape, lat, lon, 20.0, 10.0);
        let st = m.stats();
        assert!(st.is_closed_manifold(), "{shape:?} {lat}x{lon}: {st:?}");
        assert!(st.volume > 0.0, "{shape:?} {lat}x{lon} is inside out");
        let b = m.aabb();
        assert!(b.min.z.abs() < 1e-3 && (b.max.z - 10.0).abs() < 1e-3);
        assert!(b.size().x <= 20.0 + 1e-3 && b.size().y <= 20.0 + 1e-3);
    }
}

#[test]
fn custom_centres_across_and_keeps_proportions() {
    for p in Primitive::ALL {
        let m = primitives::custom(p, 6, 8, 30.0, 12.0);
        let b = m.aabb();
        let c = b.center();
        assert!(c.x.abs() < 1e-3 && c.y.abs() < 1e-3, "{p:?} {b:?}");
        assert!(b.min.z.abs() < 1e-3, "{p:?} {b:?}");
        let s = b.size();
        assert!((s.x.max(s.y) - 30.0).abs() < 1e-3, "{p:?} {b:?}");
        // Same aspect across as the default shape.
        let d = p.build().aabb().size();
        assert!((s.x / s.y - d.x / d.y).abs() < 1e-3, "{p:?} {s:?} vs {d:?}");
    }
}

#[test]
fn round_with_no_amount_returns_the_same_mesh() {
    let cube = primitives::cube(10.0);
    assert_eq!(solid::round(&cube, 0.0).unwrap(), cube);
    assert_eq!(solid::round(&cube, -1.0).unwrap(), cube);
}

#[test]
fn round_cube_stays_closed() {
    let r = solid::round(&primitives::cube(10.0), 0.5).unwrap();
    let st = r.stats();
    assert!(st.is_closed_manifold(), "{st:?}");
    assert!(st.volume > 0.0, "{st:?}");
}

#[test]
fn round_cube_stays_inside_its_box() {
    let cube = primitives::cube(10.0);
    let r = solid::round(&cube, 0.5).unwrap();
    let (b, o) = (r.aabb(), cube.aabb());
    let eps = Vec3::splat(1e-3);
    assert!(
        b.min.cmpge(o.min - eps).all() && b.max.cmple(o.max + eps).all(),
        "{b:?} vs {o:?}"
    );
}

#[test]
fn round_cube_loses_volume() {
    let cube = primitives::cube(10.0);
    let r = solid::round(&cube, 0.5).unwrap();
    let (v, v0) = (r.stats().volume, cube.stats().volume);
    assert!(v < v0 - 1.0, "rounded {v} vs original {v0}");
}

#[test]
fn round_open_mesh_errors() {
    let mut open = primitives::cube(10.0);
    open.triangles.pop();
    assert!(solid::round(&open, 0.5).is_err());
}

#[test]
fn round_refuses_too_many_triangles() {
    let big = primitives::custom(Primitive::Cube, 1, 130, 20.0, 10.0);
    assert!(big.triangles.len() > 60_000);
    assert!(solid::round(&big, 0.5).is_err());
    // Below the cap is fine.
    let ok = primitives::custom(Primitive::Cube, 1, 10, 20.0, 10.0);
    assert!(solid::round(&ok, 0.5).is_ok());
}

#[test]
fn scale_all_scales_about_the_footprint_and_keeps_layout() {
    let mut b = obj(primitives::cube(10.0), Vec3::new(30.0, 5.0, 4.0));
    b.rotation = Vec3::new(0.0, 0.0, 0.3);
    let (mut s, ids) = scene_with(vec![obj(primitives::cube(10.0), Vec3::ZERO), b]);
    s.selection.insert(ids[1]);
    let before = s.scene_aabb();
    let pivot = Vec3::new(before.center().x, before.center().y, before.min.z);
    let offsets: Vec<Vec3> = s
        .objects
        .iter()
        .map(|o| o.world_aabb().center() - pivot)
        .collect();

    s.scale_all(2.0);

    let after = s.scene_aabb();
    assert!(close(after.size(), before.size() * 2.0), "{after:?}");
    assert!(close(
        Vec3::new(after.center().x, after.center().y, after.min.z),
        pivot
    ));
    for (o, off) in s.objects.iter().zip(&offsets) {
        assert!(close(o.world_aabb().center() - pivot, *off * 2.0));
        assert!(close(o.scale, Vec3::splat(2.0)));
    }
    assert!(close(
        s.get(ids[1]).unwrap().rotation,
        Vec3::new(0.0, 0.0, 0.3)
    ));
    assert_eq!(
        s.selection.iter().copied().collect::<Vec<_>>(),
        vec![ids[1]]
    );
    assert!(s.dirty);
}

#[test]
fn scale_all_ignores_bad_factors_and_empty_scenes() {
    let (mut s, _) = scene_with(vec![obj(primitives::cube(10.0), Vec3::ZERO)]);
    for k in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        s.scale_all(k);
        assert!(close(s.objects[0].scale, Vec3::ONE), "k = {k}");
    }
    let mut empty = Scene::default();
    empty.scale_all(2.0);
    assert!(empty.objects.is_empty());
}

#[test]
fn reset_orientation_zeroes_rotations() {
    let mut a = obj(primitives::cube(10.0), Vec3::ZERO);
    a.rotation = Vec3::new(0.4, -1.0, 2.0);
    let mut b = obj(primitives::cube(10.0), Vec3::new(20.0, 0.0, 0.0));
    b.rotation = Vec3::new(0.0, 0.5, 0.0);
    let (mut s, _) = scene_with(vec![a, b]);
    s.reset_orientation();
    assert!(s.objects.iter().all(|o| o.rotation == Vec3::ZERO));
    assert!(s.dirty);
}

#[test]
fn reset_scale_returns_to_one_and_keeps_centres() {
    let mut a = obj(primitives::cube(10.0), Vec3::new(3.0, -2.0, 1.0));
    a.scale = Vec3::new(2.0, 0.5, 3.0);
    let mut b = obj(primitives::cube(10.0), Vec3::new(25.0, 4.0, 0.0));
    b.scale = Vec3::splat(0.25);
    b.rotation = Vec3::new(0.0, 0.0, 0.7);
    let (mut s, _) = scene_with(vec![a, b]);
    let centres: Vec<Vec3> = s.objects.iter().map(|o| o.world_aabb().center()).collect();
    s.reset_scale();
    for (o, c) in s.objects.iter().zip(&centres) {
        assert_eq!(o.scale, Vec3::ONE);
        assert!(close(o.world_aabb().center(), *c), "{:?}", o.world_aabb());
    }
    assert!(s.dirty);
}
