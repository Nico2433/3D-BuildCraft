use buildcraft3d::io::{ply, ModelObject};
use buildcraft3d::mesh::Mesh;
use buildcraft3d::primitives;
use glam::{Mat4, Vec3};
use std::path::PathBuf;

fn file(dir: &tempfile::TempDir, text: &str) -> PathBuf {
    let p = dir.path().join("m.ply");
    std::fs::write(&p, text).unwrap();
    p
}

const HEADER_TRI: &str = "ply\nformat ascii 1.0\nelement vertex 3\nproperty float x\nproperty float y\nproperty float z\nelement face 1\nproperty list uchar int vertex_indices\nend_header\n";

#[test]
fn ascii_ply_reads_positions_and_faces() {
    let dir = tempfile::tempdir().unwrap();
    let p = file(&dir, &format!("{HEADER_TRI}0 0 0\n2 0 0\n0 3 1\n3 0 1 2\n"));
    let o = ply::read(&p, "tri").unwrap();
    assert_eq!(o.name, "tri");
    assert_eq!(
        o.mesh.positions,
        vec![
            Vec3::ZERO,
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(0.0, 3.0, 1.0)
        ]
    );
    assert_eq!(o.mesh.triangles, vec![[0, 1, 2]]);
}

#[test]
fn ply_quad_becomes_two_triangles_covering_the_same_area() {
    let dir = tempfile::tempdir().unwrap();
    let text = "ply\nformat ascii 1.0\nelement vertex 4\nproperty float x\nproperty float y\nproperty float z\nelement face 1\nproperty list uchar int vertex_indices\nend_header\n0 0 0\n2 0 0\n2 3 0\n0 3 0\n4 0 1 2 3\n";
    let o = ply::read(&file(&dir, text), "quad").unwrap();
    assert_eq!(o.mesh.triangles.len(), 2);
    let s = o.mesh.stats();
    assert!((s.area - 6.0).abs() < 1e-4, "{s:?}");
    // Both halves face the same way.
    let n: Vec<Vec3> = o
        .mesh
        .triangle_positions()
        .map(|[a, b, c]| Mesh::face_normal(a, b, c))
        .collect();
    assert!(n[0].dot(n[1]) > 0.99);
}

#[test]
fn ply_vertex_colours_become_rgba_with_opaque_alpha() {
    let dir = tempfile::tempdir().unwrap();
    let text = "ply\nformat ascii 1.0\nelement vertex 3\nproperty float x\nproperty float y\nproperty float z\nproperty uchar red\nproperty uchar green\nproperty uchar blue\nelement face 1\nproperty list uchar int vertex_indices\nend_header\n0 0 0 10 20 30\n1 0 0 10 20 30\n0 1 0 10 20 30\n3 0 1 2\n";
    let o = ply::read(&file(&dir, text), "c").unwrap();
    assert_eq!(o.color, [10, 20, 30, 255]);
}

#[test]
fn ply_without_colours_gets_the_default_colour() {
    let dir = tempfile::tempdir().unwrap();
    let p = file(&dir, &format!("{HEADER_TRI}0 0 0\n1 0 0\n0 1 0\n3 0 1 2\n"));
    assert_eq!(
        ply::read(&p, "n").unwrap().color,
        buildcraft3d::scene::DEFAULT_COLOR
    );
}

#[test]
fn truncated_ply_is_an_error_not_a_panic() {
    let dir = tempfile::tempdir().unwrap();
    let full = format!("{HEADER_TRI}0 0 0\n1 0 0\n0 1 0\n3 0 1 2\n");
    let cases = [
        // Cut inside the header.
        "ply\nformat ascii 1.0\nelement vertex 3\nproperty fl",
        // Cut inside the vertex data.
        &format!("{HEADER_TRI}0 0 0\n1 0 0\n0 1"),
        // Cut inside the face data.
        &format!("{HEADER_TRI}0 0 0\n1 0 0\n0 1 0\n3 0 1"),
        // Header only.
        HEADER_TRI,
    ];
    for (i, c) in cases.iter().enumerate() {
        let p = file(&dir, c);
        assert!(ply::read(&p, "t").is_err(), "case {i} should fail");
    }
    // Sanity: the untruncated file reads.
    assert!(ply::read(&file(&dir, &full), "t").is_ok());
}

#[test]
fn malformed_ply_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let bad_type = HEADER_TRI.replace("property float y", "property banana y");
    let cases = [
        // Unknown property type.
        format!("{bad_type}0 0 0\n1 0 0\n0 1 0\n3 0 1 2\n"),
        // Face index out of range.
        format!("{HEADER_TRI}0 0 0\n1 0 0\n0 1 0\n3 0 1 7\n"),
        // Non-numeric vertex value.
        format!("{HEADER_TRI}0 0 0\n1 x 0\n0 1 0\n3 0 1 2\n"),
        // Unsupported encoding.
        HEADER_TRI.replace("ascii", "binary_big_endian"),
    ];
    for (i, c) in cases.iter().enumerate() {
        assert!(
            ply::read(&file(&dir, c), "t").is_err(),
            "case {i} should fail"
        );
    }
}

#[test]
fn ply_write_then_read_keeps_geometry_and_colour_of_baked_objects() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("w.ply");
    let objs = vec![
        ModelObject {
            name: "a".into(),
            mesh: primitives::cube(10.0),
            transform: Mat4::from_translation(Vec3::new(100.0, 0.0, 0.0)),
            color: [200, 30, 90, 255],
        },
        ModelObject {
            name: "b".into(),
            mesh: primitives::cube(10.0),
            transform: Mat4::from_scale(Vec3::splat(2.0)),
            color: [1, 2, 3, 255],
        },
    ];
    ply::write(&p, &objs).unwrap();
    let back = ply::read(&p, "w").unwrap();
    // Face indices of the second object continue after the first object's vertices.
    let s = back.mesh.stats();
    assert!(s.is_closed_manifold(), "{s:?}");
    assert_eq!(s.components, 2);
    assert!((s.volume - (1000.0 + 8000.0)).abs() < 1e-2);
    let b = back.mesh.aabb();
    // First cube moved to x 95..105, second scaled to -10..10.
    assert!((b.min.x + 10.0).abs() < 1e-4 && (b.max.x - 105.0).abs() < 1e-4);
    // The reader keeps the first colour it sees.
    assert_eq!(back.color, [200, 30, 90, 255]);
}
