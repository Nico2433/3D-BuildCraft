//! Built-in textures and stencil shapes, all drawn by code (noise, weaves and polygons):
//! no picture files ship with the app. Every texture repeats seamlessly at its edges.

use image::{Rgba, RgbaImage};
use std::f32::consts::{PI, TAU};

/// The built-in pictures: 24 repeating textures, then 6 white stencil shapes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Paper,
    Linen,
    Denim,
    Burlap,
    WaterDrops,
    CrumpledPaper,
    Sandstone,
    Gravel,
    GreenMarble,
    WhiteMarble,
    BrownMarble,
    Granite,
    PlasterWhite,
    PlasterBeige,
    Cream,
    Travertine,
    IcePlaster,
    PinkPlaster,
    Felt,
    Pearl,
    Cork,
    DarkWood,
    LightWood,
    Walnut,
    Star4,
    Star8,
    Arrow,
    Chevron,
    Cloud,
    Ring,
}

impl Kind {
    /// The texture grid, row by row.
    pub const TEXTURES: [Kind; 24] = [
        Kind::Paper,
        Kind::Linen,
        Kind::Denim,
        Kind::Burlap,
        Kind::WaterDrops,
        Kind::CrumpledPaper,
        Kind::Sandstone,
        Kind::Gravel,
        Kind::GreenMarble,
        Kind::WhiteMarble,
        Kind::BrownMarble,
        Kind::Granite,
        Kind::PlasterWhite,
        Kind::PlasterBeige,
        Kind::Cream,
        Kind::Travertine,
        Kind::IcePlaster,
        Kind::PinkPlaster,
        Kind::Felt,
        Kind::Pearl,
        Kind::Cork,
        Kind::DarkWood,
        Kind::LightWood,
        Kind::Walnut,
    ];

    pub const STENCILS: [Kind; 6] = [
        Kind::Star4,
        Kind::Star8,
        Kind::Arrow,
        Kind::Chevron,
        Kind::Cloud,
        Kind::Ring,
    ];

    pub fn is_stencil(self) -> bool {
        Self::STENCILS.contains(&self)
    }
}

type Rgb = [f32; 3];

fn rgb(r: u8, g: u8, b: u8) -> Rgb {
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0]
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    [
        lerp(a[0], b[0], t),
        lerp(a[1], b[1], t),
        lerp(a[2], b[2], t),
    ]
}

fn scale(c: Rgb, k: f32) -> Rgb {
    [c[0] * k, c[1] * k, c[2] * k]
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn hash2(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x27d4_eb2d)
        ^ (y as u32).wrapping_mul(0x1656_67b1)
        ^ seed.wrapping_mul(0x9e37_79b1);
    h ^= h >> 15;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 13;
    h = h.wrapping_mul(0xc2b2_ae35);
    h ^= h >> 16;
    (h & 0xFF_FFFF) as f32 / 16_777_215.0
}

/// Smooth value noise on a grid of `px` by `py` cells that wraps at the edges.
fn vnoise(u: f32, v: f32, px: i32, py: i32, seed: u32) -> f32 {
    let (x, y) = (u * px as f32, v * py as f32);
    let (xi, yi) = (x.floor(), y.floor());
    let (fx, fy) = (x - xi, y - yi);
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let (x0, y0) = (xi as i32, yi as i32);
    let g = |dx: i32, dy: i32| hash2((x0 + dx).rem_euclid(px), (y0 + dy).rem_euclid(py), seed);
    lerp(lerp(g(0, 0), g(1, 0), sx), lerp(g(0, 1), g(1, 1), sx), sy)
}

/// Layered noise, 0..1: each octave has twice the cells and half the weight.
fn fbm(u: f32, v: f32, base: i32, octaves: u32, seed: u32) -> f32 {
    fbm_xy(u, v, base, base, octaves, seed)
}

fn fbm_xy(u: f32, v: f32, px: i32, py: i32, octaves: u32, seed: u32) -> f32 {
    let (mut sum, mut amp, mut total) = (0.0, 1.0, 0.0);
    for o in 0..octaves {
        let k = 1 << o;
        sum += amp * vnoise(u, v, px * k, py * k, seed + o);
        total += amp;
        amp *= 0.5;
    }
    sum / total
}

