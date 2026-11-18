//! Plane cut ("Split"): splits a mesh with a plane and caps both halves, so a closed mesh
//! gives two closed meshes.

use crate::mesh::Mesh;
use crate::texture::FaceTex;
use glam::{EulerRot, Mat4, Quat, Vec2, Vec3};
use std::collections::HashMap;

/// Which halves "Split" keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keep {
    Top,
    Bottom,
    Both,
}

/// The smallest width or length a drag leaves the plane, mm.
const MIN_EXTENT: f32 = 1.0;

/// The cutting plane of the Split tool: through `center`, moved `offset` along its normal,
/// turned by `rotation` degrees (roll about X, pitch about Y, yaw about Z, as an object's).
/// Unturned, the normal is +Z.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CutPlane {
    pub center: Vec3,
    pub offset: f32,
    pub rotation: Vec3,
    /// Width and length along the plane's own X and Y, mm; `None` is infinite. An
    /// infinite component leaves the plane full along that direction only.
    pub size: Option<Vec2>,
}

impl CutPlane {
    /// The horizontal plane at height `z`.
    pub fn at_height(z: f32) -> Self {
        Self {
            center: Vec3::new(0.0, 0.0, z),
            offset: 0.0,
            rotation: Vec3::ZERO,
            size: None,
        }
    }

