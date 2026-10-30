//! Ribbon under the tabs: one row of icon buttons per tab, as in the original.

use super::icons::Icon;
use super::style;
use super::*;

impl App {
    pub(super) fn ribbon(&mut self, ctx: &egui::Context) {
        if self.ribbon_hidden
            || self.tool.is_some()
            || !self.pending_import.is_empty()
            || self.start_visible()
        {
            return;
        }
        let lang = self.settings.lang();
        let mut opened = false;
        egui::TopBottomPanel::top("ribbon")
            .exact_height(style::RIBBON_H)
            .frame(
                egui::Frame::new()
                    .fill(style::RIBBON)
                    .inner_margin(egui::Margin::symmetric(4, 0)),
            )
            .show_separator_line(false)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.horizontal_centered(|ui| {
                    let has_sel = !self.scene.selection.is_empty();
                    let two = self.scene.selection.len() >= 2;
                    let one = self.scene.selection.len() == 1;
                    let sep =
                        |ui: &mut egui::Ui| style::separator(ui, style::RIBBON_H, style::SEPARATOR);
                    // A button per (icon, label, action, enabled); no action is "coming soon".
                    let mut clicked: Vec<Action> = Vec::new();
                    let soon = tr(lang, SOON);
                    let mut row =
                        |ui: &mut egui::Ui, items: &[(Icon, &str, Option<Action>, bool)]| {
                            for (icon, label, action, enabled) in items {
                                let enabled = *enabled && action.is_some();
                                let r = style::ribbon(ui, *icon, label, enabled, false);
                                if r.clicked() {
                                    clicked.extend(action.clone());
                                }
                                if action.is_none() {
                                    r.on_hover_text(soon);
                                }
                            }
                        };
                    match self.tab {
                        Tab::Insert => {
                            if style::ribbon(ui, Icon::Add, tr(lang, "Add"), true, false).clicked()
                            {
                                self.add_open = true;
                                // Folders may have changed since the last look.
                                self.library.refresh();
                            }
                            sep(ui);
                            row(
                                ui,
                                &[(
                                    Icon::Custom,
                                    tr(lang, "Custom"),
                                    Some(Action::StartCustom),
                                    true,
                                )],
                            );
                            sep(ui);
                            for p in Primitive::ALL {
                                let icon = match p {
                                    Primitive::Cube => Icon::Cube,
                                    Primitive::Cylinder => Icon::Cylinder,
                                    Primitive::Pyramid => Icon::Pyramid,
                                    Primitive::Cone => Icon::Cone,
                                    Primitive::Sphere => Icon::Sphere,
                                    Primitive::Hexagon => Icon::Hexagon,
                                    Primitive::Wedge => Icon::Wedge,
                                    Primitive::Torus => Icon::Torus,
                                    Primitive::Tetrahedron => Icon::Tetrahedron,
                                };
                                row(
                                    ui,
                                    &[(icon, tr(lang, p.label()), Some(Action::Insert(p)), true)],
                                );
                            }
                        }
                        Tab::Object => {
                            let paste = !self.clipboard.is_empty();
                            row(
                                ui,
                                &[
                                    (
                                        Icon::Duplicate,
                                        tr(lang, "Duplicate"),
                                        Some(Action::Duplicate),
                                        has_sel,
                                    ),
                                    (Icon::Copy, tr(lang, "Copy"), Some(Action::Copy), has_sel),
                                    (Icon::Cut, tr(lang, "Cut"), Some(Action::Cut), has_sel),
                                    (Icon::Paste, tr(lang, "Paste"), Some(Action::Paste), paste),
                                    (
                                        Icon::Delete,
                                        tr(lang, "Delete"),
                                        Some(Action::Delete),
                                        has_sel,
                                    ),
                                ],
                            );
                            sep(ui);
                            row(
                                ui,
                                &[(
                                    Icon::Settle,
                                    tr(lang, "Settle"),
                                    Some(Action::Settle),
                                    has_sel,
                                )],
                            );
                            let mirror =
                                style::ribbon(ui, Icon::Mirror, tr(lang, "Mirror"), has_sel, false);
                            // The axes drop down under the button, as the other lists.
                            let open_id = egui::Id::new("mirror_axes");
                            let mut open =
                                ui.data(|d| d.get_temp::<bool>(open_id)).unwrap_or(false);
                            if mirror.clicked() {
                                open = !open;
                            }
                            let mut mirror_axis = None;
                            if open {
                                let axes = [Axis::X, Axis::Y, Axis::Z];
                                let names = ["X axis", "Y axis", "Z axis"].map(|n| tr(lang, n));
                                mirror_axis = style::list(
                                    ui.ctx(),
                                    open_id,
                                    mirror.rect.left_bottom(),
                                    140.0,
                                    &names,
                                    None,
                                    &mut open,
                                    mirror.clicked(),
                                )
                                .map(|k| axes[k]);
                            }
                            ui.data_mut(|d| d.insert_temp(open_id, open));
                            row(
                                ui,
                                &[(
                                    Icon::Measure,
                                    tr(lang, "Measure"),
                                    Some(Action::Measure),
                                    true,
                                )],
                            );
                            if let Some(a) = mirror_axis {
                                row(ui, &[]);
                                self.actions.push(Action::Mirror(a));
                            }
                        }
                        Tab::Edit => row(
                            ui,
                            &[
                                (
                                    Icon::Simplify,
                                    tr(lang, "Simplify"),
                                    Some(Action::StartSimplify),
                                    has_sel,
                                ),
                                (
                                    Icon::Split,
                                    tr(lang, "Split"),
                                    Some(Action::StartCut),
                                    has_sel,
                                ),
                                (
                                    Icon::Smooth,
                                    tr(lang, "Smooth"),
                                    Some(Action::Smooth),
                                    has_sel,
                                ),
                                (
                                    Icon::Emboss,
                                    tr(lang, "Emboss"),
                                    Some(Action::StartEmboss),
                                    one,
                                ),
                                (
                                    Icon::ExtrudeDown,
                                    tr(lang, "Extrude down"),
                                    Some(Action::ExtrudeDown),
                                    has_sel,
                                ),
                                (
                                    Icon::Merge,
                                    tr(lang, "Merge"),
                                    Some(Action::Boolean(Op::Union)),
                                    two,
                                ),
                                (
                                    Icon::Intersect,
                                    tr(lang, "Intersect"),
                                    Some(Action::Boolean(Op::Intersect)),
                                    two,
                                ),
                                (
                                    Icon::Subtract,
                                    tr(lang, "Subtract"),
                                    Some(Action::Boolean(Op::Subtract)),
                                    has_sel,
                                ),
                                (
                                    Icon::Hollow,
                                    tr(lang, "Hollow"),
                                    Some(Action::StartHollow),
                                    has_sel,
                                ),
                            ],
                        ),
                        Tab::Paint => {
                            row(ui, &[(Icon::Material, tr(lang, "Material"), None, true)]);
                            sep(ui);
                            row(
                                ui,
                                &[
                                    (
                                        Icon::Color,
                                        tr(lang, "Color"),
                                        Some(Action::StartPaint),
                                        true,
                                    ),
                                    (Icon::Texture, tr(lang, "Texture"), None, true),
                                ],
                            );
                        }
                        Tab::View => {
                            row(
                                ui,
                                &[(
                                    Icon::Eye,
                                    tr(lang, "Center view"),
                                    Some(Action::Frame),
                                    true,
                                )],
                            );
                            sep(ui);
                            let r = &mut self.settings.render;
                            let mut changed = false;
                            let mut toggle =
                                |ui: &mut egui::Ui, icon, label: &str, v: &mut bool| {
                                    if style::ribbon(ui, icon, label, true, *v).clicked() {
                                        *v = !*v;
                                        changed = true;
                                    }
                                };
                            toggle(ui, Icon::Shading, tr(lang, "Shading"), &mut r.shading);
                            toggle(ui, Icon::Shadows, tr(lang, "Shadows"), &mut r.shadows);
                            toggle(ui, Icon::Colors, tr(lang, "Colors"), &mut r.colors);
                            toggle(
                                ui,
                                Icon::Reflections,
                                tr(lang, "Reflections"),
                                &mut r.reflections,
                            );
                            toggle(ui, Icon::Smoothing, tr(lang, "Smoothing"), &mut r.smoothing);
                            sep(ui);
                            toggle(ui, Icon::Wireframe, tr(lang, "Wireframe"), &mut r.wireframe);
                            // Grid opens its properties under it, as in the original.
                            let grid = style::ribbon(ui, Icon::Grid, tr(lang, "Grid"), true, false);
                            sep(ui);
                            toggle(ui, Icon::XRay, tr(lang, "X-ray"), &mut r.xray);
                            if changed {
                                let _ = self.settings.save();
                            }
                            if grid.clicked() || self.grid_menu.is_some() {
                                let at = egui::pos2(grid.rect.center().x, grid.rect.bottom() + 9.0);
                                opened |= self.grid_menu.is_none();
                                self.grid_menu = Some(at);
                            }
                        }
                        Tab::Help => {
                            if style::ribbon(ui, Icon::Help, tr(lang, "Introduction"), true, false)
                                .clicked()
                            {
                                self.about_open = true;
                            }
                            row(
                                ui,
                                &[
                                    (Icon::Info, tr(lang, "Supported printers"), None, true),
                                    (Icon::Guide, tr(lang, "User guide"), None, true),
                                ],
                            );
                        }
                    }
                    drop(row);
                    self.actions.extend(clicked);
                });
            });
        self.grid_flyout(ctx, opened);
    }

    /// "Grid properties": the floor's tone (or none) and its distance labels.
    fn grid_flyout(&mut self, ctx: &egui::Context, just_opened: bool) {
        let Some(at) = self.grid_menu else {
            return;
        };
        let lang = self.settings.lang();
        let size = Vec2::new(236.0, 355.0);
        let area = egui::Area::new(egui::Id::new("grid_flyout"))
            .fixed_pos(at - Vec2::new(size.x / 2.0, 0.0))
            .order(egui::Order::Foreground)
            .fade_in(false)
            .show(ctx, |ui| {
                let (rect, _) = ui.allocate_exact_size(size, Sense::click());
                let p = ui.painter();
                p.rect_filled(rect, egui::CornerRadius::ZERO, Color32::from_gray(0x2B));
                let head = egui::Rect::from_min_size(rect.min, Vec2::new(size.x, 40.0));
                p.rect_filled(head, egui::CornerRadius::ZERO, Color32::from_gray(0x19));
                p.rect_stroke(
                    rect,
                    egui::CornerRadius::ZERO,
                    Stroke::new(1.0_f32, Color32::from_gray(0x76)),
                    egui::StrokeKind::Inside,
                );
                let text = |pos: Pos2, t: &str, size: f32| {
                    p.text(
                        pos,
                        Align2::LEFT_CENTER,
                        t,
                        egui::FontId::proportional(size),
                        style::TEXT,
                    )
                };
                text(
                    head.left_center() + Vec2::new(12.0, 0.0),
                    tr(lang, "Grid properties"),
                    12.0,
                );
                text(
                    rect.min + Vec2::new(20.0, 75.0),
                    tr(lang, "Grid mode"),
                    14.0,
                );
                let r = &mut self.settings.render;
                // (shown, tone): None hides the floor.
                let modes = [
                    (tr(lang, "Grid None"), false, 0),
                    (tr(lang, "Light"), true, 1),
                    (tr(lang, "Dark"), true, 2),
                    (tr(lang, "Default"), true, 0),
                ];
                let mut changed = false;
                for (k, (label, shown, tone)) in modes.into_iter().enumerate() {
                    let row = egui::Rect::from_min_size(
                        rect.min + Vec2::new(20.0, 95.0 + 40.0 * k as f32),
                        Vec2::new(size.x - 40.0, 40.0),
                    );
                    let resp = ui.interact(row, ui.id().with(("grid_mode", k)), Sense::click());
                    let current = r.grid == shown && (!shown || r.grid_tone == tone);
                    let fill = if current {
                        Color32::from_rgb(0x12, 0x4A, 0x62)
                    } else if resp.hovered() {
                        style::RIBBON_HOVER
                    } else {
                        Color32::TRANSPARENT
                    };
                    let p = ui.painter();
                    p.rect_filled(row, egui::CornerRadius::ZERO, fill);
                    let icon = egui::Rect::from_center_size(
                        row.left_center() + Vec2::new(22.0, 0.0),
                        Vec2::splat(20.0),
                    );
                    let (light, dark) = (Color32::from_gray(0x9F), Color32::from_gray(0x86));
                    match k {
                        0 => {
                            let s = Stroke::new(1.5_f32, style::TEXT);
                            p.circle_stroke(icon.center(), 9.0, s);
                            let d = Vec2::splat(6.4);
                            p.line_segment([icon.center() - d, icon.center() + d], s);
                        }
                        1 => {
                            p.rect_filled(icon, egui::CornerRadius::ZERO, light);
                        }
                        2 => {
                            p.rect_filled(icon, egui::CornerRadius::ZERO, dark);
                        }
                        _ => {
                            let (l, rr) = icon.split_left_right_at_fraction(0.5);
                            p.rect_filled(l, egui::CornerRadius::ZERO, light);
                            p.rect_filled(rr, egui::CornerRadius::ZERO, dark);
                        }
                    }
                    p.text(
                        row.left_center() + Vec2::new(44.0, 0.0),
                        Align2::LEFT_CENTER,
                        label,
                        egui::FontId::proportional(14.0),
                        style::TEXT,
                    );
                    if resp.clicked() {
                        (r.grid, r.grid_tone) = (shown, tone);
                        changed = true;
                    }
                }
                ui.painter().text(
                    rect.min + Vec2::new(20.0, 290.0),
                    Align2::LEFT_CENTER,
                    tr(lang, "Show grid labels"),
                    egui::FontId::proportional(14.0),
                    style::TEXT,
                );
                let mut child =
                    ui.new_child(egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
                        rect.min + Vec2::new(20.0, 309.0),
                        Vec2::new(200.0, 24.0),
                    )));
                child.horizontal(|ui| {
                    changed |=
                        style::switch(ui, &mut r.grid_labels, tr(lang, "On"), tr(lang, "Off"))
                            .changed();
                });
                if changed {
                    let _ = self.settings.save();
                }
            });
        if !just_opened && area.response.clicked_elsewhere() {
            self.grid_menu = None;
        }
    }
}
