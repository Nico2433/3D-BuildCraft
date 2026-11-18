//! Circular edges: the feature edges of a mesh (open edges and creases), the chains they
//! form, and the circle each chain fits. The Measure tool ("Circular edge") and the MCP
//! `measure` tool (`edge_loop`, `edge_loops`) share it.
//!
//! A *feature edge* is an edge with one face (the mesh is open there), more than two
//! faces, or two faces whose normals differ by more than [`FEATURE_DEG`]. At each vertex
//! the feature edges are paired off by the smallest turn (at most [`MAX_TURN_DEG`]), so
//! every edge belongs to exactly one *chain* whatever edge it is reached from: a closed
//! loop, or an open arc. A cylinder's rim stays one loop even where a side edge of a
//! coarse mesh meets it, because the rim continues with a 45° turn and the side edge
//! would need 90°.
//!
//! A chain is *circular* when a circle fits its vertices (see [`EdgeLoop::why_not`]):
//! RMS distance from the circle under [`CIRCLE_RMS`] of the radius, no vertex further than
//! [`CIRCLE_MAX`], at least [`MIN_CLOSED`] vertices round a closed loop (a hexagon or a
//! square also has all its corners on a circle but is a polygon, not a round edge), or for
//! an open arc [`MIN_ARC`] vertices spanning [`MIN_SWEEP_DEG`] degrees.
use crate::measure::{centroid, fit_circle_around, Round};
use crate::mesh::Mesh;
use glam::{Mat4, Vec3};
use rustc_hash::FxHashMap as HashMap;
use std::sync::{Arc, Weak};

/// Faces bent more than this (degrees between their normals) share a feature edge.
pub const FEATURE_DEG: f32 = 40.0;
/// Soft mode's threshold: rounded rims bend only a few degrees from one facet to the next,
/// so the circular-edge search can lower the crease angle to find them.
pub const SOFT_DEG: f32 = 6.0;
/// Two feature edges at a vertex are one line only when the path turns less than this.
pub const MAX_TURN_DEG: f32 = 150.0;
/// RMS distance of a loop's vertices from its circle, as a fraction of the radius.
pub const CIRCLE_RMS: f64 = 0.01;
/// The largest distance of any vertex from the circle, as a fraction of the radius.
pub const CIRCLE_MAX: f64 = 0.025;
/// Vertices a closed loop needs to count as round.
pub const MIN_CLOSED: usize = 8;
/// Vertices an open arc needs to count as round.
pub const MIN_ARC: usize = 5;
/// Degrees an open arc must span to count as round.
pub const MIN_SWEEP_DEG: f32 = 40.0;

/// A path of feature edges: vertices (indices into the feature points) in order, and
/// whether it closes on itself (the first vertex is then not repeated at the end).
pub struct Chain {
    pub vertices: Vec<u32>,
    pub closed: bool,
}

/// The feature edges of one mesh, in its own space. Built once per mesh
/// ([`FeatureEdges::compute`], off the UI thread through `scene::WorkCache`).
pub struct FeatureEdges {
    points: Vec<Vec3>,
    edges: Vec<[u32; 2]>,
    chains: Vec<Chain>,
    /// The chain of each edge.
    chain_of: Vec<u32>,
}

/// The faces of an edge while the mesh is scanned.
#[derive(Clone, Copy)]
struct EdgeFaces {
    faces: [u32; 2],
    count: u32,
}

impl FeatureEdges {
    /// Finds the sharp feature edges of `mesh` ([`FEATURE_DEG`]) and joins them into chains.
    pub fn compute(mesh: &Mesh) -> Arc<FeatureEdges> {
        Self::compute_at(mesh, FEATURE_DEG)
    }

    /// The same with the low threshold [`SOFT_DEG`], which also finds rounded rims.
    pub fn compute_soft(mesh: &Mesh) -> Arc<FeatureEdges> {
        Self::compute_at(mesh, SOFT_DEG)
    }

