//! Indexed triangle mesh, welding, normals and topology statistics.

use glam::{Mat4, Vec3};
use std::collections::HashMap;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mesh {
    pub positions: Vec<Vec3>,
    /// Counter-clockwise when seen from outside.
    pub triangles: Vec<[u32; 3]>,
    /// RGBA per triangle, painted by area. Empty, or alpha 0 on a triangle, means the
    /// object's colour. Edits that rebuild the mesh drop it; see [`Mesh::face_color`].
    pub colors: Vec<[u8; 4]>,
}

/// Vertex data for drawing; see [`Mesh::shading`].
#[derive(Clone, Debug, Default)]
pub struct Shading {
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    /// Per vertex: a painted colour, or alpha 0 for the object's.
    pub colors: Vec<[u8; 4]>,
    pub triangles: Vec<[u32; 3]>,
    /// Creases, as pairs of vertex indices.
    pub edges: Vec<[u32; 2]>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    pub fn empty() -> Self {
        Self {
            min: Vec3::splat(f32::INFINITY),
            max: Vec3::splat(f32::NEG_INFINITY),
        }
    }
    pub fn grow(&mut self, p: Vec3) {
        self.min = self.min.min(p);
        self.max = self.max.max(p);
    }
    pub fn union(self, o: Aabb) -> Aabb {
        Aabb {
            min: self.min.min(o.min),
            max: self.max.max(o.max),
        }
    }
    pub fn is_valid(&self) -> bool {
        self.min.x <= self.max.x
    }
    pub fn size(&self) -> Vec3 {
        if self.is_valid() {
            self.max - self.min
        } else {
            Vec3::ZERO
        }
    }
    pub fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }
}

/// Same counters as `analisis/scripts/stlstats.py`, so results compare with the original app.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MeshStats {
    pub vertices: usize,
    pub triangles: usize,
    pub degenerate_triangles: usize,
    pub boundary_edges: usize,
    pub nonmanifold_edges: usize,
    pub misoriented_edges: usize,
    pub components: usize,
    pub area: f64,
    pub volume: f64,
}

impl MeshStats {
    pub fn is_closed_manifold(&self) -> bool {
        self.boundary_edges == 0 && self.nonmanifold_edges == 0 && self.misoriented_edges == 0
    }
}

impl Mesh {
    pub fn new(positions: Vec<Vec3>, triangles: Vec<[u32; 3]>) -> Self {
        Self {
            positions,
            triangles,
            colors: Vec::new(),
        }
    }

    /// Builds an indexed mesh from loose triangles, merging bit-identical vertices.
    pub fn from_triangle_soup(tris: &[[Vec3; 3]]) -> Self {
        let mut map: HashMap<[u32; 3], u32> = HashMap::with_capacity(tris.len());
        let mut positions = Vec::with_capacity(tris.len() / 2 + 3);
        let mut triangles = Vec::with_capacity(tris.len());
        for t in tris {
            let mut idx = [0u32; 3];
            for (k, p) in t.iter().enumerate() {
                let key = [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()];
                idx[k] = *map.entry(key).or_insert_with(|| {
                    positions.push(*p);
                    (positions.len() - 1) as u32
                });
            }
            triangles.push(idx);
        }
        Self {
            positions,
            triangles,
            colors: Vec::new(),
        }
    }

    /// Merges vertices with identical coordinates and drops triangles that collapse.
    pub fn welded(&self) -> Mesh {
        let soup: Vec<[Vec3; 3]> = self.triangle_positions().collect();
        let mut m = Mesh::from_triangle_soup(&soup);
        m.triangles
            .retain(|t| t[0] != t[1] && t[1] != t[2] && t[0] != t[2]);
        m
    }

