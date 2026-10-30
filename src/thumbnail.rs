//! Small pictures of a mesh for tiles (recent files, libraries, the selection pane),
//! drawn on the CPU so they need no GPU work and look the same everywhere.

use crate::io::Rgba;
use crate::mesh::Mesh;
use glam::{Mat4, Vec3};

/// One mesh in one colour; see [`render_parts`].
pub fn render(mesh: &Mesh, color: Rgba, size: u32) -> Vec<u8> {
    render_parts(&[(mesh, color)], size)
}

/// The meshes, each in its colour, seen from the front left and above, filling a
/// `size`×`size` RGBA image with a transparent background. Drawn at twice the size and
/// averaged, for smooth edges.
pub fn render_parts(parts: &[(&Mesh, Rgba)], size: u32) -> Vec<u8> {
    let s = (size * 2) as usize;
    let mut rgba = vec![0u8; s * s * 4];
    let b = parts
        .iter()
        .fold(crate::mesh::Aabb::empty(), |acc, (m, _)| {
            acc.union(m.aabb())
        });
    if !b.is_valid() || parts.iter().all(|(m, _)| m.triangles.is_empty()) {
        return vec![0; (size * size * 4) as usize];
    }
    let centre = b.center();
    let radius = (b.size().length() * 0.5).max(1e-6);
    let (yaw, pitch) = (-60f32.to_radians(), 28f32.to_radians());
    let dir = Vec3::new(
        pitch.cos() * yaw.cos(),
        pitch.cos() * yaw.sin(),
        pitch.sin(),
    );
    let view = Mat4::look_at_rh(centre + dir * radius * 3.0, centre, Vec3::Z);
    let scale = s as f32 * 0.46 / radius;
    let light = Vec3::new(-0.35, -0.55, 0.75).normalize();
    let mut depth = vec![f32::NEG_INFINITY; s * s];
    for (mesh, color) in parts {
        let pts: Vec<Vec3> = mesh
            .positions
            .iter()
            .map(|p| {
                let v = view.transform_point3(*p);
                Vec3::new(
                    s as f32 * 0.5 + v.x * scale,
                    s as f32 * 0.5 - v.y * scale,
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
                (a.x.max(bb.x).max(c.x).ceil() as usize).min(s - 1),
            );
            let (y0, y1) = (
                a.y.min(bb.y).min(c.y).floor().max(0.0) as usize,
                (a.y.max(bb.y).max(c.y).ceil() as usize).min(s - 1),
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
                    let i = y * s + x;
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
    let n = size as usize;
    let mut out = vec![0u8; n * n * 4];
    for y in 0..n {
        for x in 0..n {
            let mut acc = [0u32; 4];
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let i = ((2 * y + dy) * s + 2 * x + dx) * 4;
                let a = rgba[i + 3] as u32;
                for c in 0..3 {
                    acc[c] += rgba[i + c] as u32 * a;
                }
                acc[3] += a;
            }
            let o = (y * n + x) * 4;
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
