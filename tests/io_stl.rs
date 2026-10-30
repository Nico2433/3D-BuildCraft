use buildcraft3d::io::{stl, ModelObject};
use buildcraft3d::primitives;
use glam::{Mat4, Vec3};

fn binary(header: &[u8; 80], tris: &[[Vec3; 3]]) -> Vec<u8> {
    let mut v = header.to_vec();
    v.extend((tris.len() as u32).to_le_bytes());
    for t in tris {
        v.extend([0u8; 12]);
        for p in t {
            for x in p.to_array() {
                v.extend(x.to_le_bytes());
            }
        }
        v.extend([0u8; 2]);
    }
    v
}

fn two_tris() -> Vec<[Vec3; 3]> {
    vec![
        [Vec3::ZERO, Vec3::X, Vec3::Y],
        [Vec3::X, Vec3::new(1.0, 1.0, 0.0), Vec3::Y],
    ]
}

#[test]
fn binary_stl_whose_header_starts_with_solid_is_still_binary() {
    let mut header = [0u8; 80];
    header[..11].copy_from_slice(b"solid model");
    let m = stl::parse(&binary(&header, &two_tris())).unwrap();
    assert_eq!(m.triangles.len(), 2);
    assert_eq!(m.positions.len(), 4, "shared vertices are merged");
    assert!(m.positions.contains(&Vec3::new(1.0, 1.0, 0.0)));
}

#[test]
fn ascii_stl_with_vertex_count_not_a_multiple_of_three_is_an_error() {
    let s = "solid t\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nendloop\nendfacet\nendsolid t\n";
    assert!(stl::parse(s.as_bytes()).is_err());
}

#[test]
fn ascii_stl_with_a_bad_vertex_is_an_error() {
    let s = "solid t\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 zero 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid t\n";
    assert!(stl::parse(s.as_bytes()).is_err());
    let short = "solid t\nvertex 0 0\nendsolid t\n";
    assert!(stl::parse(short.as_bytes()).is_err());
}

#[test]
fn truncated_binary_stl_is_an_error() {
    let mut data = binary(&[0xFF; 80], &two_tris());
    data.truncate(data.len() - 10);
    assert!(stl::parse(&data).is_err());
}

#[test]
fn stl_write_has_a_zero_header_and_a_little_endian_count_and_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("c.stl");
    let obj = ModelObject {
        name: "c".into(),
        mesh: primitives::cube(10.0),
        transform: Mat4::from_translation(Vec3::new(50.0, 0.0, 0.0)),
        color: [1, 2, 3, 255],
    };
    stl::write(&p, std::slice::from_ref(&obj)).unwrap();
    let data = std::fs::read(&p).unwrap();
    assert!(data[..80].iter().all(|b| *b == 0));
    assert_eq!(u32::from_le_bytes(data[80..84].try_into().unwrap()), 12);
    assert_eq!(data.len(), 84 + 12 * 50);

    let back = stl::read(&p, "c").unwrap();
    assert_eq!(back.name, "c");
    let s = back.mesh.stats();
    assert!(s.is_closed_manifold(), "{s:?}");
    assert!((s.volume - 1000.0).abs() < 1e-3);
    // The object's transform is baked into the file.
    assert!((back.mesh.aabb().min.x - 45.0).abs() < 1e-4);
}

#[test]
fn stl_write_concatenates_several_objects() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("two.stl");
    let o = |x: f32| ModelObject {
        name: "o".into(),
        mesh: primitives::cube(10.0),
        transform: Mat4::from_translation(Vec3::new(x, 0.0, 0.0)),
        color: [0, 0, 0, 255],
    };
    stl::write(&p, &[o(0.0), o(30.0)]).unwrap();
    let back = stl::read(&p, "two").unwrap().mesh.stats();
    assert_eq!(back.triangles, 24);
    assert_eq!(back.components, 2);
}
