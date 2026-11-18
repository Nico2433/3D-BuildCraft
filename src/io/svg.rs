//! SVG import, as Tinkercad does it: filled areas are extruded, stroked paths are extruded
//! as the band their stroke covers. One object per paint colour; where shapes overlap, the
//! one drawn later wins, so the objects do not overlap. usvg resolves transforms, shapes,
//! styles and units; text must be converted to paths first. The same outlines also draw
//! the SVG as a picture ([`rasterize`]) for the tools that take one.

use super::ModelObject;
use crate::outline::{self, TOLERANCE};
use crate::scene::DEFAULT_COLOR;
use anyhow::{bail, Context, Result};
use clipper2_rust::{
    inflate_paths_d, union_subjects_d, EndType, FillRule, JoinType, PathsD, Point,
};
use glam::Mat4;
use manifold_rust::cross_section::CrossSection;
use std::path::Path;
use usvg::tiny_skia_path::{PathSegment, Point as SvgPoint, Transform};

/// How tall the drawing is extruded, mm.
pub const HEIGHT: f32 = 3.0;
/// SVG user units are CSS pixels, 96 to the inch; usvg turns mm, cm and in into them.
const MM_PER_PX: f64 = 25.4 / 96.0;
/// Decimal places clipper2 keeps (in mm).
const PRECISION: i32 = 6;

pub fn read(path: &Path) -> Result<Vec<ModelObject>> {
    let (data, opt) = load(path)?;
    parse(&data, &opt, super::file_stem(path))
}

/// The file's bytes, with options that find the files it links to beside it.
fn load(path: &Path) -> Result<(Vec<u8>, usvg::Options<'static>)> {
    let data = std::fs::read(path).with_context(|| format!("Could not read {}", path.display()))?;
    let opt = usvg::Options {
        resources_dir: path.parent().map(Path::to_path_buf),
        ..Default::default()
    };
    Ok((data, opt))
}

fn tree(data: &[u8], opt: &usvg::Options) -> Result<usvg::Tree> {
    usvg::Tree::from_data(data, opt).context("Not a valid SVG file")
}

/// The drawing's layers, `unit` output units per SVG pixel, y flipped up.
fn layers(tree: &usvg::Tree, unit: f64) -> Result<Layers> {
    let mut layers = Vec::new();
    collect(tree.root(), &mut layers, unit);
    layers.retain(|(_, area)| !area.is_empty());
    if layers.is_empty() {
        bail!("The SVG has no filled or stroked shapes (convert text to paths first)");
    }
    Ok(layers)
}

/// The drawing's solids, named after `name`.
pub fn parse(data: &[u8], opt: &usvg::Options, name: &str) -> Result<Vec<ModelObject>> {
    let layers = layers(&tree(data, opt)?, MM_PER_PX)?;
    let many = layers.len() > 1;
    Ok(layers
        .into_iter()
        .enumerate()
        .map(|(i, (color, area))| ModelObject {
            name: if many {
                format!("{name} {}", i + 1)
            } else {
                name.to_string()
            },
            mesh: outline::extrude(&area, 0.0, HEIGHT),
            transform: Mat4::IDENTITY,
            color: color.unwrap_or(DEFAULT_COLOR),
        })
        .collect())
}

/// The area each colour covers, in drawing order; None is a gradient or pattern.
type Layers = Vec<(Option<super::Rgba>, CrossSection)>;

fn collect(group: &usvg::Group, layers: &mut Layers, unit: f64) {
    for node in group.children() {
        match node {
            usvg::Node::Group(g) => collect(g, layers, unit),
            usvg::Node::Path(p) if p.is_visible() => {
                let ts = p.abs_transform();
                let contours = flatten(p.data(), ts, unit);
                let fill = p
                    .fill()
                    .map(|f| (paint_color(f.paint()), fill_area(&contours, f.rule())));
                let stroke = p
                    .stroke()
                    .map(|s| (paint_color(s.paint()), stroke_area(&contours, s, ts, unit)));
                let painted = match p.paint_order() {
                    usvg::PaintOrder::FillAndStroke => [fill, stroke],
                    usvg::PaintOrder::StrokeAndFill => [stroke, fill],
                };
                for (color, area) in painted.into_iter().flatten() {
                    paint(layers, color, area);
                }
            }
            _ => {}
        }
    }
}