    pub fn triangle_positions(&self) -> impl Iterator<Item = [Vec3; 3]> + '_ {
        self.triangles.iter().map(|t| {
            [
                self.positions[t[0] as usize],
                self.positions[t[1] as usize],
                self.positions[t[2] as usize],
            ]
        })
    }

    pub fn is_empty(&self) -> bool {
        self.triangles.is_empty()
    }

    pub fn aabb(&self) -> Aabb {
        let mut b = Aabb::empty();
        for p in &self.positions {
            b.grow(*p);
        }
        b
    }

    /// Volume centroid from signed tetrahedra, or None when the volume is near zero.
    pub fn centroid(&self) -> Option<Vec3> {
        let mut volume = 0.0;
        let mut moment = glam::DVec3::ZERO;
        for [a, b, c] in self.triangle_positions() {
            let (a, b, c) = (a.as_dvec3(), b.as_dvec3(), c.as_dvec3());
            let v = a.dot(b.cross(c)) / 6.0;
            volume += v;
            moment += (a + b + c) * (v / 4.0);
        }
        if volume.abs() < 1e-9 {
            None
        } else {
            Some((moment / volume).as_vec3())
        }
    }

    pub fn aabb_transformed(&self, m: &Mat4) -> Aabb {
        let mut b = Aabb::empty();
        for p in &self.positions {
            b.grow(m.transform_point3(*p));
        }
        b
    }

    pub fn transformed(&self, m: &Mat4) -> Mesh {
        let mut out = Mesh {
            positions: self
                .positions
                .iter()
                .map(|p| m.transform_point3(*p))
                .collect(),
            triangles: self.triangles.clone(),
            colors: self.colors.clone(),
        };
        // A mirroring transform turns the surface inside out; keep it facing outward.
        if m.determinant() < 0.0 {
            out.flip();
        }
        out
    }

    pub fn flip(&mut self) {
        for t in &mut self.triangles {
            t.swap(1, 2);
        }
    }

    pub fn append(&mut self, other: &Mesh) {
        if self.has_colors() || other.has_colors() {
            let own = self.triangles.len();
            self.colors.resize(own, [0; 4]);
            let mut theirs = other.colors.clone();
            theirs.resize(other.triangles.len(), [0; 4]);
            self.colors.extend(theirs);
        }
        let base = self.positions.len() as u32;
        self.positions.extend_from_slice(&other.positions);
        self.triangles.extend(
            other
                .triangles
                .iter()
                .map(|t| [t[0] + base, t[1] + base, t[2] + base]),
        );
    }

    /// Whether triangles carry their own colours (and the list still matches them).
    pub fn has_colors(&self) -> bool {
        self.colors.len() == self.triangles.len() && self.colors.iter().any(|c| c[3] > 0)
    }

    /// Triangle `i`'s own colour, if it was painted.
    pub fn face_color(&self, i: usize) -> Option<[u8; 4]> {
        if self.colors.len() != self.triangles.len() {
            return None;
        }
        self.colors.get(i).copied().filter(|c| c[3] > 0)
    }

    /// Triangles that touch `start` through shared edges and bend no more than `max_deg`
    /// between adjacent faces: the area "Color" paints with a given coverage.
    pub fn face_region(&self, start: usize, max_deg: f32) -> Vec<usize> {
        let unit: Vec<Vec3> = self
            .triangle_positions()
            .map(|[a, b, c]| Mesh::face_normal(a, b, c))
            .collect();
        let Some(_) = unit.get(start) else {
            return Vec::new();
        };
        let cos = max_deg.clamp(0.0, 180.0).to_radians().cos();
        // Quantize positions for topology only; preserve triangle numbering and colours.
        let tolerance = self.aabb().size().max_element().max(1.0) * 1e-6;
        let mut vertices = HashMap::new();
        let welded: Vec<u32> = self
            .positions
            .iter()
            .map(|p| {
                let key = (p / tolerance).round().as_ivec3().to_array();
                let next = vertices.len() as u32;
                *vertices.entry(key).or_insert(next)
            })
            .collect();
        let triangles: Vec<_> = self
            .triangles
            .iter()
            .map(|t| t.map(|v| welded[v as usize]))
            .collect();
        let mut by_edge: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
        for (f, t) in triangles.iter().enumerate() {
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                by_edge.entry((a.min(b), a.max(b))).or_default().push(f);
            }
        }
        let mut seen = vec![false; self.triangles.len()];
        seen[start] = true;
        let mut out = vec![start];
        let mut i = 0;
        while i < out.len() {
            let t = triangles[out[i]];
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                for &g in &by_edge[&(a.min(b), a.max(b))] {
                    if !seen[g] && unit[g].dot(unit[out[i]]) >= cos - 1e-6 {
                        seen[g] = true;
                        out.push(g);
                    }
                }
            }
            i += 1;
        }
        out
    }

    pub fn face_normal(a: Vec3, b: Vec3, c: Vec3) -> Vec3 {
        (b - a).cross(c - a).normalize_or_zero()
    }

    /// Area-weighted vertex normals for smooth shading.
    pub fn vertex_normals(&self) -> Vec<Vec3> {
        let mut n = vec![Vec3::ZERO; self.positions.len()];
        for t in &self.triangles {
            let [a, b, c] = t.map(|i| self.positions[i as usize]);
            let fnrm = (b - a).cross(c - a);
            for i in t {
                n[*i as usize] += fnrm;
            }
        }
        n.iter().map(|v| v.normalize_or_zero()).collect()
    }

    /// The mesh as drawn: vertices split where faces meet at more than `crease_deg`, so
    /// flat faces look flat and smooth ones stay smooth, with the crease edges that
    /// outline a selected object.
    pub fn shading(&self, crease_deg: f32) -> Shading {
        let cos = crease_deg.to_radians().cos();
        let face: Vec<Vec3> = self
            .triangles
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|i| self.positions[i as usize]);
                (b - a).cross(c - a)
            })
            .collect();
        let unit: Vec<Vec3> = face.iter().map(|n| n.normalize_or_zero()).collect();
        let mut around: Vec<Vec<u32>> = vec![Vec::new(); self.positions.len()];
        for (f, t) in self.triangles.iter().enumerate() {
            for &v in t {
                around[v as usize].push(f as u32);
            }
        }
        let mut out = Shading::default();
        let mut index: HashMap<(u32, [i32; 3], [u8; 4]), u32> = HashMap::new();
        let mut corner = vec![[0u32; 3]; self.triangles.len()];
        for (f, t) in self.triangles.iter().enumerate() {
            for k in 0..3 {
                let v = t[k];
                // Faces at this vertex that bend less than the crease from this one.
                let n: Vec3 = around[v as usize]
                    .iter()
                    .filter(|&&g| g as usize == f || unit[f].dot(unit[g as usize]) >= cos)
                    .map(|&g| face[g as usize])
                    .sum::<Vec3>()
                    .normalize_or_zero();
                // Painted faces get their own vertices, so colours stay crisp.
                let color = self.face_color(f).unwrap_or([0; 4]);
                let key = (v, (n * 1000.0).round().as_ivec3().to_array(), color);
                corner[f][k] = *index.entry(key).or_insert_with(|| {
                    out.positions.push(self.positions[v as usize]);
                    out.normals.push(n);
                    out.colors.push(color);
                    out.positions.len() as u32 - 1
                });
            }
        }
        out.triangles = corner.clone();
        let mut edges: HashMap<(u32, u32), Vec<(usize, usize)>> = HashMap::new();
        for (f, t) in self.triangles.iter().enumerate() {
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                edges.entry((a.min(b), a.max(b))).or_default().push((f, k));
            }
        }
        for faces in edges.values() {
            // Borders are left out: they mark broken meshes, which get a red box instead.
            let sharp = match faces[..] {
                [(f, _), (g, _)] => unit[f].dot(unit[g]) < cos,
                _ => false,
            };
            if sharp {
                let (f, k) = faces[0];
                out.edges.push([corner[f][k], corner[f][(k + 1) % 3]]);
            }
        }
        out
    }

    /// Reduces the triangle count to about `keep` (0..1) of the current one with
    /// meshoptimizer's edge-collapse simplifier ("Simplify").
    pub fn simplified(&self, keep: f32) -> Mesh {
        let indices: Vec<u32> = self.triangles.iter().flatten().copied().collect();
        let target = ((indices.len() as f32 * keep.clamp(0.0, 1.0)) as usize / 3) * 3;
        let Ok(adapter) =
            meshopt::VertexDataAdapter::new(bytemuck::cast_slice(&self.positions), 12, 0)
        else {
            return self.clone();
        };
        // Error 1.0 (relative to the mesh size) lets it reach the target count.
        let out = meshopt::simplify(
            &indices,
            &adapter,
            target,
            1.0,
            meshopt::SimplifyOptions::None,
            None,
        );
        Mesh::new(
            self.positions.clone(),
            out.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect(),
        )
        .welded()
    }

    pub fn stats(&self) -> MeshStats {
        let mut s = MeshStats {
            vertices: self.positions.len(),
            triangles: self.triangles.len(),
            ..Default::default()
        };
        let mut undirected: HashMap<(u32, u32), u32> =
            HashMap::with_capacity(self.triangles.len() * 2);
        let mut directed: HashMap<(u32, u32), u32> =
            HashMap::with_capacity(self.triangles.len() * 3);
        let mut dsu = Dsu::new(self.positions.len());
        for t in &self.triangles {
            if t[0] == t[1] || t[1] == t[2] || t[0] == t[2] {
                s.degenerate_triangles += 1;
            }
            let [a, b, c] = t.map(|i| self.positions[i as usize].as_dvec3());
            let cr = (b - a).cross(c - a);
            s.area += 0.5 * cr.length();
            s.volume += a.dot(b.cross(c)) / 6.0;
            for (u, v) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                *undirected.entry((u.min(v), u.max(v))).or_default() += 1;
                *directed.entry((u, v)).or_default() += 1;
                dsu.union(u as usize, v as usize);
            }
        }
        for c in undirected.values() {
            match c {
                1 => s.boundary_edges += 1,
                2 => {}
                _ => s.nonmanifold_edges += 1,
            }
        }
        s.misoriented_edges = directed.values().filter(|c| **c > 1).count();
        let mut roots: Vec<usize> = self
            .triangles
            .iter()
            .map(|t| dsu.find(t[0] as usize))
            .collect();
        roots.sort_unstable();
        roots.dedup();
        s.components = roots.len();
        s
    }
}

