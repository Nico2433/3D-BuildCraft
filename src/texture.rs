//! Textures on meshes. Each triangle may carry a [`FaceTex`]: a picture (a "layer" of the
//! registry below, which the GPU holds as one array texture) and a UV for each corner.
//! Two ways to fill them: [`apply_decal`] projects a picture as a sticker, [`apply_box`]
//! tiles it over chosen faces by box mapping (each face is projected along its dominant
//! axis, which is triplanar mapping decided per face).
//!
//! The pictures live in a process-wide registry that only grows, so a layer number kept in
//! an undo snapshot stays valid. Textures are not saved in project files.

use crate::mesh::Mesh;
pub use crate::texture_gen::Kind;
use glam::{Mat4, Vec3, Vec4};
use image::{imageops::FilterType, RgbaImage};
use std::sync::{Arc, Mutex};

/// Side of every picture on the GPU, in pixels.
pub const SIZE: u32 = 256;
/// Pictures the GPU array holds.
pub const LAYERS: usize = 64;
/// Recent pictures the menu lists.
pub const RECENT: usize = 6;

/// A triangle's texture. `layer` 0 means none; otherwise it is the registry index + 1.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FaceTex {
    pub layer: u16,
    /// A sticker: drawn only inside the picture's rectangle, not repeated.
    pub decal: bool,
    pub uv: [[f32; 2]; 3],
}

impl FaceTex {
    /// The vertex attribute the shader reads: layer in the low 16 bits, sticker flag above.
    pub fn pack(self) -> u32 {
        if self.layer == 0 {
            0
        } else {
            self.layer as u32 | (self.decal as u32) << 16
        }
    }
}

impl FaceTex {
    /// The picture coordinates at point `p` of a triangle with corners `tri`, found
    /// barycentrically from this triangle's corner UVs.
    fn uv_at(&self, tri: [Vec3; 3], p: Vec3) -> [f32; 2] {
        let (v0, v1, v2) = (tri[1] - tri[0], tri[2] - tri[0], p - tri[0]);
        let (d00, d01, d11) = (v0.dot(v0), v0.dot(v1), v1.dot(v1));
        let (d20, d21) = (v2.dot(v0), v2.dot(v1));
        let den = d00 * d11 - d01 * d01;
        if den.abs() < 1e-20 {
            return self.uv[0];
        }
        let v = (d11 * d20 - d01 * d21) / den;
        let w = (d00 * d21 - d01 * d20) / den;
        let u = 1.0 - v - w;
        let at = |k: usize| u * self.uv[0][k] + v * self.uv[1][k] + w * self.uv[2][k];
        [at(0), at(1)]
    }

    /// The same texture for a triangle `to` cut out of the one with corners `from`: each
    /// new corner gets the UV the source has at that point, so the picture stays put.
    pub fn within(self, from: [Vec3; 3], to: [Vec3; 3]) -> FaceTex {
        FaceTex {
            uv: to.map(|p| self.uv_at(from, p)),
            ..self
        }
    }
}

#[derive(PartialEq)]
enum Key {
    Kind(Kind),
    Image(u64),
}

struct Entry {
    key: Key,
    image: Arc<RgbaImage>,
    /// Width over height of the picture before it was squared.
    aspect: f32,
}

static REGISTRY: Mutex<Vec<Entry>> = Mutex::new(Vec::new());
static RECENT_LAYERS: Mutex<Vec<u16>> = Mutex::new(Vec::new());

fn add(key: Key, image: RgbaImage, aspect: f32) -> Option<u16> {
    let mut all = REGISTRY.lock().unwrap();
    if let Some(i) = all.iter().position(|e| e.key == key) {
        return Some(i as u16 + 1);
    }
    if all.len() >= LAYERS {
        return None;
    }
    all.push(Entry {
        key,
        image: Arc::new(image),
        aspect,
    });
    Some(all.len() as u16)
}

