//! A Model Context Protocol server, so an AI agent can use the app's tools: JSON-RPC 2.0
//! over stdio, one message per line, or over HTTP from the open app (`http`). The tools
//! themselves run in the app; this module only speaks the protocol and describes them.

pub mod http;

use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// What a tool call answers when no app window takes MCP calls.
pub const NOT_OPEN: &str =
    "3D BuildCraft is not open, or external agents (MCP) are off in its AI assistant settings. Open the app, turn them on and try again.";

/// Protocol versions we speak, newest first.
const VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

/// What a tool returns: MCP content blocks, or an error message for the agent.
pub type Reply = Result<Vec<Value>, String>;

/// Answers requests from `input` on `output` until `input` closes. `call` runs a tool.
pub fn serve(
    input: impl BufRead,
    mut output: impl Write,
    mut call: impl FnMut(&str, Value) -> Reply,
) -> std::io::Result<()> {
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(r) = reply(line.as_bytes(), &mut call) {
            writeln!(output, "{r}")?;
            output.flush()?;
        }
    }
    Ok(())
}

/// The JSON-RPC error code for a message that is not JSON.
const PARSE_ERROR: i32 = -32700;

/// The answer to one JSON-RPC message, or None when it needs none.
fn reply(message: &[u8], call: &mut impl FnMut(&str, Value) -> Reply) -> Option<Value> {
    match serde_json::from_slice::<Value>(message) {
        Ok(msg) => handle(&msg, call),
        Err(e) => Some(error(
            Value::Null,
            PARSE_ERROR,
            &format!("parse error: {e}"),
        )),
    }
}

fn handle(msg: &Value, call: &mut impl FnMut(&str, Value) -> Reply) -> Option<Value> {
    // Notifications (no id) and the client's own responses (no method) get no answer.
    let id = msg.get("id")?.clone();
    msg.get("method")?;
    let params = msg.get("params").cloned().unwrap_or(json!({}));
    let result = match msg["method"].as_str().unwrap_or("") {
        "initialize" => {
            let asked = params["protocolVersion"].as_str().unwrap_or("");
            let version = VERSIONS
                .iter()
                .find(|v| **v == asked)
                .unwrap_or(&VERSIONS[0]);
            json!({
                "protocolVersion": version,
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "buildcraft3d", "version": env!("CARGO_PKG_VERSION") },
                "instructions": INSTRUCTIONS,
            })
        }
        "ping" => json!({}),
        "tools/list" => json!({ "tools": tools() }),
        "tools/call" => {
            let name = params["name"].as_str().unwrap_or("");
            if !tools().iter().any(|t| t["name"] == name) {
                return Some(error(id, -32602, &format!("unknown tool: {name}")));
            }
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            match call(name, args) {
                Ok(content) => json!({ "content": content, "isError": false }),
                Err(e) => json!({ "content": [text(&e)], "isError": true }),
            }
        }
        other => return Some(error(id, -32601, &format!("method not found: {other}"))),
    };
    Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

fn error(id: Value, code: i32, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

pub fn text(s: &str) -> Value {
    json!({ "type": "text", "text": s })
}

pub fn png(bytes: &[u8]) -> Value {
    json!({ "type": "image", "data": base64(bytes), "mimeType": "image/png" })
}

fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | (*b as u32) << (16 - 8 * i));
        for i in 0..4 {
            out.push(if i <= chunk.len() {
                ABC[(n >> (18 - 6 * i) & 63) as usize] as char
            } else {
                '='
            });
        }
    }
    out
}

const INSTRUCTIONS: &str = concat!(
    "These tools work on the scene open in 3D BuildCraft, a 3D modelling app for 3D printing (the app must be open, with external agents on in its AI assistant settings). ",
    "Every change shows in the app and is one undo step. Tools that change the scene reply with the scene as get_scene lists it. ",
    "Call screenshot to see the result; give it a view (front, top, iso...) and focus ids to look from another angle without moving the user's camera. ",
    "If unsure whether a format or argument works, try it: the error says what is accepted and what to use instead.\n",
    crate::rules!()
);

/// How to use the tools, kept short and concrete so small local models follow it; the
/// in-app chat sends the same rules (`crate::assistant::SYSTEM`).
#[macro_export]
macro_rules! rules {
    () => {
        "Rules:\n\
- Units are millimetres, Z is up, the floor is Z = 0.\n\
- insert_shape puts a 40 mm shape on the floor at the centre and selects it. transform, \
set_color and edit act on the selection, so select the object you mean first.\n\
- In transform, position is the CENTRE of the bounding box. To set the positions of the \
sides instead, give bottom (min Z), left/right (min/max X), front/back (min/max Y). \
Bottom 0 = standing on the floor.\n\
- To measure, use measure; never cut or edit to measure.\n\
- To put B on top of A: get_scene, select B, transform with position [A.x, A.y, 0] and \
bottom = A's top.\n\
- To put B to the right of A: transform B with left = A's right; in front: back = A's \
front; behind: front = A's back.\n\
- Do exactly what the user asks and nothing else: no extra objects, colours, edits, prints, \
saves or opens. Never delete or replace objects unless asked. If the request is unclear, \
ask instead of guessing. When done, say briefly what you did.\n\
Example, a house: insert_shape cube; transform size [40, 40, 40]; insert_shape cone; \
transform size [50, 50, 30], position [0, 0, 0], bottom 40. The roof now sits on the box."
    };
}

/// The files a tool takes, for the inline-file fields' descriptions.
struct Files {
    /// A file name to show in `file_name`.
    example: &'static str,
    /// The formats `file_text` carries.
    text: &'static str,
    /// The formats `file_base64` carries.
    base64: &'static str,
}

/// Model files: `open` and `import`.
const MODEL_FILES: Files = Files {
    example: "part.stl",
    text: "SVG, OBJ, ASCII STL or PLY",
    base64: "3MF, STL, OBJ, PLY, GLB or SVG (glTF only with its data embedded)",
};

/// Pictures: Load image, Emboss's image pattern, textures and stickers.
const PICTURE_FILES: Files = Files {
    example: "logo.svg",
    text: "SVG",
    base64: "PNG, JPEG or SVG",
};

/// Adds `file_name`, `file_text` and `file_base64` to a tool's schema: an agent
/// on another machine or in a sandbox sends the file itself instead of `key`.
fn with_file(mut schema: Value, key: &str, files: &Files) -> Value {
    let text = |what: String| json!({ "type": "string", "description": what });
    let props = schema["properties"]
        .as_object_mut()
        .expect("an object schema");
    props.insert(
        "file_name".into(),
        text(format!(
            "instead of {key}, when the app cannot read the agent's files: the file's name with extension (it gives the format), e.g. {}; send its content in file_text or file_base64",
            files.example
        )),
    );
    props.insert(
        "file_text".into(),
        text(format!("the whole file as text: {}", files.text)),
    );
    props.insert(
        "file_base64".into(),
        text(format!(
            "the whole file in base64: {}; up to {} MB",
            files.base64,
            INLINE_MAX >> 20
        )),
    );
    schema
}

