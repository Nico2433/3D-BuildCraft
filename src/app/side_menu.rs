//! Left menu (☰), a blue column over the window, and the dark panel it opens beside it for
//! Settings, recent files or the model's details.

use super::icons::Icon;
use super::style;
use super::*;

/// The dark panel beside the left menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Flyout {
    Settings,
    Recent,
    About,
}

impl App {
    /// Opens `f` beside the menu, or closes it when it is the one open.
    pub(super) fn toggle_flyout(&mut self, f: Flyout) {
        self.flyout = (self.flyout != Some(f)).then_some(f);
        if f == Flyout::About {
            self.about_draft = self.scene.file_info(&self.scene_title());
        }
    }

    fn close_menu(&mut self) {
        self.menu_open = false;
        self.flyout = None;
    }

    pub(super) fn side_menu(&mut self, ctx: &egui::Context) {
        if !self.menu_open {
            return;
        }
        let lang = self.settings.lang();
        let screen = ctx.screen_rect();
        let top = screen.top() + style::TITLE_H;
        // Two parts that never overlap, so neither covers the other's buttons: the top
        // groups, and the last group held at the bottom as in the original.
        const BOTTOM_H: f32 = 60.0;
        let column = egui::Rect::from_min_max(
            egui::pos2(screen.left(), top),
            egui::pos2(screen.left() + style::MENU_W, screen.bottom() - BOTTOM_H),
        );
        let bottom = egui::Rect::from_min_max(
            egui::pos2(screen.left(), column.bottom()),
            egui::pos2(column.right(), screen.bottom()),
        );
        egui::Area::new(egui::Id::new("menu"))
            .fixed_pos(column.min)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                ui.set_min_size(column.size());
                ui.painter()
                    .rect_filled(column, egui::CornerRadius::ZERO, style::BLUE);
                ui.spacing_mut().item_spacing.y = 0.0;
                if style::icon_button(
                    ui,
                    Icon::Menu,
                    Vec2::splat(style::TAB_H),
                    true,
                    style::BLUE_HOVER,
                )
                .clicked()
                {
                    self.menu_open = false;
                }
                ui.add_space(8.0);
                let mut pick = None;
                let groups: [&[(Icon, &str, Option<Action>)]; 3] = [
                    &[
                        (Icon::NewScene, tr(lang, "New scene"), Some(Action::New)),
                        (Icon::Open, tr(lang, "Open"), Some(Action::Open)),
                        (Icon::Save, tr(lang, "Save"), Some(Action::Save)),
                        (Icon::SaveAs, tr(lang, "Save as"), Some(Action::SaveAs)),
                    ],
                    &[
                        (Icon::Print3D, tr(lang, "3D print"), Some(Action::OpenPrint)),
                        (
                            Icon::Printer,
                            tr(lang, "Print on paper"),
                            Some(Action::OpenPaper),
                        ),
                    ],
                    &[
                        (
                            Icon::AboutModel,
                            tr(lang, "About the model"),
                            Some(Action::AboutModel),
                        ),
                        (
                            Icon::PrinterStatus,
                            tr(lang, "3D printer status"),
                            Some(Action::ShowPrintStatus),
                        ),
                    ],
                ];
                for group in groups {
                    for (icon, label, action) in group {
                        // Open shows the recent files first, like the original's Open page.
                        let flyout = match action {
                            Some(Action::Open) if !self.settings.recent.is_empty() => {
                                Some(Flyout::Recent)
                            }
                            Some(Action::AboutModel) => Some(Flyout::About),
                            _ => None,
                        };
                        let r = menu_item(
                            ui,
                            *icon,
                            label,
                            action.is_some(),
                            flyout.is_some() && flyout == self.flyout,
                        );
                        if let Some(f) = flyout.filter(|_| r.clicked()) {
                            self.toggle_flyout(f);
                        } else if r.clicked() {
                            pick = action.clone();
                        }
                        if action.is_none() {
                            r.on_hover_text(tr(lang, SOON));
                        }
                    }
                    ui.add_space(20.0);
                }
                if let Some(a) = pick {
                    self.actions.push(a);
                    self.menu_open = false;
                }
            });
        egui::Area::new(egui::Id::new("menu_bottom"))
            .fixed_pos(bottom.min)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                ui.set_min_size(bottom.size());
                ui.painter()
                    .rect_filled(bottom, egui::CornerRadius::ZERO, style::BLUE);
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    ui.add_space(6.0);
                    if menu_item(
                        ui,
                        Icon::Settings,
                        tr(lang, "Settings"),
                        true,
                        self.flyout == Some(Flyout::Settings),
                    )
                    .clicked()
                    {
                        self.toggle_flyout(Flyout::Settings);
                    }
                });
            });
        if let Some(flyout) = self.flyout {
            let panel = egui::Rect::from_min_size(
                egui::pos2(column.right(), top),
                Vec2::new(style::FLYOUT_W, screen.bottom() - top),
            );
            egui::Area::new(egui::Id::new("menu_flyout"))
                .fixed_pos(panel.min)
                .fade_in(false)
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    ui.set_min_size(panel.size());
                    ui.set_max_width(panel.width() - 48.0);
                    ui.painter()
                        .rect_filled(panel, egui::CornerRadius::ZERO, style::FLYOUT);
                    egui::Frame::new()
                        .inner_margin(egui::Margin::symmetric(16, 20))
                        .show(ui, |ui| match flyout {
                            Flyout::Settings => self.settings_panel(ui, ctx),
                            Flyout::Recent => self.recent_panel(ui),
                            Flyout::About => self.about_panel(ui),
                        });
                });
        }
        // A click outside the menu closes it.
        let outside = ctx.input(|i| {
            i.pointer.primary_clicked()
                && i.pointer.interact_pos().is_some_and(|p| {
                    let open_w = if self.flyout.is_some() {
                        style::MENU_W + style::FLYOUT_W
                    } else {
                        style::MENU_W
                    };
                    p.x > screen.left() + open_w && p.y > top
                })
        });
        if outside {
            self.close_menu();
        }
    }

    fn settings_panel(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let lang = self.settings.lang();
        let before = self.settings.clone();
        ui.spacing_mut().item_spacing.y = 0.0;
        let heading = |ui: &mut egui::Ui, t: &str| {
            ui.add_space(20.0);
            ui.label(
                egui::RichText::new(t)
                    .size(style::MENU_FONT)
                    .color(style::TEXT),
            );
            ui.add_space(4.0);
        };
        ui.label(
            egui::RichText::new(tr(lang, "Units"))
                .size(style::MENU_FONT)
                .color(style::TEXT),
        );
        ui.add_space(4.0);
        egui::Grid::new("units")
            .num_columns(2)
            .min_col_width(112.0)
            .spacing(Vec2::new(8.0, 0.0))
            .show(ui, |ui| {
                for (i, u) in Units::ALL.into_iter().enumerate() {
                    if style::radio(ui, self.settings.units == u, tr(lang, u.label())).clicked() {
                        self.settings.units = u;
                    }
                    if i % 2 == 1 {
                        ui.end_row();
                    }
                }
            });
        heading(ui, tr(lang, "App color mode"));
        for (m, t) in [
            (ColorMode::System, tr(lang, "Use system setting")),
            (ColorMode::Light, tr(lang, "Light")),
            (ColorMode::Dark, tr(lang, "Dark")),
        ] {
            if style::radio(ui, self.settings.color_mode == m, t).clicked() {
                self.settings.color_mode = m;
            }
        }
        heading(ui, tr(lang, "Language"));
        for l in Lang::ALL {
            if style::radio(ui, self.settings.lang() == l, l.label()).clicked() {
                self.settings.lang = Some(l);
            }
        }
        heading(ui, tr(lang, "Snap"));
        ui.horizontal(|ui| {
            style::switch(ui, &mut self.settings.snap, tr(lang, "On"), tr(lang, "Off"));
        });
        heading(ui, tr(lang, "Collision"));
        ui.horizontal(|ui| {
            ui.add_enabled_ui(false, |ui| {
                style::switch(
                    ui,
                    &mut self.settings.collision,
                    tr(lang, "On"),
                    tr(lang, "Off"),
                );
            })
            .response
            .on_disabled_hover_text(tr(lang, SOON));
        });
        ui.add_space(30.0);
        let text = |ui: &mut egui::Ui, t: &str| {
            ui.label(
                egui::RichText::new(t)
                    .size(style::MENU_FONT)
                    .color(style::TEXT),
            );
        };
        text(ui, &format!("3D BuildCraft {}", env!("CARGO_PKG_VERSION")));
        text(
            ui,
            tr(lang, "Build, repair and prepare 3D models for printing."),
        );
        if self.settings != before {
            apply_theme(ctx, self.settings.color_mode);
            let _ = self.settings.save();
        }
    }

    /// The 3MF metadata of the scene, written into the file when it is saved.
    fn about_panel(&mut self, ui: &mut egui::Ui) {
        let lang = self.settings.lang();
        ui.spacing_mut().item_spacing.y = 0.0;
        let width = ui.available_width();
        for (name, value) in self.about_draft.fields_mut() {
            ui.label(
                egui::RichText::new(tr(lang, about_label(name)))
                    .size(style::MENU_FONT)
                    .color(style::TEXT),
            );
            ui.add_space(4.0);
            let edit = if name == "Description" {
                egui::TextEdit::multiline(value).desired_rows(5)
            } else {
                egui::TextEdit::singleline(value)
            };
            ui.add(edit.desired_width(width).margin(Vec2::new(8.0, 6.0)));
            ui.add_space(14.0);
        }
        ui.add_space(6.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
            let update = egui::Button::new(
                egui::RichText::new(tr(lang, "Update"))
                    .size(style::MENU_FONT)
                    .color(style::TEXT),
            );
            let changed = self.about_draft != self.scene.file_info(&self.scene_title());
            if ui.add_enabled(changed, update).clicked() {
                self.scene.info = self.about_draft.clone();
                self.scene.dirty = true;
            }
        });
    }

    fn recent_panel(&mut self, ui: &mut egui::Ui) {
        let lang = self.settings.lang();
        ui.label(
            egui::RichText::new(tr(lang, "Recent"))
                .size(style::MENU_FONT + 2.0)
                .color(style::TEXT),
        );
        ui.add_space(8.0);
        let browse = egui::Button::new(
            egui::RichText::new(tr(lang, "Browse files…"))
                .size(style::MENU_FONT)
                .color(style::TEXT),
        );
        if ui.add(browse).clicked() {
            self.actions.push(Action::Open);
            self.close_menu();
        }
        ui.add_space(12.0);
        for r in self.settings.recent.clone().into_iter().take(12) {
            let resp = ui
                .add(
                    egui::Button::new(
                        egui::RichText::new(&r.title)
                            .size(style::MENU_FONT)
                            .color(style::TEXT),
                    )
                    .frame(false),
                )
                .on_hover_text(r.path.display().to_string());
            if resp.clicked() {
                self.actions.push(Action::OpenPath(r.path));
                self.close_menu();
            }
            ui.add_space(4.0);
        }
    }
}

