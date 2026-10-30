//! The 3D print view: a navigation column, the open page, the scene with its view buttons,
//! and a black bar with the cost, warnings and actions. Print saves the scene as 3MF and
//! opens it in the chosen slicer, where it is sliced and sent; there is no online service.

use super::icons::Icon;
use super::style;
use super::*;

const NAV: Color32 = Color32::from_rgb(0x1D, 0x1D, 0x1D);
const PAGE: Color32 = Color32::from_rgb(0x1F, 0x1F, 0x1F);
const FIELD: Color32 = Color32::from_rgb(0x13, 0x13, 0x13);
const FIELD_BORDER: Color32 = Color32::from_rgb(0x79, 0x79, 0x79);
const BUTTON: Color32 = Color32::from_rgb(0x5A, 0x5A, 0x5A);
const SELECTED_BAR: Color32 = Color32::from_rgb(0x88, 0x17, 0x98);
const COST: Color32 = Color32::from_rgb(0x3A, 0x8D, 0xDB);
const WARNING: Color32 = Color32::from_rgb(0xD9, 0x3E, 0x05);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Printer,
    Material,
    Design,
    About,
}

impl Page {
    pub fn from_name(name: &str) -> Option<Page> {
        Some(match name {
            "printer" => Page::Printer,
            "material" => Page::Material,
            "design" => Page::Design,
            "about" => Page::About,
            _ => return None,
        })
    }
}

impl App {
    pub(super) fn send_to_slicer(&mut self) {
        let lang = self.settings.lang();
        let Some(slicer) = self.settings.slicer.clone() else {
            return;
        };
        let title = self.scene_title();
        let file: String = title
            .chars()
            .filter(|c| c.is_alphanumeric() || " -_".contains(*c))
            .collect();
        let dir = crate::settings::dirs()
            .map(|d| d.cache_dir().join("print"))
            .unwrap_or_else(|| std::env::temp_dir().join("buildcraft3d-print"));
        let path = dir.join(format!(
            "{}.3mf",
            if file.is_empty() { "print" } else { &file }
        ));
        let sent = std::fs::create_dir_all(&dir)
            .map_err(anyhow::Error::from)
            .and_then(|()| io::save(&path, &self.scene.to_models(), Some(&title)))
            .and_then(|()| crate::slicer::open(&slicer, &path));
        match sent {
            Ok(()) => {
                let name = crate::slicer::from_path(&slicer).name;
                self.toast(format!("{} {name}", tr(lang, "Opened in")), false);
            }
            Err(e) => self.toast(format!("{} {e:#}", tr(lang, "Could not print:")), true),
        }
    }