    /// Feature edges where faces bend more than `angle` degrees.
    pub fn compute_at(mesh: &Mesh, angle: f32) -> Arc<FeatureEdges> {
        let (welded, count) = mesh.welded_indices();
        let tris: Vec<[u32; 3]> = mesh
            .triangles
            .iter()
            .map(|t| t.map(|v| welded[v as usize]))
            .collect();
        let normals: Vec<Vec3> = mesh
            .triangle_positions()
            .map(|[a, b, c]| Mesh::face_normal(a, b, c))
            .collect();
        let mut seen: HashMap<u64, EdgeFaces> = HashMap::default();
        seen.reserve(tris.len() * 3 / 2);
        for (f, t) in tris.iter().enumerate() {
            if t[0] == t[1] || t[1] == t[2] || t[0] == t[2] {
                continue;
            }
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                let key = (a.min(b) as u64) << 32 | a.max(b) as u64;
                let e = seen.entry(key).or_insert(EdgeFaces {
                    faces: [f as u32; 2],
                    count: 0,
                });
                if e.count == 1 {
                    e.faces[1] = f as u32;
                }
                e.count += 1;
            }
        }
        let cos = angle.to_radians().cos();
        let mut keys: Vec<u64> = seen
            .iter()
            .filter(|(_, e)| match e.count {
                2 => {
                    let [n0, n1] = e.faces.map(|f| normals[f as usize]);
                    n0 != Vec3::ZERO && n1 != Vec3::ZERO && n0.dot(n1) < cos
                }
                _ => true,
            })
            .map(|(k, _)| *k)
            .collect();
        keys.sort_unstable();
        // Number the vertices the feature edges use and take their positions.
        let mut index = vec![u32::MAX; count];
        let mut edges = Vec::with_capacity(keys.len());
        let mut used = 0u32;
        for key in &keys {
            let ends = [(key >> 32) as usize, (key & 0xFFFF_FFFF) as usize];
            edges.push(ends.map(|w| {
                if index[w] == u32::MAX {
                    index[w] = used;
                    used += 1;
                }
                index[w]
            }));
        }
        let mut points = vec![Vec3::ZERO; used as usize];
        for (v, p) in mesh.positions.iter().enumerate() {
            let i = index[welded[v] as usize];
            if i != u32::MAX {
                points[i as usize] = *p;
            }
        }
        let mut out = FeatureEdges {
            points,
            edges,
            chains: Vec::new(),
            chain_of: Vec::new(),
        };
        out.join();
        Arc::new(out)
    }

    /// Pairs the edges at each vertex by the smallest turn and follows the pairs into chains.
    fn join(&mut self) {
        let n = self.points.len();
        let mut first = vec![0u32; n + 1];
        for &v in self.edges.iter().flatten() {
            first[v as usize + 1] += 1;
        }
        for v in 0..n {
            first[v + 1] += first[v];
        }
        let mut fill = first.clone();
        let mut around = vec![0u32; self.edges.len() * 2];
        for (e, ends) in self.edges.iter().enumerate() {
            for &v in ends {
                around[fill[v as usize] as usize] = e as u32;
                fill[v as usize] += 1;
            }
        }
        let other = |edges: &[[u32; 2]], e: u32, v: u32| {
            let [a, b] = edges[e as usize];
            if a == v {
                b
            } else {
                a
            }
        };
        // The partner of each (vertex, edge) slot, `u32::MAX` for none.
        let mut pair = vec![u32::MAX; around.len()];
        let max_turn = MAX_TURN_DEG.to_radians();
        for v in 0..n {
            let (lo, hi) = (first[v] as usize, first[v + 1] as usize);
            if hi - lo < 2 {
                continue;
            }
            let dir = |slot: usize| {
                let u = other(&self.edges, around[slot], v as u32);
                (self.points[u as usize] - self.points[v]).normalize_or_zero()
            };
            let mut options = Vec::new();
            for i in lo..hi {
                for j in i + 1..hi {
                    // Going in along one edge and out along the other.
                    let turn = (-dir(i)).dot(dir(j)).clamp(-1.0, 1.0).acos();
                    if turn <= max_turn {
                        options.push((turn, i, j));
                    }
                }
            }
            options.sort_by(|a, b| a.0.total_cmp(&b.0));
            for (_, i, j) in options {
                if pair[i] == u32::MAX && pair[j] == u32::MAX {
                    pair[i] = around[j];
                    pair[j] = around[i];
                }
            }
        }
        let partner = |v: u32, e: u32| {
            let (lo, hi) = (first[v as usize] as usize, first[v as usize + 1] as usize);
            (lo..hi)
                .find(|&s| around[s] == e)
                .map(|s| pair[s])
                .filter(|p| *p != u32::MAX)
        };
        let mut chain_of = vec![u32::MAX; self.edges.len()];
        let mut chains = Vec::new();
        for e in 0..self.edges.len() as u32 {
            if chain_of[e as usize] != u32::MAX {
                continue;
            }
            let id = chains.len() as u32;
            chain_of[e as usize] = id;
            let [a, b] = self.edges[e as usize];
            // Walk on from `b`; when it comes back to this edge the chain is closed.
            let mut forward = vec![a, b];
            let (mut v, mut cur, mut closed) = (b, e, false);
            while let Some(next) = partner(v, cur) {
                if next == e {
                    closed = true;
                    forward.pop();
                    break;
                }
                if chain_of[next as usize] != u32::MAX {
                    break;
                }
                chain_of[next as usize] = id;
                v = other(&self.edges, next, v);
                forward.push(v);
                cur = next;
            }
            let mut vertices = forward;
            if !closed {
                let (mut v, mut cur) = (a, e);
                let mut before = Vec::new();
                while let Some(next) = partner(v, cur) {
                    if chain_of[next as usize] != u32::MAX {
                        break;
                    }
                    chain_of[next as usize] = id;
                    v = other(&self.edges, next, v);
                    before.push(v);
                    cur = next;
                }
                before.reverse();
                before.extend(vertices);
                vertices = before;
            }
            chains.push(Chain { vertices, closed });
        }
        self.chains = chains;
        self.chain_of = chain_of;
    }

    pub fn chain_count(&self) -> usize {
        self.chains.len()
    }

    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// The feature edge nearest to world point `p` when the mesh is placed by `model`,
    /// within `tolerance` mm: its chain and its distance.
    pub fn nearest_chain(&self, model: &Mat4, p: Vec3, tolerance: f32) -> Option<(usize, f32)> {
        let mut best: Option<(u32, f32)> = None;
        for (e, [a, b]) in self.edges.iter().enumerate() {
            let a = model.transform_point3(self.points[*a as usize]);
            let b = model.transform_point3(self.points[*b as usize]);
            let ab = b - a;
            let t = ((p - a).dot(ab) / ab.length_squared().max(f32::MIN_POSITIVE)).clamp(0.0, 1.0);
            let d = (a + ab * t).distance(p);
            if d <= tolerance && best.is_none_or(|(_, bd)| d < bd) {
                best = Some((self.chain_of[e], d));
            }
        }
        best.map(|(c, d)| (c as usize, d))
    }

    /// The chain as a fitted loop in world space.
    pub fn edge_loop(&self, chain: usize, model: &Mat4) -> EdgeLoop {
        let c = &self.chains[chain];
        let points: Vec<Vec3> = c
            .vertices
            .iter()
            .map(|&v| model.transform_point3(self.points[v as usize]))
            .collect();
        EdgeLoop::new(chain, points, c.closed)
    }

    /// Every chain with enough vertices to be round, fitted, in world space.
    pub fn loops(&self, model: &Mat4) -> impl Iterator<Item = EdgeLoop> + '_ {
        let model = *model;
        (0..self.chains.len())
            .filter(|&c| self.chains[c].vertices.len() >= MIN_ARC)
            .map(move |c| self.edge_loop(c, &model))
    }
}

