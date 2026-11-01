//! Emboss ("Relieve"): text, a shape or an image raised from or engraved into an object.
//! The pattern is laid out flat, then each point is sent along a ray towards the object
//! (straight down, towards the vertical axis, or towards the centre) and the pattern grows
//! out of the surface along that ray, so it follows the surface.

use crate::boolean::{self, Op};
use crate::cut::fill;
use crate::io::Rgba;
use crate::mesh::{ray_triangle, Mesh};
use anyhow::{anyhow, bail, Result};
use glam::{Vec2, Vec3};
use std::collections::HashMap;
use std::f32::consts::{FRAC_PI_2, PI, TAU};
use std::path::PathBuf;
use std::sync::OnceLock;

mod outlines;
use outlines::pattern_outlines;

/// How the pattern is sent onto the object.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Projection {
    /// Straight down onto the top.
    #[default]
    Planar,
    /// Wrapped around the vertical axis through the object's centre, on its front (−Y).
    Cylindrical,
    /// Wrapped around the object's centre, on its front (−Y).
    Spherical,
}

impl Projection {
    pub const ALL: [Projection; 3] = [
        Projection::Planar,
        Projection::Cylindrical,
        Projection::Spherical,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Projection::Planar => "Planar",
            Projection::Cylindrical => "Cylindrical",
            Projection::Spherical => "Spherical",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Default)]
pub enum Pattern {
    #[default]
    Text,
    Shape(Shape),
    /// Dark, opaque pixels of the image form the pattern.
    Image(PathBuf),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Star,
    Heart,
    Circle,
}

impl Shape {
    pub const ALL: [Shape; 3] = [Shape::Star, Shape::Heart, Shape::Circle];

    pub fn label(self) -> &'static str {
        match self {
            Shape::Star => "Star",
            Shape::Heart => "Heart",
            Shape::Circle => "Circle",
        }
    }
}

/// How the pattern sits on the object. Lengths in mm, angle in degrees.
#[derive(Clone, Debug, PartialEq)]
pub struct Emboss {
    pub pattern: Pattern,
    pub text: String,
    /// A system font family; `None` is the font bundled with the app (Ubuntu Light).
    pub font: Option<String>,
    /// Capital letter height for text, overall height for shapes and images.
    pub size: f32,
    /// Height above the surface, or depth below it when engraving.
    pub depth: f32,
    /// Turn of the pattern in its own plane.
    pub angle: f32,
    /// Pattern centre in its own plane: from the centre of the top for planar, along the
    /// surface from the front for cylindrical and spherical.
    pub offset: Vec2,
    pub engrave: bool,
    pub projection: Projection,
    /// Slopes the edges: they start at a third of the height and rise to full height.
    pub bevel: bool,
    /// Colour of the top layer and of the sides of raised patterns; `None` keeps the
    /// object's colour. A different colour makes that part a separate object.
    pub top_color: Option<Rgba>,
    pub side_color: Option<Rgba>,
}

impl Default for Emboss {
    fn default() -> Self {
        Self {
            pattern: Pattern::Text,
            text: "Text".into(),
            font: None,
            size: 10.0,
            depth: 1.0,
            angle: 0.0,
            offset: Vec2::ZERO,
            engrave: false,
            projection: Projection::Planar,
            bevel: false,
            top_color: None,
            side_color: None,
        }
    }
}

/// The object after embossing, plus the parts that got a colour of their own.
pub struct Embossed {
    pub object: Mesh,
    pub parts: Vec<(Mesh, Rgba)>,
}

/// How far the pattern reaches past the surface on the side that is not seen, so the
/// boolean has overlap to work with.
const OVERLAP: f32 = 0.5;
/// Height of the edge of a bevelled pattern, as a fraction of the full height.
const BEVEL_EDGE: f32 = 1.0 / 3.0;

