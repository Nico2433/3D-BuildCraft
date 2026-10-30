//! Load image: a picture turned into a solid by one of the original's methods: terraces
//! of brightness (contour), a smooth height map, the dark areas' outline extruded (edge),
//! or a mirrored two-level stamp.

use crate::emboss::{self, Emboss, Pattern};
use crate::mesh::Mesh;
use crate::primitives::box_mesh;
use anyhow::{anyhow, bail, Result};
use glam::Vec3;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Mode {
    /// Brightness in `levels` flat steps.
    #[default]
    Contour,
    /// Brightness becomes height over a thin base.
    HeightMap,
    /// The dark areas' outline, extruded.
    Edge,
    /// Dark areas raised on a plate, mirrored so a print stamps the right way round.
    Stamp,
}

impl Mode {
    pub const ALL: [Mode; 4] = [Mode::Contour, Mode::HeightMap, Mode::Edge, Mode::Stamp];

    pub fn label(self) -> &'static str {
        match self {
            Mode::Contour => "Contour",
            Mode::HeightMap => "Height map",
            Mode::Edge => "Edge",
            Mode::Stamp => "Stamp",
        }
    }
}

/// Everything the tool bar sets.
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    pub path: PathBuf,
    pub mode: Mode,
    /// Size along the longer side (or the height, for Edge) and the most relief, in mm.
    pub size: f32,
    pub depth: f32,
    /// Contour steps, 2 to 8.
    pub levels: u32,
    /// Blur before sampling, 0 (none) to 1.
    pub smooth: f32,
    pub invert: bool,
    /// Colour the top with the picture.
    pub textures: bool,
}

impl Options {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            mode: Mode::default(),
            size: 80.0,
            depth: 3.0,
            levels: 8,
            smooth: 1.0,
            invert: false,
            textures: true,
        }
    }
}

/// Picture files Load image opens.
pub const EXTENSIONS: [&str; 3] = ["png", "jpg", "jpeg"];

/// Thickness under the lowest point of a relief, so it holds together.
const BASE: f32 = 0.6;
/// Most samples along the longer side of a relief.
const MAX_SAMPLES: u32 = 160;

/// The image as a solid on the floor, centred.
pub fn build(o: &Options) -> Result<Mesh> {
    if !(o.size > 0.0 && o.depth > 0.0 && o.size.is_finite() && o.depth.is_finite()) {
        bail!("Size and depth must be greater than zero");
    }
    match o.mode {
        Mode::HeightMap => relief(o, None, false),
        Mode::Contour => relief(o, Some(o.levels.clamp(2, 8)), false),
        Mode::Stamp => relief(o, Some(2), true),
        Mode::Edge => {
            // The outline is Emboss's image pattern, raised from a flat plate it is then
            // lifted off: the plate's top is at z = 1 and the pattern sinks 0.5 into it.
            let plate = box_mesh(Vec3::new(o.size * 4.0, o.size * 4.0, 1.0));
            let e = Emboss {
                pattern: Pattern::Image(o.path.clone()),
                size: o.size,
                depth: (o.depth - 0.5).max(0.1),
                ..Emboss::default()
            };
            let solid = emboss::text_solid(&plate, &e)?;
            Ok(solid.transformed(&glam::Mat4::from_translation(Vec3::new(0.0, 0.0, -0.5))))
        }
    }
}

