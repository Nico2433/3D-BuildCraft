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
/// Emboss preview lines: the card the pattern is projected from, its border and the
/// projection cylinder.
pub const EMBOSS_CARD: Color32 = Color32::from_rgb(0xB4, 0xE6, 0x96);
pub const EMBOSS_BORDER: Color32 = Color32::from_rgba_premultiplied(0x6C, 0x8A, 0x5A, 0xA0);
pub const EMBOSS_GUIDE: Color32 = Color32::from_rgba_premultiplied(0xA0, 0xA0, 0xA0, 0xC0);
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

/// Shared interaction colours for both widths of the selection pane.
pub const PANE_BAR: Bar = Bar {
    height: 48.0,
    hover: Color32::from_rgb(0x57, 0x60, 0x64),
    active: ACTIVE,
};

/// Flat rows share state precedence while retaining their bar's palette.
/// Disabled rows never react to the pointer; an existing selection stays visible.
pub fn interaction_fill(response: &Response, enabled: bool, selected: bool, bar: Bar) -> Color32 {
    if selected || (enabled && response.enabled() && response.is_pointer_button_down_on()) {
        bar.active
    } else if enabled && response.enabled() && response.hovered() {
        bar.hover
    } else {
        Color32::TRANSPARENT
    }
}

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
    let height = bar.height;
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
    let fill = interaction_fill(&resp, enabled, active, bar);
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
    p.rect_filled(
        rect,
        CornerRadius::ZERO,
        interaction_fill(&resp, enabled, false, TOOL_BAR),
    );
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
    icon_button_in_bar(ui, icon, size, enabled, Bar { hover, ..TAB_BAR })
}

/// An icon button with the same interaction palette as the surrounding rows.
pub fn icon_button_in_bar(
    ui: &mut Ui,
    icon: Icon,
    size: Vec2,
    enabled: bool,
    bar: Bar,
) -> Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    ui.painter().rect_filled(
        rect,
        CornerRadius::ZERO,
        interaction_fill(&resp, enabled, false, bar),
    );
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
    ui.label(label_text(text))
}

/// `text` as [`label`] writes it.
fn label_text(text: &str) -> egui::RichText {
    egui::RichText::new(text).size(LABEL_FONT + 1.3).color(TEXT)
}

/// A label that belongs to the control `add` draws after it, as the original's bars have
/// them ("Text" before its field, "Font" before its list): the pointer over the label
/// shows the control hovered, and a click on it acts on the control: a field takes the
/// focus, a drop-down opens, a switch turns. The controls of this module do it; another
/// egui widget can through [`lead_target`]. Returns what `add` returns.
pub fn labeled<R>(ui: &mut Ui, text: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    labeled_core(ui, label_text(text), 0.0, add)
}

/// [`labeled`] taking at least `min_width`, for labels lined up in a column: the gap after
/// a short one counts as the label.
pub fn labeled_wide<R>(
    ui: &mut Ui,
    text: &str,
    min_width: f32,
    add: impl FnOnce(&mut Ui) -> R,
) -> R {
    labeled_core(ui, label_text(text), min_width, add)
}

/// [`labeled`] with the label in its own text style, as in panels and pages.
pub fn labeled_rich<R>(ui: &mut Ui, text: egui::RichText, add: impl FnOnce(&mut Ui) -> R) -> R {
    labeled_core(ui, text, 0.0, add)
}

fn labeled_core<R>(
    ui: &mut Ui,
    text: egui::RichText,
    min_width: f32,
    add: impl FnOnce(&mut Ui) -> R,
) -> R {
    let mut label = ui.add(
        egui::Label::new(text)
            .selectable(false)
            .sense(Sense::click()),
    );
    let pad = min_width - label.rect.width();
    if pad > 0.0 {
        // The space up to the column's width answers as the label does.
        let wide = label.rect.with_max_x(label.rect.left() + min_width);
        label = label.union(ui.interact(wide, label.id.with("pad"), Sense::click()));
        ui.add_space(pad);
    }
    let lead = Lead {
        hovered: label.hovered(),
        clicked: label.clicked(),
    };
    ui.data_mut(|d| d.insert_temp(lead_id(), lead));
    let out = add(ui);
    // Nothing in `add` took it.
    ui.data_mut(|d| d.remove::<Lead>(lead_id()));
    out
}

