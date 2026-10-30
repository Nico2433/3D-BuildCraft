//! The handles around the selection, fixed in 3D space like the original's, so they turn
//! with the view. Move: arrows on the faces along X, Y and Z. Rotate: a ring about each
//! axis with a handle on it. Scale: a box with corner dots for uniform size and double
//! arrows on the faces for one axis. Objects move only through these.

use super::TransformMode;
use crate::mesh::Aabb;
use eframe::egui::{self, Color32, Pos2, Stroke, Vec2};
use glam::Vec3;

pub const BLUE: Color32 = Color32::from_rgb(0x1E, 0xA0, 0xE8);
pub const GREEN: Color32 = Color32::from_rgb(0x6C, 0xE0, 0x3C);
/// How close, in points, the pointer must be to grab a handle.
pub const GRAB: f32 = 16.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    /// Move along an axis (0 X, 1 Y, 2 Z).
    Move(usize),
    /// Turn about an axis.
    Turn(usize),
    /// Resize along an axis from the face on this side (-1 or 1); the other face stays.
    Size(usize, f32),
    /// Resize evenly from a corner of the box.
    Uniform,
}

#[derive(Clone, Copy, Debug)]
pub struct Handle {
    pub kind: Kind,
    pub at: Pos2,
    /// Screen points per unit of drag: per mm (move, size), per degree (turn), or per
    /// whole size (uniform, from the box centre to this corner).
    pub per: Vec2,
    /// Turn handles: the ring's centre on screen, so a drag turns with the pointer going
    /// round it. None when the ring is seen almost edge on; then it slides along `per`.
    pub pivot: Option<Pos2>,
}

impl Kind {
    /// The bottom bar field this handle works on: X, Y or Z.
    pub fn field(self) -> Option<usize> {
        match self {
            Kind::Move(a) | Kind::Turn(a) | Kind::Size(a, _) => Some(a),
            Kind::Uniform => None,
        }
    }
}

impl Handle {
    /// How far a drag by `d` points goes, in the handle's unit.
    pub fn amount(&self, d: Vec2) -> f32 {
        d.dot(self.per) / self.per.length_sq().max(1e-6)
    }

    /// Degrees turned while the pointer goes from `a` to `b` round the pivot, positive the
    /// way `per` points; None for handles without a pivot.
    pub fn turned(&self, a: Pos2, b: Pos2) -> Option<f32> {
        let c = self.pivot?;
        let (u, v) = (a - c, b - c);
        let sign = cross(self.at - c, self.per).signum();
        Some(sign * cross(u, v).atan2(u.dot(v)).to_degrees())
    }
}

fn cross(a: Vec2, b: Vec2) -> f32 {
    a.x * b.y - a.y * b.x
}

pub struct Gizmo {
    pub handles: Vec<Handle>,
    /// Each ring's points and the axis it turns about.
    rings: Vec<(Vec<Pos2>, usize)>,
    frame: Option<egui::Rect>,
}

/// The handles for `mode` around the box `b`, seen from `eye`.
pub fn build(
    mode: TransformMode,
    b: &Aabb,
    eye: Vec3,
    angles: [f32; 3],
    to_screen: &dyn Fn(Vec3) -> Option<Pos2>,
) -> Gizmo {
    let mut g = Gizmo {
        handles: Vec::new(),
        rings: Vec::new(),
        frame: None,
    };
    if !b.is_valid() {
        return g;
    }
    let c = b.center();
    let axes = [Vec3::X, Vec3::Y, Vec3::Z];
    // The face centre on `side` of `axis`.
    let face = |axis: usize, side: f32| {
        let mut p = c;
        p[axis] = if side > 0.0 { b.max[axis] } else { b.min[axis] };
        p
    };
    // The Y face towards the viewer.
    let front = if eye.y < c.y { -1.0 } else { 1.0 };
    let mut add = |kind: Kind, p: Vec3, along: Vec3, unit: f32| {
        if let (Some(at), Some(q)) = (to_screen(p), to_screen(p + along * unit)) {
            g.handles.push(Handle {
                kind,
                at,
                per: (q - at) / unit,
                pivot: None,
            });
        }
    };
    match mode {
        TransformMode::Move => {
            add(Kind::Move(0), face(0, 1.0), Vec3::X, 1.0);
            add(Kind::Move(1), face(1, front), Vec3::Y, 1.0);
            add(Kind::Move(2), face(2, 1.0), Vec3::Z, 1.0);
        }
        TransformMode::Scale => {
            for (axis, side) in [(0, -1.0), (0, 1.0), (1, front), (2, -1.0), (2, 1.0)] {
                add(Kind::Size(axis, side), face(axis, side), axes[axis], 1.0);
            }
        }
        TransformMode::Rotate => {
            let r = b.size().length() * 0.5 * 1.05;
            // Ring about each axis, its handle's point and which way positive turns go.
            let rings = [
                (0, -Vec3::Y, -Vec3::Z),
                (1, Vec3::Z, Vec3::X),
                (2, Vec3::X, Vec3::Y),
            ];
            for (axis, at, tangent) in rings {
                let (u, v) = (at, tangent);
                let pts: Vec<Pos2> = (0..=72)
                    .filter_map(|i| {
                        let t = i as f32 / 72.0 * std::f32::consts::TAU;
                        to_screen(c + (u * t.cos() + v * t.sin()) * r)
                    })
                    .collect();
                g.rings.push((pts, axis));
                let q = glam::Quat::from_axis_angle(axes[axis], angles[axis].to_radians());
                let radial = q * at;
                add(
                    Kind::Turn(axis),
                    c + radial * r,
                    axes[axis].cross(radial) * r * 1f32.to_radians(),
                    1.0,
                );
            }
            // Turn round the ring's centre unless the ring is nearly a line on screen.
            if let Some(centre) = to_screen(c) {
                for h in &mut g.handles {
                    let (radius, tangent) = (h.at - centre, h.per.normalized());
                    if cross(radius.normalized(), tangent).abs() > 0.3 {
                        h.pivot = Some(centre);
                    }
                }
            }
        }
    }
    if mode == TransformMode::Scale {
        let corners: Vec<Pos2> = (0..8)
            .filter_map(|i| {
                to_screen(Vec3::new(
                    if i & 1 != 0 { b.max.x } else { b.min.x },
                    if i & 2 != 0 { b.max.y } else { b.min.y },
                    if i & 4 != 0 { b.max.z } else { b.min.z },
                ))
            })
            .collect();
        if corners.len() == 8 {
            let frame = egui::Rect::from_points(&corners);
            for p in [
                frame.left_top(),
                frame.right_top(),
                frame.left_bottom(),
                frame.right_bottom(),
            ] {
                g.handles.push(Handle {
                    kind: Kind::Uniform,
                    at: p,
                    per: p - frame.center(),
                    pivot: None,
                });
            }
            g.frame = Some(frame);
        }
    }
    g
}

