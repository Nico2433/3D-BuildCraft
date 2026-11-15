//! The Emboss tool in the view, drawn over the scene as the original does: the pattern's
//! outlines in white where they land, the card they are projected from in pale green
//! (with the projection cylinder for Cylindrical), and one handle for the bottom bar's
//! mode: a double arrow to move, a circle to turn, an arrow at the corner to size.

use super::*;
use crate::emboss::{Anchor, Preview};

/// What a drag in the view does to the pattern while Emboss is open.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::app) enum EmbossDrag {
    /// Over the object: moves the pattern under the pointer (Shift turns it).
    Place,
    /// On the circle: turns the pattern with the pointer. The pointer's last angle round
    /// the pattern's centre, in degrees.
    Turn(f32),
    /// On the corner arrow: sizes the pattern. The pointer's last distance from the
    /// centre, in mm.
    Size(f32),
}

/// Segments of the turn circle.
const CIRCLE: usize = 72;

impl App {
    fn emboss_preview(&self) -> Option<&Preview> {
        match &self.tool {
            Some(Tool::Emboss { preview: Ok(p), .. }) => Some(p),
            _ => None,
        }
    }

    /// The turn circle's radius: round the whole pattern, with a margin.
    fn turn_radius(a: &Anchor) -> f32 {
        a.half.length() * 1.15
    }

    /// The size arrow's place: just past the pattern's top right corner.
    fn size_corner(a: &Anchor) -> Vec3 {
        a.at(a.half * 1.1)
    }

    /// Where the pointer at `pos` meets the pattern's plane, in the plane's own
    /// coordinates (along right and up from the centre, not turned with the pattern, so
    /// turning it does not move the pointer's angle).
    fn emboss_plane_point(
        &self,
        a: &Anchor,
        pos: Pos2,
        rect: egui::Rect,
        aspect: f32,
    ) -> Option<glam::Vec2> {
        let (o, d) = self.camera.ray(to_ndc(rect, pos), aspect);
        let n = a.right.cross(a.up);
        let along = d.dot(n);
        if along.abs() < 1e-4 {
            return None;
        }
        let p = o + d * ((a.center - o).dot(n) / along) - a.center;
        Some(glam::Vec2::new(p.dot(a.right), p.dot(a.up)))
    }

    /// The drag a press at `pos` starts: a handle of the current mode under it, else
    /// placing the pattern when it is over the selection.
    pub(super) fn emboss_drag_start(
        &self,
        pos: Pos2,
        rect: egui::Rect,
        aspect: f32,
    ) -> Option<EmbossDrag> {
        self.emboss_handle_at(pos, rect, aspect).or_else(|| {
            let (o, d) = self.camera.ray(to_ndc(rect, pos), aspect);
            self.pick(o, d)
                .is_some_and(|(id, _)| self.scene.selection.contains(&id))
                .then_some(EmbossDrag::Place)
        })
    }

    /// The drag the current mode's handle under `pos` starts, if there is one there.
    fn emboss_handle_at(&self, pos: Pos2, rect: egui::Rect, aspect: f32) -> Option<EmbossDrag> {
        let to_screen = |p: Vec3| self.to_screen(p, rect, aspect);
        let a = self.emboss_preview()?.anchor;
        let local = self.emboss_plane_point(&a, pos, rect, aspect);
        match self.mode {
            TransformMode::Rotate => {
                let near = circle(&a)
                    .windows(2)
                    .filter_map(|w| Some((to_screen(w[0])?, to_screen(w[1])?)))
                    .any(|(p, q)| segment_distance(pos, p, q) < gizmo::GRAB * 0.5);
                if let (true, Some(l)) = (near, local) {
                    return Some(EmbossDrag::Turn(l.y.atan2(l.x).to_degrees()));
                }
            }
            TransformMode::Scale => {
                let near =
                    to_screen(Self::size_corner(&a)).is_some_and(|c| c.distance(pos) < gizmo::GRAB);
                if let (true, Some(l)) = (near, local) {
                    return Some(EmbossDrag::Size(l.length()));
                }
            }
            TransformMode::Move => {}
        }
        None
    }