/// `edit`'s operations.
pub const EDIT_OPERATIONS: [&str; 16] = [
    "delete",
    "duplicate",
    "group",
    "ungroup",
    "merge",
    "subtract",
    "intersect",
    "repair",
    "smooth",
    "extrude_down",
    "settle",
    "mirror_x",
    "mirror_y",
    "mirror_z",
    "undo",
    "redo",
];

/// The tools, with their JSON schemas.
pub fn tools() -> Vec<Value> {
    let vec3 = |what: &str| json!({ "type": "array", "items": { "type": "number" }, "minItems": 3, "maxItems": 3, "description": what });
    let none = json!({ "type": "object", "properties": {} });
    let tool = |name: &str, description: &str, schema: Value| json!({ "name": name, "description": description, "inputSchema": schema });
    vec![
        tool(
            "measure",
            "Read-only measurements, all in mm. No arguments: min, max, size, volume (mm³) and surface_area (mm²) of each selected object and of the whole selection. from and to: the distance between two points and its X, Y, Z parts. ids: the gap between two objects' bounding boxes along X, Y and Z (0 on an axis where they touch or overlap, so touching boxes give [0, 0, 0] too) and the distance between their centres. radius_of (an object id): the radius and diameter of a round object: a sphere, a cylinder or the widest circle of a cone or torus (kind says which), with its centre and axis; errors when the object is not round. circle (three points on it): the radius, diameter, centre and axis of the circle through them, for a hole, a round edge or any feature. snap (true) moves each point of from/to/circle to the nearest vertex, edge middle, edge or face centre of an object within snap_tolerance mm (else leaves it), and for from/to lines the second point up with the first (perpendicular distance when both lie on parallel faces, exact along an axis when within 3 degrees of it; align false turns that off); the answer then also lists the points used, what each snapped to and the alignment. Without a mode argument it needs a selection.",
            json!({"type":"object","properties":{
                "from":vec3("first point [x, y, z], mm; needs to"),
                "to":vec3("second point [x, y, z], mm"),
                "ids":{"type":"array","items":{"type":"integer"},"minItems":2,"maxItems":2,"description":"two object ids from get_scene"},
                "radius_of":{"type":"integer","description":"object id from get_scene: radius and diameter of a sphere, cylinder, cone or torus"},
                "circle":{"type":"array","items":{"type":"array","items":{"type":"number"},"minItems":3,"maxItems":3},"minItems":3,"maxItems":3,"description":"three points [x, y, z] on a circle, mm: radius and diameter of the circle through them"},
                "snap":{"type":"boolean","description":"snap from/to/circle points to vertices, edge middles, edges and face centres of the objects (default false)"},
                "snap_tolerance":{"type":"number","description":"how far, in mm, a point may be from a feature to snap to it (default 1)"},
                "align":{"type":"boolean","description":"with snap, line the second point up with the first: perpendicular between parallel faces, or along an axis (default true)"}
            }}),
        ),
        tool(
            "get_scene",
            "Read-only: the objects, each with id, name, colour, center, center_of_mass (volume centroid, or the bounding box centre for near-zero volume), size, bottom/top (min/max Z), left/right (min/max X), front/back (min/max Y), rotation, triangle count, valid (a closed mesh), selected, material (none, simple, realistic or translucent) and the surface's extras as true/false: painted (areas painted with paint_area), textured (a texture brushed on) and stickers; plus the chosen slicer and can_undo/can_redo. Units mm and degrees.",
            none.clone(),
        ),
        tool(
            "select",
            "Replace the selection with the objects whose ids are given (ids from get_scene); [] selects nothing. transform, set_color, edit and the other editing tools act on the selection.",
            json!({ "type": "object", "properties": { "ids": { "type": "array", "items": { "type": "integer" }, "description": "object ids" } }, "required": ["ids"] }),
        ),
        tool(
            "transform",
            "Move, rotate or resize the selection. position is the centre of its bounding box; bottom sets min Z, left/right set min/max X, front/back set min/max Y, after position (left then right, front then back). To stack on an object, bottom = that object's top. rotation is degrees about X, Y, Z (each selected object), size is the bounding box in mm (the bottom stays where it was unless position or bottom is given).",
            json!({ "type": "object", "properties": { "position": vec3("centre, mm"), "rotation": vec3("degrees about X, Y, Z"), "size": vec3("bounding box, mm, all > 0"), "bottom": { "type": "number", "description": "height of the underside, mm; overrides position's Z (0 = on the floor)" }, "left": { "type": "number", "description": "minimum X, mm; overrides position's X" }, "right": { "type": "number", "description": "maximum X, mm; applied after left" }, "front": { "type": "number", "description": "minimum Y, mm; overrides position's Y" }, "back": { "type": "number", "description": "maximum Y, mm; applied after front" } } }),
        ),
        tool(
            "edit",
            "Apply an operation to the selection. delete removes it. duplicate adds copies to its right and selects them. group joins two or more objects into one without merging their shapes (each keeps its colour); ungroup splits an object made of separate solids into one object per solid. merge/intersect need two or more objects. subtract with one object cuts it out of every object it overlaps and removes it; with more, it takes the others away from the one with the lowest id. Pieces a subtraction leaves apart become separate objects. repair fixes invalid (not closed) meshes. smooth is the smooth tool with its defaults (3 passes, edges false); use the smooth tool for other settings. extrude_down fills the space under overhangs down to the floor. settle drops objects onto the floor. mirror_x/y/z flip it along that axis in place. undo and redo step through history and need no selection.",
            json!({ "type": "object", "properties": { "operation": { "type": "string", "enum": EDIT_OPERATIONS } }, "required": ["operation"] }),
        ),
        tool(
            "set_color",
            "Give each selected object this colour.",
            json!({ "type": "object", "properties": { "color": { "type": "string", "description": "#RRGGBB" } }, "required": ["color"] }),
        ),
        tool(
            "hollow",
            "Hollow each selected object, keeping walls of the given thickness.",
            json!({ "type": "object", "properties": { "wall": { "type": "number", "description": "wall thickness, mm, > 0" } }, "required": ["wall"] }),
        ),
        tool(
            "smooth",
            "Smooth the selection, as the Smooth tool. Without edges each pass splits faces while the model is small and relaxes the surface, keeping about its size (it shrinks slightly); with edges each pass rounds edges and corners off (Loop subdivision), shrinking it.",
            json!({ "type": "object", "properties": {
                "passes": { "type": "integer", "description": "0..6, default 3" },
                "edges": { "type": "boolean", "description": "smooth sharp edges too, default false" }
            } }),
        ),
        tool(
            "simplify",
            "Reduce the number of triangles of the selection, keeping its shape as well as it can.",
            json!({ "type": "object", "properties": { "remove": { "type": "number", "description": "fraction of the triangles to remove, 0..1 (0.5 halves them)" } }, "required": ["remove"] }),
        ),
        tool(
            "open",
            "Open a model file, replacing the scene: 3MF, STL, OBJ, PLY, glTF/GLB, or SVG (each colour becomes an object 3 mm tall). STL, OBJ and PLY are read as mm. Fails while the scene has unsaved changes unless discard_changes is true. The opened objects get new ids and are all selected; the undo history is cleared. To add to the scene instead use import; for a picture (PNG, JPEG) use load_image.",
            with_file(json!({ "type": "object", "properties": {
                "path": { "type": "string", "description": "model file path on the app's computer" },
                "discard_changes": { "type": "boolean", "description": "drop the scene's unsaved changes; default false" }
            } }), "path", &MODEL_FILES),
        ),
        tool(
            "save",
            "Save the whole scene to a file on the app's computer. The extension picks the format: 3MF (keeps colours, materials and the model's details), STL, OBJ, PLY, GLB or glTF; a path without one gets .3mf. Fails if the file exists unless overwrite is true.",
            json!({ "type": "object", "properties": {
                "path": { "type": "string", "description": "file path, e.g. /home/me/box.3mf" },
                "overwrite": { "type": "boolean", "description": "replace an existing file; default false" }
            }, "required": ["path"] }),
        ),
        tool(
            "print",
            "Only when the user explicitly asks to print: open the scene in the user's chosen slicer, where it is sliced and sent to the printer. The file carries the printer profile, print profile and filaments chosen with print_setup.",
            none,
        ),
        tool(
            "screenshot",
            "A PNG picture of the app window as it is now, with any dialog that is open. With any of view, azimuth, elevation, focus, isolate or size it instead draws the scene off screen from that angle (parallel projection, object and painted colours, textures and stickers, grey background), framed on the focused objects; this never moves the user's camera or changes the selection, and works while the app is minimized. Example: view top, focus [3], isolate true shows object 3 alone from above.",
            json!({ "type": "object", "properties": {
                "view": { "type": "string", "enum": ["front", "back", "left", "right", "top", "bottom", "iso"], "description": "front looks from -Y towards +Y (X to the right), left from -X, right from +X, top down from +Z with the front at the bottom of the picture; iso is from the front right and above. Not with azimuth/elevation" },
                "azimuth": { "type": "number", "description": "degrees around the vertical, 0 = front, 90 = right, 180 = back, 270 = left; default 45. Not with view" },
                "elevation": { "type": "number", "description": "degrees above the floor, -90 (from below) to 90 (from above); default 35. Not with view" },
                "focus": { "description": "what to centre and frame: \"all\" (default), \"selection\", or a list of object ids from get_scene", "oneOf": [{ "type": "string", "enum": ["all", "selection"] }, { "type": "array", "items": { "type": "integer" }, "minItems": 1 }] },
                "isolate": { "type": "boolean", "description": "draw only the focused objects; default false" },
                "size": { "type": "array", "items": { "type": "integer" }, "minItems": 2, "maxItems": 2, "description": "[width, height] in pixels, each 64..2048; default [800, 600]" }
            } }),
        ),
    ]
    .into_iter()
    .chain(model_tools())
    .chain(print_tools())
    .collect()
}

