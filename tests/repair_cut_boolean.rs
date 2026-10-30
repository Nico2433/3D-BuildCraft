use buildcraft3d::boolean::{apply, Op};
use buildcraft3d::cut::cut;
use buildcraft3d::mesh::Mesh;
use buildcraft3d::primitives;
use buildcraft3d::repair::repair;
use glam::{Mat4, Vec3};

fn shifted(m: &Mesh, x: f32, y: f32) -> Mesh {
    m.transformed(&Mat4::from_translation(Vec3::new(x, y, 0.0)))
}

fn closed_volume(m: &Mesh) -> f64 {
    let s = m.stats();
    assert!(s.is_closed_manifold(), "{s:?}");
    s.volume
}

/// Every triangle with its own three vertices.
fn unwelded(m: &Mesh) -> Mesh {
    let pos: Vec<Vec3> = m.triangle_positions().flatten().collect();
    let tris = (0..m.triangles.len() as u32)
        .map(|i| [3 * i, 3 * i + 1, 3 * i + 2])
        .collect();
    Mesh::new(pos, tris)
}

// ---- repair ----

#[test]
fn repair_fixes_a_soup_with_a_hole_and_a_flipped_face_together() {
    let mut m = primitives::cube(10.0);
    m.triangles.remove(5); // hole
    m.triangles[2].swap(1, 2); // flipped face elsewhere
    let (fixed, r) = repair(&unwelded(&m));
    assert!((closed_volume(&fixed) - 1000.0).abs() < 1e-3);
    assert!(r.holes_filled >= 1, "{r:?}");
    assert!(r.flipped >= 1, "{r:?}");
    assert_eq!(r.holes_left, 0);
    assert_eq!(fixed.stats().components, 1);
}

#[test]
fn repair_fixes_a_hole_a_flip_a_duplicate_and_a_degenerate_triangle_at_once() {
    let mut m = primitives::cube(10.0);
    m.triangles.remove(0);
    m.triangles[7].swap(0, 1);
    let dup = m.triangles[3];
    m.triangles.push(dup);
    m.triangles.push([1, 1, 2]);
    let (fixed, r) = repair(&m);
    assert!((closed_volume(&fixed) - 1000.0).abs() < 1e-3);
    assert!(
        r.removed_degenerate >= 1 && r.removed_duplicate >= 1,
        "{r:?}"
    );
}

#[test]
fn repair_closes_a_multi_triangle_hole_in_a_sphere_and_keeps_the_volume_close() {
    let sphere = primitives::sphere(20.0, 32, 16);
    let v = sphere.stats().volume;
    let mut m = sphere.clone();
    // Punch out a few triangles around one vertex.
    let v0 = m.triangles[100][0];
    m.triangles.retain(|t| !t.contains(&v0));
    assert!(m.stats().boundary_edges > 0);
    let (fixed, r) = repair(&m);
    assert!(r.holes_filled >= 1);
    assert!((closed_volume(&fixed) - v).abs() / v < 0.02);
}

#[test]
fn repair_keeps_each_component_of_a_multi_part_mesh() {
    let a = primitives::cube(10.0);
    let mut open = shifted(&primitives::cube(10.0), 40.0, 0.0);
    open.triangles.pop();
    let mut m = a.clone();
    m.append(&open);
    let (fixed, _) = repair(&m);
    assert_eq!(fixed.stats().components, 2);
    assert!((closed_volume(&fixed) - 2000.0).abs() < 1e-2);
}

#[test]
fn repair_flips_an_inside_out_mesh_to_a_positive_volume() {
    let mut m = primitives::pyramid(10.0, 12.0);
    m.flip();
    assert!(m.stats().volume < 0.0);
    let (fixed, _) = repair(&m);
    assert!(closed_volume(&fixed) > 0.0);
}

#[test]
fn repair_of_an_empty_mesh_does_not_panic() {
    let (m, r) = repair(&Mesh::default());
    assert!(m.is_empty());
    assert_eq!(r.holes_filled, 0);
}

#[test]
fn repair_is_idempotent_on_its_own_output() {
    let mut m = primitives::cube(10.0);
    m.triangles.remove(4);
    let (once, _) = repair(&m);
    let (twice, r) = repair(&once);
    assert_eq!(once.stats(), twice.stats());
    assert_eq!(
        (r.holes_filled, r.flipped, r.removed_nonmanifold),
        (0, 0, 0)
    );
}

// ---- cut ----

