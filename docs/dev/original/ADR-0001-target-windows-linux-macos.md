# ADR-0001 Target Windows, Linux and macOS

## Context

The original 3D Builder is a UWP app tied to Windows 10/11: XAML, Direct3D 11, the Windows 3D print workflow and WSD printing. The Rust reimplementation ([Rust port](rust-port.md)) must run on all three desktop systems.

## Decision

The Rust app must build, run and pass its tests on **Windows, Linux and macOS** from one codebase. Every feature in the spec is available on all three; nothing ships Windows-only.

## Alternatives considered

- Windows only, like the original: rejected; cross-platform support is a requirement.
- Linux and Windows first, macOS later: rejected; Metal and app signing problems are cheaper to find early.
- Web app (wasm + WebGPU): not chosen now. egui and wgpu keep that door open, but file access and Manifold in wasm add risk.

## Consequences

- Rendering through `wgpu` (DX12 on Windows, Vulkan on Linux, Metal on macOS); no direct Direct3D, OpenGL-only or Metal-only code.
- UI with a portable toolkit (`egui`/`eframe` or `slint`); native file dialogs through `rfd`; data and config paths through `directories`.
- Windows-only features of the original are dropped or replaced: WSD and the Windows 3D print dialog become OctoPrint/Moonraker HTTP or "export and open in slicer"; the UWP settings hive becomes a TOML file.
- Fonts for emboss cannot assume Segoe UI: list system fonts (`font-kit` or `fontdb`) and bundle one open font as default.
- Native dependencies (Manifold, meshoptimizer) must build with each platform's toolchain; CI checks this.
- CI runs build and tests on `windows-latest`, `ubuntu-latest` and `macos-latest` (Apple Silicon), and macOS Intel if still needed.
- Packaging per system: MSI or MSIX on Windows, signed and notarised `.app` in a `.dmg` on macOS, AppImage and Flatpak on Linux (`cargo-dist` or `cargo-bundle` to start).
- File associations for STL, 3MF, OBJ, PLY, glTF/GLB are registered by each installer.
