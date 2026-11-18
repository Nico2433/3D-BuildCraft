//! Reproducible timings of the mesh hot paths on a large generated sphere. Run with
//! `cargo test --offline --release -j 4 --test perf_bench -- --ignored --nocapture`.

use buildcraft3d::io::{self, ModelInfo, ModelObject};
use buildcraft3d::mesh::Mesh;
use buildcraft3d::{primitives, repair, scene::DEFAULT_COLOR};
use glam::Mat4;
use std::time::Instant;

fn time<T>(name: &str, f: impl FnOnce() -> T) -> T {
    let t = Instant::now();
    let r = f();
    println!("BENCH {name}: {:.1} ms", t.elapsed().as_secs_f64() * 1e3);
    r
}

#[test]
#[ignore]
fn mesh_hot_paths() {
    let m = primitives::sphere(10.0, 400, 400);
    println!("BENCH triangles: {}", m.triangles.len());
    let soup: Vec<_> = m.triangle_positions().collect();
    time("from_triangle_soup", || Mesh::from_triangle_soup(&soup));
    time("welded", || m.welded());
    time("stats", || m.stats());
    time("shading", || m.shading(30.0));
    time("vertex_normals", || m.vertex_normals());
    time("aabb", || m.aabb());
    time("solids", || m.solids());
    time("repair", || repair::repair(&m));
    let obj = ModelObject {
        name: "s".into(),
        mesh: m.clone(),
        transform: Mat4::IDENTITY,
        color: DEFAULT_COLOR,
    };
    let path = std::env::temp_dir().join("bench_sphere.stl");
    time("stl_write", || {
        io::save(&path, std::slice::from_ref(&obj), &ModelInfo::default()).unwrap()
    });
    time("stl_read", || io::load(&path).unwrap());
    let p3 = std::env::temp_dir().join("bench_sphere.3mf");
    time("3mf_write", || {
        io::save(&p3, std::slice::from_ref(&obj), &ModelInfo::default()).unwrap()
    });
    time("3mf_read", || io::load(&p3).unwrap());
    time("thumbnail", || {
        buildcraft3d::thumbnail::render(&m, DEFAULT_COLOR, 256)
    });
}

#[test]
#[ignore]
fn boolean_and_cut() {
    use buildcraft3d::boolean::{apply, Op};
    use glam::Vec3;
    let a = primitives::sphere(10.0, 200, 200);
    let b = a.transformed(&Mat4::from_translation(Vec3::new(5.0, 0.0, 0.0)));
    println!("BENCH boolean triangles each: {}", a.triangles.len());
    let r = time("boolean_subtract", || {
        apply(Op::Subtract, &[a.clone(), b.clone()]).unwrap()
    });
    println!("BENCH boolean result triangles: {}", r.triangles.len());
    time("boolean_union", || {
        apply(Op::Union, &[a.clone(), b.clone()]).unwrap()
    });
}