impl Gizmo {
    /// The handle under `p`, nearest first.
    pub fn grab(&self, p: Pos2) -> Option<Handle> {
        self.handles
            .iter()
            .map(|h| (h, h.at.distance(p)))
            .filter(|(_, d)| *d < GRAB)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(h, _)| *h)
    }

    /// Draws the handles: green the `hot` one (dragged or under the pointer) and those for
    /// the bottom bar's current `field`, blue the rest.
    pub fn paint(&self, painter: &egui::Painter, hot: Option<Kind>, field: usize) {
        let color = |k: Kind| {
            if hot == Some(k) || k.field() == Some(field) {
                GREEN
            } else {
                BLUE
            }
        };
        for (pts, axis) in &self.rings {
            let c = color(Kind::Turn(*axis));
            painter.add(egui::Shape::line(pts.clone(), Stroke::new(1.0_f32, c)));
        }
        if let Some(frame) = self.frame {
            painter.rect_stroke(
                frame,
                egui::CornerRadius::ZERO,
                Stroke::new(1.0_f32, Color32::WHITE),
                egui::StrokeKind::Middle,
            );
        }
        // Arrows shrink with their axis's foreshortening, as in the original.
        let longest = self
            .handles
            .iter()
            .filter(|h| matches!(h.kind, Kind::Move(_) | Kind::Size(..)))
            .map(|h| h.per.length())
            .fold(1e-6, f32::max);
        for h in &self.handles {
            match h.kind {
                Kind::Move(_) => {
                    let len = 16.0 * (h.per.length() / longest).max(0.35);
                    arrow(painter, h.at, h.per.normalized() * len, color(h.kind));
                }
                Kind::Turn(_) => {
                    arrow(painter, h.at, h.per.normalized() * 14.0, color(h.kind));
                }
                Kind::Size(_, side) => {
                    let len = (h.per.length() / longest).max(0.35);
                    chevrons(
                        painter,
                        h.at,
                        h.per.normalized() * side * len,
                        color(h.kind),
                    );
                }
                Kind::Uniform => {
                    painter.circle_filled(h.at, 4.0, Color32::WHITE);
                }
            }
        }
    }
}

/// A double arrow through `at`, `half` long each way.
fn arrow(painter: &egui::Painter, at: Pos2, half: Vec2, color: Color32) {
    let (a, b) = (at - half, at + half);
    painter.line_segment([a, b], Stroke::new(3.0_f32, color));
    let d = half.normalized();
    let n = d.rot90();
    let heads = [(a, -d), (b, d)];
    for (tip, dir) in heads {
        painter.add(egui::Shape::convex_polygon(
            vec![
                tip + dir * 3.0,
                tip - dir * 5.0 + n * 6.0,
                tip - dir * 5.0 - n * 6.0,
            ],
            color,
            Stroke::NONE,
        ));
    }
}

