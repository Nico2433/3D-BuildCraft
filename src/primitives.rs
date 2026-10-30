//! Built-in shapes. All fit a 40 mm box (the original's default size), are centred on
//! the Z axis and rest on the floor (z = 0). Z is up.

use crate::mesh::Mesh;
use glam::Vec3;
use std::f32::consts::{PI, TAU};

pub const DEFAULT_SIZE: f32 = 40.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Primitive {
    Cube,
    Cylinder,
    Pyramid,
    Cone,
    Sphere,
    Hexagon,
    Wedge,
    Torus,
    Tetrahedron,
}

impl Primitive {
    pub const ALL: [Primitive; 9] = [
        Primitive::Cube,
        Primitive::Cylinder,
        Primitive::Pyramid,
        Primitive::Cone,
        Primitive::Sphere,
        Primitive::Hexagon,
        Primitive::Wedge,
        Primitive::Torus,
        Primitive::Tetrahedron,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Primitive::Cube => "Cube",
            Primitive::Cylinder => "Cylinder",
            Primitive::Pyramid => "Pyramid",
            Primitive::Cone => "Cone",
            Primitive::Sphere => "Sphere",
            Primitive::Hexagon => "Hexagon",
            Primitive::Wedge => "Wedge",
            Primitive::Torus => "Torus",
            Primitive::Tetrahedron => "Tetrahedron",
        }
    }

    pub fn build(self) -> Mesh {
        let s = DEFAULT_SIZE;
        match self {
            Primitive::Cube => cube(s),
            Primitive::Cylinder => prism(120, s / 2.0, s),
            Primitive::Pyramid => pyramid(s, s),
            Primitive::Cone => cone(120, s / 2.0, s),
            Primitive::Sphere => sphere(s / 2.0, 64, 32),
            Primitive::Hexagon => prism(6, s / 2.0, s),
            Primitive::Wedge => wedge(s, s, s),
            Primitive::Torus => torus(s * 0.375, s * 0.125, 96, 32),
            Primitive::Tetrahedron => tetrahedron(s),
        }
    }
}

/// "Custom": a shape with its own subdivisions and size, centred and resting on the floor.
/// `lon` splits it around (or across, for the box); `lat` splits the box, sphere and torus
/// from bottom to top. Shapes with a fixed number of sides (pyramid, hexagon, wedge,
/// tetrahedron) only take the size. The shape keeps its proportions across, so it fits
/// within width × width.
pub fn custom(shape: Primitive, lat: usize, lon: usize, width: f32, height: f32) -> Mesh {
    let (lat, lon) = (lat.clamp(1, 200), lon.clamp(3, 200));
    let s = DEFAULT_SIZE;
    let base = match shape {
        Primitive::Cube => return grid_box(Vec3::new(width, width, height), lon, lat),
        Primitive::Cylinder => prism(lon, s / 2.0, s),
        Primitive::Cone => cone(lon, s / 2.0, s),
        Primitive::Sphere => sphere(s / 2.0, lon, lat.max(2)),
        Primitive::Torus => torus(s * 0.375, s * 0.125, lon, lat.max(3)),
        other => other.build(),
    };
    // Fit the shape's own box to width × width × height, on the floor.
    let b = base.aabb();
    let size = (b.max - b.min).max(Vec3::splat(1e-6));
    let k = Vec3::new(
        width / size.x.max(size.y),
        width / size.x.max(size.y),
        height / size.z,
    );
    let centre = (b.min + b.max) * 0.5;
    base.transformed(
        &(glam::Mat4::from_scale(k)
            * glam::Mat4::from_translation(Vec3::new(-centre.x, -centre.y, -b.min.z))),
    )
}

/// A box whose sides are split into `across` × `up` cells and top and bottom into
/// `across` × `across`.
fn grid_box(size: Vec3, across: usize, up: usize) -> Mesh {
    let h = size * 0.5;
    let (x, y, z) = (
        Vec3::new(size.x, 0.0, 0.0),
        Vec3::new(0.0, size.y, 0.0),
        Vec3::new(0.0, 0.0, size.z),
    );
    let low = Vec3::new(-h.x, -h.y, 0.0);
    // Corner, then two edges whose cross product points out.
    let faces = [
        (low + z, x, y, across, across),
        (low, y, x, across, across),
        (low + x, y, z, across, up),
        (low, z, y, up, across),
        (low + y, z, x, up, across),
        (low, x, z, across, up),
    ];
    let mut soup = Vec::new();
    for (o, du, dv, nu, nv) in faces {
        let at = |i: usize, j: usize| o + du * (i as f32 / nu as f32) + dv * (j as f32 / nv as f32);
        for i in 0..nu {
            for j in 0..nv {
                let (a, b, c, d) = (at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1));
                soup.push([a, b, c]);
                soup.push([a, c, d]);
            }
        }
    }
    Mesh::from_triangle_soup(&soup)
}

pub fn cube(size: f32) -> Mesh {
    box_mesh(Vec3::splat(size))
}