/// Cell noise on `n` by `n` wrapping cells: distance to the nearest and second nearest
/// feature point (in cells) and the offset to the nearest.
fn worley(u: f32, v: f32, n: i32, seed: u32) -> (f32, f32, [f32; 2]) {
    let (x, y) = (u * n as f32, v * n as f32);
    let (cx, cy) = (x.floor() as i32, y.floor() as i32);
    let (mut f1, mut f2, mut off) = (9.0_f32, 9.0_f32, [0.0, 0.0]);
    for dy in -1..=1 {
        for dx in -1..=1 {
            let (gx, gy) = (cx + dx, cy + dy);
            let (wx, wy) = (gx.rem_euclid(n), gy.rem_euclid(n));
            let px = gx as f32 + 0.15 + 0.7 * hash2(wx, wy, seed);
            let py = gy as f32 + 0.15 + 0.7 * hash2(wx, wy, seed + 77);
            let (ox, oy) = (px - x, py - y);
            let d = (ox * ox + oy * oy).sqrt();
            if d < f1 {
                f2 = f1;
                f1 = d;
                off = [ox, oy];
            } else if d < f2 {
                f2 = d;
            }
        }
    }
    (f1, f2, off)
}

/// Fine per-pixel grain at the 256 pixel picture size.
fn grain(u: f32, v: f32, seed: u32) -> f32 {
    vnoise(u, v, 256, 256, seed)
}

/// A woven cloth: `n` threads each way, over and under, with `gap` the dark between them.
fn weave(u: f32, v: f32, n: f32, seed: u32) -> (f32, f32) {
    let (x, y) = (u * n, v * n);
    let over = (x.floor() as i32 + y.floor() as i32) & 1 == 0;
    let (fx, fy) = (x.fract(), y.fract());
    // Brightness across the thread that is on top here.
    let across = if over { fy } else { fx };
    let along = if over { fx } else { fy };
    let round = (across * PI).sin();
    let fibre = grain(u, v, seed);
    let end = (along * PI).sin().sqrt();
    (round * (0.75 + 0.25 * end) * (0.8 + 0.2 * fibre), end)
}

fn marble(u: f32, v: f32, base: [Rgb; 2], vein: Rgb, strength: f32, seed: u32) -> Rgb {
    let cloud = fbm(u, v, 3, 5, seed);
    let warp = fbm(u, v, 2, 5, seed + 9);
    let line = |a: f32, b: f32, k: f32| {
        let s = (TAU * (a * u + b * v) + k * warp).sin().abs();
        1.0 - smoothstep(0.0, 0.16, s)
    };
    let veins = line(2.0, 1.0, 7.0).max(0.7 * line(1.0, -2.0, 9.0)) * strength;
    let c = mix(base[0], base[1], cloud);
    mix(c, vein, veins * (0.5 + 0.5 * fbm(u, v, 8, 3, seed + 31)))
}

fn plaster(u: f32, v: f32, c: Rgb, contrast: f32, seed: u32) -> Rgb {
    let blotch = fbm(u, v, 6, 5, seed) - 0.5;
    let speck = grain(u, v, seed + 5) - 0.5;
    scale(c, 1.0 + contrast * blotch + 0.05 * speck)
}

fn wood(u: f32, v: f32, dark: Rgb, light: Rgb, rings: f32, seed: u32) -> Rgb {
    // Planks run along u: few cells that way, many across.
    let warp = fbm_xy(u, v, 2, 5, 4, seed);
    let t = v * rings + 3.0 * warp;
    let ring = t.fract();
    let band = smoothstep(0.0, 0.8, ring) * (1.0 - smoothstep(0.8, 1.0, ring));
    let fibre = fbm_xy(u, v, 6, 160, 2, seed + 3);
    let pore = vnoise(u, v, 24, 256, seed + 8);
    let k = (0.55 * band + 0.35 * fibre + 0.1 * pore).clamp(0.0, 1.0);
    mix(dark, light, k)
}