    /// The plane through `point` whose normal (the "top" side) is `normal`; `None` for a
    /// zero or non-finite normal.
    pub fn through(point: Vec3, normal: Vec3) -> Option<Self> {
        let n = normal.try_normalize()?;
        let (z, y, x) = Quat::from_rotation_arc(Vec3::Z, n).to_euler(EulerRot::ZYX);
        Some(Self {
            center: point,
            offset: 0.0,
            rotation: Vec3::new(x, y, z) * (180.0 / std::f32::consts::PI),
            size: None,
        })
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

    /// The plane's own X and Y directions, along which its width and length run.
    pub fn axes(&self) -> (Vec3, Vec3) {
        let q = self.quat();
        (q * Vec3::X, q * Vec3::Y)
    }

    /// The width (`axis` 0) or length (1) in mm; `None` when full along it.
    pub fn extent(&self, axis: usize) -> Option<f32> {
        self.size.map(|s| s[axis]).filter(|e| e.is_finite())
    }

    /// Sets the width (`axis` 0) or length (1); `None`, 0 or less makes it full again,
    /// and a plane full both ways is infinite.
    pub fn set_extent(&mut self, axis: usize, extent: Option<f32>) {
        let mut size = self.size.unwrap_or(Vec2::INFINITY);
        size[axis] = extent.filter(|e| *e > 0.0).unwrap_or(f32::INFINITY);
        self.size = (size != Vec2::INFINITY).then_some(size);
    }

    /// Half the width and length; infinite when full along that direction.
    pub fn half_size(&self) -> Vec2 {
        self.size.unwrap_or(Vec2::INFINITY) * 0.5
    }

    /// Grows the width (`axis` 0) or length (1) by `by` mm, both edges moving so the
    /// middle stays. A full direction counts as `full` mm wide: growing leaves it full,
    /// shrinking makes it finite from there. It never goes below [`MIN_EXTENT`].
    pub fn grow_extent(&mut self, axis: usize, by: f32, full: f32) {
        let now = match self.extent(axis) {
            Some(e) => e,
            None if by >= 0.0 => return,
            None => full,
        };
        self.set_extent(axis, Some((now + by).max(MIN_EXTENT)));
    }

    /// Turns the plane by `q` (world space) about its middle `point()`. The centre stays
    /// put and the offset follows, so the middle stays over the object instead of
    /// drifting sideways after a few moves and turns.
    pub fn turn(&mut self, q: Quat) {
        let middle = self.point();
        let (z, y, x) = (q * self.quat()).normalize().to_euler(EulerRot::ZYX);
        self.rotation = Vec3::new(x, y, z) * (180.0 / std::f32::consts::PI);
        self.offset = self.normal().dot(middle - self.center);
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

/// Why `plane` cuts nothing of `mesh` (world coordinates), when the whole mesh lies on
/// one side of it; `name` names the object in the message.
pub fn misses(mesh: &Mesh, plane: &CutPlane, name: &str) -> Option<String> {
    let (n, p) = (plane.normal(), plane.point());
    let (lo, hi) = mesh
        .positions
        .iter()
        .map(|v| n.dot(*v - p))
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), d| {
            (lo.min(d), hi.max(d))
        });
    // A hair inside still cuts nothing worth keeping.
    const EPS: f32 = 1e-4;
    if mesh.positions.is_empty() || (lo < -EPS && hi > EPS) {
        return None;
    }
    let below = hi <= EPS;
    let b = mesh.aabb();
    Some(
        if n.abs_diff_eq(Vec3::Z, 1e-4) || n.abs_diff_eq(-Vec3::Z, 1e-4) {
            // Level: in heights, as the agent and the panel give them.
            let (side, z) = if p.z >= b.max.z - EPS {
                ("above", b.max.z)
            } else {
                ("below", b.min.z)
            };
            let edge = if side == "above" { "top" } else { "bottom" };
            format!(
                "z {} is {side} the {edge} of {name} ({z:.2} mm); nothing to cut",
                (p.z * 100.0).round() / 100.0
            )
        } else {
            let side = if below { "below" } else { "above" };
            format!("the plane misses {name}, which lies wholly {side} it; nothing to cut")
        },
    )
}

/// The pieces of `mesh` (world coordinates) that "Split" keeps, empty ones left out. An
/// infinite plane cuts exactly with `cut_plane`. A finite one only cuts inside the prism
/// its rectangle sweeps along the normal: Top removes the part of the prism below the
/// plane, Bottom the part above, and Both gives that part below and the rest. That runs
/// as booleans, so the mesh must be closed and manifold.
pub fn split(mesh: &Mesh, plane: &CutPlane, keep: Keep) -> anyhow::Result<Vec<Mesh>> {
    use crate::boolean::{apply, Op};
    let pieces = if plane.size.is_none() {
        let (top, bottom) = cut_plane(mesh, plane);
        match keep {
            Keep::Top => vec![top],
            Keep::Bottom => vec![bottom],
            Keep::Both => vec![top, bottom],
        }
    } else if mesh.is_empty() {
        vec![]
    } else {
        let below = || prism(mesh, plane, false);
        match keep {
            Keep::Top => vec![apply(Op::Subtract, &[mesh.clone(), below()])?],
            Keep::Bottom => vec![apply(
                Op::Subtract,
                &[mesh.clone(), prism(mesh, plane, true)],
            )?],
            Keep::Both => {
                let below = below();
                vec![
                    apply(Op::Subtract, &[mesh.clone(), below.clone()])?,
                    apply(Op::Intersect, &[mesh.clone(), below])?,
                ]
            }
        }
    };
    Ok(pieces.into_iter().filter(|m| !m.is_empty()).collect())
}

/// The box over the plane's rectangle, on the side the normal points to when `above`,
/// reaching past `mesh` along the normal and along any full direction.
fn prism(mesh: &Mesh, plane: &CutPlane, above: bool) -> Mesh {
    let p = plane.point();
    let reach = mesh
        .positions
        .iter()
        .map(|v| v.distance(p))
        .fold(0.0, f32::max)
        + 1.0;
    let half = plane.half_size().min(Vec2::splat(reach));
    let size = Vec3::new(half.x * 2.0, half.y * 2.0, reach);
    // The box stands on its base, so the one below starts `reach` under the plane.
    let base = if above { p } else { p - plane.normal() * reach };
    let place = Mat4::from_rotation_translation(plane.quat(), base);
    crate::primitives::box_mesh(size).transformed(&place)
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

    // Each output triangle keeps the colour and texture of the one it was cut from; a
    // piece cut out of it gets UVs found barycentrically, so the picture does not move.
    let colored = mesh.colors.len() == mesh.triangles.len();
    let textured = mesh.has_tex();
    let (mut top, mut bottom): (Vec<Part>, Vec<Part>) = (Vec::new(), Vec::new());
    // Cap edges, directed the way the top half's cap must run.
    let mut cap_next: HashMap<u32, u32> = HashMap::new();
    for (ti, t) in mesh.triangles.iter().enumerate() {
        let color = if colored { mesh.colors[ti] } else { [0; 4] };
        let tex = mesh
            .tex
            .get(ti)
            .copied()
            .filter(|_| textured)
            .unwrap_or_default();
        let side = t.map(|i| above[i as usize]);
        if side[0] == side[1] && side[1] == side[2] {
            if side[0] { &mut top } else { &mut bottom }.push(Part { t: *t, color, tex });
            continue;
        }
        let from = t.map(|i| mesh.positions[i as usize]);
        let part = |t: [u32; 3], positions: &[Vec3]| Part {
            t,
            color,
            tex: if tex.layer > 0 {
                tex.within(from, t.map(|i| positions[i as usize]))
            } else {
                tex
            },
        };
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
        lone.push(part([l, i1, i2], &positions));
        rest.extend([part([i1, x, y], &positions), part([i1, y, i2], &positions)]);
        // The lone half has the edge i1→i2, so its cap needs i2→i1, and the other way round.
        if side[k] {
            cap_next.insert(i2, i1);
        } else {
            cap_next.insert(i1, i2);
        }
    }

    // Caps are new surface: no texture, and no colour of their own.
    for tri in cap(&positions, cap_next) {
        top.push(Part::new(tri));
        bottom.push(Part::new([tri[0], tri[2], tri[1]]));
    }
    let half = |parts: Vec<Part>| {
        let mut m = Mesh::new(positions.clone(), parts.iter().map(|p| p.t).collect());
        if colored && parts.iter().any(|p| p.color[3] > 0) {
            m.colors = parts.iter().map(|p| p.color).collect();
        }
        if textured && parts.iter().any(|p| p.tex.layer > 0) {
            m.tex = parts.iter().map(|p| p.tex).collect();
        }
        m.welded()
    };
    (half(top), half(bottom))
}

/// An output triangle of a cut with the attributes it inherits.
struct Part {
    t: [u32; 3],
    color: [u8; 4],
    tex: FaceTex,
}

impl Part {
    fn new(t: [u32; 3]) -> Self {
        Self {
            t,
            color: [0; 4],
            tex: FaceTex::default(),
        }
    }
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
    fn growing_an_extent_keeps_full_until_shrunk_and_a_minimum() {
        let mut plane = CutPlane::at_height(0.0);
        // Growing a full direction leaves it full; shrinking starts from the drawn size.
        plane.grow_extent(0, 10.0, 80.0);
        assert_eq!(plane.size, None);
        plane.grow_extent(0, -20.0, 80.0);
        assert_eq!(plane.extent(0), Some(60.0));
        assert_eq!(plane.extent(1), None);
        plane.grow_extent(0, 5.0, 80.0);
        assert_eq!(plane.extent(0), Some(65.0));
        plane.grow_extent(0, -1000.0, 80.0);
        assert_eq!(plane.extent(0), Some(MIN_EXTENT));
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
    fn cut_keeps_textures_and_colors_with_uvs_inside_the_source_range() {
        use crate::texture::apply_box;
        let cube = primitives::cube(10.0);
        let all: Vec<usize> = (0..cube.triangles.len()).collect();
        let mut textured = apply_box(&cube, &Mat4::IDENTITY, &all, 1, 10.0);
        textured.colors = vec![[10, 20, 30, 255]; cube.triangles.len()];
        let src: Vec<[f32; 2]> = textured.tex.iter().flat_map(|t| t.uv).collect();
        let lo = src
            .iter()
            .fold([f32::MAX; 2], |a, u| [a[0].min(u[0]), a[1].min(u[1])]);
        let hi = src
            .iter()
            .fold([f32::MIN; 2], |a, u| [a[0].max(u[0]), a[1].max(u[1])]);
        let mut plane = CutPlane::at_height(0.0);
        plane.center = cube.aabb().center();
        plane.turn(Quat::from_rotation_y(30f32.to_radians()));
        let (top, bottom) = cut_plane(&textured, &plane);
        for half in [&top, &bottom] {
            let n = half.triangles.len();
            assert_eq!((half.tex.len(), half.colors.len()), (n, n));
            let with = (0..n).filter(|&i| half.face_tex(i).is_some()).count();
            // Everything but the cap keeps its picture; the cap has none.
            assert!(with > n / 2 && with < n, "{with} of {n}");
            for t in half.tex.iter().filter(|t| t.layer > 0) {
                for u in t.uv {
                    assert!(u[0] >= lo[0] - 1e-3 && u[0] <= hi[0] + 1e-3);
                    assert!(u[1] >= lo[1] - 1e-3 && u[1] <= hi[1] + 1e-3);
                }
            }
        }
    }

    #[test]
    fn plane_outside_the_mesh_keeps_it_whole() {
        let m = primitives::cube(10.0);
        let (top, bottom) = cut(&m, 50.0);
        assert!(top.is_empty());
        assert_eq!(bottom.stats(), m.welded().stats());
    }

    #[test]
    fn finite_plane_cuts_only_inside_its_prism() {
        // A 20 mm cube on the floor and a 10 x 10 plane through its middle.
        let cube = primitives::cube(20.0);
        let mut plane = CutPlane::at_height(10.0);
        plane.size = Some(Vec2::splat(10.0));
        let volumes = |plane: &CutPlane, keep| -> Vec<f64> {
            let pieces = split(&cube, plane, keep).unwrap();
            pieces.iter().map(closed_volume).collect()
        };
        let near = |a: Vec<f64>, b: &[f64]| {
            a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-2)
        };
        assert!(near(volumes(&plane, Keep::Top), &[7000.0]));
        assert!(near(volumes(&plane, Keep::Bottom), &[7000.0]));
        assert!(near(volumes(&plane, Keep::Both), &[7000.0, 1000.0]));
        // Full along its length: a 10 mm wide slot across the whole cube.
        plane.size = None;
        plane.set_extent(0, Some(10.0));
        assert!(near(volumes(&plane, Keep::Top), &[6000.0]));
        // A turned plane: a 45 degree yawed square removes the same volume.
        plane.set_extent(1, Some(10.0));
        plane.rotation.z = 45.0;
        assert!(near(volumes(&plane, Keep::Bottom), &[7000.0]));
    }