    pub(super) fn print_view(&mut self, ctx: &egui::Context) {
        let lang = self.settings.lang();
        let Some(page) = self.print else {
            return;
        };
        let warnings = self.print_warnings();

        egui::TopBottomPanel::bottom("print_bar")
            .exact_height(62.0)
            .frame(
                egui::Frame::new()
                    .fill(Color32::BLACK)
                    .inner_margin(egui::Margin::symmetric(20, 0)),
            )
            .show_separator_line(false)
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "{} {}",
                            tr(lang, "Estimated cost:"),
                            tr(lang, "Not available")
                        ))
                        .size(19.0)
                        .color(COST),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if bar_button(ui, tr(lang, "Cancel"), true).clicked() {
                            self.print = None;
                        }
                        let ready =
                            self.settings.slicer.is_some() && !self.scene.objects.is_empty();
                        let print = bar_button(ui, tr(lang, "Print"), ready);
                        if print.clicked() {
                            self.actions.push(Action::SendToSlicer);
                        }
                        if self.settings.slicer.is_none() {
                            print.on_hover_text(tr(lang, "Choose a slicer first."));
                        }
                        bar_button(ui, tr(lang, "Order online"), false)
                            .on_hover_text(tr(lang, SOON));
                        style::separator(ui, 40.0, FIELD_BORDER);
                        if style::stacked(ui, Icon::Redo, tr(lang, "Redo"), self.history.can_redo())
                            .clicked()
                        {
                            self.actions.push(Action::Redo);
                        }
                        if style::stacked(ui, Icon::Undo, tr(lang, "Undo"), self.history.can_undo())
                            .clicked()
                        {
                            self.actions.push(Action::Undo);
                        }
                        let label = format!(
                            "{} {}",
                            warnings.len(),
                            tr(
                                lang,
                                if warnings.len() == 1 {
                                    "warning"
                                } else {
                                    "warnings"
                                }
                            )
                        );
                        let r = style::stacked(ui, Icon::Warning, &label, true);
                        if !warnings.is_empty() {
                            let line = egui::Rect::from_min_max(
                                egui::pos2(r.rect.left() + 6.0, r.rect.bottom() - 3.0),
                                egui::pos2(r.rect.right() - 6.0, r.rect.bottom()),
                            );
                            ui.painter()
                                .rect_filled(line, egui::CornerRadius::ZERO, WARNING);
                            r.on_hover_text(warnings.join("\n"));
                        }
                    });
                });
            });

        egui::SidePanel::left("print_nav")
            .resizable(false)
            .exact_width(230.0)
            .frame(egui::Frame::new().fill(NAV))
            .show_separator_line(false)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                style::icon_button(
                    ui,
                    Icon::Menu,
                    Vec2::splat(style::TAB_H),
                    true,
                    style::RIBBON_HOVER,
                );
                ui.add_space(8.0);
                for (p, icon, label) in [
                    (Some(Page::Printer), Icon::Print3D, tr(lang, "Printer")),
                    (Some(Page::Material), Icon::Material, tr(lang, "Material")),
                    (Some(Page::Design), Icon::Custom, tr(lang, "Design")),
                    (None, Icon::PrinterStatus, tr(lang, "3D printer status")),
                    (Some(Page::About), Icon::Info, tr(lang, "About")),
                ] {
                    let selected = p == Some(page);
                    if nav_item(ui, icon, label, selected).clicked() {
                        match p {
                            Some(p) => self.print = Some(p),
                            None => self.print_status_open = true,
                        }
                    }
                }
            });

        let width = if page == Page::Design { 200.0 } else { 373.0 };
        egui::SidePanel::left("print_page")
            .resizable(false)
            .exact_width(width)
            .frame(egui::Frame::new().fill(PAGE).inner_margin(egui::Margin {
                left: 18,
                right: 18,
                top: 24,
                bottom: 12,
            }))
            .show_separator_line(false)
            .show(ctx, |ui| {
                ui.visuals_mut().override_text_color = Some(style::TEXT);
                dark_fields(ui);
                match page {
                    Page::Printer => self.printer_page(ui),
                    Page::Material => self.material_page(ui),
                    Page::Design => self.design_page(ui),
                    Page::About => self.about_page(ui),
                }
            });

        egui::TopBottomPanel::bottom("print_tools")
            .exact_height(42.0)
            .frame(egui::Frame::new().fill(PAGE))
            .show_separator_line(false)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.x = 20.0;
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let size = Vec2::new(48.0, 40.0);
                    style::icon_button(ui, Icon::More, size, false, style::RIBBON_HOVER)
                        .on_hover_text(tr(lang, SOON));
                    if style::icon_button(ui, Icon::Pencil, size, true, style::RIBBON_HOVER)
                        .on_hover_text(tr(lang, "Edit"))
                        .clicked()
                    {
                        self.print = None;
                    }
                    if style::icon_button(ui, Icon::ZoomOut, size, true, style::RIBBON_HOVER)
                        .clicked()
                    {
                        self.camera.zoom(-120.0);
                    }
                    if style::icon_button(ui, Icon::ZoomIn, size, true, style::RIBBON_HOVER)
                        .clicked()
                    {
                        self.camera.zoom(120.0);
                    }
                    if style::icon_button(ui, Icon::Rotate, size, true, style::RIBBON_HOVER)
                        .on_hover_text(tr(lang, "Center view"))
                        .clicked()
                    {
                        self.actions.push(Action::Frame);
                    }
                });
            });
    }

    /// What the original warns about before printing that we can tell: broken meshes and
    /// objects that do not sit on the floor.
    fn print_warnings(&self) -> Vec<String> {
        let lang = self.settings.lang();
        let mut out = Vec::new();
        if !self.invalid.is_empty() {
            out.push(format!(
                "{} {}",
                self.invalid.len(),
                tr(lang, "objects are not valid; repair them.")
            ));
        }
        let floating = self
            .scene
            .objects
            .iter()
            .filter(|o| o.world_aabb().min.z.abs() > 0.05)
            .count();
        if floating > 0 {
            out.push(format!(
                "{floating} {}",
                tr(lang, "objects do not sit on the floor.")
            ));
        }
        out
    }

    fn printer_page(&mut self, ui: &mut egui::Ui) {
        let lang = self.settings.lang();
        title(ui, tr(lang, "Printer selection"));
        text(ui, tr(lang, "Choose the slicer that prints the model"));
        ui.add_space(12.0);
        card(ui, tr(lang, "Online 3D print service"), false).on_hover_text(tr(lang, SOON));
        ui.add_space(24.0);
        heading_info(ui, tr(lang, "Installed slicers"));
        ui.add_space(8.0);
        let mut shown = self.slicers.clone();
        // One picked by hand stays on the list even when it is not one we look for.
        if let Some(chosen) = &self.settings.slicer {
            if !shown.iter().any(|s| &s.path == chosen) {
                shown.push(crate::slicer::from_path(chosen));
            }
        }
        if shown.is_empty() {
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                text(ui, tr(lang, "None"));
            });
        }
        for s in shown {
            let chosen = self.settings.slicer.as_ref() == Some(&s.path);
            let r = card(ui, &s.name, chosen).on_hover_text(s.path.display().to_string());
            if r.clicked() {
                self.actions.push(Action::ChooseSlicer(s.path));
            }
            ui.add_space(8.0);
        }
        ui.add_space(16.0);
        ui.horizontal(|ui| {
            if page_button(ui, tr(lang, "Refresh")).clicked() {
                self.actions.push(Action::DetectSlicers);
            }
            if page_button(ui, tr(lang, "Browse…")).clicked() {
                self.actions.push(Action::BrowseSlicer);
            }
        });
    }

    fn material_page(&mut self, ui: &mut egui::Ui) {
        let lang = self.settings.lang();
        title(ui, tr(lang, "Material"));
        text(ui, tr(lang, "Material type"));
        ui.add_enabled_ui(false, |ui| {
            egui::ComboBox::from_id_salt("print_material")
                .width(248.0)
                .selected_text("")
                .show_ui(ui, |_| {});
        });
        ui.add_space(40.0);
        text(ui, tr(lang, "Select finish and color"));
    }

    fn design_page(&mut self, ui: &mut egui::Ui) {
        let lang = self.settings.lang();
        title(ui, tr(lang, "Design"));
        text(ui, tr(lang, "Units"));
        let mut units = self.settings.units;
        egui::ComboBox::from_id_salt("print_units")
            .width(160.0)
            .selected_text(tr(lang, units.name()))
            .show_ui(ui, |ui| {
                for u in Units::ALL {
                    ui.selectable_value(&mut units, u, tr(lang, u.name()));
                }
            });
        if units != self.settings.units {
            self.settings.units = units;
            let _ = self.settings.save();
        }
        let k = units.to_mm();
        let b = self.scene.scene_aabb();
        if !b.is_valid() {
            return;
        }
        let size = b.size();
        // An edit scales from the size it started at, and becomes one undo step when it
        // first changes something.
        let base = self.design_edit.as_ref().map_or(size, |(base, _, _)| *base);
        let mut target: Option<f32> = None;
        let mut touched = false;
        let mut scale = size.x / base.x.max(1e-6) * 100.0;
        ui.add_space(10.0);
        text(ui, tr(lang, "Scale factor"));
        let r = ui.add_sized(
            [160.0, 30.0],
            egui::DragValue::new(&mut scale)
                .speed(0.5)
                .range(1.0..=10_000.0)
                .clamp_existing_to_range(false)
                .update_while_editing(false)
                .suffix("%"),
        );
        touched |= r.has_focus() || r.dragged();
        if r.changed() {
            target = Some(scale / 100.0);
        }
        for (label, i) in [
            (tr(lang, "Width"), 0),
            (tr(lang, "Height"), 2),
            (tr(lang, "Depth"), 1),
        ] {
            ui.add_space(10.0);
            text(ui, label);
            let mut v = size[i] / k;
            let r = ui.add_enabled(
                // A flat side cannot set the others.
                base[i] > 1e-6,
                egui::DragValue::new(&mut v)
                    .speed(0.2)
                    .range(0.001..=1.0e6)
                    .clamp_existing_to_range(false)
                    .update_while_editing(false)
                    .suffix(format!(" {}", tr(lang, units.label()))),
            );
            touched |= r.has_focus() || r.dragged();
            if r.changed() {
                // Uniform, like the original: one size sets all three.
                target = Some(v * k / base[i]);
            }
        }
        match (touched, &mut self.design_edit) {
            (true, None) => self.design_edit = Some((size, self.snapshot(), false)),
            (false, Some(_)) => self.design_edit = None,
            _ => {}
        }
        if let (Some(f), Some((base, before, pushed))) = (target, &mut self.design_edit) {
            let now = self.scene.scene_aabb().size().x.max(1e-6);
            if !*pushed {
                self.history.push(before.clone());
                *pushed = true;
            }
            self.scene.scale_all(base.x * f / now);
        }
        ui.add_space(30.0);
        if page_button(ui, tr(lang, "Reset orientation")).clicked() {
            self.edit(Scene::reset_orientation);
        }
        ui.add_space(12.0);
        if page_button(ui, tr(lang, "Reset scale")).clicked() {
            self.edit(Scene::reset_scale);
        }
    }

    fn about_page(&mut self, ui: &mut egui::Ui) {
        let lang = self.settings.lang();
        title(ui, tr(lang, "About"));
        let units = self.settings.units;
        let b = self.scene.scene_aabb();
        let triangles: usize = self
            .scene
            .objects
            .iter()
            .map(|o| o.mesh.triangles.len())
            .sum();
        let volume: Option<f64> = self
            .scene
            .objects
            .iter()
            .map(|o| {
                crate::scene::cached_stats(&o.mesh)
                    .map(|s| s.volume * f64::from(o.scale.element_product().abs()))
            })
            .sum();
        if volume.is_none() {
            ui.ctx().request_repaint_after(Duration::from_millis(50));
        }
        text(ui, &self.scene_title());
        ui.add_space(10.0);
        for (label, value) in [
            (tr(lang, "Objects"), self.scene.objects.len().to_string()),
            (tr(lang, "Triangles"), triangles.to_string()),
            (
                tr(lang, "Size"),
                if b.is_valid() {
                    let s = b.size() / units.to_mm();
                    format!(
                        "{:.1} × {:.1} × {:.1} {}",
                        s.x,
                        s.y,
                        s.z,
                        tr(lang, units.label())
                    )
                } else {
                    "-".into()
                },
            ),
            (
                tr(lang, "Volume"),
                volume
                    .map(|v| format!("{:.1} cm³", v / 1000.0))
                    .unwrap_or_else(|| "…".into()),
            ),
        ] {
            ui.label(
                egui::RichText::new(label)
                    .size(style::LABEL_FONT)
                    .color(style::TEXT_DIM),
            );
            text(ui, &value);
            ui.add_space(8.0);
        }
        ui.add_space(20.0);
        text(ui, &format!("3D BuildCraft {}", env!("CARGO_PKG_VERSION")));
    }

    /// The "3D printer status" window: no print jobs to show yet.
    pub(super) fn print_status(&mut self, ctx: &egui::Context) {
        if !self.print_status_open {
            return;
        }
        let lang = self.settings.lang();
        let mut open = true;
        egui::Window::new(format!("{} - 3D BuildCraft", tr(lang, "3D printer status")))
            .id(egui::Id::new("print_status"))
            .open(&mut open)
            .collapsible(false)
            .default_size([800.0, 600.0])
            .frame(egui::Frame::window(&ctx.style()).fill(PAGE))
            .show(ctx, |ui| {
                ui.visuals_mut().override_text_color = Some(style::TEXT);
                style::icon_button(
                    ui,
                    Icon::Rotate,
                    Vec2::splat(36.0),
                    true,
                    style::RIBBON_HOVER,
                );
                ui.add_space(16.0);
                text(ui, tr(lang, "There are no current 3D print jobs."));
                ui.allocate_space(ui.available_size());
            });
        self.print_status_open = open;
    }
}

