//! Emboss's settings in the tool bar, in two rows as the original's: the pattern, its
//! projection, the Bevel button and the top and side colours above; the text and its font
//! below, then our Engrave switch (its size is in the bottom bar, as the
//! original's). Each row keeps a label next to its control.

use super::icons::{self, Icon};
use super::paint_menu::{self, MenuRow, PaintMenu, Picked};
use super::style;
use super::*;

impl App {
    pub(super) fn emboss_settings(&mut self, ui: &mut egui::Ui) {
        let lang = self.settings.lang();
        let target = match self.scene.selection.len() {
            1 => self.scene.selected().next(),
            _ => None,
        };
        let Some(Tool::Emboss {
            params,
            preview,
            built,
            menu,
            ..
        }) = &mut self.tool
        else {
            return;
        };
        let font_list = &mut self.font_list;
        let recent = &self.settings.recent_colors;
        let mut picked = None;
        ui.add_space(12.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 7.0;
            row(ui, |ui| {
                pattern_dropdown(ui, lang, params);
                ui.add_space(GROUP);
                style::label(ui, tr(lang, "Projection"));
                let names = Projection::ALL.map(|p| tr(lang, p.label()));
                let mut k = Projection::ALL
                    .iter()
                    .position(|&p| p == params.projection)
                    .unwrap_or(0);
                if style::dropdown(ui, "emboss_projection", fit(ui, &names), &names, &mut k) {
                    params.projection = Projection::ALL[k];
                }
                ui.add_space(GROUP);
                let bar = style::Bar {
                    height: style::FIELD_H,
                    ..style::TOOL_BAR
                };
                if style::ribbon_button(ui, bar, Icon::Bevel, tr(lang, "Bevel"), true, params.bevel)
                    .clicked()
                {
                    params.bevel = !params.bevel;
                }
                // An engraved pattern has no top or sides of its own to colour.
                if !params.engrave {
                    ui.add_space(GROUP);
                    picked = colors(ui, lang, params, menu, recent);
                }
            });
            row(ui, |ui| {
                // The switch after the text and font: the switch, its name and gaps.
                let switch = 50.0 + label_width(ui, tr(lang, "Engrave")) + 2.0 * 6.0 + GAP + 17.0;
                text_and_font(ui, lang, params, font_list, switch);
                style::separator(ui, style::FIELD_H, style::BLUE_FIELD_BORDER);
                toggle(ui, &mut params.engrave, tr(lang, "Engrave"));
            });
        });
        if params.engrave {
            menu.1 = PaintMenu::Closed;
        }
        let key = (
            params.clone(),
            target.map(|o| (o.id, std::sync::Arc::as_ptr(&o.mesh) as usize, o.matrix())),
        );
        if built.as_ref() != Some(&key) {
            *preview = match target {
                Some(o) => crate::emboss::preview(&o.world_mesh(), params)
                    .map(std::sync::Arc::new)
                    .map_err(|e| format!("{e:#}")),
                None => Err(tr(lang, "Select one object").to_string()),
            };
            *built = Some(key);
        }
        // A colour picked goes first in the recent ones, as Paint's do.
        if let Some(c) = picked {
            self.remember_color(c);
        }
    }
}

impl App {
    /// Sets Emboss up for a screenshot: `spherical` picks that projection.
    pub(super) fn emboss_preset(&mut self, preset: &str) {
        let Some(Tool::Emboss { params, .. }) = &mut self.tool else {
            return;
        };
        if preset == "spherical" {
            params.projection = Projection::Spherical;
        }
    }
}

/// The pattern: text, a shape, or an image (which asks for the picture).
fn pattern_dropdown(ui: &mut egui::Ui, lang: Lang, params: &mut Emboss) {
    style::label(ui, tr(lang, "Pattern"));
    let mut patterns: Vec<&str> = vec![tr(lang, "Text")];
    patterns.extend(Shape::ALL.map(|s| tr(lang, s.label())));
    patterns.push(tr(lang, "Image"));
    let mut k = match &params.pattern {
        Pattern::Text => 0,
        Pattern::Shape(s) => 1 + Shape::ALL.iter().position(|o| o == s).unwrap_or(0),
        Pattern::Image(_) => patterns.len() - 1,
    };
    if style::dropdown(ui, "emboss_pattern", fit(ui, &patterns), &patterns, &mut k) {
        if k == 0 {
            params.pattern = Pattern::Text;
        } else if let Some(&s) = Shape::ALL.get(k - 1) {
            params.pattern = Pattern::Shape(s);
        } else if let Some(p) = rfd::FileDialog::new()
            .add_filter(tr(lang, "Image"), &crate::image3d::EXTENSIONS)
            .pick_file()
        {
            params.pattern = Pattern::Image(p);
        }
    }
}

