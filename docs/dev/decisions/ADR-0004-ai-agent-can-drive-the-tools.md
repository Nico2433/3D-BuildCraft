# ADR-0004 An AI agent can drive the tools

## Context

An AI agent should be able to work with the app's tools: insert and edit shapes, repair, emboss, hollow, prepare a print, and so on, from plain-language requests. Built on 2026-10-28 (`0c15daa`).

## Decision

Expose the edits the UI already funnels through as a tool interface an agent can call, instead of having it click the UI:

- **One command layer.** Every edit already goes through `Action` and `App::apply` (`src/app/mod.rs`) or through library functions (`boolean`, `solid`, `emboss`, `cut`, `repair`, `primitives::custom`, `image3d`). A shared, serialisable command set (JSON) maps one-to-one onto those, so UI and agent use the same path, with the same undo steps.
- **Transport: MCP.** Serve the commands as Model Context Protocol tools (stdio for a local agent, optionally a local socket while the app runs). MCP works with Claude, ChatGPT-style clients and local models, so the app does not tie itself to one provider.
- **Tools to start with:** list/select objects, insert primitive or custom shape, move/rotate/scale, booleans, split, smooth, hollow, extrude down, emboss (text, shape, image), repair, paint (object or area), open/save/export, scene stats (size, volume, validity) and a screenshot (`--screenshot` already renders the window) so the agent can see the result.
- **Safety:** the agent works on the open scene only; file writes go through the same save paths and ask before overwriting; every agent change is an undo step and is shown in the UI as it happens.
- **Headless mode** later: the same commands from the command line, for batch jobs.

## As built

- `buildcraft3d --mcp` opens the window and serves MCP (JSON-RPC 2.0, one message per line) on stdio: `src/mcp.rs` speaks the protocol and lists the tools; `src/app/agent.rs` runs each call on the UI thread through `App::apply`, so it is an undo step and shows on screen, with one "AI assistant: <tool>" notice.
- Tools: get_scene, select, insert_shape, transform, edit (delete, duplicate, merge, subtract, intersect, repair, smooth, extrude_down, settle, mirror, undo, redo), set_color, hollow, simplify, split, emboss_text, open, save, print, screenshot (PNG of the window).
- Errors the actions show as notices go back to the agent as tool errors. open refuses unsaved changes unless `discard_changes`; save refuses an existing file unless `overwrite`.
- No new dependency (serde_json; base64 written by hand). Since `82b6002`: a local port (127.0.0.1 + token file readable by the user only) lets `--mcp` attach to the open window, and an in-app chat (`src/assistant.rs`, `src/app/assistant_ui.rs`) drives the same tools from an installed agent (Claude Code, Codex), a custom command or an OpenAI-/Anthropic-compatible endpoint; keys in the system keychain. Not yet: emboss of shapes/images, paint by area, Custom, Load image, headless mode.

## Alternatives considered

- An agent that clicks the UI through screenshots: works without code changes but is slow and brittle.
- A provider-specific plugin: quicker to wire, but locks the app to one vendor.

## Consequences

- New edits should keep going through `Action` / library functions, not ad-hoc UI code, so they stay reachable by the agent.
- Needs a stable, documented command schema and an MCP server crate (or a small JSON-RPC over stdio).

## Update 2026-10-28

The in-app chat no longer launches installed agents (Claude Code, Codex) or a custom command. The chat uses API endpoints the user configures; external agents connect over MCP: the user adds `buildcraft3d --mcp` to their own agent.
