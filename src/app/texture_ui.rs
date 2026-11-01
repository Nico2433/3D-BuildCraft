//! Paint's Texture tool: the "Selected" menu that picks a picture (a file, an object's
//! texture, the camera, a QR code or a built-in texture or stencil), and the two ways to
//! put it on an object: a sticker projected where the pointer is, or a brush that tiles it
//! over the faces under the pointer. The maths and the picture registry are in
//! `crate::texture`; the shader draws the live sticker preview (see `render.rs`).

use super::icons::Icon;
use super::style;
#[path = "texture_menu.rs"]
mod menu;
#[path = "texture_preset.rs"]
mod preset;
use super::*;
use crate::render::DecalPreview;
use crate::texture::{self, Decal, Kind};
pub(super) use preset::Preset;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode {
    Sticker,
    Brush,
}

/// The last sticker placed: what to put back to take it off, and what it made.
struct Placed {
    id: u64,
    decal: Decal,
    layer: u16,
    before: Arc<Mesh>,
    after: Arc<Mesh>,
}

/// A sticker lifted off its object to be moved, scaled and turned.
struct Editing {
    id: u64,
    decal: Decal,
    layer: u16,
}

/// Small egui copies of the pictures, for the menu and the "Selected" swatch.
#[derive(Default)]
struct Thumbs {
    kinds: Vec<egui::TextureHandle>,
    layers: HashMap<u16, egui::TextureHandle>,
}

const THUMB: u32 = 96;

