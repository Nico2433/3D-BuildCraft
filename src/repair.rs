//! Mesh repair: makes a mesh closed, edge-manifold and consistently outward-facing with
//! as little change as possible, like the original's repair (finding H35: it removes
//! stray faces, fixes orientation and closes holes without remeshing).

use crate::mesh::Mesh;
use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RepairReport {
    pub removed_degenerate: usize,
    pub removed_duplicate: usize,
    pub removed_nonmanifold: usize,
    /// Vertices duplicated to separate surfaces that touch along an edge.
    pub split_vertices: usize,
    pub flipped: usize,
    pub holes_filled: usize,
    /// Boundary loops that could not be traced as a simple cycle.
    pub holes_left: usize,
}

pub fn repair(mesh: &Mesh) -> (Mesh, RepairReport) {
    let mut r = RepairReport::default();
    let mut m = mesh.welded();
    r.removed_degenerate = mesh.triangles.len() - m.triangles.len();

    let welded_count = m.triangles.len();
    let mut seen = HashSet::with_capacity(welded_count);
    m.triangles.retain(|t| seen.insert(sorted(t)));
    r.removed_duplicate = welded_count - m.triangles.len();

    let winding: HashMap<[u32; 3], [u32; 3]> = m
        .triangles
        .iter()
        .map(|t| (sorted(t), canonical(t)))
        .collect();
    // A fill can land on an existing edge, so repeat until clean (twice is typical).
    for _ in 0..4 {
        orient(&mut m);
        r.removed_nonmanifold += remove_conflicting_faces(&mut m);
        r.split_vertices += split_touching_sheets(&mut m);
        r.removed_nonmanifold += remove_open_patches(&mut m);
        let (filled, left) = fill_holes(&mut m);
        r.holes_filled += filled;
        r.holes_left = left;
        orient(&mut m);
        if m.stats().is_closed_manifold() {
            break;
        }
    }
    r.flipped = m
        .triangles
        .iter()
        .filter(|t| winding.get(&sorted(t)).is_some_and(|w| *w != canonical(t)))
        .count();
    (m, r)
}

fn edge_faces(tris: &[[u32; 3]]) -> HashMap<(u32, u32), Vec<usize>> {
    let mut map: HashMap<(u32, u32), Vec<usize>> = HashMap::with_capacity(tris.len() * 3 / 2);
    for (f, t) in tris.iter().enumerate() {
        for (a, b) in edges(t) {
            map.entry((a.min(b), a.max(b))).or_default().push(f);
        }
    }
    map
}

fn sorted(t: &[u32; 3]) -> [u32; 3] {
    let mut k = *t;
    k.sort_unstable();
    k
}

/// Same triangle and winding gives the same value, whatever vertex it starts at.
fn canonical(t: &[u32; 3]) -> [u32; 3] {
    let i = (0..3).min_by_key(|&i| t[i]).unwrap();
    [t[i], t[(i + 1) % 3], t[(i + 2) % 3]]
}

fn edges(t: &[u32; 3]) -> [(u32, u32); 3] {
    [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])]
}

/// How many of an edge's faces can stay: all of an even count (surfaces touching along
/// the edge, split apart later), one when two faces run the edge the same way after
/// orienting (a fold no winding can fix), otherwise two.
fn allowed(tris: &[[u32; 3]], (a, b): (u32, u32), faces: &[usize]) -> usize {
    match faces {
        [f, g] if has_directed(&tris[*f], a, b) == has_directed(&tris[*g], a, b) => 1,
        _ if faces.len() > 2 && faces.len() % 2 == 0 => faces.len(),
        _ => 2,
    }
}

/// Gives each surface that meets at a non-manifold edge its own copy of the edge's
/// vertices. Around a vertex, faces joined by ordinary edges form one surface.
fn split_touching_sheets(m: &mut Mesh) -> usize {
    let map = edge_faces(&m.triangles);
    let mut around: HashMap<u32, Vec<usize>> = HashMap::new();
    for (e, faces) in &map {
        if faces.len() > 2 {
            for v in [e.0, e.1] {
                around.entry(v).or_default();
            }
        }
    }
    for (f, t) in m.triangles.iter().enumerate() {
        for v in t {
            if let Some(list) = around.get_mut(v) {
                list.push(f);
            }
        }
    }
    let mut splits: Vec<(u32, Vec<usize>)> = Vec::new();
    for (&v, faces) in &around {
        // Group the faces around `v` through the ordinary edges they share at `v`.
        let mut group: HashMap<usize, usize> = faces.iter().map(|&f| (f, f)).collect();
        fn root(g: &mut HashMap<usize, usize>, mut f: usize) -> usize {
            while g[&f] != f {
                f = g[&f];
            }
            f
        }
        for &f in faces {
            for (a, b) in edges(&m.triangles[f]) {
                let w = if a == v {
                    b
                } else if b == v {
                    a
                } else {
                    continue;
                };
                let shared = &map[&(v.min(w), v.max(w))];
                if shared.len() == 2 {
                    let (x, y) = (root(&mut group, shared[0]), root(&mut group, shared[1]));
                    group.insert(x, y);
                }
            }
        }
        let mut by_root: HashMap<usize, Vec<usize>> = HashMap::new();
        for &f in faces {
            by_root.entry(root(&mut group, f)).or_default().push(f);
        }
        splits.extend(by_root.into_values().skip(1).map(|fs| (v, fs)));
    }
    for (v, faces) in &splits {
        let copy = m.positions.len() as u32;
        m.positions.push(m.positions[*v as usize]);
        for &f in faces {
            for i in &mut m.triangles[f] {
                if i == v {
                    *i = copy;
                }
            }
        }
    }
    splits.len()
}