/// The sharp feature edges of a mesh for callers that can wait for them (the MCP tools):
/// computed on the calling thread and kept for the last few meshes.
pub fn feature_edges(mesh: &Arc<Mesh>) -> Arc<FeatureEdges> {
    feature_edges_at(mesh, FEATURE_DEG)
}

/// [`feature_edges`] for a crease angle in degrees (kept per mesh and angle).
pub fn feature_edges_at(mesh: &Arc<Mesh>, angle: f32) -> Arc<FeatureEdges> {
    type Kept = Vec<(Weak<Mesh>, u32, Arc<FeatureEdges>)>;
    thread_local! { static KEPT: std::cell::RefCell<Kept> = const { std::cell::RefCell::new(Vec::new()) }; }
    KEPT.with(|kept| {
        let mut kept = kept.borrow_mut();
        kept.retain(|(m, _, _)| m.strong_count() > 0);
        let bits = angle.to_bits();
        if let Some((_, _, f)) = kept
            .iter()
            .find(|(m, a, _)| m.as_ptr() == Arc::as_ptr(mesh) && *a == bits)
        {
            return f.clone();
        }
        let f = FeatureEdges::compute_at(mesh, angle);
        if kept.len() >= 6 {
            kept.remove(0);
        }
        kept.push((Arc::downgrade(mesh), bits, f.clone()));
        f
    })
}

