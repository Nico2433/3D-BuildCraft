# Overview

The study concerns Microsoft 3D Builder 20.0.4.0, a native Windows 3D mesh editor for building, repairing and preparing models for printing. These notes describe behaviour, layout, measurements and format conventions to support an independent Rust implementation.

The initial pass inspected package metadata and native dependencies. Later passes examined exported models, recovery behaviour and reference photos of the running UI. No source or decompiled algorithms are included here. Original packages, extracted evidence and reference photos are kept outside the repository.

## Evidence conventions

- **Observed:** supported by package metadata, an export or a UI capture.
- **Inferred:** an interpretation of those observations, without a runtime trace.
- **Pending:** requires a controlled behavioural test.

A type name or label does not establish that a feature is reachable. A service reference does not prove a connection. Signatures and trust chains were not validated, and equivalence across processor architectures was not checked.

## Platform and dependencies

The original targets Windows Universal and was distributed for x86, x64, ARM and ARM64, with language and display-scale resources. The declared minimum Windows version is 10.0.15063.0; the tested-version declaration is not an installation ceiling.

Installation requires matching-architecture XAML 2.0 and VCLibs 140 frameworks. An x64 installation was validated with XAML 2.1810.18004.0 and VCLibs 14.0.33519.0. Kinect is not a declared dependency. See H03 and H23–H26 in [Findings](findings.md).

## Scope and limits

The useful evidence covers the UI, supported formats, settings, autosave, export conventions and a repair before/after comparison. It does not establish internal algorithm implementations, the full import compatibility matrix, network traffic or current service availability.

Continue with [Architecture](architecture.md), [Data model](data-model.md), [User interface](ui.md), [Findings](findings.md) and the [Rust port spec](rust-port.md).
