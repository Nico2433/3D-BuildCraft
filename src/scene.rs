//! Scene objects, selection and the edits that act on them.

use crate::cut::Keep;
use crate::io::{ModelObject, Rgba};
use crate::mesh::{Aabb, Mesh};
use glam::{EulerRot, Mat4, Quat, Vec3};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

/// Turquoise, the colour new objects get in the original.
pub const DEFAULT_COLOR: Rgba = [0x00, 0xA0, 0xE8, 0xFF];

/// The first three names and values come from the original's 3MF files; the rest are ours.
pub const PALETTE: [(&str, Rgba); 10] = [
    ("Turquoise", DEFAULT_COLOR),
    ("Yellow", [0xFE, 0xF1, 0x00, 0xFF]),
    ("Green", [0x21, 0xBB, 0x4C, 0xFF]),
    ("Red", [0xE5, 0x1A, 0x2B, 0xFF]),
    ("Orange", [0xF7, 0x8B, 0x1F, 0xFF]),
    ("Purple", [0x86, 0x3B, 0xC9, 0xFF]),
    ("Pink", [0xF2, 0x6D, 0xB4, 0xFF]),
    ("White", [0xF2, 0xF2, 0xF2, 0xFF]),
    ("Gray", [0x8A, 0x8A, 0x8A, 0xFF]),
    ("Black", [0x26, 0x26, 0x26, 0xFF]),
];

/// How an object's surface is drawn (Paint's "Material").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Material {
    /// No override: drawn as Simple.
    #[default]
    None,
    Simple,
    /// Glossy, with a stronger highlight and a sheen at grazing angles.
    Realistic,
    /// See-through: drawn after the opaque objects, without writing depth.
    Translucent,
}

impl Material {
    pub const ALL: [Material; 4] = [
        Material::None,
        Material::Simple,
        Material::Realistic,
        Material::Translucent,
    ];

    /// Its name for agents and files: none, simple, realistic or translucent.
    pub fn name(self) -> &'static str {
        match self {
            Material::None => "none",
            Material::Simple => "simple",
            Material::Realistic => "realistic",
            Material::Translucent => "translucent",
        }
    }

    /// The i18n key of its name.
    pub fn label(self) -> &'static str {
        match self {
            Material::None => "No material",
            Material::Simple => "Simple",
            Material::Realistic => "Realistic",
            Material::Translucent => "Translucent",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Object {
    pub id: u64,
    pub name: String,
    pub mesh: Arc<Mesh>,
    pub translation: Vec3,
    /// Degrees, applied X then Y then Z.
    pub rotation: Vec3,
    pub scale: Vec3,
    pub color: Rgba,
    pub material: Material,
}

impl Object {
    pub fn new(name: impl Into<String>, mesh: Mesh, color: Rgba) -> Self {
        Self {
            id: 0,
            name: name.into(),
            mesh: Arc::new(mesh),
            translation: Vec3::ZERO,
            rotation: Vec3::ZERO,
            scale: Vec3::ONE,
            color,
            material: Material::None,
        }
    }

    pub fn rotation_quat(&self) -> Quat {
        let r = self.rotation * (std::f32::consts::PI / 180.0);
        Quat::from_euler(EulerRot::ZYX, r.z, r.y, r.x)
    }

    /// Turns the object by `q`, in world space, about the point `pivot`.
    pub fn turn_about(&mut self, q: Quat, pivot: Vec3) {
        let (z, y, x) = (q * self.rotation_quat())
            .normalize()
            .to_euler(EulerRot::ZYX);
        self.rotation = Vec3::new(x, y, z) * (180.0 / std::f32::consts::PI);
        self.translation = pivot + q * (self.translation - pivot);
    }

    pub fn matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation_quat(), self.translation)
    }

    pub fn world_aabb(&self) -> Aabb {
        cached_aabb(&self.mesh, &self.matrix())
    }

    /// Takes a file object's placement as translation/rotation/scale. A transform that
    /// cannot be expressed that way (shear) is baked into the mesh instead.
    pub fn from_model(m: ModelObject) -> Self {
        let (scale, rot, t) = m.transform.to_scale_rotation_translation();
        let (z, y, x) = rot.to_euler(EulerRot::ZYX);
        let mut o = Object::new(m.name, m.mesh, m.color);
        o.translation = t;
        o.rotation = Vec3::new(x, y, z) * (180.0 / std::f32::consts::PI);
        o.scale = scale;
        // Compare only the linear part: a large translation must not hide a shear.
        let linear = |m: Mat4| glam::Mat3::from_mat4(m);
        let tol = 1e-4 * scale.abs().max_element().max(1.0);
        if !linear(o.matrix()).abs_diff_eq(linear(m.transform), tol) {
            o.mesh = Arc::new(o.mesh.transformed(&m.transform));
            o.translation = Vec3::ZERO;
            o.rotation = Vec3::ZERO;
            o.scale = Vec3::ONE;
        }
        o
    }

    pub fn to_model(&self) -> ModelObject {
        // Files cannot hold textures, so tiled ones leave as their average colour.
        let mut mesh = (*self.mesh).clone();
        crate::texture::paint_average_colors(&mut mesh);
        ModelObject {
            name: self.name.clone(),
            mesh,
            transform: self.matrix(),
            color: self.color,
        }
    }

    /// Applies the current transform to the vertices and resets it.
    pub fn bake(&mut self) {
        self.set_world_mesh(self.world_mesh());
    }

    /// Replaces the mesh with one already in world space and resets the transform.
    pub fn set_world_mesh(&mut self, mesh: Mesh) {
        self.mesh = Arc::new(mesh);
        self.translation = Vec3::ZERO;
        self.rotation = Vec3::ZERO;
        self.scale = Vec3::ONE;
    }

    /// The world mesh for a boolean whose result takes colour `base`: where this object's
    /// own colour differs, its unpainted faces are painted with it so the result keeps it.
    pub fn world_mesh_for(&self, base: Rgba) -> Mesh {
        let mut mesh = self.world_mesh();
        if self.color != base {
            mesh.colors = (0..mesh.triangles.len())
                .map(|i| mesh.face_color(i).unwrap_or(self.color))
                .collect();
        }
        mesh
    }

    pub fn world_mesh(&self) -> Mesh {
        self.mesh.transformed(&self.matrix())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
    Z,
}

