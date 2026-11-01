//! Paint's Texture tool: the "Selected" menu that picks a picture (a file, an object's
//! texture, the camera, a QR code or a built-in texture or stencil), and the two ways to
//! put it on an object: a sticker projected where the pointer is, or a brush that tiles it
//! over the faces under the pointer. The maths and the picture registry are in
//! `crate::texture`; the shader draws the live sticker preview (see `render.rs`).

use super::icons::Icon;
use super::paint_menu::{BUTTON, ROW, ROW_DARK};
use super::style;
use super::*;
use crate::render::DecalPreview;
use crate::texture::{self, Decal, Kind};
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

const PANEL_W: f32 = 445.0;
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
        let (rect, opening) = super::paint_menu::selected_header(ui, lang, |p, swatch| {
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

    /// The big menu under the swatch. Returns what was chosen.
    fn texture_menu(
        &mut self,
        ctx: &egui::Context,
        lang: Lang,
        swatch: egui::Rect,
        t: &mut Texture,
        opening: bool,
    ) -> Option<Pick> {
        let camera_ok = *t
            .camera_present
            .get_or_insert_with(crate::webcam::available);
        t.thumbs.kinds(ctx);
        let recent: Vec<(u16, egui::TextureId)> = texture::recent()
            .into_iter()
            .filter_map(|l| t.thumbs.layer(ctx, l).map(|h| (l, h.id())))
            .collect();
        let kinds: Vec<egui::TextureId> = t.thumbs.kinds.iter().map(|h| h.id()).collect();
        let selected = t.layer;
        let mut pick = None;
        let top = ctx.screen_rect().top() + 32.0;
        let area = egui::Area::new(egui::Id::new("texture_menu"))
            .fixed_pos(egui::pos2(swatch.right() + 12.0, top))
            .order(egui::Order::Foreground)
            .fade_in(false)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                let font = egui::FontId::proportional(13.0);
                // The three button rows.
                for (k, (label, enabled)) in [
                    (tr(lang, "Load"), true),
                    (tr(lang, "Texture picker"), true),
                    (tr(lang, "Camera"), camera_ok),
                ]
                .into_iter()
                .enumerate()
                {
                    let sense = if enabled {
                        Sense::click()
                    } else {
                        Sense::hover()
                    };
                    let (r, resp) = ui.allocate_exact_size(Vec2::new(PANEL_W, 49.0), sense);
                    let fill = if enabled && resp.hovered() {
                        Color32::from_gray(0x63)
                    } else {
                        BUTTON
                    };
                    ui.painter().rect_filled(r, egui::CornerRadius::ZERO, fill);
                    ui.painter().text(
                        r.left_center() + Vec2::new(12.0, 0.0),
                        Align2::LEFT_CENTER,
                        label,
                        font.clone(),
                        if enabled {
                            style::TEXT
                        } else {
                            style::TEXT_DIM
                        },
                    );
                    let (sep, _) = ui.allocate_exact_size(Vec2::new(PANEL_W, 2.0), Sense::hover());
                    ui.painter().rect_filled(sep, egui::CornerRadius::ZERO, ROW);
                    if resp.clicked() {
                        pick = Some(match k {
                            0 => Pick::Load,
                            1 => Pick::Picker,
                            _ => Pick::Camera,
                        });
                    }
                }
                if self.texture_qr_block(ui, lang, t) {
                    pick = Some(Pick::Qr);
                }
                // Recent materials, then the texture grid and the stencils.
                let header = |ui: &mut egui::Ui, text: &str| {
                    let (r, _) = ui.allocate_exact_size(Vec2::new(PANEL_W, 41.0), Sense::hover());
                    ui.painter()
                        .rect_filled(r, egui::CornerRadius::ZERO, ROW_DARK);
                    ui.painter().text(
                        r.left_center() + Vec2::new(12.0, 0.0),
                        Align2::LEFT_CENTER,
                        text,
                        font.clone(),
                        style::TEXT,
                    );
                };
                // A picture button: image in `r`, boxed when hovered or in use.
                let swatch_button = |ui: &mut egui::Ui,
                                     r: egui::Rect,
                                     id: egui::TextureId,
                                     k: usize,
                                     on: bool|
                 -> bool {
                    let resp = ui.interact(r, ui.id().with(("texture_cell", k)), Sense::click());
                    ui.painter().image(
                        id,
                        r,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                    if resp.hovered() || on {
                        ui.painter().rect_stroke(
                            r,
                            egui::CornerRadius::ZERO,
                            Stroke::new(2.0_f32, if on { style::TOGGLE_ON } else { style::TEXT }),
                            egui::StrokeKind::Inside,
                        );
                    }
                    resp.clicked()
                };
                header(ui, tr(lang, "Recent materials"));
                if !recent.is_empty() {
                    let (band, _) =
                        ui.allocate_exact_size(Vec2::new(PANEL_W, 52.0), Sense::hover());
                    ui.painter()
                        .rect_filled(band, egui::CornerRadius::ZERO, ROW_DARK);
                    for (k, (layer, id)) in recent.iter().enumerate() {
                        let r = egui::Rect::from_min_size(
                            band.min + Vec2::new(5.0 + k as f32 * 46.0, 6.0),
                            Vec2::splat(40.0),
                        );
                        if swatch_button(ui, r, *id, 100 + k, *layer == selected) {
                            pick = Some(Pick::Layer(*layer));
                        }
                    }
                }
                header(ui, tr(lang, "Texture"));
                let rows = [(0, 4, 4.0), (24, 1, 4.0)];
                for (first, count, pad) in rows {
                    let h = count as f32 * (CELL + GAP) + pad + if first > 0 { 4.0 } else { 0.0 };
                    let (area, _) = ui.allocate_exact_size(Vec2::new(PANEL_W, h), Sense::hover());
                    ui.painter()
                        .rect_filled(area, egui::CornerRadius::ZERO, ROW_DARK);
                    let n = if first == 0 { 24 } else { 6 };
                    for j in 0..n {
                        let r = egui::Rect::from_min_size(
                            area.min
                                + Vec2::new(
                                    5.0 + (j % 6) as f32 * (CELL + GAP),
                                    pad + (j / 6) as f32 * (CELL + GAP),
                                ),
                            Vec2::splat(CELL),
                        );
                        let k = first + j;
                        let layer = if k < 24 {
                            Kind::TEXTURES.get(k)
                        } else {
                            Kind::STENCILS.get(k - 24)
                        }
                        .and_then(|kind| texture::known_layer(*kind));
                        let on = layer == Some(selected) && selected != 0;
                        if swatch_button(ui, r, kinds[k], k, on) {
                            let kind = if k < 24 {
                                Kind::TEXTURES[k]
                            } else {
                                Kind::STENCILS[k - 24]
                            };
                            pick = Some(Pick::Kind(kind));
                        }
                    }
                }
            });
        if pick.is_some() || (!opening && area.response.clicked_elsewhere()) {
            t.menu = false;
        }
        pick
    }

    /// "Generate QR texture for" with its field and button, and the transparency switch.
    /// True when a QR picture was asked for.
    fn texture_qr_block(&mut self, ui: &mut egui::Ui, lang: Lang, t: &mut Texture) -> bool {
        let (block, _) = ui.allocate_exact_size(Vec2::new(PANEL_W, 78.0), Sense::hover());
        ui.painter()
            .rect_filled(block, egui::CornerRadius::ZERO, BUTTON);
        let font = egui::FontId::proportional(13.0);
        let label = ui.painter().layout_no_wrap(
            tr(lang, "Generate QR texture for").to_owned(),
            font.clone(),
            style::TEXT,
        );
        let label_w = label.size().x;
        ui.painter().galley(
            egui::pos2(
                block.left() + 12.0,
                block.top() + 22.0 - label.size().y / 2.0,
            ),
            label,
            style::TEXT,
        );
        let button = egui::Rect::from_min_size(
            egui::pos2(block.right() - 58.0, block.top() + 5.0),
            Vec2::new(48.0, 34.0),
        );
        let field = egui::Rect::from_min_max(
            egui::pos2(block.left() + 20.0 + label_w, block.top() + 5.0),
            egui::pos2(button.left() - 6.0, button.bottom()),
        );
        ui.painter()
            .rect_filled(field, egui::CornerRadius::ZERO, Color32::from_gray(0x3C));
        ui.painter().rect_stroke(
            field,
            egui::CornerRadius::ZERO,
            Stroke::new(2.0_f32, Color32::from_gray(0x9A)),
            egui::StrokeKind::Inside,
        );
        let mut child =
            ui.new_child(egui::UiBuilder::new().max_rect(field.shrink2(Vec2::new(8.0, 3.0))));
        child.visuals_mut().override_text_color = Some(style::TEXT);
        let edit = child.add_sized(
            field.shrink2(Vec2::new(8.0, 3.0)).size(),
            egui::TextEdit::singleline(&mut t.url)
                .frame(false)
                .hint_text(tr(lang, "Insert URL"))
                .font(egui::FontId::proportional(14.0)),
        );
        let hit = ui.interact(button, ui.id().with("qr_ok"), Sense::click());
        ui.painter().rect_filled(
            button,
            egui::CornerRadius::ZERO,
            if hit.hovered() {
                Color32::from_gray(0x77)
            } else {
                Color32::from_gray(0x66)
            },
        );
        super::icons::paint(
            ui.painter(),
            button.shrink2(Vec2::new(12.0, 5.0)),
            Icon::Check,
            style::TEXT,
        );
        let go =
            hit.clicked() || (edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
        let row = egui::Rect::from_min_size(
            egui::pos2(block.left() + 12.0, block.top() + 45.0),
            Vec2::new(PANEL_W - 24.0, 28.0),
        );
        let mut line = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(row)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        line.spacing_mut().item_spacing.x = 12.0;
        line.label(
            egui::RichText::new(tr(lang, "Transparent background"))
                .size(13.0)
                .color(style::TEXT),
        );
        style::switch(
            &mut line,
            &mut t.transparent,
            tr(lang, "On"),
            tr(lang, "Off"),
        );
        go
    }

    /// Carries out a menu choice.
    fn texture_choose(&mut self, t: &mut Texture, pick: Pick, ctx: &egui::Context) {
        let lang = self.settings.lang();
        let layer = match pick {
            Pick::Kind(kind) => texture::layer_of(kind),
            Pick::Layer(l) => Some(l),
            Pick::Load => {
                let Some(path) = rfd::FileDialog::new()
                    .add_filter(tr(lang, "Image"), &crate::image3d::EXTENSIONS)
                    .pick_file()
                else {
                    return;
                };
                match image::open(&path) {
                    Ok(img) => texture::register_image(&img.into_rgba8()),
                    Err(e) => {
                        return self.toast(format!("{} {e}", tr(lang, "Could not open:")), true)
                    }
                }
            }
            Pick::Qr => {
                if t.url.trim().is_empty() {
                    return self.toast(tr(lang, "Type a URL first."), true);
                }
                match texture::qr_image(t.url.trim(), t.transparent) {
                    Some(img) => texture::register_image(&img),
                    None => return self.toast(tr(lang, "The text is too long for a QR."), true),
                }
            }
            Pick::Picker => {
                t.picker = true;
                return self.toast(tr(lang, "Click an object to take its texture."), false);
            }
            Pick::Camera => {
                t.camera = Some(crate::webcam::Webcam::open());
                ctx.request_repaint();
                return;
            }
        };
        match layer {
            Some(l) => {
                t.layer = l;
                t.hint = false;
            }
            None => self.toast(tr(lang, "There are too many pictures loaded."), true),
        }
    }

    /// The camera's first frame becomes the picture.
    fn texture_poll_camera(&mut self, t: &mut Texture, ctx: &egui::Context) {
        let Some(cam) = &t.camera else {
            return;
        };
        let lang = self.settings.lang();
        match cam.latest() {
            Some(Ok((_, frame))) => {
                match texture::register_image(&frame) {
                    Some(l) => {
                        t.layer = l;
                        t.hint = false;
                    }
                    None => self.toast(tr(lang, "There are too many pictures loaded."), true),
                }
                t.camera = None;
            }
            Some(Err(_)) => {
                self.toast(
                    tr(lang, "No camera was found, or it is in use by another app."),
                    true,
                );
                t.camera = None;
            }
            None => ctx.request_repaint_after(Duration::from_millis(100)),
        }
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
    fn texture_decal(&self, hit: (u64, usize, Vec3), layer: u16, toward: Vec3) -> Option<Decal> {
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
                    self.texture_dab(t.layer, hit, new_stroke);
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
    fn texture_place(&mut self, t: &mut Texture, id: u64, decal: Decal, layer: u16) {
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
    fn texture_dab(&mut self, layer: u16, hit: (u64, usize, Vec3), new_stroke: bool) {
        let (id, tri, at) = hit;
        let Some(o) = self.scene.get(id) else {
            return;
        };
        let (mesh, model) = (o.mesh.clone(), o.matrix());
        let faces = texture::brush_faces(
            &mesh,
            &model,
            tri,
            at,
            texture::natural_size(&mesh, &model, 0.08),
        );
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
    fn texture_edit_sticker(&mut self, t: &mut Texture) {
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

/// A state to open the tool in, for screenshots (`--tool texture-menu` and so on).
#[derive(Clone, Copy)]
pub(super) enum Preset {
    Menu,
    Sticker,
    Edit,
    Brush,
}

impl Preset {
    pub fn from_tool(name: &str) -> Self {
        match name {
            "texture-sticker" => Preset::Sticker,
            "texture-edit" => Preset::Edit,
            "texture-brush" => Preset::Brush,
            _ => Preset::Menu,
        }
    }
}

impl App {
    /// The triangle of the first selected (or first) object that faces the camera most, and
    /// the centre of the flat region around it.
    fn texture_facing(&self) -> Option<(u64, usize, Vec3)> {
        let o = self
            .scene
            .selected()
            .next()
            .or(self.scene.objects.first())?;
        let (model, eye) = (o.matrix(), self.camera.eye());
        let centre = |i: usize| {
            o.mesh.triangles[i]
                .map(|v| model.transform_point3(o.mesh.positions[v as usize]))
                .into_iter()
                .sum::<Vec3>()
                / 3.0
        };
        let score = |i: usize| {
            texture::world_normal(&o.mesh, i, &model).dot((eye - centre(i)).normalize_or_zero())
        };
        let tri = (0..o.mesh.triangles.len()).max_by(|a, b| score(*a).total_cmp(&score(*b)))?;
        let flat = o.mesh.face_region(tri, 5.0);
        let at = flat.iter().map(|&i| centre(i)).sum::<Vec3>() / flat.len().max(1) as f32;
        Some((o.id, tri, at))
    }

    /// Sets the tool up as a screenshot asks: menu open, a sticker placed or being edited,
    /// or faces painted.
    pub(super) fn texture_preset(&mut self, preset: Preset) {
        let Some(Tool::Texture(mut t)) = self.tool.take() else {
            return;
        };
        let hit = self.texture_facing();
        match (preset, hit) {
            (Preset::Menu, _) => t.open_menu(),
            (Preset::Sticker | Preset::Edit, Some(hit)) => {
                t.layer = texture::layer_of(Kind::Star8).unwrap_or(t.layer);
                let toward = hit.2 - self.camera.eye();
                if let Some(mut decal) = self.texture_decal(hit, t.layer, toward) {
                    decal.angle = 0.2;
                    let layer = t.layer;
                    self.texture_place(&mut t, hit.0, decal, layer);
                }
                if matches!(preset, Preset::Edit) {
                    self.texture_edit_sticker(&mut t);
                    if let Some(e) = &mut t.editing {
                        e.decal.width *= 1.3;
                        e.decal.height *= 1.3;
                        e.decal.angle = 0.5;
                    }
                }
            }
            (Preset::Brush, Some(hit)) => {
                t.set_brush();
                t.layer = texture::layer_of(Kind::Walnut).unwrap_or(t.layer);
                self.texture_dab(t.layer, hit, true);
                if let Some(o) = self.scene.get(hit.0) {
                    // The highest face too, so the brush shows on more than one side.
                    let model = o.matrix();
                    let top = (0..o.mesh.triangles.len()).max_by(|a, b| {
                        let key = |i: usize| {
                            let n = texture::world_normal(&o.mesh, i, &model);
                            (
                                n.z > 0.9,
                                o.mesh.triangles[i]
                                    .iter()
                                    .map(|&v| {
                                        model.transform_point3(o.mesh.positions[v as usize]).z
                                    })
                                    .sum::<f32>(),
                            )
                        };
                        let (x, y) = (key(*a), key(*b));
                        x.0.cmp(&y.0).then(x.1.total_cmp(&y.1))
                    });
                    if let Some(tri) = top {
                        let at = hit.2;
                        self.texture_dab(t.layer, (hit.0, tri, at), false);
                    }
                }
            }
            _ => {}
        }
        self.tool = Some(Tool::Texture(t));
    }
}