/// A height relief; `levels` makes flat steps of it, `mirror` flips it left to right.
fn relief(o: &Options, levels: Option<u32>, mirror: bool) -> Result<Mesh> {
    let img = image::open(&o.path)
        .map_err(|e| anyhow!("Could not open the image {}: {e}", o.path.display()))?;
    let img = if img.width().max(img.height()) > MAX_SAMPLES {
        img.resize(
            MAX_SAMPLES,
            MAX_SAMPLES,
            image::imageops::FilterType::Triangle,
        )
    } else {
        img
    };
    let img = if mirror { img.fliph() } else { img };
    let img = if o.smooth > 0.0 {
        img.blur(o.smooth * 2.0)
    } else {
        img
    }
    .to_rgba8();
    let (w, h) = (img.width() as usize, img.height() as usize);
    if w < 2 || h < 2 {
        bail!("The image is too small");
    }
    let step = o.size / (w.max(h) - 1) as f32;
    // Transparent pixels count as white: no height.
    let height = |x: usize, y: usize| {
        let p = img.get_pixel(x as u32, y as u32);
        let lum = if p[3] < 128 {
            1.0
        } else {
            (0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32) / 255.0
        };
        let mut t = if o.invert { lum } else { 1.0 - lum };
        if let Some(n) = levels {
            t = ((t * n as f32).floor().min(n as f32 - 1.0)) / (n as f32 - 1.0);
        }
        BASE + o.depth * t
    };
    let (ox, oy) = ((w - 1) as f32 * step * 0.5, (h - 1) as f32 * step * 0.5);
    // Image rows go down; y in the model goes up.
    let at = |x: usize, y: usize, z: f32| Vec3::new(x as f32 * step - ox, oy - y as f32 * step, z);
    let mut soup = Vec::new();
    // The picture's colour on each top triangle when textures are on; none elsewhere.
    let mut colors = Vec::new();
    for y in 0..h - 1 {
        for x in 0..w - 1 {
            let top = |x, y| at(x, y, height(x, y));
            let (a, b, c, d) = (top(x, y), top(x + 1, y), top(x + 1, y + 1), top(x, y + 1));
            soup.push([a, c, b]);
            soup.push([a, d, c]);
            let p = img.get_pixel(x as u32, y as u32);
            let c = if o.textures {
                [p[0], p[1], p[2], 255]
            } else {
                [0; 4]
            };
            colors.extend([c, c]);
            let low = |x, y| at(x, y, 0.0);
            let (a, b, c, d) = (low(x, y), low(x + 1, y), low(x + 1, y + 1), low(x, y + 1));
            soup.push([a, b, c]);
            soup.push([a, c, d]);
            colors.extend([[0; 4]; 2]);
        }
    }
    // Walls around the border, from the floor up to the relief.
    let border: Vec<(usize, usize)> = (0..w)
        .map(|x| (x, 0))
        .chain((1..h).map(|y| (w - 1, y)))
        .chain((0..w - 1).rev().map(|x| (x, h - 1)))
        .chain((1..h - 1).rev().map(|y| (0, y)))
        .collect();
    for i in 0..border.len() {
        let (p, q) = (border[i], border[(i + 1) % border.len()]);
        let (pt, qt) = (
            at(p.0, p.1, height(p.0, p.1)),
            at(q.0, q.1, height(q.0, q.1)),
        );
        let (pb, qb) = (at(p.0, p.1, 0.0), at(q.0, q.1, 0.0));
        soup.push([pb, qt, qb]);
        soup.push([pb, pt, qt]);
        colors.extend([[0; 4]; 2]);
    }
    let mut m = Mesh::from_triangle_soup(&soup);
    if o.textures {
        m.colors = colors;
    }
    // Whichever way the walls came out, make the whole solid face outwards.
    if m.stats().volume < 0.0 {
        for t in &mut m.triangles {
            t.swap(1, 2);
        }
    }
    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// Plain options: no blur or colours, so heights are exact.
    pub(crate) fn opts(
        path: std::path::PathBuf,
        mode: Mode,
        size: f32,
        depth: f32,
        invert: bool,
    ) -> Options {
        Options {
            mode,
            size,
            depth,
            invert,
            smooth: 0.0,
            textures: false,
            ..Options::new(path)
        }
    }

    fn gradient(dir: &Path) -> std::path::PathBuf {
        let p = dir.join("g.png");
        image::GrayImage::from_fn(40, 20, |x, _| image::Luma([(x * 255 / 39) as u8]))
            .save(&p)
            .unwrap();
        p
    }

    #[test]
    fn relief_is_closed_sized_and_dark_is_high() {
        let dir = tempfile::tempdir().unwrap();
        let m = build(&opts(
            gradient(dir.path()),
            Mode::HeightMap,
            40.0,
            3.0,
            false,
        ))
        .unwrap();
        let s = m.stats();
        assert!(s.is_closed_manifold(), "{s:?}");
        assert!(s.volume > 0.0);
        let b = m.aabb();
        assert!((b.max.x - b.min.x - 40.0).abs() < 1e-3 && b.min.z.abs() < 1e-6);
        assert!((b.max.z - (BASE + 3.0)).abs() < 0.05, "{b:?}");
        // The dark (left) side is the high one.
        let left = m
            .positions
            .iter()
            .filter(|p| p.x < -19.0)
            .map(|p| p.z)
            .fold(0.0, f32::max);
        let right = m
            .positions
            .iter()
            .filter(|p| p.x > 19.0)
            .map(|p| p.z)
            .fold(0.0, f32::max);
        assert!(left > right + 2.0, "{left} {right}");
        let inverted = build(&opts(
            gradient(dir.path()),
            Mode::HeightMap,
            40.0,
            3.0,
            true,
        ))
        .unwrap();
        let right = inverted
            .positions
            .iter()
            .filter(|p| p.x > 19.0)
            .map(|p| p.z)
            .fold(0.0, f32::max);
        assert!(right > 3.0);
    }

    #[test]
    fn contour_steps_and_stamp_mirrors() {
        let dir = tempfile::tempdir().unwrap();
        let o = Options {
            levels: 3,
            ..opts(gradient(dir.path()), Mode::Contour, 40.0, 3.0, false)
        };
        let m = build(&o).unwrap();
        assert!(m.stats().is_closed_manifold());
        let mut zs: Vec<i32> = m
            .positions
            .iter()
            .map(|p| (p.z * 100.0).round() as i32)
            .collect();
        zs.sort_unstable();
        zs.dedup();
        // The floor and three steps.
        assert_eq!(zs.len(), 4, "{zs:?}");
        let stamp = build(&opts(gradient(dir.path()), Mode::Stamp, 40.0, 3.0, false)).unwrap();
        // Mirrored: the dark side is now on the right.
        let high_x: f32 = stamp
            .positions
            .iter()
            .filter(|p| p.z > 2.0)
            .map(|p| p.x)
            .sum();
        assert!(high_x > 0.0);
        let colored = build(&Options {
            textures: true,
            ..o
        })
        .unwrap();
        assert!(colored.has_colors());
    }

    #[test]
    fn outline_is_the_dark_part_on_the_floor() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("dot.png");
        image::GrayImage::from_fn(30, 30, |x, y| {
            let d = (x as f32 - 15.0).hypot(y as f32 - 15.0);
            image::Luma([if d < 10.0 { 0 } else { 255 }])
        })
        .save(&p)
        .unwrap();
        let m = build(&opts(p.clone(), Mode::Edge, 20.0, 2.0, false)).unwrap();
        assert!(m.stats().is_closed_manifold());
        let b = m.aabb();
        assert!(
            b.min.z.abs() < 1e-3 && (b.max.z - 2.0).abs() < 1e-3,
            "{b:?}"
        );
        assert!((b.max.y - b.min.y - 20.0).abs() < 0.1);
        assert!(build(&opts(p.clone(), Mode::HeightMap, 0.0, 1.0, false)).is_err());
    }
}
