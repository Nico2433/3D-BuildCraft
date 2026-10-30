use buildcraft3d::cut::Keep;
use buildcraft3d::io::ModelObject;
use buildcraft3d::primitives;
use buildcraft3d::scene::{Axis, Object, Scene};
use glam::{Mat4, Quat, Vec3};

const RED: [u8; 4] = [255, 0, 0, 255];

fn cube_at(x: f32, z: f32) -> Object {
    let mut o = Object::new("cube", primitives::cube(10.0), RED);
    o.translation = Vec3::new(x, 0.0, z);
    o
}

fn scene_with(objs: Vec<Object>) -> (Scene, Vec<u64>) {
    let mut s = Scene::default();
    let ids = objs.into_iter().map(|o| s.add(o)).collect();
    s.dirty = false;
    (s, ids)
}

fn close(a: Vec3, b: Vec3) -> bool {
    (a - b).length() < 1e-3
}

#[test]
fn add_assigns_distinct_ids_and_marks_dirty() {
    let mut s = Scene::default();
    assert!(!s.dirty);
    let (a, b) = (s.add(cube_at(0.0, 0.0)), s.add(cube_at(20.0, 0.0)));
    assert_ne!(a, b);
    assert!(s.dirty);
    assert!(s.get(a).is_some() && s.get(b).is_some() && s.get(999).is_none());
}

#[test]
fn settle_lowers_a_floating_object_onto_the_floor() {
    let (mut s, ids) = scene_with(vec![cube_at(0.0, 7.0)]);
    s.settle(ids[0]);
    assert!(s.get(ids[0]).unwrap().world_aabb().min.z.abs() < 1e-4);
    assert!(s.dirty);
}

#[test]
fn settle_raises_an_object_below_the_floor() {
    let (mut s, ids) = scene_with(vec![cube_at(0.0, -3.0)]);
    s.settle(ids[0]);
    assert!(s.get(ids[0]).unwrap().world_aabb().min.z.abs() < 1e-4);
    assert!(s.dirty);
}

#[test]
fn settle_accounts_for_rotation_and_scale() {
    let mut o = cube_at(0.0, 50.0);
    o.rotation = Vec3::new(0.0, 45.0, 0.0);
    o.scale = Vec3::new(2.0, 1.0, 3.0);
    let (mut s, ids) = scene_with(vec![o]);
    s.settle(ids[0]);
    assert!(s.get(ids[0]).unwrap().world_aabb().min.z.abs() < 1e-3);
}

#[test]
fn settle_with_an_unknown_id_changes_nothing() {
    let (mut s, _) = scene_with(vec![cube_at(0.0, 7.0)]);
    s.settle(12345);
    assert!(!s.dirty);
    assert_eq!(s.objects[0].translation.z, 7.0);
}

#[test]
fn duplicate_selects_only_the_copy_and_shifts_it_by_width_plus_five() {
    let (mut s, ids) = scene_with(vec![cube_at(0.0, 0.0), cube_at(100.0, 0.0)]);
    s.selection.insert(ids[0]);
    s.duplicate_selected();
    assert_eq!(s.objects.len(), 3);
    assert_eq!(s.selection.len(), 1);
    let copy = *s.selection.iter().next().unwrap();
    assert!(!ids.contains(&copy));
    let (orig, dup) = (
        s.get(ids[0]).unwrap().world_aabb(),
        s.get(copy).unwrap().world_aabb(),
    );
    assert!(
        close(dup.min - orig.min, Vec3::new(15.0, 0.0, 0.0)),
        "{:?}",
        dup.min - orig.min
    );
    assert!(s.dirty);
}

#[test]
fn duplicate_of_several_objects_shifts_by_the_selection_width_plus_five() {
    let (mut s, ids) = scene_with(vec![cube_at(0.0, 0.0), cube_at(30.0, 0.0)]);
    s.select_all();
    s.duplicate_selected();
    assert_eq!(s.objects.len(), 4);
    assert_eq!(s.selection.len(), 2);
    assert!(ids.iter().all(|i| !s.selection.contains(i)));
    // Selection spanned x -5..35 (40 wide), so the copies move by 45.
    let copy_min = s
        .selected()
        .map(|o| o.world_aabb().min.x)
        .fold(f32::MAX, f32::min);
    assert!((copy_min - 40.0).abs() < 1e-3, "{copy_min}");
}

