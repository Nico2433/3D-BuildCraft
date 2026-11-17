//! Bottom bar: move, rotate or size the selection, three mode buttons then X, Y and Z with
//! the unit. While splitting, the same bar holds the cut plane's offset, angles and size.

use super::icons::Icon;
use super::style;
use super::*;

impl App {
    pub(super) fn apply_emboss(&mut self) {
        let lang = self.settings.lang();
        if let Some(Tool::Emboss { params, .. }) = &self.tool {
            let params = params.clone();
            let target = match self.scene.selection.len() {
                1 => self.scene.selected().next(),
                _ => None,
            };
            let Some(target) = target else {
                self.toast(tr(lang, "Select one object"), true);
                return;
            };
            let (id, name) = (target.id, target.name.clone());
            // The panel stays open when it fails, so the settings can be changed.
            match crate::emboss::apply(&target.world_mesh(), &params) {
                Ok(out) => {
                    self.edit(|s| {
                        s.get_mut(id).unwrap().set_world_mesh(out.object);
                        s.selection = BTreeSet::from([id]);
                        for (mesh, color) in out.parts {
                            let part = s.add(Object::new(name.clone(), mesh, color));
                            s.selection.insert(part);
                        }
                    });
                    self.refresh_invalid();
                    self.tool = None;
                }
                Err(e) => self.toast(format!("{e:#}"), true),
            }
        }
    }

    pub(super) fn transform_bar(&mut self, ctx: &egui::Context) {
        if let Some(Tool::Cut { .. }) = self.tool {
            return self.cut_bar(ctx);
        }
        if let Some(Tool::Emboss { .. }) = self.tool {
            return self.emboss_bar(ctx);
        }
        if !self.scene.selection_aabb().is_valid()
            || self.tool.is_some()
            || !self.pending_import.is_empty()
        {
            // The editor closes with the bar, not to reopen with the next selection.
            self.bar_edit = None;
            return;
        }
        let lang = self.settings.lang();
        let units = self.settings.units;
        let k = units.to_mm();
        bottom_bar(ctx, |ui| {
            for (m, icon, tip) in [
                (TransformMode::Move, Icon::Move, tr(lang, "Move")),
                (TransformMode::Rotate, Icon::Rotate, tr(lang, "Rotate")),
                (TransformMode::Scale, Icon::Scale, tr(lang, "Scale")),
            ] {
                if mode_button(ui, icon, self.mode == m)
                    .on_hover_text(tip)
                    .clicked()
                {
                    self.mode = m;
                    self.bar_edit = None;
                }
            }
            let b = self.scene.selection_aabb();
            let single = self.scene.selected_ids().len() == 1;
            let unscaled = self.unscaled_size();
            let mut fields = Fields {
                current: &mut self.field,
                edit: &mut self.bar_edit,
                lang,
            };
            let change = match self.mode {
                TransformMode::Move => {
                    let mut c = b.center() / k;
                    c.z = b.min.z / k;
                    let before = c;
                    let set = xyz_fields(ui, ["X", "Y", "Z"], &mut c, &mut fields);
                    unit_label(ui, tr(lang, units.label()));
                    // The Z field is the bottom of the selection, like the original's.
                    let target = Vec3::new(c.x * k, c.y * k, c.z * k + b.size().z * 0.5);
                    (set && c != before).then_some(Action::MoveSelectionTo(target))
                }
                TransformMode::Rotate => {
                    if !single {
                        unit_label(ui, tr(lang, "Select one object"));
                        return;
                    }
                    let mut r = self.scene.selected().next().unwrap().rotation;
                    let before = r;
                    let names = ["Roll", "Pitch", "Yaw"].map(|n| tr(lang, n));
                    let set = xyz_fields(ui, names, &mut r, &mut fields);
                    unit_label(ui, tr(lang, "deg."));
                    let reset = mode_button(ui, Icon::Axes, false)
                        .on_hover_text(tr(lang, "Reset orientation"))
                        .clicked();
                    if reset {
                        r = Vec3::ZERO;
                    }
                    ((set || reset) && r != before).then_some(Action::SetRotation(r))
                }
                TransformMode::Scale => {
                    let size = b.size();
                    // Size per field unit: one unit of length, or 1 % of the unscaled size.
                    let per = if self.scale_percent {
                        unscaled.max(Vec3::splat(1e-6)) / 100.0
                    } else {
                        Vec3::splat(k)
                    };
                    let mut s = size / per;
                    let before = s;
                    let set = xyz_fields(ui, ["X", "Y", "Z"], &mut s, &mut fields);
                    s = s.max(Vec3::splat(0.01));
                    percent_toggle(ui, &mut self.scale_percent, tr(lang, units.label()));
                    lock_toggle(ui, &mut self.lock_scale, tr(lang, "Uniform scale"));
                    let new = if self.lock_scale {
                        // Uniform: apply the ratio of whichever field changed.
                        let ratio = (s / before.max(Vec3::splat(1e-9))).to_array();
                        let f = ratio
                            .into_iter()
                            .find(|r| (r - 1.0).abs() > 1e-6)
                            .unwrap_or(1.0);
                        size * f
                    } else {
                        s * per
                    };
                    (set && s != before).then_some(Action::SetSize(new))
                }
            };
            // One undo step per typed value.
            if let Some(change) = change {
                self.actions.push(Action::BeginEdit);
                self.actions.push(change);
            }
        });
    }