/// Lays `area` over what is drawn so far: other colours lose what it covers.
fn paint(layers: &mut Layers, color: Option<super::Rgba>, area: CrossSection) {
    if area.is_empty() {
        return;
    }
    for (_, a) in layers.iter_mut().filter(|(c, _)| *c != color) {
        *a = a.difference(&area);
    }
    match layers.iter_mut().find(|(c, _)| *c == color) {
        Some((_, a)) => *a = a.union(&area),
        None => layers.push((color, area)),
    }
}

fn paint_color(p: &usvg::Paint) -> Option<super::Rgba> {
    match p {
        usvg::Paint::Color(c) => Some([c.red, c.green, c.blue, 255]),
        _ => None,
    }
}

/// One subpath in output units (mm for a model), y up, and whether it was closed.
struct Contour {
    points: Vec<[f64; 2]>,
    closed: bool,
}

/// The path's subpaths as polylines, `unit` per SVG pixel, curves flattened within
/// [`TOLERANCE`]. The drawing's y goes down; the floor's goes up, so it is flipped to read
/// the same from above.
fn flatten(path: &usvg::tiny_skia_path::Path, ts: Transform, unit: f64) -> Vec<Contour> {
    let mm = |p: SvgPoint| {
        let mut p = p;
        ts.map_point(&mut p);
        [p.x as f64 * unit, -(p.y as f64) * unit]
    };
    let mut out: Vec<Contour> = Vec::new();
    let mut current: Vec<[f64; 2]> = Vec::new();
    let mut finish = |current: &mut Vec<[f64; 2]>, closed: bool| {
        if closed && current.len() > 1 && current.first() == current.last() {
            current.pop();
        }
        if current.len() > 1 {
            out.push(Contour {
                points: std::mem::take(current),
                closed,
            });
        }
        current.clear();
    };
    for seg in path.segments() {
        let last = current.last().copied().unwrap_or([0.0; 2]);
        match seg {
            PathSegment::MoveTo(p) => {
                finish(&mut current, false);
                current.push(mm(p));
            }
            PathSegment::LineTo(p) => current.push(mm(p)),
            PathSegment::QuadTo(a, b) => curve(&[last, mm(a), mm(b)], &mut current),
            PathSegment::CubicTo(a, b, c) => curve(&[last, mm(a), mm(b), mm(c)], &mut current),
            PathSegment::Close => {
                let start = current.first().copied();
                finish(&mut current, true);
                // A path may go on from where the closed one started.
                current.extend(start);
            }
        }
    }
    finish(&mut current, false);
    out
}

/// Appends a quadratic or cubic Bézier (control points `c`, the first already in `out`)
/// as enough straight pieces to stay within tolerance (Wang's formula).
fn curve(c: &[[f64; 2]], out: &mut Vec<[f64; 2]>) {
    let degree = (c.len() - 1) as f64;
    let second = (0..c.len() - 2)
        .map(|i| {
            let d = |k: usize| c[i][k] - 2.0 * c[i + 1][k] + c[i + 2][k];
            d(0).hypot(d(1))
        })
        .fold(0.0, f64::max);
    let n = (degree * (degree - 1.0) / 8.0 * second / TOLERANCE)
        .sqrt()
        .ceil()
        .clamp(1.0, 1000.0) as usize;
    for i in 1..=n {
        let t = i as f64 / n as f64;
        // De Casteljau.
        let mut p = c.to_vec();
        while p.len() > 1 {
            p = p
                .windows(2)
                .map(|w| {
                    [
                        w[0][0] + (w[1][0] - w[0][0]) * t,
                        w[0][1] + (w[1][1] - w[0][1]) * t,
                    ]
                })
                .collect();
        }
        out.push(p[0]);
    }
}

