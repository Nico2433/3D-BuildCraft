//! Main window. Layout follows the original's (vault: Projects/3d-builder/docs/ui):
//! tab bar, ribbon, viewport, selection pane on the right, transform bar at the bottom.

use crate::boolean::Op;
use crate::camera::OrbitCamera;
use crate::cut::{CutPlane, Keep};
use crate::emboss::{Emboss, Pattern, Projection, Shape};
use crate::history::{History, Snapshot};
use crate::i18n::{tr, Lang};
use crate::io::{self, Format, ModelObject, Rgba};
use crate::mesh::{ray_aabb, ray_triangle, Aabb, Mesh};
use crate::primitives::Primitive;
use crate::render::{DrawItem, FrameCallback, Renderer, ViewFlags};
use crate::scene::{Axis, Material, Object, Scene, PALETTE};
use crate::settings::{Autosave, ColorMode, Settings, Units};
use eframe::egui::{
    self, Align2, Color32, Key, KeyboardShortcut, Modifiers, PointerButton, Pos2, Sense, Stroke,
    Vec2,
};
use eframe::egui_wgpu;
use glam::{Vec2 as GVec2, Vec3};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Opacity of a translucent object.
const TRANSLUCENT_ALPHA: u8 = 128;
const VIEWPORT_BG: Color32 = Color32::from_rgb(0x44, 0x44, 0x44);
mod actions;
mod add_dialog;
mod agent;
mod assistant_ui;
mod dialogs;
mod gizmo;
mod icons;
mod jobs;
mod launch;
mod material;
mod paint_menu;
mod picking;
mod power;
mod print;
mod ribbon;
mod selection;
mod settle;
mod side_menu;
mod smooth;
mod start;
mod style;
mod texture_ui;
mod thumbs;
mod tool_bar;
mod top_bar;
mod transform;
mod viewport;

use actions::Action;
pub use launch::Launch;

/// One frame at the 60 fps cap.
const FRAME_TIME: Duration = Duration::from_micros(16_667);
const SOON: &str = "Coming soon";

/// Emboss settings and the one selected object they apply to: id, mesh pointer, transform.
type EmbossKey = (Emboss, Option<(u64, usize, glam::Mat4)>);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Insert,
    Object,
    Edit,
    Paint,
    View,
    Help,
}