    /// The selection's size if every object had scale 1: what 100 % means.
    fn unscaled_size(&self) -> Vec3 {
        self.scene
            .selected()
            .map(|o| {
                let m = glam::Mat4::from_rotation_translation(o.rotation_quat(), o.translation);
                crate::scene::cached_aabb(&o.mesh, &m)
            })
            .fold(Aabb::empty(), Aabb::union)
            .size()
    }

    /// The split plane: move sets its offset along its normal, rotate its roll, pitch and
    /// yaw, as in the original, and size its width and length, "Full" (infinite) unless
    /// typed: an empty field or 0 makes it full again, as does the button after them.
    fn cut_bar(&mut self, ctx: &egui::Context) {
        let lang = self.settings.lang();
        let units = self.settings.units;
        let mode = &mut self.mode;
        let Some(Tool::Cut { plane, .. }) = &mut self.tool else {
            return;
        };
        let mut f = Fields {
            current: &mut self.field,
            edit: &mut self.bar_edit,
            lang,
        };
        bottom_bar(ctx, |ui| {
            for (m, icon, tip) in [
                (TransformMode::Move, Icon::Move, tr(lang, "Offset")),
                (TransformMode::Rotate, Icon::Rotate, tr(lang, "Rotate")),
                (TransformMode::Scale, Icon::Scale, tr(lang, "Size")),
            ] {
                if mode_button(ui, icon, *mode == m)
                    .on_hover_text(tip)
                    .clicked()
                {
                    *mode = m;
                }
            }
            let k = units.to_mm();
            match mode {
                TransformMode::Rotate => {
                    let names = ["Roll", "Pitch", "Yaw"].map(|n| tr(lang, n));
                    xyz_fields(ui, names, &mut plane.rotation, &mut f);
                    unit_label(ui, tr(lang, "deg."));
                }
                TransformMode::Move => {
                    let mut v = plane.offset / k;
                    if field(ui, &mut f, 0, "", &mut v) {
                        plane.offset = v * k;
                    }
                    unit_label(ui, tr(lang, units.label()));
                }
                TransformMode::Scale => {
                    for (axis, name) in ["Width", "Length"].into_iter().enumerate() {
                        let mut v = plane.extent(axis).map(|e| e / k);
                        if extent_field(ui, &mut f, axis, tr(lang, name), &mut v, tr(lang, "Full"))
                        {
                            plane.set_extent(axis, v.map(|e| e * k));
                        }
                    }
                    unit_label(ui, tr(lang, units.label()));
                    let full = mode_button(ui, Icon::Maximize, plane.size.is_none())
                        .on_hover_text(tr(lang, "Full size"))
                        .clicked();
                    if full {
                        plane.size = None;
                    }
                }
            }
        });
    }
}

