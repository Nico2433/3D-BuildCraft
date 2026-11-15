//! The Emboss tool in the view, drawn over the scene as the original does: the pattern's
//! outlines in white where they land, the card they are projected from in pale green
//! (with the projection cylinder for Cylindrical), and the handles for the bottom bar's
//! mode, the object handles themselves: two move arrows along the pattern's plane, a turn
//! ring round it with its arrow, or arrows at its right and top edges and a dot at its
//! corner to size.

use super::*;
use crate::emboss::{Anchor, Preview};
use gizmo::Kind;

/// What a drag in the view does to the pattern while Emboss is open.
#[derive(Clone, Copy, Debug)]
pub(in crate::app) enum EmbossDrag {
    /// Over the object: moves the pattern under the pointer (Shift turns it).
    Place,
    /// On the turn ring's arrow: turns the pattern as an object's ring turns it, the
    /// pointer going round the ring's centre. The grabbed handle, the angle when the drag
    /// began, the degrees turned since, and where the pointer was pressed and last seen.
    Turn {
        handle: gizmo::Handle,
        start: f32,
        turned: f32,
        from: Pos2,
        last: Pos2,
    },
    /// On a move arrow: slides the pattern along its plane's right (0, the offset's X) or
    /// up (1, its Y) only.
    Slide(usize),
    /// On a size handle: sizes the pattern's width (0) or height (1) alone at the arrows
    /// on its edges (both with the lock), or both at the corner (`None`). The pointer's
    /// last distance from the centre, along that axis or straight, in mm.
    Size(Option<usize>, f32),
}

impl EmbossDrag {
    /// The handle kind it drags, for its colour and the bar field it works on.
    fn kind(self) -> Option<Kind> {
        match self {
            EmbossDrag::Place => None,
            EmbossDrag::Turn { .. } => Some(Kind::Turn(0)),
            EmbossDrag::Slide(axis) => Some(Kind::Move(axis)),
            EmbossDrag::Size(Some(axis), _) => Some(Kind::Size(axis, 1.0)),
            EmbossDrag::Size(None, _) => Some(Kind::Uniform),
        }
    }
}

/// Space, in points, between the pattern and its move and size handles.
const HANDLE_GAP: f32 = 18.0;

impl App {
    fn emboss_preview(&self) -> Option<&Preview> {
        match &self.tool {
            Some(Tool::Emboss { preview: Ok(p), .. }) => Some(p),
            _ => None,
        }
    }

    fn emboss_params(&self) -> Option<&crate::emboss::Emboss> {
        match &self.tool {
            Some(Tool::Emboss { params, .. }) => Some(params),
            _ => None,
        }
    }

    /// The turn ring round the pattern in its plane, as an object's: its arrow rests on
    /// the pattern's left, turned with it, and while a turn drag is under way the ring
    /// shows where it began and the wedge swept since.
    fn emboss_ring(&self, rect: egui::Rect, aspect: f32) -> Option<gizmo::Gizmo> {
        let a = self.emboss_preview()?.anchor;
        let (angle, turned) = match self.emboss_drag {
            Some(EmbossDrag::Turn { start, turned, .. }) => {
                (start, self.rotation_drag_angle(turned))
            }
            _ => (self.emboss_params()?.angle, 0.0),
        };
        let ring = gizmo::Ring {
            axis: 0,
            centre: a.center,
            // A turn goes from right towards up, so from the left towards down.
            rest: -a.right,
            tangent: -a.up,
            radius: a.half.length() * 1.15,
            angle,
            turned,
        };
        let to_screen = |p: Vec3| self.to_screen(p, rect, aspect);
        Some(gizmo::ring(&ring, &to_screen))
    }

    /// Millimetres per screen point at `p`, across the view.
    fn mm_per_point(&self, p: Vec3, rect: egui::Rect, aspect: f32) -> f32 {
        let eye = self.camera.eye();
        let across = (p - eye).cross(Vec3::Z).try_normalize().unwrap_or(Vec3::X);
        match (
            self.to_screen(p, rect, aspect),
            self.to_screen(p + across, rect, aspect),
        ) {
            (Some(a), Some(b)) => 1.0 / a.distance(b).max(1e-6),
            _ => 0.0,
        }
    }