impl Tab {
    const ALL: [Tab; 6] = [
        Tab::Insert,
        Tab::Object,
        Tab::Edit,
        Tab::Paint,
        Tab::View,
        Tab::Help,
    ];
    fn label(self) -> &'static str {
        match self {
            Tab::Insert => "Insert",
            Tab::Object => "Object",
            Tab::Edit => "Edit",
            Tab::Paint => "Paint",
            Tab::View => "View",
            Tab::Help => "Help",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TransformMode {
    Move,
    Rotate,
    Scale,
}

// There is only ever one tool, so the size of its largest variant does not matter.
#[allow(clippy::large_enum_variant)]
enum Tool {
    Settle {
        bodies: Vec<settle::Body>,
        before: Box<Checkpoint>,
    },
    Measure {
        points: Vec<Vec3>,
        /// The surface point under the cursor.
        hover: Option<Vec3>,
    },
    Cut {
        plane: CutPlane,
        keep: Keep,
    },
    /// Smoothness 0..1 and whether sharp edges are smoothed too, previewed in the scene.
    Smooth {
        amount: f32,
        edges: bool,
        /// The scene, undo depth and unsaved flag before smoothing, for Cancel.
        before: Box<Checkpoint>,
        run: smooth::Run,
    },
    /// Fraction of faces to remove, 0..1.
    Simplify {
        level: f32,
    },
    /// Minimum wall thickness, mm.
    Hollow {
        wall: f32,
    },
    /// "Custom": a shape built from its settings, previewed in grey.
    Custom {
        shape: Primitive,
        lat: usize,
        lon: usize,
        width: f32,
        height: f32,
        /// 0 (sharp) to 10 (round), as the original's list.
        roundness: u8,
        wire: bool,
        preview: Result<std::sync::Arc<Mesh>, String>,
        built: Option<(Primitive, usize, usize, f32, f32, u8)>,
    },
    /// "Load image": a picture turned into a solid, previewed in grey.
    Image {
        opts: crate::image3d::Options,
        preview: Result<std::sync::Arc<Mesh>, String>,
        built: Option<crate::image3d::Options>,
    },
    /// "Camera": the webcam's live picture, whose snapshot goes to "Load image".
    Camera {
        cam: crate::webcam::Webcam,
        /// The frame on screen and its number.
        tex: Option<(u64, egui::TextureHandle)>,
    },
    /// Colour that clicked areas get; `coverage` 0..1 is how far it spreads over the
    /// connected surface, as an adjacent-normal angle from 0 to 180 degrees.
    Paint {
        color: Rgba,
        coverage: f32,
        /// The scene, undo depth and unsaved flag before painting, for Cancel.
        before: Box<Checkpoint>,
        menu: paint_menu::PaintMenu,
        /// The next click on an object takes its colour instead of painting.
        dropper: bool,
    },
    /// Paint's "Texture": stickers and a brush with a chosen picture.
    Texture(Box<texture_ui::Texture>),
    /// Paint → Material: the style and colour the selected objects take, shown live.
    Material {
        kind: Material,
        color: Rgba,
        /// The scene, undo depth and unsaved flag before the change, for Cancel.
        before: Box<Checkpoint>,
        /// The style and colour the scene shows now.
        shown: (Material, Rgba),
        menu: paint_menu::PaintMenu,
        type_open: bool,
        /// The next click on an object takes its style and colour.
        dropper: bool,
    },
    Emboss {
        params: Emboss,
        /// The text solid for `built`, or why it could not be made.
        preview: Result<std::sync::Arc<Mesh>, String>,
        /// What the preview was made for: the settings and the one selected object (id,
        /// mesh and transform), so undo or a new selection rebuilds it.
        built: Option<EmbossKey>,
    },
}

impl Tool {
    /// A measurement with its points preset, for screenshots: see `Launch::measure`.
    fn measure_preset(spec: &str) -> Self {
        let mut points = vec![];
        let mut hover = None;
        for entry in spec.split(';') {
            let v: Vec<f32> = entry
                .trim_start_matches('~')
                .split(',')
                .filter_map(|n| n.trim().parse().ok())
                .collect();
            if let [x, y, z] = v[..] {
                if entry.starts_with('~') {
                    hover = Some(Vec3::new(x, y, z));
                } else {
                    points.push(Vec3::new(x, y, z));
                }
            }
        }
        Tool::Measure { points, hover }
    }

    /// "Load image" with its default settings.
    fn image(path: PathBuf) -> Self {
        Tool::Image {
            opts: crate::image3d::Options::new(path),
            preview: Err(String::new()),
            built: None,
        }
    }
}

struct PendingImport {
    path: PathBuf,
    objects: Vec<ModelObject>,
    /// The objects' meshes for drawing while the units are chosen.
    preview: Vec<std::sync::Arc<Mesh>>,
    units: Units,
    replace_scene: bool,
    /// Where on the floor it lands: under the pointer for a dropped file, else the centre.
    at: Vec3,
}

impl PendingImport {
    /// Each object's mesh, placement and colour as it will be imported: at the chosen
    /// units, the whole file moved so its footprint is centred on `at` and it rests on
    /// the floor; the objects keep their places relative to each other.
    fn placed(&self) -> impl Iterator<Item = (std::sync::Arc<Mesh>, glam::Mat4, Rgba)> + '_ {
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

    /// The move that puts the whole file on the floor, centred on `at`.
    fn shift(&self) -> Vec3 {
        let b = self
            .objects
            .iter()
            .zip(&self.preview)
            .map(|(o, mesh)| crate::scene::cached_aabb(mesh, &self.model(o)))
            .fold(Aabb::empty(), Aabb::union);
        let c = b.center();
        Vec3::new(self.at.x - c.x, self.at.y - c.y, -b.min.z)
    }
}

/// What Copy and Cut put on the system clipboard, so the window reports the paste.
const CLIP_TEXT: &str = "3D BuildCraft objects";

/// Labels of the job cards, also how a running job is recognised.
const REPAIRING: &str = "Repairing...";
const LOADING: &str = "Loading...";
const SMOOTHING: &str = "Smoothing...";

struct Toast {
    text: String,
    until: Instant,
    error: bool,
}

pub struct App {
    power: power::State,
    picking: picking::Cache,
    fps: bool,
    last_frame: Option<Instant>,
    update_ms: f64,
    scene: Scene,
    history: History,
    settings: Settings,
    autosave: Autosave,
    camera: OrbitCamera,
    tab: Tab,
    mode: TransformMode,
    tool: Option<Tool>,
    clipboard: Vec<Object>,
    /// Files waiting for the units dialog; the first one is shown.
    pending_import: Vec<PendingImport>,
    /// Loading and repairing, running on their own threads.
    jobs: Vec<jobs::Job>,
    /// The Emboss tool is moving (or, with Shift, turning) the pattern.
    emboss_drag: bool,
    /// Dragging one of the selection's handles (move, turn or size).
    handle: Option<viewport::HandleDrag>,
    rotate_angles: [f32; 3],
    rotate_selection: BTreeSet<u64>,
    /// The view moving to frame something: where it started, where it goes, since when.
    glide: Option<(OrbitCamera, OrbitCamera, Instant)>,
    /// Installed font families, loaded when the Emboss panel first needs them.
    font_list: Option<Vec<String>>,
    toasts: Vec<Toast>,
    invalid: BTreeSet<u64>,
    restore: Vec<PathBuf>,
    menu_open: bool,
    add_open: bool,
    /// The start screen, until a tile, a tab or the scene takes over.
    start_open: bool,
    library: add_dialog::LibraryView,
    /// Library folders from `--library`, shown but never saved.
    extra_libraries: Vec<PathBuf>,
    thumbs: thumbs::Thumbs,
    /// The 3D print view and its open page.
    print: Option<print::Page>,
    /// Slicers found on this computer, looked up when the print view opens.
    slicers: Vec<crate::slicer::Slicer>,
    /// The chosen slicer's filament slots, read with the slicers.
    filaments: Vec<crate::slicer::Filament>,
    /// The filament slot picked for a scene colour; other colours use the closest filament.
    filament_of: Vec<(Rgba, usize)>,
    /// Tool calls from the AI assistant: the chat, `--mcp` on stdio, or agents attached
    /// over the local port.
    agent: std::sync::mpsc::Receiver<agent::Request>,
    agent_tx: std::sync::mpsc::Sender<agent::Request>,
    /// The local MCP port while the assistant needs it, or why it could not open.
    listener: Option<Result<crate::mcp::Listener, String>>,
    chat: assistant_ui::ChatState,
    /// Where the screenshot the assistant asked for goes, once it is taken.
    agent_shot: Option<std::sync::mpsc::Sender<crate::mcp::Reply>>,
    print_status_open: bool,
    /// A Design-page edit in progress: the size it started at, the scene before, and
    /// whether its undo step was pushed.
    design_edit: Option<(Vec3, Snapshot, bool)>,
    /// The open tab was clicked again: only the tab bar shows.
    ribbon_hidden: bool,
    items_open: bool,
    settings_open: bool,
    recent_open: bool,
    /// Bottom-bar field shown green: the last one used (0 = X).
    field: usize,
    about_open: bool,
    /// Waits for "save changes?" before running this scene-replacing action.
    confirm: Option<Action>,
    lock_scale: bool,
    /// Where an Alt+drag selection box started.
    marquee: Option<egui::Pos2>,
    /// Text in the bottom bar's editor while it is open.
    bar_edit: Option<String>,
    /// Where the open grid properties flyout hangs from (the Grid button's bottom centre).
    grid_menu: Option<egui::Pos2>,
    /// The size fields show percent of the unscaled size instead of length.
    scale_percent: bool,
    /// Width / height of the viewport in the last frame, for framing outside it.
    aspect: f32,
    /// Where the 3D view is on screen.
    view_rect: egui::Rect,
    actions: Vec<Action>,
    /// `--screenshot`: where to save the window and how many frames to wait first.
    screenshot: Option<(PathBuf, u32)>,
}

/// The scene before an interactive tool started: snapshot, undo depth and dirty flag.
type Checkpoint = (Snapshot, usize, bool);

impl App {
    // ----- edits -------------------------------------------------------------

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            objects: self.scene.objects.clone(),
            selection: self.scene.selection.clone(),
        }
    }

