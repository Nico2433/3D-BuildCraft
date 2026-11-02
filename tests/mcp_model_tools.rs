//! The geometry and schemas behind the agent's modelling tools (`src/app/agent_model.rs`).

use buildcraft3d::cut::{cut_plane, CutPlane};
use buildcraft3d::mcp::tools;
use buildcraft3d::mesh::closest_on_triangle;
use buildcraft3d::primitives;
use buildcraft3d::texture_gen::Kind;
use glam::{Mat4, Vec3};
use serde_json::Value;

fn tool(name: &str) -> Value {
    tools()
        .into_iter()
        .find(|t| t["name"] == name)
        .unwrap_or_else(|| panic!("no tool {name}"))
}

#[test]
fn model_tools_are_listed_once_with_their_arguments() {
    let names: Vec<String> = tools()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_owned())
        .collect();
    let mut unique = names.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), names.len(), "{names:?}");
    for (name, args) in [
        ("insert_shape", &["shape", "size", "custom"][..]),
        ("split", &["z", "point", "normal", "rotation", "keep"]),
        (
            "emboss_text",
            &[
                "pattern",
                "image",
                "font",
                "projection",
                "bevel",
                "top_color",
                "side_color",
                "at",
            ],
        ),
        ("paint_area", &["point", "color", "coverage", "id"]),
        ("set_material", &["type", "color"]),
        (
            "apply_texture",
            &["texture", "image", "mode", "point", "coverage"],
        ),
        ("import", &["path", "units"]),
        ("load_image", &["path", "method", "levels", "invert"]),
    ] {
        let props = &tool(name)["inputSchema"]["properties"];
        for a in args {
            assert!(props.get(*a).is_some(), "{name} has no {a}");
        }
    }
    // The old forms still work: split by z alone, emboss_text without a pattern.
    assert!(tool("split")["inputSchema"]["required"].is_null());
    assert!(tool("emboss_text")["inputSchema"]["required"].is_null());
}

#[test]
fn every_texture_name_round_trips() {
    let listed = tool("apply_texture")["inputSchema"]["properties"]["texture"]["enum"].clone();
    let listed = listed.as_array().unwrap();
    assert_eq!(listed.len(), Kind::all().count());
    for k in Kind::all() {
        assert_eq!(Kind::from_name(&k.name()), Some(k));
    }
    assert_eq!(Kind::WaterDrops.name(), "water_drops");
    assert_eq!(Kind::from_name("Green_Marble"), Some(Kind::GreenMarble));
    assert_eq!(Kind::from_name("plywood"), None);
}

#[test]
fn closest_point_on_a_triangle_covers_its_regions() {
    let (a, b, c) = (Vec3::ZERO, Vec3::X * 2.0, Vec3::Y * 2.0);
    let near = |p: Vec3| closest_on_triangle(p, a, b, c);
    assert_eq!(near(Vec3::new(0.5, 0.5, 3.0)), Vec3::new(0.5, 0.5, 0.0));
    assert_eq!(near(Vec3::new(-1.0, -1.0, 0.0)), a);
    assert_eq!(near(Vec3::new(5.0, -1.0, 0.0)), b);
    assert_eq!(near(Vec3::new(1.0, -3.0, 1.0)), Vec3::new(1.0, 0.0, 0.0));
    assert!(near(Vec3::new(2.0, 2.0, 0.0)).distance(Vec3::new(1.0, 1.0, 0.0)) < 1e-6);
}

#[test]
fn the_closest_triangle_follows_the_placement() {
    // A 40 mm cube on the floor, moved 100 mm along X.
    let cube = primitives::Primitive::Cube.build();
    let model = Mat4::from_translation(Vec3::new(100.0, 0.0, 0.0));
    let top = Vec3::new(100.0, 0.0, 41.0);
    let (tri, at) = cube.closest_triangle(&model, top).unwrap();
    assert!(at.distance(Vec3::new(100.0, 0.0, 40.0)) < 1e-4);
    let [p, q, r] = cube.triangles[tri].map(|v| cube.positions[v as usize]);
    assert!((buildcraft3d::mesh::Mesh::face_normal(p, q, r) - Vec3::Z).length() < 1e-5);
    // The side facing -X is nearest to a point left of the cube.
    let (tri, _) = cube
        .closest_triangle(&model, Vec3::new(70.0, 0.0, 20.0))
        .unwrap();
    let [p, q, r] = cube.triangles[tri].map(|v| cube.positions[v as usize]);
    assert!((buildcraft3d::mesh::Mesh::face_normal(p, q, r) + Vec3::X).length() < 1e-5);
}

#[test]
fn a_plane_through_a_point_faces_its_normal_and_cuts_there() {
    let n = Vec3::new(1.0, 0.0, 1.0).normalize();
    let p = Vec3::new(0.0, 0.0, 20.0);
    let plane = CutPlane::through(p, n * 3.0).unwrap();
    assert!(plane.normal().distance(n) < 1e-5);
    assert!(plane.point().distance(p) < 1e-5);
    for down in [Vec3::NEG_Z, Vec3::Z] {
        let level = CutPlane::through(Vec3::ZERO, down).unwrap();
        assert!(level.normal().distance(down) < 1e-5);
    }
    assert!(CutPlane::through(p, Vec3::ZERO).is_none());
    // A 45 degree cut through a 40 mm cube's centre leaves two equal halves.
    let cube = primitives::Primitive::Cube.build();
    let (top, bottom) = cut_plane(&cube, &plane);
    let (vt, vb) = (top.stats().volume, bottom.stats().volume);
    assert!(
        (vt - vb).abs() < 1.0 && (vt + vb - 64000.0).abs() < 1.0,
        "{vt} {vb}"
    );
    assert!(top.aabb().max.x > 19.0 && top.aabb().min.z > -1e-3);
}
