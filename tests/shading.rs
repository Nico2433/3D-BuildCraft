use buildcraft3d::mesh::{Mesh, Shading};
use buildcraft3d::primitives;
use glam::{Mat4, Vec3};
use std::collections::BTreeSet;

const CYLINDER_SEGMENTS: usize = 120;

fn assert_shading_preserves_mesh(mesh: &Mesh, shading: &Shading) {
    assert_eq!(shading.triangles.len(), mesh.triangles.len());
    assert_eq!(shading.normals.len(), shading.positions.len());
    for (source, drawn) in mesh.triangles.iter().zip(&shading.triangles) {
        for (&source_index, &drawn_index) in source.iter().zip(drawn) {
            assert_eq!(
                shading.positions[drawn_index as usize],
                mesh.positions[source_index as usize]
            );
        }
    }
    assert!(shading
        .edges
        .iter()
        .flatten()
        .all(|&index| (index as usize) < shading.positions.len()));
}

fn assert_unit_or_degenerate_normals(mesh: &Mesh, shading: &Shading) {
    for (index, normal) in shading.normals.iter().enumerate() {
        let length = normal.length();
        assert!(normal.is_finite(), "normal {index} is {normal:?}");
        if length <= 1e-6 {
            let only_on_degenerate_faces = shading
                .triangles
                .iter()
                .enumerate()
                .filter(|(_, triangle)| triangle.contains(&(index as u32)))
                .all(|(face, _)| {
                    let [a, b, c] =
                        mesh.triangles[face].map(|vertex| mesh.positions[vertex as usize]);
                    (b - a).cross(c - a).length_squared() <= f32::EPSILON
                });
            assert!(
                only_on_degenerate_faces,
                "zero normal {index} is used by a non-degenerate face"
            );
        } else {
            assert!((length - 1.0).abs() <= 1e-4, "normal {index}: {normal:?}");
        }
    }
}

#[test]
fn shading_preserves_triangle_geometry_and_has_valid_indices() {
    let meshes = [
        primitives::cube(40.0),
        primitives::Primitive::Cylinder.build(),
        primitives::Primitive::Sphere.build(),
    ];
    for mesh in &meshes {
        for angle in [0.0, 40.0, 180.0] {
            assert_shading_preserves_mesh(mesh, &mesh.shading(angle));
        }
    }
}

#[test]
fn shading_normals_are_unit_length() {
    let meshes = [
        primitives::cube(40.0),
        primitives::Primitive::Cylinder.build(),
        primitives::Primitive::Sphere.build(),
    ];
    for mesh in &meshes {
        for angle in [40.0, 180.0] {
            assert_unit_or_degenerate_normals(mesh, &mesh.shading(angle));
        }
    }
}

#[test]
fn zero_degree_shading_normals_are_unit_length() {
    for mesh in [
        primitives::cube(40.0),
        primitives::Primitive::Cylinder.build(),
        primitives::Primitive::Sphere.build(),
    ] {
        assert_unit_or_degenerate_normals(&mesh, &mesh.shading(0.0));
    }
}

#[test]
fn cylinder_at_forty_degrees_splits_caps_from_smooth_sides() {
    let mesh = primitives::Primitive::Cylinder.build();
    let shading = mesh.shading(40.0);
    assert_eq!(shading.edges.len(), 2 * CYLINDER_SEGMENTS);
    assert_eq!(shading.positions.len(), 4 * CYLINDER_SEGMENTS);

    for edge in &shading.edges {
        let [a, b] = edge.map(|index| shading.positions[index as usize]);
        assert!((a.z - b.z).abs() < 1e-5, "non-rim edge: {a:?} to {b:?}");
        assert!(a.z.abs() < 1e-5 || (a.z - 40.0).abs() < 1e-5);
    }

    for original in 0..mesh.positions.len() as u32 {
        let mut side = BTreeSet::new();
        let mut cap = BTreeSet::new();
        for (face, source) in mesh.triangles.iter().enumerate() {
            for (corner, &vertex) in source.iter().enumerate() {
                if vertex == original {
                    let drawn = shading.triangles[face][corner];
                    if face < 2 * CYLINDER_SEGMENTS {
                        side.insert(drawn);
                    } else {
                        cap.insert(drawn);
                    }
                }
            }
        }
        assert_eq!(side.len(), 1, "side vertex {original} was not shared");
        assert_eq!(cap.len(), 1, "cap vertex {original} was not shared");
        assert_ne!(side, cap, "rim vertex {original} was not split");

        let side_normal = shading.normals[*side.first().unwrap() as usize];
        let cap_normal = shading.normals[*cap.first().unwrap() as usize];
        assert!(side_normal.z.abs() < 1e-4, "{side_normal:?}");
        assert!(cap_normal.x.abs() < 1e-4 && cap_normal.y.abs() < 1e-4);
        assert!((cap_normal.z.abs() - 1.0).abs() < 1e-4);
    }
}

