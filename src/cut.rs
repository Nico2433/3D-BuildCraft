//! Plane cut ("Split"): splits a mesh with a horizontal plane and caps both halves, so
//! a closed mesh gives two closed meshes.

use crate::mesh::Mesh;
use glam::{Vec2, Vec3};
use std::collections::HashMap;

/// Which halves "Split" keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keep {
    Top,
    Bottom,
    Both,
}

/// Splits `mesh` (world coordinates) at height `z`. Returns (above, below); either can be
/// empty. Only closed cut contours are capped, so an open input stays open.
// shortcut: horizontal planes only; rotate the object first for other angles. A rotatable
// plane comes with the cut gizmo.
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