#[derive(Clone, Debug, Default)]
pub struct Scene {
    pub objects: Vec<Object>,
    pub selection: BTreeSet<u64>,
    next_id: u64,
    /// Name shown in the title bar; also the 3MF Title when `info` has none.
    pub name: Option<String>,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    /// "About the model": title, designer, copyright, licence and description.
    pub info: crate::io::ModelInfo,
}

impl Scene {
    /// [`Scene::info`] as files get it: the title falls back to `name`.
    pub fn file_info(&self, name: &str) -> crate::io::ModelInfo {
        let mut info = self.info.clone();
        if info.title.is_empty() {
            info.title = name.to_string();
        }
        info
    }

    pub fn add(&mut self, mut o: Object) -> u64 {
        self.next_id += 1;
        o.id = self.next_id;
        self.objects.push(o);
        self.dirty = true;
        self.next_id
    }

    /// Adds an object centred on the origin and resting on the floor, then selects it.
    pub fn insert_on_floor(&mut self, o: Object) -> u64 {
        let id = self.add(o);
        self.settle(id);
        let obj = self.get_mut(id).unwrap();
        let c = obj.world_aabb().center();
        obj.translation -= Vec3::new(c.x, c.y, 0.0);
        self.selection = BTreeSet::from([id]);
        id
    }

