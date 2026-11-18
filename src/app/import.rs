//! Files waiting for Import model: their preview, units and placement. Files dropped
//! together form one batch, previewed side by side and imported with one confirm.

use super::*;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Space between the files of a batch laid out in a row, in millimetres.
const BATCH_GAP: f32 = 10.0;

/// Where a file belongs among files dropped together.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct BatchSlot {
    /// The drop it came from; files with the same id are confirmed together.
    pub id: u64,
    /// Its place in the drop, which keeps the row in the order the files were dropped.
    pub index: usize,
    /// The point on the floor the row is centred on.
    pub drop: Vec3,
}

impl BatchSlot {
    /// A new batch id, different from every earlier one.
    pub fn new_id() -> u64 {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        NEXT.fetch_add(1, Ordering::Relaxed)
    }
}

pub(super) struct PendingImport {
    pub path: PathBuf,
    pub objects: Vec<ModelObject>,
    /// The objects' meshes for drawing while the units are chosen.
    pub preview: Vec<Arc<Mesh>>,
    pub units: Units,
    pub replace_scene: bool,
    /// What the file says about the model; kept when it replaces the scene.
    pub info: io::ModelInfo,
    /// Where on the floor it lands: under the pointer for a dropped file, else the centre.
    pub at: Vec3,
    /// Set for a file dropped together with others.
    pub batch: Option<BatchSlot>,
}

impl PendingImport {
    /// Each object's mesh, placement and colour as it will be imported: at the chosen
    /// units, the whole file moved so its footprint is centred on `at` and it rests on
    /// the floor; the objects keep their places relative to each other.
    pub fn placed(&self) -> impl Iterator<Item = (Arc<Mesh>, glam::Mat4, Rgba)> + '_ {
        let shift = glam::Mat4::from_translation(self.shift());
        self.objects
            .iter()
            .zip(&self.preview)
            .map(move |(o, mesh)| (mesh.clone(), shift * self.model(o), o.color))
    }

    /// An object's placement at the chosen units, before the shift.
    fn model(&self, o: &ModelObject) -> glam::Mat4 {
        glam::Mat4::from_scale(Vec3::splat(self.units.to_mm())) * o.transform
    }

    /// Fails when the file is larger than the tools allow at the chosen units.
    pub fn check_size(&self) -> Result<(), String> {
        io::check_size(self.bounds().size()).map_err(|e| {
            format!("{e:#}; if the file is in other units, give units (micron, mm, cm, inch, foot or m)")
        })
    }

    /// The box round the whole file at the chosen units, before the shift.
    fn bounds(&self) -> Aabb {
        self.objects
            .iter()
            .zip(&self.preview)
            .map(|(o, mesh)| crate::scene::cached_aabb(mesh, &self.model(o)))
            .fold(Aabb::empty(), Aabb::union)
    }

    /// The box round the whole file where the preview shows it.
    fn placed_bounds(&self) -> Aabb {
        let b = self.bounds();
        let s = self.shift();
        Aabb {
            min: b.min + s,
            max: b.max + s,
        }
    }

    /// The move that puts the whole file on the floor, centred on `at`.
    pub fn shift(&self) -> Vec3 {
        let b = self.bounds();
        let c = b.center();
        Vec3::new(self.at.x - c.x, self.at.y - c.y, -b.min.z)
    }

    /// The batch this file belongs to, if it was dropped with others.
    fn batch_id(&self) -> Option<u64> {
        self.batch.map(|b| b.id)
    }
}

/// Where each footprint's centre goes in a row along X, `gap` apart, with the whole row
/// centred on 0. Footprints keep their order and never overlap.
pub(super) fn row_centres(widths: &[f32], gap: f32) -> Vec<f32> {
    let total = widths.iter().sum::<f32>() + gap * widths.len().saturating_sub(1) as f32;
    let mut left = -total / 2.0;
    widths
        .iter()
        .map(|w| {
            let c = left + w / 2.0;
            left += w + gap;
            c
        })
        .collect()
}

