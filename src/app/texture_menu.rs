//! The "Selected" menu of the Texture tool: picking a picture from a file, the camera, a QR
//! code or the built-in textures and stencils.

use super::super::paint_menu::{BUTTON, ROW, ROW_DARK};
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
