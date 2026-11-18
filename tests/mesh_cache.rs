//! The renderer's copy-selection logic (`MeshCache`) without a GPU: no visible shading
//! change when a new mesh replaces another, and no needless shading work per tool entry.
//! One test function runs everything, because the worker counter is process-wide.

use buildcraft3d::mesh::{Mesh, Shading};
use buildcraft3d::mesh_cache::{MeshCache, CREASE_DEG, SYNC_SHADING_TRIANGLES, WORKER_SHADINGS};
use buildcraft3d::{primitives, solid};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

type Cache = MeshCache<Shading>;

fn build(_: &Arc<Mesh>, s: &Shading) -> Shading {
    s.clone()
}

fn frame(cache: &mut Cache, owner: u64, mesh: &Arc<Mesh>) -> usize {
    let key = cache.resolve(owner, mesh, &mut build, || {});
    cache.finish_frame();
    key
}

fn same(a: &Shading, b: &Shading) -> bool {
    a.positions == b.positions
        && a.normals == b.normals
        && a.triangles == b.triangles
        && a.colors == b.colors
        && a.edges == b.edges
}

fn workers() -> usize {
    WORKER_SHADINGS.load(Ordering::Relaxed)
}

fn wait_until_final(cache: &mut Cache, owner: u64, mesh: &Arc<Mesh>) -> usize {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let before = cache.stats.worker_results;
        let key = frame(cache, owner, mesh);
        if cache.stats.worker_results > before {
            return key;
        }
        assert!(Instant::now() < deadline, "the worker never answered");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn shading_is_final_from_the_first_frame_and_not_redone() {
    let cube = Arc::new(primitives::cube(10.0));
    let sphere = Arc::new(primitives::sphere(10.0, 40, 40));
    assert!(sphere.triangles.len() <= SYNC_SHADING_TRIANGLES);
    let workers_at_start = workers();

    // 1. Small meshes: the first upload is the final shading, no provisional, no worker.
    for (i, mesh) in [&cube, &sphere].into_iter().enumerate() {
        let mut cache = Cache::default();
        let key = frame(&mut cache, i as u64, mesh);
        assert!(same(cache.get(key).unwrap(), &mesh.shading(CREASE_DEG)));
        assert_eq!(cache.stats.provisional, 0);
        assert_eq!(cache.stats.held, 0);
    }
    assert_eq!(workers(), workers_at_start);

    // 2. Tool entry, many frames, leave, undo, redo: one shading per distinct mesh. (The
    // hollow cube has 25k triangles, so the limit is raised to keep all of them small.)
    let mut cache = Cache::with_sync_limit(100_000);
    let simplified = Arc::new(sphere.simplified(0.5));
    let hollow = Arc::new(solid::hollow(&cube, 1.0).unwrap());
    let smoothed = Arc::new(solid::smooth_by(&cube, 0.5, true).unwrap());
    let mut expected = 0;
    frame(&mut cache, 1, &sphere);
    expected += 1;
    for preview in [&simplified, &hollow, &smoothed] {
        for _ in 0..30 {
            frame(&mut cache, 1, preview); // entering the tool: one new mesh
        }
        expected += 1;
        for _ in 0..30 {
            frame(&mut cache, 1, &sphere); // leaving it (cancel): the original is back
        }
        for _ in 0..3 {
            frame(&mut cache, 1, preview); // redo, undo, redo
            frame(&mut cache, 1, &sphere);
        }
    }
    assert_eq!(cache.stats.sync_shadings, expected, "{:?}", cache.stats);
    assert_eq!(workers(), workers_at_start);

    // 3. Big meshes (limit lowered to keep the test fast): the previous copy is held and
    // the worker's result replaces it; provisional only for the first load.
    let mut cache = Cache::with_sync_limit(100);
    let a = Arc::new(primitives::sphere(10.0, 30, 30));
    let b = Arc::new(a.simplified(0.5));
    let ka = frame(&mut cache, 7, &a);
    assert_eq!(cache.stats.provisional, 1, "first load has nothing to show");
    assert!(
        !same(cache.get(ka).unwrap(), &a.shading(CREASE_DEG)),
        "the first-load copy is the provisional one"
    );
    let ka = wait_until_final(&mut cache, 7, &a);
    assert!(same(cache.get(ka).unwrap(), &a.shading(CREASE_DEG)));
    let provisional = cache.stats.provisional;
    let held = cache.stats.held;
    // A new mesh for the same object: its old final copy stays on screen.
    let k = frame(&mut cache, 7, &b);
    assert_eq!(k, ka);
    assert!(cache.stats.held > held);
    assert_eq!(cache.stats.provisional, provisional);
    let kb = wait_until_final(&mut cache, 7, &b);
    assert_ne!(kb, ka);
    assert!(same(cache.get(kb).unwrap(), &b.shading(CREASE_DEG)));
    assert_eq!(cache.stats.provisional, provisional, "never provisional");
    // Back to the original (leave the tool) and undo/redo: cached, no new worker run.
    let runs = workers();
    for _ in 0..5 {
        let k = frame(&mut cache, 7, &a);
        assert!(same(cache.get(k).unwrap(), &a.shading(CREASE_DEG)));
        let k = frame(&mut cache, 7, &b);
        assert!(same(cache.get(k).unwrap(), &b.shading(CREASE_DEG)));
    }
    assert_eq!(workers(), runs);
    // An owner that is gone drops its copies once the spares overflow.
    drop(cache);

    // 4. Results of meshes dropped meanwhile are not used for another mesh.
    let mut cache = Cache::with_sync_limit(100);
    let old = Arc::new(primitives::sphere(10.0, 30, 30));
    frame(&mut cache, 9, &old);
    drop(old);
    let new = Arc::new(primitives::sphere(10.0, 20, 20));
    let key = wait_until_final(&mut cache, 9, &new);
    assert!(same(cache.get(key).unwrap(), &new.shading(CREASE_DEG)));
}
