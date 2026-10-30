use buildcraft3d::mesh::{ray_aabb, ray_triangle, Aabb, Mesh};
use buildcraft3d::primitives;
use glam::{Mat4, Vec3};

fn tri(i: [u32; 3]) -> [u32; 3] {
    i
}

#[test]
fn single_triangle_is_open() {
    let m = Mesh::new(vec![Vec3::ZERO, Vec3::X, Vec3::Y], vec![tri([0, 1, 2])]);
    let s = m.stats();
    assert_eq!(s.boundary_edges, 3);
    assert!(!s.is_closed_manifold());
}

#[test]
fn three_triangles_on_one_edge_are_non_manifold() {
    let m = Mesh::new(
        vec![Vec3::ZERO, Vec3::X, Vec3::Y, -Vec3::Y, Vec3::Z],
        vec![[0, 1, 2], [1, 0, 3], [0, 1, 4]],
    );
    let s = m.stats();
    assert_eq!(s.nonmanifold_edges, 1);
    assert!(!s.is_closed_manifold());
}

#[test]
fn two_triangles_running_a_shared_edge_the_same_way_are_misoriented() {
    let pos = vec![Vec3::ZERO, Vec3::X, Vec3::Y, -Vec3::Y];
    let bad = Mesh::new(pos.clone(), vec![[0, 1, 2], [0, 1, 3]]);
    assert_eq!(bad.stats().misoriented_edges, 1);
    let good = Mesh::new(pos, vec![[0, 1, 2], [1, 0, 3]]);
    assert_eq!(good.stats().misoriented_edges, 0);
}

#[test]
fn separate_triangles_are_separate_components() {
    let m = Mesh::new(
        vec![
            Vec3::ZERO,
            Vec3::X,
            Vec3::Y,
            Vec3::new(10.0, 0.0, 0.0),
            Vec3::new(11.0, 0.0, 0.0),
            Vec3::new(10.0, 1.0, 0.0),
        ],
        vec![[0, 1, 2], [3, 4, 5]],
    );
    assert_eq!(m.stats().components, 2);
    assert_eq!(primitives::cube(1.0).stats().components, 1);
}

#[test]
fn degenerate_triangles_are_counted() {
    let m = Mesh::new(
        vec![Vec3::ZERO, Vec3::X, Vec3::Y],
        vec![[0, 1, 2], [0, 0, 1]],
    );
    assert_eq!(m.stats().degenerate_triangles, 1);
}

#[test]
fn area_and_volume_of_a_known_solid() {
    let s = primitives::box_mesh(Vec3::new(2.0, 3.0, 4.0)).stats();
    assert!((s.area - 2.0 * (6.0 + 8.0 + 12.0)).abs() < 1e-9);
    assert!((s.volume - 24.0).abs() < 1e-9);
}

#[test]
fn welded_drops_triangles_with_repeated_indices_and_keeps_the_rest() {
    let m = Mesh::new(
        vec![Vec3::ZERO, Vec3::X, Vec3::Y],
        vec![[0, 1, 2], [0, 0, 1], [2, 2, 2]],
    );
    let w = m.welded();
    assert_eq!(w.triangles.len(), 1);
    assert_eq!(
        w.triangle_positions().next().unwrap(),
        [Vec3::ZERO, Vec3::X, Vec3::Y]
    );
}

#[test]
fn welded_drops_triangles_that_collapse_onto_one_point_after_merging() {
    // Vertices 3 and 4 sit exactly on vertex 0, so [0,3,4] has three distinct indices
    // but a single position.
    let m = Mesh::new(
        vec![Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::ZERO, Vec3::ZERO],
        vec![[0, 1, 2], [0, 3, 4], [3, 1, 2]],
    );
    let w = m.welded();
    assert_eq!(w.triangles.len(), 2);
    assert_eq!(w.positions.len(), 3, "coincident vertices merged");
    for t in w.triangle_positions() {
        assert_eq!(t, [Vec3::ZERO, Vec3::X, Vec3::Y]);
    }
}

#[test]
fn welded_merges_a_cube_split_into_loose_triangles() {
    let c = primitives::cube(10.0);
    let soup: Vec<[Vec3; 3]> = c.triangle_positions().collect();
    let unwelded = Mesh::new(
        soup.iter().flatten().copied().collect(),
        (0..soup.len() as u32)
            .map(|i| [3 * i, 3 * i + 1, 3 * i + 2])
            .collect(),
    );
    assert!(!unwelded.stats().is_closed_manifold());
    let w = unwelded.welded();
    assert_eq!(w.positions.len(), 8);
    assert!(w.stats().is_closed_manifold());
}

#[test]
fn ray_triangle_hit_returns_the_distance() {
    let (a, b, c) = (Vec3::ZERO, Vec3::X, Vec3::Y);
    let t = ray_triangle(Vec3::new(0.25, 0.25, 5.0), -Vec3::Z, a, b, c).unwrap();
    assert!((t - 5.0).abs() < 1e-5);
    // The distance scales with the direction length.
    let t = ray_triangle(
        Vec3::new(0.25, 0.25, 5.0),
        Vec3::new(0.0, 0.0, -2.0),
        a,
        b,
        c,
    )
    .unwrap();
    assert!((t - 2.5).abs() < 1e-5);
}

