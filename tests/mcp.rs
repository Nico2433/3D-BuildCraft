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
    assert!(scene.objects.is_empty());
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
