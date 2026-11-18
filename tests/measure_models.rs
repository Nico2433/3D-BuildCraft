//! The section tool (`section`, behind Measure's "Section at height" and the MCP `measure`
//! `section`) on generated shapes, checked against their exact geometry, and on real flower
//! pots: the ignored test prints a table per pot (first round outer contour going down from
//! the top with a horizontal plane and with the automatic tilt, diameters, tilt, latency),
//! the pots read from the folder in `BUILDCRAFT_POTS` (default `/run/media/nico/KODAK/3D/Macetas/Robert/`;
//! a missing folder is skipped):
//! `cargo test --offline --release -j 4 --test measure_models -- --ignored --nocapture`.

use buildcraft3d::io;
use buildcraft3d::mesh::Mesh;
use buildcraft3d::primitives;
use buildcraft3d::section::{section, section_tilted, Outline, Section, Tilt};
use glam::{Mat4, Vec2, Vec3};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// How the pot's mouth is shaped.
#[derive(Clone, Copy)]
enum Rim {
    /// A flat top: sharp corners.
    Flat,
    /// A half circle of the wall's thickness over the top, in this many steps.
    Round(usize),
}

/// A flower pot: flared outer wall from `base` radius to `top` radius over `h`, wall
/// `wall` thick, floor `floor` thick. Profile counter-clockwise from the bottom axis.
fn pot_profile(base: f32, top: f32, h: f32, wall: f32, floor: f32, rim: Rim) -> Vec<(f32, f32)> {
    let ri = top - wall;
    let mut p = vec![(0.0, 0.0), (base, 0.0)];
    match rim {
        Rim::Flat => p.extend([(top, h), (ri, h)]),
        Rim::Round(steps) => {
            let (rm, zc, rr) = (top - wall / 2.0, h - wall / 2.0, wall / 2.0);
            p.push((top, zc));
            for s in 1..steps {
                let a = s as f32 / steps as f32 * std::f32::consts::PI;
                p.push((rm + rr * a.cos(), zc + rr * a.sin()));
            }
            p.push((ri, zc));
        }
    }
    let zi = if let Rim::Round(_) = rim {
        h - wall / 2.0
    } else {
        h
    };
    // The inner wall runs parallel to the outer one, down to the floor.
    let slope = (top - base) / h;
    p.push((ri - slope * (zi - floor), floor));
    p.push((0.0, floor));
    p
}

/// Radii at which the profile (a closed polygon in (r, z)) crosses height `z`, largest first.
fn profile_radii(profile: &[(f32, f32)], z: f32) -> Vec<f32> {
    let mut out = Vec::new();
    for i in 0..profile.len() {
        let ((r0, z0), (r1, z1)) = (profile[i], profile[(i + 1) % profile.len()]);
        if (z0 >= z) != (z1 >= z) {
            out.push(r0 + (r1 - r0) * (z - z0) / (z1 - z0));
        }
    }
    out.retain(|r| *r > 1e-6);
    out.sort_by(|a, b| b.total_cmp(a));
    out
}

/// A circle measured by the section.
#[derive(Clone, Copy, Debug)]
struct Found {
    diameter: f32,
    center: Vec3,
}

fn found(o: &Outline) -> Option<Found> {
    let r = o.round()?;
    Some(Found {
        diameter: r.diameter(),
        center: r.center,
    })
}

/// The horizontal section at `z`, its round closed contours (outer first) and the time (ms).
fn section_circles(mesh: &Mesh, z: f32) -> (Section, Vec<Found>, f64) {
    let t = Instant::now();
    let s = section(mesh, &Mat4::IDENTITY, Vec3::new(0.0, 0.0, z), Vec3::Z, None);
    let ms = t.elapsed().as_secs_f64() * 1e3;
    let circles = s
        .contours
        .iter()
        .filter(|c| c.outline.closed)
        .filter_map(|c| found(&c.outline))
        .collect();
    (s, circles, ms)
}

fn near(a: f32, b: f32, tolerance: f32) -> bool {
    (a - b).abs() <= tolerance
}

// ---- generated shapes ----------------------------------------------------------------