/// The object with the pattern added or cut away. `target` is in world space.
pub fn apply(target: &Mesh, e: &Emboss) -> Result<Embossed> {
    let pieces = pieces(target, e)?;
    // Pieces (glyphs) may overlap their neighbours, so they are joined by a union.
    let join = |layer: Layer| -> Result<Mesh> {
        let solids: Vec<Mesh> = pieces.iter().map(|p| p.solid(e, layer)).collect();
        boolean::apply(Op::Union, &solids)
    };
    let with = |op, m: Mesh| boolean::apply(op, &[target.clone(), m]);
    if e.engrave {
        return Ok(Embossed {
            object: with(Op::Subtract, join(Layer::Whole)?)?,
            parts: Vec::new(),
        });
    }
    // Coloured sides leave the object; their solid is trimmed to the surface.
    let sides = |c: Rgba| -> Result<(Mesh, Rgba)> {
        Ok((
            boolean::apply(Op::Subtract, &[join(Layer::Body)?, target.clone()])?,
            c,
        ))
    };
    Ok(match (e.top_color, e.side_color) {
        (None, None) => Embossed {
            object: with(Op::Union, join(Layer::Whole)?)?,
            parts: Vec::new(),
        },
        (Some(top), None) => Embossed {
            object: with(Op::Union, join(Layer::Body)?)?,
            parts: vec![(join(Layer::Cap)?, top)],
        },
        (None, Some(side)) => Embossed {
            object: with(Op::Union, join(Layer::Cap)?)?,
            parts: vec![sides(side)?],
        },
        (Some(top), Some(side)) => Embossed {
            object: target.clone(),
            parts: vec![sides(side)?, (join(Layer::Cap)?, top)],
        },
    })
}

/// The whole pattern draped over `target` as one mesh, for the preview. Neighbouring
/// pieces may overlap; `apply` joins them properly.
pub fn text_solid(target: &Mesh, e: &Emboss) -> Result<Mesh> {
    let mut out = Mesh::new(Vec::new(), Vec::new());
    for piece in pieces(target, e)? {
        let g = piece.solid(e, Layer::Whole);
        let n = out.positions.len() as u32;
        out.positions.extend(g.positions);
        out.triangles
            .extend(g.triangles.iter().map(|t| t.map(|i| i + n)));
    }
    Ok(out)
}

/// The `offset` that centres the pattern on `p`, a point on `target`'s surface.
pub fn offset_at(target: &Mesh, projection: Projection, p: Vec3) -> Vec2 {
    Frame::new(target, projection).key(p)
}

/// Reuse the projection frame while dragging an immutable placed target. This avoids
/// cloning and transforming all its vertices for every pointer movement.
pub fn offset_at_placed(
    target: &std::sync::Arc<Mesh>,
    matrix: glam::Mat4,
    projection: Projection,
    p: Vec3,
) -> Vec2 {
    type Cached = (std::sync::Weak<Mesh>, glam::Mat4, Projection, Frame);
    thread_local! { static LAST: std::cell::RefCell<Option<Cached>> = const { std::cell::RefCell::new(None) }; }
    LAST.with(|last| {
        let mut last = last.borrow_mut();
        let key = std::sync::Arc::downgrade(target);
        if !last
            .as_ref()
            .is_some_and(|(m, t, kind, _)| m.ptr_eq(&key) && *t == matrix && *kind == projection)
        {
            let frame = Frame::new(&target.transformed(&matrix), projection);
            *last = Some((key, matrix, projection, frame));
        }
        last.as_ref().unwrap().3.key(p)
    })
}

/// Families of the fonts installed on this computer, sorted. Loaded once.
pub fn font_families() -> Vec<String> {
    let mut names: Vec<String> = fonts()
        .faces()
        .flat_map(|f| f.families.first().map(|(n, _)| n.clone()))
        .collect();
    names.sort_by_key(|n| n.to_lowercase());
    names.dedup();
    names
}

/// The regular face of an installed family and its index in the file, or `None` when the
/// family is not installed.
pub fn font_data(family: &str) -> Option<(Vec<u8>, u32)> {
    let db = fonts();
    let id = db.query(&fontdb::Query {
        families: &[fontdb::Family::Name(family)],
        ..fontdb::Query::default()
    })?;
    // `query` falls back to another family when the name is unknown.
    let face = db.face(id)?;
    if !face
        .families
        .iter()
        .any(|(n, _)| n.eq_ignore_ascii_case(family))
    {
        return None;
    }
    db.with_face_data(id, |data, index| (data.to_vec(), index))
}

