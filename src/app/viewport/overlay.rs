//! Viewport 2D overlays: grid labels, selection box, handles, measure marks, webcam feed.

use super::*;

impl App {
    /// 2D overlays drawn over the 3D view.
    pub(super) fn overlays(
        &self,
        ui: &egui::Ui,
        resp: &egui::Response,
        rect: egui::Rect,
        aspect: f32,
        snap_guides: Vec<(Vec3, Vec3)>,
    ) {
        let r = self.settings.render;
        let to_screen = |p: Vec3| self.to_screen(p, rect, aspect);
        let painter = ui.painter_at(rect);
        for (a, b) in snap_guides {
            if let (Some(a), Some(b)) = (to_screen(a), to_screen(b)) {
                painter.line_segment(
                    [a, b],
                    Stroke::new(1.0_f32, Color32::from_rgb(90, 210, 245)),
                );
            }
        }
        if r.grid && r.grid_labels {
            // Distances along the X and Y axes, written on the floor and smaller
            // far away.
            for (axis, k) in [Vec3::X, Vec3::Y]
                .into_iter()
                .flat_map(|a| (1..=12).map(move |k| (a, k)))
            {
                let p = axis * (k as f32 * 50.0);
                let Some(s) = to_screen(p) else { continue };
                // Hidden behind objects, roughly: their boxes block the label.
                let eye = self.camera.eye();
                let dir = (p - eye).normalize();
                let far = eye.distance(p);
                let hidden = self.scene.objects.iter().any(|o| {
                    let b = o.world_aabb();
                    ray_aabb(eye, dir, &b) && b.center().distance(eye) < far
                });
                if hidden {
                    continue;
                }
                let size = 22.0 * 260.0 / self.camera.eye().distance(p);
                // Too far to read: they would only pile up at the horizon.
                if size < 9.0 {
                    continue;
                }
                let size = size.min(22.0);
                painter.text(
                    s,
                    Align2::CENTER_CENTER,
                    format!("{} mm", k * 50),
                    egui::FontId::proportional(size),
                    Color32::from_rgba_unmultiplied(0x2A, 0x2A, 0x2A, 190),
                );
            }
        }
        if let (Some(from), Some(to)) = (self.marquee, resp.interact_pointer_pos()) {
            let area = egui::Rect::from_two_pos(from, to);
            painter.rect_filled(
                area,
                0.0,
                Color32::from_rgba_unmultiplied(0x1E, 0xA0, 0xE8, 40),
            );
            painter.rect_stroke(
                area,
                0.0,
                Stroke::new(1.0_f32, Color32::from_rgb(0x1E, 0xA0, 0xE8)),
                egui::StrokeKind::Inside,
            );
        }
        if let Some(g) = self.gizmo(aspect, rect) {
            // The handle being dragged, or the one under the pointer, is green and
            // the pointer becomes a hand over it.
            let hot = self
                .handle
                .as_ref()
                .and_then(|h| h.handle.map(|h| h.kind))
                .or_else(|| resp.hover_pos().and_then(|p| g.grab(p)).map(|h| h.kind));
            // An open hand over a handle or the selection, a closed one dragging.
            let over_selection = || {
                resp.hover_pos()
                    .is_some_and(|p| self.selection_under(p, rect, aspect))
            };
            icons::grab_cursor(
                ui.ctx(),
                self.handle.is_some(),
                hot.is_some() || over_selection(),
            );
            g.paint(&painter, hot, self.field);
        }
        self.paint_emboss(&painter, resp.hover_pos(), rect, aspect);
        if let Some(Tool::Measure(m)) = &self.tool {
            measure_overlay(&painter, m, &to_screen, ui.ctx().pixels_per_point());
        }
    }
}

