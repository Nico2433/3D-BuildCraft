//! The built-in shapes a pattern can be, the set the original's Pattern menu offers, each
//! drawn here as closed outlines (y up): outer contours counter-clockwise, holes
//! clockwise, as glyph holes are. The same outlines are embossed and drawn as the menu's
//! icons ([`shape_fill`]), so an icon always matches its result.

use crate::cut::fill;
use glam::{Vec2, Vec3};
use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI, TAU};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// A four-pointed star.
    Star,
    /// A star with eight sharp points.
    Burst,
    Arrow,
    Chevron,
    Cloud,
    /// A circle with a round hole.
    Ring,
    /// A banner with wavy top and bottom edges.
    Wave,
    Heart,
    Lightning,
    /// A crescent moon.
    Moon,
    /// A diagonal cross.
    Cross,
    /// A ring crossed by a diagonal bar.
    NoSign,
    Circle,
    Plus,
    Square,
    RoundedSquare,
    /// A face: a disc with eyes and a smile cut out.
    Smiley,
    Triangle,
    /// A ribbon bent like a flag in the wind.
    Flag,
}

impl Shape {
    /// In the order of the original's menu, six to a row.
    pub const ALL: [Shape; 19] = [
        Shape::Star,
        Shape::Burst,
        Shape::Arrow,
        Shape::Chevron,
        Shape::Cloud,
        Shape::Ring,
        Shape::Wave,
        Shape::Heart,
        Shape::Lightning,
        Shape::Moon,
        Shape::Cross,
        Shape::NoSign,
        Shape::Circle,
        Shape::Plus,
        Shape::Square,
        Shape::RoundedSquare,
        Shape::Smiley,
        Shape::Triangle,
        Shape::Flag,
    ];

    /// Its name in the UI (translated through `tr`).
    pub fn label(self) -> &'static str {
        match self {
            Shape::Star => "Star",
            Shape::Burst => "Burst",
            Shape::Arrow => "Arrow",
            Shape::Chevron => "Chevron",
            Shape::Cloud => "Cloud",
            Shape::Ring => "Ring",
            Shape::Wave => "Wave",
            Shape::Heart => "Heart",
            Shape::Lightning => "Lightning",
            Shape::Moon => "Moon",
            Shape::Cross => "Cross",
            Shape::NoSign => "No sign",
            Shape::Circle => "Circle",
            Shape::Plus => "Plus",
            Shape::Square => "Square",
            Shape::RoundedSquare => "Rounded square",
            Shape::Smiley => "Smiley",
            Shape::Triangle => "Triangle",
            Shape::Flag => "Flag",
        }
    }

    /// A stable snake_case name, for agents: `RoundedSquare` is "rounded_square".
    pub fn name(self) -> String {
        self.label().to_lowercase().replace(' ', "_")
    }

    /// The shape called `name` (see [`Shape::name`]).
    pub fn from_name(name: &str) -> Option<Shape> {
        Shape::ALL.into_iter().find(|s| s.name() == name)
    }

    /// Its closed outlines, outer ones counter-clockwise and holes clockwise, in units of
    /// about ±1 (the pattern scales them to its size).
    pub fn outline(self) -> Vec<Vec<Vec2>> {
        let outer = |p: Vec<Vec2>| vec![wound(p, true)];
        match self {
            Shape::Star => outer(star(4, 0.27)),
            Shape::Burst => outer(star(8, 0.62)),
            Shape::Arrow => outer(points(&[
                (-0.92, 0.26),
                (0.08, 0.26),
                (0.08, 0.7),
                (0.95, 0.0),
                (0.08, -0.7),
                (0.08, -0.26),
                (-0.92, -0.26),
            ])),
            Shape::Chevron => outer(points(&[
                (-0.75, 0.92),
                (-0.2, 0.92),
                (0.8, 0.0),
                (-0.2, -0.92),
                (-0.75, -0.92),
                (0.25, 0.0),
            ])),
            Shape::Cloud => outer(cloud()),
            Shape::Ring => with_holes(circle(Vec2::ZERO, 1.0), vec![circle(Vec2::ZERO, 0.66)]),
            Shape::Wave => outer(band(0.32, 0.1, 1.5)),
            Shape::Heart => outer(heart()),
            Shape::Lightning => outer(points(&[
                (-0.2, 1.0),
                (0.45, 1.0),
                (0.1, 0.25),
                (0.5, 0.25),
                (-0.35, -1.0),
                (-0.05, -0.1),
                (-0.45, -0.1),
            ])),
            Shape::Moon => outer(moon()),
            Shape::Cross => outer(
                plus(0.24)
                    .iter()
                    .map(|p| p.rotate(Vec2::from_angle(FRAC_PI_4)))
                    .collect(),
            ),
            Shape::NoSign => no_sign(),
            Shape::Circle => outer(circle(Vec2::ZERO, 1.0)),
            Shape::Plus => outer(plus(0.3)),
            Shape::Square => outer(points(&[
                (-1.0, -1.0),
                (1.0, -1.0),
                (1.0, 1.0),
                (-1.0, 1.0),
            ])),
            Shape::RoundedSquare => outer(rounded_square(0.3)),
            Shape::Smiley => smiley(),
            Shape::Triangle => outer(points(&[(-1.155, -1.0), (1.155, -1.0), (0.0, 1.0)])),
            Shape::Flag => outer(band(0.27, 0.2, 0.5)),
        }
    }
}

