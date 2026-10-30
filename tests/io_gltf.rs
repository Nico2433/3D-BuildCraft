use buildcraft3d::io::gltf_io;
use buildcraft3d::io::ModelObject;
use buildcraft3d::mesh::Aabb;
use buildcraft3d::primitives;
use glam::{Mat4, Vec3};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// One primitive to put in a hand-built GLB.
struct Prim {
    positions: Vec<[f32; 3]>,
    indices: Option<Vec<u32>>,
    mode: u32,
}

fn tris(positions: Vec<[f32; 3]>, indices: Vec<u32>) -> Prim {
    Prim {
        positions,
        indices: Some(indices),
        mode: 4,
    }
}

/// Builds a GLB with one mesh made of `prims`; `nodes` replaces the default single node.
fn glb(prims: &[Prim], nodes: Option<Value>) -> Vec<u8> {
    let mut bin = Vec::new();
    let (mut views, mut accessors, mut primitives) = (vec![], vec![], vec![]);
    for p in prims {
        let off = bin.len();
        for v in &p.positions {
            for x in v {
                bin.extend(x.to_le_bytes());
            }
        }
        views.push(json!({"buffer":0,"byteOffset":off,"byteLength":bin.len()-off}));
        let min: Vec<f32> = (0..3)
            .map(|k| p.positions.iter().map(|v| v[k]).fold(f32::MAX, f32::min))
            .collect();
        let max: Vec<f32> = (0..3)
            .map(|k| p.positions.iter().map(|v| v[k]).fold(f32::MIN, f32::max))
            .collect();
        accessors.push(json!({"bufferView":views.len()-1,"componentType":5126,"count":p.positions.len(),"type":"VEC3","min":min,"max":max}));
        let mut prim = json!({"attributes":{"POSITION":accessors.len()-1},"mode":p.mode});
        if let Some(ix) = &p.indices {
            let off = bin.len();
            for i in ix {
                bin.extend(i.to_le_bytes());
            }
            views.push(json!({"buffer":0,"byteOffset":off,"byteLength":bin.len()-off}));
            accessors.push(json!({"bufferView":views.len()-1,"componentType":5125,"count":ix.len(),"type":"SCALAR"}));
            prim["indices"] = json!(accessors.len() - 1);
        }
        primitives.push(prim);
    }
    let doc = json!({
        "asset":{"version":"2.0"},
        "scene":0,"scenes":[{"nodes":[0]}],
        "nodes": nodes.unwrap_or(json!([{"mesh":0,"name":"part"}])),
        "meshes":[{"primitives":primitives}],
        "accessors":accessors,"bufferViews":views,"buffers":[{"byteLength":bin.len()}]
    });
    let mut js = serde_json::to_vec(&doc).unwrap();
    while js.len() % 4 != 0 {
        js.push(b' ');
    }
    while bin.len() % 4 != 0 {
        bin.push(0);
    }
    let total = 12 + 8 + js.len() + 8 + bin.len();
    let mut out = b"glTF".to_vec();
    out.extend(2u32.to_le_bytes());
    out.extend((total as u32).to_le_bytes());
    out.extend((js.len() as u32).to_le_bytes());
    out.extend(b"JSON");
    out.extend(js);
    out.extend((bin.len() as u32).to_le_bytes());
    out.extend(b"BIN\0");
    out.extend(bin);
    out
}

fn read_bytes(dir: &tempfile::TempDir, bytes: &[u8]) -> anyhow::Result<Vec<ModelObject>> {
    let p = dir.path().join("h.glb");
    std::fs::write(&p, bytes).unwrap();
    gltf_io::read(&p)
}

fn world_aabb(o: &ModelObject) -> Aabb {
    o.mesh.aabb_transformed(&o.transform)
}

fn close(a: Vec3, b: Vec3, tol: f32) -> bool {
    (a - b).length() < tol
}

fn unit_triangle() -> Vec<[f32; 3]> {
    vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]
}