/// The layer of a built-in picture, if it has been drawn already.
pub fn known_layer(kind: Kind) -> Option<u16> {
    REGISTRY
        .lock()
        .unwrap()
        .iter()
        .position(|e| e.key == Key::Kind(kind))
        .map(|i| i as u16 + 1)
}

/// The layer of a built-in picture, drawn on first use. `None` when the registry is full.
pub fn layer_of(kind: Kind) -> Option<u16> {
    if let Some(layer) = known_layer(kind) {
        return Some(layer);
    }
    add(
        Key::Kind(kind),
        crate::texture_gen::generate(kind, SIZE),
        1.0,
    )
}

/// Adds a picture of any size (a file, a camera frame, a QR code), squared to [`SIZE`].
/// The same pixels give the same layer. `None` when the registry is full.
pub fn register_image(img: &RgbaImage) -> Option<u16> {
    let hash = img.as_raw().iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
        (h ^ *b as u64).wrapping_mul(0x100_0000_01b3)
    }) ^ ((img.width() as u64) << 32 | img.height() as u64);
    let squared = image::imageops::resize(img, SIZE, SIZE, FilterType::Triangle);
    add(
        Key::Image(hash),
        squared,
        img.width() as f32 / img.height().max(1) as f32,
    )
}

/// Width over height of the picture on `layer`.
pub fn aspect(layer: u16) -> f32 {
    REGISTRY
        .lock()
        .unwrap()
        .get((layer as usize).wrapping_sub(1))
        .map_or(1.0, |e| e.aspect)
}

/// The picture on `layer`.
pub fn image(layer: u16) -> Option<Arc<RgbaImage>> {
    REGISTRY
        .lock()
        .unwrap()
        .get((layer as usize).wrapping_sub(1))
        .map(|e| e.image.clone())
}

/// The pictures from registry index `start` on, for the renderer to upload.
pub fn images_from(start: usize) -> Vec<Arc<RgbaImage>> {
    REGISTRY
        .lock()
        .unwrap()
        .iter()
        .skip(start)
        .map(|e| e.image.clone())
        .collect()
}

/// Puts `layer` first in the recent list.
pub fn remember(layer: u16) {
    let mut recent = RECENT_LAYERS.lock().unwrap();
    recent.retain(|l| *l != layer);
    recent.insert(0, layer);
    recent.truncate(RECENT);
}

pub fn recent() -> Vec<u16> {
    RECENT_LAYERS.lock().unwrap().clone()
}

/// A QR code for `text` as a picture: black modules on white, or on nothing.
pub fn qr_image(text: &str, transparent: bool) -> Option<RgbaImage> {
    use qrcode::{Color, QrCode};
    let code = QrCode::new(text.as_bytes()).ok()?;
    let n = code.width() as u32;
    let colors = code.to_colors();
    let margin = 2;
    let side = n + 2 * margin;
    let cell = (SIZE / side).max(1);
    let mut img = RgbaImage::from_pixel(
        side * cell,
        side * cell,
        image::Rgba([255, 255, 255, if transparent { 0 } else { 255 }]),
    );
    for (i, c) in colors.iter().enumerate() {
        if *c == Color::Dark {
            let (cx, cy) = (
                (i as u32 % n + margin) * cell,
                (i as u32 / n + margin) * cell,
            );
            for y in cy..cy + cell {
                for x in cx..cx + cell {
                    img.put_pixel(x, y, image::Rgba([0, 0, 0, 255]));
                }
            }
        }
    }
    Some(img)
}

/// A picture placed on a surface: where, which way it faces, its turn and its size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Decal {
    pub center: Vec3,
    /// Out of the surface, unit length.
    pub normal: Vec3,
    /// Turn about the normal, radians.
    pub angle: f32,
    pub width: f32,
    pub height: f32,
}

impl Decal {
    /// A decal of the picture's shape with `width` across.
    pub fn new(center: Vec3, normal: Vec3, width: f32, layer: u16) -> Self {
        Self {
            center,
            normal: normal.normalize_or(Vec3::Z),
            angle: 0.0,
            width,
            height: width / aspect(layer).max(0.05),
        }
    }

