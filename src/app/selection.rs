//! Selection pane on the right: a narrow strip of icons with "Selection" written down it,
//! or, open, the same actions with labels and a tile per object.

use super::icons::Icon;
use super::style;
use super::*;

impl App {
    pub(super) fn selection_pane(&mut self, ctx: &egui::Context) {
        if self.tool.is_some() || !self.pending_import.is_empty() {
            return;
        }
        let lang = self.settings.lang();
        let open = self.settings.selection_pane_open;
        egui::SidePanel::right("selection")
            .resizable(false)
            .exact_width(if open { style::PANEL_W } else { style::STRIP_W })
            .frame(egui::Frame::new().fill(if open { style::PANEL } else { style::STRIP }))
            .show_separator_line(false)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let w = ui.available_width();
                ui.add_space(10.0);
                let chevron = if open {
                    Icon::ChevronRight
                } else {
                    Icon::ChevronLeft
                };
                if style::icon_button_in_bar(ui, chevron, Vec2::new(w, 44.0), true, style::PANE_BAR)
                    .clicked()
                {
                    self.settings.selection_pane_open = !open;
                    let _ = self.settings.save();
                }
                ui.add_space(8.0);
                let items: [(Icon, &str, Option<Action>); 6] = [
                    (Icon::Group, tr(lang, "Group"), Some(Action::Group)),
                    (Icon::Ungroup, tr(lang, "Ungroup"), Some(Action::Ungroup)),
                    (
                        Icon::SelectAll,
                        tr(lang, "Select all"),
                        Some(Action::SelectAll),
                    ),
                    (
                        Icon::SelectNone,
                        tr(lang, "Select none"),
                        Some(Action::SelectNone),
                    ),
                    (
                        Icon::Invert,
                        tr(lang, "Invert selection"),
                        Some(Action::Invert),
                    ),
                    (Icon::SpecialSelect, tr(lang, "Special selection"), None),
                ];
                for (icon, label, action) in items {
                    let enabled = match &action {
                        Some(Action::Group) => self.scene.selection.len() >= 2,
                        Some(Action::Ungroup) => self.scene.can_ungroup(),
                        _ => action.is_some(),
                    };
                    // Object selection is the mode the original shows as current.
                    let current = icon == Icon::SpecialSelect;
                    let r = pane_item(ui, icon, if open { label } else { "" }, enabled, current, w);
                    if r.clicked() {
                        self.actions.extend(action.clone());
                    }
                    let r = if open { r } else { r.on_hover_text(label) };
                    if action.is_none() {
                        r.on_hover_text(tr(lang, SOON));
                    }
                }
                if !open {
                    // "Selection" written bottom to top under the icons.
                    let galley = ui.painter().layout_no_wrap(
                        tr(lang, "Selection").to_owned(),
                        egui::FontId::proportional(21.0),
                        Color32::from_gray(0x9A),
                    );
                    let x = ui.max_rect().center().x - galley.size().y / 2.0;
                    let y = ui.cursor().top() + 8.0 + galley.size().x;
                    let mut text =
                        egui::epaint::TextShape::new(egui::pos2(x, y), galley, Color32::WHITE);
                    text.angle = -std::f32::consts::FRAC_PI_2;
                    ui.painter().add(text);
                    return;
                }
                ui.add_space(4.0);
                let (rect, response) = ui.allocate_exact_size(Vec2::new(w, 32.0), Sense::click());
                if response.clicked() {
                    self.items_open = !self.items_open;
                }
                ui.painter().rect_filled(
                    rect,
                    egui::CornerRadius::ZERO,
                    style::interaction_fill(&response, true, false, style::PANE_BAR),
                );
                ui.painter().text(
                    egui::pos2(rect.left() + 8.0, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    tr(lang, "Items"),
                    egui::FontId::proportional(style::LABEL_FONT),
                    style::TEXT,
                );
                super::icons::paint(
                    ui.painter(),
                    egui::Rect::from_center_size(
                        egui::pos2(rect.right() - 16.0, rect.center().y),
                        Vec2::splat(16.0),
                    ),
                    if self.items_open {
                        Icon::ChevronUp
                    } else {
                        Icon::ChevronDown
                    },
                    style::TEXT,
                );
                if !self.items_open {
                    return;
                }
                ui.add_space(8.0);
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let add = ui.input(|i| i.modifiers.command || i.modifiers.shift);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::splat(6.0);
                        ui.add_space(6.0);
                        let thumbs = &mut self.thumbs;
                        for o in &self.scene.objects {
                            let (rect, resp) =
                                ui.allocate_exact_size(Vec2::splat(60.0), Sense::click());
                            let selected = self.scene.selection.contains(&o.id);
                            let p = ui.painter();
                            p.rect_filled(rect, egui::CornerRadius::ZERO, Color32::WHITE);
                            p.rect_filled(
                                rect,
                                egui::CornerRadius::ZERO,
                                style::interaction_fill(&resp, true, selected, style::PANE_BAR),
                            );
                            match thumbs.object(o) {
                                Some(id) => {
                                    p.image(
                                        id,
                                        rect.shrink(3.0),
                                        egui::Rect::from_min_max(
                                            egui::pos2(0.0, 0.0),
                                            egui::pos2(1.0, 1.0),
                                        ),
                                        Color32::WHITE,
                                    );
                                }
                                None => {
                                    let c = o.color;
                                    super::icons::paint(
                                        p,
                                        rect.shrink(12.0),
                                        Icon::Cube,
                                        Color32::from_rgb(c[0], c[1], c[2]),
                                    );
                                }
                            }
                            if self.invalid.contains(&o.id) {
                                p.rect_stroke(
                                    rect,
                                    egui::CornerRadius::ZERO,
                                    Stroke::new(2.0_f32, style::CARD_ERROR),
                                    egui::StrokeKind::Inside,
                                );
                            }
                            let resp = resp.on_hover_ui(|ui| {
                                let Some(s) = crate::scene::cached_stats(&o.mesh) else {
                                    ui.label(&o.name);
                                    ui.spinner();
                                    ui.ctx().request_repaint_after(Duration::from_millis(50));
                                    return;
                                };
                                let mut tip = format!(
                                    "{}\n{} {} · {} {}",
                                    o.name,
                                    s.triangles,
                                    tr(lang, "triangles"),
                                    s.vertices,
                                    tr(lang, "vertices")
                                );
                                if !s.is_closed_manifold() {
                                    tip += &format!(
                                        "\n{}: {} · {}: {} · {}: {}",
                                        tr(lang, "Open edges"),
                                        s.boundary_edges,
                                        tr(lang, "non-manifold"),
                                        s.nonmanifold_edges,
                                        tr(lang, "misoriented"),
                                        s.misoriented_edges
                                    );
                                }
                                ui.label(tip);
                            });
                            if resp.clicked() {
                                self.actions.push(Action::Select(o.id, add));
                            }
                            // As in the view: a double click centres it.
                            if resp.double_clicked() {
                                self.actions.push(Action::Select(o.id, false));
                                self.actions.push(Action::Frame);
                            }
                        }
                    });
                });
            });
    }
}

/// A row of the selection pane: the icon, and the label when the pane is open.
fn pane_item(
    ui: &mut egui::Ui,
    icon: Icon,
    label: &str,
    enabled: bool,
    current: bool,
    width: f32,
) -> egui::Response {
    let color = if enabled || current {
        style::TEXT
    } else {
        style::TEXT_DIM
    };
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 48.0), sense);
    ui.painter().rect_filled(
        rect,
        egui::CornerRadius::ZERO,
        style::interaction_fill(&resp, enabled, current, style::PANE_BAR),
    );
    let icon_x = rect.left() + style::STRIP_W / 2.0;
    super::icons::paint(
        ui.painter(),
        egui::Rect::from_center_size(egui::pos2(icon_x, rect.center().y), Vec2::splat(18.0)),
        icon,
        color,
    );
    if !label.is_empty() {
        let galley = ui.painter().layout(
            label.to_owned(),
            egui::FontId::proportional(style::LABEL_FONT),
            color,
            width - style::STRIP_W - 4.0,
        );
        ui.painter().galley(
            egui::pos2(icon_x + 22.0, rect.center().y - galley.size().y / 2.0),
            galley,
            color,
        );
    }
    resp
}
