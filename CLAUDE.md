# 3D BuildCraft

Clean-room Rust reimplementation of Microsoft 3D Builder for Windows, Linux and macOS (egui + wgpu). The development notes and study of the original live in `docs/dev/`; start with `docs/dev/README.md`.

Check before finishing: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`.

The UI copies the original's look: colours and sizes live in `src/app/style.rs`, icons are drawn in `src/app/icons.rs` (our own drawings, no assets from the original). After UI work, take screenshots with `--screenshot` (see README) at `--size 1920x1080 --zoom 1.5`, compare them with reference photos of the original, kept outside the repository, and send them to the user.

## Project memory

- At the start of work, read `docs/dev/README.md` before reading code.
- After a change lands, update the relevant notes in `docs/dev/`.

## Reviews

- Every review is done by openai-reviewer, core changes included. Do not review the same change again with reviewer; use reviewer only if openai-reviewer reports GPT is unavailable.
- Every plan critique (product-planner's debate included) is done by openai-devils-advocate, core decisions included. Do not critique the same plan again with devils-advocate; use it only if openai-devils-advocate reports GPT is unavailable.
- Production code is written by the Claude agents. Unit, integration and database tests are written by openai-tester; tester writes end-to-end tests and takes over if openai-tester reports GPT is unavailable.
- Before reading more than a few files to understand an unfamiliar area, ask openai-researcher for a map with file:line, then open only what it points to.

## Language

- Code, identifiers, comments, commit messages, pull requests and docs are in English.
- Reply to the user in the language they write in, and tell subagents which language that is.
- The UI is in English and Spanish. UI text is written in English in `src/app.rs` as `tr(lang, "…")` and translated in the Spanish table in `src/i18n.rs`; a test fails if a text has no Spanish entry. Shape, unit, format and palette names (`label()` methods, `scene::PALETTE`) go through `tr` too. Error details from the library (`src/io/`, `src/boolean.rs`) are English only.