#[test]
fn child_node_transform_is_composed_with_the_parent() {
    // glTF is Y-up, metres. Child: scale 2, rotate 90 deg about Y, move +2 Z. Parent: move +1 X.
    let s = std::f32::consts::FRAC_1_SQRT_2;
    let nodes = json!([
        {"children":[1], "translation":[1.0,0.0,0.0]},
        {"mesh":0,"name":"kid","translation":[0.0,0.0,2.0],"rotation":[0.0,s,0.0,s],"scale":[2.0,2.0,2.0]}
    ]);
    let dir = tempfile::tempdir().unwrap();
    let o = read_bytes(
        &dir,
        &glb(&[tris(unit_triangle(), vec![0, 1, 2])], Some(nodes)),
    )
    .unwrap();
    assert_eq!(o.len(), 1);
    assert_eq!(o[0].name, "kid");
    // glTF-space results: (0,0,0)->(1,0,2); (1,0,0)->(1,0,0); (0,1,0)->(1,2,2).
    // App space is mm, Z-up: (gx,gy,gz) -> (1000 gx, -1000 gz, 1000 gy).
    let w = |i| o[0].transform.transform_point3(o[0].mesh.positions[i]);
    assert!(
        close(w(0), Vec3::new(1000.0, -2000.0, 0.0), 0.05),
        "{:?}",
        w(0)
    );
    assert!(close(w(1), Vec3::new(1000.0, 0.0, 0.0), 0.05), "{:?}", w(1));
    assert!(
        close(w(2), Vec3::new(1000.0, -2000.0, 2000.0), 0.05),
        "{:?}",
        w(2)
    );
}

fn cube_split_in_two_primitives() -> Vec<Prim> {
    // A unit cube; each primitive owns its own copy of the vertices it uses.
    let cube = primitives::cube(1.0);
    let pos: Vec<[f32; 3]> = cube.positions.iter().map(|p| p.to_array()).collect();
    let flat: Vec<u32> = cube.triangles.iter().flatten().copied().collect();
    vec![
        tris(pos.clone(), flat[..18].to_vec()),
        tris(pos, flat[18..].to_vec()),
    ]
}

#[test]
fn primitives_of_one_mesh_are_welded_into_one_closed_object() {
    let dir = tempfile::tempdir().unwrap();
    let o = read_bytes(&dir, &glb(&cube_split_in_two_primitives(), None)).unwrap();
    assert_eq!(o.len(), 1);
    let s = o[0].mesh.stats();
    assert_eq!(s.vertices, 8, "coincident vertices must be merged");
    assert_eq!(s.triangles, 12);
    assert!(s.is_closed_manifold(), "{s:?}");
}

#[test]
fn glb_is_metres_y_up_and_comes_back_as_mm_z_up() {
    // 0.01 m along X, 0.04 m along glTF Y (up), 0.02 m along glTF Z.
    let pos = vec![
        [0.0, 0.0, 0.0],
        [0.01, 0.0, 0.0],
        [0.01, 0.04, 0.0],
        [0.0, 0.04, 0.0],
        [0.0, 0.0, 0.02],
        [0.01, 0.0, 0.02],
        [0.01, 0.04, 0.02],
        [0.0, 0.04, 0.02],
    ];
    let dir = tempfile::tempdir().unwrap();
    let o = read_bytes(&dir, &glb(&[tris(pos, vec![0, 1, 2])], None)).unwrap();
    let size = world_aabb(&o[0]).size();
    assert!(close(size, Vec3::new(10.0, 20.0, 40.0), 1e-2), "{size:?}");
}

#[test]
fn written_gltf_is_metres_y_up_and_cube_reads_back_as_40_mm() {
    let dir = tempfile::tempdir().unwrap();
    let obj = ModelObject {
        name: "box".into(),
        mesh: primitives::box_mesh(Vec3::new(10.0, 20.0, 30.0)),
        transform: Mat4::IDENTITY,
        color: [1, 2, 3, 255],
    };
    let p = dir.path().join("b.gltf");
    gltf_io::write(&p, std::slice::from_ref(&obj), false).unwrap();

    // Raw file: node matrix maps the 10 x 20 x 30 mm box to 0.010 x 0.030 x 0.020 m (Y up).
    let doc: Value = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
    let m: Vec<f32> = doc["nodes"][1]["matrix"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap() as f32)
        .collect();
    let m = Mat4::from_cols_slice(&m);
    let size = obj.mesh.aabb_transformed(&m).size();
    assert!(
        close(size, Vec3::new(0.010, 0.030, 0.020), 1e-6),
        "{size:?}"
    );

    let cube = ModelObject {
        mesh: primitives::cube(40.0),
        ..obj
    };
    gltf_io::write(&p, &[cube], false).unwrap();
    let back = gltf_io::read(&p).unwrap();
    assert!(close(world_aabb(&back[0]).size(), Vec3::splat(40.0), 1e-3));
    assert!(
        world_aabb(&back[0]).min.z.abs() < 1e-3,
        "rests on the floor"
    );
}