/// The measure marks: arrows on the placed points (blue the first, green the others) and on
/// the live one (the arrow is the tool's cursor), the faint line between two of them, and
/// the fitted circle or sphere with its radius line.
fn measure_overlay(
    painter: &egui::Painter,
    m: &Measure,
    to_screen: &dyn Fn(Vec3) -> Option<Pos2>,
    ppp: f32,
) {
    let faint = |a: Pos2, b: Pos2| {
        painter.line_segment(
            [a, b],
            Stroke::new(11.0 / ppp, Color32::from_white_alpha(13)),
        );
    };
    let live = if m.points.len() < m.capacity() {
        m.hover
    } else {
        None
    };
    let marks: Vec<(Vec3, Color32)> = m
        .points
        .iter()
        .enumerate()
        .map(|(i, p)| (p.point, if i == 0 { MEASURE_BLUE } else { MEASURE_GREEN }))
        .chain(live.map(|p| (p.point, MEASURE_GREEN)))
        .collect();
    if let (Mode::Distance, [(a, _), (b, _)]) = (m.mode, &marks[..]) {
        if let Some((a, b)) = to_screen(*a).zip(to_screen(*b)) {
            faint(a, b);
        }
    }
    if let Some(r) = m.current_round() {
        // Seen from any side: a circle about its axis, or three for a sphere.
        let axes = r
            .axis
            .map_or([Vec3::X, Vec3::Y, Vec3::Z].to_vec(), |a| vec![a]);
        for axis in axes {
            let (u, v) = crate::measure::plane_basis(axis);
            let ring: Vec<Pos2> = (0..=96)
                .filter_map(|i| {
                    let t = i as f32 / 96.0 * std::f32::consts::TAU;
                    to_screen(r.center + (u * t.cos() + v * t.sin()) * r.radius)
                })
                .collect();
            for w in ring.windows(2) {
                faint(w[0], w[1]);
            }
            painter.add(egui::Shape::line(
                ring,
                Stroke::new(1.5 / ppp, MEASURE_GREEN),
            ));
        }
        // Radius line to the rim (the whole diameter for a section, to
        // the outline's first vertex), the centre as a dot with a small cross.
        let outline = m.outline();
        let on_outline = outline.is_some();
        let rim = match outline.and_then(|l| l.points.first()) {
            Some(p) => (*p - r.center).normalize_or_zero(),
            None => r.axis.map_or(Vec3::X, |a| a.any_orthonormal_vector()),
        };
        let start = if on_outline {
            r.center - rim * r.radius
        } else {
            r.center
        };
        if let Some((c, e)) = to_screen(start).zip(to_screen(r.center + rim * r.radius)) {
            faint(c, e);
            painter.line_segment([c, e], Stroke::new(1.5 / ppp, MEASURE_GREEN));
        }
        if let Some(c) = to_screen(r.center) {
            let arm = 6.0 / ppp;
            let stroke = Stroke::new(1.5 / ppp, MEASURE_GREEN);
            painter.line_segment([c - egui::vec2(arm, 0.0), c + egui::vec2(arm, 0.0)], stroke);
            painter.line_segment([c - egui::vec2(0.0, arm), c + egui::vec2(0.0, arm)], stroke);
            painter.circle_filled(c, 3.0 / ppp, MEASURE_GREEN);
        }
    }
    // The section's contours in yellow, like the snap indicator; the one clicked in green.
    let loop_line = |l: &crate::section::Outline, color: Color32| {
        let mut line: Vec<Pos2> = l.points.iter().filter_map(|p| to_screen(*p)).collect();
        if l.closed {
            line.extend(line.first().copied());
        }
        for w in line.windows(2) {
            faint(w[0], w[1]);
        }
        painter.add(egui::Shape::line(line, Stroke::new(2.5 / ppp, color)));
    };
    if let Some(s) = m.section.as_ref().filter(|_| m.mode == Mode::Section) {
        for (i, c) in s.section.contours.iter().enumerate() {
            if Some(i) != s.chosen {
                loop_line(&c.outline, SNAP_YELLOW);
            }
        }
        if let Some(c) = s.chosen.and_then(|i| s.section.contours.get(i)) {
            loop_line(&c.outline, MEASURE_GREEN);
        }
    }
    for (p, color) in marks {
        if let Some(tip) = to_screen(p) {
            measure_arrow(painter, tip, color, ppp);
        }
    }
}

/// The webcam's live picture, fitted in the view, in place of the scene.
pub(super) fn camera_feed(
    ui: &egui::Ui,
    rect: egui::Rect,
    cam: &crate::webcam::Webcam,
    tex: &mut Option<(u64, egui::TextureHandle)>,
    lang: Lang,
) {
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::ZERO, Color32::BLACK);
    match cam.latest() {
        Some(Ok((n, frame))) => {
            if tex.as_ref().map(|t| t.0) != Some(n) {
                let size = [frame.width() as usize, frame.height() as usize];
                let image = egui::ColorImage::from_rgba_unmultiplied(size, frame.as_raw());
                match tex {
                    Some((shown, t)) => {
                        t.set(image, egui::TextureOptions::LINEAR);
                        *shown = n;
                    }
                    None => {
                        let t =
                            ui.ctx()
                                .load_texture("camera", image, egui::TextureOptions::LINEAR);
                        *tex = Some((n, t));
                    }
                }
            }
            if let Some((_, t)) = tex {
                let size = t.size_vec2();
                let k = (rect.width() / size.x).min(rect.height() / size.y);
                let fit = egui::Rect::from_center_size(rect.center(), size * k);
                let uv = egui::Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0));
                ui.painter().image(t.id(), fit, uv, Color32::WHITE);
            }
        }
        Some(Err(_)) => {}
        None => {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                tr(lang, "Starting the camera…"),
                egui::FontId::proportional(18.0),
                style::TEXT_DIM,
            );
        }
    }
    // New frames keep coming while the camera is open.
    ui.ctx().request_repaint_after(Duration::from_millis(33));
}

const MEASURE_GREEN: Color32 = Color32::from_rgb(0x50, 0xE8, 0x28);
const MEASURE_BLUE: Color32 = Color32::from_rgb(0x20, 0xA8, 0xF0);
const SNAP_YELLOW: Color32 = Color32::from_rgb(0xFF, 0xD8, 0x3C);

/// The measure marker: a thick downward arrow about 25 px tall whose tip is `tip`, with a
/// small red dot on the tip.
fn measure_arrow(painter: &egui::Painter, tip: Pos2, color: Color32, ppp: f32) {
    let u = 1.0 / ppp;
    let head = Pos2::new(tip.x, tip.y - 10.0 * u);
    painter.rect_filled(
        egui::Rect::from_min_max(
            Pos2::new(tip.x - 2.0 * u, tip.y - 25.0 * u),
            Pos2::new(tip.x + 2.0 * u, head.y),
        ),
        0.0,
        color,
    );
    painter.add(egui::Shape::convex_polygon(
        vec![
            tip,
            Pos2::new(tip.x - 5.0 * u, head.y),
            Pos2::new(tip.x + 5.0 * u, head.y),
        ],
        color,
        Stroke::NONE,
    ));
    painter.circle_filled(tip, 1.2 * u, Color32::from_rgb(0xE0, 0x20, 0x20));
}
