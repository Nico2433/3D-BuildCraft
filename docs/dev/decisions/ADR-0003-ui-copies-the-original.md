# ADR-0003 The UI copies the original first

## Context

The first UI used stock egui widgets and did not look like Microsoft 3D Builder. The goal is familiarity for users of the original.

## Decision

Make the front end look like the original first, then extend it module by module; send screenshots after each task. Colours are sampled from the original's screenshots and sizes taken in points from its 150 % screenshots (`src/app/style.rs`). Icons are our own painter drawings (`src/app/icons.rs`); no Microsoft assets are copied. Font: Segoe UI when installed, else an open font found via `fontdb`. Done in `fca124a`, decided 2026-10-28.

## Alternatives considered

- Keep the stock egui look: less work, but unfamiliar to users of the original.
- Ship the original's icons and fonts: not ours to distribute.

## Consequences

- `--screenshot` options (`a6b58bc`) make side-by-side checks against the original repeatable; app captures in `docs/screenshots/`, compared with reference photos of the original kept outside the repository.
- Some features changed shape to match: tool bars replace the tabs, emboss values moved to the bottom bar, Paint became a whole-object Color tool.
- Parts not yet copied are listed in [Architecture](../architecture.md).
