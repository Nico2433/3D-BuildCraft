//! Paint → Material: the tool's Type drop-down, with a preview swatch for each style, and
//! the live change of the selected objects.

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

/// The "Type" drop-down: a field with the current choice that opens a list of the other
/// materials, each with a swatch of `color`. True when the choice changed.
pub(super) fn type_dropdown(
    ui: &mut egui::Ui,
    lang: Lang,
    kind: &mut Material,
    color: Rgba,
    open: &mut bool,
) -> bool {
    let width = 150.0;
    let mut k = Material::ALL.iter().position(|m| m == kind).unwrap_or(0);
    let changed = style::dropdown_rows(
        ui,
        egui::Id::new("material_type"),
        width,
        tr(lang, kind.label()),
        Material::ALL.len(),
        &mut k,
        open,
        Vec2::new(width + 76.0, ROW_H),
        |p, row, k| {
            let m = Material::ALL[k];
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
        },
    );
    *kind = Material::ALL[k];
    changed
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
            self.commit(*before);
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
