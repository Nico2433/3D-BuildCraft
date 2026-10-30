# Rust port

## Goal

A desktop app in Rust for **Windows, Linux and macOS** ([ADR-0001](ADR-0001-target-windows-linux-macos.md)) that does what 3D Builder does for a user (open, build, edit, repair and prepare 3D models for printing), written clean-room from the documented feature surface. Not a translation of Microsoft's code (there is none to translate) and not a copy of its assets, name or branding.

Verdict: **feasible.** The feature list is fully known from [Architecture](architecture.md); the internals are not, and do not need to be. The hard parts are robust mesh repair, booleans and slicing, which netfabb covered in the original.

## Acceptance criteria

MVP (phase 1):

1. Opens STL, OBJ, PLY, 3MF, glTF/GLB; saves 3MF and STL.
2. 3D viewport with orbit/pan/zoom, selection, move/rotate/scale gizmos and snapping.
3. Primitives: cube, cylinder, pyramid, cone, sphere, hexagon, wedge, torus, tetrahedron.
4. Undo/redo for every edit; autosave and recovery after a crash.
5. Union, subtract, intersect between two closed meshes.
6. Plane cut, measure distance, reduce faces, merge/split objects.
7. Builds, runs and passes all tests on Windows, Linux and macOS from one codebase, checked in CI on all three; installers for each (MSI/MSIX, signed `.dmg`, AppImage/Flatpak) with file associations.
8. 3MF output matches the original's shape (see [Data model](data-model.md)): mm units, `basematerials` with RGBA colours, per-triangle material, thumbnail; but welded vertices.
9. Repair on `models/camel.stl` gives 0 boundary, 0 non-manifold and 0 misoriented edges, with volume change under 0.01 % (the original reached −0.0016 %). Check boundary, non-manifold and misoriented edge counts with a mesh topology inspector.
10. OBJ, PLY and glTF/GLB export follow the conventions in [Data model](data-model.md): mm and Z-up for OBJ/PLY, metres and Y-up with linear colours for glTF.
11. Import asks for the file's units; a mesh that is not closed shows a warning that offers repair.
12. UI follows the tab layout in [User interface](ui.md): Insert, Object, Edit, Paint, View, Help, a selection pane and a bottom transform bar. Tools open a contextual bar with Cancel / Apply and their own undo.
13. Custom primitive: shape, latitude/longitude segments, width, height, roundness (defaults 10, 10, 40 mm, 40 mm, 0), with a wireframe preview.

Phase 2: repair (merge vertices, fix orientation, fill holes, remove fragments), smooth/subdivide, hollow (minimum wall thickness, default 1 mm), extrude down, emboss text/image (planar/cylindrical/spherical projection, bevel, top and side colour, font), image to heightmap/contour, colour/texture painting with coverage brush, PBR materials.

Phase 3: print prep (units, scale, width/height/depth, reset orientation/scale), supports, slicer with Marlin G-code, send to printer (OctoPrint/Moonraker) and a job status view.

## Design

Mapping from the original stack to Rust crates:

| Original | Rust |
|---|---|
| XAML UI | `egui` / `eframe` (fastest path); `slint` if a more native look matters; both run on Windows, Linux and macOS |
| Direct3D 11 | `wgpu` (DX12 on Windows, Vulkan on Linux, Metal on macOS) with a small custom renderer |
| UWP file pickers | `rfd` native dialogs |
| Segoe UI for emboss | system font list via `fontdb` + one bundled open font |
| `MeshTools::Mesh` / `TopologicalMesh` | own indexed mesh + half-edge struct; `glam` for math; `parry3d` for ray picking and BVH |
| Lib3mfUAP | `zip` + `quick-xml` (3MF core spec is small); `threemf` crate as a start |
| STL / OBJ / PLY / glTF | `stl_io`, `tobj`, `ply-rs`, `gltf` |
| WRL (VRML) | no good crate; write a minimal parser, last priority |
| netfabb booleans | Manifold via `manifold3d` bindings (robust, fast); pure Rust alternatives (`csgrs`, BSP) are not robust enough |
| Face reduction | `meshopt` (meshoptimizer simplifier) |
| Repair | own code: weld, orient, hole fill, component filter; Manifold rejects non-manifold input, so repair runs first |
| Hollow | SDF/voxel offset (`fast-surface-nets`) then boolean subtract |
| Smooth / subdivide | own Taubin smoothing and Loop subdivision on the half-edge mesh |
| Emboss text | `ttf-parser` + `lyon` tessellation + extrude + boolean |
| Image tools | `image` crate: heightmap grid, contour tracing |
| `CCommandManager` | command pattern with an undo stack of mesh snapshots or diffs |
| AutoSave | periodic 3MF write to the platform data dir (`directories` crate), like the original's `LocalState/AutoSaveFiles` |
| `LocalSettings` hive | a TOML/JSON settings file: render flags, recent items (24-slot list with thumbnails), last session path |
| netfabb slicer / Marlin export | slice by plane → polygons; `clipper2` (or `geo`) for offsets; zigzag infill; G-code writer. Or export 3MF and call the PrusaSlicer/OrcaSlicer CLI |
| WSD / Windows print workflow | drop; Windows-only and tied to UWP. Replace with OctoPrint/Moonraker HTTP |
| Remix3D / Hubble / online catalog | drop; Remix3D closed in 2020. Ship own primitives and sample models |
| Kinect scan | drop |