    #[test]
    fn infinite_plane_splits_as_before() {
        // The exact cut fills its caps in hash order, so meshes are compared by their
        // points, triangle count and volume, not triangle by triangle.
        fn shape(m: &Mesh) -> (Vec<[u32; 3]>, usize, i64) {
            let mut points: Vec<[u32; 3]> = m
                .positions
                .iter()
                .map(|p| p.to_array().map(f32::to_bits))
                .collect();
            points.sort();
            let volume = (closed_volume(m) * 10.0).round() as i64;
            (points, m.triangles.len(), volume)
        }
        let m = Primitive::Sphere.build();
        let plane = CutPlane::through(Vec3::new(1.0, 2.0, 7.0), Vec3::new(1.0, 0.5, 2.0)).unwrap();
        let (top, bottom) = cut_plane(&m, &plane);
        for (keep, expected) in [
            (Keep::Top, vec![&top]),
            (Keep::Bottom, vec![&bottom]),
            (Keep::Both, vec![&top, &bottom]),
        ] {
            let pieces = split(&m, &plane, keep).unwrap();
            assert_eq!(pieces.len(), expected.len(), "{keep:?}");
            for (got, want) in pieces.iter().zip(expected) {
                assert_eq!(shape(got), shape(want), "{keep:?}");
            }
        }
    }

    #[test]
    fn zero_or_cleared_extent_is_full_again() {
        let mut plane = CutPlane::at_height(0.0);
        plane.set_extent(0, Some(30.0));
        assert_eq!(plane.extent(0), Some(30.0));
        assert_eq!(plane.extent(1), None);
        plane.set_extent(1, Some(20.0));
        plane.set_extent(0, Some(0.0));
        assert_eq!(plane.size, Some(Vec2::new(f32::INFINITY, 20.0)));
        plane.set_extent(1, None);
        assert_eq!(plane.size, None);
    }
}
