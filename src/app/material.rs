//! Paint → Material: the tool's Type drop-down, with a preview swatch for each style, and
//! the live change of the selected objects.

use super::icons::Icon;
use super::style;
use super::*;
use eframe::egui::CornerRadius;

/// Height of a row of the Type list.
const ROW_H: f32 = 56.0;

/// A swatch of `color` as `kind` draws it: flat and bevelled, glossy or see-through.
fn swatch(p: &egui::Painter, r: egui::Rect, color: Rgba, kind: Material) {
    let c = Color32::from_rgb(color[0], color[1], color[2]);
    match kind {
        Material::None => {
            // A circle with a slash.
            let (centre, radius) = (r.center(), r.width() * 0.3);
            let stroke = Stroke::new(2.0_f32, style::TEXT);
            p.circle_stroke(centre, radius, stroke);
            let d = radius * std::f32::consts::FRAC_1_SQRT_2;
            p.line_segment([centre - Vec2::splat(d), centre + Vec2::splat(d)], stroke);
        }
        Material::Simple => paint_menu::bevel(p, r, c),
        Material::Realistic => {
            paint_menu::bevel(p, r, c);
            // A window-like glint and a pale edge, as a glossy surface.
            let glint = egui::Rect::from_min_size(r.min, Vec2::new(r.width(), r.height() * 0.45));
            p.rect_filled(glint, CornerRadius::ZERO, Color32::from_white_alpha(48));
            p.circle_filled(
                r.min + r.size() * Vec2::new(0.28, 0.25),
                r.width() * 0.09,
                Color32::from_white_alpha(200),
            );
        }
        Material::Translucent => {
            // Over a light and a dark square, to show it lets the background through.
            for (k, g) in [(0.0, 0xC8), (1.0, 0x50)] {
                let half = egui::Rect::from_min_size(
                    r.min + Vec2::new(r.width() * 0.5 * k, 0.0),
                    Vec2::new(r.width() * 0.5, r.height()),
                );
                p.rect_filled(half, CornerRadius::ZERO, Color32::from_gray(g));
            }
            paint_menu::bevel(
                p,
                r,
                Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), TRANSLUCENT_ALPHA),
            );
        }
    }
}

/// The "Type" drop-down: a field with the current choice that opens a list of the four
/// materials, each with a swatch of `color`. True when the choice changed.
pub(super) fn type_dropdown(
    ui: &mut egui::Ui,
    lang: Lang,
    kind: &mut Material,
    color: Rgba,
    open: &mut bool,
) -> bool {
    let width = 150.0;
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(width, style::FIELD_H), egui::Sense::click());
    style::field_box(ui, rect, resp.hovered() || *open);
    ui.painter().text(
        rect.left_center() + Vec2::new(10.0, 0.0),
        Align2::LEFT_CENTER,
        tr(lang, kind.label()),
        egui::FontId::proportional(style::MENU_FONT),
        style::TEXT,
    );
    let chevron = egui::Rect::from_center_size(
        rect.right_center() - Vec2::new(16.0, 0.0),
        Vec2::splat(14.0),
    );
    super::icons::paint(ui.painter(), chevron, Icon::ChevronDown, style::TEXT);
    let opening = resp.clicked();
    if opening {
        *open = !*open;
    }
    let mut picked = None;
    if *open {
        let size = Vec2::new(width + 76.0, ROW_H * Material::ALL.len() as f32);
        let list = egui::Rect::from_min_size(rect.left_top() - Vec2::new(0.0, 6.0), size);
        let area = egui::Area::new(egui::Id::new("material_type"))
            .fixed_pos(list.min)
            .order(egui::Order::Foreground)
            .fade_in(false)
            .show(ui.ctx(), |ui| {
                ui.allocate_rect(list, egui::Sense::hover());
                ui.painter().rect_filled(
                    list,
                    CornerRadius::ZERO,
                    Color32::from_rgb(0x22, 0x26, 0x2B),
                );
                for (k, m) in Material::ALL.into_iter().enumerate() {
                    let row = egui::Rect::from_min_size(
                        list.min + Vec2::new(0.0, k as f32 * ROW_H),
                        Vec2::new(size.x, ROW_H),
                    );
                    let r = ui.interact(row, ui.id().with(k), egui::Sense::click());
                    let fill = if *kind == m {
                        Color32::from_rgb(0x5A, 0x25, 0x61)
                    } else if r.hovered() {
                        Color32::from_rgb(0x3A, 0x3E, 0x44)
                    } else {
                        Color32::TRANSPARENT
                    };
                    let p = ui.painter();
                    p.rect_filled(row.shrink(1.0), CornerRadius::ZERO, fill);
                    let sw = egui::Rect::from_center_size(
                        row.left_center() + Vec2::new(32.0, 0.0),
                        Vec2::splat(38.0),
                    );
                    swatch(p, sw, color, m);
                    if m != Material::None {
                        p.rect_stroke(
                            sw,
                            CornerRadius::ZERO,
                            Stroke::new(1.0_f32, Color32::from_gray(0x76)),
                            egui::StrokeKind::Outside,
                        );
                    }
                    p.text(
                        row.left_center() + Vec2::new(64.0, 0.0),
                        Align2::LEFT_CENTER,
                        tr(lang, m.label()),
                        egui::FontId::proportional(style::MENU_FONT),
                        style::TEXT,
                    );
                    if r.clicked() {
                        picked = Some(m);
                    }
                }
                ui.painter().rect_stroke(
                    list,
                    CornerRadius::ZERO,
                    Stroke::new(1.0_f32, Color32::from_gray(0x76)),
                    egui::StrokeKind::Inside,
                );
            });
        if picked.is_some() || (area.response.clicked_elsewhere() && !opening) {
            *open = false;
        }
    }
    picked.is_some_and(|m| std::mem::replace(kind, m) != m)
}

impl App {
    /// Shows the tool's type and colour on the selected objects, each as it was before.
    pub(super) fn material_step(&mut self) {
        let Some(Tool::Material {
            kind,
            color,
            before,
            shown,
            ..
        }) = &mut self.tool
        else {
            return;
        };
        if *shown == (*kind, *color) {
            return;
        }
        *shown = (*kind, *color);
        for o in before
            .0
            .objects
            .iter()
            .filter(|o| before.0.selection.contains(&o.id))
        {
            let Some(slot) = self.scene.get_mut(o.id) else {
                continue;
            };
            *slot = o.clone();
            slot.material = *kind;
            if *kind != Material::None {
                slot.color = *color;
                // The whole object takes the colour, not only what was painted.
                if !slot.mesh.colors.is_empty() {
                    let mut mesh = (*slot.mesh).clone();
                    mesh.colors.clear();
                    slot.mesh = std::sync::Arc::new(mesh);
                }
            }
        }
    }

    pub(super) fn apply_material(&mut self) {
        if let Some(Tool::Material { color, before, .. }) = self.tool.take() {
            let (snapshot, depth, _) = *before;
            self.history.truncate(depth);
            self.history.push(snapshot);
            self.scene.dirty = true;
            self.remember_color(color);
        }
    }

    /// The eyedropper: takes the style and colour of the clicked object.
    pub(super) fn pick_material(&mut self, id: u64, tri: usize) {
        let (
            Some(o),
            Some(Tool::Material {
                kind,
                color,
                dropper,
                ..
            }),
        ) = (self.scene.get(id), &mut self.tool)
        else {
            return;
        };
        *kind = match o.material {
            Material::None => Material::Simple,
            m => m,
        };
        *color = o.mesh.face_color(tri).unwrap_or(o.color);
        *dropper = false;
    }
}
