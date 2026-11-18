//! The agent's tools with extreme arguments (NaN-like, zero, negative, 1e9, 1e-9, huge
//! texts, the edges of every range) on a small scene: each call must give an error or a
//! result without panicking and within a time bound. The app runs a tool on its UI thread,
//! so what bounds the work is the ranges of `mcp::args::limits`; these tests prove it.
//! `emboss_text` with a size of 100000 once ran for minutes and killed the app.

use buildcraft3d::cut;
use buildcraft3d::emboss;
use buildcraft3d::mcp::{args, measure};
use buildcraft3d::mesh::Mesh;
use buildcraft3d::primitives::{self, Primitive};
use buildcraft3d::scene::{Object, Scene, DEFAULT_COLOR};
use buildcraft3d::{image3d, outline, solid};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

/// The longest any call may take, debug test profile included.
const BOUND: Duration = Duration::from_secs(20);

/// Values an agent could send for a number, none of which is sane for every argument.
/// (JSON has no NaN or infinity; 1e300 turns into infinity as an f32.)
fn extremes() -> Vec<Value> {
    vec![
        json!(0),
        json!(-1),
        json!(-1e9),
        json!(1e-9),
        json!(1e-300),
        json!(1e9),
        json!(1e300),
        json!(0.5),
        json!(null),
        json!("1e9"),
        json!([1e9, 1e9, 1e9]),
        json!([0, 0, 0]),
        json!([-1e300, 1e300, 1e300]),
        json!({}),
    ]
}

/// A cube 40 mm on a side, the scene every tool is tried on.
fn cube() -> Mesh {
    Primitive::Cube.build()
}

fn scene() -> (Scene, u64) {
    let mut scene = Scene::default();
    let id = scene.add(Object::new("cube", cube(), DEFAULT_COLOR));
    scene.selection = [id].into();
    (scene, id)
}

/// Runs `f` and checks it neither panics nor takes more than [`BOUND`].
fn timed<T>(what: &str, f: impl FnOnce() -> T) -> (T, Duration) {
    let start = Instant::now();
    let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
        .unwrap_or_else(|_| panic!("{what} panicked"));
    let took = start.elapsed();
    assert!(took < BOUND, "{what} took {took:?}");
    (out, took)
}

/// Every key of `base` replaced by each extreme in turn (and every key alone with the
/// others left out), for a tool whose arguments are `base`.
fn variants(base: &Value) -> Vec<Value> {
    let mut all = vec![base.clone()];
    for key in base.as_object().unwrap().keys() {
        for v in extremes() {
            let mut a = base.clone();
            a[key] = v;
            all.push(a);
        }
    }
    all
}

/// A 64×48 PNG, for the picture tools.
fn picture(dir: &std::path::Path) -> std::path::PathBuf {
    let path = dir.join("p.png");
    let img = image::RgbaImage::from_fn(64, 48, |x, y| {
        image::Rgba([(x * 4) as u8, (y * 5) as u8, 90, 255])
    });
    img.save(&path).unwrap();
    path
}

/// Whether `e` is free of the letters only Spanish text has (UI text is Spanish only in
/// the i18n table; errors an agent reads are English).
fn english(e: &str) -> bool {
    !e.contains(|c| "áéíóúñÁÉÍÓÚÑ¿¡".contains(c))
}

/// Whether `e` reads as an English error that names the argument.
fn explains(e: &str, key: &str) -> bool {
    e.contains(key) && english(e)
}

#[test]
fn emboss_text_with_a_huge_size_is_refused_at_once() {
    // The original crash: size 100000 on a cube.
    let a = json!({"text": "X", "size": 100000});
    let (e, took) = timed("emboss size 100000", || args::emboss(&a).unwrap_err());
    assert_eq!(e, "size must be a number between 1 and 200 mm");
    eprintln!("emboss size 100000 refused in {took:?}");
    assert!(took < Duration::from_millis(50), "{took:?}");
}