fn to_paths<'a>(contours: impl Iterator<Item = &'a Contour>) -> PathsD {
    contours
        .map(|c| c.points.iter().map(|p| Point::new(p[0], p[1])).collect())
        .collect()
}

fn to_section(paths: &PathsD) -> CrossSection {
    use manifold_rust::linalg::Vec2;
    CrossSection::new(
        paths
            .iter()
            .map(|p| p.iter().map(|q| Vec2::new(q.x, q.y)).collect())
            .collect(),
    )
}

/// The area a fill covers; open subpaths fill as if closed.
fn fill_area(contours: &[Contour], rule: usvg::FillRule) -> CrossSection {
    let rule = match rule {
        usvg::FillRule::NonZero => FillRule::NonZero,
        usvg::FillRule::EvenOdd => FillRule::EvenOdd,
    };
    let paths = to_paths(contours.iter().filter(|c| c.points.len() > 2));
    to_section(&union_subjects_d(&paths, rule, PRECISION))
}

/// The band a stroke covers, with its joins and caps; dashes are drawn solid.
fn stroke_area(contours: &[Contour], s: &usvg::Stroke, ts: Transform, unit: f64) -> CrossSection {
    // A transform that scales unevenly gets its average scale.
    let scale = ((ts.sx * ts.sy - ts.kx * ts.ky) as f64).abs().sqrt();
    let half = s.width().get() as f64 * scale * unit / 2.0;
    let join = match s.linejoin() {
        usvg::LineJoin::Miter | usvg::LineJoin::MiterClip => JoinType::Miter,
        usvg::LineJoin::Round => JoinType::Round,
        usvg::LineJoin::Bevel => JoinType::Bevel,
    };
    let cap = match s.linecap() {
        usvg::LineCap::Butt => EndType::Butt,
        usvg::LineCap::Round => EndType::Round,
        usvg::LineCap::Square => EndType::Square,
    };
    let mut bands = PathsD::new();
    for (closed, end) in [(true, EndType::Joined), (false, cap)] {
        let paths = to_paths(contours.iter().filter(|c| c.closed == closed));
        if !paths.is_empty() && half > 0.0 {
            bands.extend(inflate_paths_d(
                &paths,
                half,
                join,
                end,
                s.miterlimit().get() as f64,
                PRECISION,
                TOLERANCE,
            ));
        }
    }
    to_section(&union_subjects_d(&bands, FillRule::NonZero, PRECISION))
}

/// Pixels along the longer side of an SVG drawn as a picture.
pub const RASTER_SIZE: u32 = 1024;
/// Rows sampled per pixel row when drawing, for smooth edges.
const SUBROWS: usize = 4;

/// The SVG file drawn as a picture: see [`rasterize`].
pub fn rasterize_file(path: &Path) -> Result<image::RgbaImage> {
    let (data, opt) = load(path)?;
    rasterize(&data, &opt)
}

/// The drawing on a transparent canvas, [`RASTER_SIZE`] pixels along its longer side,
/// with smooth edges. Each area is filled with its colour (gradients and patterns black);
/// the fill rules and later shapes covering earlier ones are already resolved in the
/// layers, so filling their outlines nonzero is exact.
pub fn rasterize(data: &[u8], opt: &usvg::Options) -> Result<image::RgbaImage> {
    let tree = tree(data, opt)?;
    let size = tree.size();
    let unit = RASTER_SIZE as f64 / size.width().max(size.height()) as f64;
    let layers = layers(&tree, unit)?;
    let w = ((size.width() as f64 * unit).round() as usize).max(1);
    let h = ((size.height() as f64 * unit).round() as usize).max(1);
    // Premultiplied colour and coverage per pixel; the layers do not overlap, so they add.
    let mut sum = vec![[0f32; 4]; w * h];
    let mut cover = vec![0f32; w * h];
    for (color, area) in &layers {
        let [r, g, b, _] = color.unwrap_or([0, 0, 0, 255]);
        cover.iter_mut().for_each(|c| *c = 0.0);
        fill(&area.to_polygons(), w, h, &mut cover);
        for (s, c) in sum.iter_mut().zip(&cover) {
            for (k, v) in [r, g, b, 255].into_iter().enumerate() {
                s[k] += v as f32 * c;
            }
        }
    }
    let pixels = sum.iter().flat_map(|s| {
        let a = (s[3] / 255.0).min(1.0);
        let channel = |k: usize| {
            if a > 0.0 {
                (s[k] / s[3] * 255.0).round()
            } else {
                0.0
            }
        };
        [channel(0), channel(1), channel(2), a * 255.0].map(|v| v.round().clamp(0.0, 255.0) as u8)
    });
    Ok(image::RgbaImage::from_raw(w as u32, h as u32, pixels.collect()).expect("w × h pixels"))
}