/// A shape filled, for drawing its icon: its outlines' points centred in a unit square
/// (the larger side is 1, y up), where each outline runs in that list, and the triangles
/// that cover it, holes left open.
pub struct Filled {
    pub points: Vec<Vec2>,
    pub contours: Vec<std::ops::Range<usize>>,
    pub triangles: Vec<[u32; 3]>,
}

/// `shape`'s outlines filled (see [`Filled`]), from the same outlines it embosses.
pub fn shape_fill(shape: Shape) -> Filled {
    let outlines = shape.outline();
    let (lo, hi) = outlines
        .iter()
        .flatten()
        .fold((Vec2::MAX, Vec2::MIN), |(lo, hi), p| {
            (lo.min(*p), hi.max(*p))
        });
    let (mid, scale) = ((lo + hi) * 0.5, 1.0 / (hi - lo).max_element().max(1e-6));
    let mut points = Vec::new();
    let mut contours = Vec::new();
    for c in &outlines {
        contours.push(points.len()..points.len() + c.len());
        points.extend(c.iter().map(|p| (*p - mid) * scale));
    }
    let flat: Vec<Vec3> = points.iter().map(|p| p.extend(0.0)).collect();
    let loops: Vec<Vec<u32>> = contours
        .iter()
        .map(|r| (r.start as u32..r.end as u32).collect())
        .collect();
    let triangles = fill(&flat, &loops);
    Filled {
        points,
        contours,
        triangles,
    }
}

fn points(p: &[(f32, f32)]) -> Vec<Vec2> {
    p.iter().map(|&(x, y)| Vec2::new(x, y)).collect()
}

/// Twice the signed area of a closed outline: positive when it runs counter-clockwise.
fn area(p: &[Vec2]) -> f32 {
    (0..p.len())
        .map(|i| p[i].perp_dot(p[(i + 1) % p.len()]))
        .sum()
}

/// The outline running counter-clockwise (`ccw`) or clockwise.
fn wound(mut p: Vec<Vec2>, ccw: bool) -> Vec<Vec2> {
    if (area(&p) > 0.0) != ccw {
        p.reverse();
    }
    p
}

/// An outer outline and its holes, each wound its way.
fn with_holes(outer: Vec<Vec2>, holes: Vec<Vec<Vec2>>) -> Vec<Vec<Vec2>> {
    let mut out = vec![wound(outer, true)];
    out.extend(holes.into_iter().map(|h| wound(h, false)));
    out
}

/// Points along the circle round `c` from angle `from` to `to` (radians), both ends in.
fn arc(c: Vec2, r: f32, from: f32, to: f32, n: usize) -> Vec<Vec2> {
    (0..=n)
        .map(|k| c + Vec2::from_angle(from + (to - from) * k as f32 / n as f32) * r)
        .collect()
}

fn circle(c: Vec2, r: f32) -> Vec<Vec2> {
    let mut p = arc(c, r, 0.0, TAU, 96);
    p.pop();
    p
}

/// A star with `n` points of radius 1 (one straight up) and valleys at `inner`.
fn star(n: usize, inner: f32) -> Vec<Vec2> {
    (0..2 * n)
        .map(|i| {
            let r = if i % 2 == 0 { 1.0 } else { inner };
            Vec2::from_angle(FRAC_PI_2 + PI * i as f32 / n as f32) * r
        })
        .collect()
}

/// A plus whose arms reach 1 and are `half` wide either side of the centre lines.
fn plus(half: f32) -> Vec<Vec2> {
    let (a, h) = (1.0, half);
    points(&[
        (h, h),
        (h, a),
        (-h, a),
        (-h, h),
        (-a, h),
        (-a, -h),
        (-h, -h),
        (-h, -a),
        (h, -a),
        (h, -h),
        (a, -h),
        (a, h),
    ])
}

fn rounded_square(r: f32) -> Vec<Vec2> {
    let c = 1.0 - r;
    [(c, c), (-c, c), (-c, -c), (c, -c)]
        .iter()
        .enumerate()
        .flat_map(|(k, &(x, y))| {
            let from = k as f32 * FRAC_PI_2;
            arc(Vec2::new(x, y), r, from, from + FRAC_PI_2, 12)
        })
        .collect()
}