/// Lays the files of one batch out side by side on the floor, in the order they were
/// dropped, the row centred on the drop point. Files without a batch are left alone.
pub(super) fn lay_out<'a>(files: impl IntoIterator<Item = &'a mut PendingImport>) {
    let mut files: Vec<&mut PendingImport> =
        files.into_iter().filter(|p| p.batch.is_some()).collect();
    files.sort_by_key(|p| p.batch.map(|b| b.index));
    let widths: Vec<f32> = files.iter().map(|p| p.bounds().size().x).collect();
    for (p, x) in files.into_iter().zip(row_centres(&widths, BATCH_GAP)) {
        let drop = p.batch.map(|b| b.drop).unwrap_or(p.at);
        p.at = drop + Vec3::X * x;
    }
}

/// Adds a confirmed file's objects to the scene where the preview showed them, at the
/// chosen units when `apply_units`. Returns the new objects' ids; those that are not
/// closed are also put in `invalid`.
pub(super) fn add_imported(
    scene: &mut Scene,
    invalid: &mut BTreeSet<u64>,
    p: PendingImport,
    apply_units: bool,
) -> BTreeSet<u64> {
    // Opening a file keeps its placement; adding one puts it where the preview showed.
    let k = if apply_units { p.units.to_mm() } else { 1.0 };
    let keep =
        !apply_units && p.objects.len() > 1 || Format::from_path(&p.path) == Some(Format::ThreeMf);
    let shift = if p.replace_scene && keep {
        Vec3::ZERO
    } else {
        p.shift()
    };
    let mut ids = BTreeSet::new();
    for mut m in p.objects {
        if k != 1.0 {
            m.mesh = m.mesh.transformed(&glam::Mat4::from_scale(Vec3::splat(k)));
            m.transform.w_axis = (m.transform.w_axis.truncate() * k).extend(1.0);
        }
        let stats = m.mesh.stats();
        let mut obj = Object::from_model(m);
        obj.translation += shift;
        let id = scene.add(obj);
        if !stats.is_closed_manifold() {
            invalid.insert(id);
        }
        ids.insert(id);
    }
    ids
}

/// True for a dropped or pasted picture that opens Load image. An SVG is imported as a
/// model instead, as Open does; Load image's own file dialog takes it as a picture.
pub(super) fn is_image(path: &Path) -> bool {
    crate::picture::is_picture(path) && !crate::picture::is_svg(path)
}

impl App {
    /// Queues a loaded file for import, or reports why it could not be read. A file added
    /// to the scene waits for Import model (and its units, when the format has none), as
    /// in the original; one that replaces the scene with known units opens at once.
    pub(super) fn queue_import(
        &mut self,
        path: &Path,
        replace_scene: bool,
        model: anyhow::Result<io::LoadedModel>,
        at: Vec3,
        batch: Option<BatchSlot>,
    ) -> bool {
        let lang = self.settings.lang();
        match model {
            Ok(m) => {
                let p = PendingImport {
                    path: path.to_path_buf(),
                    preview: m.objects.iter().map(|o| Arc::new(o.mesh.clone())).collect(),
                    objects: m.objects,
                    info: m.info,
                    units: Units::Millimeter,
                    replace_scene,
                    at,
                    batch,
                };
                if m.units_known && replace_scene {
                    self.pending_import.insert(0, p);
                    self.finish_import(false);
                } else {
                    let id = p.batch_id();
                    self.pending_import.push(p);
                    // A batch's row grows with each file; the view follows the whole row.
                    let n = self.pending_import.len() - 1;
                    let b = match id {
                        Some(id) => {
                            lay_out(self.batch_members(id));
                            self.pending_import
                                .iter()
                                .filter(|p| p.batch_id() == Some(id))
                                .map(PendingImport::placed_bounds)
                                .fold(Aabb::empty(), Aabb::union)
                        }
                        None => self.pending_import[n].placed_bounds(),
                    };
                    self.glide_to(&b);
                }
                true
            }
            Err(e) => {
                self.toast(
                    format!("{} {e:#}", tr(lang, "Could not open the file:")),
                    true,
                );
                false
            }
        }
    }

    /// The waiting files of one batch.
    fn batch_members(&mut self, id: u64) -> impl Iterator<Item = &mut PendingImport> {
        self.pending_import
            .iter_mut()
            .filter(move |p| p.batch_id() == Some(id))
    }

