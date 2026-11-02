//! A Model Context Protocol server, so an AI agent can use the app's tools: JSON-RPC 2.0
//! over stdio, one message per line. The tools themselves run in the app; this module
//! only speaks the protocol and describes them.

use serde_json::{json, Value};
use std::io::{BufRead, Write};

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
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => handle(&msg, &mut call),
            Err(e) => Some(error(Value::Null, -32700, &format!("parse error: {e}"))),
        };
        if let Some(r) = reply {
            writeln!(output, "{r}")?;
            output.flush()?;
        }
    }
    Ok(())
}

fn handle(msg: &Value, call: &mut impl FnMut(&str, Value) -> Reply) -> Option<Value> {
    // Notifications (no id) get no answer.
    let id = msg.get("id")?.clone();
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
    "Open 3D BuildCraft and enable external agents to use the tools. Every change shows in the app and can be undone. ",
    "Call screenshot to see the result.\n",
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

/// The tools, with their JSON schemas.
pub fn tools() -> Vec<Value> {
    let vec3 = |what: &str| json!({ "type": "array", "items": { "type": "number" }, "minItems": 3, "maxItems": 3, "description": what });
    let none = json!({ "type": "object", "properties": {} });
    let tool = |name: &str, description: &str, schema: Value| json!({ "name": name, "description": description, "inputSchema": schema });
    vec![
        tool("measure", "Read-only measurements in mm, mm² and mm³. Selection statistics, point distance, or gaps between two object bounds.", json!({"type":"object","properties":{"from":vec3("Start"),"to":vec3("End"),"ids":{"type":"array","items":{"type":"integer"},"minItems":2,"maxItems":2}}})),
        tool(
            "get_scene",
            "List the objects: id, name, colour, centre, center_of_mass (volume centroid, or bounding box centre for near-zero volume), size, bottom/top (min/max Z), left/right (min/max X), front/back (min/max Y), rotation, triangle count, whether the mesh is valid (closed) and whether it is selected; plus the chosen slicer.",
            none.clone(),
        ),
        tool(
            "select",
            "Select objects by id; an empty list selects nothing.",
            json!({ "type": "object", "properties": { "ids": { "type": "array", "items": { "type": "integer" } } }, "required": ["ids"] }),
        ),
        tool(
            "transform",
            "Move, rotate or resize the selection. position is the centre of its bounding box; bottom sets min Z, left/right set min/max X, front/back set min/max Y, after position (left then right, front then back). To stack on an object, bottom = that object's top. rotation is degrees about X, Y, Z (each selected object), size is the bounding box in mm (the bottom stays where it was unless position or bottom is given).",
            json!({ "type": "object", "properties": { "position": vec3("centre, mm"), "rotation": vec3("degrees"), "size": vec3("mm, all > 0"), "bottom": { "type": "number", "description": "height of the underside, mm; overrides position's Z (0 = on the floor)" }, "left": { "type": "number", "description": "minimum X, mm; overrides position's X" }, "right": { "type": "number", "description": "maximum X, mm; applied after left" }, "front": { "type": "number", "description": "minimum Y, mm; overrides position's Y" }, "back": { "type": "number", "description": "maximum Y, mm; applied after front" } } }),
        ),
        tool(
            "edit",
            "Apply an operation to the selection. merge/intersect need two or more objects. subtract with one object cuts it out of every object it overlaps and removes it; with more, it takes the others away from the one with the lowest id. Pieces a subtraction leaves apart become separate objects. settle drops objects onto the floor. repair fixes invalid meshes. undo and redo step through history.",
            json!({ "type": "object", "properties": { "operation": { "type": "string", "enum": ["delete", "duplicate", "merge", "subtract", "intersect", "repair", "smooth", "extrude_down", "settle", "mirror_x", "mirror_y", "mirror_z", "undo", "redo"] } }, "required": ["operation"] }),
        ),
        tool(
            "set_color",
            "Colour the selection.",
            json!({ "type": "object", "properties": { "color": { "type": "string", "description": "#RRGGBB" } }, "required": ["color"] }),
        ),
        tool(
            "hollow",
            "Hollow the selection, keeping walls of the given thickness.",
            json!({ "type": "object", "properties": { "wall": { "type": "number", "description": "mm, > 0" } }, "required": ["wall"] }),
        ),
        tool(
            "simplify",
            "Reduce the faces of the selection.",
            json!({ "type": "object", "properties": { "remove": { "type": "number", "description": "fraction of faces to remove, 0..1" } }, "required": ["remove"] }),
        ),
        tool(
            "open",
            "Open a model file (3MF, STL, OBJ, PLY, glTF/GLB), replacing the scene. Fails if the scene has unsaved changes unless discard_changes is true.",
            json!({ "type": "object", "properties": { "path": { "type": "string" }, "discard_changes": { "type": "boolean" } }, "required": ["path"] }),
        ),
        tool(
            "save",
            "Save the scene; the format follows the extension (3MF by default). Fails if the file exists unless overwrite is true.",
            json!({ "type": "object", "properties": { "path": { "type": "string" }, "overwrite": { "type": "boolean" } }, "required": ["path"] }),
        ),
        tool(
            "print",
            "Only when the user explicitly asks to print: open the scene in the user's chosen slicer, where it is sliced and sent to the printer. The file carries the printer profile, print profile and filaments chosen with print_setup.",
            none.clone(),
        ),
        tool("screenshot", "A picture of the app window as it is now.", none),
    ]
    .into_iter()
    .chain(model_tools())
    .chain(print_tools())
    .collect()
}

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
    let textures: Vec<&str> = textures.iter().map(String::as_str).collect();
    let point = "a world point in mm on (or within 5 mm of) the surface; the nearest triangle is used. Points from get_scene bounds work, e.g. [center x, center y, top] is the middle of a flat top";
    vec![
        tool(
            "insert_shape",
            "Add a shape on the floor at the centre; it becomes the selection. Without size it fits a 40 mm box. custom builds it as the Custom tool does (cube, sphere, cone, torus or cylinder only) with its own subdivisions and roundness. One undo step.",
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
            "Cut the selection with a plane, as the Split tool. With only z the plane is horizontal at that height. For a slanted plane give point (or z, which replaces point's Z; the default point is the selection's centre) and either normal or rotation. top is the side the normal points to (+Z when level). The kept pieces stay in place and become the selection.",
            json!({ "type": "object", "properties": {
                "z": num("height of a horizontal plane, mm"),
                "point": vec3("a point the plane passes through, mm"),
                "normal": vec3("direction the plane faces (its top side), any length > 0"),
                "rotation": vec3("the Split tool's plane angles in degrees about X, Y, Z (as an object's rotation); [0, 0, 0] is level"),
                "keep": one_of(&["top", "bottom", "both"], "default top"),
            } }),
        ),
        tool(
            "emboss_text",
            "Emboss (or engrave) a pattern onto the one selected object, as the Emboss tool: text, a star, heart or circle, or the dark areas of a picture. planar projects straight down onto the top, centred unless offset or at moves it; cylindrical and spherical wrap it around the object's vertical axis or centre on its front (-Y). A top_color or side_color different from the object's makes that part a separate object; all pieces are selected afterwards.",
            json!({ "type": "object", "properties": {
                "pattern": one_of(&["text", "star", "heart", "circle", "image"], "default text"),
                "text": text("the text, for pattern text"),
                "image": text("PNG or JPEG path, for pattern image"),
                "font": text("installed font family; default the bundled font"),
                "size": num("capital letter height for text, overall height for shapes and images, mm, default 10"),
                "depth": num("height above the surface (or depth when engraving), mm, default 1"),
                "engrave": flag("cut into the surface instead of raising"),
                "bevel": flag("slope the edges"),
                "angle": num("turn in its own plane, degrees"),
                "projection": one_of(&["planar", "cylindrical", "spherical"], "default planar"),
                "offset": { "type": "array", "items": { "type": "number" }, "minItems": 2, "maxItems": 2, "description": "[x, y] mm from the centre of the top (planar) or along the surface from the front (cylindrical, spherical)" },
                "at": vec3("a world point on the surface to centre the pattern on, mm; overrides offset"),
                "top_color": color("colour of the top layer"),
                "side_color": color("colour of the sides of a raised pattern"),
            } }),
        ),
        tool(
            "paint_area",
            "Paint the surface region around a point, as the Color tool's click. coverage 0 paints the flat face there; higher values spread across edges that bend up to coverage × 180 degrees, so 1 paints the whole connected surface. Does not change the selection. One undo step.",
            json!({ "type": "object", "properties": {
                "point": vec3(point),
                "color": color("the paint"),
                "coverage": num("0..1, default 0"),
                "id": int("object to paint; default the object nearest to point"),
            }, "required": ["point", "color"] }),
        ),
        tool(
            "set_material",
            "Give the selection a material, as Paint → Material: none (plain), simple (matte), realistic (glossy) or translucent (see-through), with a colour that replaces any painted areas. One undo step.",
            json!({ "type": "object", "properties": {
                "type": one_of(&["none", "simple", "realistic", "translucent"], "the material"),
                "color": color("default the first selected object's colour; not used with none"),
            }, "required": ["type"] }),
        ),
        tool(
            "apply_texture",
            "Put a built-in texture or a picture on the selection, as Paint → Texture. brush tiles it with box mapping: over every selected object when no point is given, else over the region around point (coverage as in paint_area). sticker projects it once at point, facing out of the surface. One undo step.",
            json!({ "type": "object", "properties": {
                "texture": one_of(&textures, "a built-in texture (the last six are white stencil shapes)"),
                "image": text("PNG or JPEG path, instead of texture"),
                "mode": one_of(&["brush", "sticker"], "default brush"),
                "point": vec3(point),
                "coverage": num("brush with point: 0..1, default 1 (the whole connected surface)"),
                "id": int("a selected object to use; default the selected object nearest to point"),
                "size": num("sticker width, mm; default 40% of the object's longest side"),
                "angle": num("sticker turn about the surface normal, degrees"),
            } }),
        ),
        tool(
            "import",
            "Add a model file (3MF, STL, OBJ, PLY, glTF/GLB) to the scene without replacing it, as Add → Import model. It lands centred on the floor and its objects become the selection. One undo step.",
            json!({ "type": "object", "properties": {
                "path": text("model file path"),
                "units": one_of(&["micron", "mm", "cm", "inch", "foot", "m"], "what one unit of an STL, OBJ or PLY file is, default mm; 3MF and glTF carry their own"),
            }, "required": ["path"] }),
        ),
        tool(
            "load_image",
            "Turn a picture into a solid, as Load image, added on the floor at the centre and selected. contour: brightness in flat steps; height_map: smooth relief; edge: the dark areas' outline extruded; stamp: a mirrored two-level stamp. Dark is high unless invert. One undo step.",
            json!({ "type": "object", "properties": {
                "path": text("PNG or JPEG path"),
                "method": one_of(&["contour", "height_map", "edge", "stamp"], "default contour"),
                "size": num("mm along the longer side (the height for edge), default 80"),
                "depth": num("most relief, mm, default 3"),
                "levels": int("contour steps, 2..8, default 8"),
                "smooth": num("blur before sampling, 0..1, default 1"),
                "invert": flag("light is high instead of dark"),
                "textures": flag("colour the top with the picture, default true"),
            }, "required": ["path"] }),
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
            "Read or set the model's metadata (\"About the model\"), saved in 3MF files and sent to the slicer. With no arguments it only reads. Given fields replace those fields; an empty string clears one. The title also names the window and the print file.",
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

/// The app's MCP server on a local port, so agents started elsewhere (or by the in-app
/// chat) work on the window that is already open. Stops when dropped.
pub struct Listener {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl Listener {
    /// Listens on 127.0.0.1; a client must send the token from the endpoint file (readable
    /// by this user only) as its first line.
    pub fn start(
        call: impl Fn(&str, Value) -> Reply + Clone + Send + 'static,
    ) -> anyhow::Result<Self> {
        use std::sync::atomic::{AtomicBool, Ordering};
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let token = uuid::Uuid::new_v4().to_string();
        let file = endpoint_file().ok_or_else(|| anyhow::anyhow!("no data folder"))?;
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let port = listener.local_addr()?.port();
        options.open(&file)?.write_all(
            json!({ "port": port, "token": token })
                .to_string()
                .as_bytes(),
        )?;
        let stop = std::sync::Arc::new(AtomicBool::new(false));
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
                let (call, token) = (call.clone(), token.clone());
                std::thread::spawn(move || {
                    let _ = stream.set_nonblocking(false);
                    let Ok(out) = stream.try_clone() else { return };
                    let mut input = std::io::BufReader::new(stream);
                    let mut first = String::new();
                    if input.read_line(&mut first).is_err() || first.trim() != token {
                        return;
                    }
                    if let Err(e) = serve(input, out, call) {
                        log::info!("MCP client left: {e}");
                    }
                });
            }
            if let Some(f) = endpoint_file() {
                let _ = std::fs::remove_file(f);
            }
        });
        Ok(Self { stop })
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
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

/// Computes measurements without modifying the scene.
pub fn measure(scene: &crate::scene::Scene, args: &Value) -> Result<Value, String> {
    let vector = |v: glam::Vec3| v.to_array();
    if args.get("from").is_some() || args.get("to").is_some() {
        let point = |key: &str| -> Result<glam::Vec3, String> {
            let a = args[key]
                .as_array()
                .filter(|a| a.len() == 3)
                .ok_or("points need three numbers")?;
            let mut p = [0.0; 3];
            for i in 0..3 {
                p[i] = a[i].as_f64().ok_or("points need numbers")? as f32;
            }
            Ok(glam::Vec3::from_array(p))
        };
        let d = point("to")? - point("from")?;
        return Ok(json!({"distance":d.length(),"differences":vector(d)}));
    }
    if let Some(ids) = args.get("ids") {
        let ids = ids
            .as_array()
            .filter(|a| a.len() == 2)
            .ok_or("ids needs two object IDs")?;
        let object = |i: usize| {
            scene
                .get(ids[i].as_u64().unwrap_or(u64::MAX))
                .ok_or_else(|| "unknown object ID".to_owned())
        };
        let a = object(0)?.world_aabb();
        let b = object(1)?.world_aabb();
        return Ok(
            json!({"gap":vector((a.min-b.max).max(b.min-a.max).max(glam::Vec3::ZERO)),"centre_distance":a.center().distance(b.center())}),
        );
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
