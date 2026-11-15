//! Conform to the surface (OrcaSlicer's "Use surface"): each point of the pattern rises
//! (or sinks) along the surface normal where its ray meets the object, instead of along
//! the ray, so the pattern keeps an even height over curved surfaces. The normal is
//! smoothed across the corners of each triangle, but not across sharp edges, so a flat
//! face keeps its own normal.

use crate::mesh::Mesh;
use glam::Vec3;
use std::collections::HashMap;

/// Faces that bend more than this from a triangle do not smooth its corner normals.
const CREASE_DEG: f32 = 60.0;

/// Smoothed normals at the corners of some triangles of a mesh.
pub(super) struct Normals {
    corners: HashMap<u32, [Vec3; 3]>,
}

impl Normals {
    /// Corner normals of `triangles` (indices into `mesh.triangles`, repeats allowed).
    pub(super) fn new(mesh: &Mesh, triangles: impl Iterator<Item = u32>) -> Self {
        let wanted: Vec<u32> = {
            let mut t: Vec<u32> = triangles.collect();
            t.sort_unstable();
            t.dedup();
            t
        };
        let face = |f: usize| {
            let [a, b, c] = mesh.triangles[f].map(|v| mesh.positions[v as usize]);
            (b - a).cross(c - a)
        };
        // Faces around each corner of the wanted triangles, from one pass over the mesh.
        let mut around: HashMap<u32, Vec<u32>> = wanted
            .iter()
            .flat_map(|&f| mesh.triangles[f as usize])
            .map(|v| (v, Vec::new()))
            .collect();
        for (f, t) in mesh.triangles.iter().enumerate() {
            for v in t {
                if let Some(faces) = around.get_mut(v) {
                    faces.push(f as u32);
                }
            }
        }
        let cos = CREASE_DEG.to_radians().cos();
        let corners = wanted
            .iter()
            .map(|&f| {
                let own = face(f as usize);
                let unit = own.normalize_or_zero();
                let n = mesh.triangles[f as usize].map(|v| {
                    around[&v]
                        .iter()
                        .map(|&g| face(g as usize))
                        .filter(|n| n.normalize_or_zero().dot(unit) >= cos)
                        .sum::<Vec3>()
                        .try_normalize()
                        .unwrap_or(unit)
                });
                (f, n)
            })
            .collect();
        Self { corners }
    }

    /// The direction into the object at `p` on triangle `f`, hit by a ray going `dir`:
    /// against the smoothed normal there. A normal that faces away from the ray (a
    /// triangle seen from inside) leaves the ray's direction.
    pub(super) fn inward(&self, mesh: &Mesh, f: u32, p: Vec3, dir: Vec3) -> Vec3 {
        let Some(corner) = self.corners.get(&f) else {
            return dir;
        };
        let [a, b, c] = mesh.triangles[f as usize].map(|v| mesh.positions[v as usize]);
        let w = barycentric(p, a, b, c);
        let n = (corner[0] * w.x + corner[1] * w.y + corner[2] * w.z).normalize_or_zero();
        if n.dot(-dir) > 0.05 {
            -n
        } else {
            dir
        }
    }
}

/// Weights of `a`, `b` and `c` for `p` in their plane.
fn barycentric(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Vec3 {
    let (ab, ac, ap) = (b - a, c - a, p - a);
    let (d00, d01, d11) = (ab.dot(ab), ab.dot(ac), ac.dot(ac));
    let (d20, d21) = (ap.dot(ab), ap.dot(ac));
    let det = d00 * d11 - d01 * d01;
    if det.abs() < 1e-20 {
        return Vec3::splat(1.0 / 3.0);
    }
    let v = (d11 * d20 - d01 * d21) / det;
    let w = (d00 * d21 - d01 * d20) / det;
    Vec3::new(1.0 - v - w, v, w)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::Primitive;

    #[test]
    fn flat_faces_keep_their_normal_and_curved_ones_are_smoothed() {
        let cube = Primitive::Cube.build();
        let all = Normals::new(&cube, 0..cube.triangles.len() as u32);
        for (f, t) in cube.triangles.iter().enumerate() {
            let [a, b, c] = t.map(|v| cube.positions[v as usize]);
            let n = Mesh::face_normal(a, b, c);
            let p = (a + b + c) / 3.0;
            assert!(all.inward(&cube, f as u32, p, -n).abs_diff_eq(-n, 1e-4));
        }
        // On a sphere the smoothed normal at a point is close to the radius through it.
        let sphere = Primitive::Sphere.build();
        let centre = Vec3::new(0.0, 0.0, 20.0);
        let all = Normals::new(&sphere, 0..sphere.triangles.len() as u32);
        for (f, t) in sphere.triangles.iter().enumerate().step_by(37) {
            let [a, b, c] = t.map(|v| sphere.positions[v as usize]);
            let p = a * 0.2 + b * 0.3 + c * 0.5;
            let radial = (p - centre).normalize();
            let into = all.inward(&sphere, f as u32, p, -radial);
            assert!(into.dot(-radial) > 0.995, "{into} vs {radial}");
        }
    }
}
