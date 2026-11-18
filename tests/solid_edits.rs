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
fn hollow_says_whether_the_object_is_thin_or_the_wall_too_thick_for_the_voxels() {
    let cube = primitives::cube(10.0);
    // Walls up to 4.5 work on a 10 mm cube; 4.99 leaves a cavity under one voxel.
    assert!(solid::hollow(&cube, 4.5).is_ok());
    let e = solid::hollow(&cube, 4.99).unwrap_err().to_string();
    assert!(e.starts_with("The wall is too thick for this size"), "{e}");
    assert!(e.contains("voxels") && e.contains("thinner wall"), "{e}");
    // Not more than twice the wall is plain thinness.
    for wall in [5.0, 6.0] {
        let e = solid::hollow(&cube, wall).unwrap_err().to_string();
        assert!(e.starts_with("The object is too thin to hollow"), "{e}");
        assert!(e.contains("smallest side is 10.00 mm"), "{e}");
    }
}

#[test]
fn solid_edits_reject_an_open_mesh() {
    let mut open = primitives::cube(40.0);
    open.triangles.pop();

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

/// Flat faces between the level-set grid planes made manifold-rust panic ("table full
/// before any vert") and took the app down; these sizes all did before the grid shifts.
#[test]
fn hollow_cubes_whose_walls_fall_between_grid_planes() {
    for (size, wall) in [(30.0, 2.0), (27.0, 1.0), (45.0, 3.0), (57.0, 3.0)] {
        let cube = primitives::cube(size);
        let hollow = solid::hollow(&cube, wall).unwrap();
        assert_closed(&hollow);
        // The cavity is a cube whose wall is `wall` plus up to the grid margin.
        let cavity = (size as f64).powi(3) - hollow.stats().volume;
        let side = |w: f64| (size as f64 - 2.0 * w).powi(3);
        assert!(cavity < side(wall as f64), "{size} {wall}: {cavity}");
        assert!(cavity > side(wall as f64 + 2.0), "{size} {wall}: {cavity}");
        println!(
            "{size} {wall}: cavity {cavity:.0} of {:.0}",
            side(wall as f64)
        );
    }
}
