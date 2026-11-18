//! A Model Context Protocol server, so an AI agent can use the app's tools: JSON-RPC 2.0
//! over stdio, one message per line, or over HTTP from the open app (`http`). The tools
//! themselves run in the app; this module only speaks the protocol and describes them.

pub mod args;
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
/// The code for JSON that is not a request object, such as a batch.
const INVALID_REQUEST: i32 = -32600;

/// The answer to one JSON-RPC message, or None when it needs none.
fn reply(message: &[u8], call: &mut impl FnMut(&str, Value) -> Reply) -> Option<Value> {
    match serde_json::from_slice::<Value>(message) {
        Ok(msg @ Value::Object(_)) => handle(&msg, call),
        Ok(Value::Array(_)) => Some(error(
            Value::Null,
            INVALID_REQUEST,
            "invalid request: batches are not supported; send one request object per message",
        )),
        Ok(_) => Some(error(
            Value::Null,
            INVALID_REQUEST,
            "invalid request: a JSON-RPC message is an object with jsonrpc, id, method and params",
        )),
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
    "Every change shows in the app and is one undo step. Tools that change the scene reply with the scene as get_scene lists it, plus changes: the ids created, removed and modified by that call. ",
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
- Units are millimetres, Z is up, the floor is Z = 0. Colours are #RRGGBB (the # is optional). \
An argument given as null counts as not given, in every tool.\n\
- insert_shape puts a 40 mm shape on the floor at the centre and selects it. transform, \
set_color, edit, hollow, smooth, simplify, split, emboss_text and the other editing tools act on the \
CURRENT selection, which changes after insert_shape, split, emboss_text, import and load_image: \
call select with the ids from get_scene before each one.\n\
- If a file path fails with \"no such file\", the app may run in a sandbox (Flatpak) that does not share \
your disk: send the file itself with file_name and file_text or file_base64. save writes on the app's \
computer, in a folder that exists there.\n\
- In transform, position is the CENTRE of the bounding box. To set the positions of the \
sides instead, give bottom (min Z), left/right (min/max X), front/back (min/max Y). \
Bottom 0 = standing on the floor.\n\
- To measure, use measure; never cut or edit to measure. For the diameter, centre or wall thickness of anything round, measure with section (near a point on the feature).\n\
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
            "Read-only measurements; lengths in mm, areas in mm², volume in mm³. Give ONE kind of measurement per call (two kinds is an error; call again). With none it measures the selection.\n\
- no argument: bounding box, volume and surface area of each selected object and of the selection.\n\
- section: slice an object with a plane and read the circles fitted to the cut. THE way to get the exact diameter, centre or wall thickness of anything round: a cylinder's rim, a pot's mouth, a hole, a cone at some height, a torus, a hollow wall. Complete example: {\"section\": {\"id\": 1, \"near\": [20, 0, 40]}} (id goes inside section).\n\
- diameter_of: only when the WHOLE object is a sphere, cylinder, cone (its base circle, not a rim: a cone has no top rim; for the diameter at another height use section with z) or torus (its outer equator). For a sphere use diameter_of, or section at the equator. Values carry about 1e-5 mm of floating point noise: round to 0.01 mm.\n\
- circle: three known points on a circle.\n\
- from and to: distance between two points. ids: the gap between the two objects' axis-aligned bounding boxes (for rotated parts use from/to with snap on both faces). Example: B 25 mm to the right of A: transform B with left = A.right + 25.\n\
- snap, snap_tolerance, align: catch from, to and circle points on the model. On a curved wall snap catches a facet of the polygon, so for diameters use section and for the gap between objects ids.\n\
Each argument's description says what it returns, with an example.",
            json!({"type":"object","properties":{
                "section":{"type":"object","properties":{
                    "id":{"type":"integer","description":"object id from get_scene"},
                    "near":vec3("[x, y, z] in mm on or near the feature (a rim, base edge, hole or wall). Alone it also gives the plane: horizontal at its height, tilted to where the contour is round (see tilt). With z or point it only picks the contour. A cylinder's top rim is [right, centre y, top] of get_scene"),
                    "z":{"type":"number","description":"height in mm of an exactly horizontal plane (not with point); give near too to choose the contour"},
                    "point":vec3("[x, y, z] in mm the plane passes through (not with z). Alone: horizontal there, tilted like near. With normal: exactly that plane"),
                    "normal":vec3("the plane's normal: [x, y, z] of any length above 0 (only the direction counts), default [0, 0, 1]; with z it must be vertical. A given normal is used exactly, never tilted"),
                    "tilt":{"type":"string","enum":["auto","none"],"description":"auto (default with near, or point without normal): when the horizontal contour is not round, as on a tilted cylinder or a pot cut at a slant, turn the plane to where it is, so diameters are true; the answer's plane.tilt_degrees and auto_tilted say so. none keeps it horizontal"}
                },"required":["id"],"description":"Slice object id with a plane (give near, or z, or point with normal). Returns plane, total, contours (role outer, inner or open; circular; diameter and radius in mm; center; axis; fit_error; why_not when not round; 50 listed at most), walls (thickness = outer radius minus inner radius, center_offset) and nearest (the contour closest to near, else null). A plane in a flat face (a rim, a base) is moved into the material by 0.02 mm per 100 mm of the object's largest size (shifted_mm); on the base or rim face of a cone the circle is measured that little inside, so its diameter differs by about 0.01 mm. Examples: cylinder top rim {id, near: [right, centre y, top]}, read contours[nearest].diameter; pot mouth: near on its rim; wall thickness: near on the outer wall at mid height, read walls[0].thickness"},
                "diameter_of":{"type":"integer","description":"object id from get_scene, only when the whole object is a sphere, cylinder, cone or torus. Returns kind, diameter and radius in mm, center, axis. A cone gives its widest circle, the base (center at the base); a torus its outer equator; both with kind circle. Errors when the object is not round: use section"},
                "circle":{"type":"array","items":{"type":"array","items":{"type":"number"},"minItems":3,"maxItems":3},"minItems":3,"maxItems":3,"description":"three points [x, y, z] in mm on a circle, e.g. [[5,0,0],[0,5,0],[-5,0,0]]. Returns kind, diameter and radius in mm, center, axis. Errors when they lie on a line"},
                "from":vec3("first point [x, y, z] in mm; needs to. Returns distance in mm and differences [dx, dy, dz]"),
                "to":vec3("second point [x, y, z] in mm; needs from"),
                "ids":{"type":"array","items":{"type":"integer"},"minItems":2,"maxItems":2,"description":"two object ids from get_scene. Returns gap [x, y, z] in mm between their bounding boxes (0 on an axis where they touch or overlap) and centre_distance"},
                "snap":{"type":"boolean","description":"default false. Moves each from, to and circle point to the nearest vertex, edge middle, edge or face centre of an object within snap_tolerance (else leaves it). The answer's snapped lists, per point, {kind, object id, curved_face} or null; for from/to also from, to and alignment. On a curved wall (a polygonal cylinder or sphere) a point lands on one facet, not on the true surface: use section for diameters and ids for the gap between two objects. Ignored by section"},
                "snap_tolerance":{"type":"number","description":format!("with snap: how far, in mm, a point may be from a feature to catch it; default 1, {}; 0 turns snapping and align off", args::limits::SNAP_TOLERANCE)},
                "align":{"type":"boolean","description":"with snap and from/to, default true: lines the second point up with the first, so the distance is perpendicular between parallel flat faces, or exact along X, Y or Z when within 3 degrees of it; never along the facets of a curved wall; false keeps the snapped points as they are"}
            }}),
        ),
        tool(
            "get_scene",
            "Read-only: the objects, each with id, name, colour, center, center_of_mass (volume centroid, or the bounding box centre for near-zero volume), size (the world bounding box, axis-aligned: for a rotated object it is the box round the turned shape, not the object's own dimensions), bottom/top (min/max Z), left/right (min/max X), front/back (min/max Y), rotation, triangle count, valid (a closed mesh), selected, material (none, simple, realistic or translucent) and the surface's extras as true/false: painted (areas painted with paint_area), textured (a texture brushed on) and stickers; plus the chosen slicer and can_undo/can_redo. Units mm and degrees. valid false: run edit repair. Call it to learn the ids; tools that change the scene reply in the same format, plus changes with the ids created, removed and modified.",
            none.clone(),
        ),
        tool(
            "select",
            "Replace the selection with the objects whose ids are given (ids from get_scene); [] selects nothing. transform, set_color, edit, hollow, smooth, simplify, split, emboss_text and the other editing tools act on the CURRENT selection; it changes after insert_shape, split, emboss_text, import and load_image, so select first.",
            json!({ "type": "object", "properties": { "ids": { "type": "array", "items": { "type": "integer" }, "description": "object ids" } }, "required": ["ids"] }),
        ),
        tool(
            "transform",
            "Move, rotate or resize the CURRENT selection, every unit mm or degrees (select first). Applied in this order: rotation, size, position, then bottom, left, right, front, back, which override position on their axis. To stack on an object, bottom = that object's top. Give only what changes; with none of the arguments nothing changes and the reply's note says so.",
            json!({ "type": "object", "properties": {
                "position": vec3(&format!("centre of the selection's bounding box, mm, each {}", args::limits::COORD)),
                "rotation": vec3(&format!("each selected object's orientation as degrees about X, Y, Z, as get_scene lists it, each {}: it replaces the current rotation (it is not added to it) and turns the object about its box centre", args::limits::ANGLE)),
                "size": vec3(&format!("bounding box, mm, each {} (the object is scaled; the bottom stays where it was unless position or bottom is given)", args::limits::OBJECT_SIZE)),
                "bottom": { "type": "number", "description": format!("height of the underside, mm, {}; overrides position's Z (0 = on the floor)", args::limits::COORD) },
                "left": { "type": "number", "description": "minimum X, mm; overrides position's X; same range as position" },
                "right": { "type": "number", "description": "maximum X, mm; applied after left" },
                "front": { "type": "number", "description": "minimum Y, mm; overrides position's Y" },
                "back": { "type": "number", "description": "maximum Y, mm; applied after front" } } }),
        ),
        tool(
            "edit",
            "Apply an operation to the CURRENT selection (select first). delete removes it. duplicate adds copies to its right and selects them. group joins two or more objects into one without merging their shapes (each keeps its colour); ungroup splits an object made of separate solids into one object per solid. merge/intersect need two or more selected objects, and they act on the selection only. CAUTION, subtract: with ONE selected object it cuts that object out of EVERY object it overlaps, selected or not, and removes it; with more, it takes the others away from the one with the lowest id. Pieces a subtraction leaves apart become separate objects. repair fixes invalid (not closed) meshes. smooth is the smooth tool with its defaults (3 passes, edges false); use the smooth tool for other settings. extrude_down fills the space under overhangs down to the floor. settle drops objects onto the floor. mirror_x/y/z flip it along that axis in place. undo and redo step through the history and need no selection: undo undoes the last history step, and a call that fails creates no step, so an undo after an error undoes the work before it (check can_undo and the scene it replies with).",
            json!({ "type": "object", "properties": { "operation": { "type": "string", "enum": EDIT_OPERATIONS, "description": "what to do to the selection; delete, duplicate, group, ungroup, merge, subtract, intersect and the rest are explained in the tool description" } }, "required": ["operation"] }),
        ),
        tool(
            "set_color",
            "Give each object of the CURRENT selection this colour (its whole surface; use paint_area for one region).",
            json!({ "type": "object", "properties": { "color": { "type": "string", "description": "#RRGGBB, e.g. #FF8000 (the # is optional, as in every colour argument)" } }, "required": ["color"] }),
        ),
        tool(
            "hollow",
            "Hollow each object of the CURRENT selection, leaving a closed wall of the given thickness: the cavity is sealed, so the top is closed too. To open the top, split the result at z = top - wall - 1 mm at least (keep bottom): at exactly top - wall the cavity stays closed, and within 0.5 mm of it the rim is thicker than the wall; the object ends that much shorter. The objects keep their ids. The wall comes out within about 0.25 mm of the request (3 mm gives about 3.07) and never thinner; voxels are a quarter of the wall, but never finer than 1/160 of the model's largest size, so very large models get a somewhat thicker wall; check it with measure section (walls[0].thickness). Errors: too thin when the smallest side is not more than twice the wall; too thick for this size when the cavity left is smaller than a voxel (a 10 mm cube takes walls up to 4.5 mm, not 4.99): ask for a thinner wall.",
            json!({ "type": "object", "properties": { "wall": { "type": "number", "description": format!("wall thickness, mm, {}, e.g. 2; the smallest side of the object must be more than twice the wall, and a little more than twice can still fail because the cavity is under one voxel (wall/4); the error says which", args::limits::WALL) } }, "required": ["wall"] }),
        ),
        tool(
            "smooth",
            "Smooth the CURRENT selection, as the Smooth tool; the objects keep their ids. Each pass splits every face in four while the model is small (a cube has 12, 48, 192, 768 and 3072 triangles after 0 to 4 passes) and relaxes the surface; without edges the surface relaxes into a pillow that is a little BIGGER than the model (a 10 mm cube is about 10.8 mm across after 3 passes, with about 80% of its volume), with edges each pass rounds edges and corners off (Loop subdivision), shrinking it a lot (the same cube is about 8.8 mm across). Measure afterwards and resize with transform if the size matters. Two passes leave a cube visibly faceted; use 3 or 4 for round.",
            json!({ "type": "object", "properties": {
                "passes": { "type": "integer", "description": "how strongly to smooth, a whole number 0..6, default 3; each pass multiplies the triangles by about 4 on a small model" },
                "edges": { "type": "boolean", "description": "round sharp edges and corners off too (the model shrinks a lot), default false" }
            } }),
        ),
        tool(
            "simplify",
            "Reduce the number of triangles of the CURRENT selection, keeping its shape as well as it can; the objects keep their ids.",
            json!({ "type": "object", "properties": { "remove": { "type": "number", "description": "fraction of the triangles to remove, 0..1 (0.5 halves them); get_scene lists the triangle count" } }, "required": ["remove"] }),
        ),
        tool(
            "open",
            "Open a model file, replacing the scene: 3MF, STL, OBJ, PLY, glTF/GLB, or SVG (each colour becomes an object 3 mm tall). STL, OBJ and PLY are read as mm. Fails while the scene has unsaved changes unless discard_changes is true. The opened objects get new ids (all selected; call get_scene) and the undo history is cleared. A file with no triangles, or larger than the limit of a tool's size (10000 mm along a side), is an error. To add to the scene instead use import; for a picture (PNG, JPEG) use load_image.",
            with_file(json!({ "type": "object", "properties": {
                "path": { "type": "string", "description": "model file path on the app's computer; if it says no such file, send the file with file_name and file_text or file_base64" },
                "discard_changes": { "type": "boolean", "description": "drop the scene's unsaved changes; default false" }
            } }), "path", &MODEL_FILES),
        ),
        tool(
            "save",
            "Save the whole scene to a file on the app's computer, as the Save as command does. The extension picks the format: 3MF (keeps colours, materials and the model's details), STL, OBJ, PLY, GLB or glTF. The path must be absolute and name a file with an extension: a relative path, a folder or a name without extension is an error. Fails if the file exists unless overwrite is true. Replies with the scene plus saved_to (the path written) and bytes. Like Save as, it names the scene after the file, so the model's default title (see model_info) becomes the file's name; a title set with model_info stays. The app writes on its own computer: the folder must exist there and be writable for it. A Flatpak app sees the real home folder but has its own private /tmp, not the agent's; other folders (/root, /proc) may be missing or read-only for it, and the error says which.",
            json!({ "type": "object", "properties": {
                "path": { "type": "string", "description": "absolute file path with an extension, in an existing folder, e.g. /home/me/box.3mf" },
                "overwrite": { "type": "boolean", "description": "replace an existing file; default false" }
            }, "required": ["path"] }),
        ),
        tool(
            "print",
            "Only when the user explicitly asks to print. It does NOT send anything to the printer: it saves the scene as a 3MF project (with the printer profile, print profile and filaments chosen with print_setup) and opens it in the user's chosen slicer, where the user slices and confirms the print. Without a chosen slicer it looks for installed ones and uses the first; if none is found it fails with no slicer found.",
            none,
        ),
        tool(
            "screenshot",
            "A PNG picture of the app window as it is now, with any dialog that is open. With any of view, azimuth, elevation, focus, isolate or size it instead draws the scene off screen from that angle (parallel projection, object and painted colours, textures and stickers, grey background), framed on the focused objects; this never moves the user's camera or changes the selection, and works while the app is minimized. Example: view top, focus [3], isolate true shows object 3 alone from above. view together with azimuth or elevation is an error (give one or the other); a focus id that does not exist is an error naming it.",
            json!({ "type": "object", "properties": {
                "view": { "type": "string", "enum": ["front", "back", "left", "right", "top", "bottom", "iso"], "description": "front looks from -Y towards +Y (X to the right), left from -X, right from +X, top down from +Z with the front at the bottom of the picture; iso is from the front right and above. Not with azimuth/elevation" },
                "azimuth": { "type": "number", "description": "degrees around the vertical, 0 = front, 90 = right, 180 = back, 270 = left; default 45. Not with view" },
                "elevation": { "type": "number", "description": "degrees above the floor, -90 (from below) to 90 (from above); default 35. Not with view" },
                "focus": { "description": "what to centre and frame: \"all\" (default), \"selection\", or a list of object ids from get_scene", "oneOf": [{ "type": "string", "enum": ["all", "selection"] }, { "type": "array", "items": { "type": "integer" }, "minItems": 1 }] },
                "isolate": { "type": "boolean", "description": "draw only the focused objects; default false" },
                "size": { "type": "array", "items": { "type": "integer" }, "minItems": 2, "maxItems": 2, "description": "[width, height] in pixels, whole numbers, each clamped to 64..2048 (0, a negative or 1000000000 give 64 or 2048); a decimal is an error; default [800, 600]" }
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
    let color = |what: &str| text(&format!("{what}, #RRGGBB (the # is optional)"));
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
            "Add a shape on the floor at the centre (it may overlap objects already there; move it with transform); it gets a new id and becomes the selection (the previous selection is replaced). Without size it fits a 40 mm box. custom builds it as the Custom tool does (cube, sphere, cone, torus or cylinder only) with its own subdivisions and roundness.",
            json!({ "type": "object", "properties": {
                "shape": { "type": "string", "enum": ["cube", "cylinder", "pyramid", "cone", "sphere", "hexagon", "wedge", "torus", "tetrahedron"], "description": "the shape to add" },
                "size": vec3(&format!("bounding box [x, y, z] in mm, each {}; applied last, the bottom stays on the floor", args::limits::OBJECT_SIZE)),
                "custom": { "type": "object", "description": "Custom tool settings, only for cube, sphere, cone, torus or cylinder; leave out for the plain shape. More subdivisions and roundness make smoother shapes with more triangles: a sphere has 180 triangles at 10x10 and 4512 at 48x48, and 16 times as many at roundness 10 (2880 and 72192)", "properties": {
                    "latitude": int(&format!("rings from bottom to top of a sphere or torus (rows on a cube's faces), {}, default 10", args::limits::CUSTOM_STEPS)),
                    "longitude": int(&format!("segments around the shape (columns on a cube's faces), {}, default 10", args::limits::CUSTOM_STEPS)),
                    "width": num(&format!("mm across, {}, default 40; the shape keeps its proportions across", args::limits::CUSTOM_SIZE)),
                    "height": num(&format!("mm, {}, default 40", args::limits::CUSTOM_SIZE)),
                    "roundness": int(&format!("0 (sharp) to 10 (round), {}, default 0: each step rounds the edges and corners off more and adds triangles", args::limits::ROUNDNESS)),
                } },
            }, "required": ["shape"] }),
        ),
        tool(
            "split",
            "Cut the CURRENT selection with a plane, as the Split tool. With only z the plane is horizontal at that height. For a slanted plane give point (or z, which replaces point's Z; the default point is the selection's centre) and either normal or rotation. top is the side the normal points to (+Z when level). width and length give the plane a finite size (along its own X and Y); then only the part of the object inside the box that rectangle sweeps along the normal is cut (needs a closed, manifold object). The kept pieces stay in place and replace the cut objects: the old ids are gone, the pieces have NEW ids (the answer's changes lists removed and created) and are the selection. Fails, changing nothing, when the plane misses a selected object or does not cut through it: z must be strictly between the object's bottom and top, and a finite plane must reach some of it.",
            json!({ "type": "object", "properties": {
                "z": num(&format!("height of a horizontal plane, mm, {}", args::limits::COORD)),
                "point": vec3("a point the plane passes through, mm; same range as z"),
                "normal": vec3("direction the plane faces (its top side), any length > 0"),
                "rotation": vec3(&format!("the Split tool's plane angles in degrees about X, Y, Z (as an object's rotation), each {}; [0, 0, 0] is level", args::limits::ANGLE)),
                "keep": one_of(&["top", "bottom", "both"], "default top; with a finite plane, both gives the part inside the box below the plane and the rest"),
                "width": num(&format!("plane width along its own X, mm, {}; default 0, the whole object", args::limits::PLANE_EXTENT)),
                "length": num(&format!("plane length along its own Y, mm, {}; default 0, the whole object", args::limits::PLANE_EXTENT)),
            } }),
        ),
        tool(
            "emboss_text",
            "Emboss (or engrave) a pattern onto the one selected object, as the Emboss tool: text, one of the built-in shapes (named in pattern), or the dark areas of a picture (PNG, JPEG or SVG). planar projects straight onto a face: the top, centred, unless at puts it on another face (any side or slant) or offset moves it; cylindrical wraps it around the object's vertical axis on its front (-Y), as a label round a mug; spherical wraps it around the object's centre on its front, as raised lettering on a ball. The pattern has the object's colour, so the relief is hard to see: give top_color. With top_color or side_color different from the object's, the relief (or part of it) ends up in OTHER, new objects (changes.created lists their ids) and the original object can stay unchanged (not in changes.modified); without them the object keeps its id and is modified. All pieces are selected afterwards. The work grows with the pattern: text up to 60 characters, and its box at most 100000 mm².",
            with_file(json!({ "type": "object", "properties": {
                "pattern": one_of(&patterns, "text, a shape (star is four-pointed, burst eight-pointed, cross a diagonal X, no_sign a ring with a bar, smiley a face with eyes and mouth cut out) or image; default text"),
                "text": text(&format!("the text, for pattern text, up to {} characters", args::limits::TEXT_CHARS)),
                "image": text(&format!("for pattern image, {picture}. Its dark, opaque areas are embossed")),
                "font": text("installed font family (see list_fonts); default the bundled font"),
                "size": num(&format!("capital letter height for text, overall height for shapes and images, mm, {}, default 10; the width follows in proportion unless width is given", args::limits::EMBOSS_SIZE)),
                "width": num(&format!("overall width of the pattern, mm, {}, stretching it sideways without changing its height; default: in proportion to size", args::limits::EMBOSS_WIDTH)),
                "depth": num(&format!("TOTAL height above the surface (or depth when engraving), mm, {}, default 1; with top_color only the top layer, 25% of the depth but at most 0.4 mm, is coloured", args::limits::EMBOSS_DEPTH)),
                "engrave": flag("cut into the surface instead of raising; default false"),
                "bevel": flag("slope the edges; default false"),
                "angle": num(&format!("turn in its own plane, degrees, {}, default 0", args::limits::TURN)),
                "projection": one_of(&["planar", "cylindrical", "spherical"], "default planar"),
                "offset": { "type": "array", "items": { "type": "number" }, "minItems": 2, "maxItems": 2, "description": format!("[x, y] mm from the object's centre across the face the pattern lies on (planar; the top unless at chose another) or along the surface from the front (cylindrical, spherical), each {}", args::limits::COORD) },
                "at": vec3("a world point on the surface to centre the pattern on, mm; overrides offset. A planar pattern lies on the face of the triangle nearest to it, facing out; on an edge or corner that choice is ambiguous, so aim at the middle of the face"),
                "top_color": color("colour of the top layer (the cap, see depth); default the object's, which makes the relief hard to see"),
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
                "radius": num(&format!("paint only triangles whose centre is within this many mm of point (the triangle at point always), {}; default no limit", args::limits::BRUSH_RADIUS)),
                "id": int("object to paint; default the object nearest to point"),
            }, "required": ["point", "color"] }),
        ),
        tool(
            "set_material",
            "Give the CURRENT selection a material, as Paint → Material: none (plain), simple (matte), realistic (glossy) or translucent (see-through). Any type but none also gives each object the colour and clears its painted areas; textures and stickers stay. none only removes the material, keeping colour and paint.",
            json!({ "type": "object", "properties": {
                "type": one_of(&["none", "simple", "realistic", "translucent"], "the material"),
                "color": color("default the first selected object's colour; not used with none"),
            }, "required": ["type"] }),
        ),
        tool(
            "apply_texture",
            "Put a built-in texture or a picture (PNG, JPEG or SVG) on the CURRENT selection's surface, as Paint → Texture; it colours the surface and does not change the shape. brush tiles it with box mapping: over every selected object when no point is given, else over the region around point (coverage and radius as in paint_area; on curved meshes give radius, or even a small coverage covers the whole curved surface). sticker prints it once at point as a flat picture facing out of the surface (no geometry; for raised lettering or shapes use emboss_text).",
            with_file(json!({ "type": "object", "properties": {
                "texture": one_of(&textures, "a built-in texture (the last six are white stencil shapes); give this or image"),
                "image": text(&format!("{picture}; give this or texture")),
                "mode": one_of(&["brush", "sticker"], "default brush; sticker needs point"),
                "point": vec3(point),
                "coverage": num("brush with point: 0..1, default 1 (the whole connected surface)"),
                "radius": num(&format!("brush with point: texture only triangles whose centre is within this many mm of point (the triangle at point always), {}; default no limit", args::limits::BRUSH_RADIUS)),
                "id": int("a selected object to use; default the selected object nearest to point"),
                "size": num(&format!("sticker width, mm, {}; default 40% of the object's longest side", args::limits::STICKER_SIZE)),
                "angle": num(&format!("sticker turn about the surface normal, degrees, {}, default 0", args::limits::TURN)),
            } }), "image", &PICTURE_FILES),
        ),
        tool(
            "import",
            "Add a model file to the scene without replacing it, as Add → Import model: 3MF, STL, OBJ, PLY, glTF/GLB, or an SVG drawing (each colour becomes an object 3 mm tall, at the drawing's real size). It lands centred on the floor (it may overlap objects already there; move it with transform); its objects get new ids (changes.created) and become the selection, replacing the previous one. A file with no triangles, or whose size (after units) is over 10000 mm along a side, is an error. For a picture (PNG, JPEG) use load_image.",
            with_file(json!({ "type": "object", "properties": {
                "path": text("model file path on the app's computer; if it says no such file, send the file with file_name and file_text or file_base64"),
                "units": one_of(&["micron", "mm", "cm", "inch", "foot", "m"], "what one unit of an STL, OBJ or PLY file is, default mm; 3MF, glTF and SVG carry their own"),
            } }), "path", &MODEL_FILES),
        ),
        tool(
            "extrude_outline",
            "Add a border around (or inside) the CURRENT selection's outline seen from above, as Edit → Extrude outline, e.g. to make a badge of an imported SVG. Several selected objects share one border. It rises from the lowest bottom. keep_model with one object merges it with its border into one object (the first selected object's id is kept, its colour; changes.modified can then mean a totally new shape); with several objects (or a mesh that is not a closed solid) the border is a new object of the first one's colour, selected with them. keep_model false DELETES the original: only the border remains, in the first selected object (it keeps that id) and the other selected objects are removed.",
            json!({ "type": "object", "properties": {
                "side": one_of(&["outside", "inside"], "outside: a ring around the outline; inside: a band just within its edge. Default outside"),
                "width": num(&format!("border width, mm, {}, default 2", args::limits::BORDER_WIDTH)),
                "height": num(&format!("border height from the object's bottom, mm, {}; default the selection's height", args::limits::BORDER_HEIGHT)),
                "keep_model": flag("merge the border with the object (default true); false DELETES the original and leaves only the border"),
                "smooth_outer": num("round the corners of the border's outer edge, 0 (sharp, default) to 1"),
                "smooth_inner": num("round the corners of the border's inner edge, 0 (sharp, default) to 1"),
            } }),
        ),
        tool(
            "load_image",
            "Turn a picture (PNG, JPEG or SVG) into a new solid, as Load image, added on the floor at the centre (it may overlap objects already there; move it with transform) as a new object with a new id, which becomes the selection. contour: brightness in flat steps; height_map: smooth relief; edge: the dark areas' outline extruded; stamp: a mirrored two-level stamp. Dark is high unless invert; transparent counts as white. For a solid made of an SVG's own shapes at their real size, use import instead.",
            with_file(json!({ "type": "object", "properties": {
                "path": text(&format!("{picture}; if it says no such file, send the file with file_name and file_text or file_base64")),
                "method": one_of(&["contour", "height_map", "edge", "stamp"], "default contour"),
                "size": num(&format!("mm along the longer side (the height for edge), {}, default 80; edge outlines may cover at most 100000 mm²", args::limits::IMAGE_SIZE)),
                "depth": num(&format!("most relief, mm, {}, default 3; contour, height_map and stamp stand on a 0.6 mm base, so depth 3 is 3.6 mm tall", args::limits::IMAGE_DEPTH)),
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
    let field = |what: &str| json!({ "type": "string", "description": format!("{what}, up to {} characters", args::limits::INFO_CHARS) });
    vec![
        tool(
            "model_info",
            "Read or set the model's metadata (\"About the model\"), saved in 3MF files and sent to the slicer. With no arguments it only reads. Given fields replace those fields; an empty string clears one, and clearing the title restores the default one, the scene's file name (or New scene). The title also names the window and the print file. Replies with all the fields as they are now, the default title included.",
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
                "filaments": { "type": "array", "description": "filament slot per scene colour", "items": { "type": "object", "properties": { "color": field("a scene colour, #RRGGBB"), "slot": { "type": "integer", "minimum": 1, "description": "filament slot, from 1" } }, "required": ["color", "slot"] } },
            } }),
        ),
        tool(
            "list_fonts",
            "Read-only: the installed font families that emboss_text accepts in font, as count (all matches) and fonts. Without font, emboss_text uses the bundled font.",
            json!({ "type": "object", "properties": {
                "filter": field("only families whose name contains this text, ignoring case"),
                "limit": { "type": "integer", "description": "list at most this many families (count still says how many match); default all" },
            } }),
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
            // Objects inside objects and inside lists are as strict as the top level.
            match (prop["type"].as_str(), v) {
                (Some("object"), v) if !v.is_null() => {
                    check_object(tool, &format!("{path}{key}."), prop, v)?;
                }
                (Some("array"), Value::Array(items)) if prop["items"]["type"] == "object" => {
                    for (i, item) in items.iter().enumerate() {
                        check_object(tool, &format!("{path}{key}[{i}]."), &prop["items"], item)?;
                    }
                }
                _ => {}
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

/// What `schema` takes, for an error: its type and the unit its description names.
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
    let unit = if has("mm") {
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
    kind
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
pub fn list_fonts(all: &[String], filter: Option<&str>, limit: Option<usize>) -> Value {
    let filter = filter.map(|f| f.trim().to_lowercase()).unwrap_or_default();
    let fonts: Vec<&String> = all
        .iter()
        .filter(|n| n.to_lowercase().contains(&filter))
        .collect();
    json!({ "count": fonts.len(), "fonts": fonts.iter().take(limit.unwrap_or(usize::MAX)).collect::<Vec<_>>() })
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

/// A point argument `[x, y, z]` in mm, each within [`args::limits::COORD`].
fn point_arg(v: &Value, what: &str) -> Result<glam::Vec3, String> {
    args::vec3(&json!({ "p": v }), "p", args::limits::COORD)
        .ok()
        .flatten()
        .ok_or_else(|| {
            format!(
                "{what} must be a point [x, y, z], three numbers each {}",
                args::limits::COORD
            )
        })
}

fn round_json(r: &crate::measure::Round) -> Value {
    json!({"kind":r.kind.name(),"diameter":r.diameter(),"radius":r.radius,"center":r.center.to_array(),"axis":r.axis.map(|a| a.to_array())})
}

/// The object the `id` argument named `key` points to; it must have triangles.
fn object_arg<'a>(
    scene: &'a crate::scene::Scene,
    id: &Value,
    key: &str,
) -> Result<&'a crate::scene::Object, String> {
    if id.is_null() {
        return Err(format!(
            "{key} is missing: give an object id from get_scene"
        ));
    }
    let o = id
        .as_u64()
        .ok_or_else(|| format!("{key} must be an object id from get_scene"))?;
    let o = scene
        .get(o)
        .ok_or_else(|| format!("no object with id {id}; get_scene lists the ids"))?;
    require_geometry(o)?;
    Ok(o)
}

/// The error for an object with no triangles, which no tool can work on.
pub fn require_geometry(o: &crate::scene::Object) -> Result<(), String> {
    if o.mesh.is_empty() {
        return Err(format!(
            "object {} has no geometry (0 triangles); delete it with edit delete",
            o.id
        ));
    }
    Ok(())
}

/// The most contours `measure`'s `section` lists (`total` says how many there are).
const MAX_LISTED_CONTOURS: usize = 50;

/// `measure`'s `section`: the contours a plane cuts out of an object, with circle fits.
fn section_json(scene: &crate::scene::Scene, spec: &Value) -> Result<Value, String> {
    use crate::section::{section_tilted, Tilt};
    let o = object_arg(scene, &spec["id"], "section.id")?;
    let normal = match args::normal(spec, "normal").map_err(|e| format!("section.{e}"))? {
        Some(n) => n,
        None => glam::Vec3::Z,
    };
    let Some(normal) = normal.try_normalize() else {
        return Err("section.normal must not be zero: give its direction, e.g. [0, 0, 1]".into());
    };
    let near = spec
        .get("near")
        .filter(|n| !n.is_null())
        .map(|n| point_arg(n, "section.near"))
        .transpose()?;
    let given = spec.get("point").filter(|p| !p.is_null());
    // The plane's point and the point that picks the contour (and, for a plane lying in a
    // flat face, the side with material).
    let (point, hint, horizontal) = match (spec["z"].as_f64(), given, near) {
        (Some(_), Some(_), _) => return Err("section takes z or point, not both".into()),
        (Some(_), None, _) => {
            let z = args::number_in(&spec["z"], "section.z", args::limits::COORD)?;
            (glam::Vec3::new(0.0, 0.0, z), near, true)
        }
        (None, Some(p), _) => {
            let p = point_arg(p, "section.point")?;
            (p, near.or(Some(p)), false)
        }
        (None, None, Some(n)) => (n, Some(n), false),
        (None, None, None) => {
            return Err(
                "section needs near [x, y, z] (the horizontal plane at that height), z (a world height in mm) or point [x, y, z] with an optional normal"
                    .into(),
            )
        }
    };
    if horizontal && normal.dot(glam::Vec3::Z) < 1.0 - 1e-6 {
        return Err(
            "section.z is a horizontal plane; use point with normal for a slanted one".into(),
        );
    }
    // Only a plane the caller did not fix is turned.
    let free = !horizontal && spec.get("normal").is_none_or(Value::is_null);
    let tilt = match (spec["tilt"].as_str(), free) {
        (Some("none"), _) | (None, false) => Tilt::None,
        (Some("auto"), false) => {
            return Err(
                "section.tilt auto needs near, or point without normal: z and normal give exactly the plane asked for"
                    .into(),
            )
        }
        (Some("auto") | None, true) => Tilt::Auto,
        (Some(other), _) => return Err(format!("section.tilt must be auto or none, not {other}")),
    };
    // A normal the caller gave is the plane's own; otherwise the plane is horizontal and
    // may be turned.
    let s = if !horizontal && !free {
        crate::section::section(&o.mesh, &o.matrix(), point, normal, hint)
    } else {
        section_tilted(&o.mesh, &o.matrix(), point, hint, tilt)
    };
    if s.contours.is_empty() {
        let b = o.world_aabb();
        return Err(format!(
            "the plane does not cut object {} (its bounding box is z {:.2} to {:.2} mm); choose a height inside it",
            o.id, b.min.z, b.max.z
        ));
    }
    let list: Vec<Value> = s
        .contours
        .iter()
        .enumerate()
        .take(MAX_LISTED_CONTOURS)
        .map(|(i, c)| {
            let l = &c.outline;
            let role = match (l.closed, c.depth % 2) {
                (false, _) => "open",
                (true, 0) => "outer",
                (true, _) => "inner",
            };
            let mut out = json!({"index":i,"role":role,"depth":c.depth,"closed":l.closed,"vertices":l.points.len(),"area_mm2":c.area,"enclosed_by":c.parent});
            match (l.round(), l.fit) {
                (Some(r), Some(fit)) => {
                    out["circular"] = json!(true);
                    out["center"] = json!(r.center.to_array());
                    out["axis"] = json!(r.axis.map(|a| a.to_array()));
                    out["diameter"] = json!(r.diameter());
                    out["radius"] = json!(r.radius);
                    out["fit_error"] = json!(fit.rms);
                    out["fit_error_mm"] = json!(fit.rms * r.radius);
                }
                (_, fit) => {
                    out["circular"] = json!(false);
                    out["why_not"] = json!(l.why_not());
                    out["best_fit_diameter"] = json!(fit.map(|f| f.round.diameter()));
                }
            }
            out
        })
        .collect();
    let walls: Vec<Value> = s
        .walls()
        .iter()
        .map(|w| json!({"outer":w.outer,"inner":w.inner,"thickness":w.thickness,"center_offset":w.offset}))
        .collect();
    let nearest = near.and_then(|p| s.nearest(p));
    let mut out = json!({"kind":"section","object":o.id,"plane":{"point":s.point.to_array(),"normal":s.normal.to_array(),"tilt_degrees":s.tilt_degrees(),"auto_tilted":s.auto_tilted},"total":s.contours.len(),"contours":list,"walls":walls,"nearest":nearest});
    let mut notes = Vec::new();
    if s.shift != 0.0 {
        out["shifted_mm"] = json!(s.shift);
        notes.push(format!(
            "the plane lies in a flat face of the object (a rim, a base or an end), where it cuts no material; the slice was taken {:.3} mm inside the solid, on the side that has material, and the contours are projected back onto the plane",
            s.shift.abs()
        ));
    }
    if s.auto_tilted {
        notes.push(format!(
            "the horizontal contour there is not round, so the plane was tilted {:.1} degrees from vertical to cut a round one; the diameters are measured in the tilted plane",
            s.tilt_degrees()
        ));
    }
    let moved = s.point.distance(point);
    if moved > 1e-4 {
        notes.push(format!(
            "the plane was taken {moved:.2} mm under the point, below the crest of a rounded rim"
        ));
    }
    if !notes.is_empty() {
        out["note"] = json!(notes.join("; "));
    }
    Ok(out)
}

/// A snapped point: the id of the object it landed on and how.
type Snapped = (u64, crate::snapping::PointSnap);

/// `snapped` in the answer: what each point caught and on which object, null when nothing.
fn snapped_json(hit: Option<Snapped>) -> Value {
    hit.map_or(
        Value::Null,
        |(id, h)| json!({"kind": h.kind.name(), "object": id, "curved_face": h.curved}),
    )
}

/// Computes measurements without modifying the scene.
pub fn measure(scene: &crate::scene::Scene, args: &Value) -> Result<Value, String> {
    use crate::snapping::{align, Alignment};
    let vector = |v: glam::Vec3| v.to_array();
    let flag = |key: &str, default: bool| args[key].as_bool().unwrap_or(default);
    let snap = flag("snap", false);
    let tolerance = args::number(args, "snap_tolerance", 1.0, args::limits::SNAP_TOLERANCE)?;
    // A tolerance of 0 catches nothing, and lines nothing up either.
    let snap = snap && tolerance > 0.0;
    // Each point, snapped to the nearest face feature of an object when asked to.
    let pick = |v: &Value, what: &str| -> Result<(glam::Vec3, Option<Snapped>), String> {
        let p = point_arg(v, what)?;
        let hit = snap
            .then(|| crate::measure::snap_to_scene(scene, p, tolerance))
            .flatten();
        Ok((hit.map_or(p, |h| h.1.point), hit))
    };
    // Null counts as left out, as in `check_args`.
    let given = |key: &str| args.get(key).filter(|v| !v.is_null());
    const MODES: [&str; 6] = ["section", "diameter_of", "circle", "from", "to", "ids"];
    let used: Vec<&str> = MODES.into_iter().filter(|k| given(k).is_some()).collect();
    let pair = used.iter().all(|k| matches!(*k, "from" | "to"));
    if used.len() > 1 && !pair {
        return Err(format!(
            "measure takes one kind of measurement at a time, not {}; call it once for each",
            used.join(" and ")
        ));
    }
    if let Some(spec) = given("section") {
        return section_json(scene, spec);
    }
    if let Some(id) = given("diameter_of") {
        let o = object_arg(scene, id, "diameter_of")?;
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
    if let Some(points) = given("circle") {
        let bad = "circle must be three points [[x, y, z], [x, y, z], [x, y, z]] on the circle, mm";
        let p = points.as_array().filter(|p| p.len() == 3).ok_or(bad)?;
        let picked = [
            pick(&p[0], "circle's first point")?,
            pick(&p[1], "circle's second point")?,
            pick(&p[2], "circle's third point")?,
        ];
        let round = crate::measure::circle_through(picked[0].0, picked[1].0, picked[2].0)
            .ok_or("the three points lie on a line or coincide, so no circle goes through them")?;
        let mut out = round_json(&round);
        if snap {
            out["snapped"] = json!(picked.iter().map(|p| snapped_json(p.1)).collect::<Vec<_>>());
            out["points"] = json!(picked.iter().map(|p| vector(p.0)).collect::<Vec<_>>());
        }
        return Ok(out);
    }
    if given("from").is_some() || given("to").is_some() {
        if given("from").is_none() || given("to").is_none() {
            return Err("from and to must both be given, as points [x, y, z] in mm".into());
        }
        let (a, ha) = pick(&args["from"], "from")?;
        let (mut b, hb) = pick(&args["to"], "to")?;
        let mut alignment = None;
        if snap && flag("align", true) {
            // A facet of a curved wall has no normal to line up along.
            let flat = |h: Option<Snapped>| h.filter(|h| !h.1.curved).map(|h| h.1.normal);
            if let Some((p, how)) = align(a, flat(ha), b, flat(hb)) {
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
            out["snapped"] = json!([snapped_json(ha), snapped_json(hb)]);
            out["alignment"] = json!(alignment);
        }
        return Ok(out);
    }
    if let Some(ids) = given("ids") {
        let ids = ids
            .as_array()
            .filter(|a| a.len() == 2)
            .ok_or("ids must be two object ids from get_scene")?;
        let object = |i: usize| object_arg(scene, &ids[i], "ids");
        let a = object(0)?.world_aabb();
        let b = object(1)?.world_aabb();
        return Ok(
            json!({"gap":vector((a.min-b.max).max(b.min-a.max).max(glam::Vec3::ZERO)),"centre_distance":a.center().distance(b.center())}),
        );
    }
    if scene.selection.is_empty() {
        return Err(format!(
            "{}; or give from and to, ids, diameter_of, circle or section, to measure without a selection",
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
        require_geometry(o)?;
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

    fn rounded_pot() -> (crate::scene::Scene, u64) {
        let mut scene = crate::scene::Scene::default();
        let id = scene.add(crate::scene::Object::new(
            "pot",
            crate::primitives::rounded_pot(),
            crate::scene::DEFAULT_COLOR,
        ));
        (scene, id)
    }

    #[test]
    fn measure_section_gives_outer_and_inner_diameters_and_the_wall_of_a_pot() {
        let (scene, id) = rounded_pot();
        let top = scene.get(id).unwrap().world_aabb().max.z as f64;
        // The section 2 mm under the top: outer and inner contours and the wall.
        let s = measure(
            &scene,
            &json!({"section":{"id":id,"z":top - 2.0,"near":[0,40,top - 2.0]}}),
        )
        .unwrap();
        assert_eq!(s["total"], 2);
        let (outer, inner) = (&s["contours"][0], &s["contours"][1]);
        assert_eq!(
            (outer["role"].as_str(), inner["role"].as_str()),
            (Some("outer"), Some("inner"))
        );
        assert!(
            (outer["diameter"].as_f64().unwrap() - 79.8).abs() < 0.3,
            "{outer}"
        );
        assert!((s["walls"][0]["thickness"].as_f64().unwrap() - 3.0).abs() < 0.1);
        assert_eq!(s["nearest"], 0);
        assert!(s.get("shifted_mm").is_none());
        // A plane through a point with a normal, a plane that misses, and bad arguments.
        let slanted = measure(
            &scene,
            &json!({"section":{"id":id,"point":[0,0,45],"normal":[0,0,1]}}),
        )
        .unwrap();
        assert_eq!(slanted["contours"][0]["circular"], true);
        assert!(slanted["nearest"].is_null());
        let miss = measure(&scene, &json!({"section":{"id":id,"z":500}})).unwrap_err();
        assert!(miss.contains("does not cut"), "{miss}");
        let none = measure(&scene, &json!({"section":{"id":id}})).unwrap_err();
        assert!(none.contains("near") && none.contains("z"), "{none}");
        assert!(measure(&scene, &json!({"section":{"id":id,"z":1,"point":[0,0,1]}})).is_err());
        assert!(measure(&scene, &json!({"section":{"id":id,"z":1,"normal":[1,0,0]}})).is_err());
        assert!(measure(&scene, &json!({"section":{"id":99,"z":1}})).is_err());
        assert!(measure(&scene, &json!({"section":{"id":id,"near":[0,0]}})).is_err());
    }

    #[test]
    fn measure_section_near_alone_cuts_at_the_points_height_and_names_the_contour() {
        use crate::primitives::Primitive;
        let (scene, id) = scene_with(Primitive::Cylinder);
        let b = scene.get(id).unwrap().world_aabb();
        let r = (b.size().x / 2.0) as f64;
        // Half way up, on the side: the plane is the one through that height.
        let mid = json!([b.center().x + r as f32, b.center().y, b.center().z]);
        let s = measure(&scene, &json!({"section":{"id":id,"near":mid}})).unwrap();
        assert_eq!(
            (s["total"].as_u64(), s["nearest"].as_u64()),
            (Some(1), Some(0))
        );
        assert!((s["contours"][0]["diameter"].as_f64().unwrap() - 2.0 * r).abs() < 0.02);
        assert!(s.get("shifted_mm").is_none());
        // On the top rim and the base: the plane lies in a face, so it goes a hair inside.
        for z in [b.max.z, b.min.z] {
            let rim = json!([b.center().x + r as f32, b.center().y, z]);
            let s = measure(&scene, &json!({"section":{"id":id,"near":rim}})).unwrap();
            assert_eq!(s["nearest"], 0);
            assert!((s["contours"][0]["diameter"].as_f64().unwrap() - 2.0 * r).abs() < 0.02);
            assert!((s["contours"][0]["center"][2].as_f64().unwrap() - z as f64).abs() < 1e-3);
            assert!(s["shifted_mm"].as_f64().unwrap().abs() > 0.0);
            assert!(s["note"].as_str().unwrap().contains("inside the solid"));
        }
    }

    #[test]
    fn measure_section_tilts_the_plane_of_a_tilted_cylinder_and_tilt_none_does_not() {
        use crate::primitives::Primitive;
        let (mut scene, id) = scene_with(Primitive::Cylinder);
        let unit = scene.get(id).unwrap().world_aabb().size().x as f64 / 2.0;
        let o = scene.objects.iter_mut().find(|o| o.id == id).unwrap();
        o.rotation.x = 20.0;
        let m = o.matrix();
        // A point on the wall half way up the tilted cylinder.
        let local = o.mesh.aabb();
        let mid = m.transform_point3(glam::Vec3::new(
            local.center().x + unit as f32,
            local.center().y,
            local.center().z,
        ));
        let near = json!([mid.x, mid.y, mid.z]);
        let s = measure(&scene, &json!({"section":{"id":id,"near":near}})).unwrap();
        assert_eq!(s["plane"]["auto_tilted"], true);
        assert!((s["plane"]["tilt_degrees"].as_f64().unwrap() - 20.0).abs() < 0.3);
        assert_eq!(s["contours"][0]["circular"], true);
        assert!((s["contours"][0]["diameter"].as_f64().unwrap() - 2.0 * unit).abs() < 0.05);
        assert!(s["note"].as_str().unwrap().contains("tilted"));
        // Kept horizontal, or with a plane of its own, the contour is an ellipse.
        for spec in [
            json!({"id":id,"near":near,"tilt":"none"}),
            json!({"id":id,"point":near,"normal":[0,0,1]}),
            json!({"id":id,"z":mid.z}),
        ] {
            let s = measure(&scene, &json!({ "section": spec })).unwrap();
            assert_eq!(s["plane"]["auto_tilted"], false);
            assert_eq!(s["contours"][0]["circular"], false);
        }
        let bad = measure(
            &scene,
            &json!({"section":{"id":id,"point":near,"normal":[0,0,1],"tilt":"auto"}}),
        )
        .unwrap_err();
        assert!(bad.contains("tilt"), "{bad}");
        assert!(check_args(
            "measure",
            &json!({"section":{"id":1,"near":[0,0,0],"tilt":"sideways"}})
        )
        .is_err());
    }

    #[test]
    fn measure_section_arguments_are_checked_against_the_schema() {
        assert!(check_args("measure", &json!({"section":{"id":1,"near":[0,0,0]}})).is_ok());
        assert!(check_args("measure", &json!({"section":{"id":1,"z":5}})).is_ok());
        assert!(check_args("measure", &json!({"section":{"id":1,"height":5}})).is_err());
        assert!(check_args("measure", &json!({"section":{"id":1,"near":[0,0]}})).is_err());
        // The edge-loop arguments are gone.
        let err = check_args("measure", &json!({"edge_loop":{"id":1,"near":[0,0,0]}})).unwrap_err();
        assert!(
            err.contains("unknown argument") && err.contains("section"),
            "{err}"
        );
        assert!(check_args("measure", &json!({"soft":true})).is_err());
    }

    #[test]
    fn measure_diameter_of_objects_and_circles() {
        use crate::primitives::Primitive;
        let (scene, id) = scene_with(Primitive::Cylinder);
        let r = measure(&scene, &json!({"diameter_of":id})).unwrap();
        assert!(check_args("measure", &json!({"radius_of":id})).is_err());
        assert_eq!(r["kind"], "cylinder");
        let radius = r["radius"].as_f64().unwrap();
        assert!((r["diameter"].as_f64().unwrap() - 2.0 * radius).abs() < 1e-3);
        assert_eq!(
            measure(&scene_with(Primitive::Sphere).0, &json!({"diameter_of":1})).unwrap()["kind"],
            "sphere"
        );
        let (cube, id) = scene_with(Primitive::Cube);
        let err = measure(&cube, &json!({"diameter_of":id})).unwrap_err();
        assert!(err.contains("not round"), "{err}");
        assert!(measure(&cube, &json!({"diameter_of":99}))
            .unwrap_err()
            .contains("no object"));
        let c = measure(&cube, &json!({"circle":[[5,0,0],[0,5,0],[-5,0,0]]})).unwrap();
        assert!((c["radius"].as_f64().unwrap() - 5.0).abs() < 1e-4);
        assert!(measure(&cube, &json!({"circle":[[0,0,0],[1,0,0],[2,0,0]]})).is_err());
        assert!(measure(&cube, &json!({"circle":[[0,0,0],[1,0,0]]})).is_err());
    }

    #[test]
    fn measure_rejects_conflicting_or_malformed_arguments_with_clear_messages() {
        let (scene, id) = scene_with(crate::primitives::Primitive::Cylinder);
        let err = |args: Value| measure(&scene, &args).unwrap_err();
        let both = err(json!({"from":[0,0,0],"circle":[[0,0,0],[1,0,0],[0,1,0]]}));
        assert!(both.contains("one kind") && both.contains("from"), "{both}");
        assert!(err(json!({"diameter_of":id,"section":{"id":id,"z":1}})).contains("one kind"));
        // Each point is named in its own error, and a lone from or to is explained.
        let point = err(json!({"from":[0,0,0],"to":[1,2]}));
        assert!(point.starts_with("to must be a point"), "{point}");
        assert!(err(json!({"from":[0,0,0]})).contains("both"));
        let circle = err(json!({"circle":[[0,0,0],[1,0],[0,1,0]]}));
        assert!(
            circle.starts_with("circle's second point must be a point"),
            "{circle}"
        );
        // Null is left out; a scaled-up vertical normal is still horizontal.
        assert!(measure(&scene, &json!({"section":null,"diameter_of":id})).is_ok());
        let up = json!({"section":{"id":id,"z":1,"normal":[0,0,2]}});
        assert!(measure(&scene, &up).is_ok());
        let tilted = json!({"section":{"id":id,"z":1,"normal":[0,0.1,1]}});
        assert!(measure(&scene, &tilted).is_err());
        let zero = err(json!({"section":{"id":id,"point":[0,0,1],"normal":[0,0,0]}}));
        assert!(zero.contains("must not be zero"), "{zero}");
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
        assert_eq!(r["snapped"][0]["kind"], "Vertex");
        assert_eq!(r["snapped"][0]["object"], 1);
        assert_eq!(r["snapped"][0]["curved_face"], false);
        assert_eq!(r["alignment"], "perpendicular");
        let height = (hi.z - lo.z) as f64;
        assert!((r["distance"].as_f64().unwrap() - height).abs() < 1e-3);
        let off = measure(&scene, &json!({"from":[hi.x - 0.2, hi.y - 0.1, hi.z],"to":[lo.x + 3.0, lo.y + 2.0, lo.z],"snap":true,"align":false})).unwrap();
        assert!(off["alignment"].is_null() && off["distance"].as_f64().unwrap() > height + 1.0);
        assert!(measure(
            &scene,
            &json!({"from":[0,0,0],"to":[1,1,1],"snap":true,"snap_tolerance":-1})
        )
        .unwrap_err()
        .contains("between 0 and 1000 mm"));
        let circle = measure(
            &scene,
            &json!({"circle":[[hi.x,hi.y,hi.z],[lo.x,hi.y,hi.z],[lo.x,lo.y,hi.z]],"snap":true}),
        )
        .unwrap();
        assert_eq!(circle["snapped"][2]["kind"], "Vertex");
        // A tolerance of 0 is no snapping at all, even for a point on the surface.
        let none = measure(
            &scene,
            &json!({"from":[hi.x, hi.y, hi.z],"to":[lo.x, lo.y, lo.z],"snap":true,"snap_tolerance":0}),
        )
        .unwrap();
        assert!(none.get("snapped").is_none() && none.get("alignment").is_none());
    }

    #[test]
    fn measure_snap_on_a_polygonal_cylinder_does_not_line_up_along_a_facet() {
        let (scene, id) = scene_with(crate::primitives::Primitive::Cylinder);
        let b = scene.get(id).unwrap().world_aabb();
        let c = b.center();
        let args = json!({"from":[b.min.x, c.y, c.z],"to":[b.max.x, c.y, c.z],"snap":true});
        let r = measure(&scene, &args).unwrap();
        assert_eq!(r["snapped"][0]["curved_face"], true);
        assert_eq!(r["snapped"][0]["object"], id);
        // Not moved onto the perpendicular of a facet: still along X, the diameter.
        assert!(
            r["alignment"] == "along X" || r["alignment"].is_null(),
            "{r}"
        );
        let d = r["distance"].as_f64().unwrap();
        assert!((d - (b.max.x - b.min.x) as f64).abs() < 0.2, "{d}");
    }

    #[test]
    fn measure_section_cuts_the_slanted_plane_that_was_asked_for() {
        let (scene, id) = scene_with(crate::primitives::Primitive::Cylinder);
        let b = scene.get(id).unwrap().world_aabb();
        let c = b.center();
        // A vertical plane through the axis cuts the cylinder in a rectangle, not a circle.
        let s = measure(
            &scene,
            &json!({"section":{"id":id,"point":[c.x,c.y,c.z],"normal":[1,0,0]}}),
        )
        .unwrap();
        assert!((s["plane"]["tilt_degrees"].as_f64().unwrap() - 90.0).abs() < 1e-3);
        assert_eq!(s["contours"][0]["circular"], false);
    }

    /// Every description in a schema: (path, text) for the schema's own properties and
    /// those nested in objects and in the items of lists.
    fn property_descriptions(path: &str, schema: &Value, out: &mut Vec<(String, Option<String>)>) {
        for (key, prop) in schema["properties"].as_object().into_iter().flatten() {
            let here = format!("{path}.{key}");
            out.push((
                here.clone(),
                prop["description"].as_str().map(str::to_owned),
            ));
            property_descriptions(&here, prop, out);
            property_descriptions(&here, &prop["items"], out);
        }
    }

    #[test]
    fn every_tool_and_argument_has_a_description_of_a_sane_length() {
        const TOOL_MAX: usize = 1500;
        const PROPERTY_MAX: usize = 900;
        let list = tools();
        assert!(list.len() >= 20);
        for t in &list {
            let name = t["name"].as_str().unwrap();
            let d = t["description"].as_str().unwrap_or("");
            assert!(!d.trim().is_empty(), "{name} has no description");
            assert!(
                d.chars().count() <= TOOL_MAX,
                "{name}'s description has {} characters, over {TOOL_MAX}; move details to its arguments",
                d.chars().count()
            );
            let mut props = Vec::new();
            property_descriptions(name, &t["inputSchema"], &mut props);
            for (path, d) in props {
                let d = d.unwrap_or_default();
                assert!(!d.trim().is_empty(), "{path} has no description");
                assert!(
                    d.chars().count() <= PROPERTY_MAX,
                    "{path}'s description has {} characters, over {PROPERTY_MAX}",
                    d.chars().count()
                );
            }
        }
    }

    #[test]
    fn check_args_accepts_exactly_the_properties_of_the_schema() {
        for t in tools() {
            let name = t["name"].as_str().unwrap();
            let props: Vec<&str> = t["inputSchema"]["properties"]
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect();
            // Each property of the schema is accepted (null counts as left out)...
            for p in &props {
                assert!(
                    check_args(
                        name,
                        &Value::Object([(p.to_string(), Value::Null)].into_iter().collect())
                    )
                    .is_ok(),
                    "{name} rejects its own argument {p}"
                );
            }
            // ... and the error for an unknown one lists them all, and only them.
            let err = check_args(name, &json!({"no_such_argument": 1})).unwrap_err();
            let listed = err.split(" takes ").nth(1).unwrap_or("");
            if props.is_empty() {
                assert!(err.contains("takes no arguments"), "{err}");
            }
            let named: Vec<&str> = listed.split(", ").filter(|s| !s.is_empty()).collect();
            if !props.is_empty() {
                assert_eq!(named, props, "{name}: {err}");
            }
            // Every required argument is a property.
            for r in t["inputSchema"]["required"]
                .as_array()
                .into_iter()
                .flatten()
            {
                assert!(props.contains(&r.as_str().unwrap()), "{name} requires {r}");
            }
        }
        assert!(check_args("measure", &json!({"radius_of": 1})).is_err());
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