impl App {
    /// The pattern's place, turn and size for Emboss, under the same three mode buttons
    /// as objects have: move sets the offset, rotate the angle, size its width, height and
    /// depth as X, Y and Z, in mm or percent and with a lock, as the objects' sizes.
    fn emboss_bar(&mut self, ctx: &egui::Context) {
        let lang = self.settings.lang();
        let mode = &mut self.mode;
        let percent = &mut self.scale_percent;
        let Some(Tool::Emboss {
            params,
            lock,
            opened,
            ..
        }) = &mut self.tool
        else {
            return;
        };
        let mut f = Fields {
            current: &mut self.field,
            edit: &mut self.bar_edit,
            lang,
        };
        bottom_bar(ctx, |ui| {
            for (m, icon, tip) in [
                (TransformMode::Move, Icon::Move, tr(lang, "Offset")),
                (TransformMode::Rotate, Icon::Rotate, tr(lang, "Angle")),
                (TransformMode::Scale, Icon::Scale, tr(lang, "Size")),
            ] {
                if mode_button(ui, icon, *mode == m)
                    .on_hover_text(tip)
                    .clicked()
                {
                    *mode = m;
                }
            }
            match mode {
                TransformMode::Move => {
                    field(ui, &mut f, 0, "X", &mut params.offset.x);
                    field(ui, &mut f, 1, "Y", &mut params.offset.y);
                    unit_label(ui, "mm");
                }
                TransformMode::Rotate => {
                    field(ui, &mut f, 0, "Z", &mut params.angle);
                    params.angle = params.angle.clamp(-180.0, 180.0);
                    unit_label(ui, "°");
                }
                TransformMode::Scale => {
                    // X is the pattern's width, Y its size (capital height for text), Z
                    // its depth; in percent, of what they were when the tool opened.
                    let now = params.extents();
                    let shown = if *percent {
                        now / opened.max(Vec3::splat(1e-6)) * 100.0
                    } else {
                        let width = crate::emboss::pattern_size(params).map_or(now.x, |s| s.x);
                        Vec3::new(width, params.size, params.depth)
                    };
                    let mut v = shown;
                    let set = xyz_fields(ui, ["X", "Y", "Z"], &mut v, &mut f);
                    percent_toggle(ui, percent, "mm");
                    lock_toggle(
                        ui,
                        lock,
                        tr(lang, "Keep the width and height in proportion"),
                    );
                    // The field that changed scales its extent by the same ratio.
                    if let Some(axis) = (0..3).find(|&k| set && v[k] != shown[k]) {
                        let ratio = v[axis].max(1e-3) / shown[axis].max(1e-6);
                        crate::emboss::rescale(params, axis, ratio, *lock);
                    }
                }
            }
        });
    }
}

/// The floating dark bar at the bottom centre.
fn bottom_bar(ctx: &egui::Context, add: impl FnOnce(&mut egui::Ui)) {
    egui::Area::new(egui::Id::new("transform_bar"))
        .anchor(Align2::CENTER_BOTTOM, [0.0, -28.0])
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(style::BOTTOM_BAR)
                .inner_margin(egui::Margin::same(4))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.horizontal(add);
                });
        });
}

fn mode_button(ui: &mut egui::Ui, icon: Icon, active: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(48.0), Sense::click());
    let fill = if active {
        Color32::from_rgb(0x59, 0x62, 0x66)
    } else if resp.hovered() {
        style::RIBBON_HOVER
    } else {
        Color32::TRANSPARENT
    };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::ZERO, fill);
    let color = if ui.is_enabled() {
        style::TEXT
    } else {
        style::TEXT_DIM
    };
    super::icons::paint(ui.painter(), rect.shrink(13.0), icon, color);
    resp
}

