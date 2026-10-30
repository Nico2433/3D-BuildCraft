//! Title bar (the window has no system frame, like the original) and the tab bar: menu
//! button, tabs, 3D print, save, undo and redo.

use super::icons::Icon;
use super::style;
use super::*;

impl App {
    pub(super) fn title_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("title")
            .exact_height(style::TITLE_H)
            .frame(egui::Frame::new().fill(style::BLUE))
            .show_separator_line(false)
            .show(ctx, |ui| {
                let rect = ui.max_rect();
                // Dragging the empty bar moves the window; a double click maximises it.
                let bar = ui.interact(rect, egui::Id::new("title_drag"), Sense::click_and_drag());
                if bar.drag_started_by(PointerButton::Primary) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                if bar.double_clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                }
                ui.painter().text(
                    egui::pos2(rect.left() + 12.0, rect.center().y),
                    Align2::LEFT_CENTER,
                    format!("3D BuildCraft - {}", self.scene_title()),
                    egui::FontId::proportional(12.0),
                    style::TEXT,
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    let size = Vec2::new(46.0, style::TITLE_H);
                    let close_hover = Color32::from_rgb(0xE8, 0x11, 0x23);
                    if style::icon_button(ui, Icon::Close, size, true, close_hover).clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    if style::icon_button(ui, Icon::Maximize, size, true, style::BLUE_HOVER)
                        .clicked()
                    {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                    }
                    if style::icon_button(ui, Icon::Minimize, size, true, style::BLUE_HOVER)
                        .clicked()
                    {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                    }
                });
            });
    }

    pub(super) fn top_bar(&mut self, ctx: &egui::Context) {
        if self.tool.is_some() || !self.pending_import.is_empty() {
            return self.tool_bar(ctx);
        }
        let lang = self.settings.lang();
        egui::TopBottomPanel::top("tabs")
            .exact_height(style::TAB_H)
            .frame(egui::Frame::new().fill(style::BLUE))
            .show_separator_line(false)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.horizontal_centered(|ui| {
                    let square = Vec2::splat(style::TAB_H);
                    if style::icon_button(ui, Icon::Menu, square, true, style::BLUE_HOVER).clicked()
                    {
                        self.menu_open = !self.menu_open;
                    }
                    ui.add_space(6.0);
                    let start = self.start_visible();
                    for t in Tab::ALL {
                        let selected = self.tab == t && !self.ribbon_hidden && !start;
                        if tab(ui, tr(lang, t.label()), selected).clicked() {
                            // Clicking the open tab folds the ribbon away, as in the original.
                            if self.tab == t && !start {
                                self.ribbon_hidden = !self.ribbon_hidden;
                            } else {
                                self.tab = t;
                                self.ribbon_hidden = false;
                                self.start_open = false;
                            }
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(8.0);
                        if style::icon_button(
                            ui,
                            Icon::Redo,
                            square,
                            self.history.can_redo(),
                            style::BLUE_HOVER,
                        )
                        .on_hover_text(tr(lang, "Redo (Ctrl+Y)"))
                        .clicked()
                        {
                            self.actions.push(Action::Redo);
                        }
                        ui.add_space(8.0);
                        if style::icon_button(
                            ui,
                            Icon::Undo,
                            square,
                            self.history.can_undo(),
                            style::BLUE_HOVER,
                        )
                        .on_hover_text(tr(lang, "Undo (Ctrl+Z)"))
                        .clicked()
                        {
                            self.actions.push(Action::Undo);
                        }
                        style::separator(ui, style::TAB_H, style::BLUE_FIELD_BORDER);
                        if style::icon_button(ui, Icon::Save, square, true, style::BLUE_HOVER)
                            .on_hover_text(tr(lang, "Save (Ctrl+S)"))
                            .clicked()
                        {
                            self.actions.push(Action::Save);
                        }
                        if self.settings.ai.enabled {
                            ui.add_space(8.0);
                            if style::icon_button(
                                ui,
                                Icon::Assistant,
                                square,
                                true,
                                style::BLUE_HOVER,
                            )
                            .on_hover_text(tr(lang, "AI assistant"))
                            .clicked()
                            {
                                self.chat.open = !self.chat.open;
                            }
                        }
                        ui.add_space(8.0);
                        if style::ribbon_button(
                            ui,
                            style::TAB_BAR,
                            Icon::Print3D,
                            tr(lang, "3D print"),
                            true,
                            false,
                        )
                        .clicked()
                        {
                            self.actions.push(Action::OpenPrint);
                        }
                    });
                });
            });
    }
}

/// A tab: large text; the open tab takes the ribbon's colour, so they read as one.
fn tab(ui: &mut egui::Ui, text: &str, selected: bool) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        egui::FontId::proportional(style::TAB_FONT),
        style::TEXT,
    );
    let size = Vec2::new(galley.size().x + 36.0, style::TAB_H);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let fill = if selected {
        style::RIBBON
    } else if resp.hovered() {
        style::BLUE_HOVER
    } else {
        Color32::TRANSPARENT
    };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::ZERO, fill);
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, style::TEXT);
    resp
}