/// A chain of feature edges in world space and the circle fitted to it.
#[derive(Clone, Debug)]
pub struct EdgeLoop {
    /// Its chain in the mesh's [`FeatureEdges`].
    pub chain: usize,
    /// The vertices in order; a closed loop does not repeat the first.
    pub points: Vec<Vec3>,
    pub closed: bool,
    /// The best circle through the vertices, whether or not it is good enough.
    pub fit: Option<CircleFit>,
}

/// A circle fitted to a loop's vertices.
#[derive(Clone, Copy, Debug)]
pub struct CircleFit {
    pub round: Round,
    /// RMS distance of the vertices from the circle, as a fraction of the radius.
    pub rms: f32,
    /// The largest such distance.
    pub max: f32,
    /// Degrees the vertices go round the circle (360 for a closed loop).
    pub sweep: f32,
}

impl EdgeLoop {
    /// Fits a circle to `points` (a chain of feature edges or a section's contour).
    pub fn new(chain: usize, points: Vec<Vec3>, closed: bool) -> Self {
        let fit = fit_loop(&points, closed);
        EdgeLoop {
            chain,
            points,
            closed,
            fit,
        }
    }

    /// Why the loop is not a round edge, or `None` when it is (see the module notes).
    pub fn why_not(&self) -> Option<String> {
        let Some(fit) = &self.fit else {
            return Some("its vertices lie on a line".into());
        };
        let n = self.points.len();
        let (rms, max) = (fit.rms as f64, fit.max as f64);
        if self.closed && n < MIN_CLOSED {
            Some(format!(
                "a closed loop of {n} vertices is a polygon, not a round edge (needs {MIN_CLOSED})"
            ))
        } else if !self.closed && (n < MIN_ARC || fit.sweep < MIN_SWEEP_DEG) {
            Some(format!(
                "an open edge of {n} vertices spanning {:.0}° is too short to say it is round (needs {MIN_ARC} vertices and {MIN_SWEEP_DEG:.0}°)",
                fit.sweep
            ))
        } else if rms > CIRCLE_RMS || max > CIRCLE_MAX {
            Some(format!(
                "no circle fits it: {:.1}% RMS and {:.1}% worst deviation of the radius (limits {:.1}% and {:.1}%)",
                rms * 100.0,
                max * 100.0,
                CIRCLE_RMS * 100.0,
                CIRCLE_MAX * 100.0
            ))
        } else {
            None
        }
    }

    pub fn is_circular(&self) -> bool {
        self.why_not().is_none()
    }

    /// The fitted circle when the loop is round.
    pub fn round(&self) -> Option<Round> {
        self.why_not()
            .is_none()
            .then(|| self.fit.map(|f| f.round))?
    }
}

