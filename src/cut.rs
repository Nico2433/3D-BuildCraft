//! Plane cut ("Split"): splits a mesh with a plane and caps both halves, so a closed mesh
//! gives two closed meshes.

use crate::mesh::Mesh;
use glam::{EulerRot, Mat4, Quat, Vec2, Vec3};
use std::collections::HashMap;

/// Which halves "Split" keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keep {
    Top,
    Bottom,
    Both,
}

/// The cutting plane of the Split tool: through `center`, moved `offset` along its normal,
/// turned by `rotation` degrees (roll about X, pitch about Y, yaw about Z, as an object's).
/// Unturned, the normal is +Z.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CutPlane {
    pub center: Vec3,
    pub offset: f32,
    pub rotation: Vec3,
}

impl CutPlane {
    /// The horizontal plane at height `z`.
    pub fn at_height(z: f32) -> Self {
        Self {
            center: Vec3::new(0.0, 0.0, z),
            offset: 0.0,
            rotation: Vec3::ZERO,
        }
    }

    pub fn quat(&self) -> Quat {
        let r = self.rotation * (std::f32::consts::PI / 180.0);
        Quat::from_euler(EulerRot::ZYX, r.z, r.y, r.x)
    }

    pub fn normal(&self) -> Vec3 {
        self.quat() * Vec3::Z
    }

    /// The plane's middle.
    pub fn point(&self) -> Vec3 {
        self.center + self.normal() * self.offset
    }

    /// Turns the plane by `q` (world space) about its center.
    pub fn turn(&mut self, q: Quat) {
        let (z, y, x) = (q * self.quat()).normalize().to_euler(EulerRot::ZYX);
        self.rotation = Vec3::new(x, y, z) * (180.0 / std::f32::consts::PI);
    }
}

/// Splits `mesh` (world coordinates) by `plane`. Returns (above, below), above being the
/// side the normal points to; either can be empty.
pub fn cut_plane(mesh: &Mesh, plane: &CutPlane) -> (Mesh, Mesh) {
    // Turn the plane level, cut there and turn the halves back.
    let q = plane.quat();
    let level = Mat4::from_quat(q.inverse());
    let z = level.transform_point3(plane.point()).z;
    let back = Mat4::from_quat(q);
    let (top, bottom) = cut(&mesh.transformed(&level), z);
    (
        top.transformed(&back).welded(),
        bottom.transformed(&back).welded(),
    )
}

/// Splits `mesh` (world coordinates) at height `z`. Returns (above, below); either can be
/// empty. Only closed cut contours are capped, so an open input stays open.
pub fn cut(mesh: &Mesh, z: f32) -> (Mesh, Mesh) {
    let mut positions = mesh.positions.clone();
    let above: Vec<bool> = positions.iter().map(|p| p.z >= z).collect();
    let mut crossing: HashMap<(u32, u32), u32> = HashMap::new();
    let mut point_on = |a: u32, b: u32, positions: &mut Vec<Vec3>| {
        *crossing.entry((a.min(b), a.max(b))).or_insert_with(|| {
            let (pa, pb) = (positions[a as usize], positions[b as usize]);
            let mut p = pa.lerp(pb, (z - pa.z) / (pb.z - pa.z));
            p.z = z;
            positions.push(p);
            (positions.len() - 1) as u32
        })
    };

    let (mut top, mut bottom) = (Vec::new(), Vec::new());
    // Cap edges, directed the way the top half's cap must run.
    let mut cap_next: HashMap<u32, u32> = HashMap::new();
    for t in &mesh.triangles {
        let side = t.map(|i| above[i as usize]);
        if side[0] == side[1] && side[1] == side[2] {
            if side[0] { &mut top } else { &mut bottom }.push(*t);
            continue;
        }
        // Rotate (keeping the winding) so the vertex alone on its side comes first.
        let k = (0..3)
            .find(|&k| side[(k + 1) % 3] == side[(k + 2) % 3])
            .unwrap();
        let [l, x, y] = [t[k], t[(k + 1) % 3], t[(k + 2) % 3]];
        let (i1, i2) = (
            point_on(l, x, &mut positions),
            point_on(l, y, &mut positions),
        );
        let (lone, rest) = if side[k] {
            (&mut top, &mut bottom)
        } else {
            (&mut bottom, &mut top)
        };
        lone.push([l, i1, i2]);
        rest.extend([[i1, x, y], [i1, y, i2]]);
        // The lone half has the edge i1→i2, so its cap needs i2→i1, and the other way round.
        if side[k] {
            cap_next.insert(i2, i1);
        } else {
            cap_next.insert(i1, i2);
        }
    }

    for tri in cap(&positions, cap_next) {
        top.push(tri);
        bottom.push([tri[0], tri[2], tri[1]]);
    }
    let half = |tris: Vec<[u32; 3]>| Mesh::new(positions.clone(), tris).welded();
    (half(top), half(bottom))
}

/// Triangulates the closed contours of a cut, facing down (the top half's cap).
fn cap(positions: &[Vec3], mut next: HashMap<u32, u32>) -> Vec<[u32; 3]> {
    let mut loops: Vec<Vec<u32>> = Vec::new();
    while let Some(&start) = next.keys().next() {
        let mut ring = vec![start];
        let mut cur = start;
        while let Some(n) = next.remove(&cur) {
            if n == start {
                loops.push(std::mem::take(&mut ring));
                break;
            }
            ring.push(n);
            cur = n;
        }
    }
    fill(positions, &loops)
}