    /// Unit directions of the picture's right and up, on the surface. Up follows world Z
    /// where the surface is not flat on the floor, so pictures stand upright on walls.
    pub fn axes(&self) -> (Vec3, Vec3) {
        let up = if self.normal.z.abs() > 0.95 {
            Vec3::Y
        } else {
            Vec3::Z
        };
        let right = up.cross(self.normal).normalize_or(Vec3::X);
        let top = self.normal.cross(right);
        let (s, c) = self.angle.sin_cos();
        (right * c + top * s, top * c - right * s)
    }

    /// The picture coordinates (0..1, y down) of a world point, and its height above the
    /// decal's plane.
    pub fn uv(&self, p: Vec3) -> ([f32; 2], f32) {
        let (right, top) = self.axes();
        let d = p - self.center;
        (
            [
                0.5 + d.dot(right) / self.width,
                0.5 - d.dot(top) / self.height,
            ],
            d.dot(self.normal),
        )
    }

    /// How far from its plane a surface still takes the decal: enough for a gentle curve,
    /// not for the next part of the model behind it.
    pub fn reach(&self) -> f32 {
        self.width.max(self.height) * 0.3
    }

    /// World to (u, v, height) for the shader's live preview.
    pub fn shader_matrix(&self) -> Mat4 {
        let (right, top) = self.axes();
        let (r0, r1, r2) = (right / self.width, -top / self.height, self.normal);
        let t = Vec3::new(
            0.5 - r0.dot(self.center),
            0.5 - r1.dot(self.center),
            -r2.dot(self.center),
        );
        Mat4::from_cols(
            Vec4::new(r0.x, r1.x, r2.x, 0.0),
            Vec4::new(r0.y, r1.y, r2.y, 0.0),
            Vec4::new(r0.z, r1.z, r2.z, 0.0),
            t.extend(1.0),
        )
    }
}

/// The triangle's corners in world space under `model` and its outward unit normal.
fn world_triangle(mesh: &Mesh, i: usize, model: &Mat4) -> ([Vec3; 3], Vec3) {
    let p = mesh.triangles[i].map(|v| model.transform_point3(mesh.positions[v as usize]));
    let flip = if model.determinant() < 0.0 { -1.0 } else { 1.0 };
    let n = (p[1] - p[0]).cross(p[2] - p[0]).normalize_or_zero() * flip;
    (p, n)
}

/// The surface normal of triangle `i` in world space.
pub fn world_normal(mesh: &Mesh, i: usize, model: &Mat4) -> Vec3 {
    world_triangle(mesh, i, model).1
}

fn with_tex(mesh: &Mesh) -> Mesh {
    let mut out = mesh.clone();
    out.tex.resize(out.triangles.len(), FaceTex::default());
    out
}

/// The mesh with `layer` stuck on every triangle the decal covers: those that face the
/// same way, lie near its plane and overlap its rectangle. Corner UVs come from the
/// planar projection, and the shader clips to the rectangle per pixel, so a decal may
/// cover only part of a large triangle. A triangle holds one picture: a later sticker
/// replaces an earlier one on the triangles they share.
pub fn apply_decal(mesh: &Mesh, model: &Mat4, decal: &Decal, layer: u16) -> Mesh {
    let mut out = with_tex(mesh);
    for i in 0..mesh.triangles.len() {
        let (p, n) = world_triangle(mesh, i, model);
        if n.dot(decal.normal) < 0.25 {
            continue;
        }
        let centroid = (p[0] + p[1] + p[2]) / 3.0;
        if decal.uv(centroid).1.abs() > decal.reach() {
            continue;
        }
        let uv = p.map(|q| decal.uv(q).0);
        let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
        for c in uv {
            for k in 0..2 {
                lo[k] = lo[k].min(c[k]);
                hi[k] = hi[k].max(c[k]);
            }
        }
        if lo[0] <= 1.0 && lo[1] <= 1.0 && hi[0] >= 0.0 && hi[1] >= 0.0 {
            out.tex[i] = FaceTex {
                layer,
                decal: true,
                uv,
            };
        }
    }
    out
}