/// A printer or slicer the user can pick; the chosen one has a coloured border.
fn card(ui: &mut egui::Ui, label: &str, chosen: bool) -> egui::Response {
    let (card, resp) = ui.allocate_exact_size(Vec2::new(248.0, 62.0), Sense::click());
    let p = ui.painter();
    let fill = if resp.hovered() { 0x2B } else { 0x20 };
    p.rect_filled(card, egui::CornerRadius::ZERO, Color32::from_gray(fill));
    let border = if chosen {
        style::TOGGLE_ON
    } else {
        Color32::from_gray(0x50)
    };
    p.rect_stroke(
        card,
        egui::CornerRadius::ZERO,
        Stroke::new(1.5_f32, border),
        egui::StrokeKind::Inside,
    );
    let icon = egui::pos2(card.left() + 28.0, card.center().y);
    p.circle_filled(icon, 20.0, Color32::from_rgb(0x50, 0x4F, 0x4F));
    super::icons::paint(
        p,
        egui::Rect::from_center_size(icon, Vec2::splat(22.0)),
        Icon::Print3D,
        style::TEXT,
    );
    let galley = p.layout(
        label.to_owned(),
        egui::FontId::proportional(style::MENU_FONT),
        style::TEXT,
        card.width() - 70.0,
    );
    p.galley(
        egui::pos2(card.left() + 60.0, card.center().y - galley.size().y / 2.0),
        galley,
        style::TEXT,
    );
    resp
}

