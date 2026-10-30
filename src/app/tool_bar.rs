//! The blue bar that replaces the tabs while a tool is open: Cancel and the tool's action on
//! the left, its settings in the middle and, for some tools, Undo and Redo on the right.

use super::icons::Icon;
use super::style;
use super::*;

impl App {
    pub(super) fn capture_photo(&mut self) {
        let lang = self.settings.lang();
        if let Some(Tool::Camera { cam, .. }) = &self.tool {
            if let Some(Ok((_, frame))) = cam.latest() {
                match save_photo(&frame) {
                    Ok(path) => self.tool = Some(Tool::image(path)),
                    Err(e) => self.toast(format!("{} {e:#}", tr(lang, "Could not save:")), true),
                }
            }
        }
    }

    pub(super) fn apply_custom(&mut self) {
        let lang = self.settings.lang();
        let shape = match &self.tool {
            Some(Tool::Custom {
                shape,
                preview: Ok(mesh),
                ..
            }) => Some((tr(lang, shape.label()).to_owned(), mesh.clone())),
            Some(Tool::Image {
                opts,
                preview: Ok(mesh),
                ..
            }) => Some((
                opts.path
                    .file_stem()
                    .map_or_else(String::new, |s| s.to_string_lossy().into_owned()),
                mesh.clone(),
            )),
            _ => None,
        };
        if let Some((name, mesh)) = shape {
            let mesh = (*mesh).clone();
            self.edit(|s| {
                s.insert_on_floor(Object::new(name, mesh, crate::scene::DEFAULT_COLOR));
            });
            self.tool = None;
        }
    }