/// What the pointer does on the label of the control being drawn ([`labeled`]).
#[derive(Clone, Copy, Default)]
pub struct Lead {
    pub hovered: bool,
    pub clicked: bool,
}

fn lead_id() -> egui::Id {
    egui::Id::new("style_label_lead")
}

/// The label's pointer state for the control drawn now, taken so that only the first
/// control after the label gets it; nothing outside a [`labeled`] or in a disabled ui. A
/// control of a module's own calls it before drawing, to look hovered and act on a click.
pub fn take_lead(ui: &Ui) -> Lead {
    ui.data_mut(|d| {
        let lead = d.get_temp::<Lead>(lead_id());
        d.remove::<Lead>(lead_id());
        lead
    })
    .filter(|_| ui.is_enabled())
    .unwrap_or_default()
}

/// Gives a plain egui widget (such as a `TextEdit`) the label's part in a [`labeled`]:
/// drawn as hovered while the label is, and focused when the label is clicked.
pub fn lead_target(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> Response) -> Response {
    let lead = take_lead(ui);
    let resp = hovered_look(ui, lead.hovered, add);
    if lead.clicked {
        resp.request_focus();
        ui.ctx().request_repaint();
    }
    resp
}

/// Runs `add` with idle egui widgets drawn as under the pointer when `on`.
fn hovered_look<R>(ui: &mut Ui, on: bool, add: impl FnOnce(&mut Ui) -> R) -> R {
    if !on {
        return add(ui);
    }
    let before = ui.visuals().widgets.inactive;
    let w = &mut ui.visuals_mut().widgets;
    w.inactive = w.hovered;
    let out = add(ui);
    ui.visuals_mut().widgets.inactive = before;
    out
}

/// A radio row with theme-independent contrast on the dark flyout.
pub fn radio(ui: &mut Ui, on: bool, text: &str) -> Response {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font(MENU_FONT), TEXT);
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(32.0 + galley.size().x, 32.0), Sense::click());
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::RadioButton, ui.is_enabled(), on, text)
    });
    let center = egui::pos2(rect.left() + 11.0, rect.center().y);
    ui.painter()
        .circle_stroke(center, 10.0, Stroke::new(2.0_f32, TEXT));
    if on {
        ui.painter().circle_filled(center, 5.0, TOGGLE_ON);
    }
    ui.painter().galley(
        egui::pos2(rect.left() + 32.0, rect.center().y - galley.size().y / 2.0),
        galley,
        TEXT,
    );
    resp
}

/// A Windows-style on/off switch followed by its state text; a click on either turns it,
/// as does one on its [`labeled`] label.
pub fn switch(ui: &mut Ui, on: &mut bool, on_text: &str, off_text: &str) -> Response {
    let lead = take_lead(ui);
    let (rect, knob) = ui.allocate_exact_size(Vec2::new(50.0, 26.0), Sense::click());
    // Drawn after its text is laid out, so the text shown is the state before a click
    // this frame; a click asks for the next frame to show the new one.
    let state = ui.add(
        egui::Label::new(label_text(if *on { on_text } else { off_text }))
            .selectable(false)
            .sense(Sense::click()),
    );
    let mut resp = knob.union(state);
    if resp.clicked() || lead.clicked {
        *on = !*on;
        resp.mark_changed();
        ui.ctx().request_repaint();
    }
    let p = ui.painter();
    let r = CornerRadius::same(13);
    if *on {
        p.rect_filled(rect, r, TOGGLE_ON);
        p.circle_filled(egui::pos2(rect.right() - 13.0, rect.center().y), 6.0, TEXT);
    } else {
        p.rect_stroke(
            rect,
            r,
            Stroke::new(2.0_f32, TEXT),
            egui::StrokeKind::Inside,
        );
        p.circle_filled(
            egui::pos2(rect.left() + 13.0, rect.center().y),
            6.0,
            TEXT_DIM,
        );
    }
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

/// The box of a field in the colours the panel set ([`blue_fields`] in a tool bar).
pub fn field_box(ui: &Ui, rect: Rect, hovered: bool) {
    let v = ui.visuals();
    let (fill, border) = (v.extreme_bg_color, v.widgets.inactive.bg_stroke.color);
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::ZERO, fill);
    let edge = if hovered { TEXT } else { border };
    p.rect_stroke(
        rect,
        CornerRadius::ZERO,
        Stroke::new(2.0_f32, edge),
        egui::StrokeKind::Inside,
    );
}