    /// The waiting files shown now: the first one and, if it was dropped with others,
    /// the rest of its batch.
    pub(super) fn shown_imports(&self) -> impl Iterator<Item = &PendingImport> {
        let id = self
            .pending_import
            .first()
            .and_then(PendingImport::batch_id);
        self.pending_import
            .iter()
            .enumerate()
            .filter(move |(i, p)| *i == 0 || id.is_some() && p.batch_id() == id)
            .map(|(_, p)| p)
    }

    /// Takes the shown files out of the queue, in the order they were dropped.
    fn take_shown_imports(&mut self) -> Vec<PendingImport> {
        let Some(first) = self.pending_import.first() else {
            return Vec::new();
        };
        let id = first.batch_id();
        let (mut shown, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut self.pending_import)
            .into_iter()
            .enumerate()
            .partition(|(i, p)| *i == 0 || id.is_some() && p.batch_id() == id);
        self.pending_import = rest.into_iter().map(|(_, p)| p).collect();
        shown.sort_by_key(|(_, p)| p.batch.map(|b| b.index));
        shown.into_iter().map(|(_, p)| p).collect()
    }

    /// The units chosen for the shown files.
    pub(super) fn import_units(&self) -> Option<Units> {
        self.pending_import.first().map(|p| p.units)
    }

    /// Sets the units of every shown file and lays a batch out again at its new size.
    pub(super) fn set_import_units(&mut self, units: Units) {
        let id = self
            .pending_import
            .first()
            .and_then(PendingImport::batch_id);
        match id {
            Some(id) => {
                for p in self.batch_members(id) {
                    p.units = units;
                }
                lay_out(self.batch_members(id));
            }
            None => {
                if let Some(p) = self.pending_import.first_mut() {
                    p.units = units;
                }
            }
        }
    }

    /// Cancel: the shown files are discarded.
    pub(super) fn cancel_import(&mut self) {
        self.take_shown_imports();
    }

    /// Import model: the shown files are added in one step that undoes at once, and
    /// they end up selected. A file that replaces the scene opens instead.
    pub(super) fn finish_import(&mut self, apply_units: bool) {
        let files = self.take_shown_imports();
        let Some(first) = files.first() else {
            return;
        };
        let replace_scene = first.replace_scene;
        let before = self.snapshot();
        if replace_scene {
            self.scene = Scene::default();
            self.history.clear();
            self.scene.name = first
                .path
                .file_stem()
                .and_then(|s| s.to_str())
                .map(str::to_string);
            self.scene.info = first.info.clone();
            if Format::from_path(&first.path) == Some(Format::ThreeMf) {
                self.scene.path = Some(first.path.clone());
            }
        }
        let paths: Vec<PathBuf> = files.iter().map(|p| p.path.clone()).collect();
        let mut new_ids = BTreeSet::new();
        for p in files {
            new_ids.extend(add_imported(
                &mut self.scene,
                &mut self.invalid,
                p,
                apply_units,
            ));
        }
        self.scene.selection = new_ids;
        if replace_scene {
            self.scene.dirty = false;
        } else {
            self.history.push(before);
        }
        for p in &paths {
            self.settings.add_recent(p);
        }
        let _ = self.settings.save();
        self.camera.frame(&self.scene.scene_aabb(), self.aspect);
    }