    pub(super) fn tool_bar(&mut self, ctx: &egui::Context) {
        let lang = self.settings.lang();
        egui::TopBottomPanel::top("tool")
            .min_height(style::TOOL_H)
            .frame(
                egui::Frame::new()
                    .fill(style::BLUE)
                    .inner_margin(egui::Margin {
                        left: 4,
                        right: 4,
                        top: 0,
                        bottom: 4,
                    }),
            )
            .show_separator_line(false)
            .show(ctx, |ui| {
                style::blue_fields(ui);
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.horizontal(|ui| {
                    let confirm: Option<(&str, Action, bool)> = if !self.pending_import.is_empty() {
                        Some((tr(lang, "Import model"), Action::ImportConfirm, true))
                    } else {
                        match &self.tool {
                            Some(Tool::Cut { .. }) => {
                                Some((tr(lang, "Split"), Action::ApplyCut, true))
                            }
                            Some(Tool::Simplify { .. }) => {
                                Some((tr(lang, "Reduce faces"), Action::ApplySimplify, true))
                            }
                            Some(Tool::Hollow { .. }) => {
                                Some((tr(lang, "Hollow"), Action::ApplyHollow, true))
                            }
                            Some(Tool::Emboss { preview, .. }) => {
                                Some((tr(lang, "Emboss"), Action::ApplyEmboss, preview.is_ok()))
                            }
                            Some(Tool::Settle { .. }) => {
                                Some((tr(lang, "Settle"), Action::ApplySettle, true))
                            }
                            Some(Tool::Paint { .. }) => {
                                Some((tr(lang, "Color"), Action::ApplyPaint, true))
                            }
                            Some(Tool::Custom { preview, .. }) => {
                                Some((tr(lang, "Add"), Action::ApplyCustom, preview.is_ok()))
                            }
                            Some(Tool::Image { preview, .. }) => Some((
                                tr(lang, "Import image"),
                                Action::ApplyCustom,
                                preview.is_ok(),
                            )),
                            Some(Tool::Camera { cam, .. }) => Some((
                                tr(lang, "Capture"),
                                Action::Capture,
                                matches!(cam.latest(), Some(Ok(_))),
                            )),
                            Some(Tool::Measure { .. }) | None => None,
                        }
                    };
                    let cancel = if self.pending_import.is_empty() {
                        Action::CancelTool
                    } else {
                        Action::ImportCancel
                    };
                    if style::stacked(ui, Icon::Back, tr(lang, "Cancel"), true).clicked() {
                        self.actions.push(cancel);
                    }
                    if let Some((label, action, enabled)) = confirm {
                        if style::stacked(ui, Icon::Check, label, enabled).clicked() {
                            self.actions.push(action);
                        }
                    }
                    style::separator(ui, style::TOOL_H, style::BLUE_FIELD_BORDER);
                    let undo = matches!(
                        self.tool,
                        Some(
                            Tool::Cut { .. }
                                | Tool::Emboss { .. }
                                | Tool::Settle { .. }
                                | Tool::Paint { .. }
                                | Tool::Custom { .. }
                        )
                    ) && self.pending_import.is_empty();
                    // The settings take the middle; Undo and Redo sit at the right edge.
                    let right = if undo { 260.0 } else { 0.0 };
                    let width = (ui.available_width() - right).max(100.0);
                    ui.allocate_ui_with_layout(
                        Vec2::new(width, style::TOOL_H),
                        egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true),
                        |ui| self.tool_settings(ui),
                    );
                    if undo {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            if style::stacked(
                                ui,
                                Icon::Redo,
                                tr(lang, "Redo"),
                                self.history.can_redo(),
                            )
                            .clicked()
                            {
                                self.actions.push(Action::Redo);
                            }
                            if style::stacked(
                                ui,
                                Icon::Undo,
                                tr(lang, "Undo"),
                                self.history.can_undo(),
                            )
                            .clicked()
                            {
                                self.actions.push(Action::Undo);
                            }
                            style::separator(ui, style::TOOL_H, style::BLUE_FIELD_BORDER);
                        });
                    }
                });
            });
    }

    /// The open tool's settings, laid out in the middle of the tool bar.
    fn tool_settings(&mut self, ui: &mut egui::Ui) {
        let lang = self.settings.lang();
        let units = self.settings.units;
        if let Some(p) = self.pending_import.first_mut() {
            style::label(ui, tr(lang, "File units"));
            ui.add_space(12.0);
            for u in Units::ALL {
                if unit_choice(ui, tr(lang, u.label()), p.units == u).clicked() {
                    p.units = u;
                }
            }
            return;
        }
        match &mut self.tool {
            Some(Tool::Cut { keep, .. }) => {
                for (k, label) in [
                    (Keep::Top, tr(lang, "Keep top")),
                    (Keep::Bottom, tr(lang, "Keep bottom")),
                    (Keep::Both, tr(lang, "Keep both")),
                ] {
                    if style::option(ui, Icon::KeepTop, label, *keep == k).clicked() {
                        *keep = k;
                    }
                }
            }
            Some(Tool::Simplify { level }) => {
                ui.add_space(12.0);
                style::label(ui, tr(lang, "Reduction level"));
                ui.spacing_mut().slider_width = 360.0;
                ui.add(egui::Slider::new(level, 0.0..=0.99).show_value(false));
                style::separator(ui, style::TOOL_H, style::BLUE_FIELD_BORDER);
                let faces: usize = self.scene.selected().map(|o| o.mesh.triangles.len()).sum();
                let kept = (faces as f32 * (1.0 - *level)) as usize;
                ui.vertical(|ui| {
                    ui.add_space(6.0);
                    style::label(ui, &format!("{}   {kept}", tr(lang, "Number of faces")));
                    style::label(
                        ui,
                        &format!("{}   {faces}", tr(lang, "Original number of faces")),
                    );
                });
            }
            Some(Tool::Hollow { wall }) => {
                ui.add_space(12.0);
                style::label(ui, tr(lang, "Minimum wall thickness"));
                ui.spacing_mut().slider_width = 360.0;
                ui.add(egui::Slider::new(wall, 0.5..=10.0).show_value(false));
                style::separator(ui, style::TOOL_H, style::BLUE_FIELD_BORDER);
                style::label(ui, &format!("{wall:.1}  mm"));
            }
            Some(Tool::Measure { points }) => {
                let d = if points.len() == 2 {
                    points[0].distance(points[1]) / units.to_mm()
                } else {
                    0.0
                };
                ui.add_space(12.0);
                style::label(
                    ui,
                    &format!(
                        "{}    {d:.2}    {}",
                        tr(lang, "Distance"),
                        tr(lang, units.label())
                    ),
                );
                style::separator(ui, style::TOOL_H, style::BLUE_FIELD_BORDER);
                style::label(ui, tr(lang, "Snap"));
                if style::switch(ui, &mut self.settings.snap, tr(lang, "On"), tr(lang, "Off"))
                    .changed()
                {
                    let _ = self.settings.save();
                }
            }
            Some(Tool::Custom {
                shape,
                lat,
                lon,
                width,
                height,
                roundness,
                wire,
                preview,
                built,
            }) => {
                ui.add_space(12.0);
                // Two rows, as the original's.
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 7.0;
                    ui.horizontal(|ui| {
                        style::label(ui, tr(lang, "Wireframe"));
                        style::switch(ui, wire, tr(lang, "On"), tr(lang, "Off"));
                        style::separator(ui, style::FIELD_H, style::BLUE_FIELD_BORDER);
                        style::label(ui, tr(lang, "Shape"));
                        let names = CUSTOM_SHAPES.map(|p| tr(lang, p.label()));
                        let mut k = CUSTOM_SHAPES.iter().position(|p| p == shape).unwrap_or(0);
                        if style::dropdown(ui, "custom_shape", 82.0, &names, &mut k) {
                            *shape = CUSTOM_SHAPES[k];
                        }
                        for (label, v) in [
                            (tr(lang, "Latitude"), &mut *lat),
                            (tr(lang, "Longitude"), &mut *lon),
                        ] {
                            style::label(ui, label);
                            number_dropdown(ui, label, &SEGMENTS, v);
                        }
                        style::label(ui, tr(lang, "Width"));
                        style::unit_field(ui, width, "mm", 1.0..=1000.0);
                    });
                    ui.horizontal(|ui| {
                        style::label(ui, tr(lang, "Height"));
                        style::unit_field(ui, height, "mm", 1.0..=1000.0);
                        style::label(ui, tr(lang, "Roundness"));
                        let mut r = *roundness as usize;
                        number_dropdown(
                            ui,
                            "roundness",
                            &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
                            &mut r,
                        );
                        *roundness = r as u8;
                    });
                });
                let key = (*shape, *lat, *lon, *width, *height, *roundness);
                let dragging = ui.input(|i| i.pointer.any_down());
                if built.as_ref() != Some(&key) && !dragging {
                    let mesh = crate::primitives::custom(*shape, *lat, *lon, *width, *height);
                    *preview = crate::solid::round(&mesh, *roundness as f32 / 10.0)
                        .map(std::sync::Arc::new)
                        .map_err(|e| format!("{e:#}"));
                    *built = Some(key);
                }
            }
            Some(Tool::Image {
                opts,
                preview,
                built,
            }) => {
                use crate::image3d::Mode;
                ui.add_space(12.0);
                // Two rows, as the original's.
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 7.0;
                    ui.horizontal(|ui| {
                        style::label(ui, tr(lang, "Method"));
                        let names = Mode::ALL.map(|m| tr(lang, m.label()));
                        let mut k = Mode::ALL.iter().position(|m| *m == opts.mode).unwrap_or(0);
                        if style::dropdown(ui, "image_method", 180.0, &names, &mut k) {
                            opts.mode = Mode::ALL[k];
                        }
                        ui.add_space(24.0);
                        style::label(ui, tr(lang, "Levels"));
                        let mut levels = opts.levels as f32;
                        style::slider(ui, &mut levels, 2.0..=8.0, 250.0, 0);
                        opts.levels = levels.round() as u32;
                        ui.add_space(24.0);
                        style::label(ui, tr(lang, "Smooth"));
                        style::slider(ui, &mut opts.smooth, 0.0..=1.0, 250.0, 9);
                    });
                    ui.horizontal(|ui| {
                        style::label(ui, tr(lang, "Textures"));
                        style::switch(ui, &mut opts.textures, tr(lang, "On"), tr(lang, "Off"));
                        ui.add_space(24.0);
                        style::label(ui, tr(lang, "Invert"));
                        style::switch(ui, &mut opts.invert, tr(lang, "On"), tr(lang, "Off"));
                    });
                });
                // Rebuilding decodes the image again: wait until a drag ends.
                let dragging = ui.input(|i| i.pointer.any_down());
                if built.as_ref() != Some(opts) && !dragging {
                    *preview = crate::image3d::build(opts)
                        .map(std::sync::Arc::new)
                        .map_err(|e| format!("{e:#}"));
                    *built = Some(opts.clone());
                }
            }
            Some(Tool::Paint {
                color,
                coverage,
                menu,
                dropper,
                ..
            }) => {
                ui.add_space(8.0);
                let recent = &self.settings.recent_colors;
                if super::paint_menu::selected_color(ui, lang, color, menu, recent) {
                    *dropper = true;
                }
                style::separator(ui, style::FIELD_H, style::BLUE_FIELD_BORDER);
                style::label(ui, tr(lang, "Coverage"));
                style::slider(ui, coverage, 0.0..=1.0, 148.0, 0);
            }
            Some(Tool::Emboss {
                params,
                preview,
                built,
            }) => {
                style::label(ui, tr(lang, "Pattern"));
                let pattern_name = match &params.pattern {
                    Pattern::Text => tr(lang, "Text"),
                    Pattern::Shape(s) => tr(lang, s.label()),
                    Pattern::Image(_) => tr(lang, "Image"),
                };
                egui::ComboBox::from_id_salt("emboss_pattern")
                    .selected_text(pattern_name)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut params.pattern, Pattern::Text, tr(lang, "Text"));
                        for s in Shape::ALL {
                            ui.selectable_value(
                                &mut params.pattern,
                                Pattern::Shape(s),
                                tr(lang, s.label()),
                            );
                        }
                        let image = matches!(params.pattern, Pattern::Image(_));
                        if ui.selectable_label(image, tr(lang, "Image")).clicked() {
                            if let Some(p) = rfd::FileDialog::new()
                                .add_filter(tr(lang, "Image"), &crate::image3d::EXTENSIONS)
                                .pick_file()
                            {
                                params.pattern = Pattern::Image(p);
                            }
                        }
                    });
                style::label(ui, tr(lang, "Projection"));
                egui::ComboBox::from_id_salt("emboss_projection")
                    .width(150.0)
                    .selected_text(tr(lang, params.projection.label()))
                    .show_ui(ui, |ui| {
                        for p in Projection::ALL {
                            ui.selectable_value(&mut params.projection, p, tr(lang, p.label()));
                        }
                    });
                if style::option(ui, Icon::Pyramid, tr(lang, "Bevel"), params.bevel).clicked() {
                    params.bevel = !params.bevel;
                }
                if style::option(ui, Icon::Hollow, tr(lang, "Engrave"), params.engrave).clicked() {
                    params.engrave = !params.engrave;
                }
                ui.end_row();
                if params.pattern == Pattern::Text {
                    style::label(ui, tr(lang, "Text"));
                    ui.add(egui::TextEdit::singleline(&mut params.text).desired_width(200.0));
                    style::label(ui, tr(lang, "Font"));
                    let fonts = self
                        .font_list
                        .get_or_insert_with(crate::emboss::font_families);
                    egui::ComboBox::from_id_salt("emboss_font")
                        .width(160.0)
                        .selected_text(params.font.as_deref().unwrap_or(tr(lang, "Built-in")))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut params.font, None, tr(lang, "Built-in"));
                            for f in fonts.iter() {
                                ui.selectable_value(&mut params.font, Some(f.clone()), f);
                            }
                        });
                }
                if !params.engrave {
                    style::label(ui, tr(lang, "Color"));
                    for (salt, label, color) in [
                        ("emboss_top", tr(lang, "Top"), &mut params.top_color),
                        ("emboss_sides", tr(lang, "Sides"), &mut params.side_color),
                    ] {
                        style::label(ui, label);
                        color_combo(ui, salt, lang, color);
                    }
                }
                let target = match self.scene.selection.len() {
                    1 => self.scene.selected().next(),
                    _ => None,
                };
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
            Some(Tool::Camera { .. } | Tool::Settle { .. }) | None => {}
        }
    }
}