Platform work (see [ADR-0001](ADR-0001-target-windows-linux-macos.md)): CI matrix on Windows, Ubuntu and macOS; Manifold and meshoptimizer built with each toolchain (CMake + MSVC/Clang/GCC); packaging with `cargo-dist`, macOS signing and notarisation, Linux AppImage/Flatpak.

Rough effort for one developer: MVP 2–3 months, plus about 2 weeks for CI and packaging on three systems; phase 2 another 3–4; a slicer of decent quality is months on its own, so the CLI route is the cheap option.

## Open questions

- Scope: MVP editor only, or full print prep with own slicer?
- UI toolkit: decided, egui + wgpu (2026-10-27).
- Do booleans need an automatic clean-up pass? The original leaves degenerate faces after joining parts (H34); Manifold output is already manifold.
- Legal: do not reuse Microsoft's icons, models, strings or the "3D Builder" name.

## Changes

- 2026-10-27: implementation started in 3D BuildCraft.
- 2026-10-27 (end of day, latest commit `61967aa`; see [log](../log/2026-10-27.md)):
  - Criteria 1–6 covered in large part, including booleans ([ADR-0001](../decisions/ADR-0001-booleans-with-manifold-rust.md), manifold-rust instead of `manifold3d`), plane cut (horizontal only) and simplify (`meshopt`). Still open: rotatable cut plane, snapping/measure not re-checked, per-object colour on import.
  - Criterion 7: code done, but CI has never run (no remote yet).
  - Criteria 8, 10, 11, 12: covered as before. Criterion 9 met: camel repair leaves no open, non-manifold or misoriented edges, volume change 1e-5 % (original 1.6e-3 %).
  - Criterion 13: primitive segment clamps added; not re-verified in full.
  - Phase 2 hollow, smooth, relief, extrude down, textures and phase 3 print prep are not started.
- 2026-10-28 (`831e99c`, see [log](../log/2026-10-28.md)): CI green on Windows, Ubuntu and macOS (criterion 7 runs). Phase 2 Smooth, Hollow and Extrude down done in `src/solid.rs`; limits: Smooth capped at 250k triangles, Hollow walls thicker than asked on big models, Extrude down fails on self-overlapping patches. Relief, textures and phase 3 print prep not started.
- 2026-10-28 (`7502949`, see [log](../log/2026-10-28.md)): Phase 2 relief started: Emboss with text, planar projection from above, raise or engrave (`src/emboss.rs`). Missing from the original's Relieve: image/shape patterns, cylindrical and spherical projection, bevel, top/side colours, font choice, on-canvas handles. Textures and phase 3 print prep not started.
- 2026-10-28 (`73cbc45`, see [log](../log/2026-10-28.md)): Phase 2 relief done: Emboss with text, shapes and images; planar, cylindrical and spherical projection; bevel; top/side colours; any installed font; drag to place. Limits: curved-projection axis fixed at the bbox centre, image detail 256 px, one colour per object part. Textures and phase 3 print prep not started.
- 2026-10-28 (`fca124a`, see [log](../log/2026-10-28.md)): UI restyled to look like the original ([ADR-0003](../decisions/ADR-0003-ui-copies-the-original.md)): layout, colours, sizes, tool bars, Color tool (whole objects), crease shading, shadow, reflection. Not copied yet: Agregar with the online library, Personalizado, Impresión 3D screens, green rotate arrow, face-coverage painting, 3D thumbnails. Textures and phase 3 print prep not started.
- 2026-10-28 (`653ade7`, see [log](../log/2026-10-28.md)): Add dialog, Custom tool (with roundness), 3D print view (design scaling, info, warnings; no printers or materials) and green turn arrow done. Still missing from the original: rendered thumbnails, face-coverage painting, Load image (image to 3D), camera, online library. Textures and phase 3 real printing not started.
- 2026-10-28 (`06f757f`, see [log](../log/2026-10-28.md)): Local folder libraries in Add, rendered thumbnails, Load image (relief or outline) and paint by area (per-triangle colours kept in 3MF) done. Still missing from the original: Camera, online library. Textures and phase 3 real printing not started. Agent tool interface proposed ([ADR-0004](../decisions/ADR-0004-ai-agent-can-drive-the-tools.md)).