#[test]
fn duplicate_without_selection_adds_nothing() {
    let (mut s, _) = scene_with(vec![cube_at(0.0, 0.0)]);
    s.duplicate_selected();
    assert_eq!(s.objects.len(), 1);
}

#[test]
fn paste_adds_copies_offset_by_ten_ten_and_selects_them() {
    let (mut s, ids) = scene_with(vec![cube_at(0.0, 0.0)]);
    s.selection.insert(ids[0]);
    let clipboard = vec![cube_at(0.0, 0.0), cube_at(50.0, 0.0)];
    s.paste(&clipboard);
    assert_eq!(s.objects.len(), 3);
    assert_eq!(s.selection.len(), 2);
    assert!(!s.selection.contains(&ids[0]));
    let mins: Vec<Vec3> = s.selected().map(|o| o.world_aabb().min).collect();
    assert!(close(mins[0], Vec3::new(5.0, 5.0, 0.0)), "{:?}", mins[0]);
    assert!(close(mins[1], Vec3::new(55.0, 5.0, 0.0)), "{:?}", mins[1]);
    assert!(s.dirty);
}

#[test]
fn pasted_objects_get_fresh_ids() {
    let (mut s, ids) = scene_with(vec![cube_at(0.0, 0.0)]);
    let clip = vec![s.objects[0].clone()];
    s.paste(&clip);
    s.paste(&clip);
    let mut all: Vec<u64> = s.objects.iter().map(|o| o.id).collect();
    all.sort();
    all.dedup();
    assert_eq!(all.len(), 3);
    assert!(all.contains(&ids[0]));
}

#[test]
fn select_all_and_invert() {
    let (mut s, ids) = scene_with(vec![cube_at(0.0, 0.0), cube_at(20.0, 0.0)]);
    s.select_all();
    assert_eq!(s.selected_ids(), ids);
    s.selection.clear();
    s.invert_selection();
    assert_eq!(s.selected_ids(), ids, "inverting nothing selects all");
    s.selection = [ids[0]].into();
    s.invert_selection();
    assert_eq!(s.selected_ids(), vec![ids[1]]);
    s.select_all();
    s.invert_selection();
    assert!(s.selection.is_empty());
}

#[test]
fn delete_selected_removes_exactly_those_and_clears_the_selection() {
    let (mut s, ids) = scene_with(vec![
        cube_at(0.0, 0.0),
        cube_at(20.0, 0.0),
        cube_at(40.0, 0.0),
    ]);
    s.selection = [ids[0], ids[2]].into();
    s.delete_selected();
    assert_eq!(s.objects.len(), 1);
    assert_eq!(s.objects[0].id, ids[1]);
    assert!(s.selection.is_empty());
    assert!(s.dirty);
}

fn model(mesh: buildcraft3d::mesh::Mesh, transform: Mat4) -> ModelObject {
    ModelObject {
        name: "m".into(),
        mesh,
        transform,
        color: RED,
    }
}

#[test]
fn sheared_transform_is_baked_into_the_mesh() {
    let shear = Mat4::from_cols(
        glam::Vec4::new(1.0, 0.0, 0.0, 0.0),
        glam::Vec4::new(0.5, 1.0, 0.0, 0.0),
        glam::Vec4::new(0.0, 0.0, 1.0, 0.0),
        glam::Vec4::new(0.0, 0.0, 0.0, 1.0),
    );
    let mesh = primitives::wedge(10.0, 20.0, 30.0);
    let o = Object::from_model(model(mesh.clone(), shear));
    assert_eq!(o.translation, Vec3::ZERO);
    assert_eq!(o.rotation, Vec3::ZERO);
    assert_eq!(o.scale, Vec3::ONE);
    for (got, orig) in o.mesh.positions.iter().zip(&mesh.positions) {
        assert!(close(*got, shear.transform_point3(*orig)));
    }
    assert!(close(
        o.world_aabb().size(),
        mesh.aabb_transformed(&shear).size()
    ));
}

