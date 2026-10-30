//! The in-app AI assistant: a chat with a model endpoint (OpenAI- or Anthropic-compatible,
//! picked by the preset). It works on the scene through the same tools an external agent
//! gets over MCP (`crate::mcp`); agents such as Claude Code or Codex attach that way.

use crate::mcp::{self, Reply};
use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

/// Tool rounds in one answer before the assistant is stopped.
const MAX_STEPS: usize = 24;
/// The name the app's MCP server goes by in agents' configurations.
pub const SERVER: &str = "buildcraft3d";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Protocol {
    #[default]
    OpenAi,
    Anthropic,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub enabled: bool,
    pub protocol: Protocol,
    pub base_url: String,
    pub model: String,
    /// Seconds one request (or one agent run) may take.
    pub timeout: u32,
    /// Lets agents outside the app attach to it over MCP.
    pub external: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: false,
            protocol: Protocol::OpenAi,
            base_url: PRESETS[0].base_url.into(),
            model: PRESETS[0].model.into(),
            timeout: 300,
            external: false,
        }
    }
}

pub struct Preset {
    pub name: &'static str,
    pub protocol: Protocol,
    pub base_url: &'static str,
    /// A model to start from; empty when the user must name one.
    pub model: &'static str,
}

pub const PRESETS: &[Preset] = &[
    Preset {
        name: "Ollama",
        protocol: Protocol::OpenAi,
        base_url: "http://localhost:11434/v1",
        model: "qwen3:14b",
    },
    Preset {
        name: "LM Studio",
        protocol: Protocol::OpenAi,
        base_url: "http://localhost:1234/v1",
        model: "",
    },
    Preset {
        name: "OpenAI",
        protocol: Protocol::OpenAi,
        base_url: "https://api.openai.com/v1",
        model: "",
    },
    Preset {
        name: "OpenRouter",
        protocol: Protocol::OpenAi,
        base_url: "https://openrouter.ai/api/v1",
        model: "",
    },
    Preset {
        name: "OpenCode Zen",
        protocol: Protocol::OpenAi,
        base_url: "https://opencode.ai/zen/v1",
        model: "",
    },
    Preset {
        name: "Anthropic",
        protocol: Protocol::Anthropic,
        base_url: "https://api.anthropic.com/v1",
        model: "claude-sonnet-5-5",
    },
];

/// A running local endpoint and its available models.
#[derive(Clone, Debug)]
pub struct LocalServer {
    pub name: &'static str,
    pub base_url: &'static str,
    pub models: Vec<String>,
}

/// An installed agent's MCP registration syntax.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentKind {
    Claude,
    Codex,
    OpenCode,
    Gemini,
    Antigravity,
}

impl AgentKind {
    /// Display name and executable name on PATH.
    pub fn names(self) -> (&'static str, &'static str) {
        match self {
            Self::Claude => ("Claude Code", "claude"),
            Self::Codex => ("Codex", "codex"),
            Self::OpenCode => ("OpenCode", "opencode"),
            Self::Gemini => ("Gemini CLI", "gemini"),
            Self::Antigravity => ("Antigravity", "antigravity"),
        }
    }

    /// Registration arguments, without a shell; Antigravity uses settings instead.
    pub fn connect_args(self, mcp: &[String]) -> Option<Vec<String>> {
        let prefix = match self {
            Self::Claude => vec!["mcp", "add", "-s", "user", SERVER, "--"],
            Self::Codex => vec!["mcp", "add", SERVER, "--"],
            Self::OpenCode => vec!["mcp", "add", "--global", SERVER, "--"],
            Self::Gemini => vec!["mcp", "add", "-s", "user", SERVER],
            Self::Antigravity => return None,
        };
        Some(
            prefix
                .into_iter()
                .map(str::to_owned)
                .chain(mcp.iter().cloned())
                .collect(),
        )
    }
}

/// An agent found on PATH, retaining the path used for registration.
#[derive(Clone, Debug)]
pub struct InstalledAgent {
    pub kind: AgentKind,
    pub path: std::path::PathBuf,
    pub registered: Result<bool, String>,
}

/// The providers available on this computer.
#[derive(Default)]
pub struct Detected {
    pub servers: Vec<LocalServer>,
    pub agents: Vec<InstalledAgent>,
}

