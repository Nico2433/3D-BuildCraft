//! Small pictures of a mesh for tiles (recent files, libraries, the selection pane),
//! drawn on the CPU so they need no GPU work and look the same everywhere.

use crate::io::Rgba;
use crate::mesh::{Aabb, Mesh};
use glam::{Mat4, Vec3};

/// One mesh in one colour; see [`render_parts`].
pub fn render(mesh: &Mesh, color: Rgba, size: u32) -> Vec<u8> {
    render_parts(&[(mesh, color)], size)
}

/// Where a picture is taken from and how big it is; see [`render_view`].
#[derive(Clone, Copy, Debug)]
pub struct View {
    /// Radians around Z from +X to the eye, as [`crate::camera::OrbitCamera::yaw`].
    pub yaw: f32,
    /// Radians of the eye above the floor plane, -90°..90°.
    pub pitch: f32,
    pub width: u32,
    pub height: u32,
    /// Filled behind the parts; `None` leaves the background transparent.
    pub background: Option<Rgba>,
    /// A box to fit snugly in the picture; `None` fits the bounding sphere of all parts.
    pub frame: Option<Aabb>,
}

impl View {
    /// The tiles' view: from the front right and above, square, transparent.
    pub fn thumbnail(size: u32) -> Self {
        Self {
            yaw: (-60f32).to_radians(),
            pitch: 28f32.to_radians(),
            width: size,
            height: size,
            background: None,
            frame: None,
        }
    }

    /// Unit vector from the scene towards the eye, as `OrbitCamera::eye` places it.
    pub fn dir(&self) -> Vec3 {
        let (sp, cp) = self.pitch.sin_cos();
        let (sy, cy) = self.yaw.sin_cos();
        Vec3::new(cp * cy, cp * sy, sp)
    }

    /// The picture's up: the eye's direction turned 90° further up. Same as Z up for an
    /// orbit camera, and still defined straight above or below (top: +Y, bottom: -Y), so
    /// those views keep X to the right as the camera has it just short of them.
    pub fn up(&self) -> Vec3 {
        let (sp, cp) = self.pitch.sin_cos();
        let (sy, cy) = self.yaw.sin_cos();
        Vec3::new(-sp * cy, -sp * sy, cp)
    }
}

/// The meshes, each in its colour, seen from the front right and above, filling a
/// `size`×`size` RGBA image with a transparent background.
pub fn render_parts(parts: &[(&Mesh, Rgba)], size: u32) -> Vec<u8> {
    render_view(parts, &View::thumbnail(size))
}

/// Part of the picture left around a framed box, on each side.
const MARGIN: f32 = 0.06;

