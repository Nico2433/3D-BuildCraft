# Architecture

This is a conceptual reconstruction from package metadata, native dependencies, exports and UI observations, not a verified call graph. Raw inventories, extracted resources and binary strings are excluded.

## Components

- Native C++ application with XAML, WinRT and likely C++/CX bindings.
- Direct3D 11 and DXGI for graphics; Direct2D and DirectWrite dependencies for drawing and text.
- A dedicated 3MF reader/writer component and XML support.
- Scene geometry, topological meshes, materials, command history and asynchronous jobs appear to be separate responsibilities. Full MVVM is not established.
- A local model catalog and a separate online-content component provide model discovery.
- Geometry and fabrication facilities appear to include netfabb integration; the version, licensing and exact responsibilities were not established.

## Editing surface

The UI exposes transforms, selection, measurement, booleans, plane cutting, simplification, smoothing, hollowing, embossing, downward extrusion, materials and repair. Image conversion includes contour, edge, height-map and stamp modes. Autosave and undo/redo support recovery and reversible editing. Scan-related evidence alone does not establish a working scan feature.

## Repair observations

**Observed repair result** (second pass, `models/camel.stl` → `camel Repair.stl`): 20 boundary, 20 non-manifold and 24 misoriented edges went to zero, with +38 triangles and +16 vertices, volume −0.0016 % and bbox moved ≤ 0.0016 mm. So repair closes small holes, resolves non-manifold edges and fixes orientation with minimal local change; it does not remesh or decimate, and pinched vertices remain (odd Euler characteristic). No web cache was left behind, consistent with local repair. In contrast, a joined multi-part scene was exported with degenerate faces and non-manifold edges, so edits do not auto-repair.

## Data flow

1. Windows activates the app directly or through file, protocol or print integration.
2. Files, primitives, catalog objects or images enter the scene.
3. Objects carry geometry, transforms and materials.
4. Editing tools change the scene through what appears to be a command/task layer.
5. Saving exports geometry; print preparation adds dimensions, material and fabrication settings.
6. Windows print integration can query capabilities, create a job, attach a 3MF document, and query or cancel it.

This sequence is inferred, not a captured execution trace.

## Printing and online services

Windows print integration and WSD/SOAP support are declared. Capability descriptions cover quality, density, supports and raft; other evidence suggests layer height, extrusion width, speeds, retraction, temperatures and wall counts. Defaults and validation are unknown.

Local slicing and Marlin G-code generation appear to be present. A packaged sample does not establish application defaults. Discovery, authentication, retry and timeout behaviour were not reconstructed.

Online-content integrations include catalogs, sign-in, hologram content and print ordering with material pricing. Embedded service references were not contacted during the static pass. UI captures later showed catalog categories with missing images and one failed print-service request; this does not establish current availability.

## Permissions and instrumentation

Declared permissions cover Internet, private networks, 3D objects and broad file access; actual grants were not observed. Event instrumentation covers repair and manipulation, but its presence does not prove telemetry upload.

## Limits

Runtime evidence is limited to UI captures, settings, exports, autosave and repair results. Internal algorithms and the local/cloud repair split remain unknown. See [Findings](findings.md) and [Data model](data-model.md).