#[test]
fn emboss_text_survives_extreme_arguments_and_the_edges_of_its_ranges() {
    let mut all = variants(&json!({
        "text": "Hello", "size": 10, "width": 30, "depth": 1, "angle": 0, "offset": [0, 0],
        "bevel": true, "engrave": false, "projection": "planar",
    }));
    // Long texts: at the limit, and far past it.
    all.push(json!({"text": "W".repeat(args::limits::TEXT_CHARS)}));
    all.push(json!({"text": "W".repeat(100_000)}));
    for projection in ["planar", "spherical"] {
        all.push(
            json!({"text": "W".repeat(args::limits::TEXT_CHARS), "bevel": true,
            "projection": projection}),
        );
    }
    // The edges of every range, with the slowest settings together.
    for projection in ["planar", "cylindrical", "spherical"] {
        for (size, depth, width) in [
            (1.0, 0.1, 0.05),
            (200.0, 50.0, 4000.0),
            (200.0, 50.0, 100.0),
        ] {
            all.push(
                json!({"text": "Hello", "size": size, "depth": depth, "width": width,
                "bevel": true, "projection": projection}),
            );
        }
    }
    let target = cube();
    let mut slowest = Duration::ZERO;
    for a in all {
        let what = format!("emboss_text {a:.80}");
        let (done, took) = timed(&what, || {
            args::emboss(&a).and_then(|params| {
                emboss::apply(&target, &params)
                    .map(|_| ())
                    .map_err(|e| format!("{e:#}"))
            })
        });
        slowest = slowest.max(took);
        if let Err(e) = done {
            assert!(english(&e), "{what}: {e}");
        }
    }
    eprintln!("slowest emboss_text: {slowest:?}");
}

#[test]
fn emboss_text_rejects_each_bad_number_naming_it_and_its_range() {
    for (key, value) in [
        ("size", json!(0)),
        ("size", json!(1e9)),
        ("width", json!(-5)),
        ("depth", json!(1e9)),
        ("depth", json!(0)),
        ("angle", json!(1e9)),
        ("offset", json!([1e9, 0])),
    ] {
        let mut a = json!({"text": "X"});
        a[key] = value;
        let e = args::emboss(&a).unwrap_err();
        assert!(explains(&e, key) && e.contains("between"), "{key}: {e}");
    }
}

#[test]
fn insert_shape_custom_stays_within_its_subdivisions_and_size() {
    for a in variants(
        &json!({"latitude": 10, "longitude": 10, "width": 40, "height": 40, "roundness": 5}),
    )
    .into_iter()
    .chain([
        json!({"latitude": 48, "longitude": 48, "roundness": 10}),
        json!({"latitude": 3, "longitude": 3, "width": 1000, "height": 1000}),
    ]) {
        for shape in [
            Primitive::Sphere,
            Primitive::Torus,
            Primitive::Cube,
            Primitive::Cone,
        ] {
            let what = format!("custom {shape:?} {a:.80}");
            let (r, _) = timed(&what, || {
                args::custom(&a).and_then(|(lat, lon, w, h, round)| {
                    let mesh = primitives::custom(shape, lat, lon, w, h);
                    solid::round(&mesh, f32::from(round) / 10.0)
                        .map(|m| m.triangles.len())
                        .map_err(|e| format!("{e:#}"))
                })
            });
            if let Ok(triangles) = r {
                assert!(triangles < 1_000_000, "{what}: {triangles} triangles");
            }
        }
    }
}

#[test]
fn load_image_extremes_are_refused_or_quick() {
    let dir = tempfile::tempdir().unwrap();
    let path = picture(dir.path());
    let base = json!({"path": path, "size": 80, "depth": 3, "levels": 8, "smooth": 1});
    let mut all = variants(&base);
    for method in ["contour", "height_map", "edge", "stamp"] {
        for (size, depth) in [(1.0, 0.1), (500.0, 100.0)] {
            all.push(json!({"path": path, "method": method, "size": size, "depth": depth}));
        }
    }
    for a in all {
        let what = format!("load_image {a:.100}");
        let (r, _) = timed(&what, || {
            args::load_image(&a)
                .and_then(|o| image3d::build(&o).map_err(|e| format!("{e:#}")))
                .map(|m| m.triangles.len())
        });
        if let Err(e) = r {
            assert!(english(&e), "{what}: {e}");
        }
    }
}

