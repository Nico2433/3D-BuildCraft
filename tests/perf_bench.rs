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
    use glam::Vec3;
    let m = primitives::sphere(10.0, 400, 400);
    let n = time("face_region_near (brush 2 mm)", || {
        m.face_region_near(1000, 0.5, &Mat4::IDENTITY, Some((m.positions[1000], 2.0)))
    });
    println!("BENCH region triangles: {}", n.len());
    let _ = Vec3::ZERO;
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

/// Circular edges (Measure > Circular edge): the time to find the feature edges of a
/// mesh (off the UI thread in the app), then what a hover or click costs once they are
/// known, on a subdivided sphere, the camel and the user's flower pots when present.
/// Prints the circular loops found with their diameter and centre.
#[test]
#[ignore]
fn circular_edge_latency() {
    use buildcraft3d::edge_loops::FeatureEdges;
    use std::path::Path;
    let mut models: Vec<(String, Mesh, Mat4)> = vec![(
        "sphere 400x400".into(),
        primitives::sphere(10.0, 400, 400),
        Mat4::IDENTITY,
    )];
    let files = [
        "/mnt/hdd/Library/Projects/3D Builder/models/camel Repair.stl",
        "/run/media/nico/KODAK/3D/Macetas/Robert/Meditacion/meditacion_medio.stl",
        "/run/media/nico/KODAK/3D/Macetas/Robert/Lector/lector_medio.stl",
        "/run/media/nico/KODAK/3D/Macetas/Robert/Cafe/cafe_medio.stl",
        "/run/media/nico/KODAK/3D/Macetas/Robert/Mascotas/mascotas_medio.stl",
        "/run/media/nico/KODAK/3D/Macetas/Robert/Madre e hijo.stl",
    ];
    for f in files.iter().filter(|f| Path::new(f).exists()) {
        let loaded = io::load(Path::new(f)).unwrap();
        for o in loaded.objects {
            models.push((f.rsplit('/').next().unwrap().into(), o.mesh, o.transform));
        }
    }
    for (name, mesh, model) in models {
        println!("BENCH {name}: {} triangles", mesh.triangles.len());
        let feats = time("  feature edges (worker)", || FeatureEdges::compute(&mesh));
        println!(
            "BENCH   {} feature edges, {} chains",
            feats.edge_count(),
            feats.chain_count()
        );
        let b = mesh.aabb_transformed(&model);
        let probe = b.max;
        let hit = time("  hover: nearest edge", || {
            feats.nearest_chain(&model, probe, f32::INFINITY)
        });
        if let Some((chain, _)) = hit {
            time("  click: loop + circle fit", || {
                feats.edge_loop(chain, &model)
            });
        }
        let mut round: Vec<_> = time("  all circular loops", || {
            feats
                .loops(&model)
                .filter_map(|l| l.round().map(|r| (r, l.points.len(), l.fit.unwrap().rms)))
                .collect()
        });
        round.sort_by(|a, b| b.0.radius.total_cmp(&a.0.radius));
        println!("BENCH   box size {:?}, centre {:?}", b.size(), b.center());
        for (r, n, rms) in round.iter().take(8) {
            println!(
                "BENCH   circle: diameter {:.2}, centre {:.2?}, axis {:.2?}, {n} vertices, rms {:.4}",
                r.diameter(),
                r.center,
                r.axis.unwrap(),
                rms
            );
        }
        println!("BENCH   {} circular loops", round.len());
    }
}