#[test]
fn cut_of_an_open_mesh_does_not_panic_and_leaves_the_halves_open() {
    let mut m = primitives::cube(10.0);
    m.triangles.remove(4); // one side triangle that spans the cut plane
    let (top, bottom) = cut(&m, 5.0);
    assert!(!top.is_empty() && !bottom.is_empty());
    assert!(top.stats().boundary_edges > 0);
    assert!(bottom.stats().boundary_edges > 0);
    assert!(top.positions.iter().all(|p| p.z >= 5.0 - 1e-5));
    assert!(bottom.positions.iter().all(|p| p.z <= 5.0 + 1e-5));
}

#[test]
fn cut_exactly_through_the_equator_vertices_of_an_octahedron() {
    let r = 10.0;
    let p = vec![
        Vec3::new(r, 0.0, 0.0),
        Vec3::new(0.0, r, 0.0),
        Vec3::new(-r, 0.0, 0.0),
        Vec3::new(0.0, -r, 0.0),
        Vec3::new(0.0, 0.0, r),
        Vec3::new(0.0, 0.0, -r),
    ];
    let mut tris = Vec::new();
    for i in 0..4u32 {
        let j = (i + 1) % 4;
        tris.push([i, j, 4]);
        tris.push([j, i, 5]);
    }
    let octa = Mesh::new(p, tris);
    let total = closed_volume(&octa);
    assert!((total - 4.0 / 3.0 * 1000.0).abs() < 1e-2);
    let (top, bottom) = cut(&octa, 0.0);
    assert!((closed_volume(&top) - total / 2.0).abs() < 1e-2);
    assert!((closed_volume(&bottom) - total / 2.0).abs() < 1e-2);
}

#[test]
fn cut_plane_through_vertices_of_the_top_face_leaves_the_whole_solid_below() {
    let m = primitives::cube(10.0);
    let (top, bottom) = cut(&m, 10.0);
    // Everything of volume is below the plane; whatever remains above is flat.
    assert!((closed_volume(&bottom) - 1000.0).abs() < 1e-3);
    assert!(top.is_empty() || top.stats().volume.abs() < 1e-3);
}

#[test]
fn cut_plane_through_the_bottom_face_keeps_the_whole_solid_above() {
    let m = primitives::cube(10.0);
    let (top, bottom) = cut(&m, 0.0);
    assert!((closed_volume(&top) - 1000.0).abs() < 1e-3);
    assert!(bottom.is_empty() || bottom.stats().volume.abs() < 1e-3);
}

#[test]
fn cut_plane_through_the_apex_of_a_pyramid() {
    let m = primitives::pyramid(10.0, 10.0);
    let (top, bottom) = cut(&m, 10.0);
    assert!((closed_volume(&bottom) - m.stats().volume).abs() < 1e-3);
    assert!(top.is_empty() || top.stats().volume.abs() < 1e-3);
}

#[test]
fn cut_halves_of_a_translated_solid_use_world_heights() {
    let m = primitives::cube(10.0).transformed(&Mat4::from_translation(Vec3::new(0.0, 0.0, 100.0)));
    let (top, bottom) = cut(&m, 103.0);
    assert!((closed_volume(&top) - 700.0).abs() < 1e-2);
    assert!((closed_volume(&bottom) - 300.0).abs() < 1e-2);
    assert!((top.aabb().min.z - 103.0).abs() < 1e-4);
    assert!((bottom.aabb().max.z - 103.0).abs() < 1e-4);
}

// ---- boolean ----

#[test]
fn union_of_disjoint_cubes_keeps_both_as_separate_closed_parts() {
    let r = apply(
        Op::Union,
        &[
            primitives::cube(10.0),
            shifted(&primitives::cube(10.0), 50.0, 0.0),
        ],
    )
    .unwrap();
    assert!((closed_volume(&r) - 2000.0).abs() < 1e-2);
    assert_eq!(r.stats().components, 2);
}

#[test]
fn subtracting_a_bigger_or_identical_solid_gives_an_empty_mesh() {
    let a = primitives::cube(10.0);
    assert!(apply(Op::Subtract, &[a.clone(), a.clone()])
        .unwrap()
        .is_empty());
    assert!(apply(Op::Subtract, &[a.clone(), primitives::cube(20.0)])
        .unwrap()
        .is_empty());
}

#[test]
fn subtracting_a_disjoint_solid_changes_nothing() {
    let a = primitives::cube(10.0);
    let r = apply(Op::Subtract, &[a.clone(), shifted(&a, 50.0, 0.0)]).unwrap();
    assert!((closed_volume(&r) - 1000.0).abs() < 1e-2);
}

