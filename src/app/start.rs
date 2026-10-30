//! The start screen over an empty scene, laid out as the original's: the logo, then a
//! panel of tiles (more information, new scene, open and the last three files). Our own
//! drawings stand in for the original's pictures.

use super::icons::Icon;
use super::style;
use super::*;

const TILE: Color32 = Color32::from_rgb(0x00, 0x78, 0xA6);
const TILE_HOVER: Color32 = Color32::from_rgb(0x1A, 0x8C, 0xB8);
const MORE: Color32 = Color32::from_rgb(0xEA, 0x31, 0x10);
/// Small tile side and the gap between tiles; the big one is two tiles and a gap.
const SIDE: f32 = 150.0;
const GAP: f32 = 8.0;

impl App {
    /// Shown at launch until the scene gets something or the user picks a tile or a tab.
    pub(super) fn start_visible(&self) -> bool {
        self.start_open
            && self.scene.objects.is_empty()
            && self.pending_import.is_empty()
            && self.tool.is_none()
            && self.jobs.is_empty()
    }

    pub(super) fn start_screen(&mut self, ctx: &egui::Context) {
        if !self.start_visible() {
            return;
        }
        let lang = self.settings.lang();
        let view = ctx.available_rect();
        let big = 2.0 * SIDE + GAP;
        let panel = Vec2::new(big + 3.0 * SIDE + 5.0 * GAP + 2.0 * 15.0, big + 30.0);
        // Logo and panel together, centred in the view.
        let block = Vec2::new(panel.x, 100.0 + panel.y);
        let top = view.center() - block / 2.0;
        egui::Area::new(egui::Id::new("start"))
            .fixed_pos(view.min)
            .order(egui::Order::Middle)
            .fade_in(false)
            .show(ctx, |ui| {
                let p = ui.painter().clone();
                // The view behind darkens, as in the original.
                p.rect_filled(
                    view,
                    egui::CornerRadius::ZERO,
                    Color32::from_black_alpha(120),
                );
                logo(&p, top);
                let frame = egui::Rect::from_min_size(top + Vec2::new(0.0, 100.0), panel);
                p.rect_filled(frame, egui::CornerRadius::ZERO, Color32::from_gray(0x1F));
                let at = |col: f32, row: f32| {
                    frame.min
                        + Vec2::new(15.0 + big + GAP, 15.0)
                        + Vec2::new(col, row) * (SIDE + GAP)
                };
                let more =
                    egui::Rect::from_min_size(frame.min + Vec2::splat(15.0), Vec2::splat(big));
                if tile(ui, more, Icon::Cube, tr(lang, "More information"), MORE).clicked() {
                    self.about_open = true;
                }
                let new = egui::Rect::from_min_size(at(0.0, 0.0), Vec2::splat(SIDE));
                if tile(ui, new, Icon::NewScene, tr(lang, "New scene"), TILE).clicked() {
                    self.start_open = false;
                }
                let open = egui::Rect::from_min_size(at(1.0, 0.0), Vec2::new(big, SIDE));
                if tile(ui, open, Icon::Open, tr(lang, "Open"), TILE).clicked() {
                    self.start_open = false;
                    self.actions.push(Action::Open);
                }
                let recent: Vec<_> = self.settings.recent.iter().take(3).cloned().collect();
                for (k, r) in recent.iter().enumerate() {
                    let rect = egui::Rect::from_min_size(at(k as f32, 1.0), Vec2::splat(SIDE));
                    let thumb = self.thumbs.file(&r.path);
                    let resp = ui.allocate_rect(rect, Sense::click());
                    let fill = if resp.hovered() { TILE_HOVER } else { TILE };
                    p.rect_filled(rect, egui::CornerRadius::ZERO, fill);
                    match thumb {
                        Some(id) => {
                            p.image(
                                id,
                                rect.shrink(12.0),
                                egui::Rect::from_min_max(
                                    egui::pos2(0.0, 0.0),
                                    egui::pos2(1.0, 1.0),
                                ),
                                Color32::WHITE,
                            );
                        }
                        None => super::icons::paint(&p, rect.shrink(50.0), Icon::Cube, style::TEXT),
                    }
                    if resp.on_hover_text(r.path.display().to_string()).clicked() {
                        self.start_open = false;
                        self.actions.push(Action::OpenPath(r.path.clone()));
                    }
                }
            });
    }
}

/// A coloured tile with an icon in the middle and its label at the bottom left; the big
/// "more information" tile has its label on a strip instead.
fn tile(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    icon: Icon,
    label: &str,
    fill: Color32,
) -> egui::Response {
    let resp = ui.allocate_rect(rect, Sense::click());
    let p = ui.painter();
    let strip = fill == MORE;
    let body = if strip { TILE } else { fill };
    p.rect_filled(rect, egui::CornerRadius::ZERO, body);
    if resp.hovered() {
        p.rect_filled(
            rect,
            egui::CornerRadius::ZERO,
            Color32::from_white_alpha(18),
        );
    }
    let font = egui::FontId::proportional(style::MENU_FONT);
    if strip {
        let bar = egui::Rect::from_min_max(egui::pos2(rect.left(), rect.bottom() - 35.0), rect.max);
        p.rect_filled(bar, egui::CornerRadius::ZERO, MORE);
        super::icons::paint(
            p,
            rect.shrink(90.0).translate(Vec2::new(0.0, -15.0)),
            icon,
            style::TEXT,
        );
        p.text(
            bar.left_center() + Vec2::new(8.0, 0.0),
            Align2::LEFT_CENTER,
            label,
            font,
            style::TEXT,
        );
    } else {
        let icon_rect =
            egui::Rect::from_center_size(rect.center() - Vec2::new(0.0, 8.0), Vec2::splat(30.0));
        super::icons::paint(p, icon_rect, icon, style::TEXT);
        p.text(
            rect.left_bottom() + Vec2::new(8.0, -16.0),
            Align2::LEFT_CENTER,
            label,
            font,
            style::TEXT,
        );
    }
    resp
}

/// Our own mark: a blue open box in perspective, then the name.
fn logo(p: &egui::Painter, at: Pos2) {
    let blue = Color32::from_rgb(0x1E, 0xA0, 0xE8);
    let pt = |x: f32, y: f32| at + Vec2::new(x, y);
    p.add(egui::Shape::convex_polygon(
        vec![pt(0.0, 18.0), pt(40.0, 6.0), pt(40.0, 74.0), pt(0.0, 62.0)],
        blue,
        Stroke::NONE,
    ));
    p.add(egui::Shape::convex_polygon(
        vec![
            pt(48.0, 6.0),
            pt(76.0, 16.0),
            pt(76.0, 64.0),
            pt(48.0, 74.0),
        ],
        Color32::from_rgb(0x14, 0x78, 0xB4),
        Stroke::NONE,
    ));
    p.text(
        pt(92.0, 40.0),
        Align2::LEFT_CENTER,
        "3D BuildCraft",
        egui::FontId::proportional(46.0),
        Color32::WHITE,
    );
}
