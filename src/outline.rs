//! Edit → Extrude outline: a border around (or inside) an object's silhouette seen from
//! above, for keychains and badges. Also the flat-shape helpers SVG import shares: a
//! cross-section from contours, and a cross-section extruded into a solid.

use crate::boolean::{self, Op};
use crate::mesh::Mesh;
use anyhow::{bail, Result};
use manifold_rust::cross_section::CrossSection;
use manifold_rust::linalg::{Vec2, Vec3};
use manifold_rust::manifold::Manifold;

/// How far round corners may stray from a true arc, mm: fine enough for small keychains.
pub const TOLERANCE: f64 = 0.05;
pub const DEFAULT_WIDTH: f32 = 2.0;
/// How far the border reaches under the model it is merged with, mm, so the two overlap
/// instead of only touching.
const OVERLAP: f64 = 0.01;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Side {
    /// A ring around the silhouette.
    #[default]
    Outside,
    /// A band just inside the silhouette's edge.
    Inside,
}

impl Side {
    pub const ALL: [Side; 2] = [Side::Outside, Side::Inside];

    /// The i18n key of its name.
    pub fn label(self) -> &'static str {
        match self {
            Side::Outside => "Outside",
            Side::Inside => "Inside",
        }
    }
}

/// Everything the tool bar sets.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Options {
    pub side: Side,
    /// Border width, mm.
    pub width: f32,
    /// Border height from the object's bottom, mm.
    pub height: f32,
    /// Merge the border with the model; off leaves the border alone.
    pub keep_model: bool,
    /// Rounds the border's free edge, 0 (follows the shape) to 1: its inward corners
    /// fill with arcs up to a quarter of the shape's narrower side across.
    pub smooth: f32,
}

impl Options {
    /// The defaults for an object (or selection) `height` mm tall.
    pub fn new(height: f32) -> Self {
        Self {
            side: Side::Outside,
            width: DEFAULT_WIDTH,
            height,
            keep_model: true,
            smooth: 0.0,
        }
    }
}

/// What the tool makes of the selection.
#[derive(Debug)]
pub enum Outlined {
    /// The first object's new world mesh (the others go): the model with its border, or
    /// with Keep model off the border alone.
    Replace(Mesh),
    /// The border as a new object beside the models, which stay as they are: several
    /// objects keep their own colours, and a mesh that is not a closed solid cannot be
    /// merged.
    Beside(Mesh),
}

/// The border around `models`, world-space meshes seen together from above, with `o`'s
/// settings. It rises from the lowest bottom among them.
pub fn outline(models: &[Mesh], o: &Options) -> Result<Outlined> {
    let ok = |v: f32| v.is_finite() && v > 0.0;
    if !ok(o.width) || !ok(o.height) {
        bail!("Width and height must be greater than zero");
    }
    let models: Vec<&Mesh> = models.iter().filter(|m| !m.is_empty()).collect();
    if models.is_empty() {
        bail!("The object is empty");
    }
    let shape = models
        .iter()
        .map(|m| silhouette(m))
        .reduce(|a, b| a.union(&b))
        .unwrap_or_default();
    if shape.is_empty() {
        bail!("The object has no area seen from above");
    }
    // One object merges with its border, which reaches a little under it so they overlap.
    let merge = o.keep_model && models.len() == 1;
    let w = o.width as f64;
    let overlap = if merge { OVERLAP } else { 0.0 };
    let b = shape.bounds();
    let radius = o.smooth.clamp(0.0, 1.0) as f64 * 0.25 * (b.max - b.min).x.min((b.max - b.min).y);
    let ring = match o.side {
        Side::Outside => {
            round_inward(&offset(&shape, w), radius).difference(&offset(&shape, -overlap))
        }
        // The inner edge is the free one: its corners turn round as the shrunk shape's
        // outward corners do.
        Side::Inside => shape.difference(&offset(&offset(&shape, -w - radius), radius)),
    };
    if ring.is_empty() {
        bail!("The border is empty; try a larger width");
    }
    let bottom = models
        .iter()
        .map(|m| m.aabb().min.z)
        .fold(f32::INFINITY, f32::min);
    let border = extrude(&ring, bottom, o.height);
    if !o.keep_model {
        return Ok(Outlined::Replace(border));
    }
    if !merge {
        return Ok(Outlined::Beside(border));
    }
    // Unpainted faces of both take the object's colour, painted ones keep their paint.
    Ok(
        match boolean::apply(Op::Union, &[models[0].clone(), border.clone()]) {
            Ok(merged) => Outlined::Replace(merged),
            Err(_) => Outlined::Beside(border),
        },
    )
}