/// The original's drop-down: a field with the choice and a chevron; clicked, a dark list
/// of the other choices opens under it. True when the choice changed.
pub fn dropdown(ui: &mut Ui, id: &str, width: f32, options: &[&str], selected: &mut usize) -> bool {
    let open_id = ui.id().with((id, "open"));
    let mut open = ui.data(|d| d.get_temp::<bool>(open_id)).unwrap_or(false);
    let current = options.get(*selected).copied().unwrap_or_default();
    // The list is as wide as its longest choice (with room for the scroll bar), so long
    // names are not cut off by a narrow field.
    let longest = ui.fonts(|f| {
        options
            .iter()
            .map(|o| {
                f.layout_no_wrap((*o).to_owned(), FontId::proportional(MENU_FONT), TEXT)
                    .size()
                    .x
            })
            .fold(0.0_f32, f32::max)
    });
    let list_w = width.max(longest + 2.0 * 10.0 + 16.0);
    let changed = dropdown_rows(
        ui,
        open_id,
        width,
        current,
        options.len(),
        selected,
        &mut open,
        Vec2::new(list_w, FIELD_H),
        |p, row, k| row_text(p, row, options[k], 10.0),
    );
    ui.data_mut(|d| d.insert_temp(open_id, open));
    changed
}

/// [`dropdown`] showing `current`, with the open state kept by the caller and `count`
/// rows of size `row`, each painted by `paint` (as with a swatch before the name).
#[allow(clippy::too_many_arguments)]
pub fn dropdown_rows(
    ui: &mut Ui,
    id: egui::Id,
    width: f32,
    current: &str,
    count: usize,
    selected: &mut usize,
    open: &mut bool,
    row: Vec2,
    paint: impl Fn(&egui::Painter, Rect, usize),
) -> bool {
    let field = |p: &egui::Painter, r: Rect| row_text(p, r, current, 10.0);
    dropdown_painted(ui, id, width, field, count, selected, open, row, paint)
}

/// [`dropdown_rows`] with the field's contents painted by `current` (given a painter
/// clipped before the chevron and the field's rect).
#[allow(clippy::too_many_arguments)]
pub fn dropdown_painted(
    ui: &mut Ui,
    id: egui::Id,
    width: f32,
    current: impl FnOnce(&egui::Painter, Rect),
    count: usize,
    selected: &mut usize,
    open: &mut bool,
    row: Vec2,
    paint: impl Fn(&egui::Painter, Rect, usize),
) -> bool {
    let lead = take_lead(ui);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, FIELD_H), Sense::click());
    let opening = resp.clicked() || lead.clicked;
    field_box(ui, rect, resp.hovered() || lead.hovered || *open);
    let text = rect.with_max_x(rect.right() - 30.0);
    current(&ui.painter().with_clip_rect(text), rect);
    let chevron = Rect::from_center_size(
        rect.right_center() - Vec2::new(16.0, 0.0),
        Vec2::splat(14.0),
    );
    icons::paint(ui.painter(), chevron, Icon::ChevronDown, TEXT);
    if opening {
        *open = !*open;
    }
    if !*open {
        return false;
    }
    // Under the field, or over it when there is more room there.
    let screen = ui.ctx().screen_rect();
    let others = count - usize::from(*selected < count);
    let height = row.y * others.min(LIST_ROWS) as f32;
    let (below, above) = (screen.bottom() - rect.bottom(), rect.top() - screen.top());
    let at = if height <= below || below >= above {
        rect.left_bottom()
    } else {
        rect.left_top() - Vec2::new(0.0, height.min(above))
    };
    let picked = list_rows(
        ui.ctx(),
        id,
        at,
        row,
        count,
        Some(*selected),
        open,
        opening,
        paint,
    );
    picked.is_some_and(|k| std::mem::replace(selected, k) != k)
}

/// Rows a list shows at most; more scroll.
const LIST_ROWS: usize = 12;

