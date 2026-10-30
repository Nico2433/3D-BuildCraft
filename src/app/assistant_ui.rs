//! The AI assistant in the app: its settings page in the menu (on or off, which agent or
//! endpoint, the API key in the system keychain, how to attach an external agent over
//! MCP) and the chat panel at the right, whose requests run the app's tools.

use super::icons::Icon;
use super::style;
use super::*;
use crate::assistant::{self, Event};
use std::sync::mpsc::Receiver;

enum Who {
    User,
    Assistant,
    Tool,
    Error,
}

#[derive(Default)]
pub struct ChatState {
    pub open: bool,
    log: Vec<(Who, String)>,
    input: String,
    busy: bool,
    /// The running chat and the settings it started with; new settings start a new one.
    running: Option<(assistant::Chat, assistant::Config)>,
    /// The API key being typed; it is kept only in the keychain.
    key: String,
    test: Option<Receiver<Result<String, String>>>,
    tested: Option<Result<String, String>>,
    detected: Option<assistant::Detected>,
    detecting: Option<Receiver<assistant::Detected>>,
    connections: Vec<Connection>,
}

struct Connection {
    kind: assistant::AgentKind,
    pending: Option<Receiver<Result<String, String>>>,
    result: Option<Result<String, String>>,
    enabled: bool,
    target: bool,
}

impl App {
    /// Opens or closes the local MCP port as the settings ask: external agents attach to
    /// the open window through it.
    pub(super) fn sync_listener(&mut self, ctx: &egui::Context) {
        let ai = &self.settings.ai;
        let want = ai.enabled && ai.external;
        match (&self.listener, want) {
            (None, true) => {
                let call = agent::caller(self.agent_tx.clone(), ctx.clone());
                let started = crate::mcp::Listener::start(call).map_err(|e| format!("{e:#}"));
                if let Err(e) = &started {
                    log::warn!("MCP listener: {e}");
                }
                self.listener = Some(started);
            }
            (Some(_), false) => self.listener = None,
            _ => {}
        }
    }

    /// Takes what the chat did since the last frame.
    pub(super) fn chat_events(&mut self) {
        let Some((chat, _)) = &self.chat.running else {
            return;
        };
        for e in chat.events.try_iter() {
            match e {
                Event::Tool(name) => self.chat.log.push((Who::Tool, name)),
                Event::Reply(t) if !t.is_empty() => self.chat.log.push((Who::Assistant, t)),
                Event::Reply(_) => {}
                Event::Error(e) => self.chat.log.push((Who::Error, e)),
                Event::Done => self.chat.busy = false,
            }
        }
        if let Some(rx) = &self.chat.test {
            if let Ok(r) = rx.try_recv() {
                self.chat.tested = Some(r);
                self.chat.test = None;
            }
        }
    }

    fn chat_send(&mut self, ctx: &egui::Context) {
        let text = self.chat.input.trim().to_owned();
        if text.is_empty() || self.chat.busy {
            return;
        }
        let cfg = self.settings.ai.clone();
        if self.chat.running.as_ref().is_some_and(|(_, c)| *c != cfg) {
            self.chat.running = None;
        }
        let (chat, _) = self.chat.running.get_or_insert_with(|| {
            let key = assistant::api_key(&cfg.base_url);
            let call = agent::caller(self.agent_tx.clone(), ctx.clone());
            let wake = ctx.clone();
            let chat =
                assistant::Chat::start(cfg.clone(), key, call, move || wake.request_repaint());
            (chat, cfg)
        });
        if chat.send(text.clone()) {
            self.chat.log.push((Who::User, text));
            self.chat.input.clear();
            self.chat.busy = true;
        }
    }

    /// What the chat talks to, in a few words.
    fn chat_target(&self) -> String {
        let ai = &self.settings.ai;
        if ai.model.trim().is_empty() {
            tr(self.settings.lang(), "No model chosen").to_owned()
        } else {
            ai.model.trim().to_owned()
        }
    }