fn round_trip(ext: &str, binary: bool) {
    let dir = tempfile::tempdir().unwrap();
    let p: PathBuf = dir.path().join(format!("rt.{ext}"));
    let objs = vec![
        ModelObject {
            name: "a".into(),
            mesh: primitives::cube(10.0),
            transform: Mat4::from_translation(Vec3::new(30.0, 5.0, 0.0)),
            color: [200, 30, 90, 255],
        },
        ModelObject {
            name: "b".into(),
            mesh: primitives::wedge(10.0, 20.0, 30.0),
            transform: Mat4::from_scale(Vec3::splat(2.0)),
            color: [0x00, 0xA0, 0xE8, 128],
        },
    ];
    gltf_io::write(&p, &objs, binary).unwrap();
    let back = gltf_io::read(&p).unwrap();
    assert_eq!(back.len(), 2);
    for o in &objs {
        let b = back.iter().find(|b| b.name == o.name).expect("object kept");
        assert_eq!(b.color, o.color, "{}", o.name);
        let (vo, vb) = (
            o.mesh.transformed(&o.transform).stats().volume,
            b.mesh.transformed(&b.transform).stats().volume,
        );
        assert!((vo - vb).abs() / vo < 1e-3, "{}: {vo} vs {vb}", o.name);
        let (ao, ab) = (o.mesh.aabb_transformed(&o.transform), world_aabb(b));
        assert!(close(ao.min, ab.min, 1e-2) && close(ao.max, ab.max, 1e-2));
    }
}

#[test]
fn round_trip_keeps_the_object_order() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("order.glb");
    let o = |n: &str| ModelObject {
        name: n.into(),
        mesh: primitives::cube(10.0),
        transform: Mat4::IDENTITY,
        color: [1, 2, 3, 255],
    };
    gltf_io::write(&p, &[o("first"), o("second"), o("third")], true).unwrap();
    let names: Vec<String> = gltf_io::read(&p)
        .unwrap()
        .into_iter()
        .map(|o| o.name)
        .collect();
    assert_eq!(names, ["first", "second", "third"]);
}

#[test]
fn glb_round_trip_keeps_volume_colour_and_placement() {
    round_trip("glb", true);
}

#[test]
fn gltf_round_trip_keeps_volume_colour_and_placement() {
    round_trip("gltf", false);
}

#[test]
fn gltf_write_creates_the_external_bin_beside_the_json() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("x.gltf");
    let o = ModelObject {
        name: "c".into(),
        mesh: primitives::cube(5.0),
        transform: Mat4::IDENTITY,
        color: [1, 1, 1, 255],
    };
    gltf_io::write(&p, &[o], false).unwrap();
    assert!(Path::new(&dir.path().join("x.bin")).exists());
}

#[test]
fn out_of_range_vertex_index_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    assert!(read_bytes(&dir, &glb(&[tris(unit_triangle(), vec![0, 1, 3])], None)).is_err());
}

fn square() -> Vec<[f32; 3]> {
    vec![
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [1.0, 1.0, 0.0],
    ]
}

#[test]
fn triangle_strips_are_read_with_consistent_winding() {
    let dir = tempfile::tempdir().unwrap();
    let strip = Prim {
        positions: square(),
        indices: Some(vec![0, 1, 2, 3]),
        mode: 5,
    };
    let m = &read_bytes(&dir, &glb(&[strip], None)).unwrap()[0].mesh;
    assert_eq!(m.triangles.len(), 2);
    assert!((m.stats().area - 1.0).abs() < 1e-6);
    let n: Vec<Vec3> = m
        .triangle_positions()
        .map(|[a, b, c]| buildcraft3d::mesh::Mesh::face_normal(a, b, c))
        .collect();
    assert!(n[0].dot(n[1]) > 0.99, "{n:?}");
}

#[test]
fn triangle_fans_are_read() {
    let dir = tempfile::tempdir().unwrap();
    let fan = Prim {
        positions: vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
        ],
        indices: Some(vec![0, 1, 2, 3]),
        mode: 6,
    };
    let m = &read_bytes(&dir, &glb(&[fan], None)).unwrap()[0].mesh;
    assert_eq!(m.triangles.len(), 2);
    assert!((m.stats().area - 1.0).abs() < 1e-6);
}

#[test]
fn primitives_without_indices_use_vertices_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let p = Prim {
        positions: unit_triangle(),
        indices: None,
        mode: 4,
    };
    assert_eq!(
        read_bytes(&dir, &glb(&[p], None)).unwrap()[0]
            .mesh
            .triangles,
        vec![[0, 1, 2]]
    );
}

#[test]
fn file_with_only_point_primitives_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let pts = Prim {
        positions: unit_triangle(),
        indices: None,
        mode: 0,
    };
    assert!(read_bytes(&dir, &glb(&[pts], None)).is_err());
}

#[test]
fn garbage_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    assert!(read_bytes(&dir, b"glTF not really").is_err());
    let p = dir.path().join("x.gltf");
    std::fs::write(&p, "{ not json").unwrap();
    assert!(gltf_io::read(&p).is_err());
}