/// The object seen from above: its world mesh projected onto the floor. Closed meshes go
/// through manifold's projection; any other is the union of its triangles' shadows.
pub fn silhouette(m: &Mesh) -> CrossSection {
    if let Ok(solid) = boolean::to_manifold(m) {
        return solid.project();
    }
    let shadows: Vec<Vec<Vec2>> = m
        .triangle_positions()
        .filter_map(|[a, b, c]| {
            let area = (b.x - a.x) * (c.y - a.y) - (c.x - a.x) * (b.y - a.y);
            let flat = |p: glam::Vec3| Vec2::new(p.x as f64, p.y as f64);
            match area {
                x if x > 0.0 => Some(vec![flat(a), flat(b), flat(c)]),
                x if x < 0.0 => Some(vec![flat(a), flat(c), flat(b)]),
                _ => None,
            }
        })
        .collect();
    CrossSection::new(shadows)
}

/// Fills a shape's inward corners and narrow notches with arcs of `radius` (grow, then
/// shrink back), leaving its outward edges where they are.
fn round_inward(shape: &CrossSection, radius: f64) -> CrossSection {
    if radius <= 0.0 {
        return shape.clone();
    }
    offset(&offset(shape, radius), -radius)
}

/// Grows (or with a negative `delta` shrinks) a shape, with round corners within
/// [`TOLERANCE`].
pub fn offset(shape: &CrossSection, delta: f64) -> CrossSection {
    if delta == 0.0 {
        return shape.clone();
    }
    // Round joins with enough segments that each one's sagitta stays within tolerance.
    let r = delta.abs();
    let step = 2.0 * (1.0 - (TOLERANCE / r).min(1.0)).acos();
    let segments = (std::f64::consts::TAU / step.max(1e-3))
        .ceil()
        .clamp(8.0, 256.0);
    shape.offset_with_params(delta, 1, 2.0, segments as i32)
}

