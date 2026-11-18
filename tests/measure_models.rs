//! The two ways to measure a round rim or section, side by side on generated shapes and on
//! real flower pots: (A) circular edges, sharp and soft (`edge_loops`), and (B) the
//! cross-section by a horizontal plane (`section`). The unit tests below check the generated
//! shapes against their exact geometry; the ignored test prints a table per model, the real
//! pots read from `/run/media/nico/KODAK/3D/Macetas/Robert/` (missing files are skipped):
//! `cargo test --offline --release -j 4 --test measure_models -- --ignored --nocapture`.

use buildcraft3d::edge_loops::{EdgeLoop, FeatureEdges};
use buildcraft3d::io;
use buildcraft3d::mesh::Mesh;
use buildcraft3d::primitives;
use buildcraft3d::section::{section_at_height, Section};
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

/// A circle measured by one of the methods.
#[derive(Clone, Copy, Debug)]
struct Found {
    diameter: f32,
    center: Vec3,
    /// RMS deviation from the circle, fraction of the radius.
    rms: f32,
    /// Where it sits along Z, for sorting out which rim it is.
    z: f32,
}

fn found(l: &EdgeLoop) -> Option<Found> {
    let r = l.round()?;
    Some(Found {
        diameter: r.diameter(),
        center: r.center,
        rms: l.fit?.rms,
        z: r.center.z,
    })
}

/// Round edges of `mesh` (world space) with creases over `angle` degrees whose circle is
/// near-horizontal (axis within 10° of Z).
fn edge_circles(mesh: &Mesh, angle: f32) -> (Vec<Found>, f64) {
    let t = Instant::now();
    let feats = FeatureEdges::compute_at(mesh, angle);
    let mut out: Vec<Found> = feats
        .loops(&Mat4::IDENTITY)
        .filter(|l| {
            l.round()
                .is_some_and(|r| r.axis.is_some_and(|a| a.z.abs() > 0.985))
        })
        .filter_map(|l| found(&l))
        .collect();
    out.sort_by(|a, b| b.z.total_cmp(&a.z).then(b.diameter.total_cmp(&a.diameter)));
    (out, t.elapsed().as_secs_f64() * 1e3)
}

