use buildcraft3d::{
    mesh::Mesh,
    primitives::{self, Primitive},
    scene::{Object, DEFAULT_COLOR},
    solid,
};
use glam::{Mat4, Vec3};

fn assert_close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "expected {expected} ± {tolerance}, got {actual}"
    );
}

fn assert_vec3_close(actual: Vec3, expected: Vec3, tolerance: f32) {
    assert!(
        actual.abs_diff_eq(expected, tolerance),
        "expected {expected:?} ± {tolerance}, got {actual:?}"
    );
}

fn assert_closed(mesh: &Mesh) {
    let stats = mesh.stats();
    assert!(stats.is_closed_manifold(), "{stats:?}");
}

#[test]
fn hollow_cube_keeps_bounds_and_leaves_at_least_a_two_mm_wall() {
    let cube = Primitive::Cube.build();
    let hollow = solid::hollow(&cube, 2.0).unwrap();
    let volume = hollow.stats().volume;

    assert_closed(&hollow);
    assert!(volume < 40.0_f64.powi(3), "{volume}");
    assert!(volume >= 40.0_f64.powi(3) - 36.0_f64.powi(3), "{volume}");
    assert_vec3_close(hollow.aabb().min, cube.aabb().min, 1e-3);
    assert_vec3_close(hollow.aabb().max, cube.aabb().max, 1e-3);
}

#[test]
fn hollow_rejects_a_box_thinner_than_twice_the_wall() {
    let thin = primitives::box_mesh(Vec3::new(40.0, 40.0, 1.0));
    assert!(solid::hollow(&thin, 2.0).is_err());
}

#[test]
fn solid_edits_reject_an_open_mesh() {
    let mut open = primitives::cube(40.0);
    open.triangles.pop();

    assert!(solid::smooth(&open).is_err());
    assert!(solid::hollow(&open, 2.0).is_err());
    assert!(solid::extrude_down(&open, 0.0).is_err());
}

#[test]
fn extrude_down_extends_a_lifted_cube_to_the_floor() {
    let cube =
        primitives::cube(40.0).transformed(&Mat4::from_translation(Vec3::new(0.0, 0.0, 10.0)));
    let extruded = solid::extrude_down(&cube, 0.0).unwrap();

    assert_closed(&extruded);
    assert_close(extruded.aabb().min.z as f64, 0.0, 1e-4);
    assert_close(extruded.aabb().max.z as f64, 50.0, 1e-4);
    assert_close(extruded.stats().volume, 40.0 * 40.0 * 50.0, 1e-2);
}

#[test]
fn extrude_down_keeps_a_cube_already_on_the_floor() {
    let extruded = solid::extrude_down(&primitives::cube(40.0), 0.0).unwrap();

    assert_closed(&extruded);
    assert_close(extruded.stats().volume, 40.0_f64.powi(3), 1e-2);
}

#[test]
fn extrude_down_fills_below_a_rotated_lifted_torus() {
    let torus = primitives::torus(15.0, 5.0, 24, 12);
    let rotated = torus.transformed(&Mat4::from_rotation_x(40.0_f32.to_radians()));
    let lifted = rotated.transformed(&Mat4::from_translation(Vec3::new(
        0.0,
        0.0,
        10.0 - rotated.aabb().min.z,
    )));
    let extruded = solid::extrude_down(&lifted, 0.0).unwrap();

    assert!(lifted.aabb().min.z > 0.0);
    assert_closed(&extruded);
    assert_close(extruded.aabb().min.z as f64, 0.0, 1e-3);
    assert!(extruded.stats().volume > torus.stats().volume);
}

#[test]
fn smooth_cube_subdivides_every_face_without_changing_its_volume() {
    let cube = primitives::cube(40.0);
    let smoothed = solid::smooth(&cube).unwrap();

    assert_closed(&smoothed);
    assert_eq!(smoothed.triangles.len(), 4 * cube.triangles.len());
    assert_close(smoothed.stats().volume, 40.0_f64.powi(3), 640.0);
}

#[test]
fn smooth_sphere_preserves_volume() {
    let sphere = primitives::sphere(20.0, 32, 16);
    let smoothed = solid::smooth(&sphere).unwrap();
    let ratio = smoothed.stats().volume / sphere.stats().volume;

    assert_closed(&smoothed);
    assert!((0.97..=1.03).contains(&ratio), "volume ratio: {ratio}");
}

#[test]
fn smooth_rejects_more_than_the_triangle_limit_before_geometry_validation() {
    let oversized = Mesh::new(
        vec![Vec3::ZERO],
        vec![[0, 0, 0]; solid::SMOOTH_MAX_TRIANGLES + 1],
    );
    assert!(solid::smooth(&oversized).is_err());
}

#[test]
fn set_world_mesh_resets_transform_and_uses_the_given_mesh() {
    let mut object = Object::new("cube", primitives::cube(10.0), DEFAULT_COLOR);
    object.translation = Vec3::new(1.0, 2.0, 3.0);
    object.rotation = Vec3::new(10.0, 20.0, 30.0);
    object.scale = Vec3::new(2.0, 3.0, 4.0);
    let world = primitives::sphere(5.0, 12, 6);

    object.set_world_mesh(world.clone());

    assert_eq!(object.translation, Vec3::ZERO);
    assert_eq!(object.rotation, Vec3::ZERO);
    assert_eq!(object.scale, Vec3::ONE);
    assert_eq!(object.world_mesh(), world);
}
