//! Look of the original: colours sampled from its screenshots, sizes in points (it ran at
//! 150 %, so a 72 px bar there is 48 pt here), the font, and the buttons every bar uses.

use super::icons::{self, Icon};
use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Rect, Response,
    Sense, Stroke, Ui, Vec2,
};

pub const BLUE: Color32 = Color32::from_rgb(0x2C, 0x45, 0x80);
/// Selected option in a blue tool bar.
pub const BLUE_ACTIVE: Color32 = Color32::from_rgb(0x4E, 0x69, 0x97);
pub const BLUE_HOVER: Color32 = Color32::from_rgb(0x3D, 0x57, 0x8C);
/// Fields and combos in a blue tool bar.
pub const BLUE_FIELD: Color32 = Color32::from_rgb(0x1A, 0x29, 0x4D);
pub const BLUE_FIELD_BORDER: Color32 = Color32::from_rgb(0x80, 0x8F, 0xB3);
pub const RIBBON: Color32 = Color32::from_rgb(0x2E, 0x2E, 0x2E);
pub const RIBBON_HOVER: Color32 = Color32::from_rgb(0x3E, 0x3E, 0x3E);
/// A toggle that is on (View tab) or the current mode (bottom bar).
pub const ACTIVE: Color32 = Color32::from_rgb(0x50, 0x59, 0x5D);
pub const SEPARATOR: Color32 = Color32::from_rgb(0x82, 0x82, 0x82);
pub const STRIP: Color32 = Color32::from_rgb(0x43, 0x43, 0x43);
pub const PANEL: Color32 = Color32::from_rgb(0x3D, 0x3D, 0x3D);
pub const FLYOUT: Color32 = Color32::from_rgb(0x3A, 0x3A, 0x3A);
pub const BOTTOM_BAR: Color32 = Color32::from_rgb(0x3B, 0x3B, 0x3B);
pub const FIELD_GREEN: Color32 = Color32::from_rgb(0x5C, 0x7E, 0x4B);
pub const FIELD_BLUE: Color32 = Color32::from_rgb(0x3E, 0x53, 0x5C);
pub const CARD: Color32 = Color32::from_rgb(0x2C, 0x2C, 0x2C);
pub const CARD_ERROR: Color32 = Color32::from_rgb(0xEC, 0x1C, 0x24);
pub const CARD_INFO: Color32 = Color32::from_rgb(0x22, 0xBD, 0xEF);
pub const TOGGLE_ON: Color32 = Color32::from_rgb(0x01, 0x5E, 0x86);
pub const TEXT: Color32 = Color32::from_rgb(0xF2, 0xF2, 0xF2);
pub const TEXT_DIM: Color32 = Color32::from_rgb(0x8A, 0x8A, 0x8A);
pub const TEXT_ON_BLUE_DIM: Color32 = Color32::from_rgb(0x7D, 0x8C, 0xB0);

pub const TITLE_H: f32 = 32.0;
pub const TAB_H: f32 = 48.0;
pub const RIBBON_H: f32 = 48.0;
pub const TOOL_H: f32 = 60.0;
pub const STRIP_W: f32 = 45.0;
pub const PANEL_W: f32 = 149.0;
pub const MENU_W: f32 = 204.0;
pub const FLYOUT_W: f32 = 296.0;

pub const TAB_FONT: f32 = 16.0;
pub const LABEL_FONT: f32 = 12.7;
pub const MENU_FONT: f32 = 14.0;
pub const ICON: f32 = 18.0;

/// The original's Segoe UI where it exists, otherwise the closest installed sans; the
/// bundled fonts stay behind it for any character it lacks.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let candidates = [
        "Segoe UI",
        "Selawik",
        "Noto Sans",
        "Open Sans",
        "Source Sans 3",
        "DejaVu Sans",
        "Helvetica Neue",
        "Arial",
    ];
    if let Some((name, data)) = candidates
        .iter()
        .find_map(|f| crate::emboss::font_data(f).map(|d| (*f, d)))
    {
        let (bytes, index) = data;
        let mut font = FontData::from_owned(bytes);
        font.index = index;
        fonts.font_data.insert(name.to_owned(), font.into());
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, name.to_owned());
    }
    ctx.set_fonts(fonts);
    // Every theme, so switching light/dark keeps these.
    ctx.all_styles_mut(|s| {
        s.text_styles
            .insert(egui::TextStyle::Body, FontId::proportional(LABEL_FONT));
        s.text_styles
            .insert(egui::TextStyle::Button, FontId::proportional(LABEL_FONT));
        s.spacing.item_spacing = Vec2::new(8.0, 6.0);
    });
}