    pub fn get(&self, id: u64) -> Option<&Object> {
        self.objects.iter().find(|o| o.id == id)
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut Object> {
        self.objects.iter_mut().find(|o| o.id == id)
    }

    pub fn selected(&self) -> impl Iterator<Item = &Object> {
        self.objects
            .iter()
            .filter(|o| self.selection.contains(&o.id))
    }

    pub fn selected_ids(&self) -> Vec<u64> {
        self.selection.iter().copied().collect()
    }

    pub fn selection_aabb(&self) -> Aabb {
        self.selected()
            .map(|o| o.world_aabb())
            .fold(Aabb::empty(), Aabb::union)
    }

    /// Radius of a sphere around the selection that stays the same as it turns: the objects'
    /// scaled local bounds, placed round the middle of their centres.
    pub fn selection_radius(&self) -> f32 {
        let spheres: Vec<(Vec3, f32)> = self
            .selected()
            .filter(|o| !o.mesh.is_empty())
            .map(|o| {
                let local = cached_aabb_local(&o.mesh);
                let r = (local.size() * o.scale.abs()).length() * 0.5;
                (o.matrix().transform_point3(local.center()), r)
            })
            .collect();
        let mid = spheres.iter().map(|s| s.0).sum::<Vec3>() / spheres.len().max(1) as f32;
        spheres
            .iter()
            .map(|(c, r)| c.distance(mid) + r)
            .fold(0.0, f32::max)
    }

    pub fn scene_aabb(&self) -> Aabb {
        self.objects
            .iter()
            .map(|o| o.world_aabb())
            .fold(Aabb::empty(), Aabb::union)
    }

    pub fn select_all(&mut self) {
        self.selection = self.objects.iter().map(|o| o.id).collect();
    }

    pub fn invert_selection(&mut self) {
        self.selection = self
            .objects
            .iter()
            .map(|o| o.id)
            .filter(|id| !self.selection.contains(id))
            .collect();
    }

    /// "Settle": put the object's lowest point on the floor.
    pub fn settle(&mut self, id: u64) {
        if let Some(o) = self.get_mut(id) {
            let b = o.world_aabb();
            if b.is_valid() {
                o.translation.z -= b.min.z;
                self.dirty = true;
            }
        }
    }

    /// Drops onto the highest overlapping bounding box below the object's top.
    pub fn drop_onto_scene(&mut self, id: u64) {
        let Some(object) = self.get(id) else { return };
        let b = object.world_aabb();
        if !b.is_valid() {
            return;
        }
        let floor = self
            .objects
            .iter()
            .filter(|o| o.id != id && !self.selection.contains(&o.id))
            .map(Object::world_aabb)
            .filter(|other| {
                other.is_valid()
                    && other.min.x < b.max.x
                    && other.max.x > b.min.x
                    && other.min.y < b.max.y
                    && other.max.y > b.min.y
                    && other.max.z <= b.max.z + 1e-4
            })
            .map(|other| other.max.z)
            .fold(0.0_f32, f32::max);
        self.settle(id);
        self.get_mut(id).unwrap().translation.z += floor;
    }

    pub fn delete_selected(&mut self) {
        self.objects.retain(|o| !self.selection.contains(&o.id));
        self.selection.clear();
        self.dirty = true;
    }

    /// Whether a selected object has several solids; `None` while their statistics are
    /// still being counted off the UI thread.
    pub fn can_ungroup(&self) -> Option<bool> {
        let mut any = false;
        for o in self.selected() {
            any |= cached_stats(&o.mesh)?.solids > 1;
        }
        Some(any)
    }

    fn free_name(&self, base: &str) -> String {
        let mut name = base.to_owned();
        let mut n = 2;
        while self.objects.iter().any(|o| o.name == name) {
            name = format!("{base} {n}");
            n += 1;
        }
        name
    }

    /// Joins world-space meshes without welding or applying a boolean union.
    pub fn group_selected(&mut self) {
        if self.selected().count() < 2 {
            return;
        }
        let name = self.free_name("Group");
        let mut mesh = Mesh::default();
        for o in self.selected() {
            let mut part = o.world_mesh();
            part.colors = (0..part.triangles.len())
                .map(|i| part.face_color(i).unwrap_or(o.color))
                .collect();
            mesh.append(&part);
        }
        self.delete_selected();
        let id = self.add(Object::new(name, mesh, DEFAULT_COLOR));
        self.selection.insert(id);
    }

    /// Splits selected multi-solid objects, preserving world placement and face colours.
    pub fn ungroup_selected(&mut self) {
        let parts: Vec<_> = self
            .selected()
            .filter_map(|o| {
                let solids = o.world_mesh().solids();
                (solids.len() > 1).then(|| (o.id, o.name.clone(), o.color, solids))
            })
            .collect();
        for (id, name, color, solids) in parts {
            self.objects.retain(|o| o.id != id);
            self.selection.remove(&id);
            for mesh in solids {
                let name = self.free_name(&name);
                let id = self.add(Object::new(name, mesh, color));
                self.selection.insert(id);
            }
        }
    }

    /// Copies the selection and places the copies beside it.
    pub fn duplicate_selected(&mut self) {
        let copies: Vec<Object> = self.selected().cloned().collect();
        self.add_copies(&copies, Vec3::X * (self.selection_aabb().size().x + 5.0));
    }

    pub fn paste(&mut self, objects: &[Object]) {
        self.add_copies(objects, Vec3::new(10.0, 10.0, 0.0));
    }

    /// Adds shifted copies and makes them the selection.
    fn add_copies(&mut self, objects: &[Object], offset: Vec3) {
        self.selection.clear();
        for o in objects {
            let mut o = o.clone();
            o.translation += offset;
            let id = self.add(o);
            self.selection.insert(id);
        }
    }

    /// Mirrors each selected object across its own centre along `axis`.
    pub fn mirror_selected(&mut self, axis: Axis) {
        let k = match axis {
            Axis::X => Vec3::new(-1.0, 1.0, 1.0),
            Axis::Y => Vec3::new(1.0, -1.0, 1.0),
            Axis::Z => Vec3::new(1.0, 1.0, -1.0),
        };
        for id in self.selected_ids() {
            let o = self.get_mut(id).unwrap();
            let c = o.world_aabb().center();
            o.bake();
            let m = Mat4::from_translation(c) * Mat4::from_scale(k) * Mat4::from_translation(-c);
            o.mesh = Arc::new(o.mesh.transformed(&m));
        }
        self.dirty = true;
    }

    /// Scales the selection about its centre so its bounding box gets `size`, moving the
    /// objects apart or together too. Rotated objects are baked first so the scale runs
    /// along the world axes the size is measured on.
    pub fn resize_selection(&mut self, size: Vec3) {
        let b = self.selection_aabb();
        let k = size / b.size().max(Vec3::splat(1e-6));
        let centre = b.center();
        for id in self.selected_ids() {
            let o = self.get_mut(id).unwrap();
            if o.rotation != Vec3::ZERO {
                o.bake();
            }
            let c = o.world_aabb().center();
            o.scale *= k;
            o.translation += centre + (c - centre) * k - o.world_aabb().center();
        }
        self.dirty = true;
    }

    /// Scales the whole scene by `k` about the middle of its footprint on the floor (the
    /// print page's design settings). Uniform, so rotations stay as they are.
    pub fn scale_all(&mut self, k: f32) {
        let b = self.scene_aabb();
        if !b.is_valid() || !(k > 0.0 && k.is_finite()) {
            return;
        }
        let pivot = Vec3::new(b.center().x, b.center().y, b.min.z);
        for o in &mut self.objects {
            let c = o.world_aabb().center();
            o.scale *= k;
            o.translation += pivot + (c - pivot) * k - o.world_aabb().center();
        }
        self.dirty = true;
    }

    /// Turns every object back to no rotation.
    pub fn reset_orientation(&mut self) {
        for o in &mut self.objects {
            o.rotation = Vec3::ZERO;
        }
        self.dirty = true;
    }

    /// Takes every object back to scale 1, keeping where its centre is.
    pub fn reset_scale(&mut self) {
        for o in &mut self.objects {
            let c = o.world_aabb().center();
            o.scale = Vec3::ONE;
            o.translation += c - o.world_aabb().center();
        }
        self.dirty = true;
    }

    /// "Split": the pieces `keep` keeps of the selected objects cut by `plane`, in place.
    /// A finite plane cuts with booleans, which fail on a mesh that needs repair. Fails
    /// when the plane misses a selected object, which cutting would delete or copy.
    pub fn cut_pieces(
        &self,
        plane: &crate::cut::CutPlane,
        keep: Keep,
    ) -> anyhow::Result<Vec<Object>> {
        let mut pieces = Vec::new();
        for o in self.selected() {
            let mesh = o.world_mesh();
            // Cutting beside an object would delete it (or copy it, keeping both).
            let name = match self.selection.len() {
                1 => "the object".to_owned(),
                _ => format!("object {} ({})", o.id, o.name),
            };
            if let Some(why) = crate::cut::misses(&mesh, plane, &name) {
                anyhow::bail!("{why}");
            }
            let cut = crate::cut::split(&mesh, plane, keep)?;
            if plane.size.is_some() && crate::cut::unchanged(&mesh, &cut) {
                let side = |axis| {
                    plane
                        .extent(axis)
                        .map_or("full".into(), |e| format!("{e} mm"))
                };
                anyhow::bail!(
                    "the plane's width ({}) and length ({}) leave nothing of {name} to cut; make them larger or move the plane over the object",
                    side(0),
                    side(1)
                );
            }
            for piece in cut {
                let mut piece = Object::new(o.name.clone(), piece, o.color);
                piece.material = o.material;
                pieces.push(piece);
            }
        }
        Ok(pieces)
    }

    /// "Split" at once: the selection becomes the pieces kept; unchanged on failure.
    pub fn cut_selected(&mut self, plane: &crate::cut::CutPlane, keep: Keep) -> anyhow::Result<()> {
        let pieces = self.cut_pieces(plane, keep)?;
        self.replace_selected(pieces);
        Ok(())
    }

    /// Replaces the selected objects with `objects`, which become the selection.
    pub fn replace_selected(&mut self, objects: Vec<Object>) {
        self.delete_selected();
        for o in objects {
            let id = self.add(o);
            self.selection.insert(id);
        }
    }

    pub fn to_models(&self) -> Vec<ModelObject> {
        self.objects.iter().map(Object::to_model).collect()
    }

    pub fn title(&self) -> String {
        self.name.clone().unwrap_or_else(|| "New scene".to_string())
    }
}

use std::sync::{mpsc, Weak};

/// Results computed off the UI thread from an immutable mesh, keyed by its identity
/// (`Arc<Mesh>`). Weak keys do not retain obsolete geometry: the result of a mesh that
/// was edited or deleted meanwhile is dropped with it and never shown for another mesh.
/// A single worker at a time bounds concurrent allocations.
pub struct WorkCache<T> {
    ready: Vec<(Weak<Mesh>, T)>,
    /// The receiver sits in a `Mutex` only to make the cache `Sync` (the renderer holds one).
    pending: Option<(Weak<Mesh>, std::sync::Mutex<mpsc::Receiver<T>>)>,
}

impl<T> Default for WorkCache<T> {
    fn default() -> Self {
        Self {
            ready: Vec::new(),
            pending: None,
        }
    }
}

impl<T: Clone + Send + 'static> WorkCache<T> {
    /// The result for `mesh` when the worker has finished it; otherwise starts the
    /// worker (if idle) and returns `None`. `on_done` runs on the worker thread after
    /// the result is sent, for example to request a repaint.
    pub fn get(
        &mut self,
        mesh: &Arc<Mesh>,
        compute: fn(&Mesh) -> T,
        on_done: impl FnOnce() + Send + 'static,
    ) -> Option<T> {
        self.ready.retain(|(m, _)| m.strong_count() > 0);
        if let Some((key, mut rx)) = self.pending.take() {
            match rx.get_mut().unwrap_or_else(|e| e.into_inner()).try_recv() {
                Ok(value) => self.ready.push((key, value)),
                Err(mpsc::TryRecvError::Empty) => self.pending = Some((key, rx)),
                Err(mpsc::TryRecvError::Disconnected) => {}
            }
        }
        let key = Arc::downgrade(mesh);
        if let Some((_, value)) = self.ready.iter().find(|(m, _)| Weak::ptr_eq(m, &key)) {
            return Some(value.clone());
        }
        if self.pending.is_none() {
            if self.ready.len() >= 64 {
                self.ready.remove(0);
            }
            let (tx, rx) = mpsc::channel();
            let mesh = mesh.clone();
            std::thread::spawn(move || {
                if tx.send(compute(&mesh)).is_ok() {
                    on_done();
                }
            });
            self.pending = Some((key, std::sync::Mutex::new(rx)));
        }
        None
    }