#[test]
fn shear_far_from_the_origin_is_still_baked() {
    let shear = Mat4::from_translation(Vec3::new(100_000.0, 0.0, 0.0))
        * Mat4::from_cols(
            glam::Vec4::new(1.0, 0.0, 0.0, 0.0),
            glam::Vec4::new(0.5, 1.0, 0.0, 0.0),
            glam::Vec4::new(0.0, 0.0, 1.0, 0.0),
            glam::Vec4::new(0.0, 0.0, 0.0, 1.0),
        );
    let mesh = primitives::cube(10.0);
    let o = Object::from_model(model(mesh.clone(), shear));
    assert_eq!(o.scale, Vec3::ONE);
    assert_eq!(o.translation, Vec3::ZERO);
    for (got, orig) in o.mesh.positions.iter().zip(&mesh.positions) {
        assert!((*got - shear.transform_point3(*orig)).length() < 0.05);
    }
}

#[test]
fn translation_rotation_scale_is_not_baked() {
    let t = Mat4::from_scale_rotation_translation(
        Vec3::new(2.0, 3.0, 4.0),
        Quat::from_rotation_z(30f32.to_radians()),
        Vec3::new(1.0, 2.0, 3.0),
    );
    let mesh = primitives::wedge(10.0, 20.0, 30.0);
    let o = Object::from_model(model(mesh.clone(), t));
    assert_eq!(
        o.mesh.positions, mesh.positions,
        "mesh stays in object space"
    );
    assert!(close(o.translation, Vec3::new(1.0, 2.0, 3.0)));
    assert!(close(o.scale, Vec3::new(2.0, 3.0, 4.0)));
    assert!((o.rotation.z - 30.0).abs() < 1e-3);
    for (got, orig) in o.world_mesh().positions.iter().zip(&mesh.positions) {
        assert!(close(*got, t.transform_point3(*orig)));
    }
    // And it writes back to the same placement.
    assert!(o.to_model().transform.abs_diff_eq(t, 1e-4));
}

#[test]
fn pure_translation_is_not_baked() {
    let o = Object::from_model(model(
        primitives::cube(10.0),
        Mat4::from_translation(Vec3::new(5.0, 6.0, 7.0)),
    ));
    assert!(close(o.translation, Vec3::new(5.0, 6.0, 7.0)));
    assert_eq!(o.scale, Vec3::ONE);
}

#[test]
fn mirrored_transform_gives_the_right_world_box_and_a_positive_volume() {
    let t = Mat4::from_translation(Vec3::new(100.0, 0.0, 0.0))
        * Mat4::from_scale(Vec3::new(-1.0, 1.0, 1.0));
    let mesh = primitives::wedge(10.0, 20.0, 30.0);
    let o = Object::from_model(model(mesh.clone(), t));
    let expect = mesh.aabb_transformed(&t);
    assert!(close(o.world_aabb().min, expect.min) && close(o.world_aabb().max, expect.max));
    let w = o.world_mesh().stats();
    assert!(w.is_closed_manifold() && w.volume > 0.0, "{w:?}");
}

#[test]
fn bake_applies_the_transform_and_resets_it() {
    let mut o = cube_at(10.0, 0.0);
    o.scale = Vec3::splat(2.0);
    let before = o.world_aabb();
    o.bake();
    assert_eq!(
        (o.translation, o.rotation, o.scale),
        (Vec3::ZERO, Vec3::ZERO, Vec3::ONE)
    );
    assert!(close(o.world_aabb().min, before.min) && close(o.world_aabb().max, before.max));
}