fn title(ui: &mut egui::Ui, t: &str) {
    ui.label(egui::RichText::new(t).size(30.0).color(style::TEXT));
    ui.add_space(12.0);
}

fn text(ui: &mut egui::Ui, t: &str) {
    ui.label(
        egui::RichText::new(t)
            .size(style::MENU_FONT)
            .color(style::TEXT),
    );
}

fn heading_info(ui: &mut egui::Ui, t: &str) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(t).size(20.0).color(style::TEXT));
        ui.add_space(16.0);
        let (r, _) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::hover());
        super::icons::paint(ui.painter(), r, Icon::Info, style::TEXT);
    });
}

fn page_button(ui: &mut egui::Ui, t: &str) -> egui::Response {
    ui.add(
        egui::Button::new(
            egui::RichText::new(t)
                .size(style::MENU_FONT)
                .color(style::TEXT),
        )
        .fill(BUTTON)
        .min_size(Vec2::new(0.0, 30.0))
        .corner_radius(0.0),
    )
}

/// A grey button in the black bar.
fn bar_button(ui: &mut egui::Ui, t: &str, enabled: bool) -> egui::Response {
    let color = if enabled {
        style::TEXT
    } else {
        style::TEXT_DIM
    };
    ui.add_enabled(
        enabled,
        egui::Button::new(egui::RichText::new(t).size(style::MENU_FONT).color(color))
            .fill(BUTTON)
            .stroke(Stroke::NONE)
            .min_size(Vec2::new(0.0, 32.0))
            .corner_radius(0.0),
    )
}