#[test]
fn intersecting_disjoint_solids_gives_an_empty_mesh() {
    let a = primitives::cube(10.0);
    assert!(apply(Op::Intersect, &[a.clone(), shifted(&a, 50.0, 0.0)])
        .unwrap()
        .is_empty());
}

#[test]
fn three_operand_union_is_applied_to_all_three() {
    let a = primitives::cube(10.0);
    let r = apply(
        Op::Union,
        &[a.clone(), shifted(&a, 5.0, 0.0), shifted(&a, 10.0, 0.0)],
    )
    .unwrap();
    // One 20 x 10 x 10 slab.
    assert!((closed_volume(&r) - 2000.0).abs() < 1e-2);
    assert_eq!(r.stats().components, 1);
}

#[test]
fn three_operand_subtract_runs_left_to_right() {
    let a = primitives::cube(10.0);
    // A minus B (x 0..5) minus C (y 0..5): the quarter x 0..5, y 0..5 was already gone.
    let r = apply(
        Op::Subtract,
        &[a.clone(), shifted(&a, 5.0, 0.0), shifted(&a, 0.0, 5.0)],
    )
    .unwrap();
    assert!((closed_volume(&r) - 250.0).abs() < 1e-2);
}

#[test]
fn three_operand_intersection_keeps_only_the_common_part() {
    let a = primitives::cube(10.0);
    let r = apply(
        Op::Intersect,
        &[a.clone(), shifted(&a, 5.0, 0.0), shifted(&a, 8.0, 0.0)],
    )
    .unwrap();
    // x range [-5,5] ∩ [0,10] ∩ [3,13] = [3,5].
    assert!((closed_volume(&r) - 200.0).abs() < 1e-2);
}

#[test]
fn a_single_operand_comes_back_unchanged_in_volume() {
    let r = apply(Op::Union, &[primitives::sphere(10.0, 24, 12)]).unwrap();
    let v = primitives::sphere(10.0, 24, 12).stats().volume;
    assert!((closed_volume(&r) - v).abs() / v < 1e-4);
}

#[test]
fn no_operands_is_an_error() {
    assert!(apply(Op::Union, &[]).is_err());
}

#[test]
fn any_open_operand_is_an_error_in_any_position() {
    let good = primitives::cube(10.0);
    let mut open = good.clone();
    open.triangles.pop();
    for list in [
        vec![good.clone(), open.clone()],
        vec![open.clone(), good.clone()],
        vec![good.clone(), shifted(&good, 5.0, 0.0), open.clone()],
    ] {
        assert!(apply(Op::Union, &list).is_err());
    }
}

// ---- separate solids ----

/// A 1 mm slab through the middle of `m`, larger than it in Y and Z.
fn slab_through(m: &Mesh) -> Mesh {
    let c = m.aabb().center();
    primitives::box_mesh(Vec3::new(1.0, 30.0, 30.0))
        .transformed(&Mat4::from_translation(c - Vec3::Z * 15.0))
}

#[test]
fn subtracting_a_slab_through_a_cube_leaves_two_solids() {
    let a = primitives::cube(10.0);
    let r = apply(Op::Subtract, &[a.clone(), slab_through(&a)]).unwrap();
    let parts = r.solids();
    assert_eq!(parts.len(), 2);
    for p in &parts {
        assert!((closed_volume(p) - 450.0).abs() < 1e-2);
    }
}

#[test]
fn a_hollow_solid_keeps_its_cavity() {
    let a = primitives::cube(10.0);
    let small = primitives::cube(4.0);
    let mut cavity = small.transformed(&Mat4::from_translation(
        a.aabb().center() - small.aabb().center(),
    ));
    cavity.flip();
    let mut hollow = a.clone();
    hollow.append(&cavity);
    let far = shifted(&a, 50.0, 0.0);
    hollow.append(&far);
    let parts = hollow.solids();
    assert_eq!(parts.len(), 2);
    let mut v: Vec<f64> = parts.iter().map(closed_volume).collect();
    v.sort_by(f64::total_cmp);
    assert!(
        (v[0] - 936.0).abs() < 1e-2 && (v[1] - 1000.0).abs() < 1e-2,
        "{v:?}"
    );
}

#[test]
fn one_solid_stays_whole() {
    let a = primitives::cube(10.0);
    assert_eq!(a.solids(), vec![a]);
}