impl Mesh {
    /// Splits the mesh into the solids that do not touch, e.g. the two halves left after a
    /// cut. A closed inner surface (the cavity of a hollowed part, wound inwards) stays with
    /// the smallest solid around it.
    pub fn solids(&self) -> Vec<Mesh> {
        let mut dsu = Dsu::new(self.positions.len());
        for t in &self.triangles {
            dsu.union(t[0] as usize, t[1] as usize);
            dsu.union(t[1] as usize, t[2] as usize);
        }
        let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
        for (i, t) in self.triangles.iter().enumerate() {
            groups.entry(dsu.find(t[0] as usize)).or_default().push(i);
        }
        if groups.len() < 2 {
            return vec![self.clone()];
        }
        let mut shells: Vec<Vec<usize>> = groups.into_values().collect();
        shells.sort_by_key(|g| g[0]);
        let volume = |g: &[usize]| -> f64 {
            g.iter()
                .map(|&i| {
                    let [a, b, c] =
                        self.triangles[i].map(|v| self.positions[v as usize].as_dvec3());
                    a.dot(b.cross(c)) / 6.0
                })
                .sum()
        };
        let aabb = |g: &[usize]| {
            let mut b = Aabb::empty();
            for &i in g {
                for v in self.triangles[i] {
                    b.grow(self.positions[v as usize]);
                }
            }
            b
        };
        let (outer, voids): (Vec<_>, Vec<_>) = shells.into_iter().partition(|g| volume(g) >= 0.0);
        if outer.is_empty() {
            return vec![self.clone()];
        }
        let boxes: Vec<(Aabb, f64)> = outer.iter().map(|g| (aabb(g), volume(g))).collect();
        let mut solids = outer;
        for v in voids {
            let b = aabb(&v);
            let around = |o: &Aabb| o.min.cmple(b.min).all() && o.max.cmpge(b.max).all();
            let host = (0..boxes.len())
                .filter(|&k| around(&boxes[k].0))
                .min_by(|&x, &y| boxes[x].1.total_cmp(&boxes[y].1))
                .unwrap_or(0);
            solids[host].extend(v);
        }
        solids.iter().map(|g| self.sub_mesh(g)).collect()
    }

