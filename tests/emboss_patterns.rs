use buildcraft3d::{
    emboss::{self, Emboss, Pattern, Projection, Shape},
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

fn small_text() -> Emboss {
    Emboss {
        text: "I".into(),
        size: 8.0,
        ..Emboss::default()
    }
}

fn centroid(mesh: &Mesh) -> Vec3 {
    mesh.positions.iter().copied().sum::<Vec3>() / mesh.positions.len() as f32
}

#[test]
fn offset_at_round_trips_a_planar_point() {
    let cube = Primitive::Cube.build();
    let point = Vec3::new(8.0, -5.0, 40.0);
    let edit = Emboss {
        offset: emboss::offset_at(&cube, Projection::Planar, point),
        ..small_text()
    };
    let centre = emboss::text_solid(&cube, &edit).unwrap().aabb().center();

    assert_close(centre.x, point.x, 0.05);
    assert_close(centre.y, point.y, 0.05);
}

#[test]
fn offset_at_round_trips_a_cylindrical_point() {
    let cylinder = Primitive::Cylinder.build();
    let azimuth = -60.0_f32.to_radians();
    let point = Vec3::new(20.0 * azimuth.cos(), 20.0 * azimuth.sin(), 20.0);
    let edit = Emboss {
        projection: Projection::Cylindrical,
        offset: emboss::offset_at(&cylinder, Projection::Cylindrical, point),
        ..small_text()
    };
    let centre = centroid(&emboss::text_solid(&cylinder, &edit).unwrap());

    assert_close(centre.y.atan2(centre.x), azimuth, 3.0_f32.to_radians());
    assert_close(centre.z, 20.0, 1.0);
}

#[test]
fn offset_at_round_trips_a_spherical_point() {
    let sphere = Primitive::Sphere.build();
    let sphere_centre = Vec3::new(0.0, 0.0, 20.0);
    let azimuth = -60.0_f32.to_radians();
    let elevation = 20.0_f32.to_radians();
    let direction = Vec3::new(
        elevation.cos() * azimuth.cos(),
        elevation.cos() * azimuth.sin(),
        elevation.sin(),
    );
    let point = sphere_centre + direction * 20.0;
    let edit = Emboss {
        projection: Projection::Spherical,
        offset: emboss::offset_at(&sphere, Projection::Spherical, point),
        ..small_text()
    };
    let preview_direction =
        (centroid(&emboss::text_solid(&sphere, &edit).unwrap()) - sphere_centre).normalize();

    assert!(
        preview_direction.angle_between(direction) < 4.0_f32.to_radians(),
        "expected {direction:?}, got {preview_direction:?}"
    );
}

#[test]
fn cylindrical_text_embosses_a_z_rotated_cylinder() {
    let cylinder = Primitive::Cylinder
        .build()
        .transformed(&Mat4::from_rotation_z(90.0_f32.to_radians()));
    let edit = Emboss {
        projection: Projection::Cylindrical,
        ..small_text()
    };

    let preview = emboss::text_solid(&cylinder, &edit).unwrap();
    assert_close(preview.aabb().min.y, -21.0, 0.2);
    assert_closed(&emboss::apply(&cylinder, &edit).unwrap().object);
}

#[test]
fn positive_spherical_vertical_offset_moves_text_up() {
    let sphere = Primitive::Sphere.build();
    let at = |y| {
        centroid(
            &emboss::text_solid(
                &sphere,
                &Emboss {
                    projection: Projection::Spherical,
                    offset: Vec2::new(0.0, y),
                    ..small_text()
                },
            )
            .unwrap(),
        )
        .z
    };

    assert!(at(8.0) > at(0.0));
}

#[test]
fn blank_and_missing_images_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let white = dir.path().join("white.png");
    image::GrayImage::from_pixel(8, 8, image::Luma([255]))
        .save(&white)
        .unwrap();
    let cube = Primitive::Cube.build();
    let image_edit = |path| Emboss {
        pattern: Pattern::Image(path),
        size: 8.0,
        ..Emboss::default()
    };

    assert!(emboss::text_solid(&cube, &image_edit(white)).is_err());
    assert!(emboss::text_solid(&cube, &image_edit(dir.path().join("missing.png"))).is_err());
}

#[test]
fn unknown_font_family_is_rejected() {
    let edit = Emboss {
        font: Some("No Such Font Family 12345".into()),
        ..small_text()
    };

    assert!(emboss::text_solid(&Primitive::Cube.build(), &edit).is_err());
}

#[test]
fn font_families_are_case_insensitively_sorted_and_unique() {
    let families = emboss::font_families();

    for pair in families.windows(2) {
        let left = pair[0].to_lowercase();
        let right = pair[1].to_lowercase();
        assert!(left <= right, "font list is not sorted: {pair:?}");
        assert_ne!(left, right, "duplicate font families: {pair:?}");
    }
}

#[test]
fn a_reported_font_can_emboss_latin_text() {
    let families = emboss::font_families();
    if families.is_empty() {
        return;
    }
    let cube = Primitive::Cube.build();

    for family in families.iter().take(10) {
        let edit = Emboss {
            text: "Ab".into(),
            font: Some(family.clone()),
            size: 6.0,
            ..Emboss::default()
        };
        if emboss::text_solid(&cube, &edit).is_err() {
            continue;
        }
        assert_closed(&emboss::apply(&cube, &edit).unwrap().object);
        return;
    }

    panic!("none of the first reported fonts could emboss Latin text");
}

#[test]
fn engraving_ignores_requested_part_colours() {
    let cube = Primitive::Cube.build();
    let edit = Emboss {
        engrave: true,
        top_color: Some([255, 0, 0, 255]),
        side_color: Some([0, 0, 255, 255]),
        ..small_text()
    };
    let output = emboss::apply(&cube, &edit).unwrap();

    assert!(output.parts.is_empty());
    assert_closed(&output.object);
    assert!(output.object.stats().volume < cube.stats().volume);
}

#[test]
fn side_colour_separates_sides_and_leaves_only_the_cap_on_the_object() {
    let cube = Primitive::Cube.build();
    let edit = Emboss {
        side_color: Some([0, 0, 255, 255]),
        ..small_text()
    };
    let output = emboss::apply(&cube, &edit).unwrap();
    let plain = emboss::apply(&cube, &small_text()).unwrap();
    let preview = emboss::text_solid(&cube, &small_text()).unwrap();
    let cube_volume = cube.stats().volume;
    let cap_gain = output.object.stats().volume - cube_volume;
    let plain_gain = plain.object.stats().volume - cube_volume;
    let expected_cap_volume = preview.stats().volume / 6.0;

    assert_eq!(output.parts.len(), 1);
    assert!(output.parts[0].0.aabb().min.z >= 40.0 - 1e-3);
    assert_closed(&output.object);
    assert!(
        cap_gain > 0.0 && cap_gain < plain_gain,
        "{cap_gain} vs {plain_gain}"
    );
    assert!(
        (cap_gain - expected_cap_volume).abs() < expected_cap_volume * 0.05,
        "expected cap volume {expected_cap_volume}, got {cap_gain}"
    );
}

#[test]
fn spherical_star_engraving_removes_volume() {
    let sphere = Primitive::Sphere.build();
    let edit = Emboss {
        pattern: Pattern::Shape(Shape::Star),
        size: 10.0,
        engrave: true,
        projection: Projection::Spherical,
        ..Emboss::default()
    };
    let output = emboss::apply(&sphere, &edit).unwrap().object;

    assert_closed(&output);
    assert!(output.stats().volume < sphere.stats().volume);
}