fn unit_label(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(48.0, 48.0), Sense::click());
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        egui::FontId::proportional(style::LABEL_FONT + 1.3),
        style::TEXT,
    );
    resp
}

/// The unit of the size fields, `length` or "%", underlined: a click switches them.
fn percent_toggle(ui: &mut egui::Ui, percent: &mut bool, length: &str) {
    let unit = if *percent { "%" } else { length };
    if unit_button(ui, unit).clicked() {
        *percent = !*percent;
    }
}

/// The size lock: closed while sizes keep their proportion; `tip` says which.
fn lock_toggle(ui: &mut egui::Ui, lock: &mut bool, tip: &str) {
    let icon = if *lock { Icon::Lock } else { Icon::Unlock };
    if mode_button(ui, icon, *lock).on_hover_text(tip).clicked() {
        *lock = !*lock;
    }
}

/// The unit, underlined, that switches the size fields between length and percent.
fn unit_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let rect = ui.available_rect_before_wrap();
    let hover = egui::Rect::from_min_size(rect.min, Vec2::splat(48.0));
    if ui.rect_contains_pointer(hover) {
        ui.painter()
            .rect_filled(hover, egui::CornerRadius::ZERO, style::RIBBON_HOVER);
    }
    let resp = unit_label(ui, text);
    let w = ui
        .painter()
        .layout_no_wrap(
            text.to_owned(),
            egui::FontId::proportional(style::LABEL_FONT + 1.3),
            style::TEXT,
        )
        .size()
        .x;
    let y = resp.rect.center().y + 9.0;
    let x = resp.rect.center().x;
    ui.painter().hline(
        (x - w / 2.0)..=(x + w / 2.0),
        y,
        Stroke::new(1.0_f32, style::TEXT),
    );
    resp
}

/// The bar's current (green) field and, while its editor is open, the text typed in it.
struct Fields<'a> {
    current: &'a mut usize,
    edit: &'a mut Option<String>,
    lang: Lang,
}

/// One value: the number, editable; true when a value was set.
fn field(ui: &mut egui::Ui, f: &mut Fields, index: usize, axis: &str, v: &mut f32) -> bool {
    let shown = short_number(*v as f64);
    let typed = field_text(ui, f, index, axis, &shown, false, || {
        short_number_exact(*v as f64)
    });
    match typed.as_deref().and_then(parse_number) {
        Some(n) if n != *v => {
            *v = n;
            true
        }
        _ => false,
    }
}

/// A length that can be full (`None`), shown dimmed as `full`; it edits as an empty text,
/// and an empty one, 0 or less sets full again. True when it changed.
fn extent_field(
    ui: &mut egui::Ui,
    f: &mut Fields,
    index: usize,
    axis: &str,
    v: &mut Option<f32>,
    full: &str,
) -> bool {
    let shown = v.map_or(full.to_owned(), |x| short_number(x as f64));
    let exact = || v.map_or(String::new(), |x| short_number_exact(x as f64));
    let Some(typed) = field_text(ui, f, index, axis, &shown, v.is_none(), exact) else {
        return false;
    };
    let new = match typed.trim() {
        "" => None,
        t => match parse_number(t) {
            Some(n) => Some(n).filter(|n| *n > 0.0),
            None => return false,
        },
    };
    let changed = new != *v;
    *v = new;
    changed
}