/// What the picture tools say about an SVG picture.
const SVG_PICTURE: &str = "an SVG is drawn as a picture (its filled and stroked shapes in their colours, transparent elsewhere)";

/// Modelling tools: shapes, painting, textures, materials, Split, Emboss and adding files.
fn model_tools() -> Vec<Value> {
    let vec3 = |what: &str| json!({ "type": "array", "items": { "type": "number" }, "minItems": 3, "maxItems": 3, "description": what });
    let num = |what: &str| json!({ "type": "number", "description": what });
    let int = |what: &str| json!({ "type": "integer", "description": what });
    let flag = |what: &str| json!({ "type": "boolean", "description": what });
    let text = |what: &str| json!({ "type": "string", "description": what });
    let one_of = |names: &[&str], what: &str| json!({ "type": "string", "enum": names, "description": what });
    let color = |what: &str| text(&format!("{what}, #RRGGBB"));
    let tool = |name: &str, description: &str, schema: Value| json!({ "name": name, "description": description, "inputSchema": schema });
    let textures: Vec<String> = crate::texture_gen::Kind::all().map(|k| k.name()).collect();
    // Text, every built-in shape by name, and a picture.
    let shapes: Vec<String> = crate::emboss::Shape::ALL.iter().map(|s| s.name()).collect();
    let patterns: Vec<&str> = std::iter::once("text")
        .chain(shapes.iter().map(String::as_str))
        .chain(["image"])
        .collect();
    let textures: Vec<&str> = textures.iter().map(String::as_str).collect();
    let point = "a world point in mm on (or within 5 mm of) the surface; the nearest triangle is used. Points from get_scene bounds work, e.g. [center x, center y, top] is the middle of a flat top";
    let picture = format!(
        "picture file path on the app's computer: {}; {SVG_PICTURE}",
        crate::picture::FORMATS
    );
    vec![
        tool(
            "insert_shape",
            "Add a shape on the floor at the centre (it may overlap objects already there; move it with transform); it becomes the selection. Without size it fits a 40 mm box. custom builds it as the Custom tool does (cube, sphere, cone, torus or cylinder only) with its own subdivisions and roundness.",
            json!({ "type": "object", "properties": {
                "shape": { "type": "string", "enum": ["cube", "cylinder", "pyramid", "cone", "sphere", "hexagon", "wedge", "torus", "tetrahedron"] },
                "size": vec3("bounding box [x, y, z] in mm, all > 0; applied last, the bottom stays on the floor"),
                "custom": { "type": "object", "description": "Custom tool settings", "properties": {
                    "latitude": int("subdivisions from bottom to top (box, sphere, torus), 3..48, default 10"),
                    "longitude": int("subdivisions around (or across for the cube), 3..48, default 10"),
                    "width": num("mm across, 1..1000, default 40; the shape keeps its proportions across"),
                    "height": num("mm, 1..1000, default 40"),
                    "roundness": int("0 (sharp) to 10 (round), default 0"),
                } },
            }, "required": ["shape"] }),
        ),
        tool(
            "split",
            "Cut the selection with a plane, as the Split tool. With only z the plane is horizontal at that height. For a slanted plane give point (or z, which replaces point's Z; the default point is the selection's centre) and either normal or rotation. top is the side the normal points to (+Z when level). width and length give the plane a finite size (along its own X and Y); then only the part of the object inside the box that rectangle sweeps along the normal is cut (needs a closed, manifold object). The kept pieces stay in place, replace the cut objects with new ids (call get_scene) and become the selection. Fails, changing nothing, when the plane misses a selected object.",
            json!({ "type": "object", "properties": {
                "z": num("height of a horizontal plane, mm"),
                "point": vec3("a point the plane passes through, mm"),
                "normal": vec3("direction the plane faces (its top side), any length > 0"),
                "rotation": vec3("the Split tool's plane angles in degrees about X, Y, Z (as an object's rotation); [0, 0, 0] is level"),
                "keep": one_of(&["top", "bottom", "both"], "default top; with a finite plane, both gives the part inside the box below the plane and the rest"),
                "width": num("plane width along its own X, mm; default 0, the whole object"),
                "length": num("plane length along its own Y, mm; default 0, the whole object"),
            } }),
        ),
        tool(
            "emboss_text",
            "Emboss (or engrave) a pattern onto the one selected object, as the Emboss tool: text, one of the built-in shapes (named in pattern), or the dark areas of a picture (PNG, JPEG or SVG). planar projects straight onto a face: the top, centred, unless at puts it on another face (any side or slant) or offset moves it; cylindrical wraps it around the object's vertical axis on its front (-Y), as a label round a mug; spherical wraps it around the object's centre on its front, as raised lettering on a ball. A top_color or side_color different from the object's makes that part a separate object; all pieces are selected afterwards.",
            with_file(json!({ "type": "object", "properties": {
                "pattern": one_of(&patterns, "text, a shape (star is four-pointed, burst eight-pointed, cross a diagonal X, no_sign a ring with a bar, smiley a face with eyes and mouth cut out) or image; default text"),
                "text": text("the text, for pattern text"),
                "image": text(&format!("for pattern image, {picture}. Its dark, opaque areas are embossed")),
                "font": text("installed font family (see list_fonts); default the bundled font"),
                "size": num("capital letter height for text, overall height for shapes and images, mm, default 10; the width follows in proportion unless width is given"),
                "width": num("overall width of the pattern, mm, stretching it sideways without changing its height; default: in proportion to size"),
                "depth": num("height above the surface (or depth when engraving), mm, default 1"),
                "engrave": flag("cut into the surface instead of raising; default false"),
                "bevel": flag("slope the edges; default false"),
                "angle": num("turn in its own plane, degrees, default 0"),
                "projection": one_of(&["planar", "cylindrical", "spherical"], "default planar"),
                "offset": { "type": "array", "items": { "type": "number" }, "minItems": 2, "maxItems": 2, "description": "[x, y] mm from the object's centre across the face the pattern lies on (planar; the top unless at chose another) or along the surface from the front (cylindrical, spherical)" },
                "at": vec3("a world point on the surface to centre the pattern on, mm; overrides offset. A planar pattern lies on the face there, facing out"),
                "top_color": color("colour of the top layer; default the object's"),
                "side_color": color("colour of the sides of a raised pattern; default the object's"),
            } }), "image", &PICTURE_FILES),
        ),
        tool(
            "paint_area",
            "Paint the surface region around a point, as the Color tool's click. The paint spreads from the triangle at point to neighbouring triangles across edges that bend by at most coverage × 180 degrees: 0 paints the flat face there, 0.5 stops at a box's 90-degree edges, 1 paints the whole connected surface. On curved meshes every edge bends a little, so even a small coverage can cover the whole curved surface; give radius to limit it. Works on any object; does not change the selection.",
            json!({ "type": "object", "properties": {
                "point": vec3(point),
                "color": color("the paint"),
                "coverage": num("0..1, default 0"),
                "radius": num("paint only triangles whose centre is within this many mm of point (the triangle at point always); default no limit"),
                "id": int("object to paint; default the object nearest to point"),
            }, "required": ["point", "color"] }),
        ),
        tool(
            "set_material",
            "Give the selection a material, as Paint → Material: none (plain), simple (matte), realistic (glossy) or translucent (see-through). Any type but none also gives each object the colour and clears its painted areas; textures and stickers stay. none only removes the material, keeping colour and paint.",
            json!({ "type": "object", "properties": {
                "type": one_of(&["none", "simple", "realistic", "translucent"], "the material"),
                "color": color("default the first selected object's colour; not used with none"),
            }, "required": ["type"] }),
        ),
        tool(
            "apply_texture",
            "Put a built-in texture or a picture (PNG, JPEG or SVG) on the selection's surface, as Paint → Texture; it colours the surface and does not change the shape. brush tiles it with box mapping: over every selected object when no point is given, else over the region around point (coverage and radius as in paint_area; on curved meshes give radius, or even a small coverage covers the whole curved surface). sticker prints it once at point as a flat picture facing out of the surface (no geometry; for raised lettering or shapes use emboss_text).",
            with_file(json!({ "type": "object", "properties": {
                "texture": one_of(&textures, "a built-in texture (the last six are white stencil shapes); give this or image"),
                "image": text(&format!("{picture}; give this or texture")),
                "mode": one_of(&["brush", "sticker"], "default brush; sticker needs point"),
                "point": vec3(point),
                "coverage": num("brush with point: 0..1, default 1 (the whole connected surface)"),
                "radius": num("brush with point: texture only triangles whose centre is within this many mm of point (the triangle at point always); default no limit"),
                "id": int("a selected object to use; default the selected object nearest to point"),
                "size": num("sticker width, mm; default 40% of the object's longest side"),
                "angle": num("sticker turn about the surface normal, degrees, default 0"),
            } }), "image", &PICTURE_FILES),
        ),
        tool(
            "import",
            "Add a model file to the scene without replacing it, as Add → Import model: 3MF, STL, OBJ, PLY, glTF/GLB, or an SVG drawing (each colour becomes an object 3 mm tall, at the drawing's real size). It lands centred on the floor (it may overlap objects already there; move it with transform); its objects get new ids (call get_scene) and become the selection. For a picture (PNG, JPEG) use load_image.",
            with_file(json!({ "type": "object", "properties": {
                "path": text("model file path on the app's computer"),
                "units": one_of(&["micron", "mm", "cm", "inch", "foot", "m"], "what one unit of an STL, OBJ or PLY file is, default mm; 3MF, glTF and SVG carry their own"),
            } }), "path", &MODEL_FILES),
        ),
        tool(
            "extrude_outline",
            "Add a border around (or inside) the selection's outline seen from above, as Edit → Extrude outline, e.g. to make a badge of an imported SVG. Several selected objects share one border. It rises from the lowest bottom. keep_model with one object merges it with its border into one object of its colour; with several objects (or a mesh that is not a closed solid) the border is a new object of the first one's colour, selected with them. Without keep_model only the border remains, replacing the selection.",
            json!({ "type": "object", "properties": {
                "side": one_of(&["outside", "inside"], "outside: a ring around the outline; inside: a band just within its edge. Default outside"),
                "width": num("border width, mm, default 2"),
                "height": num("border height from the object's bottom, mm; default the selection's height"),
                "keep_model": flag("merge the border with the object (default true); false leaves only the border"),
                "smooth_outer": num("round the corners of the border's outer edge, 0 (sharp, default) to 1"),
                "smooth_inner": num("round the corners of the border's inner edge, 0 (sharp, default) to 1"),
            } }),
        ),
        tool(
            "load_image",
            "Turn a picture (PNG, JPEG or SVG) into a new solid, as Load image, added on the floor at the centre (it may overlap objects already there; move it with transform) and selected. contour: brightness in flat steps; height_map: smooth relief; edge: the dark areas' outline extruded; stamp: a mirrored two-level stamp. Dark is high unless invert; transparent counts as white. For a solid made of an SVG's own shapes at their real size, use import instead.",
            with_file(json!({ "type": "object", "properties": {
                "path": text(&picture),
                "method": one_of(&["contour", "height_map", "edge", "stamp"], "default contour"),
                "size": num("mm along the longer side (the height for edge), default 80"),
                "depth": num("most relief, mm, default 3; contour, height_map and stamp stand on a 0.6 mm base, so depth 3 is 3.6 mm tall"),
                "levels": int("contour steps, 2..8, default 8"),
                "smooth": num("blur before sampling, 0..1, default 1"),
                "invert": flag("light is high instead of dark; default false"),
                "textures": flag("colour the top with the picture, default true"),
            } }), "path", &PICTURE_FILES),
        ),
    ]
}