/// `text` at the left of `rect`, `inset` in, as the fields and lists write it.
pub fn row_text(p: &egui::Painter, rect: Rect, text: &str, inset: f32) {
    p.text(
        rect.left_center() + Vec2::new(inset, 0.0),
        egui::Align2::LEFT_CENTER,
        text,
        FontId::proportional(MENU_FONT),
        TEXT,
    );
}

/// The original's drop-down list: dark rows from `at` (moved as needed to stay on screen),
/// without the `selected` one, which the field shows. Returns the index in `options` of
/// the row picked; `open` turns false on a pick or a click elsewhere (other than the click
/// that opened it, `opening`).
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
    list_rows(
        ctx,
        id,
        at,
        Vec2::new(width, FIELD_H),
        options.len(),
        selected,
        open,
        opening,
        |p, row, k| row_text(p, row, options[k], 10.0),
    )
}

/// [`list`] of `count` rows of size `row`, each painted by `paint`.
#[allow(clippy::too_many_arguments)]
pub fn list_rows(
    ctx: &egui::Context,
    id: egui::Id,
    at: egui::Pos2,
    row: Vec2,
    count: usize,
    selected: Option<usize>,
    open: &mut bool,
    opening: bool,
    paint: impl Fn(&egui::Painter, Rect, usize),
) -> Option<usize> {
    let shown: Vec<usize> = (0..count).filter(|&k| Some(k) != selected).collect();
    if shown.is_empty() {
        *open = false;
        return None;
    }
    let screen = ctx.screen_rect();
    let height = (row.y * shown.len().min(LIST_ROWS) as f32).min(screen.height());
    let top = at.y.min(screen.bottom() - height).max(screen.top());
    // A list wider than the room to the right of the field moves left to stay on screen.
    let row = Vec2::new(row.x.min(screen.width()), row.y);
    let left = at.x.min(screen.right() - row.x).max(screen.left());
    let list = Rect::from_min_size(egui::pos2(left, top), Vec2::new(row.x, height));
    let mut picked = None;
    let area = egui::Area::new(id.with("list"))
        .fixed_pos(list.min)
        .order(egui::Order::Foreground)
        .fade_in(false)
        .show(ctx, |ui| {
            ui.painter().rect_filled(
                list,
                CornerRadius::ZERO,
                Color32::from_rgb(0x22, 0x26, 0x2B),
            );
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            egui::ScrollArea::vertical()
                .max_height(height)
                .show(ui, |ui| {
                    for &k in &shown {
                        let (r, resp) = ui.allocate_exact_size(row, Sense::click());
                        if resp.hovered() {
                            ui.painter().rect_filled(
                                r.shrink(1.0),
                                CornerRadius::ZERO,
                                Color32::from_rgb(0x3A, 0x3E, 0x44),
                            );
                        }
                        paint(&ui.painter().with_clip_rect(r), r, k);
                        if resp.clicked() {
                            picked = Some(k);
                            *open = false;
                        }
                    }
                });
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

/// A single-line text box, `hint` shown while it is empty. Focused, it turns light with
/// an accent border and, while it has text, a × at its right that empties it.
pub fn text_field(ui: &mut Ui, text: &mut String, width: f32, hint: &str) -> Response {
    let id = ui.next_auto_id();
    let lead = take_lead(ui);
    if lead.clicked {
        ui.memory_mut(|m| m.request_focus(id));
    }
    let shown_id = id.with("clear");
    // Kept while the × is pressed, as the press takes the focus away from the box.
    let was_shown = ui.data(|d| d.get_temp::<bool>(shown_id)).unwrap_or(false);
    let focused = ui.memory(|m| m.has_focus(id));
    let margin = egui::Margin {
        left: 8,
        right: 28,
        top: 6,
        bottom: 6,
    };
    let edit = egui::TextEdit::singleline(text)
        .id(id)
        .hint_text(hint)
        .font(FontId::proportional(MENU_FONT))
        .desired_width(width)
        .margin(margin);
    let mut resp = if focused {
        let before = ui.visuals().clone();
        let v = ui.visuals_mut();
        v.selection.stroke = Stroke::new(2.0_f32, TOGGLE_ON);
        v.selection.bg_fill = Color32::from_rgb(0x99, 0xC9, 0xEF);
        v.text_cursor.stroke = Stroke::new(1.0_f32, Color32::BLACK);
        let resp = ui.add(
            edit.background_color(Color32::WHITE)
                .text_color(Color32::BLACK),
        );
        *ui.visuals_mut() = before;
        resp
    } else {
        hovered_look(ui, lead.hovered, |ui| ui.add(edit))
    };
    let pressing = ui.input(|i| i.pointer.any_down() || i.pointer.any_released());
    let shown = !text.is_empty() && (resp.has_focus() || (was_shown && pressing));
    ui.data_mut(|d| d.insert_temp(shown_id, shown));
    if shown {
        // The response is the text area inside the margin; the × sits at the box's end.
        let x = Rect::from_center_size(
            resp.rect.right_center() + Vec2::new(margin.right as f32 - 14.0, 0.0),
            Vec2::splat(20.0),
        );
        let clear = ui.interact(x, shown_id, Sense::click());
        if clear.hovered() {
            ui.painter()
                .rect_filled(x, CornerRadius::ZERO, Color32::from_gray(0xE0));
        }
        let (a, b) = (x.shrink(6.0).left_top(), x.shrink(6.0).right_bottom());
        let stroke = Stroke::new(1.2_f32, Color32::from_gray(0x33));
        ui.painter().line_segment([a, b], stroke);
        ui.painter()
            .line_segment([egui::pos2(a.x, b.y), egui::pos2(b.x, a.y)], stroke);
        if clear.clicked() {
            text.clear();
            resp.request_focus();
            resp.mark_changed();
        }
    }
    resp
}

/// A number field with its unit dimmed at the right inside the box, as "40  mm", for tool
/// bars: the value follows every number typed within `range` ([`number_field`]).
pub fn unit_field(
    ui: &mut Ui,
    id: &str,
    v: &mut f32,
    unit: &str,
    range: std::ops::RangeInclusive<f32>,
) -> Response {
    number_field(ui, id, v, unit, range, 72.0, true)
}

/// The app's one number field: typed into, never dragged, with its unit dimmed at the
/// right inside the box. With `live` the value follows every number typed within `range`;
/// without, it changes only on Enter or when the field loses focus (for edits that act on
/// the scene as a whole, where "1" on the way to "150" must not apply). Leaving the field
/// puts the value back in range, or back to what it was when the text is not a number.
/// `id` tells apart fields in one panel. Compare `v` to tell whether it changed: the
/// response's own `changed` follows the text.
pub fn number_field(
    ui: &mut Ui,
    id: &str,
    v: &mut f32,
    unit: &str,
    range: std::ops::RangeInclusive<f32>,
    width: f32,
    live: bool,
) -> Response {
    let lead = take_lead(ui);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, FIELD_H), Sense::hover());
    field_box(ui, rect, ui.rect_contains_pointer(rect) || lead.hovered);
    let id = ui.id().with(("number_field", id));
    if lead.clicked {
        ui.memory_mut(|m| m.request_focus(id.with("edit")));
    }
    let shown = |n: f32| format!("{}", (n * 1000.0).round() / 1000.0);
    // The text being typed lives in memory while the field has focus.
    let mut text = ui
        .data(|d| d.get_temp::<String>(id))
        .unwrap_or_else(|| shown(*v));
    let unit_w = if unit.is_empty() {
        0.0
    } else {
        ui.fonts(|f| {
            f.layout_no_wrap(unit.to_owned(), FontId::proportional(LABEL_FONT), TEXT)
                .size()
                .x
        }) + 12.0
    };
    let mut child =
        ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(Vec2::new(6.0, 4.0))));
    let resp = child.add_sized(
        Vec2::new(rect.width() - 12.0 - unit_w, FIELD_H - 8.0),
        egui::TextEdit::singleline(&mut text)
            .id(id.with("edit"))
            .frame(false)
            .font(FontId::proportional(LABEL_FONT)),
    );
    let typed = text.trim().replace(',', ".").parse::<f32>().ok();
    let enter = resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    if live && resp.changed() {
        if let Some(n) = typed.filter(|n| range.contains(n)) {
            *v = n;
        }
    }
    if resp.lost_focus() || enter {
        if let Some(n) = typed.filter(|n| n.is_finite()) {
            *v = n.clamp(*range.start(), *range.end());
        }
    }
    if resp.has_focus() && !enter {
        ui.data_mut(|d| d.insert_temp(id, text));
    } else {
        ui.data_mut(|d| d.remove::<String>(id));
    }
    if !unit.is_empty() {
        ui.painter().text(
            rect.right_center() - Vec2::new(8.0, 0.0),
            egui::Align2::RIGHT_CENTER,
            unit,
            FontId::proportional(LABEL_FONT),
            TEXT_DIM,
        );
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// One frame of a panel built by `add`, after `events`.
    fn frame(ctx: &egui::Context, events: Vec<egui::Event>, add: impl FnMut(&mut Ui)) {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(800.0, 600.0),
            )),
            events,
            ..Default::default()
        };
        let mut add = add;
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| ui.horizontal(|ui| add(ui)));
        });
    }

    /// A click at `at`: the pointer moves there, then presses and lets go over two frames.
    fn click(ctx: &egui::Context, at: egui::Pos2, mut add: impl FnMut(&mut Ui)) {
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        frame(ctx, vec![egui::Event::PointerMoved(at)], &mut add);
        frame(ctx, vec![button(true)], &mut add);
        frame(ctx, vec![button(false)], &mut add);
        frame(ctx, vec![], &mut add);
    }

    /// Where the label "Name" of a [`labeled`] control drawn by `add` is.
    fn label_spot(ctx: &egui::Context, mut add: impl FnMut(&mut Ui)) -> egui::Pos2 {
        let mut at = egui::Pos2::ZERO;
        frame(ctx, vec![], |ui| {
            at = ui.next_widget_position() + Vec2::new(8.0, 0.0);
            add(ui);
        });
        at
    }

    #[test]
    fn a_click_on_a_label_acts_on_its_control() {
        // A text field takes the focus.
        let ctx = egui::Context::default();
        let mut text = String::new();
        let mut field = None;
        let mut add = |ui: &mut Ui| {
            let r = labeled(ui, "Name", |ui| text_field(ui, &mut text, 100.0, ""));
            field = Some(r.id);
        };
        let at = label_spot(&ctx, &mut add);
        click(&ctx, at, &mut add);
        let id = field.unwrap();
        assert!(ctx.memory(|m| m.has_focus(id)));

        // A drop-down opens.
        let ctx = egui::Context::default();
        let mut k = 0;
        let mut open_id = None;
        let mut add = |ui: &mut Ui| {
            open_id = Some(ui.id().with(("pick", "open")));
            labeled(ui, "Name", |ui| {
                dropdown(ui, "pick", 100.0, &["a", "b"], &mut k)
            });
        };
        let at = label_spot(&ctx, &mut add);
        click(&ctx, at, &mut add);
        let id = open_id.unwrap();
        assert_eq!(ctx.data(|d| d.get_temp::<bool>(id)), Some(true));

        // A switch turns, from its label and from its own text.
        let ctx = egui::Context::default();
        let mut on = false;
        let mut add = |ui: &mut Ui| {
            labeled(ui, "Name", |ui| switch(ui, &mut on, "On", "Off"));
        };
        let at = label_spot(&ctx, &mut add);
        click(&ctx, at, &mut add);
        assert!(on);
        let mut text_at = egui::Pos2::ZERO;
        frame(&ctx, vec![], |ui| {
            let r = switch(ui, &mut on, "On", "Off");
            text_at = r.rect.right_center() - Vec2::new(4.0, 0.0);
        });
        click(&ctx, text_at, |ui| {
            switch(ui, &mut on, "On", "Off");
        });
        assert!(!on);
    }

    #[test]
    fn a_wide_list_stays_on_screen() {
        let ctx = egui::Context::default();
        let screen = Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(400.0, 300.0));
        let id = egui::Id::new("wide");
        for _ in 0..2 {
            let input = egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| {
                let mut open = true;
                list_rows(
                    ctx,
                    id,
                    egui::pos2(300.0, 40.0),
                    Vec2::new(250.0, FIELD_H),
                    3,
                    None,
                    &mut open,
                    true,
                    |_, _, _| {},
                );
            });
        }
        let rect = ctx.memory(|m| m.area_rect(id.with("list"))).unwrap();
        assert!(rect.left() >= 0.0 && rect.right() <= 400.5, "{rect:?}");
    }
}
