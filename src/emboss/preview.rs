//! What the Emboss tool shows before it is applied, as lines in world space: the pattern's
//! outlines where they land on the object, the flat "card" they are projected from,
//! floating off the surface, and for Cylindrical a ring showing the projection cylinder.
//! It uses the same layout and rays as [`super::apply`], so what is shown is what is made.

use super::{layout, pieces, Emboss, Frame, Projection, Surface, MISSES};
use crate::mesh::Mesh;
use anyhow::{bail, Result};
use glam::{Vec2, Vec3};
use std::f32::consts::{PI, TAU};

/// The preview's lines (each a polyline; closed ones repeat their first point) and where
/// its handles go.
#[derive(Clone, Debug, Default)]
pub struct Preview {
    /// The outlines on the object's surface, split where their rays miss it.
    pub surface: Vec<Vec<Vec3>>,
    /// The outlines on the card the pattern is projected from.
    pub card: Vec<Vec<Vec3>>,
    /// The card's border.
    pub border: Vec<Vec3>,
    /// Cylindrical: rings round the whole object at the card's top and bottom, joined
    /// by two upright lines.
    pub guide: Vec<Vec<Vec3>>,
    pub anchor: Anchor,
}

/// Where the pattern sits, for its handles.
#[derive(Clone, Copy, Debug, Default)]
pub struct Anchor {
    /// The pattern's centre on the surface (on the card when its ray misses the object).
    pub center: Vec3,
    /// The pattern plane's right and up there, before the pattern is turned.
    pub right: Vec3,
    pub up: Vec3,
    /// The pattern's turn, in radians.
    pub angle: f32,
    /// Half the pattern's width and height before it is turned.
    pub half: Vec2,
}

impl Anchor {
    /// A point of the turned pattern: `p.x` along its right and `p.y` along its up.
    pub fn at(&self, p: Vec2) -> Vec3 {
        let p = Vec2::from_angle(self.angle).rotate(p);
        self.center + self.right * p.x + self.up * p.y
    }
}

/// How far the card floats off the object.
fn card_gap(e: &Emboss) -> f32 {
    (e.size * 0.8).max(4.0)
}

/// The preview of `e` on `target` (world space). An error, as [`super::apply`] would give,
/// when no part of the pattern lands on the object.
pub fn preview(target: &Mesh, e: &Emboss) -> Result<Preview> {
    let l = layout(e)?;
    let surface = Surface::new(target, e, l.step, l.lo, l.hi);
    let frame = &surface.frame;
    let gap = card_gap(e);
    let contours: Vec<Vec<Vec2>> = l
        .pieces
        .iter()
        .flatten()
        .map(|c| super::densify(c, l.step))
        .collect();
    let mut out = Preview::default();
    for c in &contours {
        out.surface.extend(landed(c, &surface));
        out.card.push(closed(c.iter().map(|&q| frame.lift(q, gap))));
    }
    if out.surface.is_empty() {
        // The outlines all miss, yet a pattern larger than the object may still cover
        // it: only the full build can tell.
        if pieces(target, e).is_err() {
            bail!(MISSES);
        }
    }
    // The border, a little outside the pattern, turned with it.
    let pad = Vec2::splat(e.size * 0.1);
    let (lo, hi) = (-l.half - pad, l.half + pad);
    let corners = [lo, Vec2::new(hi.x, lo.y), hi, Vec2::new(lo.x, hi.y)];
    let corners = corners.map(|p| super::turn(e, p));
    let rim = super::densify(&corners, l.step.max(1.0));
    out.border = closed(rim.iter().map(|&q| frame.lift(q, gap)));
    if e.projection == Projection::Cylindrical {
        let (bottom, top) = corners
            .iter()
            .fold((f32::MAX, f32::MIN), |(a, b), q| (a.min(q.y), b.max(q.y)));
        out.guide = guide(frame, e.offset.x, bottom, top, gap);
    }
    let [right, up] = frame.tangents(e.offset);
    out.anchor = Anchor {
        center: surface
            .hit(e.offset)
            .map_or_else(|| frame.lift(e.offset, gap), |(p, _)| p),
        right,
        up,
        angle: e.angle.to_radians(),
        half: l.half,
    };
    Ok(out)
}

/// The runs of a closed contour whose rays land on the object, as surface polylines.
fn landed(contour: &[Vec2], surface: &Surface) -> Vec<Vec<Vec3>> {
    let hits: Vec<Option<Vec3>> = contour
        .iter()
        .map(|&q| surface.hit(q).map(|(p, _)| p))
        .collect();
    let Some(gap) = hits.iter().position(Option::is_none) else {
        return vec![closed(hits.into_iter().flatten())];
    };
    // Start just after a miss, so a run across the contour's first point stays whole.
    let n = hits.len();
    let mut runs = Vec::new();
    let mut run = Vec::new();
    for k in 1..=n {
        match hits[(gap + k) % n] {
            Some(p) => run.push(p),
            None if run.len() > 1 => runs.push(std::mem::take(&mut run)),
            None => run.clear(),
        }
    }
    runs
}

/// The points with the first repeated at the end.
fn closed(points: impl Iterator<Item = Vec3>) -> Vec<Vec3> {
    let mut v: Vec<Vec3> = points.collect();
    if let Some(&first) = v.first() {
        v.push(first);
    }
    v
}