/// The meshes, each in its colour (or their faces' painted colours), as `view` sees them
/// with a parallel projection: a `width`×`height` RGBA image. Drawn at twice the size and
/// averaged, for smooth edges.
pub fn render_view(parts: &[(&Mesh, Rgba)], view: &View) -> Vec<u8> {
    let (w, h) = (view.width as usize, view.height as usize);
    let (sw, sh) = (w * 2, h * 2);
    let fill = view
        .background
        .map_or([0; 4], |[r, g, b, _]| [r, g, b, 255]);
    let b = parts
        .iter()
        .fold(Aabb::empty(), |acc, (m, _)| acc.union(m.aabb()));
    if !b.is_valid() || parts.iter().all(|(m, _)| m.triangles.is_empty()) {
        return fill.repeat(w * h);
    }
    let mut rgba = fill.repeat(sw * sh);
    let dir = view.dir();
    let centre = view.frame.filter(Aabb::is_valid).unwrap_or(b).center();
    let look = Mat4::look_at_rh(centre + dir, centre, view.up());
    // Where the picture's middle is in view space, and pixels per mm.
    let (mid, scale) = match view.frame.filter(Aabb::is_valid) {
        Some(f) => {
            let mut lo = Vec3::splat(f32::INFINITY);
            let mut hi = Vec3::splat(f32::NEG_INFINITY);
            for i in 0..8 {
                let corner = Vec3::select(
                    glam::BVec3::new(i & 1 != 0, i & 2 != 0, i & 4 != 0),
                    f.max,
                    f.min,
                );
                let v = look.transform_point3(corner);
                lo = lo.min(v);
                hi = hi.max(v);
            }
            let ext = (hi - lo).max(Vec3::splat(1e-3));
            let room = 1.0 - 2.0 * MARGIN;
            let scale = (sw as f32 * room / ext.x).min(sh as f32 * room / ext.y);
            ((lo + hi) * 0.5, scale)
        }
        None => {
            let radius = (b.size().length() * 0.5).max(1e-6);
            (Vec3::ZERO, sw.min(sh) as f32 * 0.46 / radius)
        }
    };
    let light = Vec3::new(-0.35, -0.55, 0.75).normalize();
    let mut depth = vec![f32::NEG_INFINITY; sw * sh];
    for (mesh, color) in parts {
        let pts: Vec<Vec3> = mesh
            .positions
            .iter()
            .map(|p| {
                let v = look.transform_point3(*p) - mid;
                Vec3::new(
                    sw as f32 * 0.5 + v.x * scale,
                    sh as f32 * 0.5 - v.y * scale,
                    v.z,
                )
            })
            .collect();
        for (ti, t) in mesh.triangles.iter().enumerate() {
            let color = mesh.face_color(ti).unwrap_or(*color);
            let [a, bb, c] = t.map(|i| pts[i as usize]);
            let [wa, wb, wc] = t.map(|i| mesh.positions[i as usize]);
            let mut n = (wb - wa).cross(wc - wa).normalize_or_zero();
            // Two-sided: light the side that faces the viewer.
            if n.dot(dir) < 0.0 {
                n = -n;
            }
            let k = 0.42 + 0.58 * n.dot(light).max(0.0);
            let shade = [0, 1, 2].map(|i| (color[i] as f32 * k).min(255.0) as u8);
            let area = (bb.x - a.x) * (c.y - a.y) - (c.x - a.x) * (bb.y - a.y);
            if area.abs() < 1e-9 {
                continue;
            }
            let (x0, x1) = (
                a.x.min(bb.x).min(c.x).floor().max(0.0) as usize,
                (a.x.max(bb.x).max(c.x).ceil() as usize).min(sw - 1),
            );
            let (y0, y1) = (
                a.y.min(bb.y).min(c.y).floor().max(0.0) as usize,
                (a.y.max(bb.y).max(c.y).ceil() as usize).min(sh - 1),
            );
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                    let w0 = ((bb.x - px) * (c.y - py) - (c.x - px) * (bb.y - py)) / area;
                    let w1 = ((c.x - px) * (a.y - py) - (a.x - px) * (c.y - py)) / area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                        continue;
                    }
                    // View-space z grows towards the camera.
                    let z = w0 * a.z + w1 * bb.z + w2 * c.z;
                    let i = y * sw + x;
                    if z > depth[i] {
                        depth[i] = z;
                        rgba[i * 4..i * 4 + 4]
                            .copy_from_slice(&[shade[0], shade[1], shade[2], 255]);
                    }
                }
            }
        }
    }
    // Average each 2×2 block.
    let mut out = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0u32; 4];
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let i = ((2 * y + dy) * sw + 2 * x + dx) * 4;
                let a = rgba[i + 3] as u32;
                for c in 0..3 {
                    acc[c] += rgba[i + c] as u32 * a;
                }
                acc[3] += a;
            }
            let o = (y * w + x) * 4;
            for c in 0..3 {
                out[o + c] = acc[c].checked_div(acc[3]).unwrap_or(0) as u8;
            }
            out[o + 3] = (acc[3] / 4) as u8;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::Primitive;

    #[test]
    fn a_cube_fills_the_middle_and_leaves_the_corners_clear() {
        let px = render(&Primitive::Cube.build(), [0, 160, 232, 255], 64);
        assert_eq!(px.len(), 64 * 64 * 4);
        let at = |x: usize, y: usize| &px[(y * 64 + x) * 4..(y * 64 + x) * 4 + 4];
        assert_eq!(at(32, 32)[3], 255);
        assert_eq!(at(0, 0)[3], 0);
        assert_eq!(at(63, 63)[3], 0);
        // Lit faces differ, so the cube reads as 3D.
        let shades: std::collections::HashSet<u8> =
            (8..56).map(|y| at(32, y)[2]).filter(|b| *b > 0).collect();
        assert!(shades.len() >= 2, "{shades:?}");
    }

    #[test]
    fn an_empty_mesh_gives_a_clear_picture() {
        let px = render(&Mesh::new(Vec::new(), Vec::new()), [255; 4], 16);
        assert!(px.iter().all(|&b| b == 0));
    }
}