/// Drops the faces that break an edge, keeping the ones with the fewest broken edges: a
/// stray face over a surface touches several broken or open edges, the real ones do not.
fn remove_conflicting_faces(m: &mut Mesh) -> usize {
    let mut removed = 0;
    loop {
        let map = edge_faces(&m.triangles);
        let broken = |e: &(u32, u32), faces: &Vec<usize>| {
            faces.len() != 2 || allowed(&m.triangles, *e, faces) < 2
        };
        let bad = |f: usize| {
            edges(&m.triangles[f])
                .iter()
                .filter(|(a, b)| {
                    let e = ((*a).min(*b), (*a).max(*b));
                    broken(&e, &map[&e])
                })
                .count()
        };
        let mut drop = HashSet::new();
        for (e, faces) in &map {
            let keep = allowed(&m.triangles, *e, faces);
            if faces.len() <= keep {
                continue;
            }
            let mut fs: Vec<usize> = faces
                .iter()
                .copied()
                .filter(|f| !drop.contains(f))
                .collect();
            // Ties go to the later face: patches are usually appended at the end of a file.
            fs.sort_by_key(|&f| (Reverse(bad(f)), Reverse(f)));
            let extra = fs.len().saturating_sub(keep);
            drop.extend(fs.into_iter().take(extra));
        }
        if drop.is_empty() {
            return removed;
        }
        removed += drop.len();
        let mut i = 0;
        m.triangles.retain(|_| {
            i += 1;
            !drop.contains(&(i - 1))
        });
    }
}

fn has_directed(t: &[u32; 3], a: u32, b: u32) -> bool {
    edges(t).contains(&(a, b))
}

/// Makes neighbouring faces agree on winding, then turns every component outward.
/// Returns the components: faces joined through edges shared by exactly two faces.
fn orient(m: &mut Mesh) -> Vec<Vec<usize>> {
    let map = edge_faces(&m.triangles);
    let n = m.triangles.len();
    let mut seen = vec![false; n];
    let mut components = Vec::new();
    for start in 0..n {
        if seen[start] {
            continue;
        }
        seen[start] = true;
        let mut component = vec![start];
        let mut stack = vec![start];
        while let Some(f) = stack.pop() {
            for (a, b) in edges(&m.triangles[f]) {
                let faces = &map[&(a.min(b), a.max(b))];
                if faces.len() != 2 {
                    continue;
                }
                let g = if faces[0] == f { faces[1] } else { faces[0] };
                if seen[g] {
                    continue;
                }
                if has_directed(&m.triangles[g], a, b) {
                    m.triangles[g].swap(1, 2);
                }
                seen[g] = true;
                component.push(g);
                stack.push(g);
            }
        }
        let volume: f64 = component
            .iter()
            .map(|&f| {
                let [a, b, c] = m.triangles[f].map(|i| m.positions[i as usize].as_dvec3());
                a.dot(b.cross(c))
            })
            .sum();
        if volume < 0.0 {
            for &f in &component {
                m.triangles[f].swap(1, 2);
            }
        }
        components.push(component);
    }
    components
}

/// Removes small pieces that are not closed and hang off the main surface through a
/// vertex or a broken edge: their winding cannot be decided, so no fill closes them.
// shortcut: "small" is under 1% of the largest piece; a user-chosen fragment size
// comes with the Repair options screen.
fn remove_open_patches(m: &mut Mesh) -> usize {
    let components = orient(m);
    if components.len() < 2 {
        return 0;
    }
    let largest = components.iter().map(Vec::len).max().unwrap_or(0);
    let map = edge_faces(&m.triangles);
    let mut drop = HashSet::new();
    for c in components.iter().filter(|c| c.len() * 100 < largest) {
        let open = c.iter().any(|&f| {
            edges(&m.triangles[f]).iter().any(|(a, b)| {
                let e = ((*a).min(*b), (*a).max(*b));
                map[&e].len() != 2 || allowed(&m.triangles, e, &map[&e]) < 2
            })
        });
        if open {
            drop.extend(c.iter().copied());
        }
    }
    let mut i = 0;
    m.triangles.retain(|_| {
        i += 1;
        !drop.contains(&(i - 1))
    });
    drop.len()
}