fn font(size: f32) -> FontId {
    FontId::proportional(size)
}

/// The bar a flat button sits in: its height and its hover and "on" colours.
#[derive(Clone, Copy)]
pub struct Bar {
    pub height: f32,
    pub hover: Color32,
    pub active: Color32,
}

pub const RIBBON_BAR: Bar = Bar {
    height: RIBBON_H,
    hover: RIBBON_HOVER,
    active: ACTIVE,
};
pub const TAB_BAR: Bar = Bar {
    height: TAB_H,
    hover: BLUE_HOVER,
    active: BLUE_ACTIVE,
};
pub const TOOL_BAR: Bar = Bar {
    height: TOOL_H - 8.0,
    hover: BLUE_HOVER,
    active: BLUE_ACTIVE,
};

/// A flat button the size of its bar: icon then label, a lighter background when
/// `active` (a toggle that is on) or hovered.
pub fn ribbon_button(
    ui: &mut Ui,
    bar: Bar,
    icon: Icon,
    label: &str,
    enabled: bool,
    active: bool,
) -> Response {
    let Bar {
        height,
        hover,
        active: active_fill,
    } = bar;
    let color = if enabled { TEXT } else { TEXT_DIM };
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font(LABEL_FONT), color);
    let pad = 12.0;
    let width = pad + ICON + 9.0 + galley.size().x + pad;
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, height), sense);
    let fill = if active {
        active_fill
    } else if enabled && resp.hovered() {
        hover
    } else {
        Color32::TRANSPARENT
    };
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::ZERO, fill);
    let icon_rect = Rect::from_center_size(
        egui::pos2(rect.left() + pad + ICON / 2.0, rect.center().y),
        Vec2::splat(ICON),
    );
    icons::paint(p, icon_rect, icon, color);
    p.galley(
        egui::pos2(
            icon_rect.right() + 9.0,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        color,
    );
    resp
}

/// A ribbon button on the dark ribbon.
pub fn ribbon(ui: &mut Ui, icon: Icon, label: &str, enabled: bool, active: bool) -> Response {
    ribbon_button(ui, RIBBON_BAR, icon, label, enabled, active)
}

/// An option button in a blue tool bar.
pub fn option(ui: &mut Ui, icon: Icon, label: &str, active: bool) -> Response {
    ribbon_button(ui, TOOL_BAR, icon, label, true, active)
}

/// Icon over label, for Cancel / confirm and Undo / Redo in a tool bar.
pub fn stacked(ui: &mut Ui, icon: Icon, label: &str, enabled: bool) -> Response {
    let color = if enabled { TEXT } else { TEXT_ON_BLUE_DIM };
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font(LABEL_FONT), color);
    let width = (galley.size().x + 16.0).max(60.0);
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, TOOL_H), sense);
    let p = ui.painter();
    if enabled && resp.hovered() {
        p.rect_filled(rect, CornerRadius::ZERO, BLUE_HOVER);
    }
    let icon_rect = Rect::from_center_size(
        egui::pos2(rect.center().x, rect.top() + 20.0),
        Vec2::splat(22.0),
    );
    icons::paint(p, icon_rect, icon, color);
    p.galley(
        egui::pos2(rect.center().x - galley.size().x / 2.0, rect.top() + 34.0),
        galley,
        color,
    );
    resp
}

/// A square icon-only button, e.g. save, undo and redo on the tab bar.
pub fn icon_button(ui: &mut Ui, icon: Icon, size: Vec2, enabled: bool, hover: Color32) -> Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    if enabled && resp.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::ZERO, hover);
    }
    let color = if enabled { TEXT } else { TEXT_ON_BLUE_DIM };
    let side = 20.0f32.min(size.y - 8.0);
    icons::paint(
        ui.painter(),
        Rect::from_center_size(rect.center(), Vec2::splat(side)),
        icon,
        color,
    );
    resp
}

/// A thin vertical line between groups.
pub fn separator(ui: &mut Ui, height: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(17.0, height), Sense::hover());
    let x = rect.center().x;
    ui.painter().line_segment(
        [
            egui::pos2(x, rect.center().y - 10.0),
            egui::pos2(x, rect.center().y + 10.0),
        ],
        Stroke::new(1.0_f32, color),
    );
}

/// A label in the bar's text colour.
pub fn label(ui: &mut Ui, text: &str) -> Response {
    ui.label(egui::RichText::new(text).size(LABEL_FONT + 1.3).color(TEXT))
}