fn fonts() -> &'static fontdb::Database {
    static DB: OnceLock<fontdb::Database> = OnceLock::new();
    DB.get_or_init(|| {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        db
    })
}

#[derive(Clone, Copy)]
enum Layer {
    Whole,
    /// Everything under the top layer.
    Body,
    /// The top layer.
    Cap,
}

/// One glyph (or the whole shape or image), filled, refined and sent onto the surface.
struct Piece {
    top: Vec<[u32; 3]>,
    /// Where each vertex meets the surface and the ray direction there.
    hits: Vec<(Vec3, Vec3)>,
    /// Height fraction from the bevel, 1 without one.
    rise: Vec<f32>,
}

impl Piece {
    /// The closed solid between two heights along the rays (positive is outwards).
    fn solid(&self, e: &Emboss, layer: Layer) -> Mesh {
        let full = |r: f32| if e.engrave { OVERLAP } else { e.depth * r };
        let floor = |r: f32| if e.engrave { -e.depth * r } else { -OVERLAP };
        let cap = (e.depth * 0.25).min(0.4);
        let (lo, hi): (Vec<f32>, Vec<f32>) = self
            .rise
            .iter()
            .map(|&r| match layer {
                Layer::Whole => (floor(r), full(r)),
                Layer::Body => (floor(r), full(r) - cap),
                Layer::Cap => (full(r) - cap, full(r)),
            })
            .unzip();
        let at = |h: &[f32]| -> Vec<Vec3> {
            self.hits
                .iter()
                .zip(h)
                .map(|((p, dir), h)| *p - *dir * *h)
                .collect()
        };
        let n = self.hits.len() as u32;
        let mut positions = at(&hi);
        positions.extend(at(&lo));
        let mut triangles = Vec::with_capacity(self.top.len() * 2);
        let mut edges: HashMap<(u32, u32), ()> = HashMap::new();
        for &[a, b, c] in &self.top {
            triangles.push([a, b, c]);
            triangles.push([a + n, c + n, b + n]);
            for (x, y) in [(a, b), (b, c), (c, a)] {
                edges.insert((x, y), ());
            }
        }
        for &(a, b) in edges.keys() {
            if !edges.contains_key(&(b, a)) {
                triangles.push([b, a, a + n]);
                triangles.push([b, a + n, b + n]);
            }
        }
        Mesh::new(positions, triangles)
    }
}

fn pieces(target: &Mesh, e: &Emboss) -> Result<Vec<Piece>> {
    let finite = [e.size, e.depth, e.angle, e.offset.x, e.offset.y]
        .iter()
        .all(|v| v.is_finite());
    if !(finite && e.size > 0.0 && e.depth > 0.0) {
        bail!("Size and depth must be greater than zero");
    }
    // Narrower than half a stroke of a light font, so strokes still reach full height.
    let bevel_width = e.size * 0.04;
    let step = (e.size / 16.0).clamp(0.2, 2.0);
    // A bevel needs fine points along the outline; inside, the normal step is enough.
    let edge_step = if e.bevel {
        step.min(bevel_width / 2.0).max(0.1)
    } else {
        step
    };
    let (sin, cos) = e.angle.to_radians().sin_cos();
    let place = |p: Vec2| e.offset + Vec2::new(p.x * cos - p.y * sin, p.x * sin + p.y * cos);
    let outlines = pattern_outlines(e)?;
    if outlines.is_empty() {
        bail!("Nothing to emboss: type some text or pick a pattern");
    }
    let (lo, hi) = outlines
        .iter()
        .flatten()
        .flatten()
        .map(|&p| place(p))
        .fold((Vec2::MAX, Vec2::MIN), |(lo, hi), p| (lo.min(p), hi.max(p)));
    let surface = Surface::new(target, e.projection, step, lo, hi);
    outlines
        .iter()
        .map(|contours| {
            // Flat face in the pattern's plane: outlines with no edge longer than
            // `edge_step`, filled, then refined so it can bend with the surface.
            let mut positions = Vec::new();
            let mut loops = Vec::new();
            for contour in contours {
                let mut ids = Vec::new();
                for (i, &a) in contour.iter().enumerate() {
                    let b = contour[(i + 1) % contour.len()];
                    let parts = ((b - a).length() / edge_step).ceil().max(1.0) as usize;
                    for k in 0..parts {
                        ids.push(positions.len() as u32);
                        positions.push(place(a.lerp(b, k as f32 / parts as f32)).extend(0.0));
                    }
                }
                loops.push(ids);
            }
            // `fill` faces down; the top face must face up.
            let flat: Vec<[u32; 3]> = fill(&positions, &loops)
                .into_iter()
                .map(|[a, b, c]| [a, c, b])
                .collect();
            let top = refine(&mut positions, flat, step);
            let rise = if e.bevel {
                bevel(&positions, &top, bevel_width)
            } else {
                vec![1.0; positions.len()]
            };
            let hits = positions
                .iter()
                .map(|q| {
                    surface.hit(q.truncate()).ok_or_else(|| {
                        anyhow!(
                            "The pattern does not fit on the object; make it smaller or move it"
                        )
                    })
                })
                .collect::<Result<_>>()?;
            Ok(Piece { top, hits, rise })
        })
        .collect()
}