#[test]
fn insert_on_floor_centres_on_the_origin_rests_on_the_floor_and_selects() {
    let mut o = Object::new("w", primitives::wedge(10.0, 20.0, 30.0), RED);
    o.translation = Vec3::new(77.0, -33.0, 12.0);
    let mut s = Scene::default();
    let id = s.insert_on_floor(o);
    let b = s.get(id).unwrap().world_aabb();
    assert!(b.min.z.abs() < 1e-4);
    assert!(b.center().x.abs() < 1e-4 && b.center().y.abs() < 1e-4);
    assert_eq!(s.selected_ids(), vec![id]);
}

#[test]
fn resize_selection_gives_the_selection_box_the_requested_size_and_keeps_its_centre() {
    let (mut s, ids) = scene_with(vec![cube_at(0.0, 0.0)]);
    s.selection.insert(ids[0]);
    let centre = s.selection_aabb().center();
    s.resize_selection(Vec3::new(20.0, 10.0, 5.0));
    let b = s.selection_aabb();
    assert!(
        close(b.size(), Vec3::new(20.0, 10.0, 5.0)),
        "{:?}",
        b.size()
    );
    assert!(close(b.center(), centre));
    assert!(s.dirty);
}

#[test]
fn resize_selection_of_a_rotated_object_is_measured_on_the_world_axes() {
    let mut o = Object::new("w", primitives::box_mesh(Vec3::new(10.0, 20.0, 30.0)), RED);
    o.rotation = Vec3::new(0.0, 0.0, 90.0);
    let (mut s, ids) = scene_with(vec![o]);
    s.selection.insert(ids[0]);
    s.resize_selection(Vec3::new(40.0, 40.0, 40.0));
    assert!(
        close(s.selection_aabb().size(), Vec3::splat(40.0)),
        "{:?}",
        s.selection_aabb().size()
    );
}

#[test]
fn resize_selection_spreads_several_objects_about_their_common_centre() {
    let (mut s, _) = scene_with(vec![cube_at(0.0, 0.0), cube_at(30.0, 0.0)]);
    s.select_all();
    let centre = s.selection_aabb().center();
    s.resize_selection(Vec3::new(80.0, 20.0, 10.0));
    let b = s.selection_aabb();
    assert!(
        close(b.size(), Vec3::new(80.0, 20.0, 10.0)),
        "{:?}",
        b.size()
    );
    assert!(close(b.center(), centre));
}

#[test]
fn mirror_selected_flips_the_shape_in_place_and_keeps_it_a_valid_solid() {
    let mut o = Object::new("w", primitives::wedge(10.0, 20.0, 30.0), RED);
    o.translation = Vec3::new(40.0, 0.0, 0.0);
    let (mut s, ids) = scene_with(vec![o]);
    s.selection.insert(ids[0]);
    let before = s.get(ids[0]).unwrap().world_aabb();
    let v = s.get(ids[0]).unwrap().world_mesh().stats().volume;
    s.mirror_selected(Axis::X);
    let after = s.get(ids[0]).unwrap().world_aabb();
    assert!(close(before.min, after.min) && close(before.max, after.max));
    let st = s.get(ids[0]).unwrap().world_mesh().stats();
    assert!(
        st.is_closed_manifold() && (st.volume - v).abs() < 1e-3,
        "{st:?}"
    );
    // The ramp's high side was at -X; now it is at +X.
    let top_x = |sc: &Scene| {
        let m = sc.get(ids[0]).unwrap().world_mesh();
        m.positions
            .iter()
            .filter(|p| p.z > 29.0)
            .map(|p| p.x)
            .sum::<f32>()
            / 2.0
    };
    assert!(top_x(&s) > 40.0);
    assert!(s.dirty);
}

