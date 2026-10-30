use anyhow::Result;
use buildcraft3d::assistant::{Config, Conversation, Event, Protocol};
use buildcraft3d::mcp::text;
use serde_json::{json, Value};
use std::cell::RefCell;

/// Every request the endpoint got: url, headers, body.
type Sent = std::rc::Rc<RefCell<Vec<(String, Vec<(String, String)>, Value)>>>;

/// An endpoint that answers from a script and records what it was sent.
#[allow(clippy::type_complexity)]
fn scripted(
    answers: Vec<Value>,
) -> (
    impl Fn(&str, &[(&str, String)], &Value) -> Result<Value>,
    Sent,
) {
    let sent = std::rc::Rc::new(RefCell::new(Vec::new()));
    let log = sent.clone();
    let answers = RefCell::new(answers.into_iter());
    let post = move |url: &str, headers: &[(&str, String)], body: &Value| {
        let h = headers
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect();
        log.borrow_mut().push((url.to_owned(), h, body.clone()));
        Ok(answers.borrow_mut().next().expect("no more answers"))
    };
    (post, sent)
}

fn cfg(protocol: Protocol) -> Config {
    Config {
        protocol,
        base_url: "http://host/v1/".into(),
        model: "m".into(),
        ..Config::default()
    }
}

#[test]
fn openai_runs_the_tools_it_asks_for_then_answers() {
    let (post, sent) = scripted(vec![
        json!({"choices":[{"message":{"role":"assistant","content":"","tool_calls":[
            {"id":"c1","type":"function","function":{"name":"insert_shape","arguments":"{\"shape\":\"cube\"}"}}]}}]}),
        json!({"choices":[{"message":{"role":"assistant","content":"Done: a cube."}}]}),
    ]);
    let mut calls = Vec::new();
    let mut events = Vec::new();
    let mut chat = Conversation::default();
    chat.send_api(
        &cfg(Protocol::OpenAi),
        Some("k"),
        "make a cube",
        &post,
        &mut |name, args| {
            calls.push((name.to_owned(), args));
            Ok(vec![text("{\"objects\":[1]}")])
        },
        &mut |e| events.push(e),
    )
    .unwrap();
    assert_eq!(
        calls,
        [("insert_shape".to_owned(), json!({"shape":"cube"}))]
    );
    assert_eq!(
        events,
        [
            Event::Tool("insert_shape {\"shape\":\"cube\"}".into()),
            Event::Reply("Done: a cube.".into())
        ]
    );
    let sent = sent.borrow();
    assert_eq!(sent[0].0, "http://host/v1/chat/completions");
    assert_eq!(
        sent[0].1,
        [("Authorization".to_owned(), "Bearer k".to_owned())]
    );
    assert_eq!(sent[0].2["messages"][0]["role"], "system");
    // No screenshot: OpenAI tool results are text only.
    assert!(sent[0].2["tools"]
        .as_array()
        .unwrap()
        .iter()
        .all(|t| t["function"]["name"] != "screenshot"));
    // The tool's answer went back with its call id.
    let second = sent[1].2["messages"].as_array().unwrap();
    let tool = second.last().unwrap();
    assert_eq!(tool["role"], "tool");
    assert_eq!(tool["tool_call_id"], "c1");
    assert_eq!(tool["content"], "{\"objects\":[1]}");
}

#[test]
fn anthropic_sends_tool_results_and_images() {
    let (post, sent) = scripted(vec![
        json!({"type":"message","content":[{"type":"text","text":"Looking."},
            {"type":"tool_use","id":"t1","name":"screenshot","input":{}}],"stop_reason":"tool_use"}),
        json!({"type":"message","content":[{"type":"text","text":"It is a cube."}],"stop_reason":"end_turn"}),
    ]);
    let mut events = Vec::new();
    let mut chat = Conversation::default();
    chat.send_api(
        &cfg(Protocol::Anthropic),
        Some("key"),
        "what is there?",
        &post,
        &mut |_, _| {
            Ok(vec![
                json!({"type":"image","data":"AAA","mimeType":"image/png"}),
            ])
        },
        &mut |e| events.push(e),
    )
    .unwrap();
    assert_eq!(
        events,
        [
            Event::Reply("Looking.".into()),
            Event::Tool("screenshot".into()),
            Event::Reply("It is a cube.".into())
        ]
    );
    let sent = sent.borrow();
    assert_eq!(sent[0].0, "http://host/v1/messages");
    assert!(sent[0]
        .1
        .contains(&("x-api-key".to_owned(), "key".to_owned())));
    let result = &sent[1].2["messages"][2]["content"][0];
    assert_eq!(result["type"], "tool_result");
    assert_eq!(result["tool_use_id"], "t1");
    assert_eq!(result["content"][0]["source"]["data"], "AAA");
}

#[test]
fn endpoint_errors_and_missing_models_are_reported() {
    let (post, _) = scripted(vec![json!({"error":{"message":"bad key"}})]);
    let mut chat = Conversation::default();
    let err = chat
        .send_api(
            &cfg(Protocol::OpenAi),
            None,
            "hi",
            &post,
            &mut |_, _| Ok(vec![]),
            &mut |_| {},
        )
        .unwrap_err();
    assert_eq!(err.to_string(), "bad key");
    let none = Config {
        model: " ".into(),
        ..cfg(Protocol::OpenAi)
    };
    assert!(chat
        .send_api(
            &none,
            None,
            "hi",
            &post,
            &mut |_, _| Ok(vec![]),
            &mut |_| {}
        )
        .is_err());
}

#[test]
fn detected_model_lists() {
    use buildcraft3d::assistant::parse_models;
    assert_eq!(
        parse_models(r#"{"models":[{"name":"qwen:14b"},{"name":"other"},{"name":"qwen:14b"},{"name":""},{}]}"#, true).unwrap(),
        vec!["qwen:14b", "other"]
    );
    assert_eq!(
        parse_models(r#"{"data":[{"id":"local-model"}]}"#, false).unwrap(),
        vec!["local-model"]
    );
    assert!(parse_models(r#"{"data":[]}"#, false).unwrap().is_empty());
    assert!(parse_models("bad json", true).is_err());
    assert!(parse_models("{}", false).is_err());
}

#[test]
fn agent_registration_arguments() {
    use buildcraft3d::assistant::AgentKind;
    let mcp = vec!["/path with spaces/buildcraft3d".into(), "--mcp".into()];
    for (kind, prefix) in [
        (
            AgentKind::Claude,
            vec!["mcp", "add", "-s", "user", "buildcraft3d", "--"],
        ),
        (AgentKind::Codex, vec!["mcp", "add", "buildcraft3d", "--"]),
        (
            AgentKind::OpenCode,
            vec!["mcp", "add", "--global", "buildcraft3d", "--"],
        ),
        (
            AgentKind::Gemini,
            vec!["mcp", "add", "-s", "user", "buildcraft3d"],
        ),
    ] {
        let expected: Vec<String> = prefix
            .into_iter()
            .map(str::to_owned)
            .chain(mcp.iter().cloned())
            .collect();
        assert_eq!(kind.connect_args(&mcp), Some(expected));
    }
    assert_eq!(AgentKind::Antigravity.connect_args(&mcp), None);
}