/// For text, the text and its font; for an image, its file name; nothing for a shape.
fn text_and_font(
    ui: &mut egui::Ui,
    lang: Lang,
    params: &mut Emboss,
    font_list: &mut Option<Vec<String>>,
    after: f32,
) {
    match &params.pattern {
        Pattern::Text => {
            let fonts = font_list.get_or_insert_with(crate::emboss::font_families);
            let mut names = vec![tr(lang, "Built-in")];
            names.extend(fonts.iter().map(String::as_str));
            let mut k = params
                .font
                .as_ref()
                .and_then(|f| fonts.iter().position(|o| o == f))
                .map_or(0, |k| k + 1);
            // As wide as the chosen font's name, so it is never cut.
            let width = fit(ui, &[names[k]]).max(fit(ui, &names[..1]));
            let (text, font) = (tr(lang, "Text"), tr(lang, "Font"));
            // The text field gives up room in a narrow window, so `after` (the width of
            // what follows the font) still fits.
            let rest = label_width(ui, text) + label_width(ui, font) + width + GAP + 5.0 * 6.0;
            let field = (ui.available_width() - rest - after).clamp(70.0, 120.0);
            style::label(ui, text);
            style::text_field(ui, &mut params.text, field, "");
            ui.add_space(GAP);
            style::label(ui, font);
            if style::dropdown(ui, "emboss_font", width, &names, &mut k) {
                params.font = k.checked_sub(1).map(|k| fonts[k].clone());
            }
        }
        Pattern::Image(path) => {
            if let Some(name) = path.file_name() {
                style::label(ui, &name.to_string_lossy());
            }
        }
        Pattern::Shape(_) => {}
    }
}

/// "Color" with a button for the top (a filled star) and one for the sides (an outline
/// star), each opening Paint's colour menu with "Use background color" for the object's
/// own. `menu` is which part's menu is open (0 top, 1 sides). Returns a colour picked.
fn colors(
    ui: &mut egui::Ui,
    lang: Lang,
    params: &mut Emboss,
    menu: &mut (usize, PaintMenu),
    recent: &[Rgba],
) -> Option<Rgba> {
    let (top, sides) = (tr(lang, "Top"), tr(lang, "Sides"));
    // A narrow window keeps the stars alone, their names in tooltips.
    let full = label_width(ui, tr(lang, "Color"))
        + 6.0
        + color_button_width(ui, Some(top))
        + 6.0
        + color_button_width(ui, Some(sides));
    let compact = ui.available_width() < full;
    if !compact {
        style::label(ui, tr(lang, "Color"));
    }
    let mut picked = None;
    for (k, (icon, label, color)) in [
        (Icon::StarFilled, top, &mut params.top_color),
        (Icon::StarOutline, sides, &mut params.side_color),
    ]
    .into_iter()
    .enumerate()
    {
        let shown = (!compact).then_some(label);
        let resp = color_button(ui, icon, shown, *color).on_hover_text(label);
        if resp.clicked() {
            if menu.0 != k {
                *menu = (k, PaintMenu::Closed);
            }
            paint_menu::toggle(&mut menu.1);
        }
        if menu.0 != k {
            continue;
        }
        let look = paint_menu::MenuLook {
            rows: &[MenuRow::MoreColors, MenuRow::Background],
            recent: Some(recent),
            material: false,
        };
        // The menu starts from the colour shown, white for the object's own.
        let mut c = color.unwrap_or([0xFF; 4]);
        let at = resp.rect.left_bottom() + Vec2::new(0.0, 2.0);
        match paint_menu::color_menu(
            ui.ctx(),
            at,
            resp.clicked(),
            lang,
            &mut c,
            &mut menu.1,
            &look,
        ) {
            Some(Picked::Color) => {
                *color = Some(c);
                picked = Some(c);
            }
            Some(Picked::Row(MenuRow::Background)) => *color = None,
            _ => {}
        }
    }
    picked
}