/// The mesh with `layer` tiled over `faces`: each face is projected along the world axis
/// its normal is closest to, and one picture covers `tile` mm.
pub fn apply_box(mesh: &Mesh, model: &Mat4, faces: &[usize], layer: u16, tile: f32) -> Mesh {
    let mut out = with_tex(mesh);
    for &i in faces {
        let (p, n) = world_triangle(mesh, i, model);
        let a = n.abs();
        let project = |q: Vec3| {
            let (u, v) = if a.z >= a.x && a.z >= a.y {
                (q.x, -q.y)
            } else if a.y >= a.x {
                (q.x, -q.z)
            } else {
                (q.y, -q.z)
            };
            [u / tile, v / tile]
        };
        out.tex[i] = FaceTex {
            layer,
            decal: false,
            uv: p.map(project),
        };
    }
    out
}

/// For files, which cannot hold textures here: each tiled (not sticker) triangle without a
/// colour of its own gets the average colour of its picture. Stickers are left out.
pub fn paint_average_colors(mesh: &mut Mesh) {
    if !mesh.has_tex() {
        return;
    }
    let mut averages: std::collections::HashMap<u16, [u8; 4]> = Default::default();
    mesh.colors.resize(mesh.triangles.len(), [0; 4]);
    for i in 0..mesh.triangles.len() {
        let Some(t) = mesh.face_tex(i).filter(|t| !t.decal) else {
            continue;
        };
        if mesh.colors[i][3] > 0 {
            continue;
        }
        let color = *averages.entry(t.layer).or_insert_with(|| {
            let Some(img) = image(t.layer) else {
                return [0; 4];
            };
            let mut sum = [0u64; 3];
            for p in img.pixels() {
                for k in 0..3 {
                    sum[k] += p[k] as u64;
                }
            }
            let n = (img.width() as u64 * img.height() as u64).max(1);
            [
                (sum[0] / n) as u8,
                (sum[1] / n) as u8,
                (sum[2] / n) as u8,
                255,
            ]
        });
        mesh.colors[i] = color;
    }
}