#[test]
fn cylinder_section_gives_its_diameter_at_any_height() {
    let m = primitives::prism(96, 20.0, 60.0);
    let (_, b, _) = section_circles(&m, 59.0);
    assert_eq!(b.len(), 1);
    assert!(near(b[0].diameter, 40.0, 0.05) && near(b[0].center.z, 59.0, 1e-3));
}

#[test]
fn cone_section_gives_the_radius_at_each_height() {
    let m = primitives::cone(96, 20.0, 40.0);
    // Radius 20 at the base falls linearly to 0 at 40 mm.
    for z in [5.0f32, 10.0, 20.0, 30.0] {
        let (_, b, _) = section_circles(&m, z);
        assert_eq!(b.len(), 1, "z {z}");
        assert!(near(b[0].diameter, 40.0 * (1.0 - z / 40.0), 0.05), "z {z}");
    }
}

#[test]
fn torus_section_gives_both_circles() {
    let m = primitives::torus(20.0, 5.0, 96, 48);
    let (s, b, _) = section_circles(&m, 5.0);
    let mut d: Vec<f32> = b.iter().map(|f| f.diameter).collect();
    d.sort_by(|a, b| b.total_cmp(a));
    assert_eq!(d.len(), 2);
    assert!(near(d[0], 50.0, 0.2) && near(d[1], 30.0, 0.2), "{d:?}");
    // The hole of the torus is "inside" the outer circle: a 10 mm band between them.
    assert!(near(s.walls()[0].thickness, 10.0, 0.2));
}

#[test]
fn hollow_pot_with_a_flat_rim() {
    let profile = pot_profile(25.0, 40.0, 90.0, 3.0, 4.0, Rim::Flat);
    let m = primitives::revolve(&profile, 128);
    let (s, b, _) = section_circles(&m, 80.0);
    let r = profile_radii(&profile, 80.0);
    assert_eq!(b.len(), 2);
    assert!(near(b[0].diameter, 2.0 * r[0], 0.1) && near(b[1].diameter, 2.0 * r[1], 0.1));
    let wall = s.walls()[0];
    assert!(near(wall.thickness, r[0] - r[1], 0.05) && wall.offset < 1e-3);
    // The plane through the flat rim itself: outer 80 and inner 74 at the top.
    let top = section(
        &m,
        &Mat4::IDENTITY,
        Vec3::new(40.0, 0.0, 90.0),
        Vec3::Z,
        None,
    );
    let d: Vec<f32> = top
        .contours
        .iter()
        .filter_map(|c| found(&c.outline))
        .map(|f| f.diameter)
        .collect();
    assert!(
        d.len() == 2 && near(d[0], 80.0, 0.1) && near(d[1], 74.0, 0.1),
        "{d:?}"
    );
}

#[test]
fn rounded_rims_need_a_slice_under_the_crown() {
    // 16 steps and 6 steps of rounding over the top: the section just under it is a
    // full circle of the wall's width there.
    for (steps, z) in [(16, 89.0), (6, 85.0)] {
        let profile = pot_profile(25.0, 40.0, 90.0, 3.0, 4.0, Rim::Round(steps));
        let m = primitives::revolve(&profile, 128);
        let r = profile_radii(&profile, z);
        let (s, b, _) = section_circles(&m, z);
        assert_eq!(b.len(), 2, "{steps}: {b:?}");
        assert!(near(b[0].diameter, 2.0 * r[0], 0.1) && near(b[1].diameter, 2.0 * r[1], 0.1));
        assert!(near(s.walls()[0].thickness, r[0] - r[1], 0.05));
    }
}

// ---- tilted shapes ----------------------------------------------------------------------

/// The outer radius of `profile` at height `z` (the largest crossing).
fn outer_radius(profile: &[(f32, f32)], z: f32) -> f32 {
    profile_radii(profile, z)[0]
}

