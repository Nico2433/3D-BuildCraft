//! Main window. Layout follows the original's (vault: Projects/3d-builder/docs/ui):
//! tab bar, ribbon, viewport, selection pane on the right, transform bar at the bottom.

use crate::boolean::Op;
use crate::camera::OrbitCamera;
use crate::cut::Keep;
use crate::emboss::{Emboss, Pattern, Projection, Shape};
use crate::history::{History, Snapshot};
use crate::i18n::{tr, Lang};
use crate::io::{self, Format, ModelObject, Rgba};
use crate::mesh::{ray_aabb, ray_triangle, Aabb, Mesh};
use crate::primitives::Primitive;
use crate::render::{DrawItem, FrameCallback, Renderer, ViewFlags};
use crate::scene::{Axis, Object, Scene, PALETTE};
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

const VIEWPORT_BG: Color32 = Color32::from_rgb(0x44, 0x44, 0x44);
mod add_dialog;
mod agent;
mod assistant_ui;
mod dialogs;
mod gizmo;
mod icons;
mod jobs;
mod paint_menu;
mod picking;
mod print;
mod ribbon;
mod selection;
mod settle;
mod side_menu;
mod start;
mod style;
mod thumbs;
mod tool_bar;
mod top_bar;
mod transform;
mod viewport;

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
        before: Box<(Snapshot, usize, bool)>,
    },
    Measure {
        points: Vec<Vec3>,
    },
    Cut {
        z: f32,
        keep: Keep,
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
        before: Box<(Snapshot, usize, bool)>,
        menu: paint_menu::PaintMenu,
        /// The next click on an object takes its colour instead of painting.
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

struct Toast {
    text: String,
    until: Instant,
    error: bool,
}

#[derive(Clone)]
enum Action {
    New,
    Open,
    OpenPath(PathBuf),
    AddFile,
    Save,
    SaveAs,
    Undo,
    Redo,
    Insert(Primitive),
    Duplicate,
    Copy,
    Cut,
    Paste,
    Delete,
    Settle,
    SettleInstant,
    ApplySettle,
    Mirror(Axis),
    Boolean(Op),
    Repair,
    SetColor(Rgba),
    Frame,
    SelectAll,
    SelectNone,
    Invert,
    Select(u64, bool),
    Measure,
    StartCut,
    StartSimplify,
    ApplySimplify,
    ApplyCut,
    Smooth,
    ExtrudeDown,
    StartHollow,
    ApplyHollow,
    StartEmboss,
    ApplyEmboss,
    StartPaint,
    /// Opens Paint's colour list, or with `true` its "more colours" panel (screenshots).
    PaintMenu(bool),
    ApplyPaint,
    StartCustom,
    ApplyCustom,
    StartImage,
    StartCamera,
    /// Saves the camera's picture and opens it in "Load image".
    Capture,
    /// Paints the area around a triangle of an object: (object, triangle, colour, coverage).
    PaintArea(u64, usize, Rgba, f32),
    /// Adds a file to the scene (from the Add dialog's recent files).
    AddPath(PathBuf),
    OpenPrint,
    DetectSlicers,
    BrowseSlicer,
    ChooseSlicer(PathBuf),
    /// Saves the scene as 3MF and opens it in the chosen slicer.
    SendToSlicer,
    ShowPrintStatus,
    CancelTool,
    BeginEdit,
    MoveSelectionTo(Vec3),
    SetRotation(Vec3),
    SetSize(Vec3),
    ImportConfirm,
    ImportCancel,
    Restore(PathBuf),
    DiscardRestore,
    Quit,
}

pub struct App {
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
    /// Tool calls from the AI assistant: the chat, `--mcp` on stdio, or agents attached
    /// over the local port.
    agent: std::sync::mpsc::Receiver<agent::Request>,
    agent_tx: std::sync::mpsc::Sender<agent::Request>,
    /// The local MCP port while the assistant needs it, or why it could not open.
    listener: Option<Result<crate::mcp::Listener, String>>,
    chat: assistant_ui::ChatState,
    /// The assistant's settings page in the menu is open.
    ai_open: bool,
    /// Where the screenshot the assistant asked for goes, once it is taken.
    agent_shot: Option<std::sync::mpsc::Sender<crate::mcp::Reply>>,
    print_status_open: bool,
    /// A Design-page edit in progress: the size it started at, the scene before, and
    /// whether its undo step was pushed.
    design_edit: Option<(Vec3, Snapshot, bool)>,
    /// The open tab was clicked again: only the tab bar shows.
    ribbon_hidden: bool,
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

/// Command-line options. Everything but `file` is for taking screenshots of a known state.
#[derive(Default)]
pub struct Launch {
    pub file: Option<PathBuf>,
    pub screenshot: Option<PathBuf>,
    /// Tab to open: insert, object, edit, paint, view or help.
    pub tab: Option<String>,
    pub lang: Option<Lang>,
    pub select_all: bool,
    pub menu: bool,
    /// UI scale; the original's screenshots are at 1.5.
    pub zoom: Option<f32>,
    /// Tool to open: cut, simplify, hollow, emboss, measure or paint.
    pub tool: Option<String>,
    /// Opens Load image with this picture.
    pub image: Option<PathBuf>,
    /// Shows this folder as a library, without saving it in the settings.
    pub library: Option<PathBuf>,
    /// Serves the tools to an AI assistant over stdio (MCP).
    pub mcp: bool,
    /// Transform mode to open in: move, rotate or scale.
    pub mode: Option<String>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, launch: Launch) -> Self {
        if let Some(rs) = cc.wgpu_render_state.as_ref() {
            let renderer = Renderer::new(&rs.device, rs.target_format);
            rs.renderer.write().callback_resources.insert(renderer);
        }
        let mut settings = Settings::load();
        let (agent_tx, agent_rx) = std::sync::mpsc::channel();
        if let Some(lang) = launch.lang {
            settings.lang = Some(lang);
        }
        apply_theme(&cc.egui_ctx, settings.color_mode);
        style::install_fonts(&cc.egui_ctx);
        let mut app = Self {
            picking: picking::Cache::new(),
            fps: std::env::var("BUILDCRAFT_FPS").is_ok_and(|v| v == "1"),
            last_frame: None,
            update_ms: 0.0,
            scene: Scene::default(),
            history: History::default(),
            settings,
            autosave: Autosave::new(),
            camera: OrbitCamera::default(),
            tab: Tab::Insert,
            mode: TransformMode::Move,
            tool: None,
            clipboard: Vec::new(),
            pending_import: Vec::new(),
            jobs: Vec::new(),
            emboss_drag: false,
            handle: None,
            rotate_angles: [0.0; 3],
            rotate_selection: BTreeSet::new(),
            glide: None,
            font_list: None,
            toasts: Vec::new(),
            invalid: BTreeSet::new(),
            // A screenshot shows the requested state, not a recovery prompt.
            restore: if launch.screenshot.is_some() {
                Vec::new()
            } else {
                Autosave::leftovers()
            },
            menu_open: false,
            add_open: false,
            start_open: launch.screenshot.is_none() || launch.tool.as_deref() == Some("start"),
            library: add_dialog::LibraryView::default(),
            extra_libraries: Vec::new(),
            thumbs: thumbs::Thumbs::new(),
            print: None,
            slicers: Vec::new(),
            agent: agent_rx,
            agent_tx,
            listener: None,
            chat: Default::default(),
            ai_open: false,
            agent_shot: None,
            print_status_open: false,
            design_edit: None,
            ribbon_hidden: false,
            settings_open: false,
            recent_open: false,
            field: 0,
            about_open: false,
            confirm: None,
            lock_scale: false,
            marquee: None,
            bar_edit: None,
            grid_menu: None,
            scale_percent: false,
            aspect: 1.6,
            view_rect: egui::Rect::NOTHING,
            actions: Vec::new(),
            screenshot: launch.screenshot.map(|p| (p, 10)),
        };
        if let Some(p) = launch.file {
            app.load_file(&p, true);
        }
        let tab = launch.tab.as_deref().unwrap_or("insert");
        app.tab = match tab {
            "object" => Tab::Object,
            "edit" => Tab::Edit,
            "paint" => Tab::Paint,
            "view" => Tab::View,
            "help" => Tab::Help,
            _ => Tab::Insert,
        };
        if launch.select_all {
            app.scene.select_all();
        }
        app.menu_open = launch.menu;
        app.add_open = launch.tool.as_deref() == Some("add");
        app.extra_libraries.extend(launch.library);
        app.mode = match launch.mode.as_deref() {
            Some("rotate") => TransformMode::Rotate,
            Some("scale") => TransformMode::Scale,
            _ => TransformMode::Move,
        };
        if launch.mcp {
            agent::serve_stdio(agent::caller(app.agent_tx.clone(), cc.egui_ctx.clone()));
        }
        if let Some(path) = launch.image {
            app.tool = Some(Tool::image(path));
        }
        if let Some(page) = launch
            .tool
            .as_deref()
            .and_then(|t| t.strip_prefix("print-"))
        {
            app.print = print::Page::from_name(page);
        }
        if let Some(z) = launch.zoom {
            cc.egui_ctx.set_zoom_factor(z);
        }
        let start = match launch.tool.as_deref() {
            Some("cut") => Some(Action::StartCut),
            Some("simplify") => Some(Action::StartSimplify),
            Some("hollow") => Some(Action::StartHollow),
            Some("emboss") => Some(Action::StartEmboss),
            Some("measure") => Some(Action::Measure),
            Some("settle") => Some(Action::Settle),
            Some("paint" | "paint-colors" | "paint-more") => Some(Action::StartPaint),
            Some("custom") => Some(Action::StartCustom),
            Some("camera") => Some(Action::StartCamera),
            Some("print") => Some(Action::OpenPrint),
            _ => None,
        };
        app.actions.extend(start);
        match launch.tool.as_deref() {
            Some("paint-colors") => app.actions.push(Action::PaintMenu(false)),
            Some("paint-more") => app.actions.push(Action::PaintMenu(true)),
            _ => {}
        }
        // The assistant's settings page, or its chat.
        app.ai_open = launch.tool.as_deref() == Some("ai");
        app.menu_open |= app.ai_open;
        app.chat.open = launch.tool.as_deref() == Some("chat");
        // The question asked when closing with unsaved changes.
        if launch.tool.as_deref() == Some("unsaved") {
            app.scene.dirty = true;
            app.confirm = Some(Action::Quit);
        }
        // The bottom bar's editor open on the current field, for screenshots.
        if launch.tool.as_deref() == Some("bar-edit") {
            app.bar_edit = Some("42.998".into());
        }
        if launch.tool.as_deref() == Some("recover") {
            app.restore = vec![PathBuf::from("recover.3mf")];
        }
        if launch.tool.as_deref() == Some("grid") {
            app.tab = Tab::View;
            app.grid_menu = Some(egui::Pos2::ZERO);
        }
        // The repair card half way, for screenshots: a job that only waits.
        if launch.tool.as_deref() == Some("repairing") {
            app.jobs
                .push(jobs::Job::spawn(REPAIRING, 2, &cc.egui_ctx, |done| {
                    done.store(1, std::sync::atomic::Ordering::Relaxed);
                    std::thread::sleep(Duration::from_secs(60));
                    jobs::Done::Repaired(Vec::new())
                }));
        }
        app
    }

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

    fn apply(&mut self, a: Action, ctx: &egui::Context) {
        let lang = self.settings.lang();
        let replaces_scene = matches!(
            a,
            Action::New | Action::Open | Action::OpenPath(_) | Action::Quit
        );
        if replaces_scene && self.scene.dirty && self.confirm.is_none() {
            self.confirm = Some(a);
            return;
        }
        match a {
            Action::New => {
                self.scene = Scene::default();
                self.history.clear();
                self.invalid.clear();
                self.camera = OrbitCamera::default();
            }
            action @ (Action::Open | Action::AddFile) => {
                if let Some(p) = rfd::FileDialog::new()
                    .add_filter(tr(lang, "3D models"), &io::OPEN_EXTENSIONS)
                    .pick_file()
                {
                    self.load_file_async(&p, matches!(action, Action::Open), ctx);
                }
            }
            Action::OpenPath(p) => {
                self.load_file_async(&p, true, ctx);
            }
            Action::Save => self.save(false),
            Action::SaveAs => self.save(true),
            Action::Undo => {
                if let Some(s) = self.history.undo(self.snapshot()) {
                    self.restore_snapshot(s);
                }
            }
            Action::Redo => {
                if let Some(s) = self.history.redo(self.snapshot()) {
                    self.restore_snapshot(s);
                }
            }
            Action::Insert(p) => self.edit(|s| {
                s.insert_on_floor(Object::new(
                    tr(lang, p.label()),
                    p.build(),
                    crate::scene::DEFAULT_COLOR,
                ));
            }),
            Action::Duplicate => self.edit(Scene::duplicate_selected),
            action @ (Action::Copy | Action::Cut) => {
                self.clipboard = self.scene.selected().cloned().collect();
                // The window only reports Ctrl+V when the system clipboard holds text.
                ctx.copy_text(CLIP_TEXT.into());
                if matches!(action, Action::Cut) {
                    self.edit(Scene::delete_selected);
                }
            }
            Action::Paste => {
                let clip = self.clipboard.clone();
                self.edit(|s| s.paste(&clip));
            }
            Action::Delete => {
                self.edit(Scene::delete_selected);
                self.refresh_invalid();
            }
            Action::Settle => {
                if !self.scene.selection.is_empty() {
                    self.tool = Some(Tool::Settle {
                        bodies: self
                            .scene
                            .selected_ids()
                            .into_iter()
                            .map(settle::Body::new)
                            .collect(),
                        before: Box::new((self.snapshot(), self.history.depth(), self.scene.dirty)),
                    });
                }
            }
            Action::ApplySettle => {
                if let Some(Tool::Settle { before, .. }) = self.tool.take() {
                    let (snapshot, depth, _) = *before;
                    self.history.truncate(depth);
                    self.history.push(snapshot);
                    self.handle = None;
                }
            }
            Action::SettleInstant => self.edit(|s| {
                for id in s.selected_ids() {
                    s.settle(id);
                }
            }),
            Action::Mirror(axis) => self.edit(|s| s.mirror_selected(axis)),
            Action::Repair => {
                if self.jobs.iter().any(|j| j.label == REPAIRING) {
                    return;
                }
                let meshes: Vec<(u64, std::sync::Arc<Mesh>)> = self
                    .invalid
                    .iter()
                    .filter_map(|id| self.scene.get(*id).map(|o| (*id, o.mesh.clone())))
                    .collect();
                let steps = meshes.len() as u32;
                self.jobs
                    .push(jobs::Job::spawn(REPAIRING, steps, ctx, move |done| {
                        jobs::Done::Repaired(
                            meshes
                                .into_iter()
                                .map(|(id, mesh)| {
                                    let (fixed, _) = crate::repair::repair(&mesh);
                                    done.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                    (id, mesh, fixed)
                                })
                                .collect(),
                        )
                    }));
            }
            Action::Boolean(Op::Subtract) if self.scene.selection.len() == 1 => {
                self.subtract_selected();
                self.refresh_invalid();
            }
            Action::Boolean(op) => {
                // Selection order is scene order, so `Subtract` takes newer objects away
                // from the oldest one.
                let picked: Vec<&Object> = self.scene.selected().collect();
                let meshes: Vec<_> = picked.iter().map(|o| o.world_mesh()).collect();
                let (name, color) = (picked[0].name.clone(), picked[0].color);
                match crate::boolean::apply(op, &meshes) {
                    Ok(mesh) => self.edit(|s| {
                        s.delete_selected();
                        // A subtraction that cuts right through leaves separate objects.
                        let parts = match op {
                            Op::Subtract => mesh.solids(),
                            _ => vec![mesh],
                        };
                        for p in parts.into_iter().filter(|p| !p.is_empty()) {
                            let id = s.add(Object::new(name.clone(), p, color));
                            s.selection.insert(id);
                        }
                    }),
                    Err(e) => self.toast(format!("{e:#}"), true),
                }
                self.refresh_invalid();
            }
            Action::SetColor(c) => self.edit(|s| {
                for id in s.selected_ids() {
                    s.get_mut(id).unwrap().color = c;
                }
            }),
            Action::Frame => {
                let b = if self.scene.selection.is_empty() {
                    self.scene.scene_aabb()
                } else {
                    self.scene.selection_aabb()
                };
                self.glide_to(&b);
            }
            Action::SelectAll => self.scene.select_all(),
            Action::SelectNone => self.scene.selection.clear(),
            Action::Invert => self.scene.invert_selection(),
            Action::Select(id, add) => {
                if !add {
                    self.scene.selection.clear();
                }
                if !self.scene.selection.remove(&id) || !add {
                    self.scene.selection.insert(id);
                }
            }
            Action::Measure => self.tool = Some(Tool::Measure { points: vec![] }),
            Action::StartCut => {
                let z = self.scene.selection_aabb().center().z;
                self.tool = Some(Tool::Cut { z, keep: Keep::Top });
            }
            Action::StartSimplify => self.tool = Some(Tool::Simplify { level: 0.5 }),
            Action::ApplySimplify => {
                if let Some(Tool::Simplify { level }) = self.tool.take() {
                    self.edit(|s| s.simplify_selected(1.0 - level));
                }
            }
            Action::ApplyCut => {
                if let Some(Tool::Cut { z, keep }) = self.tool.take() {
                    self.edit(|s| s.cut_selected(z, keep));
                }
            }
            Action::Smooth => {
                self.modify_selected(crate::solid::smooth);
            }
            Action::ExtrudeDown => {
                self.modify_selected(|m| crate::solid::extrude_down(m, 0.0));
            }
            Action::StartHollow => self.tool = Some(Tool::Hollow { wall: 1.0 }),
            Action::ApplyHollow => {
                // The panel stays open when it fails, so the wall can be changed.
                if let Some(Tool::Hollow { wall }) = self.tool {
                    if self.modify_selected(|m| crate::solid::hollow(m, wall)) {
                        self.tool = None;
                    }
                }
            }
            Action::StartEmboss => {
                self.tool = Some(Tool::Emboss {
                    params: Emboss {
                        text: tr(lang, "Text").to_string(),
                        ..Emboss::default()
                    },
                    preview: Err(String::new()),
                    built: None,
                })
            }
            Action::ApplyEmboss => self.apply_emboss(),
            Action::StartPaint => {
                let color = self
                    .scene
                    .selected()
                    .next()
                    .map_or(crate::scene::DEFAULT_COLOR, |o| o.color);
                self.tool = Some(Tool::Paint {
                    color,
                    coverage: 1.0,
                    before: Box::new((self.snapshot(), self.history.depth(), self.scene.dirty)),
                    menu: Default::default(),
                    dropper: false,
                });
            }
            Action::PaintMenu(more) => {
                if let Some(Tool::Paint { color, menu, .. }) = &mut self.tool {
                    let h = egui::ecolor::Hsva::from_srgb([color[0], color[1], color[2]]);
                    *menu = if more {
                        paint_menu::PaintMenu::More {
                            hsv: [h.h, h.s, h.v],
                            hex: format!("{:02X}{:02X}{:02X}", color[0], color[1], color[2]),
                        }
                    } else {
                        paint_menu::PaintMenu::Swatches
                    };
                }
            }
            Action::ApplyPaint => {
                if let Some(Tool::Paint { color, .. }) = self.tool.take() {
                    self.remember_color(color);
                }
            }
            Action::StartCustom => {
                self.tool = Some(Tool::Custom {
                    shape: Primitive::Cube,
                    lat: 10,
                    lon: 10,
                    width: 40.0,
                    height: 40.0,
                    roundness: 0,
                    wire: true,
                    preview: Err(String::new()),
                    built: None,
                })
            }
            Action::StartImage => {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter(tr(lang, "Image"), &crate::image3d::EXTENSIONS)
                    .pick_file()
                {
                    self.tool = Some(Tool::image(path));
                }
            }
            Action::StartCamera => {
                self.tool = Some(Tool::Camera {
                    cam: crate::webcam::Webcam::open(),
                    tex: None,
                })
            }
            Action::Capture => self.capture_photo(),
            Action::ApplyCustom => self.apply_custom(),
            Action::AddPath(p) => {
                self.load_file_async(&p, false, ctx);
            }
            Action::OpenPrint => {
                self.tool = None;
                self.menu_open = false;
                self.add_open = false;
                self.print = Some(print::Page::Printer);
                self.actions.push(Action::DetectSlicers);
            }
            Action::DetectSlicers => {
                self.slicers = crate::slicer::detect();
                // With none chosen, the first found is used until the user picks one.
                if self.settings.slicer.is_none() {
                    self.settings.slicer = self.slicers.first().map(|s| s.path.clone());
                }
            }
            Action::BrowseSlicer => {
                if let Some(path) = rfd::FileDialog::new().pick_file() {
                    self.settings.slicer = Some(path);
                    let _ = self.settings.save();
                }
            }
            Action::ChooseSlicer(path) => {
                self.settings.slicer = Some(path);
                let _ = self.settings.save();
            }
            Action::SendToSlicer => self.send_to_slicer(),
            Action::ShowPrintStatus => self.print_status_open = true,
            Action::PaintArea(id, tri, color, coverage) => {
                self.paint_area(id, tri, color, coverage)
            }
            Action::CancelTool => {
                // Cancelling an interactive edit restores the scene and undo depth.
                if let Some(Tool::Paint { before, .. } | Tool::Settle { before, .. }) =
                    self.tool.take()
                {
                    let (snapshot, depth, dirty) = *before;
                    self.restore_snapshot(snapshot);
                    self.history.truncate(depth);
                    self.scene.dirty = dirty;
                    self.handle = None;
                }
            }
            Action::BeginEdit => self.history.push(self.snapshot()),
            Action::MoveSelectionTo(center) => {
                let d = center - self.scene.selection_aabb().center();
                for id in self.scene.selected_ids() {
                    self.scene.get_mut(id).unwrap().translation += d;
                }
                self.scene.dirty = true;
            }
            Action::SetRotation(r) => {
                for id in self.scene.selected_ids() {
                    // Turned in place: the box keeps its centre.
                    let o = self.scene.get_mut(id).unwrap();
                    let c = o.world_aabb().center();
                    o.rotation = r;
                    o.translation += c - o.world_aabb().center();
                }
                self.scene.dirty = true;
            }
            Action::SetSize(size) => self.scene.resize_selection(size),
            Action::ImportConfirm => self.finish_import(true),
            Action::ImportCancel => {
                if !self.pending_import.is_empty() {
                    self.pending_import.remove(0);
                }
            }
            Action::Restore(p) => {
                // A recovery file that fails to load is kept for another try.
                if self.load_file(&p, true) {
                    self.scene.name = None;
                    self.scene.path = None;
                    self.scene.dirty = true;
                    let _ = std::fs::remove_file(&p);
                }
                self.restore.retain(|x| x != &p);
            }
            Action::DiscardRestore => {
                for p in self.restore.drain(..) {
                    let _ = std::fs::remove_file(p);
                }
            }
            Action::Quit => {
                self.scene.dirty = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
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
        if *frames > 0 {
            *frames -= 1;
            if *frames == 0 {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            }
        }
        ctx.request_repaint();
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let started = Instant::now();
        let interval = self
            .last_frame
            .replace(started)
            .map(|last| started.duration_since(last));
        self.picking.prepare(&self.scene.objects, ctx);
        self.take_screenshot(ctx);
        self.thumbs.collect(ctx);
        self.thumbs.keep_objects(&self.scene.objects);
        self.shortcuts(ctx);
        if ctx.input(|i| i.viewport().close_requested()) && self.scene.dirty {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm = Some(Action::Quit);
        }
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
        self.collect_jobs();
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
        self.serve_agent(ctx);
        self.sync_listener(ctx);
        self.chat_events();

        if self.autosave.due(self.scene.dirty) {
            self.autosave.start(self.scene.objects.clone());
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!(
            "3D BuildCraft - {}{}",
            self.scene_title(),
            if self.scene.dirty { " •" } else { "" }
        )));
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
            ctx.request_repaint();
        }
        self.update_ms = started.elapsed().as_secs_f64() * 1000.0;
        ctx.request_repaint_after(Duration::from_secs(5));
    }

    fn on_exit(&mut self) {
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
