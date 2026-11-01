//! The flat outlines of an emboss pattern: glyphs, built-in shapes, and traced images.

use super::{fonts, Emboss, Pattern, Shape};
use anyhow::{anyhow, bail, Result};
use glam::Vec2;
use std::collections::HashMap;
use std::f32::consts::{FRAC_PI_2, TAU};
use std::path::PathBuf;

/// The pattern as closed outlines, one list of contours per piece, centred on the origin
/// and scaled to `size`.
pub(super) fn pattern_outlines(e: &Emboss) -> Result<Vec<Vec<Vec<Vec2>>>> {
    let mut pieces = match &e.pattern {
        Pattern::Text => text_outlines(&e.text, e.font.as_deref(), e.size)?,
        Pattern::Shape(s) => vec![vec![shape_outline(*s)]],
        Pattern::Image(path) => vec![image_outlines(path)?],
    };
    let (lo, hi) = pieces
        .iter()
        .flatten()
        .flatten()
        .fold((Vec2::MAX, Vec2::MIN), |(lo, hi), p| {
            (lo.min(*p), hi.max(*p))
        });
    let mid = (lo + hi) * 0.5;
    // Text is already sized by its capital height.
    let scale = match e.pattern {
        Pattern::Text => 1.0,
        _ => e.size / (hi.y - lo.y).max(1e-6),
    };
    for p in pieces.iter_mut().flatten().flatten() {
        *p = (*p - mid) * scale;
    }
    pieces.retain(|p| !p.is_empty());
    Ok(pieces)
}

/// Glyph outlines in mm, one piece per glyph, lines stacked downwards. Characters the
/// font lacks are skipped.
fn text_outlines(text: &str, font: Option<&str>, size: f32) -> Result<Vec<Vec<Vec<Vec2>>>> {
    let layout = |face: &ttf_parser::Face| {
        let cap = face
            .capital_height()
            .filter(|&h| h > 0)
            .map_or(face.units_per_em() as f32 * 0.7, |h| h as f32);
        let scale = size / cap;
        let line = face.height() as f32 * scale;
        let mut pen = Pen::default();
        let mut glyphs = Vec::new();
        for (row, text_line) in text.lines().enumerate() {
            let mut x = 0.0;
            for ch in text_line.chars() {
                let Some(id) = face.glyph_index(ch) else {
                    continue;
                };
                pen.origin = Vec2::new(x, -(row as f32) * line);
                pen.scale = scale;
                face.outline_glyph(id, &mut pen);
                if !pen.contours.is_empty() {
                    glyphs.push(std::mem::take(&mut pen.contours));
                }
                x += face.glyph_hor_advance(id).unwrap_or(0) as f32 * scale;
            }
        }
        glyphs
    };
    match font {
        None => {
            let face = ttf_parser::Face::parse(epaint_default_fonts::UBUNTU_LIGHT, 0)?;
            Ok(layout(&face))
        }
        Some(family) => {
            let db = fonts();
            let id = db
                .query(&fontdb::Query {
                    families: &[fontdb::Family::Name(family)],
                    ..fontdb::Query::default()
                })
                .ok_or_else(|| anyhow!("Font not found: {family}"))?;
            db.with_face_data(id, |data, index| {
                ttf_parser::Face::parse(data, index).map(|f| layout(&f))
            })
            .ok_or_else(|| anyhow!("Could not read the font {family}"))?
            .map_err(|err| anyhow!("Could not read the font {family}: {err}"))
        }
    }
}

fn shape_outline(shape: Shape) -> Vec<Vec2> {
    let ring = |n: usize, f: &dyn Fn(f32) -> Vec2| -> Vec<Vec2> {
        (0..n).map(|i| f(i as f32 / n as f32 * TAU)).collect()
    };
    match shape {
        Shape::Circle => ring(96, &|t| Vec2::new(t.cos(), t.sin())),
        Shape::Star => ring(10, &|t| {
            let r = if (t / (TAU / 10.0)).round() as usize % 2 == 0 {
                1.0
            } else {
                0.4
            };
            Vec2::new((t + FRAC_PI_2).cos(), (t + FRAC_PI_2).sin()) * r
        }),
        // The classic heart curve.
        Shape::Heart => ring(96, &|t| {
            Vec2::new(
                16.0 * t.sin().powi(3),
                13.0 * t.cos() - 5.0 * (2.0 * t).cos() - 2.0 * (3.0 * t).cos() - (4.0 * t).cos(),
            )
        }),
    }
}

/// An edge point in half pixels.
type Key = (i32, i32);

/// Outlines of the dark, opaque pixels (marching squares), in pixels with y up.
fn image_outlines(path: &std::path::Path) -> Result<Vec<Vec<Vec2>>> {
    // The preview is rebuilt on every drag frame; keep the last image's outlines.
    type Cached = (PathBuf, Option<std::time::SystemTime>, Vec<Vec<Vec2>>);
    static LAST: std::sync::Mutex<Option<Cached>> = std::sync::Mutex::new(None);
    let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok();
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((p, m, loops)) = &*last {
        if p == path && *m == modified {
            return Ok(loops.clone());
        }
    }
    let loops = trace_image(path)?;
    *last = Some((path.to_path_buf(), modified, loops.clone()));
    Ok(loops)
}