#[test]
fn extrude_outline_hollow_smooth_and_simplify_survive_extremes() {
    let target = cube();
    for a in variants(&json!({"width": 2, "height": 10, "smooth_outer": 0.5, "smooth_inner": 0.5}))
        .into_iter()
        .chain([json!({"width": 100, "height": 1000, "smooth_outer": 1, "smooth_inner": 1})])
    {
        let what = format!("extrude_outline {a:.100}");
        timed(&what, || {
            args::extrude_outline(&a, 40.0)
                .and_then(|o| {
                    outline::outline(std::slice::from_ref(&target), &o)
                        .map_err(|e| format!("{e:#}"))
                })
                .map(|_| ())
        })
        .0
        .ok();
    }
    for wall in extremes()
        .into_iter()
        .chain([json!(0.05), json!(1000), json!(2)])
    {
        let what = format!("hollow {wall}");
        timed(&what, || {
            args::wall(&json!({ "wall": wall }))
                .and_then(|w| solid::hollow(&target, w).map_err(|e| format!("{e:#}")))
                .map(|_| ())
        })
        .0
        .ok();
    }
    for passes in extremes().into_iter().chain([json!(6), json!(7), json!(3)]) {
        for edges in [false, true] {
            let what = format!("smooth {passes} edges {edges}");
            timed(&what, || {
                args::passes(&json!({ "passes": passes })).and_then(|p| {
                    let amount = p as f32 / solid::SMOOTH_STEPS as f32;
                    solid::smooth_by(&target, amount, edges).map_err(|e| format!("{e:#}"))
                })
            })
            .0
            .ok();
        }
    }
    for remove in extremes()
        .into_iter()
        .chain([json!(1), json!(0.99), json!(0.5)])
    {
        let what = format!("simplify {remove}");
        timed(&what, || {
            args::simplify_remove(&json!({ "remove": remove }))
                .and_then(|r| target.try_simplified(1.0 - r).map_err(|e| format!("{e:#}")))
                .map(|_| ())
        })
        .0
        .ok();
    }
}

#[test]
fn split_and_transform_survive_extremes() {
    let target = cube();
    let base = json!({"z": 5, "point": [0, 0, 5], "normal": [0, 0, 1], "width": 10, "length": 10});
    for mut a in variants(&base) {
        // Normal and rotation together are an error of their own.
        a.as_object_mut().unwrap().remove("rotation");
        let what = format!("split {a:.100}");
        timed(&what, || {
            args::split(&a, glam::Vec3::ZERO).and_then(|(plane, keep)| {
                cut::split(&target, &plane, keep).map_err(|e| format!("{e:#}"))
            })
        })
        .0
        .ok();
    }
    for key in ["rotation", "size", "position"] {
        for v in extremes() {
            let a = json!({ key: v });
            timed(&format!("transform {a:.100}"), || args::transform(&a))
                .0
                .ok();
        }
    }
    for side in ["bottom", "left", "right", "front", "back"] {
        for v in extremes() {
            let a = json!({ side: v });
            timed(&format!("transform {a:.100}"), || args::transform(&a))
                .0
                .ok();
        }
    }
}

#[test]
fn measure_survives_extreme_numbers() {
    let (scene, id) = scene();
    let p = |v: Value| json!([v, v, v]);
    let mut calls = Vec::new();
    for v in extremes() {
        calls.push(
            json!({"from": p(v.clone()), "to": [0, 0, 0], "snap": true, "snap_tolerance": v}),
        );
        calls.push(json!({"circle": [p(v.clone()), [0, 1, 2], [3, 2, 1]]}));
        calls.push(json!({"section": {"id": id, "z": v, "near": p(v.clone())}}));
        calls.push(json!({"section": {"id": id, "point": p(v.clone()), "normal": p(v.clone())}}));
        calls.push(json!({"ids": [v.clone(), id]}));
        calls.push(json!({"diameter_of": v}));
    }
    for a in calls {
        let what = format!("measure {a:.100}");
        timed(&what, || measure(&scene, &a)).0.ok();
    }
}

#[test]
fn text_arguments_are_capped() {
    let long = "x".repeat(args::limits::INFO_CHARS + 1);
    let e = args::text(&json!({"title": long}), "title", args::limits::INFO_CHARS).unwrap_err();
    assert!(e.contains("title") && e.contains("at most"), "{e}");
    let many: Vec<u64> = (0..=args::limits::LIST_ITEMS as u64).collect();
    assert!(args::ids(&json!(many), "ids")
        .unwrap_err()
        .contains("at most"));
}

#[test]
fn a_missing_picture_and_an_unwritable_folder_say_what_to_do() {
    let e = args::emboss(&json!({"pattern": "image"})).unwrap_err();
    assert!(e.starts_with("pattern image needs a picture"), "{e}");
    let object = buildcraft3d::io::ModelObject {
        name: "c".into(),
        mesh: cube(),
        transform: glam::Mat4::IDENTITY,
        color: [1, 2, 3, 255],
    };
    for name in ["x.3mf", "x.stl", "x.glb"] {
        let path = std::path::Path::new("/no/such/folder").join(name);
        let e = buildcraft3d::io::save(&path, std::slice::from_ref(&object), &Default::default())
            .unwrap_err();
        let e = format!("{e:#}");
        assert!(
            e.contains("/no/such/folder does not exist for the app")
                && e.contains("existing folder"),
            "{e}"
        );
        assert!(english(&e));
    }
    // A folder that exists but cannot be written in says so, not that it is missing.
    let e = buildcraft3d::io::save(
        std::path::Path::new("/proc/x.3mf"),
        std::slice::from_ref(&object),
        &Default::default(),
    )
    .unwrap_err();
    let e = format!("{e:#}");
    assert!(e.contains("/proc is not writable for the app"), "{e}");
}

