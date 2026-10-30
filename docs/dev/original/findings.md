# Findings register

Finding IDs preserve the technical history: H01–H22 describe the static package study, H23–H36 installation and sample exports, H37–H48 autosave, formats and UI, and H49–H56 modelling tools and print preparation. Raw evidence and reference photos are kept outside the repository.

Static evidence supports dependencies and feature names, not a verified call graph or working algorithms. Export and UI observations are sample-specific.

## Findings

| ID | Conclusion | Status / source | Limit |
|---|---|---|---|
| H01 | Microsoft 3D Builder 20.0.4.0 | Observed | Declared identity, signature not validated |
| H02 | 4 architectures, 62 language and 6 scale packages | Observed | Cross-arch semantics not compared |
| H03 | Helper deps insufficient for x64 | Derived | Windows may have or fetch them |
| H04 | Native app with XAML and C++/CX | Native observed; C++/CX strong inference | No source recovered |
| H05 | D3D11/DXGI/D2D/DWrite graphics deps | Observed | Render pipeline pending |
| H06 | Context, commands, tasks, controls, views | Inferred | No call graph |
| H07 | Booleans, cut, hollow, emboss, simplify tools | Resources/types observed | Results and reachability untested |
| H08 | Topological meshes and physical materials | Types observed | Memory layouts not rebuilt |
| H09 | netfabb-named code and Marlin slicing | Observed | Version and responsibilities unknown |
| H10 | Seven registered extensions | Observed | No full import/export matrix |
| H11 | 3MF writer linked, dedicated library | Observed | Exported reader not imported by EXE |
| H12 | Local catalog of 16 models | Observed | Offline start untested |
| H13 | Undo/redo and autosave intended | Inferred from static evidence | Frequency, paths, limits pending |
| H14 | Windows print extensions | Observed | Integration not run |
| H15 | WSD create/send/query/cancel flow | Templates observed; sequence inferred | No printer traffic |
| H16 | Advanced fabrication parameters | Observed | Defaults and validation unknown |
| H17 | Online content through WinRT component | Observed/inferred | Current services not queried |
| H18 | Internet, LAN, files and 3D object permissions | Observed | Actual grant not observed |
| H19 | ETW and edit/repair events | Observed | No proof of external upload |
| H20 | Scan resources but no Kinect dependency | Observed | Dynamic integration not excluded |
| H21 | Catalog, login and online print routes | Literals/types observed | Some service purposes unknown |
| H22 | Debug-symbol reference was present | Observed | Symbols were not recovered |
| H23 | Installed application files matched the distributed package; Windows added generated resource indexes | Observed | Supports applying the static findings to the installed version |
| H24 | Windows installed only the x64 app plus the `language-es` and `scale-150` split packages | Observed | Resource packages picked per machine |
| H25 | Matching x64 VCLibs 140 and XAML 2.0 dependencies allowed installation | Observed | Resolves H03 |
| H26 | The app runs on a current Windows despite `MaxVersionTested` 10.0.17763.0: it wrote settings, recent items, a session record and thumbnails | Observed | Exact Windows build unknown |
| H27 | Settings are UWP `ApplicationData.LocalSettings` in `Settings/settings.dat` (registry hive), container `LocalState`, with render flags, UI flags and subcontainers `BuilderRecentItems`, `BuilderSessions`, `PrintHistory` | Observed | Only keys written in this session |
| H28 | Recent items form a circular queue (`QueueState`: count, size 24, head, tail, timeStamp); each item has `id`, `title`, `image` = `ms-appdata:///local/Thumbnails/<GUID><n>.png`, `itemType` 2, `showTitle`, `isRendered` | Observed | Meaning of `itemType` values inferred |
| H29 | Thumbnails for recents are 340×340 PNG in `LocalState/Thumbnails/`; the 3MF thumbnail is 512×512 RGBA | Observed | |
| H30 | Autosave location is `LocalState/AutoSaveFiles/`; it was empty after the session | Observed | Empty after a clean exit; frequency still unknown (H13 partial) |
| H31 | No web cache, cookies or history in `AC/INet*`; an empty `AC/Temp/Microsoft/OPC` folder exists | Observed | No sign of online services in this session; OPC = Windows packaging API, likely used for 3MF |
| H32 | 3MF export: core 2015/02 namespace, `unit="millimeter"`, `xml:lang="en-US"`, no `<metadata>`, `basematerials` with name + RGBA `displaycolor`, per-triangle `pid`/`p1`, mesh object wrapped by a components object, build item with a translation transform, vertices not welded (1,857 vs 1,840 unique) | Observed | One sample file |
| H33 | STL export: binary, 80-byte header all zeros, attribute bytes zero (colour lost), build transform baked into coordinates (matches the 3MF mesh + transform exactly), normals stored and consistent with winding | Observed | One sample file |
| H34 | The exported multi-part scene (cube, cylinder, pyramid joined into one mesh) is not clean: 16 degenerate faces, 23 zero-area faces, 11 non-manifold edges, 23 misoriented edges | Observed | Operations the user ran are not recorded; likely merge/union output |
| H35 | Repair on `camel.stl` (1,388,746 triangles, 20 boundary, 20 non-manifold, 24 misoriented edges) produced a closed, edge-manifold, consistently oriented mesh with +38 triangles and +16 vertices; volume −6.25 mm³ (−0.0016 %), bbox moved ≤ 0.0016 mm | Observed | Local, minimal-change repair (no remesh, no decimation); whether it was the simple or enhanced repair is unknown |
| H36 | Repaired camel has odd Euler characteristic (−91) with all edges manifold, so pinched (non-manifold) vertices remain | Derived | Repair targets edges, not vertex pinches |
| H37 | Autosave file is a plain 3MF named `{GUID}.3DBuilder.3mf` with only rels, model and content types: no thumbnail, no metadata | Observed | GUID differs from the session GUID in `BuilderSessions`; link unknown |
| H38 | The autosave exists while the app runs and was gone after a clean exit (H30), so it is deleted on clean exit | Inferred | Write interval still unknown |
| H39 | Restoring the autosave and saving gives identical mesh, materials and transform; the save only adds `<metadata name="Title">` (file stem) and a 512×512 thumbnail | Observed | `object.3mf` from a new scene had no Title, so Title likely appears only when the scene has a file name |
| H40 | Object scale lives in the 3MF build transform and is not baked into vertices (cylinder mesh ≈ 69.8 mm scaled by 0.57285 to 40 mm); STL/OBJ/PLY bake it | Observed | |
| H41 | Cylinder primitive: 120 segments, 240 vertices, 476 triangles, closed and manifold; placed centred on the origin, resting on z = 0 | Observed | Default size not confirmed (user may have resized) |
| H42 | Single-colour object gets object-level `pid`/`pindex` in 3MF; multi-colour objects use per-triangle `pid`/`p1` (H32) | Observed | |
| H43 | OBJ export: an exporter-identification comment, `mtllib`, `o Object.N` per object, one `usemtl` per part, only `v`/`f` (no normals or UVs), mm, Z-up, transforms baked, 6 decimals. MTL: `newmtl <Name>_<n>` with `Kd` only, in sRGB | Observed | |
| H44 | PLY export: binary little-endian 1.0, no comments, per-vertex float xyz + uchar RGBA, faces `list uchar int`, mm, baked, vertices welded | Observed | |
| H45 | glTF/GLB export: generator `3D Builder`, glTF 2.0; one root node with a child per object; each node matrix = object transform × 0.001 (mm→m) × −90° about X (Z-up→Y-up); positions stay in object space; one primitive per part, u32 indices, no normals or UVs; PBR material with linearised base colour, metallic 0, roughness 1, OPAQUE. GLB = same JSON without the buffer URI + identical BIN chunk | Observed | |
| H46 | UI: tabs Insertar, Objeto, Editar, Pintar, Ver, Ayuda; right selection pane; bottom move/rotate/scale bar with X/Y/Z fields; hamburger menu with file, print and settings entries | Observed | See User interface |
| H47 | Import asks for file units (µm, mm, cm, in, ft, m); invalid meshes get a red box and a toast that starts repair | Observed | |
| H48 | Biblioteca 3D shows categories (Cocina, Miniaturas y figurillas, Kit de trofeo) absent from the local catalog and packaged resources, suggesting a remote catalog response in the captured session; tile images stayed empty | Inferred | Host not identified; no traffic captured |
| H49 | Custom primitive (Personalizado): shape, latitude/longitude segments, width, height, roundness; defaults Cubo, 10, 10, 40 mm, 40 mm, 0 | Observed | Supports 40 mm as the default primitive size (H41) |
| H50 | Emboss (Relieve): pattern (text or shape), projection, bevel toggle, separate top and side colours, text and font; placed with handles and scaled in % with a uniform-scale lock | Observed | Result geometry not exported |
| H51 | Simplify shows current vs original face count and a reduction slider over a wireframe preview | Observed | |
| H52 | Plane cut (Dividir) keeps top, bottom or both; the plane is moved and rotated with a gizmo and a numeric offset | Observed | |
| H53 | Hollow (Ahuecar) has one parameter, minimum wall thickness (1 mm shown), with an x-ray preview | Observed | Slider range unknown |
| H54 | Measure (Medida) picks points on the surface and shows a distance in mm, with its own snap toggle | Observed | |
| H55 | Print prep is a separate view: printer (online service, installed, detected + refresh), material, design (units, scale, W/H/D, reset orientation/scale), job status window, estimated cost and a warnings counter | Observed | No printer installed, so slicer settings were not shown |
| H56 | The online print service failed ("Error desconocido") and the cost showed "No disponible", while the online catalog listed categories (H48): print ordering failed in that session while the catalog returned categories | Observed | Single attempt, no traffic captured |


## Remaining validation

- Test imports from other tools, especially OBJ, PLY, glTF and WRL.
- Measure the autosave interval and undo history limit.
- Compare repair and boolean behaviour using controlled meshes.
- Validate network and print flows only with a configured service or printer.
- Capture the remaining UI states described in [User interface](ui.md).