fn handle(ctx: &egui::Context, name: &str, img: &image::RgbaImage) -> egui::TextureHandle {
    let size = [img.width() as usize, img.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
    ctx.load_texture(name, color, egui::TextureOptions::LINEAR)
}

impl Thumbs {
    /// The built-in pictures in menu order: the textures, then the stencils.
    fn kinds(&mut self, ctx: &egui::Context) -> &[egui::TextureHandle] {
        if self.kinds.is_empty() {
            self.kinds = Kind::TEXTURES
                .into_iter()
                .chain(Kind::STENCILS)
                .map(|k| {
                    let img = crate::texture_gen::generate(k, THUMB);
                    handle(ctx, &format!("texture {k:?}"), &img)
                })
                .collect();
        }
        &self.kinds
    }

    fn layer(&mut self, ctx: &egui::Context, layer: u16) -> Option<&egui::TextureHandle> {
        if let std::collections::hash_map::Entry::Vacant(slot) = self.layers.entry(layer) {
            let img = texture::image(layer)?;
            let small = image::imageops::thumbnail(&*img, THUMB, THUMB);
            slot.insert(handle(ctx, &format!("layer {layer}"), &small));
        }
        self.layers.get(&layer)
    }
}

/// State of the Texture tool.
pub(super) struct Texture {
    /// The scene, undo depth and unsaved flag before the tool, for Cancel.
    pub before: Box<(Snapshot, usize, bool)>,
    /// The picture in use: a registry layer.
    layer: u16,
    mode: Mode,
    /// How far a brush stroke spreads over the connected surface, 0..1, as Paint's.
    coverage: f32,
    menu: bool,
    url: String,
    transparent: bool,
    /// The next click on an object takes its texture.
    picker: bool,
    camera: Option<crate::webcam::Webcam>,
    camera_present: Option<bool>,
    thumbs: Thumbs,
    last: Option<Placed>,
    editing: Option<Editing>,
    /// A drag that began on an object is painting or moving the sticker, not orbiting.
    stroke: bool,
    /// The face the brush last painted, so one stays painted while the pointer is on it.
    last_face: Option<(u64, usize)>,
    /// The notice about how to use the tool, until the first picture is used.
    pub hint: bool,
}

/// What the viewport takes from the tool each frame.
#[derive(Default)]
pub(super) struct TextureFrame {
    /// The pointer is busy on the object, so the camera must not orbit.
    pub grab: bool,
    /// Sticker shown live over an object.
    pub preview: Option<DecalPreview>,
}

/// A choice made in the menu.
enum Pick {
    Load,
    Picker,
    Camera,
    Qr,
    Kind(Kind),
    Layer(u16),
}

impl Texture {
    /// Starts with the first texture selected.
    pub fn new(before: Box<(Snapshot, usize, bool)>) -> Self {
        Self {
            before,
            layer: texture::layer_of(Kind::Paper).unwrap_or(0),
            mode: Mode::Sticker,
            coverage: 1.0,
            menu: false,
            url: String::new(),
            transparent: true,
            picker: false,
            camera: None,
            camera_present: None,
            thumbs: Thumbs::default(),
            last: None,
            editing: None,
            stroke: false,
            last_face: None,
            hint: true,
        }
    }

    /// Opens the menu, for screenshots.
    pub fn open_menu(&mut self) {
        self.menu = true;
    }

    /// Starts in Brush mode, for screenshots.
    pub fn set_brush(&mut self) {
        self.mode = Mode::Brush;
    }

    /// Whether a sticker is being moved: the wheel then scales it instead of zooming.
    pub fn is_editing(&self) -> bool {
        self.editing.is_some()
    }
}

pub(super) const PANEL_W: f32 = 445.0;
const CELL: f32 = 68.0;
const GAP: f32 = 6.0;

impl App {
    /// The bar's settings: the swatch with its menu, the mode pair and the sticker buttons.
    pub(super) fn texture_settings(&mut self, ui: &mut egui::Ui) {
        let Some(Tool::Texture(mut t)) = self.tool.take() else {
            return;
        };
        let lang = self.settings.lang();
        ui.add_space(8.0);
        let current = t.thumbs.layer(ui.ctx(), t.layer).map(|h| h.id());
        let (rect, opening) =
            super::paint_menu::selected_header(ui, tr(lang, "Selected"), |p, swatch| {
                match current {
                    Some(id) => p.image(
                        id,
                        swatch,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        Color32::WHITE,
                    ),
                    None => p.rect_filled(swatch, egui::CornerRadius::ZERO, Color32::GRAY),
                };
            });
        if opening {
            t.menu = !t.menu;
        }
        if t.menu {
            if let Some(pick) = self.texture_menu(ui.ctx(), lang, rect, &mut t, opening) {
                self.texture_choose(&mut t, pick, ui.ctx());
            }
        }
        style::separator(ui, style::FIELD_H, style::BLUE_FIELD_BORDER);
        for (mode, icon, label) in [
            (Mode::Sticker, Icon::Sticker, tr(lang, "Sticker")),
            (Mode::Brush, Icon::Brush, tr(lang, "Brush")),
        ] {
            if style::option(ui, icon, label, t.mode == mode).clicked() && t.mode != mode {
                self.texture_finish_edit(&mut t);
                t.mode = mode;
            }
        }
        style::separator(ui, style::FIELD_H, style::BLUE_FIELD_BORDER);
        if t.mode == Mode::Brush {
            style::label(ui, tr(lang, "Coverage"));
            style::slider(ui, &mut t.coverage, 0.0..=1.0, 148.0, 0);
            self.tool = Some(Tool::Texture(t));
            return;
        }
        let editing = t.editing.is_some();
        if style::option(ui, Icon::Hand, tr(lang, "Edit sticker"), editing).clicked() {
            self.texture_edit_sticker(&mut t);
        }
        let can_remove = editing || t.last.is_some();
        if style::ribbon_button(
            ui,
            style::TOOL_BAR,
            Icon::Delete,
            tr(lang, "Remove sticker"),
            can_remove,
            false,
        )
        .clicked()
        {
            self.texture_remove_sticker(&mut t);
        }
        self.tool = Some(Tool::Texture(t));
    }

    /// Where the object under the pointer is hit: object, triangle and world point.
    fn texture_hit(
        &self,
        resp: &egui::Response,
        rect: egui::Rect,
        aspect: f32,
    ) -> Option<(u64, usize, Vec3)> {
        let p = resp.hover_pos().or(resp.interact_pointer_pos())?;
        let ndc = GVec2::new(
            (p.x - rect.left()) / rect.width() * 2.0 - 1.0,
            1.0 - (p.y - rect.top()) / rect.height() * 2.0,
        );
        let (o, d) = self.camera.ray(ndc, aspect);
        self.pick_hit(o, d)
            .map(|(id, tri, dist)| (id, tri, o + d * dist))
    }

    /// A sticker of the picture on `layer` at the hit, facing out of the surface.
    pub(super) fn texture_decal(
        &self,
        hit: (u64, usize, Vec3),
        layer: u16,
        toward: Vec3,
    ) -> Option<Decal> {
        let (id, tri, at) = hit;
        let o = self.scene.get(id)?;
        let model = o.matrix();
        let mut n = texture::world_normal(&o.mesh, tri, &model);
        if n.dot(toward) > 0.0 {
            n = -n;
        }
        let size = texture::natural_size(&o.mesh, &model, 0.4);
        Some(Decal::new(at, n, size, layer))
    }

    /// One frame of pointer work for the tool: previews, placing, painting, picking.
    pub(super) fn texture_pointer(
        &mut self,
        ui: &egui::Ui,
        resp: &egui::Response,
        rect: egui::Rect,
        aspect: f32,
    ) -> TextureFrame {
        if !matches!(self.tool, Some(Tool::Texture(_))) {
            return TextureFrame::default();
        }
        let Some(Tool::Texture(mut t)) = self.tool.take() else {
            return TextureFrame::default();
        };
        let frame = self.texture_frame(&mut t, ui, resp, rect, aspect);
        self.tool = Some(Tool::Texture(t));
        frame
    }

    fn texture_frame(
        &mut self,
        t: &mut Texture,
        ui: &egui::Ui,
        resp: &egui::Response,
        rect: egui::Rect,
        aspect: f32,
    ) -> TextureFrame {
        self.texture_poll_camera(t, ui.ctx());
        // Undo, redo or an edit elsewhere can take the last sticker away.
        if let Some(p) = &t.last {
            if self
                .scene
                .get(p.id)
                .is_none_or(|o| !Arc::ptr_eq(&o.mesh, &p.after))
            {
                t.last = None;
            }
        }
        let mut frame = TextureFrame::default();
        let toward = {
            let p = resp.hover_pos().unwrap_or(rect.center());
            let ndc = GVec2::new(
                (p.x - rect.left()) / rect.width() * 2.0 - 1.0,
                1.0 - (p.y - rect.top()) / rect.height() * 2.0,
            );
            self.camera.ray(ndc, aspect).1
        };
        let hit = if t.menu {
            None
        } else {
            self.texture_hit(resp, rect, aspect)
        };
        let lang = self.settings.lang();
        let primary = PointerButton::Primary;
        if resp.drag_started_by(primary) && hit.is_some() && !t.menu {
            t.stroke = !t.picker && (t.mode == Mode::Brush || t.editing.is_some());
        }
        if resp.drag_stopped() {
            t.stroke = false;
            t.last_face = None;
        }
        let pressed = resp.clicked_by(primary) || (t.stroke && resp.dragged_by(primary));
        frame.grab = t.stroke;

        if t.picker {
            if let (true, Some((id, tri, _))) = (resp.clicked_by(primary), hit) {
                let tex = self.scene.get(id).and_then(|o| o.mesh.face_tex(tri));
                match tex {
                    Some(f) => {
                        t.layer = f.layer;
                        t.hint = false;
                    }
                    None => self.toast(tr(lang, "That face has no texture."), true),
                }
                t.picker = false;
            }
            return frame;
        }
        if let Some(e) = &mut t.editing {
            // Drag or click moves the sticker; the wheel scales it, with Shift turns it.
            if let (true, Some(h)) = (pressed, hit.filter(|h| h.0 == e.id)) {
                if let Some(d) = self.texture_decal(h, e.layer, toward) {
                    e.decal.center = d.center;
                    e.decal.normal = d.normal;
                }
            }
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if resp.hovered() && scroll != 0.0 {
                if ui.input(|i| i.modifiers.shift) {
                    e.decal.angle += scroll * 0.004;
                } else {
                    let k = (scroll * 0.002).exp().clamp(0.5, 2.0);
                    e.decal.width = (e.decal.width * k).max(0.1);
                    e.decal.height = (e.decal.height * k).max(0.1);
                }
            }
            frame.preview = self.item_of(e.id).map(|item| DecalPreview {
                item,
                decal: e.decal,
                layer: e.layer,
            });
            return frame;
        }
        let Some(hit) = hit else {
            return frame;
        };
        match t.mode {
            Mode::Sticker => {
                let Some(decal) = self.texture_decal(hit, t.layer, toward) else {
                    return frame;
                };
                if resp.clicked_by(primary) {
                    let layer = t.layer;
                    self.texture_place(t, hit.0, decal, layer);
                }
                frame.preview = self.item_of(hit.0).map(|item| DecalPreview {
                    item,
                    decal,
                    layer: t.layer,
                });
            }
            Mode::Brush => {
                if pressed && t.last_face != Some((hit.0, hit.1)) {
                    let new_stroke = t.last_face.is_none();
                    t.last_face = Some((hit.0, hit.1));
                    self.texture_dab(t, hit, new_stroke);
                    t.hint = false;
                }
            }
        }
        frame
    }

    /// The index of object `id` among the frame's draw items.
    fn item_of(&self, id: u64) -> Option<usize> {
        self.scene.objects.iter().position(|o| o.id == id)
    }

    /// Bakes a sticker into the object's mesh as one undo step.
    pub(super) fn texture_place(&mut self, t: &mut Texture, id: u64, decal: Decal, layer: u16) {
        let Some(o) = self.scene.get(id) else {
            return;
        };
        let before = o.mesh.clone();
        let after = Arc::new(texture::apply_decal(&before, &o.matrix(), &decal, layer));
        let put = after.clone();
        self.edit(|s| {
            if let Some(o) = s.get_mut(id) {
                o.mesh = put;
            }
        });
        texture::remember(layer);
        t.hint = false;
        t.last = Some(Placed {
            id,
            decal,
            layer,
            before,
            after,
        });
    }

    /// Tiles the picture over the faces around a brush hit. A stroke is one undo step.
    pub(super) fn texture_dab(&mut self, t: &Texture, hit: (u64, usize, Vec3), new_stroke: bool) {
        let (layer, (id, tri, _)) = (t.layer, hit);
        let Some(o) = self.scene.get(id) else {
            return;
        };
        let (mesh, model) = (o.mesh.clone(), o.matrix());
        // As Paint's coverage: how far the area spreads over the connected surface.
        let faces = mesh.face_region(tri, t.coverage.clamp(0.0, 1.0) * 180.0);
        let tile = texture::natural_size(&mesh, &model, 0.5);
        let out = Arc::new(texture::apply_box(&mesh, &model, &faces, layer, tile));
        texture::remember(layer);
        let set = move |s: &mut Scene| {
            if let Some(o) = s.get_mut(id) {
                o.mesh = out;
            }
        };
        if new_stroke {
            self.edit(set);
        } else {
            set(&mut self.scene);
            self.scene.dirty = true;
        }
    }

    /// Takes the last sticker off its object. False when something changed it since.
    fn texture_revert(&mut self, p: &Placed) -> bool {
        let Some(o) = self.scene.get_mut(p.id) else {
            return false;
        };
        if !Arc::ptr_eq(&o.mesh, &p.after) {
            return false;
        }
        o.mesh = p.before.clone();
        self.history
            .truncate(self.history.depth().saturating_sub(1));
        self.scene.dirty = true;
        true
    }

    /// The "Edit sticker" button: lifts the last sticker to move it, or puts it back.
    pub(super) fn texture_edit_sticker(&mut self, t: &mut Texture) {
        if t.editing.is_some() {
            return self.texture_finish_edit(t);
        }
        let lang = self.settings.lang();
        match t.last.take() {
            Some(p) if self.texture_revert(&p) => {
                t.editing = Some(Editing {
                    id: p.id,
                    decal: p.decal,
                    layer: p.layer,
                });
                t.mode = Mode::Sticker;
            }
            _ => self.toast(tr(lang, "Place a sticker first."), true),
        }
    }

    /// Puts a sticker being edited back on its object.
    fn texture_finish_edit(&mut self, t: &mut Texture) {
        if let Some(e) = t.editing.take() {
            self.texture_place(t, e.id, e.decal, e.layer);
        }
    }

    /// The "Remove sticker" button.
    fn texture_remove_sticker(&mut self, t: &mut Texture) {
        if t.editing.take().is_none() {
            if let Some(p) = t.last.take() {
                self.texture_revert(&p);
            }
        }
    }

    /// Apply: keeps what was done. A sticker still being edited goes back on first.
    pub(super) fn apply_texture(&mut self) {
        if let Some(Tool::Texture(mut t)) = self.tool.take() {
            self.texture_finish_edit(&mut t);
        }
    }
}