/// A row of the left menu: icon and label, full width.
fn menu_item(
    ui: &mut egui::Ui,
    icon: Icon,
    label: &str,
    enabled: bool,
    active: bool,
) -> egui::Response {
    let color = if enabled {
        style::TEXT
    } else {
        style::TEXT_ON_BLUE_DIM
    };
    let galley = ui.painter().layout(
        label.to_owned(),
        egui::FontId::proportional(style::MENU_FONT),
        color,
        style::MENU_W - 64.0,
    );
    let height = (galley.size().y + 16.0).max(40.0);
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(style::MENU_W, height), sense);
    ui.painter().rect_filled(
        rect,
        egui::CornerRadius::ZERO,
        style::interaction_fill(&resp, enabled, active, style::TAB_BAR),
    );
    super::icons::paint(
        ui.painter(),
        egui::Rect::from_center_size(
            egui::pos2(rect.left() + 24.0, rect.center().y),
            Vec2::splat(20.0),
        ),
        icon,
        color,
    );
    ui.painter().galley(
        egui::pos2(rect.left() + 48.0, rect.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
    resp
}

/// The label of a 3MF metadata field, as the original names it.
fn about_label(name: &str) -> &'static str {
    match name {
        "Title" => "Title",
        "Designer" => "Designer",
        "Copyright" => "Copyright",
        "LicenseTerms" => "License terms",
        _ => "Description",
    }
}