/// The projection cylinder at the card's radius, between heights `bottom` and `top`:
/// a full ring at each, and two upright lines a quarter turn either side of `x`.
fn guide(frame: &Frame, x: f32, bottom: f32, top: f32, gap: f32) -> Vec<Vec<Vec3>> {
    let turn = TAU * frame.radius;
    let ring =
        |y: f32| closed((0..96).map(|k| frame.lift(Vec2::new(x + turn * k as f32 / 96.0, y), gap)));
    let mut lines = vec![ring(bottom), ring(top)];
    for side in [-1.0, 1.0] {
        let x = x + side * PI / 2.0 * frame.radius;
        lines.push(vec![
            frame.lift(Vec2::new(x, bottom), gap),
            frame.lift(Vec2::new(x, top), gap),
        ]);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::super::apply;
    use super::*;
    use crate::primitives::Primitive;

    #[test]
    fn planar_outlines_lie_on_the_top_and_the_card_floats_above() {
        let cube = Primitive::Cube.build();
        let e = Emboss::default();
        let p = preview(&cube, &e).unwrap();
        assert!(!p.surface.is_empty());
        for q in p.surface.iter().flatten() {
            assert!((q.z - 40.0).abs() < 1e-3, "{q}");
        }
        let lift = 40.0 + card_gap(&e);
        for q in p.card.iter().flatten().chain(&p.border) {
            assert!((q.z - lift).abs() < 1e-3, "{q}");
        }
        assert!(p.guide.is_empty());
        assert!((p.anchor.center - Vec3::new(0.0, 0.0, 40.0)).length() < 1e-3);
        assert!(p.anchor.right.abs_diff_eq(Vec3::X, 1e-5));
        // Turned a quarter, the pattern's right is the plane's up.
        let turned = preview(&cube, &Emboss { angle: 90.0, ..e }).unwrap().anchor;
        let right = turned.at(Vec2::X) - turned.center;
        assert!(right.abs_diff_eq(Vec3::Y, 1e-5), "{right}");
    }

    #[test]
    fn cylindrical_preview_has_a_ring_round_the_object() {
        let e = Emboss {
            projection: Projection::Cylindrical,
            ..Emboss::default()
        };
        let p = preview(&Primitive::Cylinder.build(), &e).unwrap();
        assert_eq!(p.guide.len(), 4);
        let r = 20.0 + card_gap(&e);
        for q in p.guide.iter().flatten() {
            assert!((q.truncate().length() - r).abs() < 0.1, "{q}");
        }
        // The outlines land on the front, at about the cylinder's radius.
        for q in p.surface.iter().flatten() {
            assert!(
                q.y < 0.0 && (q.truncate().length() - 20.0).abs() < 0.5,
                "{q}"
            );
        }
    }

    #[test]
    fn a_pattern_over_an_edge_is_cut_there() {
        // Centred on the top's +X edge: the half past it misses the cube.
        let cube = Primitive::Cube.build();
        let e = Emboss {
            text: "HHHH".into(),
            offset: Vec2::new(20.0, 0.0),
            ..Emboss::default()
        };
        let p = preview(&cube, &e).unwrap();
        assert!(p.surface.iter().flatten().all(|q| q.x <= 20.0 + 1e-3));
        let out = apply(&cube, &e).unwrap().object;
        assert!(out.stats().is_closed_manifold(), "{:?}", out.stats());
        let b = out.aabb();
        assert!((b.max.x - 20.0).abs() < 1e-3 && (b.max.z - 41.0).abs() < 1e-3);
        assert!(out.stats().volume > cube.stats().volume + 10.0);
        // The same on the front face, past its upright edge.
        let side = Emboss {
            facing: -Vec3::Y,
            offset: Vec2::new(18.0, 0.0),
            ..e
        };
        let solid = super::super::text_solid(&cube, &side).unwrap();
        assert!(solid.stats().is_closed_manifold(), "{:?}", solid.stats());
        assert!(solid.aabb().max.x < 20.0 + 1e-3 && solid.aabb().min.y < -20.5);
        assert!(apply(&cube, &side)
            .unwrap()
            .object
            .stats()
            .is_closed_manifold());
    }

    #[test]
    fn turned_patterns_over_a_corner_still_apply() {
        let cube = Primitive::Cube.build();
        for (angle, engrave) in [(30.0, false), (45.0, true), (-60.0, false)] {
            let e = Emboss {
                text: "Wave".into(),
                angle,
                engrave,
                offset: Vec2::new(18.0, 17.0),
                ..Emboss::default()
            };
            let out = apply(&cube, &e).unwrap().object;
            assert!(
                out.stats().is_closed_manifold(),
                "{angle} {:?}",
                out.stats()
            );
            let v = out.stats().volume;
            assert!((v > 64000.0) != engrave, "{angle} {v}");
        }
    }

    #[test]
    fn cylindrical_text_past_the_top_is_cut() {
        // Half above the cylinder's top rim: the rays there pass over it.
        let e = Emboss {
            projection: Projection::Cylindrical,
            offset: Vec2::new(0.0, 20.0),
            ..Emboss::default()
        };
        let cyl = Primitive::Cylinder.build();
        assert!(preview(&cyl, &e).is_ok());
        let out = apply(&cyl, &e).unwrap().object;
        assert!(out.stats().is_closed_manifold(), "{:?}", out.stats());
        assert!(out.aabb().max.z < 40.0 + 1e-3);
    }

    #[test]
    fn nothing_on_the_object_is_an_error() {
        let e = Emboss {
            offset: Vec2::new(100.0, 0.0),
            ..Emboss::default()
        };
        let cube = Primitive::Cube.build();
        assert!(preview(&cube, &e).is_err());
        assert!(apply(&cube, &e).is_err());
        // Larger than the object all round: the outlines miss, the inside still lands.
        let big = Emboss {
            pattern: super::super::Pattern::Shape(super::super::Shape::Circle),
            size: 150.0,
            ..Emboss::default()
        };
        assert!(preview(&cube, &big).is_ok());
    }
}