/// A good size for a sticker or a tile on `mesh` under `model`: a fraction of its
/// longest side.
pub fn natural_size(mesh: &Arc<Mesh>, model: &Mat4, fraction: f32) -> f32 {
    let b = crate::scene::cached_aabb(mesh, model);
    (b.size().max_element() * fraction).max(0.5)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives;

    #[test]
    fn decal_covers_only_the_face_it_lies_on() {
        let cube = primitives::cube(10.0);
        let b = cube.aabb();
        let top = Vec3::new(b.center().x, b.center().y, b.max.z);
        let decal = Decal::new(top, Vec3::Z, 4.0, 0);
        let out = apply_decal(&cube, &Mat4::IDENTITY, &decal, 1);
        let marked: Vec<usize> = (0..12).filter(|&i| out.face_tex(i).is_some()).collect();
        assert_eq!(marked.len(), 2);
        for i in marked {
            assert!(world_normal(&cube, i, &Mat4::IDENTITY).z > 0.9);
            assert!(out.tex[i].decal);
        }
        assert!(cube.tex.is_empty(), "the original is left alone");
    }

    #[test]
    fn decal_stays_on_surfaces_near_its_plane() {
        // Two stacked cubes: a decal on the lower top face leaves the higher one alone.
        let mut mesh = primitives::cube(10.0);
        let mut upper = primitives::cube(10.0);
        upper = upper.transformed(&Mat4::from_translation(Vec3::new(0.0, 0.0, 30.0)));
        mesh.append(&upper);
        let top = Vec3::new(5.0, 5.0, 10.0);
        let decal = Decal::new(top, Vec3::Z, 6.0, 0);
        let out = apply_decal(&mesh, &Mat4::IDENTITY, &decal, 1);
        assert_eq!((0..24).filter(|&i| out.face_tex(i).is_some()).count(), 2);
    }

    #[test]
    fn textures_follow_flip_and_append() {
        let cube = primitives::cube(10.0);
        let tex = apply_box(&cube, &Mat4::IDENTITY, &[0], 1, 5.0);
        let before = tex.tex[0].uv;
        let mirrored = tex.transformed(&Mat4::from_scale(Vec3::new(-1.0, 1.0, 1.0)));
        assert_eq!(mirrored.tex[0].uv[0], before[0]);
        assert_eq!(mirrored.tex[0].uv[1], before[2]);
        let mut joined = cube.clone();
        joined.append(&tex);
        assert_eq!(joined.tex.len(), 24);
        assert!(joined.face_tex(12).is_some() && joined.face_tex(0).is_none());
    }

    #[test]
    fn decal_uv_is_centred_and_upright() {
        let d = Decal::new(Vec3::new(1.0, 2.0, 3.0), -Vec3::Y, 4.0, 0);
        let ([u, v], h) = d.uv(Vec3::new(1.0, 2.0, 3.0));
        assert!((u - 0.5).abs() < 1e-6 && (v - 0.5).abs() < 1e-6 && h.abs() < 1e-6);
        // On a wall facing -Y, up in the world is up in the picture: smaller v.
        let ([_, above], _) = d.uv(Vec3::new(1.0, 2.0, 4.0));
        assert!(above < 0.5);
        // The matrix the shader uses agrees with `uv`.
        let q = d.shader_matrix().transform_point3(Vec3::new(2.0, 2.0, 4.0));
        let ([u, v], h) = d.uv(Vec3::new(2.0, 2.0, 4.0));
        assert!((q.x - u).abs() < 1e-5 && (q.y - v).abs() < 1e-5 && (q.z - h).abs() < 1e-5);
    }

    #[test]
    fn box_mapping_tiles_by_world_size() {
        let cube = primitives::cube(10.0);
        let out = apply_box(&cube, &Mat4::IDENTITY, &[0, 1], 2, 5.0);
        let t = out.tex[0];
        let span = |k: usize| {
            let v: Vec<f32> = t.uv.iter().map(|c| c[k]).collect();
            v.iter().cloned().fold(f32::MIN, f32::max) - v.iter().cloned().fold(f32::MAX, f32::min)
        };
        // A 10 mm face with 5 mm tiles repeats twice each way.
        assert!((span(0) - 2.0).abs() < 1e-4 || (span(1) - 2.0).abs() < 1e-4);
        assert!(!t.decal && out.face_tex(5).is_none());
    }

    #[test]
    fn registry_hands_out_stable_layers() {
        let a = layer_of(Kind::Paper).unwrap();
        assert_eq!(layer_of(Kind::Paper), Some(a));
        let img = RgbaImage::from_pixel(8, 4, image::Rgba([1, 2, 3, 255]));
        let b = register_image(&img).unwrap();
        assert_eq!(register_image(&img), Some(b));
        assert!((aspect(b) - 2.0).abs() < 1e-6);
        assert_eq!(image(b).unwrap().width(), SIZE);
    }

    #[test]
    fn files_get_the_average_colour_of_tiled_faces() {
        let img = RgbaImage::from_pixel(4, 4, image::Rgba([200, 100, 50, 255]));
        let layer = register_image(&img).unwrap();
        let cube = primitives::cube(10.0);
        let mut mesh = apply_box(&cube, &Mat4::IDENTITY, &[0, 1], layer, 5.0);
        paint_average_colors(&mut mesh);
        assert_eq!(mesh.face_color(0), Some([200, 100, 50, 255]));
        assert_eq!(mesh.face_color(2), None);
    }

    #[test]
    fn qr_draws_dark_modules() {
        let img = qr_image("https://example.com", true).unwrap();
        assert!(img.pixels().any(|p| p[3] == 0));
        assert!(img.pixels().any(|p| p[0] == 0 && p[3] == 255));
        assert!(qr_image("x", false).unwrap().pixels().all(|p| p[3] == 255));
    }
}
