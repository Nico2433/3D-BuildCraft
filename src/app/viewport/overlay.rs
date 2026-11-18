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
            measure_overlay(
                &painter,
                m,
                &to_screen,
                ui.ctx().pixels_per_point(),
                self.settings.lang(),
            );
        }
    }
}

/// The measure marks: arrows on the placed points (blue the first, green the others) and
/// on the live one, the faint line between two of them, the fitted circle or sphere with
/// its radius line, and the snap indicator with its name beside the live point.
fn measure_overlay(
    painter: &egui::Painter,
    m: &Measure,
    to_screen: &dyn Fn(Vec3) -> Option<Pos2>,
    ppp: f32,
    lang: Lang,
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
            let (u, v) = (
                axis.any_orthonormal_vector(),
                axis.cross(axis.any_orthonormal_vector()),
            );
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
        // Radius line to the rim (the whole diameter for a circular edge, to the loop's
        // first vertex), the centre as a dot with a small cross.
        let edge = m.edge.as_ref().or(m.edge_hover.as_ref());
        let rim = match (m.mode, edge.and_then(|l| l.points.first())) {
            (Mode::Edge, Some(p)) => (*p - r.center).normalize_or_zero(),
            _ => r.axis.map_or(Vec3::X, |a| a.any_orthonormal_vector()),
        };
        let start = if m.mode == Mode::Edge {
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
    // The edge loop under the cursor (yellow, like the snap indicator) and the chosen one.
    let loop_line = |l: &crate::edge_loops::EdgeLoop, color: Color32| {
        let mut line: Vec<Pos2> = l.points.iter().filter_map(|p| to_screen(*p)).collect();
        if l.closed {
            line.extend(line.first().copied());
        }
        for w in line.windows(2) {
            faint(w[0], w[1]);
        }
        painter.add(egui::Shape::line(line, Stroke::new(2.5 / ppp, color)));
    };
    if m.mode == Mode::Edge {
        let chosen = m.edge.as_ref().map(|l| l.chain);
        if let Some(l) = m.edge_hover.as_ref().filter(|l| Some(l.chain) != chosen) {
            loop_line(l, SNAP_YELLOW);
        }
        if let Some(l) = &m.edge {
            loop_line(l, MEASURE_GREEN);
        }
    }
    for (p, color) in marks {
        if let Some(tip) = to_screen(p) {
            measure_arrow(painter, tip, color, ppp);
        }
    }
    if let Some((at, kind)) = live.and_then(|s| to_screen(s.point).zip(s.kind)) {
        snap_indicator(painter, at, kind, ppp, lang);
    }
}

/// The snap indicator: a square on a vertex, a triangle on an edge middle, a diamond on an
/// edge, a circle on a face centre, a cross on a plane; the kind's name is beside it.
fn snap_indicator(painter: &egui::Painter, at: Pos2, kind: SnapKind, ppp: f32, lang: Lang) {
    let u = 1.0 / ppp;
    let stroke = Stroke::new(2.0 * u, SNAP_YELLOW);
    let r = 7.0 * u;
    let points = |offsets: &[(f32, f32)]| -> Vec<Pos2> {
        offsets
            .iter()
            .map(|(x, y)| Pos2::new(at.x + x * r, at.y + y * r))
            .collect()
    };
    match kind {
        SnapKind::Vertex => {
            painter.rect_stroke(
                egui::Rect::from_center_size(at, egui::vec2(2.0 * r, 2.0 * r)),
                0.0,
                stroke,
                egui::StrokeKind::Middle,
            );
        }
        SnapKind::Midpoint => {
            painter.add(egui::Shape::closed_line(
                points(&[(0.0, -1.0), (1.0, 0.8), (-1.0, 0.8)]),
                stroke,
            ));
        }
        SnapKind::Edge => {
            painter.add(egui::Shape::closed_line(
                points(&[(0.0, -1.0), (1.0, 0.0), (0.0, 1.0), (-1.0, 0.0)]),
                stroke,
            ));
        }
        SnapKind::Face => {
            painter.circle_stroke(at, r, stroke);
            painter.circle_filled(at, 2.0 * u, SNAP_YELLOW);
        }
        SnapKind::Plane => {
            painter.line_segment(
                [points(&[(-1.0, 0.0)])[0], points(&[(1.0, 0.0)])[0]],
                stroke,
            );
            painter.line_segment(
                [points(&[(0.0, -1.0)])[0], points(&[(0.0, 1.0)])[0]],
                stroke,
            );
        }
    }
    painter.text(
        Pos2::new(at.x + 12.0 * u, at.y + 8.0 * u),
        egui::Align2::LEFT_TOP,
        tr(lang, kind.name()),
        egui::FontId::proportional(13.0 * u),
        SNAP_YELLOW,
    );
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
