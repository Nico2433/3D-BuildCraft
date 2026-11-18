//! Reading the arguments of the tools: every number has a named range and an error that
//! says it, so no value an agent sends (a size of 100000 mm, 1e9, a very long text) can make
//! the app work for minutes or run out of memory; the app runs a tool on its UI thread, so
//! the ranges are what bound it. The tool descriptions in `crate::mcp` print the same
//! constants. The parsers of the heavier tools live here, away from the UI, so the tests
//! can drive them with extreme values.

use crate::cut::{CutPlane, Keep};
use crate::emboss::{Emboss, Pattern, Projection, Shape};
use crate::io::{parse_hex, Rgba};
use glam::{Vec2, Vec3};
use serde_json::Value;
use std::fmt;
use std::path::PathBuf;

/// A range of numbers a tool accepts.
#[derive(Clone, Copy, Debug)]
pub struct Limit {
    pub min: f32,
    pub max: f32,
    /// "mm", "degrees" or "" for a plain number.
    pub unit: &'static str,
}

impl Limit {
    pub const fn new(min: f32, max: f32, unit: &'static str) -> Self {
        Self { min, max, unit }
    }

    pub fn contains(&self, x: f64) -> bool {
        // Compared as f32, the type of the values, so 0.1 is within a limit of 0.1.
        let x = x as f32;
        x.is_finite() && (self.min..=self.max).contains(&x)
    }
}

/// "between 1 and 200 mm", for errors and descriptions.
impl fmt::Display for Limit {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "between {} and {}", self.min, self.max)?;
        match self.unit {
            "" => Ok(()),
            unit => write!(f, " {unit}"),
        }
    }
}

/// The ranges, in the units the tools use.
pub mod limits {
    use super::Limit;
    use crate::emboss::{DEPTHS, SIZES, STRETCHES};

    /// A world position or point.
    pub const COORD: Limit = Limit::new(-100_000.0, 100_000.0, "mm");
    /// The size of an object along one axis.
    pub const OBJECT_SIZE: Limit = Limit::new(0.01, crate::io::MAX_SIZE_MM, "mm");
    /// Rotation angles and turns of a pattern or sticker.
    pub const ANGLE: Limit = Limit::new(-3600.0, 3600.0, "degrees");
    /// A turn of the pattern in its own plane.
    pub const TURN: Limit = Limit::new(-360.0, 360.0, "degrees");
    /// `emboss_text`: the height of the pattern, its width and its depth.
    pub const EMBOSS_SIZE: Limit = Limit::new(SIZES.0, SIZES.1, "mm");
    pub const EMBOSS_DEPTH: Limit = Limit::new(DEPTHS.0, DEPTHS.1, "mm");
    pub const EMBOSS_WIDTH: Limit = Limit::new(STRETCHES.0 * SIZES.0, STRETCHES.1 * SIZES.1, "mm");
    /// The characters of an embossed text.
    pub const TEXT_CHARS: usize = 60;
    /// `load_image`: the longer side and the relief.
    pub const IMAGE_SIZE: Limit = Limit::new(1.0, 500.0, "mm");
    pub const IMAGE_DEPTH: Limit = Limit::new(0.1, 100.0, "mm");
    pub const IMAGE_LEVELS: Limit = Limit::new(2.0, 8.0, "");
    /// A fraction of the way from none to all.
    pub const FRACTION: Limit = Limit::new(0.0, 1.0, "");
    /// `extrude_outline`: the border's width and height.
    pub const BORDER_WIDTH: Limit = Limit::new(0.1, 100.0, "mm");
    pub const BORDER_HEIGHT: Limit = Limit::new(0.1, 1000.0, "mm");
    /// `hollow`: the wall.
    pub const WALL: Limit = Limit::new(0.05, 1000.0, "mm");
    /// `insert_shape`'s custom shapes: subdivisions, width and height, roundness.
    pub const CUSTOM_STEPS: Limit = Limit::new(3.0, 48.0, "");
    pub const CUSTOM_SIZE: Limit = Limit::new(1.0, 1000.0, "mm");
    pub const ROUNDNESS: Limit = Limit::new(0.0, 10.0, "");
    /// `split`: the finite plane's width and length (0 is the whole object).
    pub const PLANE_EXTENT: Limit = Limit::new(0.0, 10_000.0, "mm");
    /// The reach of a paint or texture brush, and the width of a sticker.
    pub const BRUSH_RADIUS: Limit = Limit::new(0.01, 10_000.0, "mm");
    pub const STICKER_SIZE: Limit = Limit::new(0.1, 10_000.0, "mm");
    /// `measure`'s snap tolerance.
    pub const SNAP_TOLERANCE: Limit = Limit::new(0.0, 1000.0, "mm");
    /// `list_fonts`'s `limit`.
    pub const LIST_LIMIT: Limit = Limit::new(1.0, 10_000.0, "");
    /// The most characters of a metadata field or a name given to a tool.
    pub const INFO_CHARS: usize = 10_000;
    /// The most items of a list of ids or of filament choices.
    pub const LIST_ITEMS: usize = 1000;
    /// The most objects a scene may hold when a tool adds one.
    pub const MAX_OBJECTS: usize = 1000;
    /// The largest file a path may name, bytes (inline files have their own, smaller cap).
    pub const MAX_FILE_BYTES: u64 = 256 << 20;
}