    /// The given triangles, with only the vertices they use.
    fn sub_mesh(&self, tris: &[usize]) -> Mesh {
        let mut map: HashMap<u32, u32> = HashMap::new();
        let mut m = Mesh::default();
        let colored = self.colors.len() == self.triangles.len();
        for &i in tris {
            let t = self.triangles[i].map(|v| {
                *map.entry(v).or_insert_with(|| {
                    m.positions.push(self.positions[v as usize]);
                    (m.positions.len() - 1) as u32
                })
            });
            m.triangles.push(t);
            if colored {
                m.colors.push(self.colors[i]);
            }
        }
        m
    }
}

pub(crate) struct Dsu {
    parent: Vec<usize>,
}

impl Dsu {
    pub fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
        }
    }
    pub fn find(&mut self, mut x: usize) -> usize {
        while self.parent[x] != x {
            self.parent[x] = self.parent[self.parent[x]];
            x = self.parent[x];
        }
        x
    }
    pub fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.parent[ra] = rb;
        }
    }
}

/// Ray-triangle intersection (Möller–Trumbore), returns the distance along the ray.
pub fn ray_triangle(origin: Vec3, dir: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Option<f32> {
    let e1 = b - a;
    let e2 = c - a;
    let p = dir.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-12 {
        return None;
    }
    let inv = 1.0 / det;
    let s = origin - a;
    let u = s.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = dir.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = e2.dot(q) * inv;
    (t > 0.0).then_some(t)
}

pub fn ray_aabb(origin: Vec3, dir: Vec3, b: &Aabb) -> bool {
    let (mut tmin, mut tmax) = (0.0f32, f32::INFINITY);
    for i in 0..3 {
        if dir[i] == 0.0 {
            // Parallel to this slab: inside it (faces included) or a miss. Avoids 0 * inf.
            if origin[i] < b.min[i] || origin[i] > b.max[i] {
                return false;
            }
            continue;
        }
        let (t1, t2) = (
            (b.min[i] - origin[i]) / dir[i],
            (b.max[i] - origin[i]) / dir[i],
        );
        tmin = tmin.max(t1.min(t2));
        tmax = tmax.min(t1.max(t2));
    }
    tmax >= tmin
}

#[cfg(test)]
mod tests {
    #[test]
    fn face_region_stops_at_bends() {
        let cube = primitives::cube(10.0);
        // A face of the box is two triangles; 45° does not reach round the edges.
        assert_eq!(cube.face_region(0, 45.0).len(), 2);
        assert_eq!(cube.face_region(0, 95.0).len(), 12);
        let mut painted = cube.clone();
        painted.colors = vec![[0; 4]; cube.triangles.len()];
        painted.colors[3] = [255, 0, 0, 255];
        assert!(painted.has_colors());
        assert_eq!(painted.face_color(3), Some([255, 0, 0, 255]));
        assert_eq!(painted.face_color(2), None);
        // Painted faces get their own vertices in the drawn mesh.
        assert!(painted.shading(40.0).positions.len() > cube.shading(40.0).positions.len());
    }

    #[test]
    fn shading_splits_creases_only() {
        let cube = crate::primitives::cube(10.0);
        let s = cube.shading(40.0);
        // Each corner gets one vertex per face it touches; every cube edge is a crease.
        assert_eq!(s.positions.len(), 24);
        assert_eq!(s.edges.len(), 12);
        assert!(s.normals.iter().all(|n| (n.length() - 1.0).abs() < 1e-5));
        let sphere = crate::primitives::Primitive::Sphere.build();
        let s = sphere.shading(40.0);
        assert_eq!(s.edges.len(), 0);
        assert_eq!(s.triangles.len(), sphere.triangles.len());
        assert!(s.positions.len() <= sphere.positions.len() + 2);
    }

    use super::*;
    use crate::primitives;

    #[test]
    fn cube_is_closed_and_has_expected_volume() {
        let s = primitives::cube(40.0).stats();
        assert!(s.is_closed_manifold(), "{s:?}");
        assert_eq!(s.components, 1);
        assert!((s.volume - 64000.0).abs() < 1e-3);
    }

    #[test]
    fn translated_cube_centroid_is_box_center() {
        let m =
            primitives::cube(10.0).transformed(&Mat4::from_translation(Vec3::new(12.0, -7.0, 3.0)));
        assert!((m.centroid().unwrap() - m.aabb().center()).length() < 1e-5);
        let flat = Mesh::new(vec![Vec3::ZERO, Vec3::X, Vec3::Y], vec![[0, 1, 2]]);
        assert_eq!(flat.centroid(), None);
    }

    #[test]
    fn open_mesh_reports_boundary() {
        let m = Mesh::new(vec![Vec3::ZERO, Vec3::X, Vec3::Y], vec![[0, 1, 2]]);
        assert_eq!(m.stats().boundary_edges, 3);
    }

    #[test]
    fn mirrored_transform_keeps_outward_winding() {
        let m = primitives::cube(10.0).transformed(&Mat4::from_scale(Vec3::new(-1.0, 1.0, 1.0)));
        assert!(m.stats().volume > 0.0);
    }

    #[test]
    fn simplify_reaches_the_target_and_stays_closed() {
        let m = primitives::sphere(20.0, 64, 32);
        let s = m.simplified(0.25);
        let st = s.stats();
        assert!(
            st.triangles <= m.triangles.len() / 4 + 2,
            "{}",
            st.triangles
        );
        assert!(st.is_closed_manifold(), "{st:?}");
        let ratio = st.volume / m.stats().volume;
        assert!((0.95..1.0).contains(&ratio), "{ratio}");
    }

    #[test]
    fn weld_merges_duplicate_vertices() {
        let a = Vec3::ZERO;
        let soup = [
            [a, Vec3::X, Vec3::Y],
            [Vec3::X, Vec3::new(1.0, 1.0, 0.0), Vec3::Y],
        ];
        assert_eq!(Mesh::from_triangle_soup(&soup).positions.len(), 4);
    }
}