/// Axis-aligned box. Corner index bits: x = 1, y = 2, z = 4.
pub fn box_mesh(size: Vec3) -> Mesh {
    let h = size * 0.5;
    let positions = (0..8)
        .map(|i| {
            Vec3::new(
                if i & 1 != 0 { h.x } else { -h.x },
                if i & 2 != 0 { h.y } else { -h.y },
                if i & 4 != 0 { size.z } else { 0.0 },
            )
        })
        .collect();
    let triangles = vec![
        [0, 2, 3],
        [0, 3, 1], // -z
        [4, 5, 7],
        [4, 7, 6], // +z
        [0, 1, 5],
        [0, 5, 4], // -y
        [2, 6, 7],
        [2, 7, 3], // +y
        [0, 4, 6],
        [0, 6, 2], // -x
        [1, 3, 7],
        [1, 7, 5], // +x
    ];
    Mesh::new(positions, triangles)
}

/// Regular n-sided prism. Caps are fans from the first rim vertex, like the original
/// (120 segments gives 240 vertices and 476 triangles).
pub fn prism(n: usize, radius: f32, height: f32) -> Mesh {
    let n = n.max(3);
    let mut positions = Vec::with_capacity(2 * n);
    for i in 0..n {
        let a = i as f32 * TAU / n as f32;
        let (s, c) = a.sin_cos();
        positions.push(Vec3::new(c * radius, s * radius, 0.0));
        positions.push(Vec3::new(c * radius, s * radius, height));
    }
    let b = |i: usize| (2 * (i % n)) as u32;
    let t = |i: usize| (2 * (i % n) + 1) as u32;
    let mut triangles = Vec::with_capacity(4 * n - 4);
    for i in 0..n {
        triangles.push([b(i), b(i + 1), t(i + 1)]);
        triangles.push([b(i), t(i + 1), t(i)]);
    }
    for i in 1..n - 1 {
        triangles.push([b(0), b(i + 1), b(i)]);
        triangles.push([t(0), t(i), t(i + 1)]);
    }
    Mesh::new(positions, triangles)
}

pub fn cone(n: usize, radius: f32, height: f32) -> Mesh {
    let n = n.max(3);
    let mut positions: Vec<Vec3> = (0..n)
        .map(|i| {
            let (s, c) = (i as f32 * TAU / n as f32).sin_cos();
            Vec3::new(c * radius, s * radius, 0.0)
        })
        .collect();
    positions.push(Vec3::new(0.0, 0.0, height));
    let apex = n as u32;
    let mut triangles = Vec::with_capacity(2 * n);
    for i in 0..n {
        triangles.push([i as u32, ((i + 1) % n) as u32, apex]);
    }
    for i in 1..n - 1 {
        triangles.push([0, (i + 1) as u32, i as u32]);
    }
    Mesh::new(positions, triangles)
}

pub fn pyramid(base: f32, height: f32) -> Mesh {
    let h = base / 2.0;
    let positions = vec![
        Vec3::new(-h, -h, 0.0),
        Vec3::new(h, -h, 0.0),
        Vec3::new(h, h, 0.0),
        Vec3::new(-h, h, 0.0),
        Vec3::new(0.0, 0.0, height),
    ];
    let triangles = vec![
        [0, 1, 4],
        [1, 2, 4],
        [2, 3, 4],
        [3, 0, 4],
        [0, 2, 1],
        [0, 3, 2],
    ];
    Mesh::new(positions, triangles)
}

/// Ramp: full height along the -x edge, sloping down to the +x edge.
pub fn wedge(width: f32, depth: f32, height: f32) -> Mesh {
    let (hx, hy) = (width / 2.0, depth / 2.0);
    let positions = vec![
        Vec3::new(-hx, -hy, 0.0),
        Vec3::new(hx, -hy, 0.0),
        Vec3::new(-hx, hy, 0.0),
        Vec3::new(hx, hy, 0.0),
        Vec3::new(-hx, -hy, height),
        Vec3::new(-hx, hy, height),
    ];
    let triangles = vec![
        [0, 2, 3],
        [0, 3, 1], // bottom
        [0, 4, 5],
        [0, 5, 2], // back (-x)
        [1, 3, 5],
        [1, 5, 4], // slope
        [0, 1, 4], // -y side
        [2, 5, 3], // +y side
    ];
    Mesh::new(positions, triangles)
}

pub fn tetrahedron(size: f32) -> Mesh {
    // Regular tetrahedron with a horizontal base, scaled to fit `size`.
    let r = size / 2.0;
    let base: Vec<Vec3> = (0..3)
        .map(|i| {
            let (s, c) = (PI / 2.0 + i as f32 * TAU / 3.0).sin_cos();
            Vec3::new(c * r, s * r, 0.0)
        })
        .collect();
    let edge = base[0].distance(base[1]);
    let height = edge * (2.0f32 / 3.0).sqrt();
    let mut positions = base;
    positions.push(Vec3::new(0.0, 0.0, height));
    orient_convex(Mesh::new(
        positions,
        vec![[0, 1, 2], [0, 1, 3], [1, 2, 3], [2, 0, 3]],
    ))
}