/// Whether agent programs must run outside the sandbox.
pub fn in_flatpak() -> bool {
    std::path::Path::new("/.flatpak-info").exists() || std::env::var_os("FLATPAK_ID").is_some()
}

/// Builds every agent command in the same execution environment.
fn host_command(program: impl AsRef<std::ffi::OsStr>) -> std::process::Command {
    if in_flatpak() {
        let mut command = std::process::Command::new("flatpak-spawn");
        command.args(["--host"]).arg(program);
        command
    } else {
        std::process::Command::new(program)
    }
}

/// The command registered with host agents.
pub fn mcp_command() -> Vec<String> {
    if in_flatpak() {
        ["flatpak", "run", "io.github.nico2433.BuildCraft3D", "--mcp"]
            .map(str::to_owned)
            .to_vec()
    } else {
        vec![
            std::env::current_exe()
                .map_or_else(|_| "buildcraft3d".into(), |p| p.display().to_string()),
            "--mcp".into(),
        ]
    }
}

/// Finds executable programs on PATH, including Windows command wrappers.
pub fn find_program(program: &str) -> Option<std::path::PathBuf> {
    if in_flatpak() {
        let output = host_command("sh")
            .args(["-c", "command -v -- \"$1\"", "sh", program])
            .output()
            .ok()?;
        return output
            .status
            .success()
            .then(|| std::path::PathBuf::from(String::from_utf8_lossy(&output.stdout).trim()));
    }
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let suffixes = if cfg!(windows) {
            &["", ".exe", ".cmd", ".bat"][..]
        } else {
            &[""][..]
        };
        for suffix in suffixes {
            let candidate = dir.join(format!("{program}{suffix}"));
            if !candidate.is_file() {
                continue;
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if candidate.metadata().ok()?.permissions().mode() & 0o111 == 0 {
                    continue;
                }
            }
            return Some(candidate);
        }
    }
    None
}

/// Parses an Ollama tags response or an OpenAI model list.
pub fn parse_models(body: &str, ollama: bool) -> Result<Vec<String>> {
    let value: Value = serde_json::from_str(body)?;
    let (list, key) = if ollama {
        ("models", "name")
    } else {
        ("data", "id")
    };
    let rows = value[list]
        .as_array()
        .ok_or_else(|| anyhow!("missing model list"))?;
    let mut models = Vec::new();
    for name in rows
        .iter()
        .filter_map(|row| row[key].as_str())
        .filter(|s| !s.trim().is_empty())
    {
        if !models.iter().any(|m| m == name) {
            models.push(name.to_owned());
        }
    }
    Ok(models)
}

/// Probes local servers with one-second timeouts and finds installed agents.
pub fn detect() -> Detected {
    let mut found = Detected::default();
    let http: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(1)))
        .build()
        .into();
    for (name, base_url, url, ollama) in [
        (
            "Ollama",
            "http://localhost:11434/v1",
            "http://localhost:11434/api/tags",
            true,
        ),
        (
            "LM Studio",
            "http://localhost:1234/v1",
            "http://localhost:1234/v1/models",
            false,
        ),
    ] {
        let models = (|| -> Result<Vec<String>> {
            let body = http.get(url).call()?.body_mut().read_to_string()?;
            parse_models(&body, ollama)
        })();
        if let Ok(models) = models {
            found.servers.push(LocalServer {
                name,
                base_url,
                models,
            });
        }
    }
    for kind in [
        AgentKind::Claude,
        AgentKind::Codex,
        AgentKind::OpenCode,
        AgentKind::Gemini,
        AgentKind::Antigravity,
    ] {
        if let Some(path) = find_program(kind.names().1) {
            let registered = run_agent(&path, &["mcp".into(), "list".into()])
                .map(|output| output.contains(SERVER))
                .map_err(|e| format!("{e:#}"));
            found.agents.push(InstalledAgent {
                kind,
                path,
                registered,
            });
        }
    }
    found
}

/// Registers the MCP server, capturing output and stopping after twenty seconds.
pub fn connect(agent: &InstalledAgent, mcp: &[String]) -> Result<String> {
    let args = agent
        .kind
        .connect_args(mcp)
        .ok_or_else(|| anyhow!("Use Antigravity's settings"))?;
    run_agent(&agent.path, &args)
}