/// The tools for the model's metadata, the 3D print setup and printing on paper.
fn print_tools() -> Vec<Value> {
    let none = json!({ "type": "object", "properties": {} });
    let tool = |name: &str, description: &str, schema: Value| json!({ "name": name, "description": description, "inputSchema": schema });
    let field = |what: &str| json!({ "type": "string", "description": what });
    vec![
        tool(
            "model_info",
            "Read or set the model's metadata (\"About the model\"), saved in 3MF files and sent to the slicer. With no arguments it only reads. Given fields replace those fields; an empty string clears one. The title also names the window and the print file. Replies with all the fields.",
            json!({ "type": "object", "properties": {
                "title": field("model title"),
                "designer": field("who designed it"),
                "copyright": field("copyright notice"),
                "license_terms": field("licence, e.g. CC BY 4.0"),
                "description": field("free text"),
            } }),
        ),
        tool(
            "print_setup",
            "Read or change the 3D print setup without printing. With no arguments it returns the detected slicers, the chosen slicer, its printer profiles, the print profiles of the chosen printer, the printer's filament slots (numbered from 1) and the slot each scene colour prints with. Given arguments are applied in order: slicer, printer, process, filament, filaments; each list in the answer is read again after the change. Changing the slicer clears the printer and process; changing the printer clears the process. Nothing here prints.",
            json!({ "type": "object", "properties": {
                "slicer": field("a slicer name from slicers, or the path of a slicer program"),
                "printer": field("a printer profile name from printers"),
                "process": field("a print profile name from processes"),
                "filament": { "type": "integer", "minimum": 1, "description": "filament slot for every scene colour" },
                "filaments": { "type": "array", "description": "filament slot per scene colour", "items": { "type": "object", "properties": { "color": field("a scene colour, #RRGGBB"), "slot": { "type": "integer", "minimum": 1 } }, "required": ["color", "slot"] } },
            } }),
        ),
        tool(
            "list_fonts",
            "Read-only: the installed font families that emboss_text accepts in font, as count and fonts. Without font, emboss_text uses the bundled font.",
            json!({ "type": "object", "properties": { "filter": field("only families whose name contains this text, ignoring case") } }),
        ),
        tool(
            "printer_status",
            "Read-only: the printers connected in the slicer and what they are doing (state, progress, file, nozzle and bed temperatures in °C). Offline means it did not answer within two seconds.",
            none.clone(),
        ),
        tool(
            "print_on_paper",
            "Only when the user explicitly asks to print on paper: draw the scene and open the system print dialog, where the user picks the printer and confirms.",
            none,
        ),
    ]
}