use limits::*;

/// `v` as a number in `limit`; the error names `key` and the range.
pub fn number_in(v: &Value, key: &str, limit: Limit) -> Result<f32, String> {
    v.as_f64()
        .filter(|x| limit.contains(*x))
        .map(|x| x as f32)
        .ok_or_else(|| format!("{key} must be a number {limit}"))
}

/// A number in `limit`, or `default` when it is missing.
pub fn number(a: &Value, key: &str, default: f32, limit: Limit) -> Result<f32, String> {
    match &a[key] {
        Value::Null => Ok(default),
        v => number_in(v, key, limit),
    }
}

/// A number in `limit` when it is given.
pub fn optional(a: &Value, key: &str, limit: Limit) -> Result<Option<f32>, String> {
    match &a[key] {
        Value::Null => Ok(None),
        v => number_in(v, key, limit).map(Some),
    }
}

/// A whole number in `limit`, or `default` when it is missing.
pub fn integer(a: &Value, key: &str, default: u32, limit: Limit) -> Result<u32, String> {
    match &a[key] {
        Value::Null => Ok(default),
        v => v
            .as_u64()
            .filter(|x| limit.contains(*x as f64))
            .and_then(|x| u32::try_from(x).ok())
            .ok_or_else(|| format!("{key} must be a whole number {limit}")),
    }
}

/// A true or false, or `default` when it is missing.
pub fn flag(a: &Value, key: &str, default: bool) -> Result<bool, String> {
    match &a[key] {
        Value::Null => Ok(default),
        v => v
            .as_bool()
            .ok_or_else(|| format!("{key} must be true or false")),
    }
}

/// A string of at most `max` characters, when it is given.
pub fn text(a: &Value, key: &str, max: usize) -> Result<Option<String>, String> {
    match &a[key] {
        Value::Null => Ok(None),
        Value::String(s) if s.chars().count() <= max => Ok(Some(s.clone())),
        Value::String(s) => Err(format!(
            "{key} has {} characters; at most {max} are accepted",
            s.chars().count()
        )),
        _ => Err(format!("{key} must be a string")),
    }
}

/// One of `options` by name; `default` when missing, or an error when there is none.
pub fn choice<T: Copy>(
    a: &Value,
    key: &str,
    default: Option<T>,
    options: &[(&str, T)],
) -> Result<T, String> {
    let names = || {
        options
            .iter()
            .map(|(n, _)| *n)
            .collect::<Vec<_>>()
            .join(", ")
    };
    match &a[key] {
        Value::Null => default.ok_or_else(|| format!("{key} is missing: one of {}", names())),
        v => {
            let name = v.as_str().unwrap_or("");
            options
                .iter()
                .find(|(n, _)| n.eq_ignore_ascii_case(name))
                .map(|(_, t)| *t)
                .ok_or_else(|| format!("unknown {key}: {name}; one of {}", names()))
        }
    }
}

/// An optional `#RRGGBB` colour.
pub fn color(a: &Value, key: &str) -> Result<Option<Rgba>, String> {
    match &a[key] {
        Value::Null => Ok(None),
        v => v
            .as_str()
            .and_then(parse_hex)
            .map(Some)
            .ok_or_else(|| format!("{key} must be #RRGGBB, e.g. #FF8000")),
    }
}