/// One field: the axis letter above, `shown`, and a coloured strip at the bottom (green on
/// the current field, blue-grey on the others), `shown` dimmed when `dim`. A click makes it
/// current; a click on the current one opens the editor above the bar with `exact`, as in
/// the original. Returns the text accepted there.
fn field_text(
    ui: &mut egui::Ui,
    f: &mut Fields,
    index: usize,
    axis: &str,
    shown: &str,
    dim: bool,
    exact: impl FnOnce() -> String,
) -> Option<String> {
    let text_w = |text: &str, size: f32| {
        let font = egui::FontId::proportional(size);
        ui.painter()
            .layout_no_wrap(text.to_owned(), font, style::TEXT)
            .size()
            .x
    };
    // Wide enough for the label and for a word such as "Full".
    let width = (text_w(axis, 9.5).max(text_w(shown, style::LABEL_FONT + 1.3)) + 12.0).max(48.0);
    // The bar's top left, for the editor; the bar has a 4 point margin.
    let bar = ui.max_rect().left_top() - Vec2::splat(4.0);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 48.0), Sense::click());
    let current = *f.current == index;
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(rect, egui::CornerRadius::ZERO, style::RIBBON_HOVER);
    }
    let strip = egui::Rect::from_min_max(egui::pos2(rect.left(), rect.bottom() - 17.0), rect.max);
    // The strip fades up into the bar, as in the original.
    let color = if current {
        style::FIELD_GREEN
    } else {
        style::FIELD_BLUE
    };
    let mut fade = egui::Mesh::default();
    for (pos, c) in [
        (strip.left_top(), Color32::TRANSPARENT),
        (strip.right_top(), Color32::TRANSPARENT),
        (strip.right_bottom(), color),
        (strip.left_bottom(), color),
    ] {
        fade.colored_vertex(pos, c);
    }
    fade.add_triangle(0, 1, 2);
    fade.add_triangle(0, 2, 3);
    p.add(fade);
    if !axis.is_empty() {
        p.text(
            egui::pos2(rect.center().x, rect.top() + 7.0),
            Align2::CENTER_CENTER,
            axis,
            egui::FontId::proportional(9.5),
            style::TEXT,
        );
    }
    p.text(
        egui::pos2(rect.center().x, rect.center().y + 4.0),
        Align2::CENTER_CENTER,
        shown,
        egui::FontId::proportional(style::LABEL_FONT + 1.3),
        if dim { style::TEXT_DIM } else { style::TEXT },
    );
    if resp.clicked() {
        if current {
            *f.edit = Some(exact());
        } else {
            *f.current = index;
            *f.edit = None;
        }
    }
    let text = f.edit.as_mut().filter(|_| current)?;
    let typed = editor(ui.ctx(), f.lang, bar, text, resp.clicked())?;
    *f.edit = None;
    typed
}

