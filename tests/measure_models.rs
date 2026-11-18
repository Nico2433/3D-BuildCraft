//! The section tool (`section`, behind Measure's "Section at height" and the MCP `measure`
//! `section`) on generated shapes, checked against their exact geometry, and on real flower
//! pots: the ignored test prints a table (outer and inner diameter, centre, wall, latency),
//! the pots read from `/run/media/nico/KODAK/3D/Macetas/Robert/` (missing files are skipped):
//! `cargo test --offline --release -j 4 --test measure_models -- --ignored --nocapture`.

use buildcraft3d::io;
use buildcraft3d::mesh::Mesh;
use buildcraft3d::primitives;
use buildcraft3d::section::{section, Outline, Section};
use glam::{Mat4, Vec3};
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

// ---- the real pots --------------------------------------------------------------------

const POTS: &str = "/run/media/nico/KODAK/3D/Macetas/Robert";

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

/// One table row: a click on the mouth means a cut under the crown of a rounded rim, so go
/// down from the top in 1 mm steps (at most 30 % of the height) to the first slice with a
/// round outer contour, and print it: the depth under the top, outer and inner diameter,
/// centre, wall and the time of that cut.
fn row(name: &str, mesh: &Mesh) {
    let b = mesh.aabb();
    let tris = mesh.triangles.len();
    for k in 0..=(0.3 * b.size().z).ceil() as usize {
        let z = b.max.z - 0.5 - k as f32;
        let t = Instant::now();
        let s = section(mesh, &Mat4::IDENTITY, Vec3::new(0.0, 0.0, z), Vec3::Z, None);
        let ms = t.elapsed().as_secs_f64() * 1e3;
        let outer = s
            .contours
            .iter()
            .position(|c| c.depth == 0 && c.outline.round().is_some());
        let Some(outer) = outer else { continue };
        let o = found(&s.contours[outer].outline).unwrap();
        let wall = s.walls().into_iter().find(|w| w.outer == outer);
        let inner = wall.and_then(|w| found(&s.contours[w.inner].outline));
        println!(
            "BENCH {name}: {tris} tris; {:.1} mm under the top: outer {:.2}, inner {}, centre ({:.2}, {:.2}), wall {}, {ms:.1} ms",
            b.max.z - z,
            o.diameter,
            inner.map_or("none".into(), |i| format!("{:.2}", i.diameter)),
            o.center.x,
            o.center.y,
            wall.map_or("none".into(), |w| format!("{:.2}", w.thickness)),
        );
        return;
    }
    println!("BENCH {name}: {tris} tris, no round outer contour in the top 30 %");
}

#[test]
#[ignore]
fn section_on_the_real_pots() {
    let mut files = Vec::new();
    stl_files(Path::new(POTS), &mut files);
    if files.is_empty() {
        println!("BENCH real pots: {POTS} not found, skipped");
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