/// Sizes in a colour button: its star, its chevron and the padding round each part.
const STAR: f32 = 18.0;
const CHEVRON: f32 = 12.0;
const PAD: f32 = 4.0;

/// The width of `text` in the bar's label font.
fn label_width(ui: &egui::Ui, text: &str) -> f32 {
    ui.painter()
        .layout_no_wrap(
            text.to_owned(),
            egui::FontId::proportional(style::LABEL_FONT + 1.3),
            style::TEXT,
        )
        .size()
        .x
}

/// How wide [`color_button`] is with `label` (or without one).
fn color_button_width(ui: &egui::Ui, label: Option<&str>) -> f32 {
    let text = label.map_or(0.0, |l| label_width(ui, l) + PAD);
    PAD + STAR + PAD + text + CHEVRON + PAD
}

/// A colour button: its star, its name (unless `label` is None) and a chevron, with the
/// chosen colour as a line under the name, or the star without one (a dim line for the
/// object's own colour). Highlighted under the pointer.
fn color_button(
    ui: &mut egui::Ui,
    icon: Icon,
    label: Option<&str>,
    color: Option<Rgba>,
) -> egui::Response {
    let width = color_button_width(ui, label);
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(width, style::FIELD_H), egui::Sense::click());
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(rect, egui::CornerRadius::ZERO, style::BLUE_HOVER);
    }
    let cy = rect.center().y;
    let square = |x: f32, side: f32| {
        egui::Rect::from_min_size(egui::pos2(x, cy - side / 2.0), Vec2::splat(side))
    };
    let star = square(rect.left() + PAD, STAR);
    icons::paint(p, star, icon, style::TEXT);
    // The underline goes under the name, or under the star when there is none.
    let mut under = star.x_range();
    if let Some(label) = label {
        let galley = p.layout_no_wrap(
            label.to_owned(),
            egui::FontId::proportional(style::LABEL_FONT + 1.3),
            style::TEXT,
        );
        let at = egui::pos2(star.right() + PAD, cy - galley.size().y / 2.0);
        under = egui::Rangef::new(at.x, at.x + galley.size().x);
        p.galley(at, galley, style::TEXT);
    }
    let line = egui::Rect::from_x_y_ranges(
        under,
        egui::Rangef::new(rect.bottom() - 4.0, rect.bottom() - 1.0),
    );
    let fill = color.map_or(style::BLUE_FIELD, |c| Color32::from_rgb(c[0], c[1], c[2]));
    p.rect_filled(line, egui::CornerRadius::ZERO, fill);
    icons::paint(
        p,
        square(rect.right() - PAD - CHEVRON, CHEVRON),
        Icon::ChevronDown,
        style::TEXT,
    );
    resp
}

/// Space between one label-and-control pair and the next in a group.
const GAP: f32 = 10.0;
/// Space between groups, which the original keeps apart without separators.
const GROUP: f32 = 16.0;

/// One row of the bar: as tall as a field, so labels line up with the fields beside them.
fn row(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.set_min_height(style::FIELD_H);
        ui.spacing_mut().item_spacing.x = 6.0;
        add(ui);
    });
}

/// An on/off option: the switch, then its name.
fn toggle(ui: &mut egui::Ui, on: &mut bool, name: &str) -> egui::Response {
    style::switch(ui, on, name, name)
}

/// The width of a [`style::dropdown`] that shows the longest of `names` in full: their
/// text, its inset and the chevron.
fn fit(ui: &egui::Ui, names: &[&str]) -> f32 {
    let widest = names
        .iter()
        .map(|n| {
            ui.painter()
                .layout_no_wrap(
                    (*n).to_owned(),
                    egui::FontId::proportional(style::MENU_FONT),
                    style::TEXT,
                )
                .size()
                .x
        })
        .fold(0.0, f32::max);
    (widest + 10.0 + 34.0).ceil()
}
