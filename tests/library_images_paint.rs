use buildcraft3d::image3d::{self, Mode, Options};
use buildcraft3d::io::threemf;
use buildcraft3d::library;
use buildcraft3d::primitives;
use buildcraft3d::thumbnail;
use glam::{Mat4, Vec3};
use std::fs;
use std::path::Path;

const RED: [u8; 4] = [255, 0, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];

fn touch(root: &Path, relative: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, b"").unwrap();
}

fn assert_red_and_blue(pixels: &[u8]) {
    for (dominant, other) in [(0, 2), (2, 0)] {
        let count = pixels
            .chunks_exact(4)
            .filter(|p| p[3] == 255 && p[dominant] > 80 && p[other] < 20)
            .count();
        assert!(count > 20, "too few pixels for channel {dominant}: {count}");
    }
}

#[test]
fn library_list_filters_and_sorts_nested_model_folders() {
    let dir = tempfile::tempdir().unwrap();
    for path in [
        "notes/notes.txt",
        "alpha/b/c/d/x.stl",
        "Zulu/model.obj",
        "Beta/model.3mf",
        ".hidden/model.stl",
        ".hidden.stl",
        "z.stl",
        "A.obj",
    ] {
        touch(dir.path(), path);
    }
    let entries = library::list(dir.path());
    let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["alpha", "Beta", "Zulu", "A.obj", "z.stl"]);
    for (i, entry) in entries.iter().enumerate() {
        assert_eq!(entry.is_dir, i < 3);
        assert_eq!(entry.path, dir.path().join(&entry.name));
    }
}

#[test]
fn library_search_limits_depth_and_result_count() {
    let dir = tempfile::tempdir().unwrap();
    touch(dir.path(), "shallow/Needle.STL");
    touch(dir.path(), "a/b/c/d/e/f/g/h/i/needle.stl");
    let found = library::search(dir.path(), "NEEDLE");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].path, dir.path().join("shallow/Needle.STL"));
    assert!(!found[0].is_dir);

    let many = tempfile::tempdir().unwrap();
    for i in 0..510 {
        touch(many.path(), &format!("model_{i:03}.stl"));
    }
    let capped = library::search(many.path(), "MODEL_");
    assert_eq!(capped.len(), library::MAX_RESULTS);
    let paths: std::collections::HashSet<_> = capped.iter().map(|e| &e.path).collect();
    assert_eq!(paths.len(), library::MAX_RESULTS);
    assert!(capped.iter().all(|e| !e.is_dir && e.path.is_file()));
}

#[test]
fn thumbnail_parts_show_both_object_colors() {
    let cube = primitives::cube(10.0);
    let shifted = cube.transformed(&Mat4::from_translation(Vec3::new(20.0, 0.0, 0.0)));
    let pixels = thumbnail::render_parts(&[(&cube, RED), (&shifted, BLUE)], 96);
    assert_eq!(pixels.len(), 96 * 96 * 4);
    assert_red_and_blue(&pixels);
}

#[test]
fn thumbnail_top_region_paint_overrides_object_color() {
    let mut cube = primitives::cube(10.0);
    let top = cube
        .triangle_positions()
        .position(|[a, b, c]| (b - a).cross(c - a).normalize().dot(Vec3::Z) > 0.99)
        .unwrap();
    let region = cube.face_region(top, 10.0);
    assert_eq!(region.len(), 2);
    cube.colors = vec![[0; 4]; cube.triangles.len()];
    for face in region {
        cube.colors[face] = RED;
    }
    let pixels = thumbnail::render(&cube, BLUE, 96);
    assert_red_and_blue(&pixels);
}

#[test]
fn uniform_reliefs_are_closed_flat_slabs_at_expected_heights() {
    let dir = tempfile::tempdir().unwrap();
    let depth = 2.0;
    for (name, luminance, height) in [("white", 255, 0.6), ("black", 0, 0.6 + depth)] {
        let path = dir.path().join(format!("{name}.png"));
        image::GrayImage::from_pixel(10, 10, image::Luma([luminance]))
            .save(&path)
            .unwrap();
        let o = Options {
            mode: Mode::HeightMap,
            size: 10.0,
            depth,
            smooth: 0.0,
            ..Options::new(path.clone())
        };
        let mesh = image3d::build(&o).unwrap();
        assert!(!mesh.triangles.is_empty());
        let stats = mesh.stats();
        assert!(stats.is_closed_manifold(), "{name}: {stats:?}");
        assert_eq!(stats.components, 1);
        assert!(mesh.aabb().min.z.abs() < 1e-6);
        assert!((mesh.aabb().max.z - height).abs() < 1e-6);
        assert!(mesh
            .positions
            .iter()
            .all(|p| p.z.abs() < 1e-6 || (p.z - height).abs() < 1e-6));
    }
}