fn trace_image(path: &std::path::Path) -> Result<Vec<Vec<Vec2>>> {
    // shortcut: images are traced at up to 256 px on the long side; finer detail is lost
    // until the outlines are simplified instead.
    const MAX: u32 = 256;
    let img = image::open(path)
        .map_err(|err| anyhow!("Could not open the image {}: {err}", path.display()))?;
    let img = if img.width().max(img.height()) > MAX {
        img.resize(MAX, MAX, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let img = img.to_luma_alpha8();
    let (w, h) = (img.width() as i32, img.height() as i32);
    let on = |x: i32, y: i32| {
        x >= 0 && y >= 0 && x < w && y < h && {
            let p = img.get_pixel(x as u32, y as u32);
            p[1] > 127 && p[0] < 128
        }
    };
    // Cell (x, y) has corners at pixels (x-1..=x, y-1..=y), so the border is all off.
    // Edge points are keyed in half pixels: (2x+1, 2y) is between (x, y) and (x+1, y).
    let mut next: HashMap<(i32, i32), (i32, i32)> = HashMap::new();
    for y in 0..=h {
        for x in 0..=w {
            let (a, b, c, d) = (on(x - 1, y - 1), on(x, y - 1), on(x, y), on(x - 1, y));
            let top = (2 * x - 1, 2 * y - 2);
            let right = (2 * x, 2 * y - 1);
            let bottom = (2 * x - 1, 2 * y);
            let left = (2 * x - 2, 2 * y - 1);
            // Each segment keeps "on" pixels on the same side, so loops chain in order.
            let segs: &[(Key, Key)] = match (a, b, c, d) {
                (false, false, false, false) | (true, true, true, true) => &[],
                (true, false, false, false) => &[(left, top)],
                (false, true, false, false) => &[(top, right)],
                (false, false, true, false) => &[(right, bottom)],
                (false, false, false, true) => &[(bottom, left)],
                (true, true, false, false) => &[(left, right)],
                (false, true, true, false) => &[(top, bottom)],
                (false, false, true, true) => &[(right, left)],
                (true, false, false, true) => &[(bottom, top)],
                (false, true, true, true) => &[(top, left)],
                (true, false, true, true) => &[(right, top)],
                (true, true, false, true) => &[(bottom, right)],
                (true, true, true, false) => &[(left, bottom)],
                // Diagonal pairs stay apart, so no two loops touch.
                (true, false, true, false) => &[(left, top), (right, bottom)],
                (false, true, false, true) => &[(top, right), (bottom, left)],
            };
            for &(p, q) in segs {
                next.insert(p, q);
            }
        }
    }
    let mut loops = Vec::new();
    while let Some(&start) = next.keys().next() {
        let mut ring = Vec::new();
        let mut cur = start;
        while let Some(n) = next.remove(&cur) {
            ring.push(Vec2::new(cur.0 as f32 * 0.5, -(cur.1 as f32) * 0.5));
            cur = n;
            if cur == start {
                break;
            }
        }
        if ring.len() >= 3 {
            loops.push(ring);
        }
    }
    if loops.is_empty() {
        bail!("The image has no dark areas to emboss");
    }
    Ok(loops)
}

/// Collects glyph outlines as polylines, curves flattened.
#[derive(Default)]
struct Pen {
    contours: Vec<Vec<Vec2>>,
    current: Vec<Vec2>,
    origin: Vec2,
    scale: f32,
}

impl Pen {
    fn point(&self, x: f32, y: f32) -> Vec2 {
        self.origin + Vec2::new(x, y) * self.scale
    }
    fn push(&mut self, p: Vec2) {
        if self.current.last() != Some(&p) {
            self.current.push(p);
        }
    }
}

impl ttf_parser::OutlineBuilder for Pen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.current.clear();
        let p = self.point(x, y);
        self.push(p);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.point(x, y);
        self.push(p);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (a, c, b) = (
            *self.current.last().unwrap(),
            self.point(x1, y1),
            self.point(x, y),
        );
        for k in 1..=6 {
            let t = k as f32 / 6.0;
            self.push(a.lerp(c, t).lerp(c.lerp(b, t), t));
        }
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let a = *self.current.last().unwrap();
        let (c1, c2, b) = (self.point(x1, y1), self.point(x2, y2), self.point(x, y));
        for k in 1..=8 {
            let t = k as f32 / 8.0;
            let u = 1.0 - t;
            self.push(a * u * u * u + c1 * 3.0 * u * u * t + c2 * 3.0 * u * t * t + b * t * t * t);
        }
    }
    fn close(&mut self) {
        let mut c = std::mem::take(&mut self.current);
        if c.len() > 1 && c.first() == c.last() {
            c.pop();
        }
        if c.len() >= 3 {
            self.contours.push(c);
        }
    }
}