/// Removes registrations for CLIs with a known removal command.
pub fn disconnect(agent: &InstalledAgent) -> Result<String> {
    if matches!(agent.kind, AgentKind::OpenCode | AgentKind::Antigravity) {
        bail!("Manage this server in the agent's settings");
    }
    let mut args = vec!["mcp".into(), "remove".into()];
    if matches!(agent.kind, AgentKind::Claude | AgentKind::Gemini) {
        args.extend(["-s".into(), "user".into()]);
    }
    args.push(SERVER.into());
    run_agent(&agent.path, &args)
}

fn run_agent(path: &std::path::Path, args: &[String]) -> Result<String> {
    use std::io::Read;
    use std::process::Stdio;
    let mut child = host_command(path)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    // Drain both pipes while the process runs so a full pipe cannot block it.
    fn read(mut pipe: impl Read + Send + 'static) -> std::thread::JoinHandle<String> {
        std::thread::spawn(move || {
            let mut s = String::new();
            let _ = pipe.read_to_string(&mut s);
            s
        })
    }
    let stdout = read(child.stdout.take().unwrap());
    let stderr = read(child.stderr.take().unwrap());
    let start = std::time::Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if start.elapsed() >= Duration::from_secs(20) {
            let _ = child.kill();
            let _ = child.wait();
            bail!("Registration timed out after 20 seconds");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    // Descendants may inherit the pipes; do not wait for them beyond the deadline.
    while !stdout.is_finished() || !stderr.is_finished() {
        if start.elapsed() >= Duration::from_secs(20) {
            bail!("Registration output timed out after 20 seconds");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let out = stdout.join().unwrap_or_default();
    let err = stderr.join().unwrap_or_default();
    if !status.success() {
        bail!("{status}: {err}\n{out}");
    }
    Ok(format!("{out}{err}").trim().to_owned())
}

fn keychain(base_url: &str) -> Result<keyring::Entry> {
    Ok(keyring::Entry::new("3D BuildCraft", base_url.trim())?)
}

/// The API key saved for an endpoint, if any.
pub fn api_key(base_url: &str) -> Option<String> {
    keychain(base_url)
        .ok()?
        .get_password()
        .ok()
        .filter(|k| !k.is_empty())
}

/// Saves the key for an endpoint in the system keychain; an empty key removes it.
pub fn set_api_key(base_url: &str, key: &str) -> Result<()> {
    let entry = keychain(base_url)?;
    if key.is_empty() {
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.into()),
        }
    } else {
        Ok(entry.set_password(key)?)
    }
}

pub const SYSTEM: &str = concat!(
    "You are the assistant inside 3D BuildCraft, a 3D modelling app for 3D printing. You ",
    "change the user's open scene with the tools; each change shows on screen and can be ",
    "undone. Keep answers short and answer in the language the user writes in.\n",
    crate::rules!()
);

/// What the assistant does while it answers.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// It called a tool.
    Tool(String),
    /// It said something.
    Reply(String),
    Error(String),
    /// The answer is complete.
    Done,
}

/// An HTTP POST of JSON: url, headers, body -> the JSON answer (also for error statuses).
pub type Post<'a> = dyn Fn(&str, &[(&str, String)], &Value) -> Result<Value> + 'a;

/// One chat's history, in the endpoint's own format.
#[derive(Default)]
pub struct Conversation {
    messages: Vec<Value>,
}