/// Adds the coverage (0..1) of `polygons` (x right, y up from the top edge, in pixels) to
/// `cover`, `w`×`h` pixels, filling nonzero by scanlines [`SUBROWS`] to a pixel row, each
/// span covering the columns it crosses in proportion.
fn fill(polygons: &manifold_rust::types::Polygons, w: usize, h: usize, cover: &mut [f32]) {
    // Crossings of each sample row: x and the edge's direction.
    let mut rows: Vec<Vec<(f64, i32)>> = vec![Vec::new(); h * SUBROWS];
    let sample = |k: usize| (k as f64 + 0.5) / SUBROWS as f64;
    for poly in polygons {
        for (i, a) in poly.iter().enumerate() {
            let b = poly[(i + 1) % poly.len()];
            let ((x0, y0), (x1, y1)) = ((a.x, -a.y), (b.x, -b.y));
            if y0 == y1 {
                continue;
            }
            let (dir, top, bottom) = if y0 < y1 { (1, y0, y1) } else { (-1, y1, y0) };
            // Sample rows k with top <= sample(k) < bottom.
            let first = ((top * SUBROWS as f64 - 0.5).ceil().max(0.0)) as usize;
            let mut k = first;
            while k < rows.len() && sample(k) < bottom {
                let y = sample(k);
                rows[k].push((x0 + (x1 - x0) * (y - y0) / (y1 - y0), dir));
                k += 1;
            }
        }
    }
    let weight = 1.0 / SUBROWS as f64;
    for (k, crossings) in rows.iter_mut().enumerate() {
        crossings.sort_by(|a, b| a.0.total_cmp(&b.0));
        let row = &mut cover[k / SUBROWS * w..][..w];
        let mut winding = 0;
        for pair in crossings.windows(2) {
            winding += pair[0].1;
            if winding == 0 {
                continue;
            }
            let (from, to) = (
                pair[0].0.clamp(0.0, w as f64),
                pair[1].0.clamp(0.0, w as f64),
            );
            let mut x = from.floor() as usize;
            while (x as f64) < to && x < w {
                let part = (to.min(x as f64 + 1.0) - from.max(x as f64)).max(0.0);
                row[x] += (part * weight) as f32;
                x += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    fn objects(body: &str, size: &str) -> Vec<ModelObject> {
        let svg = format!(r#"<svg xmlns="http://www.w3.org/2000/svg" {size}>{body}</svg>"#);
        parse(svg.as_bytes(), &usvg::Options::default(), "logo").unwrap()
    }

    /// The one object's footprint area in mm², checking it is a closed solid 3 mm tall.
    fn area(o: &ModelObject) -> f64 {
        let s = o.mesh.stats();
        assert!(s.is_closed_manifold(), "{s:?}");
        let b = o.mesh.aabb();
        assert!(b.min.z.abs() < 1e-5 && (b.max.z - HEIGHT).abs() < 1e-5);
        s.volume / HEIGHT as f64
    }

    const PX: f64 = MM_PER_PX * MM_PER_PX;

    #[test]
    fn a_filled_rect_is_a_plate_at_96_dpi() {
        let o = objects(
            r##"<rect x="10" y="10" width="96" height="48" fill="#ff0000"/>"##,
            r#"width="200" height="100""#,
        );
        assert_eq!(o.len(), 1);
        assert_eq!(o[0].name, "logo");
        assert_eq!(o[0].color, [255, 0, 0, 255]);
        assert!((area(&o[0]) - 96.0 * 48.0 * PX).abs() < 1e-3);
        let b = o[0].mesh.aabb();
        assert!((b.size().x - 25.4).abs() < 1e-3 && (b.size().y - 12.7).abs() < 1e-3);
    }

    #[test]
    fn real_world_units_are_honoured() {
        // 1 in = 25.4 mm; a viewBox maps 10 units onto 2 cm.
        let o = objects(
            r#"<rect width="10" height="10"/>"#,
            r#"width="2cm" height="1in" viewBox="0 0 10 10" preserveAspectRatio="none""#,
        );
        let b = o[0].mesh.aabb();
        assert!((b.size().x - 20.0).abs() < 1e-3, "{b:?}");
        assert!((b.size().y - 25.4).abs() < 1e-3, "{b:?}");
        // No fill colour given: black.
        assert_eq!(o[0].color, [0, 0, 0, 255]);
    }

    #[test]
    fn even_odd_makes_a_hole_and_nonzero_fills_it() {
        let rings = "M 0 50 A 50 50 0 1 0 100 50 A 50 50 0 1 0 0 50 Z \
                     M 25 50 A 25 25 0 1 0 75 50 A 25 25 0 1 0 25 50 Z";
        let disc = PI * 50.0 * 50.0 * PX;
        let hole = PI * 25.0 * 25.0 * PX;
        let even = objects(
            &format!(r#"<path d="{rings}" fill-rule="evenodd"/>"#),
            r#"width="100" height="100""#,
        );
        // Flattened curves lose at most the tolerance along their length.
        let slack = 2.0 * PI * 75.0 * MM_PER_PX * TOLERANCE;
        assert!((area(&even[0]) - (disc - hole)).abs() < slack);
        let nonzero = objects(
            &format!(r#"<path d="{rings}"/>"#),
            r#"width="100" height="100""#,
        );
        assert!((area(&nonzero[0]) - disc).abs() < slack);
    }

    #[test]
    fn two_shapes_of_one_colour_are_one_object() {
        let o = objects(
            r##"<circle cx="50" cy="50" r="40" fill="#00ff00"/>
               <circle cx="150" cy="50" r="40" fill="#00ff00"/>"##,
            r#"width="200" height="100""#,
        );
        assert_eq!(o.len(), 1);
        let slack = 2.0 * 2.0 * PI * 40.0 * MM_PER_PX * TOLERANCE;
        assert!((area(&o[0]) - 2.0 * PI * 1600.0 * PX).abs() < slack);
    }

    #[test]
    fn a_stroke_only_line_becomes_its_band() {
        let o = objects(
            r##"<line x1="0" y1="10" x2="96" y2="10" stroke="#0000ff" stroke-width="9.6"/>"##,
            r#"width="100" height="20""#,
        );
        assert_eq!(o.len(), 1);
        assert_eq!(o[0].color, [0, 0, 255, 255]);
        // Butt caps: 25.4 mm long, 2.54 mm wide.
        assert!((area(&o[0]) - 25.4 * 2.54).abs() < 1e-2, "{}", area(&o[0]));
        // A closed outline stroked: a square frame.
        let o = objects(
            r#"<rect x="10" y="10" width="80" height="80" fill="none" stroke="black" stroke-width="10"/>"#,
            r#"width="100" height="100""#,
        );
        assert!((area(&o[0]) - (90.0 * 90.0 - 70.0 * 70.0) * PX).abs() < 1e-2);
    }

    #[test]
    fn each_colour_is_an_object_and_later_shapes_cover_earlier_ones() {
        let o = objects(
            r##"<rect width="100" height="100" fill="#ffffff"/>
               <rect x="25" y="25" width="50" height="50" fill="#ff0000"/>
               <path d="M 0 0 L 10 0" stroke="url(#none)" stroke-width="2"/>"##,
            r#"width="100" height="100""#,
        );
        assert_eq!(o.len(), 2);
        assert_eq!(
            (o[0].name.as_str(), o[1].name.as_str()),
            ("logo 1", "logo 2")
        );
        assert_eq!(o[0].color, [255, 255, 255, 255]);
        // The white plate has a hole where the red square sits.
        assert!((area(&o[0]) - 7500.0 * PX).abs() < 1e-2);
        assert!((area(&o[1]) - 2500.0 * PX).abs() < 1e-2);
    }

    #[test]
    fn the_drawing_is_not_mirrored_from_above() {
        // Red at the top of the drawing, blue at the bottom.
        let o = objects(
            r##"<rect width="10" height="10" fill="#ff0000"/>
               <rect y="90" width="10" height="10" fill="#0000ff"/>"##,
            r#"width="100" height="100""#,
        );
        assert!(o[0].mesh.aabb().min.y > o[1].mesh.aabb().max.y);
    }

    #[test]
    fn curves_are_smooth_and_empty_drawings_are_refused() {
        let o = objects(
            r#"<circle cx="50" cy="50" r="50"/>"#,
            r#"width="100" height="100""#,
        );
        // A 13 mm circle within the flattening tolerance.
        let r = 50.0 * MM_PER_PX;
        assert!((area(&o[0]) - PI * r * r).abs() < 2.0 * PI * r * TOLERANCE);
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"></svg>"#;
        assert!(parse(svg.as_bytes(), &usvg::Options::default(), "x").is_err());
        assert!(parse(b"not svg", &usvg::Options::default(), "x").is_err());
    }

    #[test]
    fn a_black_square_on_white_rasterizes_dark_in_its_place() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100">
            <rect width="200" height="100" fill="#ffffff"/>
            <rect x="100" y="25" width="50" height="50" fill="#000000"/></svg>"##;
        let img = rasterize(svg.as_bytes(), &usvg::Options::default()).unwrap();
        // 1024 px along the longer side, the aspect kept: 5.12 px per SVG pixel.
        assert_eq!(img.dimensions(), (1024, 512));
        let at = |x: f32, y: f32| *img.get_pixel((x * 5.12) as u32, (y * 5.12) as u32);
        assert_eq!(at(125.0, 50.0), image::Rgba([0, 0, 0, 255]));
        for (x, y) in [(50.0, 50.0), (175.0, 50.0), (125.0, 10.0), (125.0, 90.0)] {
            assert_eq!(at(x, y), image::Rgba([255, 255, 255, 255]), "({x}, {y})");
        }
        // Edges are smooth: the pixel half covered by a 100.5 px wide rect is half opaque.
        let half = rasterize(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="10"><rect width="100.5" height="10"/></svg>"#,
            &usvg::Options::default(),
        )
        .unwrap();
        assert_eq!(half.get_pixel(99, 5)[3], 255);
        let p = half.get_pixel(100, 5);
        assert!(p[3] > 120 && p[3] < 135, "{p:?}");
        // Nothing drawn is transparent.
        assert_eq!(half.get_pixel(101, 5)[3], 0);
    }

    #[test]
    fn an_svg_ring_rasterizes_with_its_hole() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100">
            <path fill-rule="evenodd" d="M 0 0 H 100 V 100 H 0 Z M 25 25 H 75 V 75 H 25 Z"/></svg>"#;
        let img = rasterize(svg.as_bytes(), &usvg::Options::default()).unwrap();
        assert_eq!(img.get_pixel(100, 100)[3], 255);
        assert_eq!(img.get_pixel(512, 512)[3], 0);
    }

    #[test]
    fn svg_files_load_with_known_units_and_are_not_saved() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Tag.SVG");
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="10mm" height="10mm"><rect width="10mm" height="5mm" fill="#123456"/></svg>"##;
        std::fs::write(&path, svg).unwrap();
        assert_eq!(
            super::super::Format::from_path(&path),
            Some(super::super::Format::Svg)
        );
        let loaded = super::super::load(&path).unwrap();
        assert!(loaded.units_known);
        assert_eq!(loaded.objects[0].name, "Tag");
        assert!((area(&loaded.objects[0]) - 50.0).abs() < 1e-3);
        let out = dir.path().join("out.svg");
        assert!(super::super::save(&out, &loaded.objects, &Default::default()).is_err());
    }
}
