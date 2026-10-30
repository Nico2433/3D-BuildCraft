//! Thumbnails for tiles, made on a background thread so browsing never waits on loading
//! a model. Files are also cached on disk, keyed by path and modification time.

use super::*;
use crate::thumbnail;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::SystemTime;

/// Thumbnail size in pixels.
const SIZE: u32 = 96;
/// Most thumbnails kept in memory before the file ones are dropped and made again.
const MAX_READY: usize = 600;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    /// A file and its modification time, so an edited file gets a new picture.
    File(PathBuf, Option<SystemTime>),
    /// A scene object: its mesh (by address) and colour.
    Mesh(usize, Rgba),
}

enum Job {
    /// A file, with the browsing generation it was asked in.
    File(PathBuf, Option<SystemTime>, u64),
    Mesh(Key, Arc<Mesh>, Rgba),
}

/// What the worker sends back: a picture, `None` when the model could not be read, or
/// `Skipped` when the job was for files no longer on screen.
enum Outcome {
    Done(Option<egui::ColorImage>),
    Skipped,
}

pub struct Thumbs {
    jobs: Sender<Job>,
    done: Receiver<(Key, Outcome)>,
    ready: HashMap<Key, Option<egui::TextureHandle>>,
    waiting: HashSet<Key>,
    /// Bumped when the files on screen change; older file jobs are skipped.
    generation: Arc<AtomicU64>,
}

impl Thumbs {
    pub fn new() -> Self {
        let (jobs, rx) = channel::<Job>();
        let (tx, done) = channel();
        let generation = Arc::new(AtomicU64::new(0));
        let current = generation.clone();
        let image = |px: Vec<u8>| {
            egui::ColorImage::from_rgba_unmultiplied([SIZE as usize, SIZE as usize], &px)
        };
        std::thread::spawn(move || {
            for job in rx {
                let sent = match job {
                    Job::File(p, m, g) if g < current.load(Ordering::Relaxed) => {
                        tx.send((Key::File(p, m), Outcome::Skipped))
                    }
                    Job::File(p, m, _) => {
                        let px = file_thumbnail(&p);
                        tx.send((Key::File(p, m), Outcome::Done(px.map(image))))
                    }
                    Job::Mesh(key, mesh, color) => {
                        let px = thumbnail::render(&mesh, color, SIZE);
                        tx.send((key, Outcome::Done(Some(image(px)))))
                    }
                };
                if sent.is_err() {
                    break;
                }
            }
        });
        Self {
            jobs,
            done,
            ready: HashMap::new(),
            waiting: HashSet::new(),
            generation,
        }
    }

    /// Takes finished thumbnails; call once a frame.
    pub fn collect(&mut self, ctx: &egui::Context) {
        for (key, outcome) in self.done.try_iter() {
            self.waiting.remove(&key);
            if let Outcome::Done(image) = outcome {
                let tex = image.map(|i| {
                    ctx.load_texture(format!("thumb {key:?}"), i, egui::TextureOptions::LINEAR)
                });
                self.ready.insert(key, tex);
            }
        }
        if !self.waiting.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    /// The thumbnail of a file, or `None` while it is being made (or could not be).
    pub fn file(&mut self, path: &Path) -> Option<egui::TextureId> {
        let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok();
        let key = Key::File(path.to_path_buf(), modified);
        let g = self.generation.load(Ordering::Relaxed);
        self.get(key, || Job::File(path.to_path_buf(), modified, g))
    }

    /// The thumbnail of a scene object.
    pub fn object(&mut self, o: &Object) -> Option<egui::TextureId> {
        let key = Key::Mesh(Arc::as_ptr(&o.mesh) as usize, o.color);
        let job = Job::Mesh(key.clone(), o.mesh.clone(), o.color);
        self.get(key, || job)
    }

    fn get(&mut self, key: Key, job: impl FnOnce() -> Job) -> Option<egui::TextureId> {
        if let Some(t) = self.ready.get(&key) {
            return t.as_ref().map(|t| t.id());
        }
        if self.waiting.insert(key) {
            let _ = self.jobs.send(job());
        }
        None
    }

    /// The files on screen changed (another folder or search): queued file jobs are
    /// skipped, and the memory cache is trimmed when it grew large.
    pub fn new_page(&mut self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
        self.waiting.retain(|k| !matches!(k, Key::File(..)));
        if self.ready.len() > MAX_READY {
            self.ready.retain(|k, _| !matches!(k, Key::File(..)));
        }
    }

    /// Drops scene-object thumbnails whose mesh is gone, so memory does not grow with edits.
    pub fn keep_objects(&mut self, objects: &[Object]) {
        let live: HashSet<usize> = objects
            .iter()
            .map(|o| Arc::as_ptr(&o.mesh) as usize)
            .collect();
        self.ready.retain(|k, _| match k {
            Key::Mesh(ptr, _) => live.contains(ptr),
            Key::File(..) => true,
        });
    }
}

/// From the disk cache, or by loading the model; `None` if it cannot be read.
// shortcut: the disk cache is never pruned; add an age or size limit if it grows large.
fn file_thumbnail(path: &Path) -> Option<Vec<u8>> {
    let cache = cache_path(path);
    if let Some(img) = cache.as_ref().and_then(|c| image::open(c).ok()) {
        let img = img.to_rgba8();
        if img.width() == SIZE && img.height() == SIZE {
            return Some(img.into_raw());
        }
    }
    let model = io::load(path).ok()?;
    let meshes = io::baked(&model.objects);
    let refs: Vec<(&Mesh, Rgba)> = meshes
        .iter()
        .zip(&model.objects)
        .map(|(m, o)| (m, o.color))
        .collect();
    let px = thumbnail::render_parts(&refs, SIZE);
    if let Some(c) = cache {
        if let Some(dir) = c.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = image::RgbaImage::from_raw(SIZE, SIZE, px.clone()).map(|i| i.save(&c));
    }
    Some(px)
}

fn cache_path(path: &Path) -> Option<PathBuf> {
    let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok()?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (path, modified, SIZE).hash(&mut h);
    let dirs = crate::settings::dirs()?;
    Some(
        dirs.cache_dir()
            .join("thumbnails")
            .join(format!("{:016x}.png", h.finish())),
    )
}
