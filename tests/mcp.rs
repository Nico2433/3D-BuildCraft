use buildcraft3d::mcp::{serve, text, tools};
use serde_json::{json, Value};

fn run(lines: &[Value]) -> (Vec<Value>, Vec<String>) {
    let input: String = lines.iter().map(|l| format!("{l}\n")).collect();
    let mut out = Vec::new();
    let mut calls = Vec::new();
    serve(input.as_bytes(), &mut out, |name, args| {
        calls.push(name.to_owned());
        if args["fail"] == true {
            Err("it failed".into())
        } else {
            Ok(vec![text("done")])
        }
    })
    .unwrap();
    let replies = String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    (replies, calls)
}

#[test]
fn speaks_the_handshake_lists_tools_and_calls_them() {
    let (r, calls) = run(&[
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05"}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"get_scene"}}),
        json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"edit","arguments":{"fail":true}}}),
        json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"rm_rf"}}),
        json!({"jsonrpc":"2.0","id":6,"method":"nope"}),
    ]);
    // The notification gets no reply.
    assert_eq!(r.len(), 6);
    assert_eq!(r[0]["result"]["protocolVersion"], "2024-11-05");
    assert_eq!(
        r[1]["result"]["tools"].as_array().unwrap().len(),
        tools().len()
    );
    assert_eq!(r[2]["result"]["isError"], false);
    assert_eq!(r[2]["result"]["content"][0]["text"], "done");
    assert_eq!(r[3]["result"]["isError"], true);
    assert_eq!(r[3]["result"]["content"][0]["text"], "it failed");
    assert_eq!(r[4]["error"]["code"], -32602);
    assert_eq!(r[5]["error"]["code"], -32601);
    // Unknown tools never reach the app.
    assert_eq!(calls, ["get_scene", "edit"]);
}

