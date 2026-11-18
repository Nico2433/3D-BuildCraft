//! The open app's MCP server over Streamable HTTP (MCP 2025-06-18), so agents reach the
//! window at a fixed local URL while it is open and no process of ours outlives it. One
//! JSON-RPC message per POST, answered with plain JSON; no event streams or sessions.
//!
//! The token in the URL is the password (Codex cannot send a header of its own), and the
//! Host and Origin checks keep web pages out through DNS rebinding or cross-site requests.

use super::Reply;
use anyhow::{anyhow, Context};
use serde_json::Value;
use std::io::{BufRead, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// The port agents connect to unless the settings name another.
pub const DEFAULT_PORT: u16 = 47833;
/// Largest request body accepted.
const MAX_BODY: usize = 16 << 20;
/// Largest request line and headers together.
const MAX_HEAD: u64 = 64 << 10;

/// The URL agents register.
pub fn url(port: u16, token: &str) -> String {
    format!("http://127.0.0.1:{port}/mcp/{token}")
}

/// The secret in the URL, made once and kept (readable by this user only) so registered
/// URLs keep working after restarts.
pub fn token() -> anyhow::Result<String> {
    static TOKEN: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    if let Some(t) = TOKEN.get() {
        return Ok(t.clone());
    }
    let file = crate::settings::dirs()
        .map(|d| d.config_dir().join("mcp-token"))
        .ok_or_else(|| anyhow!("no config folder"))?;
    let t = load_or_create(&file)?;
    Ok(TOKEN.get_or_init(|| t).clone())
}

fn load_or_create(file: &std::path::Path) -> anyhow::Result<String> {
    if let Ok(t) = std::fs::read_to_string(file) {
        if uuid::Uuid::parse_str(t.trim()).is_ok() {
            return Ok(t.trim().to_owned());
        }
    }
    let t = uuid::Uuid::new_v4().to_string();
    super::write_private(file, t.as_bytes()).context("saving the MCP token")?;
    Ok(t)
}

/// The server on 127.0.0.1; it stops when dropped.
pub struct Server {
    stop: Arc<AtomicBool>,
}

impl Server {
    pub fn start(
        port: u16,
        call: impl Fn(&str, Value) -> Reply + Clone + Send + 'static,
    ) -> anyhow::Result<Self> {
        let token = token()?;
        let listener = std::net::TcpListener::bind(("127.0.0.1", port))
            .with_context(|| format!("cannot listen on 127.0.0.1:{port}"))?;
        let stop = super::accept(listener, move |stream| {
            exchange(stream, port, &token, call.clone())
        })?;
        Ok(Self { stop })
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Answers one request and closes the connection.
fn exchange(
    stream: std::net::TcpStream,
    port: u16,
    token: &str,
    mut call: impl FnMut(&str, Value) -> Reply,
) {
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(10)));
    let Ok(mut out) = stream.try_clone() else {
        return;
    };
    let mut input = std::io::BufReader::new(stream);
    let (status, body) = match read_head(&mut input) {
        None => (400, None),
        Some(head) => match screen(&head, port, token) {
            Err(status) => (status, None),
            Ok(len) => {
                let mut body = vec![0; len];
                match input.read_exact(&mut body) {
                    Ok(()) => answer(&body, &mut call),
                    Err(_) => (400, None),
                }
            }
        },
    };
    if let Err(e) = write_response(&mut out, status, body.as_ref()) {
        log::info!("MCP HTTP client left: {e}");
    }
}

/// A request line and its headers, names in lower case.
#[derive(Debug, PartialEq)]
struct Head {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
}

impl Head {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

/// Reads up to the blank line after the headers; None when it is not HTTP/1.x or too long.
fn read_head(input: &mut impl BufRead) -> Option<Head> {
    let mut limited = input.take(MAX_HEAD);
    let mut line = String::new();
    limited.read_line(&mut line).ok()?;
    let mut parts = line.split_whitespace();
    let (method, path, version) = (parts.next()?, parts.next()?, parts.next()?);
    if !version.starts_with("HTTP/1.") || parts.next().is_some() {
        return None;
    }
    let (method, path) = (method.to_owned(), path.to_owned());
    let mut headers = Vec::new();
    loop {
        line.clear();
        if limited.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            return Some(Head {
                method,
                path,
                headers,
            });
        }
        let (name, value) = line.split_once(':')?;
        headers.push((name.trim().to_ascii_lowercase(), value.trim().to_owned()));
    }
}

/// Checks a request before its body is read: the body's length, or the status to answer.
fn screen(head: &Head, port: u16, token: &str) -> Result<usize, u16> {
    let local = [format!("127.0.0.1:{port}"), format!("localhost:{port}")];
    let host = head.header("host").unwrap_or("");
    if !local.iter().any(|h| h.eq_ignore_ascii_case(host)) {
        return Err(403);
    }
    if let Some(origin) = head.header("origin") {
        if !local
            .iter()
            .any(|h| origin.eq_ignore_ascii_case(&format!("http://{h}")))
        {
            return Err(403);
        }
    }
    let path = head.path.split('?').next().unwrap_or("");
    if path != format!("/mcp/{token}") {
        return Err(404);
    }
    if head.method != "POST" {
        return Err(405);
    }
    if head.header("transfer-encoding").is_some() {
        return Err(400);
    }
    let len = head
        .header("content-length")
        .ok_or(411_u16)?
        .parse::<usize>()
        .map_err(|_| 400_u16)?;
    if len > MAX_BODY {
        return Err(413);
    }
    Ok(len)
}

/// The status and JSON for one JSON-RPC message: 202 and nothing for a notification.
fn answer(body: &[u8], call: &mut impl FnMut(&str, Value) -> Reply) -> (u16, Option<Value>) {
    match super::reply(body, call) {
        None => (202, None),
        Some(r)
            if [super::PARSE_ERROR, super::INVALID_REQUEST]
                .iter()
                .any(|c| r["error"]["code"] == *c) =>
        {
            (400, Some(r))
        }
        Some(r) => (200, Some(r)),
    }
}

fn write_response(out: &mut impl Write, status: u16, body: Option<&Value>) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        411 => "Length Required",
        413 => "Content Too Large",
        _ => "",
    };
    let mut head = format!("HTTP/1.1 {status} {reason}\r\n");
    if status == 405 {
        head.push_str("Allow: POST\r\n");
    }
    let body = body.map(Value::to_string).unwrap_or_default();
    if !body.is_empty() {
        head.push_str("Content-Type: application/json\r\n");
    }
    write!(
        out,
        "{head}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )?;
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const TOKEN: &str = "0123abcd-0000-4000-8000-000000000000";

    fn head(text: &str) -> Option<Head> {
        read_head(&mut std::io::Cursor::new(text.as_bytes()))
    }

    fn post(extra: &str) -> Head {
        head(&format!(
            "POST /mcp/{TOKEN} HTTP/1.1\r\nHost: 127.0.0.1:4000\r\n{extra}Content-Length: 12\r\n\r\n"
        ))
        .unwrap()
    }

    #[test]
    fn parses_the_request_head() {
        let h = head("POST /mcp/x?a=1 HTTP/1.1\r\nHost: localhost:9\r\nContent-Length:  5 \r\nX-Two: a:b\r\n\r\nhello").unwrap();
        assert_eq!(h.method, "POST");
        assert_eq!(h.path, "/mcp/x?a=1");
        assert_eq!(h.header("host"), Some("localhost:9"));
        assert_eq!(h.header("content-length"), Some("5"));
        assert_eq!(h.header("x-two"), Some("a:b"));
        assert_eq!(h.header("origin"), None);
        // Bare newlines are accepted too.
        assert!(head("GET / HTTP/1.0\nHost: a\n\n").is_some());
        for bad in [
            "",
            "POST /mcp HTTP/2\r\n\r\n",
            "POST /mcp\r\n\r\n",
            "POST /mcp HTTP/1.1\r\nno colon\r\n\r\n",
            "POST /mcp HTTP/1.1\r\nHost: a\r\n",
        ] {
            assert_eq!(head(bad), None, "{bad:?}");
        }
        let long = format!(
            "POST / HTTP/1.1\r\nX: {}\r\n\r\n",
            "a".repeat(MAX_HEAD as usize)
        );
        assert_eq!(head(&long), None);
    }

    #[test]
    fn screens_path_token_and_length() {
        assert_eq!(screen(&post(""), 4000, TOKEN), Ok(12));
        assert_eq!(screen(&post(""), 4000, "another-token"), Err(404));
        let mut h = post("");
        h.path = format!("/mcp/{TOKEN}?x=1");
        assert_eq!(screen(&h, 4000, TOKEN), Ok(12));
        h.path = "/mcp".into();
        assert_eq!(screen(&h, 4000, TOKEN), Err(404));
        h.path = format!("/mcp/{TOKEN}/");
        assert_eq!(screen(&h, 4000, TOKEN), Err(404));
        for method in ["GET", "DELETE"] {
            let mut h = post("");
            h.method = method.into();
            assert_eq!(screen(&h, 4000, TOKEN), Err(405));
        }
        assert_eq!(
            screen(&post("Transfer-Encoding: chunked\r\n"), 4000, TOKEN),
            Err(400)
        );
        let mut h = post("");
        h.headers.retain(|(n, _)| n != "content-length");
        assert_eq!(screen(&h, 4000, TOKEN), Err(411));
        h.headers.push(("content-length".into(), "-1".into()));
        assert_eq!(screen(&h, 4000, TOKEN), Err(400));
        h.headers.pop();
        h.headers
            .push(("content-length".into(), (MAX_BODY + 1).to_string()));
        assert_eq!(screen(&h, 4000, TOKEN), Err(413));
    }

    #[test]
    fn screens_host_and_origin() {
        let with_host = |host: &str| {
            let mut h = post("");
            h.headers.retain(|(n, _)| n != "host");
            if !host.is_empty() {
                h.headers.push(("host".into(), host.into()));
            }
            screen(&h, 4000, TOKEN)
        };
        assert_eq!(with_host("localhost:4000"), Ok(12));
        assert_eq!(with_host("LOCALHOST:4000"), Ok(12));
        for bad in [
            "",
            "127.0.0.1",
            "127.0.0.1:4001",
            "evil.example:4000",
            "[::1]:4000",
        ] {
            assert_eq!(with_host(bad), Err(403), "{bad}");
        }
        for good in ["http://127.0.0.1:4000", "http://localhost:4000"] {
            let h = post(&format!("Origin: {good}\r\n"));
            assert_eq!(screen(&h, 4000, TOKEN), Ok(12), "{good}");
        }
        for bad in [
            "https://evil.example",
            "null",
            "http://127.0.0.1:4001",
            "https://localhost:4000",
        ] {
            let h = post(&format!("Origin: {bad}\r\n"));
            assert_eq!(screen(&h, 4000, TOKEN), Err(403), "{bad}");
        }
        // The Host check comes first, so a foreign host never learns whether a token is right.
        let mut h = post("");
        h.path = "/mcp/wrong".into();
        h.headers[0].1 = "evil.example".into();
        assert_eq!(screen(&h, 4000, TOKEN), Err(403));
    }

    #[test]
    fn answers_requests_and_notifications() {
        let mut calls = Vec::new();
        let mut call = |name: &str, args: Value| -> Reply {
            calls.push((name.to_owned(), args));
            Ok(vec![super::super::text("done")])
        };
        let (status, body) = answer(
            br#"{"jsonrpc":"2.0","id":7,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#,
            &mut call,
        );
        assert_eq!(status, 200);
        let body = body.unwrap();
        assert_eq!(body["id"], 7);
        assert_eq!(body["result"]["protocolVersion"], "2025-06-18");
        let (status, body) = answer(
            br#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            &mut call,
        );
        assert_eq!((status, body), (202, None));
        // A response from the client needs no answer either.
        let (status, body) = answer(br#"{"jsonrpc":"2.0","id":3,"result":{}}"#, &mut call);
        assert_eq!((status, body), (202, None));
        let (status, body) = answer(
            br#"{"jsonrpc":"2.0","id":"a","method":"tools/call","params":{"name":"get_scene"}}"#,
            &mut call,
        );
        assert_eq!(status, 200);
        assert_eq!(body.unwrap()["result"]["content"][0]["text"], "done");
        let (status, body) = answer(b"{not json", &mut call);
        assert_eq!(status, 400);
        assert_eq!(body.unwrap()["error"]["code"], -32700);
        // Batches and other JSON that is not a request object are invalid requests.
        for bad in [
            &b"[]"[..],
            br#"[{"jsonrpc":"2.0","id":1,"method":"ping"}]"#,
            b"5",
            b"null",
            br#""x""#,
        ] {
            let (status, body) = answer(bad, &mut call);
            assert_eq!(status, 400);
            let body = body.unwrap();
            assert_eq!(body["error"]["code"], -32600);
            assert_eq!(body["id"], Value::Null);
        }
        assert_eq!(calls, vec![("get_scene".to_owned(), json!({}))]);
    }

    #[test]
    fn writes_responses() {
        let mut out = Vec::new();
        write_response(&mut out, 200, Some(&json!({"a": 1}))).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 7\r\nConnection: close\r\n\r\n{\"a\":1}"
        );
        let mut out = Vec::new();
        write_response(&mut out, 405, None).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "HTTP/1.1 405 Method Not Allowed\r\nAllow: POST\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
    }

    #[test]
    fn keeps_the_token() {
        let dir = std::env::temp_dir().join(format!("bc3d-token-{}", uuid::Uuid::new_v4()));
        let file = dir.join("mcp-token");
        let first = load_or_create(&file).unwrap();
        assert!(uuid::Uuid::parse_str(&first).is_ok());
        assert_eq!(load_or_create(&file).unwrap(), first);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&file).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        std::fs::write(&file, "garbage").unwrap();
        let second = load_or_create(&file).unwrap();
        assert_ne!(second, first);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), second);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn serves_over_a_socket() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let stop = super::super::accept(listener, move |s| {
            exchange(s, port, TOKEN, |_, _| Ok(vec![super::super::text("hi")]))
        })
        .unwrap();
        let send = |request: String| {
            let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            s.write_all(request.as_bytes()).unwrap();
            let mut text = String::new();
            s.read_to_string(&mut text).unwrap();
            text
        };
        let body =
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"get_scene"}}"#;
        let text = send(format!(
            "POST /mcp/{TOKEN} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        ));
        assert!(text.starts_with("HTTP/1.1 200 OK\r\n"), "{text}");
        let reply: Value = serde_json::from_str(text.split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(reply["result"]["content"][0]["text"], "hi");
        let text = send(format!(
            "GET /mcp/{TOKEN} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n"
        ));
        assert!(text.starts_with("HTTP/1.1 405 "), "{text}");
        stop.store(true, Ordering::Relaxed);
    }
}