#[test]
fn white_outline_rejects_an_image_with_nothing_to_extrude() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("white.png");
    image::GrayImage::from_pixel(10, 10, image::Luma([255]))
        .save(&path)
        .unwrap();
    let result = image3d::build(&Options {
        mode: Mode::Edge,
        size: 10.0,
        depth: 2.0,
        ..Options::new(path.clone())
    });
    assert!(result.is_err(), "all-white outline unexpectedly succeeded");
}

#[test]
fn topology_length_mismatch_disables_stale_face_colors() {
    let mut mesh = primitives::cube(10.0);
    mesh.colors = vec![RED; mesh.triangles.len()];
    assert!(mesh.has_colors());
    assert_eq!(mesh.face_color(0), Some(RED));
    mesh.triangles.pop().unwrap();
    assert!(!mesh.has_colors());
    for i in 0..mesh.colors.len() {
        assert_eq!(mesh.face_color(i), None, "triangle {i}");
    }
}

#[test]
fn append_preserves_paint_and_pads_unpainted_faces_in_both_orders() {
    let unpainted = primitives::cube(10.0);
    let mut painted = unpainted.clone();
    painted.colors = vec![[0; 4]; painted.triangles.len()];
    painted.colors[0] = RED;
    painted.colors[3] = BLUE;
    for paint_first in [true, false] {
        let (first, second) = if paint_first {
            (&painted, &unpainted)
        } else {
            (&unpainted, &painted)
        };
        let mut combined = first.clone();
        combined.append(second);
        let mut expected = vec![[0; 4]; combined.triangles.len()];
        let offset = if paint_first {
            0
        } else {
            unpainted.triangles.len()
        };
        expected[offset..offset + painted.triangles.len()].copy_from_slice(&painted.colors);
        assert_eq!(combined.triangles.len(), 2 * unpainted.triangles.len());
        assert_eq!(combined.colors, expected);
        assert!(combined.has_colors());
    }
}

#[test]
fn transforms_keep_paint_aligned_with_translated_and_mirrored_triangles() {
    let mut mesh = primitives::cube(10.0);
    mesh.colors = vec![[0; 4]; mesh.triangles.len()];
    mesh.colors[0] = RED;
    mesh.colors[3] = BLUE;
    for transform in [
        Mat4::from_translation(Vec3::new(3.0, 4.0, 5.0)),
        Mat4::from_scale(Vec3::new(-1.0, 1.0, 1.0)),
    ] {
        let transformed = mesh.transformed(&transform);
        assert_eq!(transformed.colors, mesh.colors);
        for (i, triangle) in mesh.triangles.iter().enumerate() {
            let mut expected = *triangle;
            if transform.determinant() < 0.0 {
                expected.swap(1, 2);
            }
            assert_eq!(transformed.triangles[i], expected);
            assert_eq!(transformed.face_color(i), mesh.face_color(i));
            for vertex in expected {
                let want = transform.transform_point3(mesh.positions[vertex as usize]);
                assert!(transformed.positions[vertex as usize].abs_diff_eq(want, 1e-6));
            }
        }
    }
}

#[test]
fn original_style_3mf_only_uses_face_colors_for_different_materials() {
    for second_material in [0, 1] {
        let xml = format!(
            r##"<model unit="millimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02">
<resources>
<basematerials id="1">
<base name="red" displaycolor="#FF0000FF"/>
<base name="blue" displaycolor="#0000FFFF"/>
</basematerials>
<object id="2" type="model" pid="1" pindex="0"><mesh><vertices>
<vertex x="0" y="0" z="0"/><vertex x="1" y="0" z="0"/>
<vertex x="0" y="1" z="0"/><vertex x="1" y="1" z="0"/>
</vertices><triangles>
<triangle v1="0" v2="1" v3="2" p1="0"/>
<triangle v1="1" v2="3" v3="2" p1="{second_material}"/>
</triangles></mesh></object>
</resources><build><item objectid="2"/></build></model>"##
        );
        let objects = threemf::parse_model(&xml, "original").unwrap();
        assert_eq!(objects.len(), 1);
        let object = &objects[0];
        assert_eq!(object.color, RED);
        assert_eq!(object.mesh.triangles.len(), 2);
        if second_material == 0 {
            assert!(!object.mesh.has_colors());
            assert_eq!(object.mesh.face_color(0), None);
            assert_eq!(object.mesh.face_color(1), None);
        } else {
            assert!(object.mesh.has_colors());
            assert_eq!(object.mesh.face_color(0), Some(RED));
            assert_eq!(object.mesh.face_color(1), Some(BLUE));
        }
    }
}