#[test]
fn unknown_versions_get_the_newest_and_bad_json_an_error() {
    let input = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"1999-01-01\"}}\nnot json\n";
    let mut out = Vec::new();
    serve(input.as_bytes(), &mut out, |_, _| Ok(vec![])).unwrap();
    let r: Vec<Value> = String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(r[0]["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(r[1]["error"]["code"], -32700);
}

#[test]
fn every_tool_has_an_object_schema() {
    for t in tools() {
        assert_eq!(t["inputSchema"]["type"], "object", "{}", t["name"]);
        assert!(!t["description"].as_str().unwrap().is_empty());
    }
}

#[test]
fn measurements_are_read_only() {
    use buildcraft3d::{mcp::measure, scene::Scene};
    let scene = Scene::default();
    let result = measure(&scene, &json!({"from":[1,2,3],"to":[4,6,3]})).unwrap();
    assert_eq!(result["distance"], 5.0);
    assert_eq!(result["differences"], json!([3.0, 4.0, 0.0]));
    assert!(measure(&scene, &json!({"ids":[1,2]})).is_err());
    assert!(measure(&scene, &json!({"from":[1]})).is_err());
    // The selection's measurements need one, rather than nulls and a volume of 0.
    assert!(measure(&scene, &json!({}))
        .unwrap_err()
        .contains("nothing is selected"));
    assert!(scene.objects.is_empty());
}

#[test]
fn arguments_are_checked_against_the_tool_schema() {
    use buildcraft3d::mcp::check_args;
    assert!(check_args("transform", &json!({"size": [10, 20, 30], "bottom": 0})).is_ok());
    assert!(check_args("get_scene", &json!(null)).is_ok());
    // Null counts as left out.
    assert!(check_args("transform", &json!({"position": null})).is_ok());
    let bad_op = check_args("edit", &json!({"operation": "select_all"})).unwrap_err();
    assert!(
        bad_op.starts_with("unknown operation: select_all; one of"),
        "{bad_op}"
    );
    let unknown = check_args("transform", &json!({"size": [1, 2, 3], "units": "cm"})).unwrap_err();
    assert!(
        unknown.starts_with(
            "unknown argument units: transform takes back, bottom, front, left, position"
        ),
        "{unknown}"
    );
    assert!(check_args("get_scene", &json!({"verbose": true}))
        .unwrap_err()
        .contains("takes no arguments"));
    assert_eq!(
        check_args("transform", &json!({"size": "3cm"})).unwrap_err(),
        "size must be [x, y, z] in mm, three numbers; for cm multiply by 10"
    );
    assert!(check_args("transform", &json!({"bottom": "2cm"}))
        .unwrap_err()
        .contains("a number in mm"));
    assert!(check_args("transform", &json!({"rotation": [0, 90]}))
        .unwrap_err()
        .contains("in degrees"));
    // Nested objects and alternatives.
    assert!(check_args(
        "insert_shape",
        &json!({"shape": "cube", "custom": {"roundness": 3}})
    )
    .is_ok());
    assert!(check_args(
        "insert_shape",
        &json!({"shape": "cube", "custom": {"round": 3}})
    )
    .unwrap_err()
    .contains("custom.round"));
    // The same inside a list of objects and in measure's section.
    let ok = json!({"filaments": [{"color": "#FF0000", "slot": 1}]});
    assert!(check_args("print_setup", &ok).is_ok());
    let e = check_args(
        "print_setup",
        &json!({"filaments": [{"color": "#FF0000", "slot": 1}, {"color": "#00FF00", "slot": 2, "slott": 3}]}),
    )
    .unwrap_err();
    assert_eq!(
        e,
        "unknown argument filaments[1].slott: print_setup filaments[1] takes color, slot"
    );
    let e = check_args("print_setup", &json!({"filaments": [{"color": 5}]})).unwrap_err();
    assert!(e.starts_with("filaments[0].color must be"), "{e}");
    let e = check_args(
        "measure",
        &json!({"section": {"id": 1, "nearby": [0, 0, 0]}}),
    )
    .unwrap_err();
    assert!(e.starts_with("unknown argument section.nearby"), "{e}");
    assert!(check_args("screenshot", &json!({"focus": "all"})).is_ok());
    assert!(check_args("screenshot", &json!({"focus": [1, 2]})).is_ok());
    assert!(check_args("screenshot", &json!({"focus": 3})).is_err());
    assert!(check_args("smooth", &json!({"passes": 2.5})).is_err());
    assert!(check_args("no_such_tool", &json!({})).is_err());
}

#[test]
fn selection_measurements_follow_transforms() {
    use buildcraft3d::{
        mcp::measure,
        primitives,
        scene::{Object, Scene, DEFAULT_COLOR},
    };
    let mut scene = Scene::default();
    let mut object = Object::new("cube", primitives::cube(2.0), DEFAULT_COLOR);
    object.id = 42;
    scene.objects.push(object);
    scene.selection.insert(42);
    let before = measure(&scene, &json!({})).unwrap();
    assert!((before["total"]["volume"].as_f64().unwrap() - 8.0).abs() < 1e-9);
    assert_eq!(before["total"]["surface_area"], 24.0);
    scene.objects[0].translation.x += 10.0;
    let after = measure(&scene, &json!({})).unwrap();
    assert_eq!(
        after["objects"][0]["min"][0].as_f64().unwrap(),
        before["objects"][0]["min"][0].as_f64().unwrap() + 10.0
    );
    assert!(
        (after["total"]["volume"].as_f64().unwrap() - before["total"]["volume"].as_f64().unwrap())
            .abs()
            < 1e-9
    );
}

#[test]
fn closed_app_still_lists_tools_and_returns_a_tool_error() {
    let missing = std::env::temp_dir().join(format!("buildcraft-missing-{}", uuid::Uuid::new_v4()));
    let input = format!(
        "{}\n{}\n",
        json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"get_scene"}})
    );
    let mut out = Vec::new();
    serve(input.as_bytes(), &mut out, |name, args| {
        buildcraft3d::mcp::call_app(&missing, name, args)
    })
    .unwrap();
    let replies: Vec<Value> = String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(
        replies[0]["result"]["tools"].as_array().unwrap().len(),
        tools().len()
    );
    assert_eq!(replies[1]["result"]["isError"], true);
    assert_eq!(
        replies[1]["result"]["content"][0]["text"],
        buildcraft3d::mcp::NOT_OPEN
    );
}