pub fn sphere(radius: f32, lon: usize, lat: usize) -> Mesh {
    let (lon, lat) = (lon.max(3), lat.max(2));
    let mut positions = vec![Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 2.0 * radius)];
    for j in 1..lat {
        let v = -PI / 2.0 + j as f32 * PI / lat as f32;
        let (sv, cv) = v.sin_cos();
        for i in 0..lon {
            let (su, cu) = (i as f32 * TAU / lon as f32).sin_cos();
            positions.push(Vec3::new(
                radius * cv * cu,
                radius * cv * su,
                radius * sv + radius,
            ));
        }
    }
    let ring = |j: usize, i: usize| (2 + (j - 1) * lon + i % lon) as u32;
    let mut triangles = Vec::new();
    for i in 0..lon {
        triangles.push([0, ring(1, i + 1), ring(1, i)]);
        triangles.push([1, ring(lat - 1, i), ring(lat - 1, i + 1)]);
    }
    for j in 1..lat - 1 {
        for i in 0..lon {
            let (a, b, c, d) = (
                ring(j, i),
                ring(j, i + 1),
                ring(j + 1, i + 1),
                ring(j + 1, i),
            );
            triangles.push([a, b, c]);
            triangles.push([a, c, d]);
        }
    }
    Mesh::new(positions, triangles)
}

pub fn torus(major: f32, minor: f32, nu: usize, nv: usize) -> Mesh {
    let (nu, nv) = (nu.max(3), nv.max(3));
    let mut positions = Vec::with_capacity(nu * nv);
    for i in 0..nu {
        let (su, cu) = (i as f32 * TAU / nu as f32).sin_cos();
        for j in 0..nv {
            let (sv, cv) = (j as f32 * TAU / nv as f32).sin_cos();
            let r = major + minor * cv;
            positions.push(Vec3::new(r * cu, r * su, minor * sv + minor));
        }
    }
    let idx = |i: usize, j: usize| ((i % nu) * nv + j % nv) as u32;
    let mut triangles = Vec::with_capacity(2 * nu * nv);
    for i in 0..nu {
        for j in 0..nv {
            let (a, b, c, d) = (idx(i, j), idx(i + 1, j), idx(i + 1, j + 1), idx(i, j + 1));
            triangles.push([a, b, c]);
            triangles.push([a, c, d]);
        }
    }
    Mesh::new(positions, triangles)
}

/// Makes every face of a convex mesh point away from its centroid.
fn orient_convex(mut m: Mesh) -> Mesh {
    let centroid = m.positions.iter().copied().sum::<Vec3>() / m.positions.len() as f32;
    for t in &mut m.triangles {
        let [a, b, c] = t.map(|i| m.positions[i as usize]);
        if (b - a).cross(c - a).dot((a + b + c) / 3.0 - centroid) < 0.0 {
            t.swap(1, 2);
        }
    }
    m
}

#[cfg(test)]
mod tests {
    #[test]
    fn custom_shapes_are_closed_and_sized() {
        for p in Primitive::ALL {
            let m = custom(p, 10, 10, 30.0, 50.0);
            let st = m.stats();
            assert!(st.is_closed_manifold(), "{p:?} {st:?}");
            let b = m.aabb();
            assert!(
                (b.max.z - 50.0).abs() < 1e-3 && b.min.z.abs() < 1e-3,
                "{p:?} {b:?}"
            );
            assert!((b.max.x - b.min.x) <= 30.0 + 1e-3, "{p:?} {b:?}");
        }
        // The box keeps its volume and has a cell grid: 2·10·10 + 4·10·10 cells, two
        // triangles each.
        let b = custom(Primitive::Cube, 10, 10, 30.0, 50.0);
        assert!((b.stats().volume - 30.0 * 30.0 * 50.0).abs() < 1e-1);
        assert_eq!(b.triangles.len(), 2 * (2 * 100 + 4 * 100));
    }

    use super::*;

    #[test]
    fn every_primitive_is_closed_outward_and_on_the_floor() {
        for p in Primitive::ALL {
            let m = p.build();
            let s = m.stats();
            assert!(s.is_closed_manifold(), "{p:?}: {s:?}");
            assert!(s.volume > 0.0, "{p:?} is inside out");
            let b = m.aabb();
            assert!(b.min.z.abs() < 1e-4, "{p:?} does not rest on the floor");
            assert!(
                b.size().max_element() <= DEFAULT_SIZE + 1e-3,
                "{p:?} too big: {:?}",
                b.size()
            );
        }
    }

    #[test]
    fn cylinder_matches_the_original_topology() {
        let m = Primitive::Cylinder.build();
        assert_eq!((m.positions.len(), m.triangles.len()), (240, 476));
    }
}