    fn restore_snapshot(&mut self, s: Snapshot) {
        self.scene.objects = s.objects;
        self.scene.selection = s.selection;
        self.scene.dirty = true;
        self.refresh_invalid();
    }

    /// Where an interactive tool (paint, settle, smooth, material, texture) starts.
    fn checkpoint(&self) -> Box<Checkpoint> {
        Box::new((self.snapshot(), self.history.depth(), self.scene.dirty))
    }

    /// Apply: the whole tool session becomes one undo step.
    fn commit(&mut self, before: Checkpoint) {
        let (snapshot, depth, _) = before;
        self.history.truncate(depth);
        self.history.push(snapshot);
    }

    /// Cancel: the scene, undo depth and dirty flag go back to the checkpoint.
    fn roll_back(&mut self, before: Checkpoint) {
        let (snapshot, depth, dirty) = before;
        self.restore_snapshot(snapshot);
        self.history.truncate(depth);
        self.scene.dirty = dirty;
    }

    fn edit(&mut self, f: impl FnOnce(&mut Scene)) {
        let before = self.snapshot();
        f(&mut self.scene);
        self.history.push(before);
        self.scene.dirty = true;
    }

    /// Applies the jobs that finished.
    fn collect_jobs(&mut self) {
        let mut finished = Vec::new();
        self.jobs.retain(|j| match j.try_take() {
            Some(d) => {
                finished.push(d);
                false
            }
            None => true,
        });
        for d in finished {
            match d {
                Some(d) => self.job_done(d),
                None => {
                    let lang = self.settings.lang();
                    self.toast(tr(lang, "The operation failed."), true);
                }
            }
        }
    }