#[test]
fn list_fonts_filters_and_find_font_suggests() {
    use buildcraft3d::mcp::{find_font, list_fonts};
    let all: Vec<String> = [
        "DejaVu Sans",
        "DejaVu Serif",
        "Noto Sans Mono",
        "Liberation Serif",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let v = list_fonts(&all, Some("dejavu"), None);
    assert_eq!(v["count"], 2);
    assert_eq!(list_fonts(&all, None, None)["count"], 4);
    let few = list_fonts(&all, None, Some(1));
    assert_eq!(
        (few["count"].as_u64(), few["fonts"].as_array().map(Vec::len)),
        (Some(4), Some(1))
    );
    assert!(tools().iter().any(|t| t["name"] == "list_fonts"));
    assert_eq!(find_font(&all, " dejavu serif ").unwrap(), "DejaVu Serif");
    let err = find_font(&all, "Noto Serif").unwrap_err();
    assert!(
        err.contains("similar: DejaVu Serif, Noto Sans Mono, Liberation Serif"),
        "{err}"
    );
    assert!(err.contains("list_fonts"));
    let err = find_font(&all, "Comic").unwrap_err();
    assert!(!err.contains("similar"), "{err}");
}

#[test]
fn inline_files_land_in_their_own_folder() {
    use buildcraft3d::mcp::inline_file;
    let dir = tempfile::tempdir().unwrap();
    // No inline file: the arguments are used as they are.
    assert!(inline_file(&json!({"path": "/x.svg"}), "path", dir.path())
        .unwrap()
        .is_none());
    let svg = "<svg xmlns='http://www.w3.org/2000/svg'/>";
    let a = inline_file(
        &json!({"file_name": "../../shield.svg", "file_text": svg, "units": "mm"}),
        "path",
        dir.path(),
    )
    .unwrap()
    .unwrap();
    let path = std::path::PathBuf::from(a["path"].as_str().unwrap());
    assert!(path.starts_with(dir.path()));
    assert_eq!(path.file_name().unwrap(), "shield.svg");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), svg);
    assert_eq!(a["units"], "mm");
    // base64 for binary files, into a new folder each time.
    let b = inline_file(
        &json!({"file_name": "shield.svg", "file_base64": "/+7dzA=="}),
        "image",
        dir.path(),
    )
    .unwrap()
    .unwrap();
    let second = std::path::PathBuf::from(b["image"].as_str().unwrap());
    assert_ne!(second, path);
    assert_eq!(std::fs::read(&second).unwrap(), [0xFF, 0xEE, 0xDD, 0xCC]);
    for bad in [
        json!({"file_text": "x"}),
        json!({"file_name": "noext", "file_text": "x"}),
        json!({"file_name": "a.png", "file_base64": "*"}),
        json!({"file_name": "a.svg", "file_text": "x", "file_base64": "eA=="}),
        json!({"file_name": "a.svg", "file_text": "x", "path": "/a.svg"}),
    ] {
        assert!(inline_file(&bad, "path", dir.path()).is_err(), "{bad}");
    }
    for name in [
        "open",
        "import",
        "load_image",
        "emboss_text",
        "apply_texture",
    ] {
        let t = tools().into_iter().find(|t| t["name"] == name).unwrap();
        assert!(
            t["inputSchema"]["properties"]["file_base64"].is_object(),
            "{name}"
        );
        assert!(t["inputSchema"]["required"].is_null(), "{name}");
    }
}

#[test]
fn inline_svg_pictures_open_for_the_picture_tools() {
    use buildcraft3d::mcp::inline_file;
    let dir = tempfile::tempdir().unwrap();
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><rect width="100" height="100" fill="white"/><rect x="50" width="50" height="50"/></svg>"#;
    for (tool, key) in [
        ("emboss_text", "image"),
        ("apply_texture", "image"),
        ("load_image", "path"),
    ] {
        let a = inline_file(
            &json!({"file_name": "logo.svg", "file_text": svg}),
            key,
            dir.path(),
        )
        .unwrap()
        .unwrap();
        let path = std::path::PathBuf::from(a[key].as_str().unwrap());
        let img = buildcraft3d::picture::open(&path).unwrap().to_rgba8();
        // The black square is the top right quarter; the rest is white.
        assert_eq!(img.get_pixel(768, 256).0, [0, 0, 0, 255], "{tool}");
        assert_eq!(img.get_pixel(256, 768).0, [255, 255, 255, 255], "{tool}");
        // The tool's inline fields name the formats it takes.
        let t = tools().into_iter().find(|t| t["name"] == tool).unwrap();
        let props = &t["inputSchema"]["properties"];
        assert_eq!(
            props["file_text"]["description"], "the whole file as text: SVG",
            "{tool}"
        );
        assert!(props[key]["description"]
            .as_str()
            .unwrap()
            .contains("PNG, JPEG or SVG"));
    }
    let open = tools().into_iter().find(|t| t["name"] == "open").unwrap();
    let text = open["inputSchema"]["properties"]["file_text"]["description"]
        .as_str()
        .unwrap();
    assert!(text.contains("OBJ") && text.contains("SVG"), "{text}");
}
