//! Emboss on any face of an object, and conformed to curved surfaces.
use buildcraft3d::{
    emboss::{self, Emboss},
    mesh::Mesh,
    primitives::Primitive,
};
use glam::{Mat4, Vec3};

fn assert_closed(mesh: &Mesh) {
    let stats = mesh.stats();
    assert!(stats.is_closed_manifold(), "{stats:?}");
}

fn text(s: &str, size: f32) -> Emboss {
    Emboss {
        text: s.into(),
        size,
        ..Emboss::default()
    }
}

/// How far `p` is from the surface of `mesh`.
fn distance(mesh: &Mesh, p: Vec3) -> f32 {
    mesh.closest_triangle(&Mat4::IDENTITY, p)
        .unwrap()
        .1
        .distance(p)
}

#[test]
fn text_embosses_onto_every_face_of_a_cube() {
    let cube = Primitive::Cube.build();
    let b = cube.aabb();
    let (centre, half) = (b.center(), (b.max - b.min) / 2.0);
    let volume = cube.stats().volume;
    for n in [Vec3::X, -Vec3::X, Vec3::Y, -Vec3::Y, Vec3::Z, -Vec3::Z] {
        // A point off the middle of the face, to check the pattern goes where it was put.
        let along = if n.z == 0.0 { Vec3::Z } else { Vec3::X };
        let point = centre + n * half.dot(n.abs()) + along * 5.0;
        for engrave in [false, true] {
            let mut edit = Emboss {
                engrave,
                ..text("Ab", 8.0)
            };
            emboss::place_at(&cube, &mut edit, point);
            assert!(edit.facing.abs_diff_eq(n, 1e-5), "{n}: {:?}", edit.facing);
            let solid = emboss::text_solid(&cube, &edit).unwrap_or_else(|e| panic!("{n}: {e}"));
            assert_closed(&solid);
            // Raised 1 mm out of the face, or cut 1 mm into it.
            let reach = solid
                .positions
                .iter()
                .map(|p| (*p - centre).dot(n))
                .fold(f32::MIN, f32::max);
            let face = half.dot(n.abs());
            let expected = if engrave { face + 0.5 } else { face + 1.0 };
            assert!((reach - expected).abs() < 1e-3, "{n}: {reach}");
            let middle = solid.aabb().center();
            let off = middle - point;
            assert!(
                (off - n * off.dot(n)).length() < 1.5,
                "{n}: {middle} vs {point}"
            );
            let out = emboss::apply(&cube, &edit).unwrap().object;
            assert_closed(&out);
            let v = out.stats().volume;
            assert!(if engrave { v < volume } else { v > volume }, "{n}: {v}");
        }
    }
}

#[test]
fn text_embosses_onto_a_slanted_face() {
    let pyramid = Primitive::Pyramid.build();
    // The middle of the largest slanted face.
    let (tri, n) = pyramid
        .triangles
        .iter()
        .map(|t| t.map(|v| pyramid.positions[v as usize]))
        .map(|[a, b, c]| ([a, b, c], (b - a).cross(c - a)))
        .filter(|(_, n)| n.z > 1e-3 && n.normalize().z < 0.99)
        .max_by(|x, y| x.1.length().total_cmp(&y.1.length()))
        .unwrap();
    let n = n.normalize();
    let point = (tri[0] + tri[1] + tri[2]) / 3.0;
    let mut edit = text("A", 5.0);
    emboss::place_at(&pyramid, &mut edit, point);
    assert!(edit.facing.abs_diff_eq(n, 1e-5));
    let solid = emboss::text_solid(&pyramid, &edit).unwrap();
    assert_closed(&solid);
    // The letter's top is 1 mm out of the slanted face, its foot 0.5 mm into it.
    for p in &solid.positions {
        let h = (*p - point).dot(n);
        assert!((h - 1.0).abs() < 1e-3 || (h + 0.5).abs() < 1e-3, "{h}");
    }
    assert_closed(&emboss::apply(&pyramid, &edit).unwrap().object);
}

/// Every vertex of a conformed pattern lies `depth` from the surface (the top of a raised
/// pattern, the floor of an engraved one) or the overlap of 0.5 mm on the other side.
fn assert_even(target: &Mesh, edit: &Emboss) {
    let solid = emboss::text_solid(target, edit).unwrap();
    assert_closed(&solid);
    let mut far = 0;
    for p in solid.positions.iter().step_by(3) {
        let d = distance(target, *p);
        let off = (d - edit.depth).abs().min((d - 0.5).abs());
        assert!(off < 0.08, "{p}: {d}");
        far += usize::from((d - edit.depth).abs() < 0.08);
    }
    assert!(far > 0);
    for result in [false, true].map(|engrave| {
        emboss::apply(
            target,
            &Emboss {
                engrave,
                ..edit.clone()
            },
        )
    }) {
        assert_closed(&result.unwrap().object);
    }
}

#[test]
fn conformed_text_keeps_an_even_height_on_a_sphere() {
    let sphere = Primitive::Sphere.build();
    for engrave in [false, true] {
        let edit = Emboss {
            conform: true,
            depth: 2.0,
            engrave,
            ..text("O", 20.0)
        };
        assert_even(&sphere, &edit);
    }
    // Without conforming, the letter rises straight up, so its top leans in where the
    // sphere falls away.
    let straight = emboss::text_solid(
        &sphere,
        &Emboss {
            depth: 2.0,
            ..text("O", 20.0)
        },
    )
    .unwrap();
    let low = straight
        .positions
        .iter()
        .map(|p| distance(&sphere, *p))
        .filter(|d| *d > 1.0)
        .fold(f32::MAX, f32::min);
    assert!(low < 1.9, "{low}");
}

#[test]
fn conformed_text_keeps_an_even_height_on_a_cylinder_side() {
    let cylinder = Primitive::Cylinder.build();
    for engrave in [false, true] {
        let mut edit = Emboss {
            conform: true,
            depth: 2.0,
            engrave,
            ..text("OI", 15.0)
        };
        emboss::place_at(&cylinder, &mut edit, Vec3::new(0.0, -20.0, 20.0));
        assert!(edit.facing.y < -0.99);
        assert_even(&cylinder, &edit);
    }
}