    /// Follows the pointer for the drag under way.
    pub(super) fn emboss_drag_update(
        &mut self,
        ui: &egui::Ui,
        resp: &egui::Response,
        rect: egui::Rect,
        aspect: f32,
    ) {
        let (Some(drag), Some(pos)) = (self.emboss_drag, resp.interact_pointer_pos()) else {
            return;
        };
        if drag == EmbossDrag::Place {
            return self.emboss_place(ui, resp, rect, aspect);
        }
        let Some(a) = self.emboss_preview().map(|p| p.anchor) else {
            return;
        };
        let Some(l) = self.emboss_plane_point(&a, pos, rect, aspect) else {
            return;
        };
        let Some(Tool::Emboss { params, .. }) = &mut self.tool else {
            return;
        };
        self.emboss_drag = Some(match drag {
            EmbossDrag::Turn(last) => {
                let now = l.y.atan2(l.x).to_degrees();
                params.angle = (params.angle + now - last + 180.0).rem_euclid(360.0) - 180.0;
                EmbossDrag::Turn(now)
            }
            EmbossDrag::Size(last) => {
                let now = l.length();
                if last > 1e-3 {
                    params.size = (params.size * now / last).clamp(1.0, 200.0);
                }
                EmbossDrag::Size(now)
            }
            EmbossDrag::Place => drag,
        });
    }

    /// The preview's lines and the current mode's handle, over the 3D view.
    pub(super) fn paint_emboss(
        &self,
        painter: &egui::Painter,
        hover: Option<Pos2>,
        rect: egui::Rect,
        aspect: f32,
    ) {
        let Some(p) = self.emboss_preview() else {
            return;
        };
        let to_screen = |q: Vec3| self.to_screen(q, rect, aspect);
        let lines = |lines: &[Vec<Vec3>], stroke: Stroke| {
            for line in lines {
                polyline(painter, line, stroke, &to_screen);
            }
        };
        lines(&p.guide, Stroke::new(1.0_f32, style::EMBOSS_GUIDE));
        polyline(
            painter,
            &p.border,
            Stroke::new(1.0_f32, style::EMBOSS_BORDER),
            &to_screen,
        );
        lines(&p.card, Stroke::new(1.0_f32, style::EMBOSS_CARD));
        lines(&p.surface, Stroke::new(2.0_f32, Color32::WHITE));
        let a = p.anchor;
        // Green while dragged or under the pointer, as the object handles.
        let hot = self.emboss_drag.is_some_and(|d| d != EmbossDrag::Place)
            || hover.is_some_and(|h| self.emboss_handle_at(h, rect, aspect).is_some());
        let color = if hot { gizmo::GREEN } else { gizmo::BLUE };
        match self.mode {
            TransformMode::Move => {
                if let (Some(c), Some(r)) = (to_screen(a.center), to_screen(a.at(glam::Vec2::X))) {
                    let dir = (r - c).normalized();
                    if dir.is_finite() {
                        gizmo::arrow(painter, c, dir * 14.0, gizmo::GREEN);
                    }
                }
            }
            TransformMode::Rotate => {
                let color = if hot {
                    gizmo::GREEN
                } else {
                    style::EMBOSS_CARD
                };
                polyline(
                    painter,
                    &circle(&a),
                    Stroke::new(1.0_f32, color),
                    &to_screen,
                );
            }
            TransformMode::Scale => {
                let corner = Self::size_corner(&a);
                if let (Some(c), Some(k)) = (to_screen(a.center), to_screen(corner)) {
                    let out = (k - c).normalized();
                    if out.is_finite() {
                        gizmo::chevrons(painter, k, out, color);
                    }
                }
            }
        }
    }
}

/// The turn circle round the pattern, in its plane.
fn circle(a: &Anchor) -> Vec<Vec3> {
    let r = App::turn_radius(a);
    (0..=CIRCLE)
        .map(|k| {
            let t = std::f32::consts::TAU * k as f32 / CIRCLE as f32;
            a.at(glam::Vec2::new(t.cos(), t.sin()) * r)
        })
        .collect()
}

/// A world polyline on screen, broken where it goes behind the camera.
fn polyline(
    painter: &egui::Painter,
    line: &[Vec3],
    stroke: Stroke,
    to_screen: &dyn Fn(Vec3) -> Option<Pos2>,
) {
    let mut run: Vec<Pos2> = Vec::new();
    for p in line.iter().map(|&p| to_screen(p)).chain([None]) {
        match p {
            Some(p) => run.push(p),
            None => {
                if run.len() > 1 {
                    painter.add(egui::Shape::line(std::mem::take(&mut run), stroke));
                }
                run.clear();
            }
        }
    }
}

/// How far `p` is from the segment `a`–`b`, on screen.
fn segment_distance(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}