    /// Waits for every running job and applies it: the agent answers only after the work.
    fn wait_jobs(&mut self) {
        for j in std::mem::take(&mut self.jobs) {
            if let Some(d) = j.wait() {
                self.job_done(d);
            }
        }
    }

    fn job_done(&mut self, d: jobs::Done) {
        let lang = self.settings.lang();
        match d {
            jobs::Done::Repaired(meshes) => {
                let total = meshes.len();
                let mut fixed = 0;
                self.edit(|s| {
                    for (id, old, mesh) in meshes {
                        // Objects edited or deleted meanwhile keep what they have now.
                        if let Some(o) = s
                            .get_mut(id)
                            .filter(|o| std::sync::Arc::ptr_eq(&o.mesh, &old))
                        {
                            fixed += usize::from(mesh.stats().is_closed_manifold());
                            o.mesh = std::sync::Arc::new(mesh);
                        }
                    }
                });
                self.invalid.retain(|id| {
                    self.scene
                        .get(*id)
                        .is_some_and(|o| !o.mesh.stats().is_closed_manifold())
                });
                self.toast(
                    format!("{} {fixed}/{total}", tr(lang, "Objects repaired:")),
                    fixed < total,
                );
            }
            jobs::Done::Smoothed { gen, result } => self.smooth_done(gen, result),
            jobs::Done::Loaded {
                path,
                replace_scene,
                model,
                at,
            } => {
                self.queue_import(&path, replace_scene, model, at);
            }
        }
    }

    /// Loads the file on a thread; it then goes on as `load_file` does.
    fn load_file_async(&mut self, path: &Path, replace_scene: bool, ctx: &egui::Context) {
        self.load_file_at(path, replace_scene, Vec3::ZERO, ctx);
    }

    /// Loads the file on a thread, to land at `at` on the floor.
    fn load_file_at(&mut self, path: &Path, replace_scene: bool, at: Vec3, ctx: &egui::Context) {
        let path = path.to_path_buf();
        self.jobs.push(jobs::Job::spawn(LOADING, 0, ctx, move |_| {
            let model = io::load(&path);
            jobs::Done::Loaded {
                path,
                replace_scene,
                model,
                at,
            }
        }));
    }

    /// The point on the floor under a screen position in the view, if it looks at the floor.
    fn floor_point(&self, pos: egui::Pos2) -> Option<Vec3> {
        let r = self.view_rect;
        let ndc = GVec2::new(
            (pos.x - r.left()) / r.width() * 2.0 - 1.0,
            1.0 - (pos.y - r.top()) / r.height() * 2.0,
        );
        let (o, d) = self.camera.ray(ndc, self.aspect);
        let t = -o.z / d.z;
        (d.z.abs() > 1e-6 && t > 0.0 && r.contains(pos)).then(|| o + d * t)
    }

