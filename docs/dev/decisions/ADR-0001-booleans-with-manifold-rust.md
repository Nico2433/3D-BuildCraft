# ADR-0001 Booleans with manifold-rust

## Context

The app needs union, intersect and subtract on closed meshes (spec criterion 5 in [Rust port](../original/rust-port.md)). It must build on Windows, Linux and macOS from one codebase ([study ADR-0001](../original/ADR-0001-target-windows-linux-macos.md)). The spec first named `manifold3d` bindings.

## Decision

Use `manifold-rust` 0.19, a pure Rust port of Manifold, in `src/boolean.rs`. Restar subtracts newer selected objects from the oldest. Decided 2026-10-27.

## Alternatives considered

- `manifold3d` / `manifold-csg` C++ bindings: robust, but need cmake and a C++ toolchain on every platform and in CI.
- `csgrs` (BSP): pure Rust, but not robust on real meshes.

## Consequences

- No cmake or C++ build step for booleans; CI stays simple.
- The port is young, so results on large or dirty meshes need tests; inputs go through [repair](../architecture.md) first.
- Switching to the C++ bindings later only touches `src/boolean.rs`.