    /// The chat at the right of the window.
    pub(super) fn chat_panel(&mut self, ctx: &egui::Context) {
        if !self.chat.open || !self.settings.ai.enabled {
            return;
        }
        let lang = self.settings.lang();
        let mut send = false;
        egui::SidePanel::right("chat")
            .resizable(true)
            .default_width(360.0)
            .width_range(280.0..=640.0)
            .frame(
                egui::Frame::new()
                    .fill(style::FLYOUT)
                    .inner_margin(egui::Margin::same(12)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let (icon, _) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::hover());
                    super::icons::paint(ui.painter(), icon, Icon::Assistant, style::TEXT);
                    ui.label(
                        egui::RichText::new(tr(lang, "AI assistant"))
                            .size(17.0)
                            .color(style::TEXT),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if style::icon_button(ui, Icon::Close, Vec2::splat(28.0), true, style::RIBBON_HOVER)
                            .on_hover_text(tr(lang, "Close"))
                            .clicked()
                        {
                            self.chat.open = false;
                        }
                        if button(ui, tr(lang, "New chat"))
                            .on_hover_text(tr(lang, "Start over; the scene stays as it is."))
                            .clicked()
                        {
                            self.chat.running = None;
                            self.chat.log.clear();
                            self.chat.busy = false;
                        }
                    });
                });
                ui.label(egui::RichText::new(self.chat_target()).color(style::TEXT_DIM));
                ui.add_space(6.0);
                let input_h = 112.0;
                egui::ScrollArea::vertical()
                    .max_height((ui.available_height() - input_h).max(80.0))
                    .stick_to_bottom(true)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if self.chat.log.is_empty() {
                            for t in [
                                tr(lang, "Ask for changes to the scene, for example: “add a 30 mm cube and engrave my name on top”. The assistant uses the app's tools; each change shows as it is made and can be undone."),
                                tr(lang, "Nothing is sent anywhere except the agent or endpoint you chose."),
                            ] {
                                ui.label(egui::RichText::new(t).color(style::TEXT_DIM));
                                ui.add_space(6.0);
                            }
                        }
                        for (who, text) in &self.chat.log {
                            match who {
                                Who::User => {
                                    ui.with_layout(egui::Layout::top_down(egui::Align::Max), |ui| {
                                        egui::Frame::new()
                                            .fill(style::BLUE)
                                            .inner_margin(egui::Margin::symmetric(10, 6))
                                            .show(ui, |ui| {
                                                ui.set_max_width(ui.available_width() * 0.85);
                                                ui.label(egui::RichText::new(text).color(style::TEXT));
                                            });
                                    });
                                }
                                Who::Assistant => {
                                    ui.label(egui::RichText::new(text).color(style::TEXT));
                                }
                                Who::Tool => {
                                    ui.label(
                                        egui::RichText::new(format!("→ {text}"))
                                            .small()
                                            .color(style::TEXT_DIM),
                                    );
                                }
                                Who::Error => {
                                    ui.label(
                                        egui::RichText::new(text)
                                            .color(Color32::from_rgb(0xFF, 0x7A, 0x7A)),
                                    );
                                }
                            }
                            ui.add_space(6.0);
                        }
                        if self.chat.busy {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(egui::RichText::new(tr(lang, "Working…")).color(style::TEXT_DIM));
                            });
                        }
                    });
                ui.add_space(6.0);
                let edit = ui.add(
                    egui::TextEdit::multiline(&mut self.chat.input)
                        .desired_rows(3)
                        .desired_width(f32::INFINITY)
                        .hint_text(tr(lang, "Ask the assistant to change the scene…")),
                );
                // Enter sends; Shift+Enter makes a new line.
                if edit.has_focus()
                    && ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift)
                {
                    self.chat.input = self.chat.input.trim_end_matches('\n').to_owned();
                    send = true;
                }
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let ready = !self.chat.busy && !self.chat.input.trim().is_empty();
                        if ui.add_enabled(ready, framed(tr(lang, "Send"))).clicked() {
                            send = true;
                        }
                    });
                });
            });
        if send {
            self.chat_send(ctx);
        }
    }

    fn start_detection(&mut self, ctx: &egui::Context) {
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(assistant::detect());
            ctx.request_repaint();
        });
        self.chat.detecting = Some(rx);
    }

    /// The assistant's page in the menu.
    pub(super) fn ai_panel(&mut self, ui: &mut egui::Ui) {
        let lang = self.settings.lang();
        let before = self.settings.ai.clone();
        if let Some(rx) = &self.chat.detecting {
            if let Ok(found) = rx.try_recv() {
                self.chat.connections.clear();
                self.chat.detected = Some(found);
                self.chat.detecting = None;
            }
        }
        for connection in &mut self.chat.connections {
            if let Some(rx) = &connection.pending {
                if let Ok(result) = rx.try_recv() {
                    if result.is_ok() {
                        connection.enabled = connection.target;
                    }
                    connection.result = Some(result);
                    connection.pending = None;
                }
            }
        }
        if self.chat.detected.is_none() && self.chat.detecting.is_none() {
            self.start_detection(ui.ctx());
        }

        let text = |ui: &mut egui::Ui, t: &str| {
            ui.label(
                egui::RichText::new(t)
                    .size(style::MENU_FONT)
                    .color(style::TEXT),
            );
        };
        let dim = |ui: &mut egui::Ui, t: &str| {
            ui.label(egui::RichText::new(t).color(style::TEXT_DIM));
        };
        let heading = |ui: &mut egui::Ui, t: &str| {
            ui.add_space(14.0);
            ui.label(egui::RichText::new(t).size(17.0).color(style::TEXT));
            ui.add_space(4.0);
        };
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            heading(ui, tr(lang, "AI assistant"));
            dim(ui, tr(lang, "Use model endpoints for the in-app chat, or connect detected agents through MCP to work on the scene. Each change shows on screen and can be undone."));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                style::switch(ui, &mut self.settings.ai.enabled, tr(lang, "On"), tr(lang, "Off"));
            });
            heading(ui, tr(lang, "Detected on this computer"));
            if ui.add_enabled(self.chat.detecting.is_none(), framed(tr(lang, "Refresh"))).clicked() {
                self.start_detection(ui.ctx());
            }
            if self.chat.detecting.is_some() { ui.spinner(); }
            if let Some(found) = &self.chat.detected {
                for server in &found.servers {
                    ui.horizontal(|ui| {
                        text(ui, server.name);
                        dim(ui, &format!("{}, {} {}", tr(lang, "running"), server.models.len(), tr(lang, "models")));
                        if button(ui, tr(lang, "Use")).clicked() {
                            let ai = &mut self.settings.ai;
                            ai.enabled = true;
                            ai.base_url = server.base_url.into();
                            ai.protocol = assistant::Protocol::OpenAi;
                            if !server.models.contains(&ai.model) {
                                ai.model = server.models.first().cloned().unwrap_or_default();
                            }
                        }
                    });
                }

            }
            let ai = &mut self.settings.ai;
            ui.add_enabled_ui(ai.enabled, |ui| {
                heading(ui, tr(lang, "Model"));
                {
                    {
                        egui::Grid::new("ai_api").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                            text(ui, tr(lang, "Preset"));
                            // The preset is known by its URL, and gives the protocol.
                            let preset = assistant::PRESETS.iter().find(|p| p.base_url == ai.base_url.trim());
                            if let Some(p) = preset {
                                ai.protocol = p.protocol;
                            }
                            let current = preset.map_or(tr(lang, "Custom"), |p| p.name);
                            egui::ComboBox::from_id_salt("ai_preset").selected_text(current).show_ui(ui, |ui| {
                                for p in assistant::PRESETS {
                                    if ui.selectable_label(current == p.name, p.name).clicked() {
                                        ai.protocol = p.protocol;
                                        ai.base_url = p.base_url.into();
                                        ai.model = p.model.into();
                                    }
                                }
                            });
                            ui.end_row();
                            text(ui, tr(lang, "Base URL"));
                            ui.add(egui::TextEdit::singleline(&mut ai.base_url).desired_width(240.0));
                            ui.end_row();
                            text(ui, tr(lang, "Model"));
                            // A detected server offers its models; any other endpoint takes a typed name.
                            let detected = self.chat.detected.as_ref().and_then(|d| {
                                d.servers.iter().find(|s| s.base_url == ai.base_url.trim().trim_end_matches('/'))
                            });
                            match detected {
                                Some(server) => {
                                    egui::ComboBox::from_id_salt("detected_models")
                                        .width(228.0)
                                        .selected_text(ai.model.clone())
                                        .show_ui(ui, |ui| {
                                            for model in &server.models {
                                                ui.selectable_value(&mut ai.model, model.clone(), model);
                                            }
                                        });
                                }
                                None => {
                                    ui.add(egui::TextEdit::singleline(&mut ai.model).desired_width(240.0));
                                }
                            }
                            ui.end_row();
                            text(ui, tr(lang, "Request timeout"));
                            ui.add(egui::DragValue::new(&mut ai.timeout).range(10..=3600).suffix(" s"));
                            ui.end_row();
                            text(ui, tr(lang, "API key"));
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.chat.key)
                                        .password(true)
                                        .desired_width(160.0)
                                        .hint_text(tr(lang, "optional")),
                                );
                                if button(ui, tr(lang, "Save key")).clicked() {
                                    let saved = assistant::set_api_key(&ai.base_url, self.chat.key.trim());
                                    self.chat.key.clear();
                                    self.chat.tested = Some(
                                        saved
                                            .map(|()| tr(lang, "Key saved in the system keychain.").to_owned())
                                            .map_err(|e| format!("{e:#}")),
                                    );
                                }
                            });
                            ui.end_row();
                        });
                        dim(ui, tr(lang, "The key is stored in the system keychain, never in the app's settings. Leave it empty for a local server; saving an empty key removes it."));
                    }
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.add_enabled(self.chat.test.is_none(), framed(tr(lang, "Test"))).clicked() {
                        let (tx, rx) = std::sync::mpsc::channel();
                        let cfg = ai.clone();
                        let ctx = ui.ctx().clone();
                        std::thread::spawn(move || {
                            let key = assistant::api_key(&cfg.base_url);
                            let _ = tx.send(assistant::test(&cfg, key.as_deref()).map_err(|e| format!("{e:#}")));
                            ctx.request_repaint();
                        });
                        self.chat.test = Some(rx);
                        self.chat.tested = None;
                    }
                    if self.chat.test.is_some() {
                        ui.spinner();
                    }
                });
                match &self.chat.tested {
                    Some(Ok(t)) => {
                        ui.label(egui::RichText::new(t).color(Color32::from_rgb(0x7C, 0xE0, 0x7C)));
                    }
                    Some(Err(e)) => {
                        ui.label(egui::RichText::new(e).color(Color32::from_rgb(0xFF, 0x7A, 0x7A)));
                    }
                    None => {}
                }
                if button(ui, tr(lang, "Open the chat")).clicked() {
                    self.chat.open = true;
                    self.menu_open = false;
                    self.ai_open = false;
                }

                heading(ui, tr(lang, "External agents (MCP)"));
                ui.horizontal(|ui| {
                    style::switch(ui, &mut ai.external, tr(lang, "On"), tr(lang, "Off"));
                });
                ui.add_enabled_ui(ai.external, |ui| {
                    if let Some(found) = &self.chat.detected {
                        for agent in &found.agents {
                            let kind = agent.kind;
                            let index = self.chat.connections.iter().position(|c| c.kind == kind).unwrap_or_else(|| {
                                let enabled = agent.registered.as_ref().copied().unwrap_or(false);
                                self.chat.connections.push(Connection { kind, pending: None, result: None, enabled, target: enabled });
                                self.chat.connections.len() - 1
                            });
                            let connection = &mut self.chat.connections[index];
                            ui.push_id(kind.names().1, |ui| {
                                ui.horizontal(|ui| {
                                    text(ui, kind.names().0);
                                    let mut enabled = connection.enabled;
                                    ui.add_enabled_ui(connection.pending.is_none(), |ui| {
                                        style::switch(ui, &mut enabled, tr(lang, "On"), tr(lang, "Off"));
                                    });
                                    if enabled != connection.enabled {
                                        let (tx, rx) = std::sync::mpsc::channel();
                                        let agent = agent.clone();
                                        let ctx = ui.ctx().clone();
                                        std::thread::spawn(move || {
                                            let result = if enabled { assistant::connect(&agent, &assistant::mcp_command()) } else { assistant::disconnect(&agent) };
                                            let _ = tx.send(result.map_err(|e| format!("{e:#}")));
                                            ctx.request_repaint();
                                        });
                                        connection.target = enabled;
                                        connection.pending = Some(rx);
                                        connection.result = None;
                                    }
                                    if connection.pending.is_some() { ui.spinner(); }
                                });
                                if kind == assistant::AgentKind::Antigravity {
                                    dim(ui, tr(lang, "Add this MCP server in Antigravity's settings"));
                                }
                                if matches!(kind, assistant::AgentKind::OpenCode | assistant::AgentKind::Antigravity) {
                                    dim(ui, tr(lang, "Manage removal in the agent's settings."));
                                }
                                if let Err(error) = &agent.registered { dim(ui, error); }
                                if let Some(Err(error)) = &connection.result { ui.colored_label(Color32::LIGHT_RED, error); }
                                ui.collapsing(tr(lang, "Show command"), |ui| {
                                    let cmd = assistant::mcp_command();
                                    let args = kind.connect_args(&cmd).map(|args| std::iter::once(kind.names().1.to_owned()).chain(args).collect::<Vec<_>>()).unwrap_or(cmd);
                                    let line = args.iter().map(|arg| format!("{arg:?}")).collect::<Vec<_>>().join(" ");
                                    code(ui, &line, lang);
                                });
                            });
                        }
                    }
                });
                dim(ui, tr(lang, "With this switch on and the app open, the agent works in this window; otherwise the command opens a new one."));
                if let Some(Err(e)) = &self.listener {
                    ui.label(egui::RichText::new(e).color(Color32::from_rgb(0xFF, 0x7A, 0x7A)));
                }
            });
        });
        if self.settings.ai != before {
            let _ = self.settings.save();
        }
    }
}

/// A command in a box, with a button that copies it.
fn code(ui: &mut egui::Ui, line: &str, lang: Lang) {
    ui.horizontal(|ui| {
        egui::Frame::new()
            .fill(Color32::from_gray(0x24))
            .inner_margin(egui::Margin::symmetric(8, 4))
            .show(ui, |ui| {
                ui.set_max_width(ui.available_width() - 70.0);
                ui.add(
                    egui::Label::new(egui::RichText::new(line).monospace().color(style::TEXT))
                        .wrap(),
                );
            });
        if button(ui, tr(lang, "Copy")).clicked() {
            ui.ctx().copy_text(line.to_owned());
        }
    });
}

/// A button with a visible face; the app's dark theme leaves plain ones without one.
fn framed(text: &str) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(text.to_owned()).color(style::TEXT))
        .fill(Color32::from_gray(0x55))
        .min_size(Vec2::new(0.0, 26.0))
}

fn button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(framed(text))
}