/// The colour of texture `kind` at `(u, v)`, both 0..1.
fn texture_pixel(kind: Kind, u: f32, v: f32) -> Rgb {
    match kind {
        Kind::Paper => {
            let g = fbm(u, v, 64, 3, 1);
            let fibre = 0.5 * vnoise(u, v, 256, 10, 2) + 0.5 * vnoise(u, v, 10, 256, 3);
            mix(
                rgb(226, 196, 138),
                rgb(250, 232, 182),
                0.55 * g + 0.45 * fibre,
            )
        }
        Kind::Linen => {
            let (t, gap) = weave(u, v, 56.0, 4);
            let shade = 0.55 + 0.45 * t * gap;
            let slub = fbm_xy(u, v, 3, 40, 3, 6);
            scale(rgb(238, 232, 218), shade * (0.9 + 0.12 * slub))
        }
        Kind::Denim => {
            // Twill: threads climbing diagonally.
            let d = ((u + v) * 72.0 * TAU).sin() * 0.5 + 0.5;
            let (t, _) = weave(u, v, 80.0, 7);
            let shade = 0.5 * d + 0.5 * t;
            let wash = fbm(u, v, 5, 4, 8);
            scale(
                mix(rgb(36, 62, 150), rgb(118, 158, 232), shade),
                0.8 + 0.3 * wash,
            )
        }
        Kind::Burlap => {
            let (t, gap) = weave(u, v, 26.0, 9);
            let holes = smoothstep(0.1, 0.5, gap * t.sqrt());
            let streak = vnoise(u, v, 8, 200, 10) * 0.5 + vnoise(u, v, 200, 8, 11) * 0.5;
            let c = mix(rgb(112, 78, 38), rgb(212, 172, 100), 0.5 * t + 0.5 * streak);
            scale(c, 0.3 + 0.7 * holes)
        }
        Kind::WaterDrops => {
            let base = mix(rgb(96, 206, 220), rgb(150, 232, 238), fbm(u, v, 5, 4, 12));
            let (f1, _, off) = worley(u, v, 9, 13);
            let r = 0.32;
            if f1 < r {
                let k = f1 / r;
                let len = (off[0] * off[0] + off[1] * off[1]).sqrt().max(1e-4);
                // Lit from the top left: bright on that side, dark rim on the other.
                let lit =
                    ((-off[0] - off[1]) / len * std::f32::consts::FRAC_1_SQRT_2).clamp(-1.0, 1.0);
                let dome = k * k;
                let shade = 1.0 + 0.45 * lit * dome - 0.25 * smoothstep(0.8, 1.0, k);
                let spec = smoothstep(0.82, 1.0, -lit * dome * 1.4 + 0.1) * 0.4;
                let c = scale(mix(base, rgb(205, 245, 250), 0.35), shade);
                [c[0] + spec, c[1] + spec, c[2] + spec]
            } else {
                base
            }
        }
        Kind::CrumpledPaper => {
            let height = |u: f32, v: f32| {
                let ridged = 1.0 - (2.0 * fbm(u, v, 5, 5, 14) - 1.0).abs();
                let (f1, f2, _) = worley(u, v, 7, 15);
                0.7 * ridged + 0.5 * (f2 - f1)
            };
            let e = 1.0 / 256.0;
            let h = height(u, v);
            let (dx, dy) = (height(u + e, v) - h, height(u, v + e) - h);
            let lit = (0.55 - 9.0 * (dx + dy) * std::f32::consts::FRAC_1_SQRT_2).clamp(0.0, 1.0);
            mix(rgb(112, 80, 46), rgb(218, 182, 126), lit)
        }
        Kind::Sandstone => {
            let strata = v * 22.0 + 3.0 * fbm(u, v, 4, 3, 16);
            let band = (strata * TAU).sin() * 0.5 + 0.5;
            let c = mix(rgb(222, 168, 128), rgb(244, 206, 164), band);
            scale(c, 0.88 + 0.24 * grain(u, v, 17))
        }
        Kind::Gravel => {
            let n = grain(u, v, 18);
            let c = mix(rgb(86, 66, 58), rgb(182, 156, 140), n);
            scale(c, 0.8 + 0.4 * fbm(u, v, 24, 3, 19))
        }
        Kind::GreenMarble => marble(
            u,
            v,
            [rgb(10, 62, 36), rgb(44, 138, 84)],
            rgb(196, 236, 208),
            0.85,
            20,
        ),
        Kind::WhiteMarble => marble(
            u,
            v,
            [rgb(214, 216, 222), rgb(248, 248, 250)],
            rgb(110, 116, 128),
            0.8,
            21,
        ),
        Kind::BrownMarble => marble(
            u,
            v,
            [rgb(58, 32, 20), rgb(116, 74, 48)],
            rgb(214, 184, 144),
            0.85,
            22,
        ),
        Kind::Granite => {
            let n = grain(u, v, 23);
            let c = if n < 0.32 {
                rgb(58, 58, 62)
            } else if n > 0.72 {
                rgb(238, 238, 240)
            } else {
                mix(rgb(120, 120, 126), rgb(176, 176, 180), grain(u, v, 24))
            };
            scale(c, 0.85 + 0.3 * fbm(u, v, 16, 3, 25))
        }
        Kind::PlasterWhite => plaster(u, v, rgb(240, 240, 238), 0.14, 26),
        Kind::PlasterBeige => plaster(u, v, rgb(224, 210, 186), 0.14, 27),
        Kind::Cream => plaster(u, v, rgb(252, 238, 212), 0.1, 28),
        Kind::Travertine => {
            let swirl = fbm(u, v, 3, 5, 29);
            let c = mix(rgb(230, 204, 156), rgb(246, 226, 188), swirl);
            scale(c, 0.94 + 0.12 * fbm(u, v, 12, 3, 30))
        }
        Kind::IcePlaster => plaster(u, v, rgb(206, 226, 250), 0.12, 31),
        Kind::PinkPlaster => plaster(u, v, rgb(240, 192, 198), 0.12, 32),
        Kind::Felt => {
            let fibre = vnoise(u, v, 256, 256, 33) * vnoise(u, v, 128, 128, 34);
            let c = mix(rgb(56, 12, 92), rgb(122, 52, 164), fibre * 1.6);
            scale(c, 0.8 + 0.4 * fbm(u, v, 8, 3, 35))
        }
        Kind::Pearl => {
            let t = fbm(u, v, 3, 4, 36);
            let stops = [
                (0.0, rgb(186, 206, 255)),
                (0.35, rgb(236, 214, 250)),
                (0.65, rgb(210, 238, 246)),
                (1.0, rgb(252, 214, 232)),
            ];
            let t = ((t - 0.3) / 0.4).clamp(0.0, 1.0);
            let mut c = stops[3].1;
            for w in stops.windows(2) {
                if t <= w[1].0 {
                    c = mix(w[0].1, w[1].1, (t - w[0].0) / (w[1].0 - w[0].0));
                    break;
                }
            }
            scale(c, 0.96 + 0.08 * grain(u, v, 37))
        }
        Kind::Cork => {
            let (f1, f2, _) = worley(u, v, 26, 38);
            let seam = 1.0 - smoothstep(0.0, 0.18, f2 - f1);
            let c = mix(rgb(206, 148, 98), rgb(168, 112, 70), fbm(u, v, 20, 3, 39));
            scale(
                mix(c, rgb(104, 62, 34), seam * 0.8),
                0.85 + 0.3 * grain(u, v, 40),
            )
        }
        Kind::DarkWood => wood(u, v, rgb(34, 20, 14), rgb(108, 70, 48), 9.0, 41),
        Kind::LightWood => wood(u, v, rgb(186, 134, 74), rgb(232, 184, 116), 7.0, 42),
        Kind::Walnut => wood(u, v, rgb(104, 62, 30), rgb(184, 124, 70), 8.0, 43),
        _ => [1.0; 3],
    }
}