/// The circle through `points` (in order), in the plane they roughly lie in.
fn fit_loop(points: &[Vec3], closed: bool) -> Option<CircleFit> {
    if points.len() < 3 {
        return None;
    }
    let c = centroid(points);
    // The plane's normal: the sum of the triangles round the centroid (Newell's), which
    // also works for an arc, closed by its chord.
    let mut normal = Vec3::ZERO;
    for (i, p) in points.iter().enumerate() {
        normal += (*p - c).cross(points[(i + 1) % points.len()] - c);
    }
    let mut axis = normal.try_normalize()?;
    // A stable direction for the axis: its largest component is positive.
    let big = axis.abs().max_element();
    if axis.to_array().iter().any(|x| *x == -big) {
        axis = -axis;
    }
    let (mut round, _) = fit_circle_around(points, axis)?;
    round.radius = round.radius.max(f32::MIN_POSITIVE);
    let r = round.radius as f64;
    let (u, v) = (
        axis.any_orthonormal_vector(),
        axis.cross(axis.any_orthonormal_vector()),
    );
    let (mut sum, mut max) = (0.0f64, 0.0f64);
    let (mut sweep, mut last) = (0.0f32, None::<f32>);
    for p in points.iter().chain(points.first().filter(|_| closed)) {
        let d = *p - round.center;
        let h = d.dot(axis);
        let radial = (d - axis * h).length();
        let off = ((radial - round.radius) as f64).hypot(h as f64) / r;
        sum += off * off;
        max = max.max(off);
        let angle = d.dot(v).atan2(d.dot(u));
        if let Some(prev) = last {
            let mut step = angle - prev;
            if step > std::f32::consts::PI {
                step -= std::f32::consts::TAU;
            } else if step < -std::f32::consts::PI {
                step += std::f32::consts::TAU;
            }
            sweep += step;
        }
        last = Some(angle);
    }
    Some(CircleFit {
        round,
        rms: (sum / (points.len() + usize::from(closed)) as f64).sqrt() as f32,
        max: max as f32,
        sweep: sweep.abs().to_degrees(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives;

    /// A tube of `n` sides: radius `r`, `h` high, open at both ends when `open`.
    fn tube(n: usize, r: f32, h: f32, open: bool) -> Mesh {
        let mut positions = Vec::new();
        for z in [0.0, h] {
            for i in 0..n {
                let a = i as f32 / n as f32 * std::f32::consts::TAU;
                positions.push(Vec3::new(r * a.cos(), r * a.sin(), z));
            }
        }
        let n = n as u32;
        let mut triangles = Vec::new();
        for i in 0..n {
            let j = (i + 1) % n;
            triangles.push([i, j, n + j]);
            triangles.push([i, n + j, n + i]);
        }
        let _ = open;
        Mesh::new(positions, triangles)
    }

    fn near(a: f32, b: f32, tolerance: f32) -> bool {
        (a - b).abs() <= tolerance
    }

    fn circular_loops(mesh: &Mesh) -> Vec<EdgeLoop> {
        FeatureEdges::compute(mesh)
            .loops(&Mat4::IDENTITY)
            .filter(|l| l.is_circular())
            .collect()
    }

    #[test]
    fn a_cylinder_has_a_round_rim_at_each_end() {
        let m = primitives::prism(48, 10.0, 30.0);
        let mut loops = circular_loops(&m);
        assert_eq!(loops.len(), 2);
        loops.sort_by(|a, b| {
            a.fit
                .unwrap()
                .round
                .center
                .z
                .total_cmp(&b.fit.unwrap().round.center.z)
        });
        for (l, z) in loops.iter().zip([0.0, 30.0]) {
            let r = l.round().unwrap();
            assert!(l.closed && l.points.len() == 48);
            assert!(near(r.radius, 10.0, 1e-3) && near(r.diameter(), 20.0, 2e-3));
            assert!(r.center.distance(Vec3::new(0.0, 0.0, z)) < 1e-3);
            assert!(r.axis.unwrap().z.abs() > 0.999);
            assert!(l.fit.unwrap().rms < 1e-4 && near(l.fit.unwrap().sweep, 360.0, 0.5));
        }
    }

    #[test]
    fn a_coarse_cylinder_keeps_its_rim_whole_where_side_edges_meet_it() {
        // 8 sides bend 45° between faces, so the side edges are creases too.
        let m = primitives::prism(8, 10.0, 30.0);
        let feats = FeatureEdges::compute(&m);
        let loops: Vec<_> = feats.loops(&Mat4::IDENTITY).collect();
        let round: Vec<_> = loops.iter().filter(|l| l.is_circular()).collect();
        assert_eq!(round.len(), 2);
        assert!(round.iter().all(|l| l.points.len() == 8 && l.closed));
        // The 8 side edges are chains of their own, too short to be round.
        assert!(feats.chain_count() >= 10);
    }

    #[test]
    fn the_cone_base_is_round_and_the_apex_has_no_rim() {
        let m = primitives::cone(60, 12.0, 20.0);
        let loops = circular_loops(&m);
        assert_eq!(loops.len(), 1);
        let r = loops[0].round().unwrap();
        assert!(near(r.radius, 12.0, 1e-3) && r.center.z.abs() < 1e-3);
    }

    #[test]
    fn a_hollow_cylinder_has_four_round_edges() {
        // Outer and inner walls with a flat ring on top and at the bottom.
        let (ro, ri, h, n) = (10.0, 7.0, 20.0, 40usize);
        let mut positions = Vec::new();
        for (r, z) in [(ro, 0.0), (ro, h), (ri, 0.0), (ri, h)] {
            for i in 0..n {
                let a = i as f32 / n as f32 * std::f32::consts::TAU;
                positions.push(Vec3::new(r * a.cos(), r * a.sin(), z));
            }
        }
        let ring = |k: usize| (k * n) as u32;
        let mut t = Vec::new();
        for i in 0..n {
            let j = (i + 1) % n;
            let q = |a: usize, b: usize, flip: bool| {
                let (a0, a1, b0, b1) = (
                    ring(a) + i as u32,
                    ring(a) + j as u32,
                    ring(b) + i as u32,
                    ring(b) + j as u32,
                );
                if flip {
                    [[a0, b1, a1], [a0, b0, b1]]
                } else {
                    [[a0, a1, b1], [a0, b1, b0]]
                }
            };
            t.extend(q(0, 1, false)); // outer wall
            t.extend(q(2, 3, true)); // inner wall
            t.extend(q(1, 3, true)); // top ring
            t.extend(q(0, 2, false)); // bottom ring
        }
        let m = Mesh::new(positions, t);
        let mut radii: Vec<f32> = circular_loops(&m)
            .iter()
            .map(|l| l.round().unwrap().radius)
            .collect();
        radii.sort_by(f32::total_cmp);
        assert_eq!(radii.len(), 4);
        assert!(near(radii[0], 7.0, 1e-3) && near(radii[3], 10.0, 1e-3));
    }

    #[test]
    fn an_open_tube_has_its_boundary_circles_and_an_arc_is_round_if_long_enough() {
        let m = tube(36, 5.0, 8.0, true);
        let loops = circular_loops(&m);
        assert_eq!(loops.len(), 2);
        assert!(loops
            .iter()
            .all(|l| near(l.round().unwrap().radius, 5.0, 1e-3)));
        // A quarter of the circle, 10 vertices: an arc.
        let arc = EdgeLoop::new(
            0,
            (0..10)
                .map(|i| {
                    let a = i as f32 / 9.0 * std::f32::consts::FRAC_PI_2;
                    Vec3::new(5.0 * a.cos(), 5.0 * a.sin(), 0.0)
                })
                .collect(),
            false,
        );
        let r = arc.round().unwrap();
        assert!(near(r.radius, 5.0, 1e-3) && r.center.length() < 1e-3);
        assert!(near(arc.fit.unwrap().sweep, 90.0, 0.5));
        // Three points, or a few degrees, say nothing.
        let few = EdgeLoop::new(0, arc.points[..4].to_vec(), false);
        assert!(few.why_not().unwrap().contains("too short"));
    }

    #[test]
    fn a_torus_section_boundary_is_round() {
        // Half of a torus, cut by the plane y = 0: two boundary circles of the tube.
        let full = primitives::torus(15.0, 5.0, 48, 24);
        let kept: Vec<[u32; 3]> = full
            .triangles
            .iter()
            .copied()
            .filter(|t| t.iter().all(|&i| full.positions[i as usize].y >= -1e-4))
            .collect();
        let half = Mesh::new(full.positions.clone(), kept);
        let loops = circular_loops(&half);
        assert!(loops.len() >= 2, "{} loops", loops.len());
        assert!(loops
            .iter()
            .all(|l| near(l.round().unwrap().radius, 5.0, 0.02)));
    }

    #[test]
    fn polygons_and_wavy_loops_are_not_round() {
        // A cube's edges are squares: four vertices on a circle, but a polygon.
        let cube = primitives::cube(10.0);
        let feats = FeatureEdges::compute(&cube);
        assert!(feats.loops(&Mat4::IDENTITY).all(|l| !l.is_circular()));
        let hex = primitives::prism(6, 10.0, 5.0);
        let hex_loops: Vec<_> = FeatureEdges::compute(&hex)
            .loops(&Mat4::IDENTITY)
            .filter(|l| l.closed && l.points.len() == 6)
            .collect();
        assert_eq!(hex_loops.len(), 2);
        let why = hex_loops[0].why_not().unwrap();
        assert!(why.contains("polygon"), "{why}");
        // The same hexagon with 8 vertices is a round edge: an octagon prism.
        assert_eq!(circular_loops(&primitives::prism(8, 10.0, 5.0)).len(), 2);
        // An ellipse 5 % longer than wide does not fit one circle.
        let ellipse = EdgeLoop::new(
            0,
            (0..64)
                .map(|i| {
                    let a = i as f32 / 64.0 * std::f32::consts::TAU;
                    Vec3::new(10.5 * a.cos(), 10.0 * a.sin(), 0.0)
                })
                .collect(),
            true,
        );
        assert!(ellipse.why_not().unwrap().contains("no circle fits"));
        // A 0.3 % bump is still round.
        let bumpy = EdgeLoop::new(
            0,
            (0..64)
                .map(|i| {
                    let a = i as f32 / 64.0 * std::f32::consts::TAU;
                    Vec3::new(
                        10.0 * a.cos(),
                        10.0 * a.sin(),
                        if i % 2 == 0 { 0.02 } else { -0.02 },
                    )
                })
                .collect(),
            true,
        );
        assert!(bumpy.is_circular());
    }

    #[test]
    fn a_sphere_has_no_sharp_edges_and_a_placed_mesh_is_measured_in_the_world() {
        let sphere = primitives::sphere(10.0, 48, 24);
        // Only the poles' open seam could appear; nothing is round.
        assert!(circular_loops(&sphere).is_empty());
        let m = primitives::prism(32, 10.0, 30.0);
        let model = Mat4::from_scale_rotation_translation(
            Vec3::splat(2.0),
            glam::Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
            Vec3::new(5.0, 6.0, 7.0),
        );
        let feats = FeatureEdges::compute(&m);
        let l = feats.loops(&model).find(|l| l.is_circular()).unwrap();
        let r = l.round().unwrap();
        assert!(near(r.radius, 20.0, 0.02));
        assert!(r.axis.unwrap().y.abs() > 0.999);
    }

    #[test]
    fn the_nearest_chain_is_found_within_the_tolerance_only() {
        let m = primitives::prism(48, 10.0, 30.0);
        let feats = FeatureEdges::compute(&m);
        let top = Vec3::new(10.0, 0.0, 30.0);
        let (chain, d) = feats
            .nearest_chain(&Mat4::IDENTITY, top + Vec3::new(0.0, 0.0, -0.4), 1.0)
            .unwrap();
        assert!(near(d, 0.4, 1e-3));
        assert!(feats.edge_loop(chain, &Mat4::IDENTITY).is_circular());
        assert!(feats
            .nearest_chain(&Mat4::IDENTITY, Vec3::new(0.0, 0.0, 15.0), 1.0)
            .is_none());
    }

    #[test]
    fn open_edges_and_broken_meshes_follow_without_looping_forever() {
        // A single triangle, a fan with a hole and a mesh with a non-manifold edge.
        let tri = Mesh::new(vec![Vec3::ZERO, Vec3::X, Vec3::Y], vec![[0, 1, 2]]);
        let f = FeatureEdges::compute(&tri);
        assert_eq!((f.edge_count(), f.chain_count()), (3, 1));
        let l = f.edge_loop(0, &Mat4::IDENTITY);
        assert!(l.closed && l.points.len() == 3 && !l.is_circular());
        // Three triangles on one edge: a feature edge with two spurs at its ends.
        let fin = Mesh::new(
            vec![Vec3::ZERO, Vec3::X, Vec3::Y, -Vec3::Y, Vec3::Z],
            vec![[0, 1, 2], [0, 1, 3], [0, 1, 4]],
        );
        let f = FeatureEdges::compute(&fin);
        for c in 0..f.chain_count() {
            let _ = f.edge_loop(c, &Mat4::IDENTITY);
        }
        // Empty and degenerate meshes.
        assert_eq!(FeatureEdges::compute(&Mesh::default()).chain_count(), 0);
        let flat = Mesh::new(vec![Vec3::ZERO; 3], vec![[0, 1, 2]]);
        assert_eq!(FeatureEdges::compute(&flat).chain_count(), 0);
    }

    #[test]
    fn the_cached_features_follow_the_mesh() {
        let a = Arc::new(primitives::prism(16, 5.0, 5.0));
        let first = feature_edges(&a);
        assert!(Arc::ptr_eq(&first, &feature_edges(&a)));
        let b = Arc::new((*a).clone());
        assert!(!Arc::ptr_eq(&first, &feature_edges(&b)));
    }
}