/// The optional `[x, y, z]` under `key`, each in `limit`.
pub fn vec3(a: &Value, key: &str, limit: Limit) -> Result<Option<Vec3>, String> {
    let v = &a[key];
    if v.is_null() {
        return Ok(None);
    }
    let xs: Option<Vec<f64>> = v
        .as_array()
        .filter(|a| a.len() == 3)
        .and_then(|a| a.iter().map(Value::as_f64).collect());
    match xs.as_deref() {
        Some(&[x, y, z]) if [x, y, z].iter().all(|c| limit.contains(*c)) => {
            Ok(Some(Vec3::new(x as f32, y as f32, z as f32)))
        }
        _ => Err(format!(
            "{key} must be [x, y, z], three numbers each {limit}"
        )),
    }
}

/// The optional `[x, y]` under `key`, each in `limit`.
fn vec2(a: &Value, key: &str, limit: Limit) -> Result<Option<Vec2>, String> {
    let v = &a[key];
    if v.is_null() {
        return Ok(None);
    }
    let xs: Option<Vec<f64>> = v
        .as_array()
        .filter(|a| a.len() == 2)
        .and_then(|a| a.iter().map(Value::as_f64).collect());
    match xs.as_deref() {
        Some(&[x, y]) if [x, y].iter().all(|c| limit.contains(*c)) => {
            Ok(Some(Vec2::new(x as f32, y as f32)))
        }
        _ => Err(format!("{key} must be [x, y], two numbers each {limit}")),
    }
}

/// An optional list of object ids.
pub fn ids(v: &Value, key: &str) -> Result<Vec<u64>, String> {
    let bad = || format!("{key} must be a list of object ids from get_scene, e.g. [1, 2]");
    let items = v.as_array().ok_or_else(bad)?;
    if items.len() > LIST_ITEMS {
        return Err(format!(
            "{key} has {} ids; at most {LIST_ITEMS} are accepted",
            items.len()
        ));
    }
    items.iter().map(|i| i.as_u64().ok_or_else(bad)).collect()
}

/// A path to an existing file under `key`; tools that take one also take it inline.
fn file(a: &Value, key: &str) -> Result<PathBuf, String> {
    let inline = "or send the file itself as file_name with file_text or file_base64 (the app may run in a sandbox that does not share the agent's disk)";
    let path = PathBuf::from(
        a[key]
            .as_str()
            .ok_or_else(|| format!("{key} is missing: give the file's path, {inline}"))?,
    );
    if !path.is_file() {
        return Err(format!(
            "no such file on the app's computer: {}; check the path, {inline}",
            path.display()
        ));
    }
    let bytes = std::fs::metadata(&path).map_or(0, |m| m.len());
    if bytes > MAX_FILE_BYTES {
        return Err(format!(
            "{} is {} MB; files over {} MB are not accepted",
            path.display(),
            bytes >> 20,
            MAX_FILE_BYTES >> 20
        ));
    }
    Ok(path)
}

/// The model file `path` for open and import; a picture gets pointed to the picture tools.
pub fn model_file(a: &Value) -> Result<PathBuf, String> {
    use crate::io::{self, Format};
    let path = file(a, "path")?;
    if Format::from_path(&path).is_some() {
        return Ok(path);
    }
    let picture = "load_image makes a solid of a picture, emboss_text (pattern image) or apply_texture puts it on an object";
    if let Some(kind) = crate::picture::unsupported(&path) {
        return Err(format!(
            "{} is a {kind} picture, which isn't supported; convert it to PNG, then {picture}",
            path.display()
        ));
    }
    let instead = if crate::picture::is_picture(&path) {
        format!("; it is a picture: {picture}")
    } else {
        String::new()
    };
    Err(format!(
        "{} is not a model; models can be {}{instead}",
        path.display(),
        io::OPEN_FORMATS
    ))
}

/// The picture file under `key` for Load image, Emboss and textures; a model gets
/// pointed to import.
pub fn picture_file(a: &Value, key: &str) -> Result<PathBuf, String> {
    let path = file(a, key)?;
    if crate::picture::is_picture(&path) {
        return Ok(path);
    }
    if let Some(kind) = crate::picture::unsupported(&path) {
        return Err(format!(
            "{kind} isn't supported; convert {} to PNG",
            path.display()
        ));
    }
    let instead = if crate::io::Format::from_path(&path).is_some() {
        "; it is a 3D model: add it with import"
    } else {
        "; convert it to one of those first"
    };
    Err(format!(
        "{} is not a picture; pictures can be {}{instead}",
        path.display(),
        crate::picture::FORMATS
    ))
}