fn in_polygon(p: [f32; 2], poly: &[[f32; 2]]) -> bool {
    let mut inside = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[j]);
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            inside = !inside;
        }
        j = i;
    }
    inside
}

fn star(points: usize, inner: f32) -> Vec<[f32; 2]> {
    (0..points * 2)
        .map(|i| {
            let r = if i % 2 == 0 { 0.95 } else { 0.95 * inner };
            let a = PI * i as f32 / points as f32 - PI / 2.0;
            [r * a.cos(), r * a.sin()]
        })
        .collect()
}

/// Whether the point (x, y in -1..1, y down) lies inside stencil `kind`.
fn stencil_covers(kind: Kind, p: [f32; 2]) -> bool {
    match kind {
        Kind::Star4 => in_polygon(p, &star(4, 0.26)),
        Kind::Star8 => in_polygon(p, &star(8, 0.62)),
        Kind::Arrow => in_polygon(
            p,
            &[
                [-0.92, -0.26],
                [0.08, -0.26],
                [0.08, -0.7],
                [0.95, 0.0],
                [0.08, 0.7],
                [0.08, 0.26],
                [-0.92, 0.26],
            ],
        ),
        Kind::Chevron => in_polygon(
            p,
            &[
                [-0.75, -0.92],
                [-0.2, -0.92],
                [0.8, 0.0],
                [-0.2, 0.92],
                [-0.75, 0.92],
                [0.25, 0.0],
            ],
        ),
        Kind::Cloud => {
            let blobs = [
                (-0.5, 0.12, 0.34),
                (-0.22, -0.22, 0.4),
                (0.22, -0.18, 0.38),
                (0.55, 0.12, 0.32),
                (0.0, 0.32, 0.4),
                (-0.28, 0.3, 0.34),
                (0.3, 0.3, 0.34),
            ];
            blobs
                .iter()
                .any(|(x, y, r)| (p[0] - x).powi(2) + (p[1] - y).powi(2) <= r * r)
        }
        Kind::Ring => {
            let r = (p[0] * p[0] + p[1] * p[1]).sqrt();
            (0.6..=0.95).contains(&r)
        }
        _ => false,
    }
}