/// Where a running app tells agents how to reach it: `{ "port", "token" }`.
fn endpoint_file() -> Option<std::path::PathBuf> {
    crate::settings::dirs().map(|d| d.data_local_dir().join("mcp.json"))
}

/// The local port the `--mcp` stdio bridge forwards tool calls to, so commands started
/// elsewhere work on the window that is already open. Stops when dropped.
pub struct Listener {
    stop: Arc<AtomicBool>,
}

impl Listener {
    /// Listens on 127.0.0.1; a client must send the token from the endpoint file (readable
    /// by this user only) as its first line.
    pub fn start(
        call: impl Fn(&str, Value) -> Reply + Clone + Send + 'static,
    ) -> anyhow::Result<Self> {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        let token = uuid::Uuid::new_v4().to_string();
        let file = endpoint_file().ok_or_else(|| anyhow::anyhow!("no data folder"))?;
        let port = listener.local_addr()?.port();
        write_private(
            &file,
            json!({ "port": port, "token": token })
                .to_string()
                .as_bytes(),
        )?;
        let stop = accept(listener, move |stream| {
            let Ok(out) = stream.try_clone() else { return };
            let mut input = std::io::BufReader::new(stream);
            let mut first = String::new();
            if input.read_line(&mut first).is_err() || first.trim() != token {
                return;
            }
            if let Err(e) = serve(input, out, call.clone()) {
                log::info!("MCP client left: {e}");
            }
        })?;
        Ok(Self { stop })
    }
}

/// Writes a file only this user can read, creating its folder.
fn write_private(file: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut f = options.open(file)?;
    // The mode above applies only to a new file.
    #[cfg(unix)]
    f.set_permissions(std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
    f.write_all(bytes)
}

/// Hands each connection to `listener` to `serve` on its own thread, until the returned
/// flag is set; the port closes soon after.
fn accept(
    listener: std::net::TcpListener,
    serve: impl Fn(std::net::TcpStream) + Clone + Send + 'static,
) -> std::io::Result<Arc<AtomicBool>> {
    listener.set_nonblocking(true)?;
    let stop = Arc::new(AtomicBool::new(false));
    let quit = stop.clone();
    std::thread::spawn(move || {
        while !quit.load(Ordering::Relaxed) {
            let stream = match listener.accept() {
                Ok((s, _)) => s,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    continue;
                }
                Err(e) => {
                    log::warn!("MCP listener: {e}");
                    break;
                }
            };
            let serve = serve.clone();
            std::thread::spawn(move || {
                let _ = stream.set_nonblocking(false);
                serve(stream);
            });
        }
    });
    Ok(stop)
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(f) = endpoint_file() {
            let _ = std::fs::remove_file(f);
        }
    }
}

/// Runs a windowless stdio server, finding the running app anew for every tool call.
pub fn bridge() -> std::io::Result<()> {
    serve(
        std::io::stdin().lock(),
        std::io::stdout().lock(),
        |name, args| {
            let file = endpoint_file().ok_or(NOT_OPEN)?;
            call_app(&file, name, args)
        },
    )
}