/// Two arrow heads one after the other, pointing along `out`; `out`'s length (0..1) sizes them.
fn chevrons(painter: &egui::Painter, at: Pos2, out: Vec2, color: Color32) {
    let k = out.length().clamp(0.35, 1.0);
    let d = out.normalized();
    let n = d.rot90();
    for step in [-0.5, 0.5] {
        let tip = at + d * (step * 7.0 * k + 4.0 * k);
        painter.add(egui::Shape::convex_polygon(
            vec![
                tip,
                tip - d * 6.0 * k + n * 6.0 * k,
                tip - d * 6.0 * k - n * 6.0 * k,
            ],
            color,
            Stroke::NONE,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A simple oblique projection, enough to check where handles go.
    fn screen(p: Vec3) -> Option<Pos2> {
        Some(Pos2::new(p.x + 0.3 * p.y, -p.z + 0.3 * p.y))
    }

    fn cube() -> Aabb {
        Aabb {
            min: Vec3::new(-10.0, -10.0, 0.0),
            max: Vec3::new(10.0, 10.0, 20.0),
        }
    }

    #[test]
    fn move_has_an_arrow_per_axis_on_the_faces() {
        let g = build(
            TransformMode::Move,
            &cube(),
            Vec3::new(0.0, -100.0, 50.0),
            [0.0; 3],
            &screen,
        );
        let kinds: Vec<Kind> = g.handles.iter().map(|h| h.kind).collect();
        assert_eq!(kinds, [Kind::Move(0), Kind::Move(1), Kind::Move(2)]);
        // Z: top face centre, one point up per mm.
        let z = g.handles[2];
        assert_eq!(z.at, Pos2::new(0.0, -20.0));
        assert!((z.amount(Vec2::new(0.0, -5.0)) - 5.0).abs() < 1e-4);
        // Y on the face towards the eye.
        assert_eq!(
            g.handles[1].at,
            screen(Vec3::new(0.0, -10.0, 10.0)).unwrap()
        );
    }

    #[test]
    fn rotate_turns_by_degrees_along_the_ring() {
        let g = build(
            TransformMode::Rotate,
            &cube(),
            Vec3::new(0.0, -100.0, 50.0),
            [0.0; 3],
            &screen,
        );
        assert_eq!(g.handles.len(), 3);
        let yaw = g.handles.iter().find(|h| h.kind == Kind::Turn(2)).unwrap();
        let radius = cube().size().length() * 0.5 * 1.05;
        let tangent = screen(cube().center() + Vec3::Y * radius * 1f32.to_radians()).unwrap()
            - screen(cube().center()).unwrap();
        assert!((yaw.per - tangent).length() < 1e-4);
        let ten = yaw.per * 10.0;
        assert!((yaw.amount(ten) - 10.0).abs() < 1e-3);
    }

    #[test]
    fn persisted_rotation_arrow_can_be_grabbed_on_its_orbit() {
        let b = cube();
        let g = build(
            TransformMode::Rotate,
            &b,
            Vec3::new(0.0, -100.0, 50.0),
            [0.0; 3],
            &screen,
        );
        let initial = g.handles[2].at;
        let g = build(
            TransformMode::Rotate,
            &b,
            Vec3::new(0.0, -100.0, 50.0),
            [0.0, 0.0, 90.0],
            &screen,
        );
        let at = g.handles[2].at;
        let radius = b.size().length() * 0.5 * 1.05;
        assert!(at.distance(screen(b.center() + Vec3::Y * radius).unwrap()) < 1e-4);
        assert!(at.distance(initial) > 1.0);
        assert_eq!(g.grab(at).unwrap().kind, Kind::Turn(2));
    }

    #[test]
    fn scale_has_faces_and_corners() {
        let g = build(
            TransformMode::Scale,
            &cube(),
            Vec3::new(0.0, -100.0, 50.0),
            [0.0; 3],
            &screen,
        );
        assert_eq!(
            g.handles.iter().filter(|h| h.kind == Kind::Uniform).count(),
            4
        );
        assert!(g.handles.iter().any(|h| h.kind == Kind::Size(1, -1.0)));
        let corner = g.handles.iter().find(|h| h.kind == Kind::Uniform).unwrap();
        // Dragging a corner out by its own offset doubles the size.
        assert!((corner.amount(corner.per) - 1.0).abs() < 1e-4);
        assert!(g.grab(corner.at + Vec2::new(3.0, 0.0)).is_some());
        assert!(g.grab(Pos2::new(500.0, 500.0)).is_none());
    }

    #[test]
    fn turn_follows_the_pointer_round_the_pivot() {
        let h = Handle {
            kind: Kind::Turn(2),
            at: Pos2::new(10.0, 0.0),
            per: Vec2::new(0.0, 1.0),
            pivot: Some(Pos2::ZERO),
        };
        // A quarter turn the way `per` points, whichever way the pointer leaves the handle.
        let t = h.turned(h.at, Pos2::new(0.0, 10.0)).unwrap();
        assert!((t - 90.0).abs() < 1e-3);
        let back = h.turned(h.at, Pos2::new(0.0, -10.0)).unwrap();
        assert!((back + 90.0).abs() < 1e-3);
    }
}