#[test]
fn crease_extremes_split_face_planes_or_share_original_vertices() {
    let cube = primitives::cube(40.0);
    let cylinder = primitives::Primitive::Cylinder.build();

    assert_eq!(cube.shading(0.0).positions.len(), 24);

    for mesh in [&cube, &cylinder] {
        let smooth = mesh.shading(180.0);
        assert_eq!(smooth.positions.len(), mesh.positions.len());
        assert!(smooth.edges.is_empty());
    }
}

#[test]
fn zero_degree_cylinder_splits_distinct_side_planes() {
    let cylinder = primitives::Primitive::Cylinder.build();
    let cylinder_flat = cylinder.shading(0.0);
    for original in 0..cylinder.positions.len() as u32 {
        let side_indices: BTreeSet<u32> = cylinder
            .triangles
            .iter()
            .zip(&cylinder_flat.triangles)
            .take(2 * CYLINDER_SEGMENTS)
            .flat_map(|(source, drawn)| source.iter().zip(drawn))
            .filter(|(source, _)| **source == original)
            .map(|(_, drawn)| *drawn)
            .collect();
        assert_eq!(
            side_indices.len(),
            2,
            "adjacent side quads shared vertex {original}"
        );
    }
}

#[test]
fn open_triangle_has_face_normals_and_no_drawn_edges() {
    let mesh = Mesh::new(vec![Vec3::ZERO, Vec3::X, Vec3::Y], vec![[0, 1, 2]]);
    for angle in [0.0, 40.0, 180.0] {
        let shading = mesh.shading(angle);
        assert!(shading.edges.is_empty());
        assert_eq!(shading.normals, vec![Vec3::Z; 3]);
    }
}

#[test]
fn coplanar_triangle_fan_is_fully_shared_and_smooth() {
    let mesh = Mesh::new(
        vec![
            Vec3::ZERO,
            Vec3::new(-1.0, -1.0, 0.0),
            Vec3::new(1.0, -1.0, 0.0),
            Vec3::new(1.0, 1.0, 0.0),
            Vec3::new(-1.0, 1.0, 0.0),
        ],
        vec![[0, 1, 2], [0, 2, 3], [0, 3, 4], [0, 4, 1]],
    );
    for angle in [0.0, 40.0, 180.0] {
        let shading = mesh.shading(angle);
        assert_eq!(shading.positions.len(), mesh.positions.len());
        assert!(shading.normals.iter().all(|&normal| normal == Vec3::Z));
        assert!(shading.edges.is_empty());
    }
}

#[test]
fn thin_boxes_keep_all_geometric_creases() {
    for scale in [Vec3::new(1.0, 1.0, 1e-3), Vec3::new(1e-3, 1.0, 1.0)] {
        let mesh = primitives::cube(40.0).transformed(&Mat4::from_scale(scale));
        let shading = mesh.shading(40.0);
        assert_eq!(shading.edges.len(), 12);
        assert_eq!(shading.positions.len(), 24);
    }
}

#[test]
fn degenerate_triangles_produce_only_finite_unit_or_zero_vectors() {
    let meshes = [
        Mesh::new(vec![Vec3::ZERO, Vec3::X, Vec3::X * 2.0], vec![[0, 1, 2]]),
        Mesh::new(
            vec![Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::X * 2.0],
            vec![[0, 1, 2], [0, 1, 3], [2, 2, 2]],
        ),
    ];
    for mesh in &meshes {
        for angle in [0.0, 40.0, 180.0] {
            let shading = mesh.shading(angle);
            assert!(shading
                .positions
                .iter()
                .all(|position| position.is_finite()));
            assert_unit_or_degenerate_normals(mesh, &shading);
        }
    }
}