/// A Windows-style on/off switch followed by its state text.
pub fn switch(ui: &mut Ui, on: &mut bool, on_text: &str, off_text: &str) -> Response {
    let (rect, mut resp) = ui.allocate_exact_size(Vec2::new(44.0, 20.0), Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let p = ui.painter();
    let r = CornerRadius::same(10);
    if *on {
        p.rect_filled(rect, r, TOGGLE_ON);
        p.circle_filled(egui::pos2(rect.right() - 10.0, rect.center().y), 5.0, TEXT);
    } else {
        p.rect_stroke(
            rect,
            r,
            Stroke::new(1.5_f32, TEXT),
            egui::StrokeKind::Inside,
        );
        p.circle_filled(egui::pos2(rect.left() + 10.0, rect.center().y), 5.0, TEXT);
    }
    label(ui, if *on { on_text } else { off_text });
    resp
}

/// Widget colours for fields and combos inside a blue tool bar.
pub fn blue_fields(ui: &mut Ui) {
    let v = ui.visuals_mut();
    v.override_text_color = Some(TEXT);
    v.extreme_bg_color = BLUE_FIELD;
    for w in [
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.bg_fill = BLUE_FIELD;
        w.weak_bg_fill = BLUE_FIELD;
        w.bg_stroke = Stroke::new(1.5_f32, BLUE_FIELD_BORDER);
        w.corner_radius = CornerRadius::ZERO;
        w.fg_stroke = Stroke::new(1.0_f32, TEXT);
    }
    v.widgets.hovered.bg_stroke = Stroke::new(1.5_f32, TEXT);
    v.selection.bg_fill = BLUE_ACTIVE;
    ui.spacing_mut().interact_size.y = 30.0;
    ui.spacing_mut().button_padding = Vec2::new(10.0, 5.0);
}

/// Height of the fields in a blue tool bar.
pub const FIELD_H: f32 = 33.0;

/// The box of a tool bar field: dark blue with a light border.
fn field_box(ui: &Ui, rect: Rect, hovered: bool) {
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::ZERO, BLUE_FIELD);
    let edge = if hovered { TEXT } else { BLUE_FIELD_BORDER };
    p.rect_stroke(
        rect,
        CornerRadius::ZERO,
        Stroke::new(2.0_f32, edge),
        egui::StrokeKind::Inside,
    );
}

/// The original's drop-down: a field with the choice and a chevron; clicked, a dark list
/// opens over it with the current choice in purple. True when the choice changed.
pub fn dropdown(ui: &mut Ui, id: &str, width: f32, options: &[&str], selected: &mut usize) -> bool {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, FIELD_H), Sense::click());
    field_box(ui, rect, resp.hovered());
    let font = FontId::proportional(MENU_FONT);
    let current = options.get(*selected).copied().unwrap_or_default();
    ui.painter().text(
        rect.left_center() + Vec2::new(10.0, 0.0),
        egui::Align2::LEFT_CENTER,
        current,
        font.clone(),
        TEXT,
    );
    let chevron = Rect::from_center_size(
        rect.right_center() - Vec2::new(16.0, 0.0),
        Vec2::splat(14.0),
    );
    icons::paint(ui.painter(), chevron, Icon::ChevronDown, TEXT);

    let open_id = ui.id().with((id, "open"));
    let mut open = ui.data(|d| d.get_temp::<bool>(open_id)).unwrap_or(false);
    if resp.clicked() {
        open = !open;
    }
    let mut changed = false;
    if open {
        // The list lies over the field with the current choice where the field was.
        let top = rect.top() - *selected as f32 * FIELD_H;
        let at = egui::pos2(rect.left(), top);
        if let Some(k) = list(
            ui.ctx(),
            open_id,
            at,
            width,
            options,
            Some(*selected),
            &mut open,
            resp.clicked(),
        ) {
            changed = *selected != k;
            *selected = k;
        }
    }
    ui.data_mut(|d| d.insert_temp(open_id, open));
    changed
}