#[test]
fn save_takes_only_an_absolute_file_path_with_an_extension() {
    let dir = std::env::temp_dir();
    let err = |p: &str| args::save_path(&json!({ "path": p })).unwrap_err();
    assert!(err("box.3mf").contains("give an absolute path"));
    assert!(err("relative/box.stl").contains("give an absolute path"));
    // A folder is not a file, and is not turned into "/tmp.3mf".
    for folder in [dir.to_str().unwrap(), "/", "/tmp/"] {
        assert!(err(folder).contains("is a folder"), "{folder}");
    }
    assert!(err("/no/such/folder/box").contains("has no extension"));
    assert!(err("/no/such/folder/box.svg").contains("cannot be saved"));
    assert!(args::save_path(&json!({}))
        .unwrap_err()
        .contains("path is missing"));
    let path = dir.join(format!("buildcraft-save-{}.stl", uuid::Uuid::new_v4()));
    let path_str = path.to_str().unwrap();
    assert_eq!(
        args::save_path(&json!({ "path": path_str })),
        Ok(path.clone())
    );
    std::fs::write(&path, b"x").unwrap();
    assert!(err(path_str).contains("pass overwrite"));
    let ok = args::save_path(&json!({ "path": path_str, "overwrite": true }));
    std::fs::remove_file(&path).unwrap();
    assert_eq!(ok, Ok(path));
}

#[test]
fn notices_raised_for_an_agent_are_english_in_a_spanish_ui() {
    use buildcraft3d::i18n::{in_english, tr, Lang};
    assert_eq!(tr(Lang::Es, "Could not save:"), "No se pudo guardar:");
    assert_eq!(
        in_english(|| tr(Lang::Es, "Could not save:")),
        "Could not save:"
    );
    assert_eq!(tr(Lang::Es, "Could not save:"), "No se pudo guardar:");
}

#[test]
fn files_without_triangles_or_larger_than_the_limit_are_refused() {
    use buildcraft3d::io;
    let dir = tempfile::tempdir().unwrap();
    let write = |name: &str, bytes: &[u8]| {
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    };
    let load = |path: &std::path::Path| io::load(path).err().map(|e| format!("{e:#}"));
    // Empty, zeros and text with no triangles in any of the formats that read plain files.
    for name in [
        "a.stl", "a.obj", "a.ply", "a.svg", "a.3mf", "a.glb", "a.gltf",
    ] {
        for bytes in [&b""[..], &[0u8; 200][..], &b"garbage"[..]] {
            let e = load(&write(name, bytes)).unwrap_or_else(|| panic!("{name} {bytes:?} loaded"));
            assert!(english(&e), "{e}");
        }
    }
    let e = load(&write("empty.stl", b"solid x\nendsolid x\n")).unwrap();
    assert!(e.contains("no triangles"), "{e}");
    // A drawing whose real size is far over the limit.
    let huge = br#"<svg xmlns="http://www.w3.org/2000/svg" width="1e9" height="10"><rect width="1e9" height="10"/></svg>"#;
    let e = load(&write("huge.svg", huge)).unwrap();
    assert!(e.contains("the most is 10000 mm"), "{e}");
    let ok = br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="10"><rect width="100" height="10"/></svg>"#;
    assert!(load(&write("ok.svg", ok)).is_none());
    // The same limit as insert_shape's size.
    assert_eq!(io::MAX_SIZE_MM, args::limits::OBJECT_SIZE.max);
    assert!(io::check_size(glam::Vec3::new(10_000.0, 1.0, 1.0)).is_ok());
    assert!(io::check_size(glam::Vec3::new(10_001.0, 1.0, 1.0)).is_err());
    assert!(io::check_size(glam::Vec3::NAN).is_err());
}

#[test]
fn load_image_of_a_blank_picture_gives_a_solid_in_every_method() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("blank.png");
    image::RgbaImage::from_pixel(32, 32, image::Rgba([255, 255, 255, 255]))
        .save(&path)
        .unwrap();
    for method in ["contour", "height_map", "edge", "stamp"] {
        let a = json!({"path": path.to_str().unwrap(), "method": method});
        let opts = args::load_image(&a).unwrap();
        match image3d::build(&opts) {
            Ok(mesh) => assert!(!mesh.is_empty(), "{method}"),
            Err(e) => assert!(english(&format!("{e:#}")), "{method}: {e:#}"),
        }
    }
}