#[test]
fn a_pot_tilted_by_20_degrees_gives_its_true_diameter_at_the_click() {
    let profile = pot_profile(25.0, 40.0, 90.0, 3.0, 4.0, Rim::Round(16));
    let m = primitives::revolve(&profile, 128);
    let model = Mat4::from_translation(Vec3::new(10.0, 20.0, 60.0))
        * Mat4::from_rotation_x(20f32.to_radians());
    let placed = m.transformed(&model);
    for (axial, azimuth) in [(60.0f32, 0.5f32), (85.0, 2.5), (89.0, 4.0)] {
        let r = outer_radius(&profile, axial);
        let p = model.transform_point3(Vec3::new(r * azimuth.cos(), r * azimuth.sin(), axial));
        let s = section_tilted(&placed, &Mat4::IDENTITY, p, Some(p), Tilt::Auto);
        let found = found(&s.contours[s.nearest(p).unwrap()].outline)
            .unwrap_or_else(|| panic!("axial {axial}: not round"));
        assert!(s.auto_tilted, "axial {axial}");
        assert!(
            near(found.diameter, 2.0 * r, 0.1),
            "axial {axial}: {} for {}",
            found.diameter,
            2.0 * r
        );
        assert!(
            near(s.tilt_degrees(), 20.0, 0.3),
            "axial {axial}: {}",
            s.tilt_degrees()
        );
    }
    // Horizontally the same clicks are ellipses.
    let p = model.transform_point3(Vec3::new(outer_radius(&profile, 60.0), 0.0, 60.0));
    let flat = section_tilted(&placed, &Mat4::IDENTITY, p, Some(p), Tilt::None);
    assert!(flat.contours[flat.nearest(p).unwrap()]
        .outline
        .round()
        .is_none());
}

#[test]
fn a_rim_cut_at_a_slant_on_an_upright_wall_is_measured_in_the_mouths_plane() {
    // The rim of a pot rises 4 mm over the 80 mm mouth along X (2.9 degrees).
    let profile = pot_profile(25.0, 40.0, 90.0, 3.0, 4.0, Rim::Round(16));
    let mut m = primitives::revolve(&profile, 128);
    let slope = 0.05;
    for p in &mut m.positions {
        if p.z > 80.0 {
            p.z += (p.z - 80.0) / 10.0 * slope * p.x;
        }
    }
    let rim = m
        .positions
        .iter()
        .copied()
        .max_by(|a, b| a.z.total_cmp(&b.z))
        .unwrap();
    // A click on the outside of the wall 1 mm under the highest part of the rim and one on
    // its crest.
    let under = m
        .positions
        .iter()
        .copied()
        .filter(|p| (p.z - (rim.z - 1.0)).abs() < 0.3 && p.x > 0.0)
        .max_by(|a, b| a.x.total_cmp(&b.x))
        .unwrap();
    for (name, p, tolerance) in [("under the rim", under, 1.5), ("rim crest", rim, 5.0)] {
        let s = section_tilted(&m, &Mat4::IDENTITY, p, Some(p), Tilt::Auto);
        let f = found(&s.contours[s.nearest(p).unwrap()].outline)
            .unwrap_or_else(|| panic!("{name}: not round"));
        assert!(near(f.diameter, 80.0, tolerance), "{name}: {}", f.diameter);
        assert!(
            near(s.tilt_degrees(), 2.9, 0.8),
            "{name}: {}",
            s.tilt_degrees()
        );
    }
}

// ---- the real pots --------------------------------------------------------------------

const DEFAULT_POTS: &str = "/run/media/nico/KODAK/3D/Macetas/Robert";

fn stl_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = read.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            stl_files(&p, out);
        } else if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("stl")) {
            out.push(p);
        }
    }
}

/// A round outer contour found by a click `depth` mm under the top.
struct Hit {
    depth: f32,
    outer: f32,
    inner: Option<f32>,
    tilt: f32,
    ms: f64,
}

impl Hit {
    fn show(&self) -> String {
        format!(
            "{:.1} mm under the top: outer {:.2}, inner {}, tilt {:.1} deg, {:.1} ms",
            self.depth,
            self.outer,
            self.inner.map_or("none".into(), |i| format!("{i:.2}")),
            self.tilt,
            self.ms
        )
    }
}

/// What a cut found round at the contour nearest to `near`, with its time.
fn hit(s: &Section, near: Vec3, depth: f32, ms: f64) -> Option<Hit> {
    let i = s.nearest(near)?;
    let outer = found(&s.contours[i].outline)?;
    let inner = s
        .walls()
        .into_iter()
        .find(|w| w.outer == i)
        .and_then(|w| found(&s.contours[w.inner].outline));
    Some(Hit {
        depth,
        outer: outer.diameter,
        inner: inner.map(|f| f.diameter),
        tilt: s.tilt_degrees(),
        ms,
    })
}

