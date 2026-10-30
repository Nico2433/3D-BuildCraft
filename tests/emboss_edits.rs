use buildcraft3d::{
    emboss::{self, Emboss},
    mesh::Mesh,
    primitives::Primitive,
};
use glam::{Mat4, Vec2, Vec3};

fn assert_close(actual: f32, expected: f32, tolerance: f32) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "expected {expected} ± {tolerance}, got {actual}"
    );
}

fn assert_closed(mesh: &Mesh) {
    let stats = mesh.stats();
    assert!(stats.is_closed_manifold(), "{stats:?}");
}

fn emboss(text: &str) -> Emboss {
    Emboss {
        text: text.into(),
        ..Emboss::default()
    }
}

#[test]
fn engraving_reaches_the_requested_depth_and_removes_volume() {
    let cube = Primitive::Cube.build();
    let edit = Emboss {
        text: "I".into(),
        depth: 2.0,
        engrave: true,
        ..Emboss::default()
    };

    let text = emboss::text_solid(&cube, &edit).unwrap();
    assert_close(text.aabb().min.z, 38.0, 1e-3);
    assert_close(text.aabb().max.z, 40.5, 1e-3);

    let engraved = emboss::apply(&cube, &edit).unwrap().object;
    assert_closed(&engraved);
    assert!(engraved.stats().volume < cube.stats().volume);
    assert_close(engraved.aabb().max.z, 40.0, 1e-3);
}

#[test]
fn multiline_text_is_taller_than_one_line() {
    let cube = Primitive::Cube.build();
    let single = emboss::text_solid(&cube, &emboss("A")).unwrap().aabb();
    let multiline = emboss::text_solid(&cube, &emboss("A\nB")).unwrap().aabb();
    let single_height = single.max.y - single.min.y;
    let multiline_height = multiline.max.y - multiline.min.y;

    assert!(
        multiline_height > single_height * 1.8,
        "single: {single_height}, multiline: {multiline_height}"
    );
}

#[test]
fn ninety_degree_angle_swaps_text_extents() {
    let cube = Primitive::Cube.build();
    let unrotated = emboss::text_solid(&cube, &emboss("II")).unwrap().aabb();
    let rotated = emboss::text_solid(
        &cube,
        &Emboss {
            text: "II".into(),
            angle: 90.0,
            ..Emboss::default()
        },
    )
    .unwrap()
    .aabb();

    assert_close(
        rotated.max.x - rotated.min.x,
        unrotated.max.y - unrotated.min.y,
        1e-3,
    );
    assert_close(
        rotated.max.y - rotated.min.y,
        unrotated.max.x - unrotated.min.x,
        1e-3,
    );
}

#[test]
fn offset_moves_the_text_aabb_centre() {
    let cube = Primitive::Cube.build();
    let base = emboss::text_solid(&cube, &emboss("A")).unwrap().aabb();
    let moved = emboss::text_solid(
        &cube,
        &Emboss {
            text: "A".into(),
            offset: Vec2::new(5.0, -3.0),
            ..Emboss::default()
        },
    )
    .unwrap()
    .aabb();
    let base_centre = (base.min + base.max).truncate() * 0.5;
    let moved_centre = (moved.min + moved.max).truncate() * 0.5;

    assert!(
        (moved_centre - base_centre).abs_diff_eq(Vec2::new(5.0, -3.0), 1e-3),
        "base: {base_centre:?}, moved: {moved_centre:?}"
    );
}

#[test]
fn text_uses_the_world_space_surface_of_a_rotated_lifted_cube() {
    let transform = Mat4::from_translation(Vec3::new(10.0, -5.0, 7.0)) * Mat4::from_rotation_z(0.3);
    let cube = Primitive::Cube.build().transformed(&transform);
    let text = emboss::text_solid(&cube, &emboss("I")).unwrap();
    let bounds = text.aabb();
    let centre = (bounds.min + bounds.max).truncate() * 0.5;

    assert_closed(&text);
    assert_close(bounds.max.z, 48.0, 1e-3);
    assert_close(bounds.min.z, 46.5, 1e-3);
    assert!(
        centre.abs_diff_eq(Vec2::new(10.0, -5.0), 1e-3),
        "centre: {centre:?}"
    );
}

#[test]
fn text_wider_than_the_object_is_rejected() {
    assert!(emboss::text_solid(&Primitive::Cube.build(), &emboss("WWWWWWWW")).is_err());
}

#[test]
fn non_positive_size_or_depth_is_rejected() {
    let cube = Primitive::Cube.build();
    for (size, depth) in [(0.0, 1.0), (-1.0, 1.0), (10.0, 0.0), (10.0, -1.0)] {
        let edit = Emboss {
            size,
            depth,
            ..Emboss::default()
        };
        assert!(
            emboss::text_solid(&cube, &edit).is_err(),
            "accepted size {size}, depth {depth}"
        );
    }
}

#[test]
fn unsupported_characters_are_skipped_and_empty_output_is_rejected() {
    let cube = Primitive::Cube.build();
    let text = emboss::text_solid(&cube, &emboss("😀A")).unwrap();

    assert_closed(&text);
    let plain = emboss::text_solid(&cube, &emboss("A")).unwrap();
    assert_eq!(text.aabb().min, plain.aabb().min);
    assert_eq!(text.aabb().max, plain.aabb().max);
    assert!(emboss::text_solid(&cube, &emboss("😀")).is_err());
}

#[test]
fn text_on_a_pyramid_follows_its_sloped_surface() {
    let edit = Emboss {
        text: "I".into(),
        size: 6.0,
        offset: Vec2::new(8.0, 0.0),
        ..Emboss::default()
    };
    let text = emboss::text_solid(&Primitive::Pyramid.build(), &edit).unwrap();
    let bounds = text.aabb();
    let expected_top = 40.0 - 2.0 * bounds.min.x.abs() + edit.depth;

    assert_closed(&text);
    assert!(bounds.max.z < 41.0, "{bounds:?}");
    assert_close(bounds.max.z, expected_top, 0.05);
    assert!(
        bounds.max.z - bounds.min.z > edit.depth + 0.5 + 0.5,
        "{bounds:?}"
    );
}