/// The editor above the bar: a white box with the value selected, a clear cross and
/// Accept. None while open; then the typed text (None if cancelled).
fn editor(
    ctx: &egui::Context,
    lang: Lang,
    bar: Pos2,
    text: &mut String,
    just_opened: bool,
) -> Option<Option<String>> {
    let size = Vec2::new(261.0, 55.0);
    let mut done = None;
    let area = egui::Area::new(egui::Id::new("bar_editor"))
        .fixed_pos(bar + Vec2::new(17.0, -3.0 - size.y))
        .order(egui::Order::Foreground)
        .fade_in(false)
        .show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
            let p = ui.painter();
            p.rect_filled(rect, egui::CornerRadius::ZERO, Color32::from_gray(0x2D));
            p.rect_stroke(
                rect,
                egui::CornerRadius::ZERO,
                Stroke::new(1.0_f32, Color32::from_gray(0x76)),
                egui::StrokeKind::Inside,
            );
            let input =
                egui::Rect::from_min_size(rect.min + Vec2::new(13.0, 10.0), Vec2::new(160.0, 35.0));
            p.rect_filled(input, egui::CornerRadius::ZERO, Color32::WHITE);
            p.rect_stroke(
                input,
                egui::CornerRadius::ZERO,
                Stroke::new(2.0_f32, style::TOGGLE_ON),
                egui::StrokeKind::Inside,
            );
            let id = egui::Id::new("bar_editor_text");
            let mut child =
                ui.new_child(egui::UiBuilder::new().max_rect(input.shrink2(Vec2::new(6.0, 6.0))));
            let w = child.visuals_mut();
            w.override_text_color = Some(Color32::BLACK);
            w.extreme_bg_color = Color32::WHITE;
            w.selection.bg_fill = style::TOGGLE_ON;
            w.selection.stroke = Stroke::new(1.0_f32, Color32::WHITE);
            let r = child.add_sized(
                Vec2::new(input.width() - 40.0, 23.0),
                egui::TextEdit::singleline(text)
                    .id(id)
                    .frame(false)
                    .font(egui::FontId::proportional(style::MENU_FONT)),
            );
            if just_opened {
                // Opens with the whole value selected, ready to be typed over.
                r.request_focus();
                let mut state =
                    egui::text_edit::TextEditState::load(ui.ctx(), id).unwrap_or_default();
                let all = egui::text::CCursorRange::two(
                    egui::text::CCursor::new(0),
                    egui::text::CCursor::new(text.chars().count()),
                );
                state.cursor.set_char_range(Some(all));
                state.store(ui.ctx(), id);
            }
            let clear = egui::Rect::from_center_size(
                input.right_center() - Vec2::new(16.0, 0.0),
                Vec2::splat(20.0),
            );
            let c = ui.interact(clear, id.with("clear"), Sense::click());
            super::icons::paint(
                ui.painter(),
                clear.shrink(4.0),
                Icon::Close,
                Color32::from_gray(0x30),
            );
            if c.clicked() {
                text.clear();
                r.request_focus();
            }
            let accept =
                egui::Rect::from_min_size(rect.min + Vec2::new(184.0, 10.0), Vec2::new(68.0, 35.0));
            let a = ui.interact(accept, id.with("accept"), Sense::click());
            let fill = if a.hovered() {
                Color32::from_gray(0x66)
            } else {
                Color32::from_gray(0x57)
            };
            ui.painter()
                .rect_filled(accept, egui::CornerRadius::ZERO, fill);
            ui.painter().text(
                accept.center(),
                Align2::CENTER_CENTER,
                tr(lang, "Accept"),
                egui::FontId::proportional(style::MENU_FONT),
                Color32::WHITE,
            );
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if a.clicked() || enter {
                done = Some(Some(text.clone()));
            } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                done = Some(None);
            }
        });
    if done.is_none() && !just_opened && area.response.clicked_elsewhere() {
        done = Some(None);
    }
    done
}

/// A typed number, with a comma or a dot for decimals; None unless finite.
fn parse_number(s: &str) -> Option<f32> {
    s.trim()
        .replace(',', ".")
        .parse()
        .ok()
        .filter(|n: &f32| n.is_finite())
}

/// Three fields named `names` for X, Y and Z; true when one was set.
fn xyz_fields(ui: &mut egui::Ui, names: [&str; 3], v: &mut Vec3, f: &mut Fields) -> bool {
    let mut set = false;
    for (i, (axis, x)) in names
        .into_iter()
        .zip([&mut v.x, &mut v.y, &mut v.z])
        .enumerate()
    {
        set |= field(ui, f, i, axis, x);
    }
    set
}

/// The value to edit: up to three decimals, as the original's editor shows it.
fn short_number_exact(n: f64) -> String {
    let s = format!("{:.3}", if n.abs() < 0.0005 { 0.0 } else { n });
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Up to two decimals without trailing zeros, and no "-0": 30 → "30", 29.98 → "29.98".
fn short_number(n: f64) -> String {
    let s = format!("{:.2}", if n.abs() < 0.005 { 0.0 } else { n });
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

#[cfg(test)]
mod tests {
    #[test]
    fn short_numbers() {
        assert_eq!(super::short_number(0.0), "0");
        assert_eq!(super::short_number(-0.001), "0");
        assert_eq!(super::short_number(29.98), "29.98");
        assert_eq!(super::short_number(40.0), "40");
        assert_eq!(super::short_number(-1.5), "-1.5");
    }

    #[test]
    fn typed_numbers() {
        assert_eq!(super::parse_number(" 12,5 "), Some(12.5));
        assert_eq!(super::parse_number("-3"), Some(-3.0));
        assert_eq!(super::parse_number("abc"), None);
        assert_eq!(super::parse_number("inf"), None);
    }
}