/// A flat shape on the XY plane raised `height` mm from `bottom`, as a closed mesh.
pub fn extrude(shape: &CrossSection, bottom: f32, height: f32) -> Mesh {
    let solid = Manifold::extrude(
        &shape.to_polygons(),
        height as f64,
        0,
        0.0,
        Vec2::new(1.0, 1.0),
    )
    .translate(Vec3::new(0.0, 0.0, bottom as f64));
    boolean::from_manifold(&solid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::cube;
    use glam::{Mat4, Vec3 as V3};

    /// A 10 mm cube centred on the origin, 10 mm tall, resting on z = 0.
    fn block() -> Mesh {
        let m = cube(10.0);
        let b = m.aabb();
        m.transformed(&Mat4::from_translation(-V3::new(
            b.center().x,
            b.center().y,
            b.min.z,
        )))
    }

    /// A 20 mm square plate, 4 mm tall, with a 10 mm square hole through its middle.
    fn ring() -> Mesh {
        let outer = cube(20.0).transformed(&Mat4::from_scale(V3::new(1.0, 1.0, 0.2)));
        let hole = cube(10.0);
        boolean::apply(Op::Subtract, &[outer, hole]).unwrap()
    }

    fn solid(o: Outlined) -> Mesh {
        let m = match o {
            Outlined::Replace(m) | Outlined::Beside(m) => m,
        };
        assert!(m.stats().is_closed_manifold(), "{:?}", m.stats());
        m
    }

    fn opts(side: Side, width: f32, height: f32, keep_model: bool) -> Options {
        Options {
            side,
            width,
            height,
            keep_model,
            smooth: 0.0,
        }
    }

    /// Area of a square of side `s` grown by `w` with round corners.
    fn grown(s: f64, w: f64) -> f64 {
        s * s + 4.0 * s * w + std::f64::consts::PI * w * w
    }

    #[test]
    fn outside_border_alone_is_a_ring_of_the_width() {
        for w in [1.0_f32, 2.0, 3.5] {
            let m = solid(outline(&[block()], &opts(Side::Outside, w, 4.0, false)).unwrap());
            let b = m.aabb();
            assert!((b.size().x - (10.0 + 2.0 * w)).abs() < 1e-3, "{b:?}");
            assert!(b.min.z.abs() < 1e-4 && (b.max.z - 4.0).abs() < 1e-4);
            let area = grown(10.0, w as f64) - 100.0;
            let v = m.stats().volume;
            assert!((v - area * 4.0).abs() / (area * 4.0) < 0.01, "{w}: {v}");
        }
    }

    #[test]
    fn inside_border_alone_is_a_band_under_the_edge() {
        let m = solid(outline(&[block()], &opts(Side::Inside, 2.0, 10.0, false)).unwrap());
        let b = m.aabb();
        assert!((b.size().x - 10.0).abs() < 1e-3);
        // 10×10 minus the 6×6 middle.
        assert!(
            (m.stats().volume - 640.0).abs() < 1.0,
            "{}",
            m.stats().volume
        );
    }

    #[test]
    fn keeping_the_model_merges_it_with_the_border() {
        let m = solid(outline(&[block()], &opts(Side::Outside, 2.0, 3.0, true)).unwrap());
        let v = m.stats().volume;
        let expected = 1000.0 + (grown(10.0, 2.0) - 100.0) * 3.0;
        assert!((v - expected).abs() / expected < 0.01, "{v} vs {expected}");
        assert_eq!(m.solids().len(), 1);
        // An inside border within a solid cube adds nothing.
        let m = solid(outline(&[block()], &opts(Side::Inside, 2.0, 5.0, true)).unwrap());
        assert!((m.stats().volume - 1000.0).abs() < 1.0);
    }

    #[test]
    fn a_ring_gets_borders_on_both_edges() {
        // Outside: round the outer edge, and the hole closes in by the width.
        let m = solid(outline(&[ring()], &opts(Side::Outside, 1.0, 4.0, false)).unwrap());
        let area = grown(20.0, 1.0) - 400.0 + (100.0 - 64.0);
        assert!((m.stats().volume - area * 4.0).abs() < 1.0);
        // Inside: a band along the outer edge and one round the hole, whose corners
        // grow round.
        let m = solid(outline(&[ring()], &opts(Side::Inside, 1.0, 4.0, false)).unwrap());
        let area = (400.0 - 324.0) + 40.0 + std::f64::consts::PI;
        assert!(
            (m.stats().volume - area * 4.0).abs() < 1.0,
            "{}",
            m.stats().volume
        );
    }

    #[test]
    fn an_open_mesh_keeps_its_border_beside_it() {
        let mut open = block();
        open.triangles.pop();
        let o = outline(&[open], &opts(Side::Outside, 2.0, 3.0, true)).unwrap();
        assert!(matches!(o, Outlined::Beside(_)));
        solid(o);
    }

    #[test]
    fn several_objects_share_one_border_beside_them() {
        let right = block().transformed(&Mat4::from_translation(V3::new(11.5, 0.0, 2.0)));
        let both = [block(), right];
        let o = outline(&both, &opts(Side::Outside, 1.0, 3.0, true)).unwrap();
        assert!(matches!(o, Outlined::Beside(_)));
        let m = solid(o);
        // Round both cubes and across the 1.5 mm gap between them, from the lower bottom.
        assert_eq!(m.solids().len(), 1);
        let b = m.aabb();
        assert!(
            (b.size().x - 23.5).abs() < 1e-3 && b.min.z.abs() < 1e-4,
            "{b:?}"
        );
        // Without the models it replaces them.
        let o = outline(&both, &opts(Side::Outside, 1.0, 3.0, false)).unwrap();
        assert!(matches!(o, Outlined::Replace(_)));
    }

    #[test]
    fn smoothing_rounds_the_inward_corners_of_the_border() {
        // An L of two blocks: its inner corner is sharp at 0 and fills in as it smooths.
        let up = block().transformed(&Mat4::from_translation(V3::new(0.0, 9.0, 0.0)));
        let side = block().transformed(&Mat4::from_translation(V3::new(9.0, 0.0, 0.0)));
        let l = [block(), up, side];
        let volume = |smooth: f32| {
            let o = Options {
                smooth,
                ..opts(Side::Outside, 1.0, 2.0, false)
            };
            solid(outline(&l, &o).unwrap()).stats().volume
        };
        let (sharp, half, full) = (volume(0.0), volume(0.5), volume(1.0));
        assert!(sharp < half && half < full, "{sharp} {half} {full}");
        // On a convex block there is nothing to fill.
        let o = Options {
            smooth: 1.0,
            ..opts(Side::Outside, 2.0, 4.0, false)
        };
        let v = solid(outline(&[block()], &o).unwrap()).stats().volume;
        let area = grown(10.0, 2.0) - 100.0;
        assert!((v - area * 4.0).abs() / (area * 4.0) < 0.01, "{v}");
    }

    #[test]
    fn bad_settings_are_refused() {
        assert!(outline(&[block()], &opts(Side::Outside, 0.0, 3.0, true)).is_err());
        assert!(outline(&[block()], &opts(Side::Outside, 2.0, f32::NAN, true)).is_err());
        assert!(outline(&[], &opts(Side::Outside, 2.0, 3.0, true)).is_err());
        // Too wide to fit inside leaves the whole silhouette, not nothing.
        let m = solid(outline(&[block()], &opts(Side::Inside, 8.0, 1.0, false)).unwrap());
        assert!((m.stats().volume - 100.0).abs() < 1e-2);
    }
}
