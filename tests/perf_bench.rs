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

#[test]
#[ignore]
fn cut_obj_emboss() {
    use buildcraft3d::cut::{cut_plane, CutPlane};
    use buildcraft3d::emboss::{self, Emboss};
    use glam::Vec3;
    let m = primitives::sphere(10.0, 400, 400);
    let plane = CutPlane::through(Vec3::new(0.0, 0.0, 1.0), Vec3::new(0.2, 0.1, 1.0)).unwrap();
    time("cut_plane", || cut_plane(&m, &plane));
    let obj = ModelObject {
        name: "s".into(),
        mesh: m.clone(),
        transform: Mat4::IDENTITY,
        color: DEFAULT_COLOR,
    };
    let path = std::env::temp_dir().join("bench_sphere.obj");
    time("obj_write", || {
        io::save(&path, std::slice::from_ref(&obj), &ModelInfo::default()).unwrap()
    });
    time("obj_read", || io::load(&path).unwrap());
    let small = primitives::sphere(10.0, 100, 100);
    let e = Emboss {
        text: "Hello".into(),
        size: 8.0,
        ..Emboss::default()
    };
    let mut e2 = e.clone();
    emboss::place(&small, &mut e2, Vec3::new(0.0, -10.0, 0.0), -Vec3::Y);
    println!("BENCH emboss target triangles: {}", small.triangles.len());
    time("emboss_apply", || emboss::apply(&small, &e2).is_ok());
}

#[test]
#[ignore]
fn paint_region() {
    let m = primitives::sphere(10.0, 400, 400);
    let n = time("face_region_near (brush 2 mm)", || {
        m.face_region_near(1000, 0.5, &Mat4::IDENTITY, Some((m.positions[1000], 2.0)))
    });
    println!("BENCH region triangles: {}", n.len());
}

/// The frame hitch of a new mesh upload: the UI thread used to run `Mesh::shading`;
/// now it builds the provisional shading and the worker computes the real one.
#[test]
#[ignore]
fn shading_upload_hitch() {
    use buildcraft3d::scene::WorkCache;
    use std::sync::Arc;
    let mesh = Arc::new(primitives::sphere(10.0, 400, 400));
    println!("BENCH triangles: {}", mesh.triangles.len());
    time("ui_thread_before (sync shading)", || mesh.shading(40.0));
    time("ui_thread_after (provisional shading)", || {
        mesh.provisional_shading()
    });
    let mut cache: WorkCache<Arc<buildcraft3d::mesh::Shading>> = WorkCache::default();
    let t = Instant::now();
    let first = time("ui_thread_after (cache poll, starts worker)", || {
        cache.get(&mesh, |m| Arc::new(m.shading(40.0)), || {})
    });
    assert!(first.is_none());
    while cache
        .get(&mesh, |m| Arc::new(m.shading(40.0)), || {})
        .is_none()
    {
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    println!(
        "BENCH total_to_final_shading: {:.1} ms",
        t.elapsed().as_secs_f64() * 1e3
    );
}

/// `Mesh::shading` and the provisional shading per triangle count, to choose
/// `render::SYNC_SHADING_TRIANGLES` (the largest mesh shaded on the UI thread).
#[test]
#[ignore]
fn shading_cost_by_size() {
    for (rings, name) in [
        (1, "cube"),
        (50, "sphere 50"),
        (100, "sphere 100"),
        (150, "sphere 150"),
        (200, "sphere 200"),
        (300, "sphere 300"),
        (400, "sphere 400"),
    ] {
        let m = if rings == 1 {
            primitives::cube(10.0)
        } else {
            primitives::sphere(10.0, rings, rings)
        };
        let best = |f: &dyn Fn()| {
            (0..5)
                .map(|_| {
                    let t = Instant::now();
                    f();
                    t.elapsed().as_secs_f64() * 1e3
                })
                .fold(f64::MAX, f64::min)
        };
        println!(
            "BENCH {name}: {} triangles, shading {:.2} ms, provisional {:.2} ms",
            m.triangles.len(),
            best(&|| {
                m.shading(40.0);
            }),
            best(&|| {
                m.provisional_shading();
            }),
        );
    }
}

/// UI-thread cost of the first frame that draws a new mesh, for 12, 20k and 320k
/// triangles: before the worker (`Mesh::shading` inline), with the worker and the
/// provisional smooth shading (commit 4808262, reproduced here from `vertex_normals`),
/// and now (`MeshCache::resolve`: inline below `SYNC_SHADING_TRIANGLES`, otherwise the
/// previous copy is held, or the flat provisional shading on first load). The GPU upload
/// is replaced by a clone of the shading in all of them.
#[test]
#[ignore]
fn new_mesh_ui_cost() {
    use buildcraft3d::mesh::Shading;
    use buildcraft3d::mesh_cache::MeshCache;
    use std::sync::Arc;
    let cases = [
        ("12 triangles", primitives::cube(10.0)),
        ("20k triangles", primitives::sphere(10.0, 100, 100)),
        ("320k triangles", primitives::sphere(10.0, 400, 400)),
    ];
    let mut build = |_: &Arc<Mesh>, s: &Shading| s.clone();
    for (name, mesh) in cases {
        let mesh = Arc::new(mesh);
        let ms = |f: &mut dyn FnMut()| {
            let t = Instant::now();
            f();
            t.elapsed().as_secs_f64() * 1e3
        };
        let before = ms(&mut || {
            std::hint::black_box(mesh.shading(40.0).clone());
        });
        let worker_commit = ms(&mut || {
            let n = mesh.vertex_normals();
            let n = std::hint::black_box(n);
            let mut s = Shading {
                positions: mesh.positions.clone(),
                normals: n,
                triangles: mesh.triangles.clone(),
                ..Default::default()
            };
            s = s.clone();
            std::hint::black_box(s);
        });
        // A new mesh replacing one already drawn by the same object.
        let mut cache: MeshCache<Shading> = MeshCache::default();
        let previous = Arc::new(primitives::sphere(10.0, 20, 20));
        let first_load = ms(&mut || {
            cache.resolve(1, &previous, &mut build, || {});
        });
        cache.finish_frame();
        let replace = ms(&mut || {
            cache.resolve(1, &mesh, &mut build, || {});
        });
        println!(
            "BENCH {name}: before {before:.2} ms | worker commit {worker_commit:.2} ms | now: new mesh over a previous one {replace:.2} ms (small previous load {first_load:.2} ms)"
        );
        // First load of this mesh with nothing to show.
        let mut cache: MeshCache<Shading> = MeshCache::default();
        let first = ms(&mut || {
            cache.resolve(2, &mesh, &mut build, || {});
        });
        println!("BENCH {name}: now, first load with nothing to show {first:.2} ms");
    }
}