/// The original's drop-down list: dark rows, the `selected` one in purple, from `at`
/// (moved as needed to stay on screen). Returns the row picked; `open` turns false on a
/// pick or a click elsewhere (other than the click that opened it, `opening`).
#[allow(clippy::too_many_arguments)]
pub fn list(
    ctx: &egui::Context,
    id: egui::Id,
    at: egui::Pos2,
    width: f32,
    options: &[&str],
    selected: Option<usize>,
    open: &mut bool,
    opening: bool,
) -> Option<usize> {
    let size = Vec2::new(width, FIELD_H * options.len() as f32);
    let screen = ctx.screen_rect();
    let top = at.y.min(screen.bottom() - size.y).max(screen.top());
    let list = Rect::from_min_size(egui::pos2(at.x, top), size);
    let font = FontId::proportional(MENU_FONT);
    let mut picked = None;
    let area = egui::Area::new(id.with("list"))
        .fixed_pos(list.min)
        .order(egui::Order::Foreground)
        .fade_in(false)
        .show(ctx, |ui| {
            ui.allocate_rect(list, Sense::hover());
            ui.painter().rect_filled(
                list,
                CornerRadius::ZERO,
                Color32::from_rgb(0x22, 0x26, 0x2B),
            );
            for (k, o) in options.iter().enumerate() {
                let row = Rect::from_min_size(
                    list.min + Vec2::new(0.0, k as f32 * FIELD_H),
                    Vec2::new(width, FIELD_H),
                );
                let r = ui.interact(row, id.with(k), Sense::click());
                let fill = if selected == Some(k) {
                    Color32::from_rgb(0x5A, 0x25, 0x61)
                } else if r.hovered() {
                    Color32::from_rgb(0x3A, 0x3E, 0x44)
                } else {
                    Color32::TRANSPARENT
                };
                ui.painter()
                    .rect_filled(row.shrink(1.0), CornerRadius::ZERO, fill);
                ui.painter().text(
                    row.left_center() + Vec2::new(10.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    *o,
                    font.clone(),
                    TEXT,
                );
                if r.clicked() {
                    picked = Some(k);
                    *open = false;
                }
            }
            ui.painter().rect_stroke(
                list,
                CornerRadius::ZERO,
                Stroke::new(1.0_f32, Color32::from_gray(0x76)),
                egui::StrokeKind::Inside,
            );
        });
    if area.response.clicked_elsewhere() && !opening {
        *open = false;
    }
    picked
}

/// A number field with its unit dimmed at the right inside the box, as "40  mm".
pub fn unit_field(
    ui: &mut Ui,
    v: &mut f32,
    unit: &str,
    range: std::ops::RangeInclusive<f32>,
) -> Response {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(72.0, FIELD_H), Sense::hover());
    field_box(ui, rect, ui.rect_contains_pointer(rect));
    let mut child =
        ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(Vec2::new(4.0, 4.0))));
    let w = child.visuals_mut();
    for s in [
        &mut w.widgets.inactive,
        &mut w.widgets.hovered,
        &mut w.widgets.active,
    ] {
        s.bg_fill = Color32::TRANSPARENT;
        s.weak_bg_fill = Color32::TRANSPARENT;
        s.bg_stroke = Stroke::NONE;
    }
    let resp = child.add_sized(
        Vec2::new(rect.width() - 36.0, FIELD_H - 8.0),
        egui::DragValue::new(v)
            .range(range)
            .speed(0.5)
            .custom_formatter(|n, _| format!("{}", (n * 100.0).round() / 100.0)),
    );
    ui.painter().text(
        rect.right_center() - Vec2::new(8.0, 0.0),
        egui::Align2::RIGHT_CENTER,
        unit,
        FontId::proportional(LABEL_FONT),
        TEXT_DIM,
    );
    resp
}

/// A slider as in the original's tool bars: a thin line and an upright white handle,
/// with optional tick marks. True when it moved.
pub fn slider(
    ui: &mut Ui,
    v: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    width: f32,
    ticks: usize,
) -> bool {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, FIELD_H), Sense::click_and_drag());
    let (lo, hi) = (*range.start(), *range.end());
    let line = rect.shrink2(Vec2::new(4.0, 0.0));
    let before = *v;
    if let Some(p) = resp.interact_pointer_pos() {
        let t = ((p.x - line.left()) / line.width()).clamp(0.0, 1.0);
        *v = lo + t * (hi - lo);
    }
    let t = ((*v - lo) / (hi - lo)).clamp(0.0, 1.0);
    let p = ui.painter();
    let y = rect.center().y;
    p.line_segment(
        [egui::pos2(line.left(), y), egui::pos2(line.right(), y)],
        Stroke::new(2.0_f32, TEXT),
    );
    for k in 0..ticks {
        let x = line.left() + line.width() * k as f32 / (ticks.max(2) - 1) as f32;
        for dy in [-8.0, 6.0] {
            p.line_segment(
                [egui::pos2(x, y + dy), egui::pos2(x, y + dy + 2.0)],
                Stroke::new(1.0_f32, TEXT_ON_BLUE_DIM),
            );
        }
    }
    let knob = Rect::from_center_size(
        egui::pos2(line.left() + t * line.width(), y),
        Vec2::new(6.0, 22.0),
    );
    p.rect_filled(knob, CornerRadius::same(3), TEXT);
    *v != before
}