/// A unit choice in the import bar: plain text, the chosen one boxed.
fn unit_choice(ui: &mut egui::Ui, text: &str, selected: bool) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        egui::FontId::proportional(style::LABEL_FONT + 1.3),
        style::TEXT,
    );
    let (rect, resp) = ui.allocate_exact_size(
        Vec2::new(galley.size().x.max(24.0) + 32.0, 48.0),
        Sense::click(),
    );
    if selected {
        ui.painter().rect_stroke(
            rect.shrink(2.0),
            egui::CornerRadius::ZERO,
            Stroke::new(2.0_f32, Color32::from_rgb(0x1F, 0x7A, 0x8C)),
            egui::StrokeKind::Inside,
        );
    } else if resp.hovered() {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::ZERO, style::BLUE_HOVER);
    }
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, style::TEXT);
    resp
}

/// A colour picker for an emboss part: the object's colour or one from the palette.
fn color_combo(ui: &mut egui::Ui, salt: &str, lang: Lang, color: &mut Option<Rgba>) {
    let name = |c: &Option<Rgba>| match c {
        None => tr(lang, "Object color"),
        Some(c) => PALETTE
            .iter()
            .find(|(_, rgba)| rgba == c)
            .map_or("?", |(n, _)| tr(lang, n)),
    };
    egui::ComboBox::from_id_salt(salt)
        .width(130.0)
        .selected_text(name(color))
        .show_ui(ui, |ui| {
            ui.selectable_value(color, None, tr(lang, "Object color"));
            for (n, c) in PALETTE {
                ui.selectable_value(color, Some(c), tr(lang, n));
            }
        });
}

/// The shapes Custom offers, in the original's order.
const CUSTOM_SHAPES: [Primitive; 5] = [
    Primitive::Cube,
    Primitive::Sphere,
    Primitive::Cone,
    Primitive::Torus,
    Primitive::Cylinder,
];

/// Latitude and longitude steps offered by Custom.
const SEGMENTS: [usize; 12] = [3, 4, 5, 6, 8, 10, 12, 16, 20, 24, 32, 48];

/// A drop-down of whole numbers; a value not in the list shows as the nearest one.
fn number_dropdown(ui: &mut egui::Ui, id: &str, values: &[usize], v: &mut usize) {
    let names: Vec<String> = values.iter().map(usize::to_string).collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let mut k = (0..values.len())
        .min_by_key(|&k| values[k].abs_diff(*v))
        .unwrap_or(0);
    if style::dropdown(ui, id, 66.0, &names, &mut k) {
        *v = values[k];
    }
}