/// Height fraction per vertex: `BEVEL_EDGE` on the outline, rising to 1 at `width` in.
fn bevel(positions: &[Vec3], top: &[[u32; 3]], width: f32) -> Vec<f32> {
    let mut edges: HashMap<(u32, u32), ()> = HashMap::new();
    for t in top {
        for k in 0..3 {
            edges.insert((t[k], t[(k + 1) % 3]), ());
        }
    }
    let border: Vec<(Vec2, Vec2)> = edges
        .keys()
        .filter(|(a, b)| !edges.contains_key(&(*b, *a)))
        .map(|&(a, b)| {
            (
                positions[a as usize].truncate(),
                positions[b as usize].truncate(),
            )
        })
        .collect();
    // Border edges binned by `width`, so each vertex only checks the edges near it.
    let cell = |p: Vec2| ((p.x / width).floor() as i32, (p.y / width).floor() as i32);
    let mut near: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    for (i, &(a, b)) in border.iter().enumerate() {
        let (lo, hi) = (cell(a.min(b)), cell(a.max(b)));
        for y in lo.1..=hi.1 {
            for x in lo.0..=hi.0 {
                near.entry((x, y)).or_default().push(i);
            }
        }
    }
    positions
        .iter()
        .map(|p| {
            let p = p.truncate();
            let (cx, cy) = cell(p);
            let d = (cx - 1..=cx + 1)
                .flat_map(|x| (cy - 1..=cy + 1).map(move |y| (x, y)))
                .filter_map(|c| near.get(&c))
                .flatten()
                .map(|&i| {
                    let (a, b) = border[i];
                    let ab = b - a;
                    let t = ((p - a).dot(ab) / ab.length_squared().max(1e-12)).clamp(0.0, 1.0);
                    p.distance(a + ab * t)
                })
                .fold(width, f32::min);
            BEVEL_EDGE + (1.0 - BEVEL_EDGE) * (d / width).min(1.0)
        })
        .collect()
}

/// Where rays start and which way they go, for a projection around `target`.
struct Frame {
    kind: Projection,
    center: Vec3,
    radius: f32,
    top: f32,
}

impl Frame {
    fn new(target: &Mesh, kind: Projection) -> Self {
        let b = target.aabb();
        let center = (b.min + b.max) * 0.5;
        let radius = target
            .positions
            .iter()
            .map(|p| match kind {
                Projection::Cylindrical => (*p - center).truncate().length(),
                _ => p.distance(center),
            })
            .fold(1e-3, f32::max);
        Self {
            kind,
            center,
            radius,
            top: b.max.z,
        }
    }