/// Closes every boundary loop with a fan of triangles. Returns (filled, left open).
/// A boundary can pass through a vertex more than once (a pinch); the walk splits it into
/// simple loops whenever it comes back to a vertex already on its path.
// shortcut: fan triangulation is fine for the small holes scans and exports leave;
// large or non-planar holes need ear clipping or a minimal-area fill.
fn fill_holes(m: &mut Mesh) -> (usize, usize) {
    let map = edge_faces(&m.triangles);
    // A hole edge runs opposite to the open edge of the face beside it.
    let mut out: HashMap<u32, Vec<u32>> = HashMap::new();
    for faces in map.values().filter(|f| f.len() == 1) {
        for (a, b) in edges(&m.triangles[faces[0]]) {
            if map[&(a.min(b), a.max(b))].len() == 1 {
                out.entry(b).or_default().push(a);
            }
        }
    }
    let (mut filled, mut left) = (0, 0);
    let starts: Vec<u32> = out.keys().copied().collect();
    for start in starts {
        let mut path = vec![start];
        let mut on_path = HashMap::from([(start, 0usize)]);
        loop {
            let cur = *path.last().unwrap();
            let Some(n) = out.get_mut(&cur).and_then(Vec::pop) else {
                if path.len() > 1 {
                    left += 1;
                }
                break;
            };
            let Some(&i) = on_path.get(&n) else {
                on_path.insert(n, path.len());
                path.push(n);
                continue;
            };
            let ring = &path[i..];
            let n = ring.len();
            if n >= 3 {
                // Fan from a corner whose diagonals are not edges already, so the fill
                // does not create a non-manifold edge.
                let free = |k: usize| {
                    (2..n - 1).all(|j| {
                        let (a, b) = (ring[k], ring[(k + j) % n]);
                        !map.contains_key(&(a.min(b), a.max(b)))
                    })
                };
                let k = (0..n).find(|&k| free(k)).unwrap_or(0);
                for j in 1..n - 1 {
                    m.triangles
                        .push([ring[k], ring[(k + j) % n], ring[(k + j + 1) % n]]);
                }
                filled += 1;
            }
            for v in path.drain(i + 1..) {
                on_path.remove(&v);
            }
        }
    }
    (filled, left)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives;
    use glam::Vec3;

    fn assert_clean(m: &Mesh, volume: f64) {
        let s = m.stats();
        assert!(s.is_closed_manifold(), "{s:?}");
        assert!((s.volume - volume).abs() < 1e-3, "volume {}", s.volume);
    }

    #[test]
    fn fills_a_missing_face() {
        let mut m = primitives::cube(10.0);
        m.triangles.truncate(10);
        let (fixed, r) = repair(&m);
        assert_eq!(r.holes_filled, 1);
        assert_clean(&fixed, 1000.0);
    }

    #[test]
    fn fixes_a_flipped_face_and_an_inside_out_mesh() {
        let mut m = primitives::cube(10.0);
        m.triangles[3].swap(1, 2);
        assert_clean(&repair(&m).0, 1000.0);
        m.flip();
        assert_clean(&repair(&m).0, 1000.0);
    }

    #[test]
    fn removes_a_stray_face_over_the_surface() {
        // Like the camel: one extra triangle on an existing edge, with its other edges open.
        let mut m = primitives::cube(10.0);
        let p = m.positions.len() as u32;
        m.positions.push(Vec3::new(0.0, 0.0, 20.0));
        m.triangles.push([0, 2, p]);
        let (fixed, r) = repair(&m);
        assert_eq!(r.removed_nonmanifold, 1);
        assert_clean(&fixed, 1000.0);
    }

    #[test]
    fn separates_cubes_touching_along_an_edge() {
        let mut m = primitives::cube(10.0);
        let mut other = primitives::cube(10.0);
        for p in &mut other.positions {
            *p += Vec3::new(10.0, 10.0, 0.0);
        }
        m.append(&other);
        let m = m.welded();
        assert_eq!(m.stats().nonmanifold_edges, 1);
        let (fixed, r) = repair(&m);
        assert_eq!((r.removed_nonmanifold, r.split_vertices), (0, 2));
        assert_clean(&fixed, 2000.0);
    }

    #[test]
    fn welds_a_triangle_soup_and_drops_duplicates() {
        let cube = primitives::cube(10.0);
        let mut soup: Vec<[Vec3; 3]> = cube.triangle_positions().collect();
        soup.push(soup[0]);
        let m = Mesh::new(
            soup.iter().flatten().copied().collect(),
            (0..soup.len() as u32)
                .map(|i| [3 * i, 3 * i + 1, 3 * i + 2])
                .collect(),
        );
        let (fixed, r) = repair(&m);
        assert_eq!(r.removed_duplicate, 1);
        assert_clean(&fixed, 1000.0);
    }

    #[test]
    fn leaves_clean_meshes_untouched() {
        let m = primitives::sphere(10.0, 32, 16);
        let (fixed, r) = repair(&m);
        assert_eq!(r, RepairReport::default());
        assert_eq!(fixed.stats(), m.stats());
    }
}
