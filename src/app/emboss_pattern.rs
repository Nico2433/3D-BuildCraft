//! Emboss's Pattern control, as the original's: "Pattern", a big icon of the current
//! pattern (a serif A for text, the shape itself, a picture for an image) and a chevron.
//! It opens a flyout with Text, Load (a picture from a file) and the grid of shapes, each
//! drawn from the outline it embosses.

use super::flyout::{Flyout, Grid, Mark, ROW};
use super::icons::{self, Icon};
use super::style;
use super::*;
use std::sync::OnceLock;

/// The flyout's width: six 80-point tiles.
const MENU_W: f32 = 480.0;
/// The icon in the bar, and the space round it and the chevron.
const ICON: f32 = 30.0;
const CHEVRON: f32 = 12.0;
const PAD: f32 = 5.0;

/// What the Pattern menu chose.
enum Choice {
    Text,
    Load,
    Shape(Shape),
}

/// The Pattern label and button, and its flyout while `open`. Changes `params.pattern` to
/// what is chosen (Load asks for the picture first).
pub(super) fn pattern_control(ui: &mut egui::Ui, lang: Lang, params: &mut Emboss, open: &mut bool) {
    style::label(ui, tr(lang, "Pattern"));
    let size = Vec2::new(PAD + ICON + PAD + CHEVRON + PAD, style::FIELD_H);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let p = ui.painter();
    if resp.hovered() || *open {
        p.rect_filled(rect, egui::CornerRadius::ZERO, style::BLUE_HOVER);
    }
    let icon = egui::Rect::from_min_size(
        egui::pos2(rect.left() + PAD, rect.center().y - ICON / 2.0),
        Vec2::splat(ICON),
    );
    paint_pattern(p, icon, &params.pattern, style::TEXT);
    let chevron = egui::Rect::from_center_size(
        egui::pos2(rect.right() - PAD - CHEVRON / 2.0, rect.center().y),
        Vec2::splat(CHEVRON),
    );
    icons::paint(p, chevron, Icon::ChevronDown, style::TEXT);
    let resp = resp.on_hover_text(tr(lang, pattern_label(&params.pattern)));
    if resp.clicked() {
        *open = !*open;
    }
    if !*open {
        return;
    }
    let top = ui.ctx().screen_rect().top() + 32.0;
    let at = egui::pos2(rect.right() + 4.0, top);
    let shown = Flyout::new("emboss_pattern", at, MENU_W).show(ui.ctx(), resp.clicked(), |m| {
        let selected = match params.pattern {
            Pattern::Shape(s) => Shape::ALL.iter().position(|&o| o == s),
            _ => None,
        };
        let mut choice = None;
        if m.icon_row(tr(lang, "Text"), |p, r, c| {
            icons::paint(p, r, Icon::SerifA, c)
        }) {
            choice = Some(Choice::Text);
        }
        if m.icon_row(tr(lang, "Load"), |p, r, c| {
            icons::paint(p, r, Icon::Picture, c)
        }) {
            choice = Some(Choice::Load);
        }
        m.header(tr(lang, "Shapes"));
        let grid = Grid {
            columns: 6,
            tile: 72.0,
            pitch: 80.0,
            first: Vec2::splat(40.0),
            extra: 0.0,
            fill: ROW,
            mark: Mark::Picture,
        };
        let paint = |p: &egui::Painter, k: usize, r: egui::Rect| {
            paint_shape(p, r.shrink(10.0), Shape::ALL[k], style::TEXT);
        };
        let on = |k: usize| selected == Some(k);
        if let Some(k) = m.tile_grid(Shape::ALL.len(), &grid, on, paint) {
            choice = Some(Choice::Shape(Shape::ALL[k]));
        }
        choice
    });
    match shown.inner {
        Some(Choice::Text) => params.pattern = Pattern::Text,
        Some(Choice::Shape(s)) => params.pattern = Pattern::Shape(s),
        Some(Choice::Load) => {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter(tr(lang, "Image"), &crate::image3d::EXTENSIONS)
                .pick_file()
            {
                params.pattern = Pattern::Image(path);
            }
        }
        None if !shown.dismissed => return,
        None => {}
    }
    *open = false;
}

/// The name of a pattern, for the button's tooltip.
fn pattern_label(pattern: &Pattern) -> &'static str {
    match pattern {
        Pattern::Text => "Text",
        Pattern::Shape(s) => s.label(),
        Pattern::Image(_) => "Image",
    }
}

/// The icon of `pattern` in `rect`.
fn paint_pattern(p: &egui::Painter, rect: egui::Rect, pattern: &Pattern, color: Color32) {
    match pattern {
        Pattern::Text => icons::paint(p, rect, Icon::SerifA, color),
        Pattern::Shape(s) => paint_shape(p, rect.shrink(2.0), *s, color),
        Pattern::Image(_) => icons::paint(p, rect, Icon::Picture, color),
    }
}

/// `shape` filled in `color`, as large as fits in `rect` and centred, with smoothed edges.
pub(super) fn paint_shape(p: &egui::Painter, rect: egui::Rect, shape: Shape, color: Color32) {
    static FILLED: OnceLock<Vec<crate::emboss::Filled>> = OnceLock::new();
    let all = FILLED.get_or_init(|| Shape::ALL.map(crate::emboss::shape_fill).into());
    let Some(k) = Shape::ALL.iter().position(|&s| s == shape) else {
        return;
    };
    let f = &all[k];
    let side = rect.width().min(rect.height());
    let at = |q: &glam::Vec2| rect.center() + Vec2::new(q.x, -q.y) * side;
    let mut mesh = egui::Mesh::default();
    for q in &f.points {
        mesh.colored_vertex(at(q), color);
    }
    mesh.indices = f.triangles.iter().flatten().copied().collect();
    p.add(egui::Shape::mesh(mesh));
    // A thin line round each outline smooths the mesh's hard edges.
    for c in &f.contours {
        let line = f.points[c.clone()].iter().map(at).collect();
        p.add(egui::Shape::closed_line(line, Stroke::new(1.0_f32, color)));
    }
}