/// `save`'s file: an absolute path to a file name with an extension, in a folder that
/// may not exist (the write says so). Returns it as given.
pub fn save_path(a: &Value) -> Result<PathBuf, String> {
    use crate::io::{self, Format};
    let path = PathBuf::from(
        a["path"]
            .as_str()
            .ok_or("path is missing: the file to write, e.g. /home/me/box.3mf")?,
    );
    if !path.is_absolute() {
        return Err(format!(
            "give an absolute path, such as /home/me/box.3mf: {} is relative and the app's working folder is not yours",
            path.display()
        ));
    }
    if path.is_dir() || path.file_name().is_none() {
        return Err(format!(
            "{} is a folder; give the file to write in it, e.g. {}/box.3mf",
            path.display(),
            path.display().to_string().trim_end_matches('/')
        ));
    }
    if path.extension().is_none() {
        return Err(format!(
            "{} has no extension, which picks the format; add one of {}, e.g. {}.3mf",
            path.display(),
            io::SAVE_FORMATS,
            path.display()
        ));
    }
    if !Format::from_path(&path).is_some_and(|f| Format::SAVE.contains(&f)) {
        return Err(format!(
            "save writes {}, chosen by the path's extension; SVG and pictures cannot be saved",
            io::SAVE_FORMATS
        ));
    }
    if path.exists() && !flag(a, "overwrite", false)? {
        return Err(format!(
            "{} exists; pass overwrite to replace it",
            path.display()
        ));
    }
    Ok(path)
}

/// The size `[x, y, z]` of an object, when it is given.
pub fn object_size(a: &Value) -> Result<Option<Vec3>, String> {
    vec3(a, "size", OBJECT_SIZE)
}

/// What `transform` was asked to do.
#[derive(Debug, Default, PartialEq)]
pub struct Transform {
    pub rotation: Option<Vec3>,
    pub size: Option<Vec3>,
    pub position: Option<Vec3>,
    /// Side positions in the order they apply: (axis, maximum side, mm).
    pub sides: Vec<(usize, bool, f32)>,
}

pub fn transform(a: &Value) -> Result<Transform, String> {
    let mut t = Transform {
        rotation: vec3(a, "rotation", ANGLE)?,
        size: object_size(a)?,
        position: vec3(a, "position", COORD)?,
        sides: Vec::new(),
    };
    for (side, axis, max) in [
        ("bottom", 2, false),
        ("left", 0, false),
        ("right", 0, true),
        ("front", 1, false),
        ("back", 1, true),
    ] {
        if let Some(v) = optional(a, side, COORD)? {
            t.sides.push((axis, max, v));
        }
    }
    Ok(t)
}

/// `insert_shape`'s `custom`: (latitude, longitude, width, height, roundness).
pub fn custom(c: &Value) -> Result<(usize, usize, f32, f32, u8), String> {
    if !c.is_object() {
        return Err("custom must be an object, e.g. {\"roundness\": 5}".into());
    }
    Ok((
        integer(c, "latitude", 10, CUSTOM_STEPS)? as usize,
        integer(c, "longitude", 10, CUSTOM_STEPS)? as usize,
        number(c, "width", 40.0, CUSTOM_SIZE)?,
        number(c, "height", 40.0, CUSTOM_SIZE)?,
        integer(c, "roundness", 0, ROUNDNESS)? as u8,
    ))
}

