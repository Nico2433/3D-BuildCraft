use buildcraft3d::io::{obj, ModelObject};
use buildcraft3d::primitives;
use glam::{Mat4, Vec3};
use std::path::PathBuf;

fn file(dir: &tempfile::TempDir, name: &str, text: &str) -> PathBuf {
    let p = dir.path().join(name);
    std::fs::write(&p, text).unwrap();
    p
}

fn tri_positions(o: &ModelObject) -> Vec<[Vec3; 3]> {
    o.mesh.triangle_positions().collect()
}

#[test]
fn negative_indices_are_relative_to_the_vertices_defined_so_far() {
    let dir = tempfile::tempdir().unwrap();
    let text = "v 0 0 0\nv 1 0 0\nv 0 1 0\nf -3 -2 -1\nv 0 0 5\nv 1 0 5\nv 0 1 5\nf -3 -2 -1\nf -6 -1 -2\n";
    let o = obj::read(&file(&dir, "n.obj", text)).unwrap();
    assert_eq!(o.len(), 1);
    let t = tri_positions(&o[0]);
    assert_eq!(t.len(), 3);
    assert_eq!(t[0], [Vec3::ZERO, Vec3::X, Vec3::Y]);
    let z5 = |x, y| Vec3::new(x, y, 5.0);
    assert_eq!(t[1], [z5(0.0, 0.0), z5(1.0, 0.0), z5(0.0, 1.0)]);
    // -6 is the first vertex, -1 the sixth, -2 the fifth.
    assert_eq!(t[2], [Vec3::ZERO, z5(0.0, 1.0), z5(1.0, 0.0)]);
}

#[test]
fn every_face_syntax_reads_the_position_index() {
    let dir = tempfile::tempdir().unwrap();
    let head = "v 0 0 0\nv 1 0 0\nv 0 1 0\nvt 0 0\nvt 1 0\nvt 0 1\nvn 0 0 1\n";
    for face in [
        "f 1/1/1 2/2/1 3/3/1",
        "f 1//1 2//1 3//1",
        "f 1/1 2/2 3/3",
        "f 1 2 3",
    ] {
        let o = obj::read(&file(&dir, "f.obj", &format!("{head}{face}\n"))).unwrap();
        assert_eq!(
            tri_positions(&o[0]),
            vec![[Vec3::ZERO, Vec3::X, Vec3::Y]],
            "{face}"
        );
    }
}

#[test]
fn quads_are_split_into_two_triangles() {
    let dir = tempfile::tempdir().unwrap();
    let text = "v 0 0 0\nv 2 0 0\nv 2 2 0\nv 0 2 0\nf 1 2 3 4\n";
    let o = obj::read(&file(&dir, "q.obj", text)).unwrap();
    assert_eq!(o[0].mesh.triangles.len(), 2);
    assert!((o[0].mesh.stats().area - 4.0).abs() < 1e-5);
}

#[test]
fn several_o_groups_give_several_objects_with_their_own_triangles() {
    let dir = tempfile::tempdir().unwrap();
    let text = "o first\nv 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\no second\nv 0 0 9\nv 1 0 9\nv 0 1 9\nv 1 1 9\nf 4 5 6\nf 5 7 6\n";
    let o = obj::read(&file(&dir, "g.obj", text)).unwrap();
    assert_eq!(o.len(), 2);
    assert_eq!(
        (o[0].name.as_str(), o[1].name.as_str()),
        ("first", "second")
    );
    assert_eq!(o[0].mesh.triangles.len(), 1);
    assert_eq!(o[1].mesh.triangles.len(), 2);
    // Each object only holds the vertices it uses.
    assert_eq!(o[0].mesh.positions.len(), 3);
    assert_eq!(o[1].mesh.positions.len(), 4);
    assert!(o[1].mesh.positions.iter().all(|p| p.z == 9.0));
}

#[test]
fn faces_without_an_o_line_use_the_file_name() {
    let dir = tempfile::tempdir().unwrap();
    let o = obj::read(&file(
        &dir,
        "thing.obj",
        "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n",
    ))
    .unwrap();
    assert_eq!(o[0].name, "thing");
}

#[test]
fn mtl_kd_becomes_the_object_colour() {
    let dir = tempfile::tempdir().unwrap();
    file(
        &dir,
        "m.mtl",
        "newmtl red\nKd 1.0 0.2 0.0\nnewmtl blue\nKd 0 0 1\n",
    );
    let text = "mtllib m.mtl\no a\nusemtl red\nv 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\no b\nusemtl blue\nf 1 2 3\n";
    let o = obj::read(&file(&dir, "c.obj", text)).unwrap();
    assert_eq!(o[0].color, [255, 51, 0, 255]);
    assert_eq!(o[1].color, [0, 0, 255, 255]);
}

#[test]
fn mtllib_name_may_contain_spaces() {
    let dir = tempfile::tempdir().unwrap();
    file(&dir, "my model.mtl", "newmtl m\nKd 0 1 0\n");
    let text = "mtllib my model.mtl\nusemtl m\nv 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n";
    let o = obj::read(&file(&dir, "s.obj", text)).unwrap();
    assert_eq!(o[0].color, [0, 255, 0, 255]);
}

#[test]
fn missing_mtl_or_unknown_material_falls_back_to_the_default_colour() {
    let dir = tempfile::tempdir().unwrap();
    let text = "mtllib nowhere.mtl\nusemtl ghost\nv 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n";
    let o = obj::read(&file(&dir, "d.obj", text)).unwrap();
    assert_eq!(o[0].color, buildcraft3d::scene::DEFAULT_COLOR);
}

#[test]
fn out_of_range_indices_are_errors() {
    let dir = tempfile::tempdir().unwrap();
    let v = "v 0 0 0\nv 1 0 0\nv 0 1 0\n";
    for face in ["f 1 2 4", "f 1 2 0", "f -4 -2 -1", "f 1 2 x", "f 1 2 -9999"] {
        let r = obj::read(&file(&dir, "e.obj", &format!("{v}{face}\n")));
        assert!(r.is_err(), "{face} should fail");
    }
}

#[test]
fn file_without_faces_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    assert!(obj::read(&file(&dir, "e.obj", "v 0 0 0\nv 1 0 0\no x\n")).is_err());
    assert!(obj::read(&file(&dir, "e2.obj", "")).is_err());
}

#[test]
fn bad_vertex_line_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    assert!(obj::read(&file(&dir, "b.obj", "v 0 0\nf 1 1 1\n")).is_err());
    assert!(obj::read(&file(&dir, "b2.obj", "v 0 a 0\nf 1 1 1\n")).is_err());
}

#[test]
fn obj_write_then_read_keeps_objects_colours_and_world_positions() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("w.obj");
    let objs = vec![
        ModelObject {
            name: "a".into(),
            mesh: primitives::cube(10.0),
            transform: Mat4::from_translation(Vec3::new(40.0, 0.0, 0.0)),
            color: [10, 20, 30, 255],
        },
        ModelObject {
            name: "b".into(),
            mesh: primitives::cube(10.0),
            transform: Mat4::IDENTITY,
            color: [250, 240, 230, 255],
        },
    ];
    obj::write(&p, &objs).unwrap();
    let back = obj::read(&p).unwrap();
    assert_eq!(back.len(), 2);
    assert_eq!(back[0].color, [10, 20, 30, 255]);
    assert_eq!(back[1].color, [250, 240, 230, 255]);
    assert!((back[0].mesh.aabb().min.x - 35.0).abs() < 1e-4);
    assert!(back.iter().all(|o| o.mesh.stats().is_closed_manifold()));
}