#[test]
fn cut_selected_keeps_the_chosen_halves_selected() {
    for (keep, volumes) in [
        (Keep::Top, vec![600.0]),
        (Keep::Bottom, vec![400.0]),
        (Keep::Both, vec![600.0, 400.0]),
    ] {
        let (mut s, ids) = scene_with(vec![cube_at(0.0, 0.0), cube_at(50.0, 0.0)]);
        s.selection.insert(ids[0]);
        s.cut_selected(4.0, keep);
        // The untouched cube is still there; the cut one is replaced by its halves.
        assert_eq!(s.objects.len(), 1 + volumes.len(), "{keep:?}");
        assert!(s.get(ids[1]).is_some());
        assert_eq!(s.selection.len(), volumes.len());
        let mut got: Vec<f64> = s
            .selected()
            .map(|o| o.world_mesh().stats().volume)
            .collect();
        got.sort_by(|a, b| b.partial_cmp(a).unwrap());
        for (g, e) in got.iter().zip(&volumes) {
            assert!((g - e).abs() < 1e-2, "{keep:?}: {got:?}");
        }
    }
}

#[test]
fn cut_selected_above_the_object_with_keep_top_removes_it() {
    let (mut s, ids) = scene_with(vec![cube_at(0.0, 0.0)]);
    s.selection.insert(ids[0]);
    s.cut_selected(50.0, Keep::Top);
    assert!(s.objects.is_empty());
}

#[test]
fn simplify_selected_reduces_triangles_only_for_the_selection() {
    let sphere = || Object::new("s", primitives::sphere(20.0, 48, 24), RED);
    let (mut s, ids) = scene_with(vec![sphere(), sphere()]);
    s.selection.insert(ids[0]);
    let n = s.objects[0].mesh.triangles.len();
    s.simplify_selected(0.25);
    assert!(s.get(ids[0]).unwrap().mesh.triangles.len() <= n / 4 + 2);
    assert_eq!(s.get(ids[1]).unwrap().mesh.triangles.len(), n);
}

#[test]
fn to_models_keeps_every_object_with_its_matrix() {
    let (s, _) = scene_with(vec![cube_at(5.0, 0.0), cube_at(9.0, 1.0)]);
    let m = s.to_models();
    assert_eq!(m.len(), 2);
    assert!(close(
        m[1].transform.transform_point3(Vec3::ZERO),
        Vec3::new(9.0, 0.0, 1.0)
    ));
}

#[test]
fn scene_aabb_and_titles() {
    let (mut s, _) = scene_with(vec![cube_at(0.0, 0.0), cube_at(30.0, 0.0)]);
    assert!(close(s.scene_aabb().size(), Vec3::new(40.0, 10.0, 10.0)));
    assert!(!Scene::default().scene_aabb().is_valid());
    assert!(!s.title().is_empty());
    s.name = Some("Mine".into());
    assert_eq!(s.title(), "Mine");
}

#[test]
fn turning_about_a_point_keeps_it_in_place_whatever_the_mesh_origin() {
    // The mesh sits away from its own origin, as after a bake.
    let mesh = buildcraft3d::primitives::cube(10.0).transformed(&glam::Mat4::from_translation(
        glam::Vec3::new(30.0, 0.0, 0.0),
    ));
    let mut o = buildcraft3d::scene::Object::new("c", mesh, buildcraft3d::scene::DEFAULT_COLOR);
    o.rotation = glam::Vec3::new(0.0, 0.0, 30.0);
    let c = o.world_aabb().center();
    for axis in [glam::Vec3::X, glam::Vec3::Y, glam::Vec3::Z] {
        o.turn_about(glam::Quat::from_axis_angle(axis, 1.0), c);
        let now = o.world_aabb().center();
        assert!(now.distance(c) < 4.0, "{axis}: {now} vs {c}");
    }
    // A quarter turn about Z twice is a half turn: the box is back to its size.
    let mut q = buildcraft3d::scene::Object::new(
        "q",
        buildcraft3d::primitives::box_mesh(glam::Vec3::new(20.0, 10.0, 5.0)),
        buildcraft3d::scene::DEFAULT_COLOR,
    );
    let quarter = glam::Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
    q.turn_about(quarter, glam::Vec3::ZERO);
    let s = q.world_aabb().size();
    assert!(
        (s.x - 10.0).abs() < 1e-3 && (s.y - 20.0).abs() < 1e-3,
        "{s}"
    );
}