/// `split`'s plane and the halves to keep; `centre` is the point it passes through when
/// neither `point` nor `z` says.
pub fn split(a: &Value, centre: Vec3) -> Result<(CutPlane, Keep), String> {
    let z = optional(a, "z", COORD)?;
    let at = vec3(a, "point", COORD)?;
    // The normal's length only has to be above zero and not absurd.
    let normal = vec3(a, "normal", Limit::new(-1e6, 1e6, ""))?;
    let rotation = vec3(a, "rotation", ANGLE)?;
    if normal.is_some() && rotation.is_some() {
        return Err("give normal or rotation, not both; they both turn the plane".into());
    }
    if z.is_none() && at.is_none() && normal.is_none() && rotation.is_none() {
        return Err(
            "give z for a level cut at that height, or point with normal or rotation for a slanted one"
                .into(),
        );
    }
    let mut centre = at.unwrap_or(centre);
    if let Some(z) = z {
        centre.z = z;
    }
    let mut plane = match normal {
        Some(n) => CutPlane::through(centre, n).ok_or("normal must not be zero")?,
        None => CutPlane {
            center: centre,
            offset: 0.0,
            rotation: rotation.unwrap_or(Vec3::ZERO),
            size: None,
        },
    };
    // 0 (the default) leaves that direction full, as an empty field in the panel.
    plane.set_extent(0, Some(number(a, "width", 0.0, PLANE_EXTENT)?));
    plane.set_extent(1, Some(number(a, "length", 0.0, PLANE_EXTENT)?));
    let keep = choice(
        a,
        "keep",
        Some(Keep::Top),
        &[
            ("top", Keep::Top),
            ("bottom", Keep::Bottom),
            ("both", Keep::Both),
        ],
    )?;
    Ok((plane, keep))
}

/// `emboss_text`'s settings, except `at`, which needs the object. The error of an
/// unknown font or a missing picture says what to use instead.
pub fn emboss(a: &Value) -> Result<Emboss, String> {
    let mut params = Emboss {
        text: text(a, "text", TEXT_CHARS)?.unwrap_or_default(),
        ..Emboss::default()
    };
    params.pattern = match a["pattern"].as_str().unwrap_or("text") {
        "text" => Pattern::Text,
        "image" if a["image"].is_null() => {
            return Err("pattern image needs a picture: give image (a PNG, JPEG or SVG path on the app's computer) or file_name with file_base64 or file_text".into())
        }
        "image" => Pattern::Image(picture_file(a, "image")?),
        p => Pattern::Shape(Shape::from_name(p).ok_or_else(|| {
            let shapes: Vec<String> = Shape::ALL.iter().map(|s| s.name()).collect();
            format!(
                "unknown pattern: {p}; one of text, image, {}",
                shapes.join(", ")
            )
        })?),
    };
    if !a["image"].is_null() && !matches!(params.pattern, Pattern::Image(_)) {
        return Err("a picture is used only with pattern image; add \"pattern\": \"image\"".into());
    }
    if params.pattern == Pattern::Text && params.text.trim().is_empty() {
        return Err(
            "text is empty; give text, or pattern image with a picture, or a shape name".into(),
        );
    }
    params.size = number(a, "size", params.size, EMBOSS_SIZE)?;
    params.depth = number(a, "depth", params.depth, EMBOSS_DEPTH)?;
    params.engrave = flag(a, "engrave", false)?;
    params.bevel = flag(a, "bevel", false)?;
    params.angle = number(a, "angle", 0.0, TURN)?;
    params.projection = choice(
        a,
        "projection",
        Some(Projection::Planar),
        &[
            ("planar", Projection::Planar),
            ("cylindrical", Projection::Cylindrical),
            ("spherical", Projection::Spherical),
        ],
    )?;
    if let Some(f) = text(a, "font", 200)? {
        params.font = Some(crate::mcp::find_font(&crate::emboss::font_families(), &f)?);
    }
    params.top_color = color(a, "top_color")?;
    params.side_color = color(a, "side_color")?;
    if let Some(offset) = vec2(a, "offset", COORD)? {
        params.offset = offset;
    }
    if let Some(width) = optional(a, "width", EMBOSS_WIDTH)? {
        // The width stretches the pattern sideways; `size` still sets its height.
        params.stretch =
            crate::emboss::stretch_for_width(&params, width).map_err(|e| format!("{e:#}"))?;
    }
    Ok(params)
}

/// `load_image`'s settings.
pub fn load_image(a: &Value) -> Result<crate::image3d::Options, String> {
    use crate::image3d::{Mode, Options};
    let mut opts = Options::new(picture_file(a, "path")?);
    opts.mode = choice(
        a,
        "method",
        Some(Mode::Contour),
        &[
            ("contour", Mode::Contour),
            ("height_map", Mode::HeightMap),
            ("edge", Mode::Edge),
            ("stamp", Mode::Stamp),
        ],
    )?;
    opts.size = number(a, "size", opts.size, IMAGE_SIZE)?;
    opts.depth = number(a, "depth", opts.depth, IMAGE_DEPTH)?;
    opts.levels = integer(a, "levels", opts.levels, IMAGE_LEVELS)?;
    opts.smooth = number(a, "smooth", opts.smooth, FRACTION)?;
    opts.invert = flag(a, "invert", opts.invert)?;
    opts.textures = flag(a, "textures", opts.textures)?;
    Ok(opts)
}

