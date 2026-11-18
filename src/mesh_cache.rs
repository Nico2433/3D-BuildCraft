//! Which mesh copy is drawn for each object, and how its shading is obtained without a
//! visible change or a UI stall. Generic over the GPU copy so tests can run it without a
//! device.
//!
//! Small meshes are shaded on the UI thread (cheap). A big one is shaded on a worker
//! ([`WorkCache`]); until the result arrives the object keeps drawing the copy it had
//! before, and only a mesh with nothing earlier to show (the first load of a huge file)
//! is drawn with the provisional shading.

use crate::mesh::{Mesh, Shading};
use crate::scene::WorkCache;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Angle above which neighbouring faces get separate vertices (a crease).
pub const CREASE_DEG: f32 = 40.0;

/// Largest mesh whose shading is computed on the UI thread: `Mesh::shading` costs about
/// 0.2 ms per 1000 triangles (release build, `shading_cost_by_size` in
/// `tests/perf_bench.rs`), so this is about 2 ms.
pub const SYNC_SHADING_TRIANGLES: usize = 10_000;

/// Copies of meshes that are not drawn now are kept for undo, redo and leaving a tool,
/// up to this many and this many triangles in all.
const SPARE_COPIES: usize = 4;
const SPARE_TRIANGLES: usize = 1_500_000;

/// Worker runs started, for tests (the worker is one function shared by all caches).
pub static WORKER_SHADINGS: AtomicUsize = AtomicUsize::new(0);

fn worker_shading(m: &Mesh) -> Arc<Shading> {
    WORKER_SHADINGS.fetch_add(1, Ordering::Relaxed);
    Arc::new(m.shading(CREASE_DEG))
}

/// What the cache did, for tests and logs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CacheStats {
    /// Shadings computed on the UI thread.
    pub sync_shadings: usize,
    /// Frames an object kept drawing its previous copy while the worker worked.
    pub held: usize,
    /// Copies built from the provisional shading (nothing earlier to show).
    pub provisional: usize,
    /// Worker results turned into copies.
    pub worker_results: usize,
}

struct Entry<G> {
    gpu: G,
    mesh: Arc<Mesh>,
    provisional: bool,
}

pub struct MeshCache<G> {
    entries: HashMap<usize, Entry<G>>,
    /// The copy each object (by owner id) drew last.
    shown: HashMap<u64, usize>,
    shading: WorkCache<Arc<Shading>>,
    sync_limit: usize,
    used: HashSet<usize>,
    owners: HashSet<u64>,
    spare: VecDeque<usize>,
    pub stats: CacheStats,
}

impl<G> Default for MeshCache<G> {
    fn default() -> Self {
        Self::with_sync_limit(SYNC_SHADING_TRIANGLES)
    }
}

impl<G> MeshCache<G> {
    pub fn with_sync_limit(sync_limit: usize) -> Self {
        Self {
            entries: HashMap::new(),
            shown: HashMap::new(),
            shading: WorkCache::default(),
            sync_limit,
            used: HashSet::new(),
            owners: HashSet::new(),
            spare: VecDeque::new(),
            stats: CacheStats::default(),
        }
    }

    pub fn get(&self, key: usize) -> Option<&G> {
        self.entries.get(&key).map(|e| &e.gpu)
    }

    pub fn get_mut(&mut self, key: usize) -> Option<&mut G> {
        self.entries.get_mut(&key).map(|e| &mut e.gpu)
    }

    pub fn values(&self) -> impl Iterator<Item = &G> {
        self.entries.values().map(|e| &e.gpu)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The key (see [`Self::get`]) of the copy to draw for `owner`'s `mesh` this frame.
    /// `build` makes a copy from a shading; `wake` runs on the worker when it finishes.
    pub fn resolve(
        &mut self,
        owner: u64,
        mesh: &Arc<Mesh>,
        build: &mut dyn FnMut(&Arc<Mesh>, &Shading) -> G,
        wake: impl FnOnce() + Send + 'static,
    ) -> usize {
        let key = Arc::as_ptr(mesh) as usize;
        self.owners.insert(owner);
        let shown = self.pick(owner, key, mesh, build, wake);
        self.used.insert(shown);
        self.shown.insert(owner, shown);
        shown
    }

    fn pick(
        &mut self,
        owner: u64,
        key: usize,
        mesh: &Arc<Mesh>,
        build: &mut dyn FnMut(&Arc<Mesh>, &Shading) -> G,
        wake: impl FnOnce() + Send + 'static,
    ) -> usize {
        if self.entries.get(&key).is_some_and(|e| !e.provisional) {
            return key;
        }
        if mesh.triangles.len() <= self.sync_limit {
            self.stats.sync_shadings += 1;
            let shading = mesh.shading(CREASE_DEG);
            self.insert(key, mesh, build(mesh, &shading), false);
            return key;
        }
        if let Some(shading) = self.shading.get(mesh, worker_shading, wake) {
            self.shading.take(mesh);
            self.stats.worker_results += 1;
            self.insert(key, mesh, build(mesh, &shading), false);
            return key;
        }
        if self.entries.contains_key(&key) {
            return key; // already on screen with the provisional shading
        }
        if let Some(&previous) = self.shown.get(&owner) {
            if self.entries.contains_key(&previous) {
                self.stats.held += 1;
                return previous;
            }
        }
        self.stats.provisional += 1;
        let shading = mesh.provisional_shading();
        self.insert(key, mesh, build(mesh, &shading), true);
        key
    }

    fn insert(&mut self, key: usize, mesh: &Arc<Mesh>, gpu: G, provisional: bool) {
        self.entries.insert(
            key,
            Entry {
                gpu,
                mesh: mesh.clone(),
                provisional,
            },
        );
    }

    /// Ends a frame: copies nobody drew are kept as spares (most recent first out), the
    /// rest are dropped, and so are owners that no longer exist.
    pub fn finish_frame(&mut self) {
        let used = std::mem::take(&mut self.used);
        let owners = std::mem::take(&mut self.owners);
        self.shown.retain(|o, _| owners.contains(o));
        self.spare.retain(|k| !used.contains(k));
        for key in self.entries.keys() {
            if !used.contains(key) && !self.spare.contains(key) {
                self.spare.push_back(*key);
            }
        }
        let mut triangles = 0;
        let mut kept = 0;
        let mut keep: HashSet<usize> = used;
        // Newest spares are at the back.
        for &key in self.spare.iter().rev() {
            let Some(e) = self.entries.get(&key) else {
                continue;
            };
            triangles += e.mesh.triangles.len();
            kept += 1;
            if kept <= SPARE_COPIES && triangles <= SPARE_TRIANGLES && !e.provisional {
                keep.insert(key);
            }
        }
        self.entries.retain(|k, _| keep.contains(k));
        self.spare
            .retain(|k| keep.contains(k) && self.entries.contains_key(k));
    }

    pub fn clear(&mut self) {
        *self = Self::with_sync_limit(self.sync_limit);
    }
}