    /// Forgets the result for `mesh`, for callers that moved it somewhere else.
    pub fn take(&mut self, mesh: &Arc<Mesh>) {
        let key = Arc::downgrade(mesh);
        self.ready.retain(|(m, _)| !Weak::ptr_eq(m, &key));
    }
}

thread_local! { static STATS: std::cell::RefCell<WorkCache<crate::mesh::MeshStats>> = std::cell::RefCell::new(WorkCache::default()); }

thread_local! {
    static LOCAL: std::cell::RefCell<Vec<(std::sync::Weak<Mesh>, Aabb)>> = const { std::cell::RefCell::new(Vec::new()) };
    static BOUNDS: std::cell::RefCell<Vec<(std::sync::Weak<Mesh>, Mat4, Aabb)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Releases rebuildable caches on the calling (UI) thread.
pub fn clear_caches() {
    STATS.with(|c| *c.borrow_mut() = WorkCache::default());
    LOCAL.with(|c| *c.borrow_mut() = Vec::new());
    BOUNDS.with(|c| *c.borrow_mut() = Vec::new());
}

/// Topology statistics are computed off the UI thread (see [`WorkCache`]).
pub fn cached_stats(mesh: &Arc<Mesh>) -> Option<crate::mesh::MeshStats> {
    STATS.with(|cache| cache.borrow_mut().get(mesh, Mesh::stats, || {}))
}

fn cached_aabb_local(mesh: &Arc<Mesh>) -> Aabb {
    LOCAL.with(|cache| {
        let mut cache = cache.borrow_mut();
        cache.retain(|(m, _)| m.strong_count() > 0);
        let key = Arc::downgrade(mesh);
        if let Some((_, b)) = cache.iter().find(|(m, _)| m.ptr_eq(&key)) {
            return *b;
        }
        let bounds = mesh.aabb();
        if cache.len() >= 64 {
            cache.remove(0);
        }
        cache.push((key, bounds));
        bounds
    })
}

/// Exact bounds cached by immutable mesh identity and linear placement. Translation
/// is applied afterwards, so moving an object never scans its vertices again.
/// Weak keys avoid retaining old meshes solely for cached bounds.
pub fn cached_aabb(mesh: &Arc<Mesh>, matrix: &Mat4) -> Aabb {
    if mesh.is_empty() {
        return Aabb::empty();
    }
    use std::sync::Weak;
    let translation = matrix.w_axis.truncate();
    let mut linear = *matrix;
    linear.w_axis = glam::Vec4::W;
    let key = Arc::downgrade(mesh);
    let bounds = BOUNDS.with(|cache| {
        let mut cache = cache.borrow_mut();
        cache.retain(|(m, _, _)| m.strong_count() > 0);
        if let Some((_, _, bounds)) = cache
            .iter()
            .find(|(m, t, _)| Weak::ptr_eq(m, &key) && *t == linear)
        {
            return *bounds;
        }
        let bounds = if linear.x_axis.y == 0.0
            && linear.x_axis.z == 0.0
            && linear.y_axis.x == 0.0
            && linear.y_axis.z == 0.0
            && linear.z_axis.x == 0.0
            && linear.z_axis.y == 0.0
        {
            // Axis-aligned scaling is exact from local bounds, including mirrors.
            let local = cached_aabb_local(mesh);
            let scale = Vec3::new(linear.x_axis.x, linear.y_axis.y, linear.z_axis.z);
            let (a, b) = (local.min * scale, local.max * scale);
            Aabb {
                min: a.min(b),
                max: a.max(b),
            }
        } else {
            mesh.aabb_transformed(&linear)
        };
        if cache.len() >= 64 {
            cache.remove(0);
        }
        cache.push((key, linear, bounds));
        bounds
    });
    Aabb {
        min: bounds.min + translation,
        max: bounds.max + translation,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::Primitive;

    fn shade(m: &Mesh) -> std::sync::Arc<crate::mesh::Shading> {
        std::sync::Arc::new(m.shading(40.0))
    }

    fn wait_for(
        cache: &mut WorkCache<std::sync::Arc<crate::mesh::Shading>>,
        mesh: &Arc<Mesh>,
    ) -> std::sync::Arc<crate::mesh::Shading> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            if let Some(s) = cache.get(mesh, shade, || {}) {
                return s;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the worker never answered"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    #[test]
    fn worker_shading_equals_synchronous_shading() {
        let mesh = Arc::new(Primitive::Sphere.build());
        let mut cache = WorkCache::default();
        // Not ready on the first call: the caller draws the provisional shading.
        let first = cache.get(&mesh, shade, || {});
        let got = first.unwrap_or_else(|| wait_for(&mut cache, &mesh));
        let want = mesh.shading(40.0);
        assert_eq!(got.positions, want.positions);
        assert_eq!(got.normals, want.normals);
        assert_eq!(got.triangles, want.triangles);
        assert_eq!(got.edges, want.edges);
        assert_eq!(got.colors, want.colors);
        cache.take(&mesh);
        assert!(cache.ready.is_empty());
    }

    #[test]
    fn superseded_mesh_result_is_discarded() {
        let mut cache = WorkCache::default();
        let old = Arc::new(Primitive::Sphere.build());
        assert!(cache.get(&old, shade, || {}).is_none());
        // The object is edited or deleted while the worker runs.
        drop(old);
        let new = Arc::new(Primitive::Cube.build());
        let got = wait_for(&mut cache, &new);
        assert_eq!(got.positions, new.shading(40.0).positions);
        // The stale result went with its mesh; only the live one is kept.
        assert!(cache.get(&new, shade, || {}).is_some());
        assert_eq!(cache.ready.len(), 1);
    }

    #[test]
    fn provisional_shading_covers_the_same_triangles() {
        let mesh = Primitive::Sphere.build();
        let p = mesh.provisional_shading();
        assert_eq!(p.triangles, mesh.triangles);
        assert_eq!(p.normals.len(), p.positions.len());
        assert_eq!(p.colors.len(), p.positions.len());
        assert!(p.edges.is_empty());
    }

    #[test]
    fn empty_mesh_world_bounds_remain_invalid() {
        let mut object = Object::new("empty", Mesh::default(), DEFAULT_COLOR);
        object.scale = Vec3::new(0.0, -1.0, 2.0);
        object.translation = Vec3::splat(10.0);
        let bounds = object.world_aabb();
        assert!(!bounds.is_valid());
        assert_eq!(bounds.center(), Vec3::ZERO);
        assert_eq!(bounds.size(), Vec3::ZERO);
    }

    #[test]
    fn group_ungroup_preserves_world_geometry_and_colours() {
        let mut scene = Scene::default();
        for (i, color) in [DEFAULT_COLOR, PALETTE[1].1].into_iter().enumerate() {
            let mut o = Object::new("part", Primitive::Cube.build(), color);
            o.translation = Vec3::new(i as f32 * 80.0, 7.0, -25.0);
            o.rotation.z = 25.0;
            o.scale = Vec3::new(1.0, 2.0, 0.5);
            let id = scene.add(o);
            scene.selection.insert(id);
        }
        let before: Vec<_> = scene.objects.iter().map(Object::world_mesh).collect();
        let ungroupable = |scene: &Scene| loop {
            if let Some(any) = scene.can_ungroup() {
                break any;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        };
        assert!(!ungroupable(&scene));
        scene.group_selected();
        assert_eq!(scene.objects.len(), 1);
        assert_eq!(scene.selection.len(), 1);
        assert_eq!(scene.objects[0].name, "Group");
        assert!(ungroupable(&scene));
        scene.ungroup_selected();
        assert_eq!(scene.objects.len(), 2);
        assert_eq!(scene.selection.len(), 2);
        for (i, (o, original)) in scene.objects.iter().zip(before).enumerate() {
            let mesh = o.world_mesh();
            assert_eq!(mesh.triangles.len(), original.triangles.len());
            assert_eq!(
                mesh.triangle_positions().collect::<Vec<_>>(),
                original.triangle_positions().collect::<Vec<_>>()
            );
            assert!(mesh
                .colors
                .iter()
                .all(|c| *c == [DEFAULT_COLOR, PALETTE[1].1][i]));
        }
    }

    #[test]
    fn ungroup_is_offered_only_when_it_splits() {
        let ungroupable = |mesh: Mesh| {
            let mut scene = Scene::default();
            let id = scene.add(Object::new("part", mesh, DEFAULT_COLOR));
            scene.selection.insert(id);
            let any = loop {
                if let Some(any) = scene.can_ungroup() {
                    break any;
                }
                std::thread::sleep(std::time::Duration::from_millis(1));
            };
            scene.ungroup_selected();
            assert_eq!(scene.objects.len(), if any { 2 } else { 1 });
            any
        };
        // The cavity of a hollowed cube stays with its shell.
        let hollowed = crate::solid::hollow(&Primitive::Cube.build(), 2.0).unwrap();
        assert_eq!(hollowed.stats().components, 2);
        assert!(!ungroupable(hollowed));
        let cube = Primitive::Cube.build();
        let mut two = cube.clone();
        two.append(&cube.transformed(&Mat4::from_translation(Vec3::X * 80.0)));
        assert!(ungroupable(two));
    }

    #[test]
    fn drop_rests_on_highest_support_and_leaves_instant_settle_on_floor() {
        let mut scene = Scene::default();
        let lower =
            scene.insert_on_floor(Object::new("lower", Primitive::Cube.build(), DEFAULT_COLOR));
        let upper = scene.add(Object::new("upper", Primitive::Cube.build(), DEFAULT_COLOR));
        scene.get_mut(upper).unwrap().translation.z = 50.0;
        let support = scene.get(lower).unwrap().world_aabb().max.z;
        scene.selection = BTreeSet::from([upper]);
        scene.drop_onto_scene(upper);
        assert!((scene.get(upper).unwrap().world_aabb().min.z - support).abs() < 1e-4);
        scene.get_mut(upper).unwrap().translation.x = 200.0;
        scene.drop_onto_scene(upper);
        assert!(scene.get(upper).unwrap().world_aabb().min.z.abs() < 1e-4);
        scene.get_mut(upper).unwrap().translation.x = 0.0;
        scene.settle(upper);
        assert!(scene.get(upper).unwrap().world_aabb().min.z.abs() < 1e-4);
    }

    #[test]
    fn cached_bounds_follow_translation_scaling_and_mesh_replacement() {
        let mut o = Object::new("bounds", Primitive::Wedge.build(), DEFAULT_COLOR);
        for rotation in [Vec3::ZERO, Vec3::new(20.0, 40.0, 10.0)] {
            o.rotation = rotation;
            for scale in [Vec3::ONE, Vec3::new(-2.0, 0.5, 3.0)] {
                o.scale = scale;
                for translation in [Vec3::ZERO, Vec3::new(30.0, -12.0, 50.0)] {
                    o.translation = translation;
                    let expected = o.world_mesh().aabb();
                    let actual = o.world_aabb();
                    assert!(actual.min.abs_diff_eq(expected.min, 1e-4));
                    assert!(actual.max.abs_diff_eq(expected.max, 1e-4));
                }
            }
        }
        o.mesh = Arc::new(Primitive::Cube.build());
        assert!(o
            .world_aabb()
            .max
            .abs_diff_eq(o.world_mesh().aabb().max, 1e-4));
    }

    #[test]
    fn inserted_objects_rest_on_the_floor_at_the_centre() {
        let mut s = Scene::default();
        let mut o = Object::new("Cube", Primitive::Cube.build(), DEFAULT_COLOR);
        o.translation = Vec3::new(30.0, 30.0, 12.0);
        let id = s.insert_on_floor(o);
        let b = s.get(id).unwrap().world_aabb();
        assert!(b.min.z.abs() < 1e-4);
        assert!(b.center().truncate().length() < 1e-4);
    }

    #[test]
    fn transform_survives_model_round_trip() {
        let mut o = Object::new("x", Primitive::Cube.build(), DEFAULT_COLOR);
        o.translation = Vec3::new(1.0, 2.0, 3.0);
        o.rotation = Vec3::new(10.0, 20.0, 30.0);
        o.scale = Vec3::new(0.5, 1.0, 2.0);
        let back = Object::from_model(o.to_model());
        assert!(back.matrix().abs_diff_eq(o.matrix(), 1e-4));
    }

    #[test]
    fn shear_is_baked_even_when_far_from_the_origin() {
        let mut t = Mat4::from_translation(Vec3::new(1000.0, 0.0, 0.0));
        t.y_axis.x = 0.5; // shear x by y
        let m = ModelObject {
            name: "s".into(),
            mesh: Primitive::Cube.build(),
            transform: t,
            color: DEFAULT_COLOR,
        };
        let o = Object::from_model(m.clone());
        assert_eq!(o.scale, Vec3::ONE, "shear must be baked, not kept as TRS");
        let expected = m.mesh.transformed(&t).aabb();
        assert!((o.world_aabb().max - expected.max).length() < 1e-3);
    }

    #[test]
    fn resize_scales_rotated_objects_and_their_spacing() {
        let mut s = Scene::default();
        let mut a = Object::new("a", Primitive::Wedge.build(), DEFAULT_COLOR);
        a.rotation.z = 30.0;
        let a = s.add(a);
        let mut b = Object::new("b", Primitive::Cube.build(), DEFAULT_COLOR);
        b.translation.x = 100.0;
        let b = s.add(b);
        s.selection = BTreeSet::from([a, b]);
        let target = s.selection_aabb().size() * Vec3::new(2.0, 1.0, 0.5);
        s.resize_selection(target);
        assert!((s.selection_aabb().size() - target).length() < 1e-2);
    }

    #[test]
    fn cut_keeps_the_chosen_halves_selected() {
        for (keep, count, volume) in [
            (Keep::Both, 2, 64000.0),
            (Keep::Top, 1, 48000.0),
            (Keep::Bottom, 1, 16000.0),
        ] {
            let mut s = Scene::default();
            let id = s.insert_on_floor(Object::new("c", Primitive::Cube.build(), DEFAULT_COLOR));
            s.selection.insert(id);
            s.cut_selected(&crate::cut::CutPlane::at_height(10.0), keep)
                .unwrap();
            assert_eq!((s.objects.len(), s.selection.len()), (count, count));
            let v: f64 = s.objects.iter().map(|o| o.mesh.stats().volume).sum();
            assert!((v - volume).abs() < 1e-2, "{keep:?}: {v}");
        }
    }

    #[test]
    fn mirror_keeps_volume_positive() {
        let mut s = Scene::default();
        let id = s.add(Object::new("w", Primitive::Wedge.build(), DEFAULT_COLOR));
        s.selection.insert(id);
        s.mirror_selected(Axis::X);
        assert!(s.get(id).unwrap().mesh.stats().volume > 0.0);
    }
}
