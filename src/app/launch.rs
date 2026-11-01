//! Command-line options and the state they open the window in.

use super::*;

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
    /// Tool to open: cut, simplify, hollow, emboss, measure, paint or texture.
    pub tool: Option<String>,
    /// Opens Load image with this picture.
    pub image: Option<PathBuf>,
    /// Shows this folder as a library, without saving it in the settings.
    pub library: Option<PathBuf>,
    /// Serves the tools to an AI assistant over stdio (MCP).
    pub mcp: bool,
    /// Transform mode to open in: move, rotate or scale.
    pub mode: Option<String>,
    /// A handle drag in progress, for screenshots: `x:60` or `rot-z:45`.
    pub drag: Option<String>,
    /// Measure points `x,y,z;x,y,z`; an entry starting with `~` is the hover point.
    pub measure: Option<String>,
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
            power: power::State::default(),
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
            printers: Vec::new(),
            processes: Vec::new(),
            profiles_job: None,
            filaments: Vec::new(),
            filament_of: Vec::new(),
            agent: agent_rx,
            agent_tx,
            listener: None,
            chat: Default::default(),
            agent_shot: None,
            print_status_open: false,
            printer_status: Vec::new(),
            printer_status_job: None,
            printer_status_at: None,
            design_edit: None,
            ribbon_hidden: false,
            items_open: true,
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
            screenshot: launch.screenshot.clone().map(|p| (p, 10)),
        };
        app.open_launch(launch, &cc.egui_ctx);
        app
    }

    /// Puts the app in the state the options ask for.
    fn open_launch(&mut self, launch: Launch, ctx: &egui::Context) {
        if let Some(p) = launch.file {
            self.load_file(&p, true);
        }
        let tab = launch.tab.as_deref().unwrap_or("insert");
        self.tab = match tab {
            "object" => Tab::Object,
            "edit" => Tab::Edit,
            "paint" => Tab::Paint,
            "view" => Tab::View,
            "help" => Tab::Help,
            _ => Tab::Insert,
        };
        if launch.select_all {
            self.scene.select_all();
        }
        self.settings_open = launch.tool.as_deref() == Some("settings");
        self.menu_open = launch.menu || self.settings_open;
        self.add_open = launch.tool.as_deref() == Some("add");
        self.extra_libraries.extend(launch.library);
        self.mode = match launch.mode.as_deref() {
            Some("rotate") => TransformMode::Rotate,
            Some("scale") => TransformMode::Scale,
            _ => TransformMode::Move,
        };
        // A drag in the Split tool moves its plane, so the tool opens first.
        let cut_now = match launch.tool.as_deref() {
            Some("cut") => launch.drag.is_some(),
            tool => tool == Some("cut-bottom"),
        };
        if cut_now {
            self.apply(Action::StartCut, ctx);
            if let Some(Tool::Cut { keep, .. }) = &mut self.tool {
                if launch.tool.as_deref() == Some("cut-bottom") {
                    *keep = Keep::Bottom;
                }
            }
        }
        if let Some(spec) = &launch.drag {
            self.simulate_drag(spec);
        }
        if let Some(path) = launch.image {
            self.tool = Some(Tool::image(path));
        }
        if let Some(page) = launch
            .tool
            .as_deref()
            .and_then(|t| t.strip_prefix("print-"))
        {
            self.print = print::Page::from_name(page);
            // The status window alone: it looks for the slicer itself.
            if page == "status" {
                self.print_status_open = true;
            } else {
                self.actions.push(Action::DetectSlicers);
            }
        }
        if let Some(z) = launch.zoom {
            ctx.set_zoom_factor(z);
        }
        let start = match launch.tool.as_deref() {
            Some("cut") if !cut_now => Some(Action::StartCut),
            Some("simplify") => Some(Action::StartSimplify),
            Some("hollow") => Some(Action::StartHollow),
            Some("emboss") => Some(Action::StartEmboss),
            Some("measure") if launch.measure.is_none() => Some(Action::Measure),
            Some("settle") => Some(Action::Settle),
            Some("paint" | "paint-colors" | "paint-more") => Some(Action::StartPaint),
            Some(
                "texture" | "texture-menu" | "texture-sticker" | "texture-edit" | "texture-brush"
                | "texture-split",
            ) => Some(Action::StartTexture),
            Some("custom") => Some(Action::StartCustom),
            Some("camera") => Some(Action::StartCamera),
            Some("print") => Some(Action::OpenPrint),
            _ => None,
        };
        self.actions.extend(start);
        // `material`, or for screenshots `material-type` and `material-color` (with that menu
        // open), `material-realistic` and `material-translucent`: all in green.
        if let Some(t) = launch
            .tool
            .as_deref()
            .and_then(|t| t.strip_prefix("material"))
        {
            self.apply(Action::StartMaterial, ctx);
            if let (
                Some(Tool::Material {
                    kind,
                    color,
                    menu,
                    type_open,
                    ..
                }),
                false,
            ) = (&mut self.tool, t.is_empty())
            {
                *color = [0x21, 0xBB, 0x4C, 0xFF];
                match t {
                    "-type" => *type_open = true,
                    "-color" => *menu = paint_menu::PaintMenu::Swatches,
                    "-realistic" => *kind = Material::Realistic,
                    "-translucent" => *kind = Material::Translucent,
                    _ => {}
                }
            }
        }
        // `smooth` or, for screenshots, `smooth-0.6` with the slider at that value.
        if let Some(t) = launch
            .tool
            .as_deref()
            .and_then(|t| t.strip_prefix("smooth"))
        {
            self.apply(Action::StartSmooth, ctx);
            if let Some(Tool::Smooth { amount, .. }) = &mut self.tool {
                *amount = t.trim_start_matches('-').parse().unwrap_or(0.0);
            }
        }
        if let Some(spec) = launch.measure {
            self.tool = Some(Tool::measure_preset(&spec));
        }
        match launch.tool.as_deref() {
            Some("paint-colors") => self.actions.push(Action::PaintMenu(false)),
            Some("paint-more") => self.actions.push(Action::PaintMenu(true)),
            Some(
                t @ ("texture-menu" | "texture-sticker" | "texture-edit" | "texture-brush"
                | "texture-split"),
            ) => self
                .actions
                .push(Action::TexturePreset(texture_ui::Preset::from_tool(t))),
            _ => {}
        }
        // The assistant's chat panel and its settings tab.
        self.chat.open = matches!(
            launch.tool.as_deref(),
            Some("ai" | "chat" | "chat-settings")
        );
        self.chat.settings_tab = matches!(launch.tool.as_deref(), Some("ai" | "chat-settings"));
        // The question asked when closing with unsaved changes.
        if launch.tool.as_deref() == Some("unsaved") {
            self.scene.dirty = true;
            self.confirm = Some(Action::Quit);
        }
        // The bottom bar's editor open on the current field, for screenshots.
        if launch.tool.as_deref() == Some("bar-edit") {
            self.bar_edit = Some("42.998".into());
        }
        if launch.tool.as_deref() == Some("recover") {
            self.restore = vec![PathBuf::from("recover.3mf")];
        }
        if launch.tool.as_deref() == Some("grid") {
            self.tab = Tab::View;
            self.grid_menu = Some(egui::Pos2::ZERO);
        }
        // The repair card half way, for screenshots: a job that only waits.
        if launch.tool.as_deref() == Some("repairing") {
            self.jobs.push(jobs::Job::spawn(REPAIRING, 2, ctx, |done| {
                done.store(1, std::sync::atomic::Ordering::Relaxed);
                std::thread::sleep(Duration::from_secs(60));
                jobs::Done::Repaired(Vec::new())
            }));
        }
    }
}