    /// Subtract with one object selected, as in the original: it is cut out of every object
    /// it overlaps and then removed. Pieces that end up apart become separate objects.
    fn subtract_selected(&mut self) {
        let Some(cutter) = self.scene.selected().next() else {
            return;
        };
        let (tool, cb) = (cutter.world_mesh(), cutter.world_aabb());
        let mut cut = Vec::new();
        for o in &self.scene.objects {
            let ob = o.world_aabb();
            if o.id == cutter.id || !(ob.min.cmplt(cb.max).all() && cb.min.cmplt(ob.max).all()) {
                continue;
            }
            let mesh = o.world_mesh();
            let before = mesh.stats().volume;
            match crate::boolean::apply(Op::Subtract, &[mesh, tool.clone()]) {
                // Boxes overlap but the shapes do not: leave the object as it is.
                Ok(m) if (m.stats().volume - before).abs() <= before.abs() * 1e-6 => {}
                Ok(m) => cut.push((o.id, o.name.clone(), o.color, m.solids())),
                Err(e) => {
                    let e = format!("{e:#}");
                    return self.toast(e, true);
                }
            }
        }
        if cut.is_empty() {
            let lang = self.settings.lang();
            return self.toast(
                tr(lang, "The selected object does not overlap any other."),
                true,
            );
        }
        self.edit(|s| {
            s.delete_selected();
            for (id, name, color, parts) in cut {
                s.objects.retain(|o| o.id != id);
                for p in parts.into_iter().filter(|p| !p.is_empty()) {
                    let id = s.add(Object::new(name.clone(), p, color));
                    s.selection.insert(id);
                }
            }
        });
    }

    fn toast(&mut self, text: impl Into<String>, error: bool) {
        self.toasts.push(Toast {
            text: text.into(),
            until: Instant::now() + Duration::from_secs(if error { 8 } else { 4 }),
            error,
        });
    }

    /// Moves the view to frame `b` over a moment, instead of jumping there.
    fn glide_to(&mut self, b: &Aabb) {
        let mut to = self.camera;
        to.frame(b, self.aspect);
        self.glide = Some((self.camera, to, Instant::now()));
    }

    /// One step of the glide; call once a frame.
    fn step_glide(&mut self, ctx: &egui::Context) {
        let Some((from, to, start)) = &self.glide else {
            return;
        };
        let t = (start.elapsed().as_secs_f32() / 0.4).min(1.0);
        // Ease out: fast at first, settling at the end.
        let k = 1.0 - (1.0 - t).powi(3);
        self.camera.target = from.target.lerp(to.target, k);
        self.camera.distance = from.distance + (to.distance - from.distance) * k;
        if t < 1.0 {
            ctx.request_repaint();
        } else {
            self.glide = None;
        }
    }

    /// The scene's name, or "New scene" in the UI language.
    fn scene_title(&self) -> String {
        self.scene
            .name
            .clone()
            .unwrap_or_else(|| tr(self.settings.lang(), "New scene").to_string())
    }

    /// Replaces each selected object with `f` of its world mesh; changes nothing when any
    /// object fails. Returns whether it succeeded.
    fn modify_selected(&mut self, f: impl Fn(&Mesh) -> anyhow::Result<Mesh>) -> bool {
        let done: anyhow::Result<Vec<(u64, Mesh)>> = self
            .scene
            .selected()
            .map(|o| Ok((o.id, f(&o.world_mesh())?)))
            .collect();
        match done {
            Ok(meshes) if meshes.is_empty() => return false,
            Ok(meshes) => self.edit(|s| {
                for (id, mesh) in meshes {
                    s.get_mut(id).unwrap().set_world_mesh(mesh);
                }
            }),
            Err(e) => {
                self.toast(format!("{e:#}"), true);
                return false;
            }
        }
        self.refresh_invalid();
        true
    }

    fn refresh_invalid(&mut self) {
        let ids: BTreeSet<u64> = self.scene.objects.iter().map(|o| o.id).collect();
        self.invalid.retain(|id| ids.contains(id));
    }

    /// Queues the file for import; formats with units import at once. False on error.
    fn load_file(&mut self, path: &Path, replace_scene: bool) -> bool {
        let model = io::load(path);
        self.queue_import(path, replace_scene, model, Vec3::ZERO)
    }