/// Builds the picture of `kind`, `size` pixels square: opaque for the textures, white
/// with an anti-aliased alpha for the stencils.
pub fn generate(kind: Kind, size: u32) -> RgbaImage {
    let mut img = RgbaImage::new(size, size);
    for (x, y, px) in img.enumerate_pixels_mut() {
        let (u, v) = (
            (x as f32 + 0.5) / size as f32,
            (y as f32 + 0.5) / size as f32,
        );
        *px = if kind.is_stencil() {
            const N: u32 = 4;
            let mut hit = 0;
            for sy in 0..N {
                for sx in 0..N {
                    let (fx, fy) = (
                        (x as f32 + (sx as f32 + 0.5) / N as f32) / size as f32,
                        (y as f32 + (sy as f32 + 0.5) / N as f32) / size as f32,
                    );
                    hit += stencil_covers(kind, [fx * 2.0 - 1.0, fy * 2.0 - 1.0]) as u32;
                }
            }
            Rgba([255, 255, 255, (hit * 255 / (N * N)) as u8])
        } else {
            let c = texture_pixel(kind, u, v);
            let b = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
            Rgba([b(c[0]), b(c[1]), b(c[2]), 255])
        };
    }
    img
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_draws_something() {
        for kind in Kind::TEXTURES.into_iter().chain(Kind::STENCILS) {
            let img = generate(kind, 48);
            assert_eq!(img.dimensions(), (48, 48));
            if kind.is_stencil() {
                let covered = img.pixels().filter(|p| p[3] > 128).count();
                assert!(covered > 100 && covered < 48 * 48, "{kind:?}: {covered}");
            } else {
                let first = img.get_pixel(0, 0);
                assert!(img.pixels().any(|p| p != first), "{kind:?} is flat");
            }
        }
    }

    #[test]
    fn textures_repeat_without_a_seam() {
        // The noise wraps, so a sample one picture away is the same.
        for kind in Kind::TEXTURES {
            let (a, b) = (texture_pixel(kind, 0.3, 0.4), texture_pixel(kind, 1.3, 1.4));
            assert!(
                a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.02),
                "{kind:?}: {a:?} {b:?}"
            );
        }
    }
}