/// The circular contours of the section at `z`, outer first, with the wall of each pair.
fn section_circles(mesh: &Mesh, z: f32) -> (Section, Vec<Found>, f64) {
    let t = Instant::now();
    let s = section_at_height(mesh, &Mat4::IDENTITY, z);
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
fn cylinder_rim_by_edges_and_by_section() {
    let m = primitives::prism(96, 20.0, 60.0);
    let (a, _) = edge_circles(&m, 40.0);
    assert!(a
        .iter()
        .any(|f| near(f.z, 60.0, 0.01) && near(f.diameter, 40.0, 0.05)));
    assert!(a
        .iter()
        .any(|f| near(f.z, 0.0, 0.01) && near(f.diameter, 40.0, 0.05)));
    let (_, b, _) = section_circles(&m, 59.0);
    assert_eq!(b.len(), 1);
    assert!(near(b[0].diameter, 40.0, 0.05) && near(b[0].center.z, 59.0, 1e-3));
}

#[test]
fn cone_base_by_edges_and_radius_by_section() {
    let m = primitives::cone(96, 20.0, 40.0);
    let (a, _) = edge_circles(&m, 40.0);
    assert!(a
        .iter()
        .any(|f| near(f.z, 0.0, 0.01) && near(f.diameter, 40.0, 0.05)));
    // Radius 20 at the base falls linearly to 0 at 40 mm.
    for z in [5.0f32, 10.0, 20.0, 30.0] {
        let (_, b, _) = section_circles(&m, z);
        assert_eq!(b.len(), 1, "z {z}");
        assert!(near(b[0].diameter, 40.0 * (1.0 - z / 40.0), 0.05), "z {z}");
    }
}

#[test]
fn torus_has_no_edges_but_its_section_gives_both_circles() {
    let m = primitives::torus(20.0, 5.0, 96, 48);
    let (a, _) = edge_circles(&m, 40.0);
    assert!(a.is_empty());
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
    let (a, _) = edge_circles(&m, 40.0);
    // Both rim edges at the top: outer 80 and inner 74.
    for d in [80.0, 74.0] {
        assert!(
            a.iter()
                .any(|f| near(f.z, 90.0, 0.01) && near(f.diameter, d, 0.1)),
            "{d}: {a:?}"
        );
    }
    let (s, b, _) = section_circles(&m, 80.0);
    let r = profile_radii(&profile, 80.0);
    assert_eq!(b.len(), 2);
    assert!(near(b[0].diameter, 2.0 * r[0], 0.1) && near(b[1].diameter, 2.0 * r[1], 0.1));
    let wall = s.walls()[0];
    assert!(near(wall.thickness, r[0] - r[1], 0.05) && wall.offset < 1e-3);
}

#[test]
fn rounded_rim_is_found_by_soft_edges_and_by_section_but_not_by_sharp_edges() {
    let profile = pot_profile(25.0, 40.0, 90.0, 3.0, 4.0, Rim::Round(16));
    let m = primitives::revolve(&profile, 128);
    // Sharp: only the base and the floor's edges, nothing near the top.
    let (sharp, _) = edge_circles(&m, 40.0);
    assert!(!sharp.iter().any(|f| f.z > 80.0), "{sharp:?}");
    // Soft: the rim's rings, the widest (the mouth, 80 mm) among them.
    let (soft, _) = edge_circles(&m, buildcraft3d::edge_loops::SOFT_DEG);
    assert!(
        soft.iter()
            .any(|f| f.z > 86.0 && near(f.diameter, 80.0, 0.2)),
        "{soft:?}"
    );
    // Section 1 mm under the top: the arc's width there.
    let z = 89.0;
    let r = profile_radii(&profile, z);
    let (s, b, _) = section_circles(&m, z);
    assert_eq!(b.len(), 2, "{b:?}");
    assert!(near(b[0].diameter, 2.0 * r[0], 0.1) && near(b[1].diameter, 2.0 * r[1], 0.1));
    assert!(near(s.walls()[0].thickness, r[0] - r[1], 0.05));
}

#[test]
fn a_coarse_rounded_rim_needs_the_section() {
    // 6 steps of 30° over the top: bends of 30° are still under the 40° sharp threshold.
    let profile = pot_profile(25.0, 40.0, 90.0, 3.0, 4.0, Rim::Round(6));
    let m = primitives::revolve(&profile, 128);
    let (sharp, _) = edge_circles(&m, 40.0);
    assert!(!sharp.iter().any(|f| f.z > 80.0));
    let (soft, _) = edge_circles(&m, buildcraft3d::edge_loops::SOFT_DEG);
    assert!(soft
        .iter()
        .any(|f| f.z > 86.0 && near(f.diameter, 80.0, 0.2)));
    let (_, b, _) = section_circles(&m, 85.0);
    assert_eq!(b.len(), 2);
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

/// A circle that could be the mouth: near the top, as wide as `min_d` mm.
fn mouth_like(f: &Found, top: f32, height: f32, min_d: f32) -> bool {
    f.z >= top - (0.12 * height).max(8.0) && f.diameter >= min_d
}

fn one(f: &Found) -> String {
    format!(
        "d{:.1}@z{:.1} c({:.1},{:.1}) fit{:.2}%",
        f.diameter,
        f.z,
        f.center.x,
        f.center.y,
        f.rms * 100.0
    )
}

/// The mouth-like rings found by an edge pass: how many, the highest and the widest.
fn describe(f: &[Found], top: f32, height: f32, min_d: f32) -> String {
    let m: Vec<&Found> = f
        .iter()
        .filter(|f| mouth_like(f, top, height, min_d))
        .collect();
    let widest = m.iter().max_by(|a, b| a.diameter.total_cmp(&b.diameter));
    match (m.first(), widest) {
        (Some(t), Some(w)) => format!("{} rings; top {}; widest {}", m.len(), one(t), one(w)),
        _ => "none".into(),
    }
}

/// Method B scanned down from the top in 1 mm steps over the top 30 % of the height:
/// the first height with a round outer contour at least `min_d` wide (the mouth's top),
/// and the widest round one with its inner contour and the wall.
fn scan(mesh: &Mesh, top: f32, height: f32, min_d: f32) -> String {
    let t = Instant::now();
    let steps = (0.3 * height).ceil() as usize;
    /// The widest round slice: its height, outer circle, inner circle and wall.
    type Widest = (f32, Found, Option<Found>, f32);
    let (mut first, mut widest): (Option<f32>, Option<Widest>) = (None, None);
    let mut rounds = 0;
    for k in 0..=steps {
        let z = top - 0.5 - k as f32;
        let s = section_at_height(mesh, &Mat4::IDENTITY, z);
        let Some(c) = s.contours.iter().find(|c| c.depth == 0) else {
            continue;
        };
        let Some(f) = found(&c.outline).filter(|f| f.diameter >= min_d) else {
            continue;
        };
        rounds += 1;
        first.get_or_insert(z);
        if widest.as_ref().is_none_or(|w| f.diameter > w.1.diameter) {
            let wall = s.walls().into_iter().find(|w| w.outer == c.outline.chain);
            let inner = wall.and_then(|w| found(&s.contours[w.inner].outline));
            widest = Some((z, f, inner, wall.map_or(0.0, |w| w.thickness)));
        }
    }
    let ms = t.elapsed().as_secs_f64() * 1e3;
    match (first, widest) {
        (Some(z0), Some((z, f, inner, wall))) => format!(
            "{rounds} round slices, first at z{z0:.1}; widest {} at slice z{z:.1}; inner {}; wall {wall:.2} ({ms:.0} ms for {} slices)",
            one(&f),
            inner.map_or("none".into(), |i| format!("d{:.1}", i.diameter)),
            steps + 1
        ),
        _ => format!("no round outer contour ({ms:.0} ms for {} slices)", steps + 1),
    }
}

fn report(name: &str, mesh: &Mesh) {
    let b = mesh.aabb();
    let (top, height) = (b.max.z, b.size().z);
    let wide = b.size().x.max(b.size().y);
    // The mouth is at least a third as wide as the pot's footprint.
    let min_d = wide / 3.0;
    println!(
        "BENCH {name}: {} tris, box {:.1}x{:.1}x{:.1}, top z {:.1}",
        mesh.triangles.len(),
        b.size().x,
        b.size().y,
        b.size().z,
        top
    );
    for angle in [40.0, 15.0, 10.0, buildcraft3d::edge_loops::SOFT_DEG, 3.0] {
        let (f, ms) = edge_circles(mesh, angle);
        println!(
            "BENCH   A {angle}°: {} round loops, {ms:.0} ms; {}",
            f.len(),
            describe(&f, top, height, min_d)
        );
    }
    println!("BENCH   B {}", scan(mesh, top, height, min_d));
    let (sharp, _) = edge_circles(mesh, 40.0);
    let low: Vec<String> = sharp
        .iter()
        .filter(|f| f.z <= b.min.z + 1.0)
        .map(one)
        .collect();
    println!("BENCH   A 40° base loops: {}", low.join("; "));
    let (s, base, ms) = section_circles(mesh, b.min.z + 1.0);
    println!(
        "BENCH   B base+1: {} contours, {ms:.0} ms; {}",
        s.contours.len(),
        base.iter().take(3).map(one).collect::<Vec<_>>().join("; ")
    );
}

#[test]
#[ignore]
fn compare_methods_on_generated_and_real_pots() {
    let shapes: Vec<(&str, Mesh)> = vec![
        ("cylinder r20 h60", primitives::prism(96, 20.0, 60.0)),
        ("cone r20 h40", primitives::cone(96, 20.0, 40.0)),
        ("torus 20/5", primitives::torus(20.0, 5.0, 96, 48)),
        (
            "pot flat rim",
            primitives::revolve(&pot_profile(25.0, 40.0, 90.0, 3.0, 4.0, Rim::Flat), 128),
        ),
        (
            "pot rounded rim x16",
            primitives::revolve(
                &pot_profile(25.0, 40.0, 90.0, 3.0, 4.0, Rim::Round(16)),
                128,
            ),
        ),
        (
            "pot rounded rim x6",
            primitives::revolve(&pot_profile(25.0, 40.0, 90.0, 3.0, 4.0, Rim::Round(6)), 128),
        ),
    ];
    for (name, m) in &shapes {
        report(name, m);
    }
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
            let name = f.file_name().unwrap().to_string_lossy().to_string();
            report(&name, &world);
        }
    }
}