    /// Queues a loaded file for import, or reports why it could not be read. A file added
    /// to the scene waits for Import model (and its units, when the format has none), as
    /// in the original; one that replaces the scene with known units opens at once.
    fn queue_import(
        &mut self,
        path: &Path,
        replace_scene: bool,
        model: anyhow::Result<io::LoadedModel>,
        at: Vec3,
    ) -> bool {
        let lang = self.settings.lang();
        match model {
            Ok(m) => {
                let p = PendingImport {
                    path: path.to_path_buf(),
                    preview: m
                        .objects
                        .iter()
                        .map(|o| std::sync::Arc::new(o.mesh.clone()))
                        .collect(),
                    objects: m.objects,
                    units: Units::Millimeter,
                    replace_scene,
                    at,
                };
                if m.units_known && replace_scene {
                    self.pending_import.insert(0, p);
                    self.finish_import(false);
                } else {
                    let b = p
                        .placed()
                        .map(|(mesh, model, _)| crate::scene::cached_aabb(&mesh, &model))
                        .fold(Aabb::empty(), Aabb::union);
                    self.glide_to(&b);
                    self.pending_import.push(p);
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

    fn finish_import(&mut self, apply_units: bool) {
        if self.pending_import.is_empty() {
            return;
        }
        let p = self.pending_import.remove(0);
        let before = self.snapshot();
        if p.replace_scene {
            self.scene = Scene::default();
            self.history.clear();
            self.scene.name = p
                .path
                .file_stem()
                .and_then(|s| s.to_str())
                .map(str::to_string);
            if Format::from_path(&p.path) == Some(Format::ThreeMf) {
                self.scene.path = Some(p.path.clone());
            }
        }
        // Opening a file keeps its placement; adding one puts it where the preview showed.
        let k = if apply_units { p.units.to_mm() } else { 1.0 };
        let keep = !apply_units && p.objects.len() > 1
            || Format::from_path(&p.path) == Some(Format::ThreeMf);
        let shift = if p.replace_scene && keep {
            None
        } else {
            Some(p.shift())
        };
        let mut new_ids = BTreeSet::new();
        for mut m in p.objects {
            if k != 1.0 {
                m.mesh = m.mesh.transformed(&glam::Mat4::from_scale(Vec3::splat(k)));
                m.transform.w_axis = (m.transform.w_axis.truncate() * k).extend(1.0);
            }
            let stats = m.mesh.stats();
            let mut obj = Object::from_model(m);
            obj.translation += shift.unwrap_or(Vec3::ZERO);
            let id = self.scene.add(obj);
            if !stats.is_closed_manifold() {
                self.invalid.insert(id);
            }
            new_ids.insert(id);
        }
        self.scene.selection = new_ids;
        if !p.replace_scene {
            self.history.push(before);
        } else {
            self.scene.dirty = false;
        }
        self.settings.add_recent(&p.path);
        let _ = self.settings.save();
        self.camera.frame(&self.scene.scene_aabb(), self.aspect);
    }

    fn save(&mut self, choose: bool) {
        let lang = self.settings.lang();
        let target = match (&self.scene.path, choose) {
            (Some(p), false) => Some(p.clone()),
            _ => {
                let mut d =
                    rfd::FileDialog::new().set_file_name(format!("{}.3mf", self.scene_title()));
                for f in Format::SAVE {
                    d = d.add_filter(tr(lang, f.label()), &[f.extension()]);
                }
                d.save_file()
            }
        };
        if let Some(path) = target {
            self.save_to(path);
        }
    }

    fn save_to(&mut self, mut path: PathBuf) {
        let lang = self.settings.lang();
        if Format::from_path(&path).is_none() {
            path.set_extension("3mf");
        }
        let title = path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(str::to_string);
        match io::save(&path, &self.scene.to_models(), title.as_deref()) {
            Ok(()) => {
                if Format::from_path(&path) == Some(Format::ThreeMf) {
                    self.scene.path = Some(path.clone());
                    self.scene.dirty = false;
                }
                self.scene.name = title;
                self.settings.add_recent(&path);
                let _ = self.settings.save();
                self.toast(format!("{} {}", tr(lang, "Saved:"), path.display()), false);
            }
            Err(e) => self.toast(format!("{} {e:#}", tr(lang, "Could not save:")), true),
        }
    }

    // ----- UI ----------------------------------------------------------------

    /// Without a system frame the window resizes from its edges here: the cursor shows
    /// the direction and a press hands the resize to the window manager.
    fn resize_edges(&self, ctx: &egui::Context) {
        use egui::{viewport::ResizeDirection as D, CursorIcon as C, ViewportCommand};
        if ctx.input(|i| i.viewport().maximized.unwrap_or(false)) {
            return;
        }
        let Some(p) = ctx.input(|i| i.pointer.hover_pos()) else {
            return;
        };
        let r = ctx.screen_rect();
        const M: f32 = 5.0;
        let (l, rt, t, b) = (
            p.x < r.left() + M,
            p.x > r.right() - M,
            p.y < r.top() + M,
            p.y > r.bottom() - M,
        );
        let dir = match (l, rt, t, b) {
            (true, _, true, _) => (D::NorthWest, C::ResizeNorthWest),
            (_, true, true, _) => (D::NorthEast, C::ResizeNorthEast),
            (true, _, _, true) => (D::SouthWest, C::ResizeSouthWest),
            (_, true, _, true) => (D::SouthEast, C::ResizeSouthEast),
            (true, ..) => (D::West, C::ResizeWest),
            (_, true, ..) => (D::East, C::ResizeEast),
            (_, _, true, _) => (D::North, C::ResizeNorth),
            (.., true) => (D::South, C::ResizeSouth),
            _ => return,
        };
        ctx.set_cursor_icon(dir.1);
        if ctx.input(|i| i.pointer.primary_pressed()) {
            ctx.send_viewport_cmd(ViewportCommand::BeginResize(dir.0));
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        // Delete or Ctrl+Z while editing a number belong to the field, not the scene.
        if ctx.wants_keyboard_input() {
            return;
        }
        // The Add dialog is modal: Escape closes it and nothing reaches the scene.
        if self.add_open {
            if ctx.input(|i| i.key_pressed(Key::Escape)) {
                self.add_open = false;
            }
            return;
        }
        let cmd = |k| KeyboardShortcut::new(Modifiers::COMMAND, k);
        let shortcuts = [
            (Key::Z, Action::Undo),
            (Key::Y, Action::Redo),
            (Key::S, Action::Save),
            (Key::O, Action::Open),
            (Key::N, Action::New),
            (Key::A, Action::SelectAll),
            (Key::D, Action::Duplicate),
        ];
        ctx.input_mut(|i| {
            for (key, action) in shortcuts {
                if i.consume_shortcut(&cmd(key)) {
                    self.actions.push(action);
                }
            }
            // Ctrl+C, X and V arrive as clipboard events, not as keys.
            for e in &i.events {
                self.actions.extend(match e {
                    egui::Event::Copy => Some(Action::Copy),
                    egui::Event::Cut => Some(Action::Cut),
                    egui::Event::Paste(_) => Some(Action::Paste),
                    _ => None,
                });
            }
            if i.consume_key(Modifiers::NONE, Key::Delete) {
                self.actions.push(Action::Delete);
            }
            if matches!(self.tool, Some(Tool::Settle { .. }))
                && i.consume_key(Modifiers::NONE, Key::Enter)
            {
                self.actions.push(Action::ApplySettle);
            }
            if i.consume_key(Modifiers::NONE, Key::Escape) {
                self.actions.push(Action::CancelTool);
            }
        });
    }
}

impl App {
    /// Saves the window to the `--screenshot` file after a few frames, then quits.
    fn take_screenshot(&mut self, ctx: &egui::Context) {
        let smoothing = matches!(&self.tool, Some(Tool::Smooth { amount, edges, run, .. })
            if !run.is_shown(*amount, *edges));
        let Some((path, frames)) = &mut self.screenshot else {
            return;
        };
        let shot = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(img) = shot {
            let [w, h] = img.size;
            let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
            let saved =
                image::RgbaImage::from_raw(w as u32, h as u32, bytes).map(|i| i.save(&*path));
            match saved {
                Some(Ok(())) => log::info!("screenshot saved to {}", path.display()),
                _ => log::error!("could not save the screenshot to {}", path.display()),
            }
            self.scene.dirty = false;
            self.screenshot = None;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        // Wait for a preview that is still being computed.
        if *frames > 0 && self.jobs.is_empty() && !smoothing {
            *frames -= 1;
            if *frames == 0 {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            }
        }
        ctx.request_repaint();
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if ctx.input(|i| i.viewport().close_requested()) {
            if self.scene.dirty {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.confirm = Some(Action::Quit);
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                return;
            }
        }
        // A pending quit must draw its confirmation even if XWayland still
        // reports the window as minimized while it is being restored.
        if !matches!(self.confirm, Some(Action::Quit)) && self.power_update(ctx, frame) {
            return;
        }
        // Frames are capped at 60 fps instead of waiting for V-Sync; idle draws nothing.
        if let Some(rest) = self
            .last_frame
            .and_then(|last| FRAME_TIME.checked_sub(last.elapsed()))
        {
            std::thread::sleep(rest);
        }
        let started = Instant::now();
        let interval = self
            .last_frame
            .replace(started)
            .map(|last| started.duration_since(last));
        let picking_ready = self.picking.prepare(&self.scene.objects, ctx);
        self.take_screenshot(ctx);
        self.thumbs.collect(ctx);
        self.thumbs.keep_objects(&self.scene.objects);
        self.shortcuts(ctx);
        // Files dropped onto the window are added to the scene.
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        // Pictures open Load image; models land where the pointer let them go.
        let at = ctx
            .input(|i| i.pointer.hover_pos())
            .and_then(|p| self.floor_point(p))
            .unwrap_or(Vec3::ZERO);
        for p in dropped {
            let ext = p
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();
            if crate::image3d::EXTENSIONS.contains(&ext.as_str()) {
                self.tool = Some(Tool::image(p));
            } else {
                self.load_file_at(&p, false, at, ctx);
            }
        }
        let jobs_before = self.jobs.len();
        self.collect_jobs();
        self.smooth_step(ctx);
        self.material_step();
        self.reclaim_after_work(ctx, picking_ready || self.jobs.len() != jobs_before);
        if !self.jobs.is_empty() {
            // Keeps the progress bars moving.
            ctx.request_repaint_after(Duration::from_millis(50));
        }

        self.step_glide(ctx);
        self.resize_edges(ctx);
        self.title_bar(ctx);
        if self.print.is_some() {
            // The 3D print view takes the whole window below the title bar.
            self.print_view(ctx);
            self.viewport(ctx);
        } else {
            self.top_bar(ctx);
            self.ribbon(ctx);
            self.side_menu(ctx);
            self.chat_panel(ctx);
            self.selection_pane(ctx);
            self.viewport(ctx);
            self.start_screen(ctx);
            self.transform_bar(ctx);
            self.add_dialog(ctx);
        }
        self.dialogs(ctx);
        self.print_status(ctx);

        for a in std::mem::take(&mut self.actions) {
            self.apply(a, ctx);
        }
        if self.clamp_cut_plane() {
            ctx.request_repaint();
        }
        self.serve_agent(ctx);
        self.sync_listener(ctx);
        self.chat_events();

        if self.autosave.due(self.scene.dirty) {
            self.autosave.start(self.scene.objects.clone());
        }
        self.update_title(ctx);
        if self.fps {
            egui::Area::new(egui::Id::new("frame_times"))
                .anchor(egui::Align2::RIGHT_BOTTOM, [-12.0, -12.0])
                .order(egui::Order::Tooltip)
                .interactable(false)
                .show(ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.monospace(format!(
                            "CPU {:.2} ms | Δ {:.2} ms",
                            self.update_ms,
                            interval.map_or(0.0, |d| d.as_secs_f64() * 1000.0)
                        ));
                    });
                });
        }
        self.update_ms = started.elapsed().as_secs_f64() * 1000.0;
        if self.scene.dirty {
            // Only unsaved edits need a timer; clean scenes sleep until an event.
            ctx.request_repaint_after(crate::settings::AUTOSAVE_INTERVAL);
        }
    }

    fn on_exit(&mut self) {
        // Stop accepting agent calls and remove the endpoint before other cleanup.
        self.listener = None;
        self.tool = None;
        self.autosave.clear();
        let _ = self.settings.save();
    }
}

fn apply_theme(ctx: &egui::Context, mode: ColorMode) {
    ctx.set_theme(match mode {
        ColorMode::System => egui::ThemePreference::System,
        ColorMode::Light => egui::ThemePreference::Light,
        ColorMode::Dark => egui::ThemePreference::Dark,
    });
}

/// Saves a camera picture where "Load image" can read it. One file, replaced by each
/// capture, so the cache does not grow.
fn save_photo(frame: &image::RgbaImage) -> anyhow::Result<PathBuf> {
    let dir = crate::settings::dirs()
        .map(|d| d.cache_dir().to_path_buf())
        .unwrap_or_else(std::env::temp_dir);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("camera.png");
    frame.save(&path)?;
    Ok(path)
}