/// Triangulates closed loops of `positions` in the XY plane, holes included (a loop
/// inside an odd number of others is a hole). Triangles face down (−Z).
pub(crate) fn fill(positions: &[Vec3], loops: &[Vec<u32>]) -> Vec<[u32; 3]> {
    let loops: Vec<&Vec<u32>> = loops.iter().filter(|l| l.len() >= 3).collect();
    let flat = |l: &[u32]| -> Vec<Vec2> {
        l.iter()
            .map(|&i| positions[i as usize].truncate())
            .collect()
    };
    let polys: Vec<Vec<Vec2>> = loops.iter().map(|l| flat(l)).collect();
    // A contour inside an odd number of others is a hole.
    let depth: Vec<usize> = (0..polys.len())
        .map(|i| {
            (0..polys.len())
                .filter(|&j| j != i && contains(&polys[j], polys[i][0]))
                .count()
        })
        .collect();

    let mut out = Vec::new();
    for (o, outer) in polys.iter().enumerate().filter(|(o, _)| depth[*o] % 2 == 0) {
        let holes: Vec<usize> = (0..polys.len())
            .filter(|&h| depth[h] == depth[o] + 1 && contains(outer, polys[h][0]))
            .collect();
        let mut ids: Vec<u32> = loops[o].to_vec();
        let mut coords: Vec<f64> = outer
            .iter()
            .flat_map(|p| [p.x as f64, p.y as f64])
            .collect();
        let mut starts = Vec::new();
        for &h in &holes {
            starts.push(ids.len());
            ids.extend(loops[h].iter());
            coords.extend(polys[h].iter().flat_map(|p| [p.x as f64, p.y as f64]));
        }
        let Ok(tris) = earcutr::earcut(&coords, &starts, 2) else {
            continue;
        };
        let tris: Vec<[u32; 3]> = tris
            .chunks_exact(3)
            .map(|c| [ids[c[0]], ids[c[1]], ids[c[2]]])
            .collect();
        // earcut winds all triangles alike; decide once from the total area, since the
        // sliver triangles along hole bridges have no reliable sign of their own.
        let up: f32 = tris
            .iter()
            .map(|t| {
                let [a, b, d] = t.map(|i| positions[i as usize]);
                (b - a).cross(d - a).z
            })
            .sum();
        out.extend(
            tris.into_iter()
                .map(|t| if up > 0.0 { [t[0], t[2], t[1]] } else { t }),
        );
    }
    out
}

/// Even-odd point-in-polygon test.
fn contains(poly: &[Vec2], p: Vec2) -> bool {
    let mut inside = false;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            inside = !inside;
        }
    }
    inside
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::{self, Primitive};

    fn closed_volume(m: &Mesh) -> f64 {
        let s = m.stats();
        assert!(s.is_closed_manifold(), "{s:?}");
        s.volume
    }

    #[test]
    fn cube_halves_are_closed_with_the_right_volumes() {
        let (top, bottom) = cut(&primitives::cube(10.0), 4.0);
        assert!((closed_volume(&top) - 600.0).abs() < 1e-3);
        assert!((closed_volume(&bottom) - 400.0).abs() < 1e-3);
    }

    #[test]
    fn oblique_cut_splits_a_cube_through_its_center_in_halves() {
        let cube = primitives::cube(10.0);
        let mut plane = CutPlane::at_height(0.0);
        plane.center = cube.aabb().center();
        plane.turn(Quat::from_rotation_y(30f32.to_radians()));
        let (top, bottom) = cut_plane(&cube, &plane);
        assert!((closed_volume(&top) - 500.0).abs() < 1e-2);
        assert!((closed_volume(&bottom) - 500.0).abs() < 1e-2);
        // The upper half lies on the normal's side.
        let n = plane.normal();
        let side = |m: &Mesh| {
            m.positions
                .iter()
                .map(|p| (*p - plane.point()).dot(n))
                .sum::<f32>()
        };
        assert!(side(&top) > 0.0 && side(&bottom) < 0.0);
        // Moving the plane along its normal takes a slice off one side only.
        plane.offset = 2.0;
        let (top, bottom) = cut_plane(&cube, &plane);
        assert!(closed_volume(&top) < 500.0 && closed_volume(&bottom) > 500.0);
        assert!((closed_volume(&top) + closed_volume(&bottom) - 1000.0).abs() < 1e-2);
    }

    #[test]
    fn torus_cut_caps_a_ring_with_a_hole() {
        let m = Primitive::Torus.build();
        let z = m.aabb().center().z + 0.37;
        let (top, bottom) = cut(&m, z);
        let total = m.stats().volume;
        assert!((closed_volume(&top) + closed_volume(&bottom) - total).abs() / total < 1e-4);
    }

    #[test]
    fn every_primitive_cuts_into_closed_halves() {
        for p in Primitive::ALL {
            let m = p.build();
            let (top, bottom) = cut(&m, 13.3);
            let total = m.stats().volume;
            let sum: f64 = [&top, &bottom]
                .iter()
                .filter(|h| !h.is_empty())
                .map(|h| closed_volume(h))
                .sum();
            assert!(
                (sum - total).abs() / total < 1e-4,
                "{p:?}: {sum} vs {total}"
            );
        }
    }

    #[test]
    fn plane_outside_the_mesh_keeps_it_whole() {
        let m = primitives::cube(10.0);
        let (top, bottom) = cut(&m, 50.0);
        assert!(top.is_empty());
        assert_eq!(bottom.stats(), m.welded().stats());
    }
}