/// Forwards one tool call through the app's authenticated local endpoint.
pub fn call_app(file: &std::path::Path, name: &str, args: Value) -> Reply {
    let forward = || -> anyhow::Result<Reply> {
        let v: Value = serde_json::from_str(&std::fs::read_to_string(file)?)?;
        let port = u16::try_from(
            v["port"]
                .as_u64()
                .ok_or_else(|| anyhow::anyhow!("invalid port"))?,
        )?;
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
        let mut stream =
            std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(1))?;
        writeln!(stream, "{}", v["token"].as_str().unwrap_or(""))?;
        writeln!(
            stream,
            "{}",
            json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":args}})
        )?;
        let mut line = String::new();
        std::io::BufReader::new(stream).read_line(&mut line)?;
        let reply: Value = serde_json::from_str(&line)?;
        let result = &reply["result"];
        if result["isError"] == true {
            return Ok(Err(result["content"][0]["text"]
                .as_str()
                .unwrap_or("Tool failed")
                .to_owned()));
        }
        Ok(Ok(result["content"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("invalid reply"))?
            .clone()))
    };
    forward().unwrap_or_else(|_| Err(NOT_OPEN.into()))
}

/// The error a tool that acts on the selection gives when nothing is selected.
pub fn nothing_selected(tool: &str) -> String {
    format!("{tool} acts on the selection, and nothing is selected; call select with ids from get_scene first")
}

/// Checks a call's arguments against the tool's own `inputSchema` from [`tools`]: every
/// argument must be one the tool has, with the JSON type its schema gives (null counts
/// as left out) and one of its `enum` names if it lists them. Ranges are left to the
/// tool, whose errors say more.
pub fn check_args(tool: &str, args: &Value) -> Result<(), String> {
    let Some(t) = tools().into_iter().find(|t| t["name"] == tool) else {
        return Err(format!("unknown tool: {tool}; tools/list lists the tools"));
    };
    if args.is_null() {
        return Ok(());
    }
    check_object(tool, "", &t["inputSchema"], args)
}

/// `value` against the object `schema`; `path` names it in errors ("custom." inside).
fn check_object(tool: &str, path: &str, schema: &Value, value: &Value) -> Result<(), String> {
    let Some(given) = value.as_object() else {
        return Err(format!("{tool}'s arguments must be an object, e.g. {{}}"));
    };
    let props = schema["properties"].as_object();
    for (key, v) in given {
        let Some(prop) = props.and_then(|p| p.get(key)) else {
            let names: Vec<&str> = props
                .map(|p| p.keys().map(String::as_str).collect())
                .unwrap_or_default();
            let what = if path.is_empty() {
                tool.to_owned()
            } else {
                format!("{tool} {}", path.trim_end_matches('.'))
            };
            return Err(if names.is_empty() {
                format!("unknown argument {path}{key}: {what} takes no arguments")
            } else {
                format!(
                    "unknown argument {path}{key}: {what} takes {}",
                    names.join(", ")
                )
            });
        };
        if let (Some(choices), Some(name)) = (prop["enum"].as_array(), v.as_str()) {
            if !choices.iter().any(|c| c == name) {
                let names: Vec<&str> = choices.iter().filter_map(Value::as_str).collect();
                return Err(format!(
                    "unknown {path}{key}: {name}; one of {}",
                    names.join(", ")
                ));
            }
        }
        if v.is_null() || fits(prop, v) {
            if prop["type"] == "object" {
                check_object(tool, &format!("{path}{key}."), prop, v)?;
            }
            continue;
        }
        return Err(format!("{path}{key} must be {}", expected(prop)));
    }
    Ok(())
}

/// Whether `v` has the JSON type `schema` asks for (any of `oneOf`).
fn fits(schema: &Value, v: &Value) -> bool {
    if let Some(options) = schema["oneOf"].as_array() {
        return options.iter().any(|o| fits(o, v));
    }
    match schema["type"].as_str() {
        Some("number") => v.is_number(),
        Some("integer") => v.is_i64() || v.is_u64(),
        Some("boolean") => v.is_boolean(),
        Some("string") => v.is_string(),
        Some("object") => v.is_object(),
        Some("array") => v.as_array().is_some_and(|items| {
            let len = items.len() as u64;
            schema["minItems"].as_u64().is_none_or(|min| len >= min)
                && schema["maxItems"].as_u64().is_none_or(|max| len <= max)
                && items.iter().all(|i| fits(&schema["items"], i))
        }),
        _ => true,
    }
}

/// What `schema` takes, for an error: its type, the unit its description names, and for
/// lengths how to convert from centimetres.
fn expected(schema: &Value) -> String {
    if let Some(options) = schema["oneOf"].as_array() {
        let all: Vec<String> = options.iter().map(expected).collect();
        return all.join(" or ");
    }
    let what = schema["description"].as_str().unwrap_or("");
    let has = |word: &str| {
        what.split(|c: char| !c.is_alphanumeric())
            .any(|w| w == word)
    };
    let mm = has("mm");
    let unit = if mm {
        " in mm"
    } else if has("degrees") {
        " in degrees"
    } else {
        ""
    };
    let kind = match schema["type"].as_str() {
        Some("number") => format!("a number{unit}"),
        Some("integer") => "a whole number".to_owned(),
        Some("boolean") => "true or false".to_owned(),
        Some("string") => match schema["enum"].as_array() {
            Some(names) => {
                let names: Vec<&str> = names.iter().filter_map(Value::as_str).collect();
                format!("one of {}", names.join(", "))
            }
            None => "a string".to_owned(),
        },
        Some("object") => "an object".to_owned(),
        Some("array") => {
            let item = match schema["items"]["type"].as_str() {
                Some("integer") => "whole numbers",
                Some("number") => "numbers",
                Some("string") => "strings",
                _ => "items",
            };
            match (schema["minItems"].as_u64(), schema["maxItems"].as_u64()) {
                (Some(2), Some(2)) if item == "numbers" => format!("[x, y]{unit}, two numbers"),
                (Some(3), Some(3)) if item == "numbers" => {
                    format!("[x, y, z]{unit}, three numbers")
                }
                (Some(a), Some(b)) if a == b => format!("a list of {a} {item}"),
                _ => format!("a list of {item}"),
            }
        }
        _ => "something else".to_owned(),
    };
    if mm {
        format!("{kind}; for cm multiply by 10")
    } else {
        kind
    }
}

/// The largest file an agent may send inline.
pub const INLINE_MAX: usize = 25 << 20;

/// Decodes standard base64 (padding optional, whitespace ignored).
fn unbase64(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let (mut n, mut bits) = (0u32, 0);
    for c in s.bytes().filter(|c| !c.is_ascii_whitespace() && *c != b'=') {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        };
        n = n << 6 | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((n >> bits) as u8);
        }
    }
    Some(out)
}