/// `extrude_outline`'s settings; `height` is the default border height.
pub fn extrude_outline(a: &Value, height: f32) -> Result<crate::outline::Options, String> {
    use crate::outline::{Options, Side};
    Ok(Options {
        side: choice(
            a,
            "side",
            Some(Side::Outside),
            &[("outside", Side::Outside), ("inside", Side::Inside)],
        )?,
        width: number(a, "width", crate::outline::DEFAULT_WIDTH, BORDER_WIDTH)?,
        height: number(
            a,
            "height",
            height.clamp(BORDER_HEIGHT.min, BORDER_HEIGHT.max),
            BORDER_HEIGHT,
        )?,
        keep_model: flag(a, "keep_model", true)?,
        smooth_outer: number(a, "smooth_outer", 0.0, FRACTION)?,
        smooth_inner: number(a, "smooth_inner", 0.0, FRACTION)?,
    })
}

/// `hollow`'s wall, mm.
pub fn wall(a: &Value) -> Result<f32, String> {
    number_in(&a["wall"], "wall", WALL)
}

/// `smooth`'s passes, 0 to the most the tool has.
pub fn passes(a: &Value) -> Result<u32, String> {
    let most = crate::solid::SMOOTH_STEPS as f32;
    integer(a, "passes", 3, Limit::new(0.0, most, ""))
}

/// `simplify`'s fraction of triangles to remove.
pub fn simplify_remove(a: &Value) -> Result<f32, String> {
    number_in(&a["remove"], "remove", FRACTION)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn arguments_take_defaults_and_reject_bad_values() {
        let a =
            json!({"coverage": 0.25, "big": 2.0, "n": 5, "on": true, "c": "#FF8000", "m": "Cm"});
        assert_eq!(number(&a, "coverage", 0.0, FRACTION), Ok(0.25));
        assert_eq!(number(&a, "missing", 0.5, FRACTION), Ok(0.5));
        assert_eq!(
            number(&a, "big", 0.0, FRACTION).unwrap_err(),
            "big must be a number between 0 and 1"
        );
        let steps = Limit::new(2.0, 8.0, "");
        assert_eq!(integer(&a, "n", 1, steps), Ok(5));
        assert!(integer(&a, "coverage", 1, steps).is_err());
        assert_eq!(flag(&a, "on", false), Ok(true));
        assert!(flag(&a, "n", false).is_err());
        assert_eq!(color(&a, "c"), Ok(Some([0xFF, 0x80, 0x00, 0xFF])));
        assert_eq!(color(&a, "none"), Ok(None));
        assert!(color(&a, "n").is_err());
        let units = [("mm", 1), ("cm", 10)];
        assert_eq!(choice(&a, "m", None, &units), Ok(10));
        assert_eq!(choice(&a, "x", Some(1), &units), Ok(1));
        assert!(choice(&a, "x", None, &units)
            .unwrap_err()
            .contains("mm, cm"));
        assert!(choice(&a, "c", None, &units).is_err());
    }

    #[test]
    fn errors_name_the_range_with_its_unit() {
        let e = number(&json!({"size": 1e5}), "size", 10.0, EMBOSS_SIZE).unwrap_err();
        assert_eq!(e, "size must be a number between 1 and 200 mm");
        let e = vec3(&json!({"size": [1, 2, 1e300]}), "size", OBJECT_SIZE).unwrap_err();
        assert!(e.contains("between 0.01 and 10000 mm"), "{e}");
        let e = text(
            &json!({"text": "x".repeat(TEXT_CHARS + 1)}),
            "text",
            TEXT_CHARS,
        )
        .unwrap_err();
        assert!(e.contains(&format!("at most {TEXT_CHARS}")), "{e}");
        assert!(!FRACTION.contains(f64::NAN) && !FRACTION.contains(f64::INFINITY));
        assert!(number_in(&json!(1e300), "x", COORD).is_err());
    }
}
