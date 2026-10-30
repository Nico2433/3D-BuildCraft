# Data model

## Entities

| Entity | Responsibility |
|---|---|
| Geometry | Triangle meshes and topology for editing and repair |
| Material | Colour, textures and metallic/roughness or specular properties |
| Format adapter | Converts external geometry, transforms and materials into scene objects |
| Supports | Generated structures with overhang and density parameters |
| Settings | Preferences and recent items |
| Autosave | Recovery scene stored as 3MF |
| History | Reversible commands |
| Catalog entry | Model identity, title, description and thumbnail reference |

## Relationships

- A scene holds objects; objects hold meshes (possibly components) and materials.
- Print prep turns the scene into a 3MF (Lib3mfUAP writer) or G-code (`cExporter_MarlinGCode`).

### File formats

Manifest file associations: **STL, 3MF, OBJ, WRL, PLY, glTF, GLB**. Not a guarantee of every variant, nor that all are exportable. 3MF output is best supported (`CreateModelWriter3mf` import, `application/vnd.ms-3mfdocument` in print template). Lib3mfUAP exports a reader too, but the EXE import table shows only the writer.

### Built-in catalog

The catalog groups sample models and nine primitives: cube, cylinder, pyramid, cone, sphere, hexagon, wedge, torus and tetrahedron. Catalog models use standard 3MF ZIP containers with mesh, component, material and build-item structures. Offline availability was not tested.

### Settings and recovery behaviour

The original stores preferences in UWP local settings. These include render toggles, floor style, selection-pane visibility and teaching hints. Observed render settings enabled textures, shadows, reflections, smoothing, floor and labels; wireframe and x-ray were off. These are session observations, not proven defaults.

Recent items use a 24-slot circular queue with identifiers, titles, thumbnail references and display flags. Recent-item thumbnails are 340×340 PNGs. Session records track use, timestamp and last saved file. Recovery files are 3MF scenes in the app's local data area, removed on a clean exit. The autosave interval remains unknown.

### Export formats (observed)

3MF written by the app (a sample scene):

- Parts: `_rels/.rels`, `3D/3dmodel.model`, `Metadata/thumbnail.png` (512×512 RGBA), `[Content_Types].xml`. Relationship IDs are generated.
- Core namespace 2015/02, `unit="millimeter"`, `xml:lang="en-US"`, prefix `m` (material 2015/02) declared but unused; no `<metadata>` elements.
- `basematerials` with `name` and RGBA `displaycolor`; triangles carry `pid`/`p1`.
- One mesh object, wrapped by a second object that only holds a `<component>`; the build item references the wrapper with a `transform`.
- Vertices are not welded (duplicates kept).

- A single-colour object uses object-level `pid`/`pindex` instead of per-triangle values.
- Object scale and position stay in the build `transform`; vertices are not rescaled.
- `<metadata name="Title">` (file stem) appears only when the scene has a file name.

Autosave (`LocalState/AutoSaveFiles/{GUID}.3DBuilder.3mf`): the same 3MF writer, but no thumbnail and no metadata. Restoring and saving gives the identical mesh, materials and transform. The file is removed on a clean exit.

STL written by the app (sample scene and repaired-mesh exports): binary, zeroed 80-byte header, zero attribute bytes (no colour), build transform baked into coordinates, normals consistent with winding.

OBJ + MTL: an exporter-identification comment, `o Object.N` per object, one `usemtl` per part, `v`/`f` only (no normals or UVs), mm, Z-up, baked transforms. MTL has `newmtl <Name>_<n>` and an sRGB `Kd` only.

PLY: binary little-endian, per-vertex xyz float + RGBA uchar, faces `list uchar int`, no comments, mm, baked.

glTF 2.0 / GLB: `asset.generator = "3D Builder"`; root node with one child node per object; the node matrix carries the object transform, mm→m (×0.001) and Z-up→Y-up (−90° about X); one primitive per part, u32 indices, no normals or UVs; material `pbrMetallicRoughness` with linearised `baseColorFactor`, metallic 0, roughness 1. GLB embeds the same JSON and an identical binary buffer.

## Access rules

Not applicable (desktop app). Declared capabilities include `broadFileSystemAccess`; see [Architecture](architecture.md).

## Migrations

Not applicable. Still unknown: autosave interval, undo history limit, command format.