/// When `args` carry a file inline (`file_name` with `file_text` or
/// `file_base64`), writes it to a new folder inside `dir` and returns the
/// arguments with `key` set to its path. Only the base name is kept, so the
/// agent never chooses where the file goes.
pub fn inline_file(
    args: &Value,
    key: &str,
    dir: &std::path::Path,
) -> Result<Option<Value>, String> {
    let (text, data) = (args["file_text"].as_str(), args["file_base64"].as_str());
    if text.is_none() && data.is_none() {
        if !args["file_name"].is_null() {
            return Err("file_name needs file_text or file_base64 with the file's content".into());
        }
        return Ok(None);
    }
    if !args[key].is_null() {
        return Err(format!(
            "send either {key} or the file inline (file_name with file_text or file_base64), not both"
        ));
    }
    let bytes = match (text, data) {
        (Some(t), None) => t.as_bytes().to_vec(),
        (None, Some(d)) => unbase64(d).ok_or(
            "file_base64 is not valid base64; send a text format (SVG, OBJ, ASCII STL, PLY) as file_text instead",
        )?,
        _ => return Err("send file_text or file_base64, not both: file_text for a text file, file_base64 for any file".into()),
    };
    if bytes.len() > INLINE_MAX {
        return Err(format!(
            "the file is over {} MB; make it smaller, or put it where the app can read it and give its path",
            INLINE_MAX >> 20
        ));
    }
    let name: String = args["file_name"]
        .as_str()
        .ok_or("file_name is missing: the file's name with its extension, which gives the format")?
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, '.' | '-' | '_' | ' '))
        .collect();
    let name = name.trim().trim_start_matches('.');
    if !name.contains('.') {
        return Err(
            "file_name needs an extension, which gives the format, e.g. logo.svg or part.stl"
                .into(),
        );
    }
    // A folder per file: an earlier one may still be in use (an emboss picture).
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let folder = loop {
        let folder = dir.join(NEXT.fetch_add(1, Ordering::Relaxed).to_string());
        if !folder.exists() {
            break folder;
        }
    };
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let path = folder.join(name);
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    let mut args = args.clone();
    args[key] = json!(path.to_string_lossy());
    Ok(Some(args))
}

/// Where files sent inline land: a folder of this run under the system temp
/// folder. Folders left by runs over a day old are removed the first time.
pub fn inline_dir() -> std::path::PathBuf {
    static DIR: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        let root = std::env::temp_dir().join("buildcraft3d-agent-files");
        let day = std::time::Duration::from_secs(24 * 3600);
        for entry in std::fs::read_dir(&root).into_iter().flatten().flatten() {
            let old = entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age > day);
            if old {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
        root.join(std::process::id().to_string())
    })
    .clone()
}

/// The font families from `all` whose names contain `filter`, ignoring case.
pub fn list_fonts(all: &[String], filter: Option<&str>) -> Value {
    let filter = filter.map(|f| f.trim().to_lowercase()).unwrap_or_default();
    let fonts: Vec<&String> = all
        .iter()
        .filter(|n| n.to_lowercase().contains(&filter))
        .collect();
    json!({ "count": fonts.len(), "fonts": fonts })
}

/// The installed family named `name`, ignoring case; the error names up to
/// five families that contain or share a word with it.
pub fn find_font(all: &[String], name: &str) -> Result<String, String> {
    let wanted = name.trim().to_lowercase();
    if let Some(n) = all.iter().find(|n| n.to_lowercase() == wanted) {
        return Ok(n.clone());
    }
    let words: Vec<&str> = wanted.split_whitespace().filter(|w| w.len() > 2).collect();
    let similar: Vec<&str> = all
        .iter()
        .filter(|n| {
            let n = n.to_lowercase();
            (!wanted.is_empty() && n.contains(&wanted)) || words.iter().any(|w| n.contains(w))
        })
        .take(5)
        .map(String::as_str)
        .collect();
    let hint = if similar.is_empty() {
        String::new()
    } else {
        format!("; similar: {}", similar.join(", "))
    };
    Err(format!(
        "font {name} is not installed{hint}; call list_fonts for the installed fonts"
    ))
}

/// A point argument `[x, y, z]` in mm.
fn point_arg(v: &Value, what: &str) -> Result<glam::Vec3, String> {
    let bad = || format!("{what} must be a point [x, y, z] in mm");
    let a = v.as_array().filter(|a| a.len() == 3).ok_or_else(bad)?;
    let mut p = [0.0; 3];
    for i in 0..3 {
        p[i] = a[i].as_f64().ok_or_else(bad)? as f32;
    }
    Ok(glam::Vec3::from_array(p))
}

fn round_json(r: &crate::measure::Round) -> Value {
    json!({"kind":r.kind.name(),"radius":r.radius,"diameter":r.diameter(),"center":r.center.to_array(),"axis":r.axis.map(|a| a.to_array())})
}