fn nav_item(ui: &mut egui::Ui, icon: Icon, label: &str, selected: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(230.0, 40.0), Sense::click());
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(rect, egui::CornerRadius::ZERO, style::RIBBON_HOVER);
    }
    if selected {
        p.rect_filled(
            egui::Rect::from_min_size(rect.min, Vec2::new(4.0, rect.height())),
            egui::CornerRadius::ZERO,
            SELECTED_BAR,
        );
    }
    super::icons::paint(
        p,
        egui::Rect::from_center_size(
            egui::pos2(rect.left() + 24.0, rect.center().y),
            Vec2::splat(20.0),
        ),
        icon,
        style::TEXT,
    );
    p.text(
        egui::pos2(rect.left() + 48.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(style::MENU_FONT),
        style::TEXT,
    );
    resp
}

/// Dark, square fields with a grey border, as on the print pages.
fn dark_fields(ui: &mut egui::Ui) {
    let v = ui.visuals_mut();
    v.extreme_bg_color = FIELD;
    for w in [
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.bg_fill = FIELD;
        w.weak_bg_fill = FIELD;
        w.bg_stroke = Stroke::new(1.5_f32, FIELD_BORDER);
        w.corner_radius = egui::CornerRadius::ZERO;
        w.fg_stroke = Stroke::new(1.0_f32, style::TEXT);
    }
    v.widgets.hovered.bg_stroke = Stroke::new(1.5_f32, style::TEXT);
    ui.spacing_mut().interact_size.y = 30.0;
}