/// Going down from the top in 1 mm steps (at most 30 % of the height), the first click
/// `click(z)` whose cut by `cut` has a round contour at the click.
fn first_round(
    mesh: &Mesh,
    click: impl Fn(f32) -> Option<Vec3>,
    cut: impl Fn(Vec3) -> Section,
) -> Option<Hit> {
    let b = mesh.aabb();
    (0..=(0.3 * b.size().z).ceil() as usize).find_map(|k| {
        let z = b.max.z - 0.5 - k as f32;
        let p = click(z)?;
        let t = Instant::now();
        let s = cut(p);
        let ms = t.elapsed().as_secs_f64() * 1e3;
        hit(&s, p, b.max.z - z, ms)
    })
}

/// The mesh vertex in the slab around height `z` that lies furthest out along `dir` from
/// `centre` within 3 mm of that ray: a click on the outside of the wall.
fn outer_wall_click(mesh: &Mesh, centre: Vec2, dir: Vec2, z: f32) -> Option<Vec3> {
    mesh.positions
        .iter()
        .filter(|p| (p.z - z).abs() <= 0.3)
        .filter(|p| {
            let d = p.truncate() - centre;
            d.dot(dir) > 0.0 && (d - dir * d.dot(dir)).length() < 3.0
        })
        .max_by(|a, b| {
            (a.truncate() - centre)
                .length()
                .total_cmp(&(b.truncate() - centre).length())
        })
        .copied()
}

/// One table row per pot: the horizontal section going down 1 mm at a time from the top
/// (where the old tool needed to click), the same clicks on the outside of the wall with the
/// automatic tilt, and one click on the highest point of the rim.
fn row(name: &str, mesh: &Mesh) {
    let tris = mesh.triangles.len();
    let b = mesh.aabb();
    let flat = first_round(
        mesh,
        |z| Some(Vec3::new(0.0, 0.0, z)),
        |p| section(mesh, &Mat4::IDENTITY, p, Vec3::Z, None),
    );
    let Some(flat) = flat else {
        println!("BENCH {name}: {tris} tris, no round outer contour in the top 30 %");
        return;
    };
    // Where the pot's axis is: the centre of that contour, found again to read it.
    let z = b.max.z - flat.depth;
    let s0 = section(mesh, &Mat4::IDENTITY, Vec3::new(0.0, 0.0, z), Vec3::Z, None);
    let centre = s0
        .contours
        .iter()
        .find_map(|c| (c.depth == 0).then(|| found(&c.outline)).flatten())
        .unwrap()
        .center
        .truncate();
    let top = mesh
        .positions
        .iter()
        .copied()
        .max_by(|a, c| a.z.total_cmp(&c.z))
        .unwrap();
    let dir = (top.truncate() - centre).normalize_or_zero();
    let tilted = |near: Vec3| section_tilted(mesh, &Mat4::IDENTITY, near, Some(near), Tilt::Auto);
    let auto = first_round(mesh, |z| outer_wall_click(mesh, centre, dir, z), tilted);
    let t = Instant::now();
    let s = tilted(top);
    let ms = t.elapsed().as_secs_f64() * 1e3;
    let rim = hit(&s, top, 0.0, ms);
    let show = |h: Option<Hit>| h.map_or("no round contour".into(), |h| h.show());
    println!("BENCH {name} ({tris} tris)");
    println!("BENCH   horizontal  {}", show(Some(flat)));
    println!("BENCH   auto-tilt   {}", show(auto));
    println!("BENCH   rim top     {}", show(rim));
}

#[test]
#[ignore]
fn section_on_the_real_pots() {
    let dir = std::env::var("BUILDCRAFT_POTS").unwrap_or_else(|_| DEFAULT_POTS.into());
    let mut files = Vec::new();
    stl_files(Path::new(&dir), &mut files);
    if files.is_empty() {
        println!("BENCH real pots: {dir} not found, skipped");
    }
    for f in files {
        let Ok(loaded) = io::load(&f) else {
            println!("BENCH {}: could not load", f.display());
            continue;
        };
        for o in loaded.objects {
            let world = o.mesh.transformed(&o.transform);
            row(&f.file_name().unwrap().to_string_lossy(), &world);
        }
    }
}