#[test]
fn ray_triangle_misses() {
    let (a, b, c) = (Vec3::ZERO, Vec3::X, Vec3::Y);
    // Outside the triangle.
    assert!(ray_triangle(Vec3::new(2.0, 2.0, 5.0), -Vec3::Z, a, b, c).is_none());
    assert!(ray_triangle(Vec3::new(0.8, 0.8, 5.0), -Vec3::Z, a, b, c).is_none());
    // Triangle behind the origin.
    assert!(ray_triangle(Vec3::new(0.25, 0.25, -5.0), -Vec3::Z, a, b, c).is_none());
    // Parallel to the plane.
    assert!(ray_triangle(Vec3::new(0.25, 0.25, 1.0), Vec3::X, a, b, c).is_none());
}

fn unit_box() -> Aabb {
    Aabb {
        min: Vec3::ZERO,
        max: Vec3::ONE,
    }
}

#[test]
fn ray_aabb_hits_and_misses() {
    let b = unit_box();
    assert!(ray_aabb(Vec3::new(-5.0, 0.5, 0.5), Vec3::X, &b));
    assert!(ray_aabb(
        Vec3::new(-5.0, -5.0, -5.0),
        Vec3::ONE.normalize(),
        &b
    ));
    assert!(
        !ray_aabb(Vec3::new(-5.0, 5.0, 0.5), Vec3::X, &b),
        "passes beside"
    );
    assert!(
        !ray_aabb(Vec3::new(5.0, 0.5, 0.5), Vec3::X, &b),
        "box behind origin"
    );
    assert!(ray_aabb(Vec3::splat(0.5), Vec3::X, &b), "origin inside");
}

#[test]
fn ray_aabb_grazing_a_face_still_hits_the_closed_box() {
    // The ray runs exactly along the y = 0 face of the box.
    assert!(ray_aabb(Vec3::new(-5.0, 0.0, 0.5), Vec3::X, &unit_box()));
}

#[test]
fn mirroring_keeps_volume_positive_on_asymmetric_solids() {
    let solids = [
        primitives::wedge(10.0, 20.0, 30.0),
        primitives::pyramid(10.0, 25.0),
        primitives::tetrahedron(10.0),
    ];
    let mirrors = [
        Mat4::from_scale(Vec3::new(-1.0, 1.0, 1.0)),
        Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0)),
        Mat4::from_scale(Vec3::new(1.0, 1.0, -1.0)),
        Mat4::from_scale(Vec3::new(2.0, -3.0, 1.0)),
        Mat4::from_scale(Vec3::new(-1.0, -1.0, -1.0)),
    ];
    for (i, m) in solids.iter().enumerate() {
        let v0 = m.stats().volume;
        assert!(v0 > 0.0);
        for t in &mirrors {
            let s = m.transformed(t).stats();
            let expect = v0 * t.determinant().abs() as f64;
            assert!(s.is_closed_manifold(), "solid {i} {t:?}: {s:?}");
            assert!(
                (s.volume - expect).abs() / expect < 1e-4,
                "solid {i} {t:?}: {} vs {expect}",
                s.volume
            );
        }
    }
}

#[test]
fn flip_reverses_the_sign_of_the_volume() {
    let mut m = primitives::pyramid(10.0, 10.0);
    let v = m.stats().volume;
    m.flip();
    assert!((m.stats().volume + v).abs() < 1e-9);
}

#[test]
fn append_offsets_the_indices_of_the_second_mesh() {
    let mut a = primitives::cube(10.0);
    a.append(
        &primitives::cube(10.0).transformed(&Mat4::from_translation(Vec3::new(50.0, 0.0, 0.0))),
    );
    let s = a.stats();
    assert_eq!(s.components, 2);
    assert!(s.is_closed_manifold());
    assert!((s.volume - 2000.0).abs() < 1e-6);
}

#[test]
fn aabb_helpers() {
    assert!(!Aabb::empty().is_valid());
    assert_eq!(Aabb::empty().size(), Vec3::ZERO);
    let mut b = Aabb::empty();
    b.grow(Vec3::new(1.0, 2.0, 3.0));
    b.grow(Vec3::new(-1.0, 0.0, 5.0));
    assert_eq!(b.size(), Vec3::new(2.0, 2.0, 2.0));
    assert_eq!(b.center(), Vec3::new(0.0, 1.0, 4.0));
    let u = b.union(Aabb::empty());
    assert_eq!(u, b);
    assert!(Mesh::default().aabb().size() == Vec3::ZERO);
}

#[test]
fn every_primitive_is_a_closed_positive_solid() {
    for p in primitives::Primitive::ALL {
        let s = p.build().stats();
        assert!(s.is_closed_manifold(), "{p:?}: {s:?}");
        assert!(s.volume > 0.0, "{p:?}");
    }
}