/// A band 2 wide whose edges, `half` above and below the middle, wave with amplitude
/// `amp` through `turns` periods.
fn band(half: f32, amp: f32, turns: f32) -> Vec<Vec2> {
    let n = 64;
    let x = |k: usize| -1.0 + 2.0 * k as f32 / n as f32;
    let y = |x: f32| amp * (turns * PI * x).sin();
    let mut p: Vec<Vec2> = (0..=n).map(|k| Vec2::new(x(k), y(x(k)) - half)).collect();
    p.extend((0..=n).rev().map(|k| Vec2::new(x(k), y(x(k)) + half)));
    p
}

/// The classic heart curve.
fn heart() -> Vec<Vec2> {
    (0..96)
        .map(|i| {
            let t = i as f32 / 96.0 * TAU;
            Vec2::new(
                16.0 * t.sin().powi(3),
                13.0 * t.cos() - 5.0 * (2.0 * t).cos() - 2.0 * (3.0 * t).cos() - (4.0 * t).cos(),
            ) / 16.0
        })
        .collect()
}

/// Puffs round a wide middle: the farthest edge of any of the circles along each ray from
/// the centre, so the dips between puffs stay sharp.
fn cloud() -> Vec<Vec2> {
    let mut puffs = vec![(Vec2::ZERO, 0.55)];
    puffs.extend((0..9).map(|k| {
        let a = TAU * k as f32 / 9.0 + 0.2;
        (Vec2::new(0.68 * a.cos(), 0.38 * a.sin()), 0.32)
    }));
    (0..180)
        .map(|i| {
            let u = Vec2::from_angle(TAU * i as f32 / 180.0);
            let reach = puffs
                .iter()
                .filter_map(|&(c, r)| {
                    let b = u.dot(c);
                    let d = b * b - c.length_squared() + r * r;
                    (d >= 0.0).then(|| b + d.sqrt())
                })
                .fold(0.0, f32::max);
            u * reach
        })
        .collect()
}

/// A crescent opening to the right: a disc with an overlapping disc taken away.
fn moon() -> Vec<Vec2> {
    let (r1, c2, r2) = (1.0_f32, Vec2::new(0.5, 0.0), 0.85_f32);
    // Where the two circles cross (above the axis; the other is its mirror).
    let x = (c2.x * c2.x + r1 * r1 - r2 * r2) / (2.0 * c2.x);
    let top = Vec2::new(x, (r1 * r1 - x * x).sqrt());
    let outer = top.y.atan2(top.x);
    let inner = (top - c2).y.atan2((top - c2).x);
    // Round the left of the disc from the top crossing to the bottom one, then back up
    // along the cut, round its left too.
    let mut p = arc(Vec2::ZERO, r1, outer, TAU - outer, 64);
    p.pop();
    let mut cut = arc(c2, r2, TAU - inner, inner, 48);
    cut.pop();
    p.extend(cut);
    p
}

/// A ring with a bar from its top left to its bottom right: two half-moon holes.
fn no_sign() -> Vec<Vec<Vec2>> {
    let (inner, half) = (0.7_f32, 0.13_f32);
    let span = (half / inner).acos();
    let holes = [FRAC_PI_4, PI + FRAC_PI_4]
        .iter()
        .map(|&mid| arc(Vec2::ZERO, inner, mid - span, mid + span, 40))
        .collect();
    with_holes(circle(Vec2::ZERO, 1.0), holes)
}

/// A disc with two eyes and a smile cut out.
fn smiley() -> Vec<Vec<Vec2>> {
    let eye = |x: f32| circle(Vec2::new(x, 0.3), 0.13);
    let c = Vec2::new(0.0, 0.08);
    let (from, to) = (-150.0_f32.to_radians(), -30.0_f32.to_radians());
    let mut mouth = arc(c, 0.6, from, to, 32);
    mouth.extend(arc(c, 0.47, to, from, 32));
    with_holes(circle(Vec2::ZERO, 1.0), vec![eye(-0.33), eye(0.33), mouth])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip_and_outlines_are_wound() {
        for s in Shape::ALL {
            assert_eq!(Shape::from_name(&s.name()), Some(s));
            let outline = s.outline();
            assert!(area(&outline[0]) > 0.0, "{s:?}");
            for hole in &outline[1..] {
                assert!(area(hole) < 0.0, "{s:?}");
            }
            let filled = shape_fill(s);
            assert!(!filled.triangles.is_empty(), "{s:?}");
            assert_eq!(filled.contours.len(), outline.len());
            for p in filled.points {
                assert!(p.abs().max_element() <= 0.5 + 1e-5, "{s:?} {p}");
            }
        }
        assert_eq!(Shape::from_name("no_sign"), Some(Shape::NoSign));
    }
}
