//! The `Action` enum and its dispatch.

use super::*;

#[derive(Clone)]
pub(super) enum Action {
    New,
    Open,
    OpenPath(PathBuf),
    AddFile,
    Save,
    SaveAs,
    Undo,
    Redo,
    Insert(Primitive),
    Group,
    Ungroup,
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
    /// Smooths the selection at once (for the assistant).
    Smooth,
    StartSmooth,
    ApplySmooth,
    ExtrudeDown,
    StartHollow,
    ApplyHollow,
    StartEmboss,
    ApplyEmboss,
    StartPaint,
    StartMaterial,
    ApplyMaterial,
    /// Opens Paint's colour list, or with `true` its "more colours" panel (screenshots).
    PaintMenu(bool),
    ApplyPaint,
    StartTexture,
    ApplyTexture,
    /// Sets the Texture tool up for a screenshot.
    TexturePreset(texture_ui::Preset),
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
    /// Print with this filament slot wherever the scene has this colour.
    ChooseFilament(Rgba, usize),
    /// Print with this printer profile of the slicer.
    ChoosePrinter(String),
    /// Print with this print (process) profile of the slicer.
    ChooseProcess(String),
    /// Saves the scene as 3MF and opens it in the chosen slicer.
    SendToSlicer,
    /// Opens the print-on-paper window.
    OpenPaper,
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

impl App {
    pub(super) fn apply(&mut self, a: Action, ctx: &egui::Context) {
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
            Action::Group => self.edit(Scene::group_selected),
            Action::Ungroup => self.edit(Scene::ungroup_selected),
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
                        before: self.checkpoint(),
                    });
                }
            }
            Action::ApplySettle => {
                if let Some(Tool::Settle { before, .. }) = self.tool.take() {
                    self.commit(*before);
                    self.handle = None;
                }
            }
            Action::SettleInstant => self.edit(|s| {
                for id in s.selected_ids() {
                    s.settle(id);
                }
            }),
            Action::Mirror(axis) => self.edit(|s| s.mirror_selected(axis)),
            Action::Repair => self.start_repair(ctx),
            Action::Boolean(Op::Subtract) if self.scene.selection.len() == 1 => {
                self.subtract_selected();
                self.refresh_invalid();
            }
            Action::Boolean(op) => self.boolean(op),
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
            Action::Measure => {
                self.tool = Some(Tool::Measure {
                    points: vec![],
                    hover: None,
                })
            }
            Action::StartCut => {
                let mut plane = CutPlane::at_height(0.0);
                plane.center = self.scene.selection_aabb().center();
                self.tool = Some(Tool::Cut {
                    plane,
                    keep: Keep::Top,
                });
                // The plane has no size mode.
                if self.mode == TransformMode::Scale {
                    self.mode = TransformMode::Move;
                }
            }
            Action::StartSimplify => self.tool = Some(Tool::Simplify { level: 0.5 }),
            Action::ApplySimplify => {
                if let Some(Tool::Simplify { level }) = self.tool.take() {
                    let mut result = Ok(());
                    self.edit(|s| result = s.simplify_selected(1.0 - level));
                    if result.is_err() {
                        self.toast(
                            tr(
                                self.settings.lang(),
                                "Simplification produced an invalid solid",
                            ),
                            true,
                        );
                    }
                }
            }
            Action::ApplyCut => {
                if let Some(Tool::Cut { plane, keep }) = self.tool.take() {
                    self.edit(|s| s.cut_selected(&plane, keep));
                }
            }
            Action::StartSmooth => {
                if !self.scene.selection.is_empty() {
                    self.tool = Some(Tool::Smooth {
                        amount: 0.0,
                        edges: true,
                        before: self.checkpoint(),
                        run: smooth::Run::new(self.scene.selected_ids(), 0.0, true),
                    });
                }
            }
            Action::ApplySmooth => self.apply_smooth(),
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
                    before: self.checkpoint(),
                    menu: Default::default(),
                    dropper: false,
                });
            }
            Action::StartMaterial => {
                if let Some(o) = self.scene.selected().next() {
                    let kind = match o.material {
                        Material::None => Material::Simple,
                        m => m,
                    };
                    self.tool = Some(Tool::Material {
                        kind,
                        color: o.color,
                        before: self.checkpoint(),
                        shown: (kind, o.color),
                        menu: Default::default(),
                        type_open: false,
                        dropper: false,
                    });
                }
            }
            Action::ApplyMaterial => self.apply_material(),
            Action::PaintMenu(more) => {
                if let Some(Tool::Paint { color, menu, .. } | Tool::Material { color, menu, .. }) =
                    &mut self.tool
                {
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
            Action::StartTexture => {
                self.tool = Some(Tool::Texture(Box::new(texture_ui::Texture::new(
                    self.checkpoint(),
                ))));
            }
            Action::ApplyTexture => self.apply_texture(),
            Action::TexturePreset(preset) => self.texture_preset(preset),
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
                self.load_filaments(ctx);
            }
            Action::BrowseSlicer => {
                if let Some(path) = rfd::FileDialog::new().pick_file() {
                    self.actions.push(Action::ChooseSlicer(path));
                }
            }
            Action::ChooseSlicer(path) => {
                if self.settings.slicer.as_ref() != Some(&path) {
                    self.settings.printer = None;
                    self.settings.process = None;
                }
                self.settings.slicer = Some(path);
                let _ = self.settings.save();
                self.load_filaments(ctx);
            }
            Action::ChoosePrinter(printer) => {
                // Processes depend on the printer.
                if self.settings.printer.as_ref() != Some(&printer) {
                    self.settings.process = None;
                }
                self.settings.printer = Some(printer);
                let _ = self.settings.save();
                self.load_filaments(ctx);
            }
            Action::ChooseProcess(process) => {
                self.settings.process = Some(process);
                let _ = self.settings.save();
            }
            Action::ChooseFilament(color, slot) => {
                self.filament_of.retain(|(c, _)| *c != color);
                self.filament_of.push((color, slot));
            }
            Action::SendToSlicer => self.send_to_slicer(),
            Action::ShowPrintStatus => self.print_status_open = true,
            Action::OpenPaper => {
                let title = self.scene_title();
                self.paper = Some(paper::Dialog::open(title, &self.scene.objects, ctx));
            }
            Action::PaintArea(id, tri, color, coverage) => {
                self.paint_area(id, tri, color, coverage)
            }
            Action::CancelTool => self.cancel_tool(),
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

    fn start_repair(&mut self, ctx: &egui::Context) {
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

    fn boolean(&mut self, op: Op) {
        // Selection order is scene order, so `Subtract` takes newer objects away
        // from the oldest one.
        let picked: Vec<&Object> = self.scene.selected().collect();
        let (name, color) = (picked[0].name.clone(), picked[0].color);
        let meshes: Vec<_> = picked.iter().map(|o| o.world_mesh_for(color)).collect();
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

    fn cancel_tool(&mut self) {
        // Cancelling an interactive edit restores the scene and undo depth.
        let before = match self.tool.take() {
            Some(
                Tool::Paint { before, .. }
                | Tool::Settle { before, .. }
                | Tool::Smooth { before, .. }
                | Tool::Material { before, .. },
            ) => Some(*before),
            Some(Tool::Texture(t)) => Some(*t.before),
            _ => None,
        };
        if let Some(before) = before {
            self.roll_back(before);
            self.handle = None;
        }
    }
}
