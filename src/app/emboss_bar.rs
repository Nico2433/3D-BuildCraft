//! Emboss's settings in the tool bar, in two rows as Custom's: what is embossed and how it
//! is sent onto the object above; its size and finish below. Each row keeps a label next
//! to its control, and groups are split by a separator.

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
        }) = &mut self.tool
        else {
            return;
        };
        let font_list = &mut self.font_list;
        ui.add_space(12.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 7.0;
            row(ui, |ui| {
                pattern_row(ui, lang, params, font_list);
                style::separator(ui, style::FIELD_H, style::BLUE_FIELD_BORDER);
                style::label(ui, tr(lang, "Projection"));
                let names = Projection::ALL.map(|p| tr(lang, p.label()));
                let mut k = Projection::ALL
                    .iter()
                    .position(|&p| p == params.projection)
                    .unwrap_or(0);
                if style::dropdown(ui, "emboss_projection", fit(ui, &names), &names, &mut k) {
                    params.projection = Projection::ALL[k];
                }
                ui.add_space(GAP);
                toggle(ui, &mut params.conform, tr(lang, "Conform")).on_hover_text(tr(
                    lang,
                    "Follow the surface: an even height over curved surfaces",
                ));
            });
            row(ui, |ui| {
                style::label(ui, tr(lang, "Size"));
                style::unit_field(ui, "emboss_size", &mut params.size, "mm", 1.0..=200.0);
                ui.add_space(GAP);
                style::label(ui, tr(lang, "Depth"));
                style::unit_field(ui, "emboss_depth", &mut params.depth, "mm", 0.1..=50.0);
                style::separator(ui, style::FIELD_H, style::BLUE_FIELD_BORDER);
                toggle(ui, &mut params.bevel, tr(lang, "Bevel"));
                ui.add_space(GAP);
                toggle(ui, &mut params.engrave, tr(lang, "Engrave"));
                // An engraved pattern has no top or sides of its own to colour.
                if !params.engrave {
                    style::separator(ui, style::FIELD_H, style::BLUE_FIELD_BORDER);
                    for (k, (salt, label, color)) in [
                        ("emboss_top", tr(lang, "Top"), &mut params.top_color),
                        ("emboss_sides", tr(lang, "Sides"), &mut params.side_color),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        if k > 0 {
                            ui.add_space(GAP);
                        }
                        style::label(ui, label);
                        color_dropdown(ui, salt, lang, color);
                    }
                }
            });
        });
        let key = (
            params.clone(),
            target.map(|o| (o.id, std::sync::Arc::as_ptr(&o.mesh) as usize, o.matrix())),
        );
        if built.as_ref() != Some(&key) {
            *preview = match target {
                Some(o) => crate::emboss::text_solid(&o.world_mesh(), params)
                    .map(std::sync::Arc::new)
                    .map_err(|e| format!("{e:#}")),
                None => Err(tr(lang, "Select one object").to_string()),
            };
            *built = Some(key);
        }
    }
}

/// The pattern and, for text, the text and its font; for an image, its file name.
fn pattern_row(
    ui: &mut egui::Ui,
    lang: Lang,
    params: &mut Emboss,
    font_list: &mut Option<Vec<String>>,
) {
    style::label(ui, tr(lang, "Pattern"));
    // Text, the shapes, then Image, which asks for the picture.
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
    match &params.pattern {
        Pattern::Text => {
            ui.add_space(GAP);
            style::label(ui, tr(lang, "Text"));
            style::text_field(ui, &mut params.text, 120.0, "");
            ui.add_space(GAP);
            style::label(ui, tr(lang, "Font"));
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
            if style::dropdown(ui, "emboss_font", width, &names, &mut k) {
                params.font = k.checked_sub(1).map(|k| fonts[k].clone());
            }
        }
        Pattern::Image(path) => {
            ui.add_space(GAP);
            if let Some(name) = path.file_name() {
                style::label(ui, &name.to_string_lossy());
            }
        }
        Pattern::Shape(_) => {}
    }
}

/// Space between one label-and-control pair and the next in a group.
const GAP: f32 = 10.0;

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

/// A colour for an emboss part: the object's own or one from the palette.
fn color_dropdown(ui: &mut egui::Ui, salt: &str, lang: Lang, color: &mut Option<Rgba>) {
    let mut names = vec![tr(lang, "Object")];
    names.extend(PALETTE.iter().map(|(n, _)| tr(lang, n)));
    let mut k = color
        .and_then(|c| PALETTE.iter().position(|(_, rgba)| *rgba == c))
        .map_or(0, |k| k + 1);
    if style::dropdown(ui, salt, fit(ui, &names), &names, &mut k) {
        *color = k.checked_sub(1).map(|k| PALETTE[k].1);
    }
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