impl Conversation {
    /// Sends `text` to a model endpoint and runs the tools it asks for until it answers.
    pub fn send_api(
        &mut self,
        cfg: &Config,
        key: Option<&str>,
        text: &str,
        post: &Post,
        call: &mut dyn FnMut(&str, Value) -> Reply,
        event: &mut dyn FnMut(Event),
    ) -> Result<()> {
        if cfg.model.trim().is_empty() {
            bail!("choose a model in the assistant's settings");
        }
        let base = cfg.base_url.trim().trim_end_matches('/');
        match cfg.protocol {
            Protocol::OpenAi => self.openai(base, cfg, key, text, post, call, event),
            Protocol::Anthropic => self.anthropic(base, cfg, key, text, post, call, event),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn openai(
        &mut self,
        base: &str,
        cfg: &Config,
        key: Option<&str>,
        text: &str,
        post: &Post,
        call: &mut dyn FnMut(&str, Value) -> Reply,
        event: &mut dyn FnMut(Event),
    ) -> Result<()> {
        // Tool results here are text only, so the screenshot tool is left out.
        let tools: Vec<Value> = mcp::tools()
            .into_iter()
            .filter(|t| t["name"] != "screenshot")
            .map(|t| {
                json!({ "type": "function", "function": {
                    "name": t["name"], "description": t["description"], "parameters": t["inputSchema"],
                } })
            })
            .collect();
        let headers: Vec<(&str, String)> = key
            .map(|k| vec![("Authorization", format!("Bearer {k}"))])
            .unwrap_or_default();
        self.messages
            .push(json!({ "role": "user", "content": text }));
        for _ in 0..MAX_STEPS {
            let mut messages = vec![json!({ "role": "system", "content": SYSTEM })];
            messages.extend(self.messages.iter().cloned());
            let body = json!({ "model": cfg.model.trim(), "messages": messages, "tools": tools });
            let answer = post(&format!("{base}/chat/completions"), &headers, &body)?;
            if let Some(e) = answer.get("error") {
                bail!("{}", e["message"].as_str().unwrap_or(&e.to_string()));
            }
            let msg = &answer["choices"][0]["message"];
            if msg.is_null() {
                bail!("unexpected answer: {answer}");
            }
            let content = msg["content"].as_str().unwrap_or("").trim().to_owned();
            let calls = msg["tool_calls"].as_array().cloned().unwrap_or_default();
            let mut kept = json!({ "role": "assistant", "content": content });
            if !calls.is_empty() {
                kept["tool_calls"] = Value::Array(calls.clone());
            }
            self.messages.push(kept);
            if !content.is_empty() {
                event(Event::Reply(content));
            }
            if calls.is_empty() {
                return Ok(());
            }
            for c in calls {
                let name = c["function"]["name"].as_str().unwrap_or("").to_owned();
                let args = match &c["function"]["arguments"] {
                    Value::String(s) => serde_json::from_str(s).unwrap_or(json!({})),
                    v if v.is_object() => v.clone(),
                    _ => json!({}),
                };
                event(Event::Tool(tool_line(&name, &args)));
                let content = reply_text(&call(&name, args));
                self.messages
                    .push(json!({ "role": "tool", "tool_call_id": c["id"], "content": content }));
            }
        }
        bail!("stopped after {MAX_STEPS} tool steps")
    }

    #[allow(clippy::too_many_arguments)]
    fn anthropic(
        &mut self,
        base: &str,
        cfg: &Config,
        key: Option<&str>,
        text: &str,
        post: &Post,
        call: &mut dyn FnMut(&str, Value) -> Reply,
        event: &mut dyn FnMut(Event),
    ) -> Result<()> {
        let tools: Vec<Value> = mcp::tools()
            .into_iter()
            .map(|t| json!({ "name": t["name"], "description": t["description"], "input_schema": t["inputSchema"] }))
            .collect();
        let mut headers = vec![("anthropic-version", "2023-06-01".to_owned())];
        if let Some(k) = key {
            headers.push(("x-api-key", k.to_owned()));
        }
        self.messages
            .push(json!({ "role": "user", "content": text }));
        for _ in 0..MAX_STEPS {
            let body = json!({
                "model": cfg.model.trim(), "max_tokens": 4096, "system": SYSTEM,
                "messages": self.messages, "tools": tools,
            });
            let answer = post(&format!("{base}/messages"), &headers, &body)?;
            if answer["type"] == "error" || answer.get("error").is_some_and(|e| e.is_object()) {
                let e = &answer["error"];
                bail!("{}", e["message"].as_str().unwrap_or(&e.to_string()));
            }
            let blocks = answer["content"]
                .as_array()
                .cloned()
                .ok_or_else(|| anyhow!("unexpected answer: {answer}"))?;
            self.messages
                .push(json!({ "role": "assistant", "content": blocks }));
            let said: Vec<&str> = blocks
                .iter()
                .filter(|b| b["type"] == "text")
                .filter_map(|b| b["text"].as_str())
                .collect();
            let said = said.join("\n").trim().to_owned();
            if !said.is_empty() {
                event(Event::Reply(said));
            }
            let uses: Vec<&Value> = blocks.iter().filter(|b| b["type"] == "tool_use").collect();
            if uses.is_empty() {
                return Ok(());
            }
            let mut results = Vec::new();
            for u in uses {
                let name = u["name"].as_str().unwrap_or("").to_owned();
                event(Event::Tool(tool_line(&name, &u["input"])));
                let out = call(&name, u["input"].clone());
                let content: Vec<Value> = match &out {
                    Ok(blocks) => blocks.iter().map(anthropic_block).collect(),
                    Err(e) => vec![json!({ "type": "text", "text": e })],
                };
                results.push(json!({
                    "type": "tool_result", "tool_use_id": u["id"],
                    "content": content, "is_error": out.is_err(),
                }));
            }
            self.messages
                .push(json!({ "role": "user", "content": results }));
        }
        bail!("stopped after {MAX_STEPS} tool steps")
    }
}

/// A tool call for the chat log: its name and, shortened, its arguments.
fn tool_line(name: &str, args: &Value) -> String {
    let args = args.to_string();
    if args == "{}" || args == "null" {
        return name.to_owned();
    }
    let short: String = args.chars().take(80).collect();
    let more = if args.chars().count() > 80 { "…" } else { "" };
    format!("{name} {short}{more}")
}

/// The text of a tool's reply, for endpoints that take text only.
fn reply_text(r: &Reply) -> String {
    match r {
        Ok(blocks) => blocks
            .iter()
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        Err(e) => format!("Error: {e}"),
    }
}

/// An MCP content block as an Anthropic one.
fn anthropic_block(b: &Value) -> Value {
    if b["type"] == "image" {
        json!({ "type": "image", "source": { "type": "base64", "media_type": b["mimeType"], "data": b["data"] } })
    } else {
        json!({ "type": "text", "text": b["text"] })
    }
}

/// POSTs JSON with a timeout; error statuses still return their JSON body.
pub fn http_post(timeout: Duration) -> impl Fn(&str, &[(&str, String)], &Value) -> Result<Value> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .http_status_as_error(false)
        .build()
        .into();
    move |url, headers, body| {
        let mut req = agent.post(url);
        for (k, v) in headers {
            req = req.header(*k, v);
        }
        let mut resp = req.send_json(body)?;
        let status = resp.status();
        let text = resp.body_mut().read_to_string()?;
        serde_json::from_str(&text).map_err(|_| {
            let text: String = text.chars().take(300).collect();
            anyhow!("{status}: {text}")
        })
    }
}

/// A chat running on its own thread.
pub struct Chat {
    send: Sender<String>,
    pub events: Receiver<Event>,
}

impl Chat {
    /// Starts a chat with `cfg`. Tool calls go to `call`; `wake` is called after each
    /// event so the UI can redraw.
    pub fn start(
        cfg: Config,
        key: Option<String>,
        mut call: impl FnMut(&str, Value) -> Reply + Send + 'static,
        wake: impl Fn() + Send + 'static,
    ) -> Self {
        let (send, inbox) = channel::<String>();
        let (tell, events) = channel();
        std::thread::spawn(move || {
            let mut chat = Conversation::default();
            let post = http_post(Duration::from_secs(cfg.timeout.max(10) as u64));
            for text in inbox {
                let mut event = |e: Event| {
                    let _ = tell.send(e);
                    wake();
                };
                let done = chat.send_api(&cfg, key.as_deref(), &text, &post, &mut call, &mut event);
                if let Err(e) = done {
                    event(Event::Error(format!("{e:#}")));
                }
                event(Event::Done);
            }
        });
        Self { send, events }
    }

    pub fn send(&self, text: String) -> bool {
        self.send.send(text).is_ok()
    }
}

/// Checks the settings with a short request; the answer or why it failed.
pub fn test(cfg: &Config, key: Option<&str>) -> Result<String> {
    let post = http_post(Duration::from_secs(cfg.timeout.clamp(10, 60) as u64));
    let mut chat = Conversation::default();
    let mut said = String::new();
    chat.send_api(
        cfg,
        key,
        "Reply with the single word OK.",
        &post,
        &mut |_, _| Err("no tools in a test".into()),
        &mut |e| {
            if let Event::Reply(t) = e {
                said = t;
            }
        },
    )?;
    Ok(said)
}