    /// The outward direction at pattern-plane point `q`.
    fn normal(&self, q: Vec2) -> Vec3 {
        let az = -FRAC_PI_2 + q.x / self.radius;
        match self.kind {
            Projection::Planar => Vec3::Z,
            Projection::Cylindrical => Vec3::new(az.cos(), az.sin(), 0.0),
            Projection::Spherical => {
                let el = (q.y / self.radius).clamp(-FRAC_PI_2, FRAC_PI_2);
                Vec3::new(el.cos() * az.cos(), el.cos() * az.sin(), el.sin())
            }
        }
    }

    /// The ray for pattern-plane point `q`: from outside the object, pointing in.
    fn ray(&self, q: Vec2) -> (Vec3, Vec3) {
        let n = self.normal(q);
        let origin = match self.kind {
            Projection::Planar => (self.center.truncate() + q).extend(self.top + 1.0),
            Projection::Cylindrical => {
                self.center + Vec3::new(0.0, 0.0, q.y) + n * (self.radius + 1.0)
            }
            Projection::Spherical => self.center + n * (self.radius + 1.0),
        };
        (origin, -n)
    }

    /// The pattern-plane point whose ray passes through `p`.
    fn key(&self, p: Vec3) -> Vec2 {
        let d = p - self.center;
        // Azimuth from the front, in (−π, π].
        let az = || {
            let a = d.y.atan2(d.x) + FRAC_PI_2;
            if a > PI {
                a - TAU
            } else if a <= -PI {
                a + TAU
            } else {
                a
            }
        };
        match self.kind {
            Projection::Planar => d.truncate(),
            Projection::Cylindrical => Vec2::new(az() * self.radius, d.z),
            Projection::Spherical => {
                let el = (d.z / d.length().max(1e-9)).clamp(-1.0, 1.0).asin();
                Vec2::new(az() * self.radius, el * self.radius)
            }
        }
    }
}

/// First surface point along each pattern ray, with the triangles binned by where their
/// corners fall in the pattern plane. Only triangles near the pattern's area (`lo`..`hi`
/// in the pattern plane) are kept.
struct Surface<'a> {
    mesh: &'a Mesh,
    frame: Frame,
    cell: f32,
    bins: HashMap<(i32, i32), Vec<u32>>,
}

impl<'a> Surface<'a> {
    fn new(mesh: &'a Mesh, kind: Projection, step: f32, lo: Vec2, hi: Vec2) -> Self {
        let (area_lo, area_hi) = (lo, hi);
        let mid = (lo.x + hi.x) * 0.5;
        let frame = Frame::new(mesh, kind);
        let cell = step.max(0.5);
        let mut bins: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
        for (i, t) in mesh.triangles.iter().enumerate() {
            let p = t.map(|v| mesh.positions[v as usize]);
            let mut k = p.map(|p| frame.key(p));
            if kind != Projection::Planar {
                // Keep the seam opposite the pattern: azimuths within half a turn of it.
                let turn = TAU * frame.radius;
                for k in &mut k {
                    k.x = mid + (k.x - mid + turn / 2.0).rem_euclid(turn) - turn / 2.0;
                }
                // A corner on the axis (a pole, a cap's centre) has no azimuth of its
                // own; it takes its neighbours'.
                let on_axis =
                    p.map(|p| (p - frame.center).truncate().length() < 1e-4 * frame.radius);
                let others: Vec<f32> = (0..3).filter(|&j| !on_axis[j]).map(|j| k[j].x).collect();
                if !others.is_empty() {
                    let avg = others.iter().sum::<f32>() / others.len() as f32;
                    for j in 0..3 {
                        if on_axis[j] {
                            k[j].x = avg;
                        }
                    }
                }
            }
            let (lo, hi) = (k[0].min(k[1]).min(k[2]), k[0].max(k[1]).max(k[2]));
            // A triangle across the seam behind the pattern would cover every column.
            if hi.x - lo.x > PI * frame.radius {
                continue;
            }
            // shortcut: on curved projections a triangle's middle can map a little
            // outside its corners' box, so the box is padded; huge triangles that bulge
            // more could be missed.
            let pad = (hi - lo) * 0.25 + Vec2::splat(cell);
            let (lo, hi) = (lo - pad, hi + pad);
            if hi.x < area_lo.x || hi.y < area_lo.y || lo.x > area_hi.x || lo.y > area_hi.y {
                continue;
            }
            for y in (lo.y / cell).floor() as i32..=(hi.y / cell).floor() as i32 {
                for x in (lo.x / cell).floor() as i32..=(hi.x / cell).floor() as i32 {
                    bins.entry((x, y)).or_default().push(i as u32);
                }
            }
        }
        Self {
            mesh,
            frame,
            cell,
            bins,
        }
    }

