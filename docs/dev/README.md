# Development notes

Notes kept while building 3D BuildCraft, for anyone who wants to work on it.

- [Architecture](architecture.md): the modules, how a frame is drawn, the tools and their limits.
- Decisions:
  [ADR-0001 booleans with manifold-rust](decisions/ADR-0001-booleans-with-manifold-rust.md),
  [ADR-0002 GPT reviews, Claude writes code](decisions/ADR-0002-gpt-reviews-claude-writes-code.md),
  [ADR-0003 the UI copies the original](decisions/ADR-0003-ui-copies-the-original.md),
  [ADR-0004 an AI agent can drive the tools](decisions/ADR-0004-ai-agent-can-drive-the-tools.md).
- Progress log: [2026-10-27](log/2026-10-27.md), [2026-10-28](log/2026-10-28.md).
- The original Microsoft 3D Builder, as studied before the port (behaviour, file formats,
  UI measurements; no Microsoft code or assets): [overview](original/overview.md),
  [architecture](original/architecture.md), [data model](original/data-model.md),
  [findings](original/findings.md), [UI](original/ui.md), [Rust port spec](original/rust-port.md),
  [target platforms](original/ADR-0001-target-windows-linux-macos.md).

Original packages, extracted evidence and reference photos are kept outside the repository;
these notes retain descriptions and measurements only. `docs/screenshots/` contains captures
of 3D BuildCraft.

## Working on it

- Branches: `main` is stable, work happens on `development`; pull requests go to `development`.
- Before a commit: `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, `cargo test`.
- Code, comments and UI strings are in English; the Spanish translation is the table in
  `src/i18n.rs`, and a test fails when a UI string has no entry.
- Screenshots for comparing with the original: `--screenshot out.png --size 1920x1080 --zoom 1.5`
  with `--lang`, `--tab`, `--tool`, `--mode`, `--select-all` (see the README).