    /// Files dropped onto the window (or pasted): pictures open Load image, models are
    /// loaded to land around `at`. Several models dropped at once form one batch.
    pub(super) fn open_dropped(&mut self, paths: Vec<PathBuf>, at: Vec3, ctx: &egui::Context) {
        let (images, models): (Vec<PathBuf>, Vec<PathBuf>) =
            paths.into_iter().partition(|p| is_image(p));
        for p in images {
            self.tool = Some(Tool::image(p));
        }
        let id = (models.len() > 1).then(BatchSlot::new_id);
        for (index, p) in models.iter().enumerate() {
            let batch = id.map(|id| BatchSlot {
                id,
                index,
                drop: at,
            });
            self.load_file_at(p, false, at, batch, ctx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cube_file(name: &str, size: f32, batch: Option<BatchSlot>) -> PendingImport {
        let mesh = crate::primitives::cube(size);
        PendingImport {
            path: PathBuf::from(name),
            preview: vec![Arc::new(mesh.clone())],
            objects: vec![ModelObject {
                name: name.into(),
                mesh,
                transform: glam::Mat4::IDENTITY,
                color: [0xC8, 0xC8, 0xC8, 0xFF],
            }],
            units: Units::Millimeter,
            replace_scene: false,
            info: Default::default(),
            at: Vec3::ZERO,
            batch,
        }
    }

    fn slot(index: usize, drop: Vec3) -> Option<BatchSlot> {
        Some(BatchSlot { id: 7, index, drop })
    }

    #[test]
    fn a_row_is_centred_and_its_footprints_do_not_overlap() {
        let widths = [10.0, 30.0, 4.0];
        let c = row_centres(&widths, 2.0);
        assert_eq!(c.len(), 3);
        for i in 1..c.len() {
            let gap = (c[i] - widths[i] / 2.0) - (c[i - 1] + widths[i - 1] / 2.0);
            assert!((gap - 2.0).abs() < 1e-4, "gap {gap}");
        }
        let left = c[0] - widths[0] / 2.0;
        let right = c[2] + widths[2] / 2.0;
        assert!((left + right).abs() < 1e-4);
        assert_eq!(row_centres(&[5.0], 2.0), vec![0.0]);
        assert!(row_centres(&[], 2.0).is_empty());
    }

    #[test]
    fn a_batch_is_laid_out_in_drop_order_around_the_drop_point() {
        let drop = Vec3::new(20.0, -5.0, 0.0);
        // Loaded out of order: the second file finished first.
        let mut files = [
            cube_file("b", 20.0, slot(1, drop)),
            cube_file("a", 10.0, slot(0, drop)),
        ];
        lay_out(files.iter_mut());
        let (a, b) = (files[1].placed_bounds(), files[0].placed_bounds());
        assert!(a.max.x < b.min.x, "a stays left of b");
        assert!((b.min.x - a.max.x - BATCH_GAP).abs() < 1e-3);
        assert!(((a.min.x + b.max.x) / 2.0 - drop.x).abs() < 1e-3);
        assert!((a.center().y - drop.y).abs() < 1e-3 && (b.center().y - drop.y).abs() < 1e-3);
        assert!(a.min.z.abs() < 1e-4 && b.min.z.abs() < 1e-4, "on the floor");
    }

    #[test]
    fn confirming_a_batch_adds_every_file_where_the_preview_showed_it() {
        let drop = Vec3::new(3.0, 4.0, 0.0);
        let mut files = vec![
            cube_file("a", 10.0, slot(0, drop)),
            cube_file("b", 10.0, slot(1, drop)),
            cube_file("c", 10.0, slot(2, drop)),
        ];
        for p in &mut files {
            p.units = Units::Centimeter;
        }
        lay_out(files.iter_mut());
        let shown: Vec<Aabb> = files.iter().map(PendingImport::placed_bounds).collect();
        let mut scene = Scene::default();
        let mut invalid = BTreeSet::new();
        let mut ids = BTreeSet::new();
        for p in files {
            ids.extend(add_imported(&mut scene, &mut invalid, p, true));
        }
        assert_eq!(ids.len(), 3);
        assert_eq!(scene.objects.len(), 3);
        assert!(invalid.is_empty());
        for (o, want) in scene.objects.iter().zip(&shown) {
            let got = o.world_aabb();
            assert!((got.min - want.min).abs().max_element() < 1e-3);
            assert!((got.size().x - 100.0).abs() < 1e-3, "at the chosen units");
        }
    }

    #[test]
    fn a_file_too_big_at_its_units_is_refused_naming_the_limit() {
        let mut file = cube_file("big.stl", 20.0, None);
        assert!(file.check_size().is_ok());
        file.units = Units::Meter;
        let e = file.check_size().unwrap_err();
        assert!(
            e.contains("the most is 10000 mm") && e.contains("give units"),
            "{e}"
        );
        file.units = Units::Centimeter;
        assert!(file.check_size().is_ok());
    }

    #[test]
    fn pictures_are_told_apart_from_models() {
        assert!(is_image(Path::new("/x/photo.PNG")));
        assert!(!is_image(Path::new("/x/part.stl")));
        assert!(!is_image(Path::new("/x/noext")));
        // A dropped SVG is a model, as Open takes it.
        assert!(!is_image(Path::new("/x/logo.svg")));
    }
}