/// Computes measurements without modifying the scene.
pub fn measure(scene: &crate::scene::Scene, args: &Value) -> Result<Value, String> {
    use crate::snapping::{align, Alignment};
    let vector = |v: glam::Vec3| v.to_array();
    let flag = |key: &str, default: bool| args[key].as_bool().unwrap_or(default);
    let snap = flag("snap", false);
    let tolerance = args["snap_tolerance"].as_f64().unwrap_or(1.0) as f32;
    if !tolerance.is_finite() || tolerance < 0.0 {
        return Err("snap_tolerance must be a distance in mm, 0 or more".into());
    }
    // Each point, snapped to the nearest face feature of an object when asked to.
    let pick = |v: &Value,
                what: &str|
     -> Result<(glam::Vec3, Option<crate::snapping::PointSnap>), String> {
        let p = point_arg(v, what)?;
        let hit = snap
            .then(|| crate::measure::snap_to_scene(scene, p, tolerance))
            .flatten();
        Ok((hit.map_or(p, |h| h.point), hit))
    };
    if let Some(id) = args.get("radius_of") {
        let o = id
            .as_u64()
            .and_then(|id| scene.get(id))
            .ok_or_else(|| format!("no object with id {id}; get_scene lists the ids"))?;
        let m = o.matrix();
        let axes = [m.x_axis, m.y_axis, m.z_axis].map(|a| a.truncate());
        return crate::measure::fit_object(&o.world_mesh(), axes)
            .map(|r| round_json(&r))
            .ok_or_else(|| {
                format!(
                    "object {} is not round: no sphere, cylinder or circular outline fits its vertices; give circle with three points on the feature instead",
                    o.id
                )
            });
    }
    if let Some(points) = args.get("circle") {
        let bad = "circle must be three points [[x, y, z], [x, y, z], [x, y, z]] on the circle, mm";
        let p = points.as_array().filter(|p| p.len() == 3).ok_or(bad)?;
        let picked = [pick(&p[0], bad)?, pick(&p[1], bad)?, pick(&p[2], bad)?];
        let round = crate::measure::circle_through(picked[0].0, picked[1].0, picked[2].0)
            .ok_or("the three points lie on a line or coincide, so no circle goes through them")?;
        let mut out = round_json(&round);
        if snap {
            out["snapped"] = json!(picked
                .iter()
                .map(|p| p.1.map(|h| h.kind.name()))
                .collect::<Vec<_>>());
            out["points"] = json!(picked.iter().map(|p| vector(p.0)).collect::<Vec<_>>());
        }
        return Ok(out);
    }
    if args.get("from").is_some() || args.get("to").is_some() {
        let both = "from and to must both be points [x, y, z] in mm";
        let (a, ha) = pick(&args["from"], both)?;
        let (mut b, hb) = pick(&args["to"], both)?;
        let mut alignment = None;
        if snap && flag("align", true) {
            if let Some((p, how)) = align(a, ha.map(|h| h.normal), b, hb.map(|h| h.normal)) {
                b = p;
                alignment = Some(match how {
                    Alignment::Perpendicular => "perpendicular".to_string(),
                    Alignment::Axis(i) => format!("along {}", ["X", "Y", "Z"][i]),
                });
            }
        }
        let d = b - a;
        let mut out = json!({"distance":d.length(),"differences":vector(d)});
        if snap {
            out["from"] = json!(vector(a));
            out["to"] = json!(vector(b));
            out["snapped"] = json!([ha.map(|h| h.kind.name()), hb.map(|h| h.kind.name())]);
            out["alignment"] = json!(alignment);
        }
        return Ok(out);
    }
    if let Some(ids) = args.get("ids") {
        let ids = ids
            .as_array()
            .filter(|a| a.len() == 2)
            .ok_or("ids must be two object ids from get_scene")?;
        let object = |i: usize| {
            scene
                .get(ids[i].as_u64().unwrap_or(u64::MAX))
                .ok_or_else(|| format!("no object with id {}; get_scene lists the ids", ids[i]))
        };
        let a = object(0)?.world_aabb();
        let b = object(1)?.world_aabb();
        return Ok(
            json!({"gap":vector((a.min-b.max).max(b.min-a.max).max(glam::Vec3::ZERO)),"centre_distance":a.center().distance(b.center())}),
        );
    }
    if scene.selection.is_empty() {
        return Err(format!(
            "{}; or give from and to, ids, radius_of or circle, to measure without a selection",
            nothing_selected("measure")
        ));
    }
    let mut bounds = crate::mesh::Aabb::empty();
    let (mut volume, mut area) = (0.0, 0.0);
    let mut objects = Vec::new();
    for o in scene
        .objects
        .iter()
        .filter(|o| scene.selection.contains(&o.id))
    {
        let b = o.world_aabb();
        let s = o.world_mesh().stats();
        bounds = bounds.union(b);
        volume += s.volume;
        area += s.area;
        objects.push(json!({"id":o.id,"min":vector(b.min),"max":vector(b.max),"size":vector(b.size()),"volume":s.volume,"surface_area":s.area}));
    }
    Ok(
        json!({"objects":objects,"total":{"min":vector(bounds.min),"max":vector(bounds.max),"size":vector(bounds.size()),"volume":volume,"surface_area":area}}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene_with(shape: crate::primitives::Primitive) -> (crate::scene::Scene, u64) {
        let mut scene = crate::scene::Scene::default();
        let o = crate::scene::Object::new("shape", shape.build(), crate::scene::DEFAULT_COLOR);
        let id = scene.add(o);
        (scene, id)
    }

    #[test]
    fn measure_radius_of_objects_and_circles() {
        use crate::primitives::Primitive;
        let (scene, id) = scene_with(Primitive::Cylinder);
        let r = measure(&scene, &json!({"radius_of":id})).unwrap();
        assert_eq!(r["kind"], "cylinder");
        let radius = r["radius"].as_f64().unwrap();
        assert!((r["diameter"].as_f64().unwrap() - 2.0 * radius).abs() < 1e-3);
        assert_eq!(
            measure(&scene_with(Primitive::Sphere).0, &json!({"radius_of":1})).unwrap()["kind"],
            "sphere"
        );
        let (cube, id) = scene_with(Primitive::Cube);
        let err = measure(&cube, &json!({"radius_of":id})).unwrap_err();
        assert!(err.contains("not round"), "{err}");
        assert!(measure(&cube, &json!({"radius_of":99}))
            .unwrap_err()
            .contains("no object"));
        let c = measure(&cube, &json!({"circle":[[5,0,0],[0,5,0],[-5,0,0]]})).unwrap();
        assert!((c["radius"].as_f64().unwrap() - 5.0).abs() < 1e-4);
        assert!(measure(&cube, &json!({"circle":[[0,0,0],[1,0,0],[2,0,0]]})).is_err());
        assert!(measure(&cube, &json!({"circle":[[0,0,0],[1,0,0]]})).is_err());
    }

    #[test]
    fn measure_snaps_and_aligns_points() {
        let (scene, id) = scene_with(crate::primitives::Primitive::Cube);
        let b = scene.get(id).unwrap().world_aabb();
        let (lo, hi) = (b.min, b.max);
        // A free distance is unchanged and has no snap fields.
        let free = measure(&scene, &json!({"from":[0,0,0],"to":[3,4,0]})).unwrap();
        assert!((free["distance"].as_f64().unwrap() - 5.0).abs() < 1e-5);
        assert!(free.get("snapped").is_none());
        // Near the top corner and over the bottom face: both snap, and the line is the height.
        let args = json!({"from":[hi.x - 0.2, hi.y - 0.1, hi.z],"to":[lo.x + 3.0, lo.y + 2.0, lo.z],"snap":true});
        let r = measure(&scene, &args).unwrap();
        assert_eq!(r["snapped"][0], "Vertex");
        assert_eq!(r["alignment"], "perpendicular");
        let height = (hi.z - lo.z) as f64;
        assert!((r["distance"].as_f64().unwrap() - height).abs() < 1e-3);
        let off = measure(&scene, &json!({"from":[hi.x - 0.2, hi.y - 0.1, hi.z],"to":[lo.x + 3.0, lo.y + 2.0, lo.z],"snap":true,"align":false})).unwrap();
        assert!(off["alignment"].is_null() && off["distance"].as_f64().unwrap() > height + 1.0);
        assert!(measure(
            &scene,
            &json!({"from":[0,0,0],"to":[1,1,1],"snap":true,"snap_tolerance":-1})
        )
        .is_err());
        let circle = measure(
            &scene,
            &json!({"circle":[[hi.x,hi.y,hi.z],[lo.x,hi.y,hi.z],[lo.x,lo.y,hi.z]],"snap":true}),
        )
        .unwrap();
        assert_eq!(circle["snapped"][2], "Vertex");
    }

    #[test]
    fn base64_matches_known_values() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64(&[0xFF, 0xFE]), "//4=");
    }
}