    /// The handles of the current mode, in screen points: the turn ring in Rotate. Move has an arrow past the pattern
    /// along its plane's right and up (not turned with it, as the offset is not); Size an
    /// arrow past its right and top edges and a dot past its corner (turned with it).
    fn emboss_gizmo(&self, rect: egui::Rect, aspect: f32) -> Option<gizmo::Gizmo> {
        let a = self.emboss_preview()?.anchor;
        let gap = HANDLE_GAP * self.mm_per_point(a.center, rect, aspect);
        let list = match self.mode {
            TransformMode::Move => {
                // How far the turned pattern reaches along the plane's right and up.
                let (sin, cos) = a.angle.sin_cos();
                let (sin, cos) = (sin.abs(), cos.abs());
                let reach = glam::Vec2::new(
                    cos * a.half.x + sin * a.half.y,
                    sin * a.half.x + cos * a.half.y,
                );
                vec![
                    (Kind::Move(0), a.center + a.right * (reach.x + gap), a.right),
                    (Kind::Move(1), a.center + a.up * (reach.y + gap), a.up),
                ]
            }
            TransformMode::Scale => {
                let right = a.at(glam::Vec2::X) - a.center;
                let up = a.at(glam::Vec2::Y) - a.center;
                let edge = |p: glam::Vec2, out: Vec3| a.at(p) + out * gap;
                vec![
                    (
                        Kind::Size(0, 1.0),
                        edge(glam::Vec2::new(a.half.x, 0.0), right),
                        right,
                    ),
                    (
                        Kind::Size(1, 1.0),
                        edge(glam::Vec2::new(0.0, a.half.y), up),
                        up,
                    ),
                    (
                        Kind::Uniform,
                        edge(a.half, (right + up) * std::f32::consts::FRAC_1_SQRT_2),
                        right + up,
                    ),
                ]
            }
            TransformMode::Rotate => return self.emboss_ring(rect, aspect),
        };
        let to_screen = |p: Vec3| self.to_screen(p, rect, aspect);
        Some(gizmo::handles(&list, &to_screen))
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

    /// How far from the centre a size drag measures at plane point `l`: along the turned
    /// pattern's width or height, or straight for the corner.
    fn size_reach(a: &Anchor, axis: Option<usize>, l: glam::Vec2) -> f32 {
        let own = glam::Vec2::from_angle(-a.angle).rotate(l);
        axis.map_or(l.length(), |axis| own[axis].abs())
    }

    /// The drag a press at `pos` starts: a handle of the current mode under it (whose bar
    /// field becomes the current one, as for objects), else placing the pattern when it is
    /// over the selection.
    pub(super) fn emboss_drag_start(
        &mut self,
        pos: Pos2,
        rect: egui::Rect,
        aspect: f32,
    ) -> Option<EmbossDrag> {
        if let Some(drag) = self.emboss_handle_at(pos, rect, aspect) {
            if let Some(f) = drag.kind().and_then(Kind::field) {
                self.field = f;
            }
            return Some(drag);
        }
        self.selection_under(pos, rect, aspect)
            .then_some(EmbossDrag::Place)
    }

    /// The drag the current mode's handle under `pos` starts, if there is one there.
    fn emboss_handle_at(&self, pos: Pos2, rect: egui::Rect, aspect: f32) -> Option<EmbossDrag> {
        let a = self.emboss_preview()?.anchor;
        let local = self.emboss_plane_point(&a, pos, rect, aspect);
        let handle = self.emboss_gizmo(rect, aspect)?.grab(pos)?;
        match handle.kind {
            Kind::Move(axis) => Some(EmbossDrag::Slide(axis)),
            Kind::Size(axis, _) => {
                let l = local?;
                Some(EmbossDrag::Size(
                    Some(axis),
                    Self::size_reach(&a, Some(axis), l),
                ))
            }
            Kind::Uniform => Some(EmbossDrag::Size(None, local?.length())),
            Kind::Turn(_) => Some(EmbossDrag::Turn {
                handle,
                start: self.emboss_params()?.angle,
                turned: 0.0,
                from: pos,
                last: pos,
            }),
        }
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
        if matches!(drag, EmbossDrag::Place) {
            return self.emboss_place(ui, resp, rect, aspect);
        }
        let Some(a) = self.emboss_preview().map(|p| p.anchor) else {
            return;
        };
        // A slide goes as far along its arrow as the pointer did, in mm, as objects move.
        let slid = match drag {
            EmbossDrag::Slide(axis) => self
                .emboss_gizmo(rect, aspect)
                .and_then(|g| g.handles.into_iter().find(|h| h.kind == Kind::Move(axis)))
                .map_or(0.0, |h| h.amount(resp.drag_delta())),
            _ => 0.0,
        };
        // A turn adds up frame by frame, as an object's, so a drag can go round; it snaps
        // as theirs does.
        let turn = match drag {
            EmbossDrag::Turn {
                handle,
                turned,
                from,
                last,
                ..
            } => {
                let turned = handle
                    .turned(last, pos)
                    .map_or_else(|| handle.amount(pos - from), |d| turned + d);
                (turned, self.rotation_drag_angle(turned))
            }
            _ => (0.0, 0.0),
        };
        let Some(l) = self.emboss_plane_point(&a, pos, rect, aspect) else {
            return;
        };
        let Some(Tool::Emboss { params, lock, .. }) = &mut self.tool else {
            return;
        };
        self.emboss_drag = Some(match drag {
            EmbossDrag::Turn {
                handle,
                start,
                from,
                ..
            } => {
                let (turned, snapped) = turn;
                params.angle = (start + snapped + 180.0).rem_euclid(360.0) - 180.0;
                EmbossDrag::Turn {
                    handle,
                    start,
                    turned,
                    from,
                    last: pos,
                }
            }
            EmbossDrag::Slide(axis) => {
                params.offset[axis] += slid;
                drag
            }
            EmbossDrag::Size(axis, last) => {
                let now = Self::size_reach(&a, axis, l);
                if last > 1e-3 {
                    // The corner keeps the proportion whatever the lock says.
                    let (along, lock) = axis.map_or((1, true), |axis| (axis, *lock));
                    crate::emboss::rescale(params, along, now / last, lock);
                }
                EmbossDrag::Size(axis, now)
            }
            EmbossDrag::Place => drag,
        });
    }

    /// The preview's lines and the current mode's handles, over the 3D view.
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
        // Green while dragged or under the pointer, as the object handles, and the pointer
        // a hand over them and over the pattern's object, closed while dragging.
        let dragged = self.emboss_drag.and_then(EmbossDrag::kind);
        let hovered = || {
            hover
                .and_then(|h| self.emboss_handle_at(h, rect, aspect))
                .and_then(EmbossDrag::kind)
        };
        let hot = dragged.or_else(hovered);
        let over = || hover.is_some_and(|h| self.selection_under(h, rect, aspect));
        icons::grab_cursor(
            painter.ctx(),
            self.emboss_drag.is_some(),
            hot.is_some() || over(),
        );
        if let Some(g) = self.emboss_gizmo(rect, aspect) {
            g.paint(painter, hot, self.field);
        }
    }
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