    fn hit(&self, q: Vec2) -> Option<(Vec3, Vec3)> {
        let (origin, dir) = self.frame.ray(q);
        let bin = (
            (q.x / self.cell).floor() as i32,
            (q.y / self.cell).floor() as i32,
        );
        let t = self
            .bins
            .get(&bin)?
            .iter()
            .filter_map(|&i| {
                let [a, b, c] =
                    self.mesh.triangles[i as usize].map(|v| self.mesh.positions[v as usize]);
                ray_triangle(origin, dir, a, b, c)
            })
            .fold(None, |m: Option<f32>, t| Some(m.map_or(t, |m| m.min(t))))?;
        Some((origin + dir * t, dir))
    }
}

/// Splits triangles along their longest edge until no edge is longer than `step`. A
/// split edge is split in every triangle that has it, so the surface stays closed.
fn refine(positions: &mut Vec<Vec3>, mut tris: Vec<[u32; 3]>, step: f32) -> Vec<[u32; 3]> {
    for _ in 0..24 {
        let mut mids: HashMap<(u32, u32), u32> = HashMap::new();
        for t in &tris {
            let (a, b) = (0..3)
                .map(|k| (t[k], t[(k + 1) % 3]))
                .max_by(|x, y| {
                    let len = |(a, b): &(u32, u32)| {
                        positions[*a as usize].distance(positions[*b as usize])
                    };
                    len(x).total_cmp(&len(y))
                })
                .unwrap();
            if positions[a as usize].distance(positions[b as usize]) > step {
                mids.entry((a.min(b), a.max(b))).or_insert_with(|| {
                    positions.push((positions[a as usize] + positions[b as usize]) * 0.5);
                    positions.len() as u32 - 1
                });
            }
        }
        if mids.is_empty() {
            break;
        }
        let mut out = Vec::with_capacity(tris.len() * 2);
        let mut stack = tris;
        while let Some(t) = stack.pop() {
            let split = (0..3).find_map(|k| {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                mids.get(&(a.min(b), a.max(b))).map(|&m| (k, m))
            });
            match split {
                Some((k, m)) => {
                    let (a, b, c) = (t[k], t[(k + 1) % 3], t[(k + 2) % 3]);
                    stack.push([a, m, c]);
                    stack.push([m, b, c]);
                }
                None => out.push(t),
            }
        }
        tris = out;
    }
    tris
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::Primitive;
    use glam::Vec3;

    #[test]
    fn placed_offset_cache_matches_world_mesh_projection() {
        let target = std::sync::Arc::new(Primitive::Cube.build());
        for projection in [
            Projection::Planar,
            Projection::Cylindrical,
            Projection::Spherical,
        ] {
            for translation in [Vec3::ZERO, Vec3::new(10.0, -20.0, 30.0)] {
                let matrix = glam::Mat4::from_translation(translation);
                let world = target.transformed(&matrix);
                for p in [Vec3::X, Vec3::Y, Vec3::Z] {
                    let p = p + translation;
                    assert!(offset_at_placed(&target, matrix, projection, p)
                        .abs_diff_eq(offset_at(&world, projection, p), 1e-5));
                }
            }
        }
    }

    #[test]
    fn text_on_a_cube_is_a_closed_solid_on_its_top() {
        let cube = Primitive::Cube.build();
        let e = Emboss::default();
        let t = text_solid(&cube, &e).unwrap();
        assert!(t.stats().is_closed_manifold(), "{:?}", t.stats());
        let b = t.aabb();
        assert!((b.max.z - 41.0).abs() < 1e-3 && (b.min.z - 39.5).abs() < 1e-3);
        // Cap height 10 mm: "Text" is about that tall.
        assert!((b.max.y - b.min.y - 10.0).abs() < 1.0, "{b:?}");
    }

    #[test]
    fn letters_with_holes_keep_them() {
        // "o" has a hole: its top face area is the ring, not the disc.
        let e = Emboss {
            text: "o".into(),
            ..Emboss::default()
        };
        let t = text_solid(&Primitive::Cube.build(), &e).unwrap();
        assert!(t.stats().is_closed_manifold());
        let ring = t.stats().volume / 1.5;
        let b = t.aabb();
        let disc = std::f64::consts::FRAC_PI_4 * ((b.max.x - b.min.x) * (b.max.y - b.min.y)) as f64;
        assert!(ring < disc * 0.8, "{ring} vs {disc}");
    }

    #[test]
    fn emboss_and_engrave_change_the_volume() {
        let cube = Primitive::Cube.build();
        let v = cube.stats().volume;
        let raised = apply(&cube, &Emboss::default()).unwrap().object;
        let cut = apply(
            &cube,
            &Emboss {
                engrave: true,
                ..Emboss::default()
            },
        )
        .unwrap()
        .object;
        assert!(raised.stats().is_closed_manifold() && cut.stats().is_closed_manifold());
        assert!(raised.stats().volume > v && cut.stats().volume < v);
        assert!((raised.aabb().max.z - 41.0).abs() < 1e-3);
    }

    #[test]
    fn text_follows_a_curved_surface() {
        let sphere = Primitive::Sphere.build();
        let t = text_solid(
            &sphere,
            &Emboss {
                text: "I".into(),
                size: 6.0,
                ..Emboss::default()
            },
        )
        .unwrap();
        assert!(t.stats().is_closed_manifold());
        // The top of the sphere is at 40; the stroke sits 1 mm above the surface there.
        assert!((t.aabb().max.z - 41.0).abs() < 0.05, "{:?}", t.aabb());
    }

    #[test]
    fn touching_letters_join_into_a_closed_result() {
        // "fj" and "ff" glyphs reach into their neighbours.
        let e = Emboss {
            text: "fjff".into(),
            ..Emboss::default()
        };
        let out = apply(&Primitive::Cube.build(), &e).unwrap().object;
        assert!(out.stats().is_closed_manifold(), "{:?}", out.stats());
    }

    fn closed(m: &Mesh) {
        assert!(m.stats().is_closed_manifold(), "{:?}", m.stats());
    }

    #[test]
    fn cylindrical_text_wraps_the_front_of_a_cylinder() {
        // Radius 20, centred on the Z axis, 40 tall: the front is at y = −20.
        let e = Emboss {
            projection: Projection::Cylindrical,
            ..Emboss::default()
        };
        let t = text_solid(&Primitive::Cylinder.build(), &e).unwrap();
        closed(&t);
        let b = t.aabb();
        // 1 mm proud of the front; the polygonal cylinder is a little inside radius 20.
        assert!((b.min.y + 21.0).abs() < 0.1, "{b:?}");
        assert!((b.max.z + b.min.z) / 2.0 - 20.0 < 0.5);
        let out = apply(&Primitive::Cylinder.build(), &e).unwrap().object;
        closed(&out);
    }

    #[test]
    fn spherical_text_sits_on_the_front_of_a_sphere() {
        let e = Emboss {
            projection: Projection::Spherical,
            ..Emboss::default()
        };
        let t = text_solid(&Primitive::Sphere.build(), &e).unwrap();
        closed(&t);
        // Every top vertex is 1 mm outside the sphere (radius 20 about z = 20).
        let c = Vec3::new(0.0, 0.0, 20.0);
        let far = t
            .positions
            .iter()
            .map(|p| p.distance(c))
            .fold(0.0, f32::max);
        assert!((far - 21.0).abs() < 0.1, "{far}");
    }

    #[test]
    fn patterns_reach_the_back_and_the_poles() {
        // Half a turn along a radius-20 cylinder is the back (+Y).
        let back = Emboss {
            projection: Projection::Cylindrical,
            offset: Vec2::new(PI * 20.0, 0.0),
            ..Emboss::default()
        };
        let t = text_solid(&Primitive::Cylinder.build(), &back).unwrap();
        closed(&t);
        assert!(
            (t.aabb().max.y - 21.0).abs() < 0.05 && t.aabb().min.y > 10.0,
            "{:?}",
            t.aabb()
        );
        // Close to the top of a sphere, where triangles meet at the pole.
        let top = Emboss {
            projection: Projection::Spherical,
            offset: Vec2::new(0.0, 25.0),
            size: 5.0,
            ..Emboss::default()
        };
        closed(&text_solid(&Primitive::Sphere.build(), &top).unwrap());
    }

    #[test]
    fn bevel_lowers_the_edges() {
        let cube = Primitive::Cube.build();
        let flat = text_solid(&cube, &Emboss::default()).unwrap();
        let bevelled = text_solid(
            &cube,
            &Emboss {
                bevel: true,
                ..Emboss::default()
            },
        )
        .unwrap();
        closed(&bevelled);
        assert!(bevelled.stats().volume < flat.stats().volume * 0.95);
        assert!(
            (bevelled.aabb().max.z - 41.0).abs() < 0.05,
            "{:?}",
            bevelled.aabb()
        );
    }

    #[test]
    fn shapes_are_sized_by_height() {
        for s in Shape::ALL {
            let e = Emboss {
                pattern: Pattern::Shape(s),
                size: 20.0,
                ..Emboss::default()
            };
            let t = text_solid(&Primitive::Cube.build(), &e).unwrap();
            closed(&t);
            let b = t.aabb();
            assert!((b.max.y - b.min.y - 20.0).abs() < 0.01, "{s:?} {b:?}");
        }
    }

    #[test]
    fn image_is_traced_with_its_holes() {
        // A dark 40×40 square ring (inner 20×20 hole) on a white 60×60 image.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ring.png");
        let img = image::GrayImage::from_fn(60, 60, |x, y| {
            let inside = |a: u32, b: u32| (a..b).contains(&x) && (a..b).contains(&y);
            image::Luma([if inside(10, 50) && !inside(20, 40) {
                0
            } else {
                255
            }])
        });
        img.save(&path).unwrap();
        let e = Emboss {
            pattern: Pattern::Image(path),
            size: 20.0,
            ..Emboss::default()
        };
        let t = text_solid(&Primitive::Cube.build(), &e).unwrap();
        closed(&t);
        // Ring area is 3/4 of its square: 20² · 0.75 = 300 mm², 1.5 mm thick.
        let v = t.stats().volume;
        assert!((v - 450.0).abs() < 25.0, "{v}");
    }

    #[test]
    fn colours_make_separate_parts() {
        let cube = Primitive::Cube.build();
        let red = [255, 0, 0, 255];
        let blue = [0, 0, 255, 255];
        let both = Emboss {
            top_color: Some(red),
            side_color: Some(blue),
            ..Emboss::default()
        };
        let out = apply(&cube, &both).unwrap();
        assert_eq!(out.parts.len(), 2);
        assert!((out.object.stats().volume - 64000.0).abs() < 1.0);
        for (m, _) in &out.parts {
            closed(m);
        }
        // The sides are trimmed at the surface, so nothing overlaps the object.
        let sides = &out.parts[0].0;
        assert!(sides.aabb().min.z >= 40.0 - 1e-3);
        let top_only = Emboss {
            top_color: Some(red),
            ..Emboss::default()
        };
        let out = apply(&cube, &top_only).unwrap();
        assert_eq!(out.parts.len(), 1);
        closed(&out.object);
        assert!(out.object.stats().volume > 64000.0);
    }

    #[test]
    fn text_off_the_object_is_refused() {
        let e = Emboss {
            offset: Vec2::new(100.0, 0.0),
            ..Emboss::default()
        };
        assert!(text_solid(&Primitive::Cube.build(), &e).is_err());
        let blank = Emboss {
            text: "  ".into(),
            ..Emboss::default()
        };
        assert!(text_solid(&Primitive::Cube.build(), &blank).is_err());
    }
}
