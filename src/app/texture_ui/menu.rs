//! The "Selected" menu of the Texture tool: picking a picture from a file, the camera, a QR
//! code or the built-in textures and stencils.

use super::super::flyout::{Flyout, Grid, Mark, Menu, BUTTON, ROW_DARK};
use super::*;

impl App {
    /// The big menu under the swatch. Returns what was chosen.
    pub(super) fn texture_menu(
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
        let top = ctx.screen_rect().top() + 32.0;
        let shown = Flyout::new(
            "texture_menu",
            egui::pos2(swatch.right() + 12.0, top),
            PANEL_W,
        )
        .heights(49.0, 41.0)
        .show(ctx, opening, |m| {
            let mut pick = None;
            for (choice, label, enabled) in [
                (Pick::Load, tr(lang, "Load"), true),
                (Pick::Picker, tr(lang, "Texture picker"), true),
                (Pick::Camera, tr(lang, "Camera"), camera_ok),
            ] {
                if m.button_row(label, enabled) {
                    pick = Some(choice);
                }
            }
            if self.texture_qr_block(m, lang, t) {
                pick = Some(Pick::Qr);
            }
            // Recent materials, then the texture grid and the stencils.
            let picture = |ids: &[egui::TextureId]| {
                let ids = ids.to_vec();
                move |p: &egui::Painter, k: usize, r: egui::Rect| {
                    let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                    p.image(ids[k], r, uv, Color32::WHITE);
                }
            };
            m.header(tr(lang, "Recent materials"));
            if !recent.is_empty() {
                let grid = Grid {
                    columns: recent.len(),
                    tile: 40.0,
                    pitch: 46.0,
                    first: Vec2::new(25.0, 26.0),
                    extra: 6.0,
                    fill: ROW_DARK,
                    mark: Mark::Picture,
                };
                let ids: Vec<egui::TextureId> = recent.iter().map(|r| r.1).collect();
                let on = |k: usize| recent[k].0 == selected;
                if let Some(k) = m.tile_grid(recent.len(), &grid, on, picture(&ids)) {
                    pick = Some(Pick::Layer(recent[k].0));
                }
            }
            m.header(tr(lang, "Texture"));
            // The textures, then the stencils a little apart.
            let stencils = Kind::TEXTURES.len();
            for (group, first, extra) in [
                (&Kind::TEXTURES[..], 0, 4.0),
                (&Kind::STENCILS[..], stencils, 8.0),
            ] {
                let grid = Grid {
                    columns: 6,
                    tile: CELL,
                    pitch: CELL + GAP,
                    first: Vec2::splat(5.0 + CELL / 2.0) - Vec2::new(0.0, 1.0),
                    extra,
                    fill: ROW_DARK,
                    mark: Mark::Picture,
                };
                let on =
                    |k: usize| texture::known_layer(group[k]) == Some(selected) && selected != 0;
                if let Some(k) = m.tile_grid(group.len(), &grid, on, picture(&kinds[first..])) {
                    pick = Some(Pick::Kind(group[k]));
                }
            }
            pick
        });
        if shown.inner.is_some() || shown.dismissed {
            t.menu = false;
        }
        shown.inner
    }

    /// "Generate QR texture for" with its field and button, and the transparency switch.
    /// True when a QR picture was asked for.
    fn texture_qr_block(&mut self, m: &mut Menu, lang: Lang, t: &mut Texture) -> bool {
        let block = m.band(78.0, BUTTON);
        let ui = &mut *m.ui;
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
    pub(super) fn texture_choose(&mut self, t: &mut Texture, pick: Pick, ctx: &egui::Context) {
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
    pub(super) fn texture_poll_camera(&mut self, t: &mut Texture, ctx: &egui::Context) {
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
}
