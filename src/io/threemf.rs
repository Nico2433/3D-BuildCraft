//! 3MF core (2015/02). Writes mm units, `basematerials` with RGBA colours, object-level
//! `pid`/`pindex`, placement in the build item `transform` (not baked) and an optional
//! Title, as the original does (H32, H39, H40, H42). Reads components and units.

use super::{color_name, ModelObject, Rgba};
use crate::mesh::Mesh;
use crate::scene::DEFAULT_COLOR;
use anyhow::{anyhow, bail, Context, Result};
use glam::{Mat4, Vec3, Vec4};
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;

const CORE_NS: &str = "http://schemas.microsoft.com/3dmanufacturing/core/2015/02";
const MODEL_REL: &str = "http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel";

/// Writes to a temporary file beside `path` and renames it over the target, so a failed
/// or interrupted save (or autosave) never destroys the previous file.
pub fn write(path: &Path, objects: &[ModelObject], title: Option<&str>) -> Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = std::path::PathBuf::from(tmp);
    let file = std::fs::File::create(&tmp)
        .with_context(|| format!("Could not create {}", tmp.display()))?;
    let written = write_to(file, objects, title).and_then(|()| Ok(std::fs::rename(&tmp, path)?));
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

pub fn write_to<W: Write + std::io::Seek>(
    out: W,
    objects: &[ModelObject],
    title: Option<&str>,
) -> Result<()> {
    let mut zip = zip::ZipWriter::new(out);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    zip.start_file("[Content_Types].xml", opts)?;
    zip.write_all(
        br#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml" /><Default Extension="model" ContentType="application/vnd.ms-package.3dmanufacturing-3dmodel+xml" /></Types>"#,
    )?;
    zip.start_file("_rels/.rels", opts)?;
    write!(
        zip,
        r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Target="/3D/3dmodel.model" Id="rel0" Type="{MODEL_REL}" /></Relationships>"#
    )?;

    zip.start_file("3D/3dmodel.model", opts)?;
    let mut s = String::with_capacity(1 << 16);
    s.push_str(&format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<model unit=\"millimeter\" xml:lang=\"en-US\" xmlns=\"{CORE_NS}\">\n"
    ));
    if let Some(t) = title {
        s.push_str(&format!(
            "  <metadata name=\"Title\">{}</metadata>\n",
            escape(t)
        ));
    }
    s.push_str("  <resources>\n");
    let colors = super::unique_colors(objects);
    s.push_str("    <basematerials id=\"1\">\n");
    for c in &colors {
        s.push_str(&format!(
            "      <base name=\"{}\" displaycolor=\"#{:02X}{:02X}{:02X}{:02X}\" />\n",
            escape(&color_name(*c)),
            c[0],
            c[1],
            c[2],
            c[3]
        ));
    }
    s.push_str("    </basematerials>\n");
    for (i, o) in objects.iter().enumerate() {
        let pindex = colors.iter().position(|c| *c == o.color).unwrap();
        s.push_str(&format!(
            "    <object id=\"{}\" type=\"model\" name=\"{}\" pid=\"1\" pindex=\"{pindex}\">\n      <mesh>\n        <vertices>\n",
            i + 2,
            escape(&o.name)
        ));
        for p in &o.mesh.positions {
            s.push_str(&format!(
                "          <vertex x=\"{}\" y=\"{}\" z=\"{}\" />\n",
                p.x, p.y, p.z
            ));
        }
        s.push_str("        </vertices>\n        <triangles>\n");
        for (ti, t) in o.mesh.triangles.iter().enumerate() {
            // A painted triangle names its own material.
            let paint = o.mesh.face_color(ti).map_or(String::new(), |c| {
                let i = colors.iter().position(|k| *k == c).unwrap();
                format!(" pid=\"1\" p1=\"{i}\"")
            });
            s.push_str(&format!(
                "          <triangle v1=\"{}\" v2=\"{}\" v3=\"{}\"{paint} />\n",
                t[0], t[1], t[2]
            ));
        }
        s.push_str("        </triangles>\n      </mesh>\n    </object>\n");
    }
    s.push_str("  </resources>\n  <build>\n");
    for (i, o) in objects.iter().enumerate() {
        s.push_str(&format!(
            "    <item objectid=\"{}\" transform=\"{}\" />\n",
            i + 2,
            format_transform(&o.transform)
        ));
    }
    s.push_str("  </build>\n</model>\n");
    zip.write_all(s.as_bytes())?;
    zip.finish()?;
    Ok(())
}

/// 3MF stores the 4×3 affine part column by column, which is glam's column-major order.
pub fn format_transform(m: &Mat4) -> String {
    let c = m.to_cols_array();
    [
        c[0], c[1], c[2], c[4], c[5], c[6], c[8], c[9], c[10], c[12], c[13], c[14],
    ]
    .iter()
    .map(|v| v.to_string())
    .collect::<Vec<_>>()
    .join(" ")
}

pub fn parse_transform(s: &str) -> Result<Mat4> {
    let v: Vec<f32> = s
        .split_whitespace()
        .map(|x| x.parse())
        .collect::<Result<_, _>>()?;
    if v.len() != 12 {
        bail!("3MF transform with {} values", v.len());
    }
    Ok(Mat4::from_cols(
        Vec4::new(v[0], v[1], v[2], 0.0),
        Vec4::new(v[3], v[4], v[5], 0.0),
        Vec4::new(v[6], v[7], v[8], 0.0),
        Vec4::new(v[9], v[10], v[11], 1.0),
    ))
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn unit_scale(unit: &str) -> f32 {
    match unit {
        "micron" => 0.001,
        "centimeter" => 10.0,
        "inch" => 25.4,
        "foot" => 304.8,
        "meter" => 1000.0,
        _ => 1.0,
    }
}

/// (basematerials id, index) of an object's colour.
type MaterialRef = (u32, usize);

#[derive(Default)]
struct ObjDef {
    name: Option<String>,
    mesh: Option<Mesh>,
    color: Option<Rgba>,
    components: Vec<(u32, Mat4)>,
}

pub fn read(path: &Path) -> Result<Vec<ModelObject>> {
    let file =
        std::fs::File::open(path).with_context(|| format!("Could not open {}", path.display()))?;
    let mut zip = zip::ZipArchive::new(file).context("The 3MF is not a valid ZIP package")?;
    let model_path = find_model_path(&mut zip).unwrap_or_else(|| "3D/3dmodel.model".to_string());
    let mut xml = String::new();
    zip.by_name(model_path.trim_start_matches('/'))
        .with_context(|| format!("{model_path} is missing from the 3MF"))?
        .read_to_string(&mut xml)?;
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Object");
    parse_model(&xml, stem)
}

fn find_model_path<R: Read + std::io::Seek>(zip: &mut zip::ZipArchive<R>) -> Option<String> {
    let mut rels = String::new();
    zip.by_name("_rels/.rels")
        .ok()?
        .read_to_string(&mut rels)
        .ok()?;
    let mut r = Reader::from_str(&rels);
    loop {
        match r.read_event().ok()? {
            Event::Empty(e) | Event::Start(e) if e.local_name().as_ref() == b"Relationship" => {
                if attr(&e, "Type").as_deref() == Some(MODEL_REL) {
                    return attr(&e, "Target");
                }
            }
            Event::Eof => return None,
            _ => {}
        }
    }
}

fn attr(e: &BytesStart, name: &str) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.local_name().as_ref() == name.as_bytes())
        .and_then(|a| a.unescape_value().ok().map(|v| v.into_owned()))
}

fn parse_color(s: &str) -> Option<Rgba> {
    let h = s.trim_start_matches('#');
    let b = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
    match h.len() {
        6 => Some([b(0)?, b(2)?, b(4)?, 255]),
        8 => Some([b(0)?, b(2)?, b(4)?, b(6)?]),
        _ => None,
    }
}

pub fn parse_model(xml: &str, default_name: &str) -> Result<Vec<ModelObject>> {
    let mut r = Reader::from_str(xml);
    let mut scale = 1.0f32;
    let mut materials: HashMap<u32, Vec<Rgba>> = HashMap::new();
    let mut objects: HashMap<u32, ObjDef> = HashMap::new();
    let mut build: Vec<(u32, Mat4)> = Vec::new();

    let mut cur_obj: Option<(u32, ObjDef, Option<MaterialRef>)> = None;
    let mut cur_mat: Option<u32> = None;
    let mut positions: Vec<Vec3> = Vec::new();
    let mut triangles: Vec<[u32; 3]> = Vec::new();
    let mut tri_colors: HashMap<(u32, usize), usize> = HashMap::new();
    // Each triangle's own (material group, index), to keep painted areas.
    let mut tri_keys: Vec<Option<(u32, usize)>> = Vec::new();

    loop {
        let ev = r
            .read_event()
            .map_err(|e| anyhow!("Invalid 3MF XML: {e}"))?;
        let e = match &ev {
            Event::Start(e) | Event::Empty(e) => Some(e.clone()),
            Event::End(end) => {
                match end.local_name().as_ref() {
                    b"object" => {
                        if let Some((id, mut def, pid)) = cur_obj.take() {
                            let (pos, tris) = (
                                std::mem::take(&mut positions),
                                std::mem::take(&mut triangles),
                            );
                            let keys = std::mem::take(&mut tri_keys);
                            // Object colour: object-level pid/pindex, else the most used triangle colour.
                            let main = pid.or_else(|| {
                                tri_colors.iter().max_by_key(|(_, n)| **n).map(|(k, _)| *k)
                            });
                            if !tris.is_empty() {
                                let mut mesh = Mesh::new(pos, tris);
                                // Any triangle in another material: keep them per face.
                                let mixed = keys.iter().any(|k| k.is_some() && *k != main);
                                if mixed && keys.len() == mesh.triangles.len() {
                                    mesh.colors = keys
                                        .iter()
                                        .map(|k| {
                                            k.and_then(|(g, i)| materials.get(&g)?.get(i).copied())
                                                .unwrap_or([0; 4])
                                        })
                                        .collect();
                                }
                                def.mesh = Some(mesh);
                            }
                            def.color = main.and_then(|(g, i)| materials.get(&g)?.get(i).copied());
                            tri_colors.clear();
                            objects.insert(id, def);
                        }
                    }
                    b"basematerials" => cur_mat = None,
                    _ => {}
                }
                None
            }
            Event::Eof => break,
            _ => None,
        };
        let Some(e) = e else { continue };
        match e.local_name().as_ref() {
            b"model" => scale = unit_scale(attr(&e, "unit").as_deref().unwrap_or("millimeter")),
            b"basematerials" => {
                let id: u32 = attr(&e, "id").and_then(|v| v.parse().ok()).unwrap_or(0);
                materials.entry(id).or_default();
                cur_mat = Some(id);
            }
            b"base" => {
                if let Some(id) = cur_mat {
                    let c = attr(&e, "displaycolor")
                        .and_then(|c| parse_color(&c))
                        .unwrap_or(DEFAULT_COLOR);
                    materials.entry(id).or_default().push(c);
                }
            }
            b"object" => {
                let id: u32 = attr(&e, "id")
                    .and_then(|v| v.parse().ok())
                    .ok_or_else(|| anyhow!("object without id"))?;
                let pid = attr(&e, "pid").and_then(|p| p.parse().ok()).map(|p| {
                    (
                        p,
                        attr(&e, "pindex").and_then(|i| i.parse().ok()).unwrap_or(0),
                    )
                });
                let def = ObjDef {
                    name: attr(&e, "name"),
                    ..Default::default()
                };
                cur_obj = Some((id, def, pid));
            }
            b"vertex" => {
                let f = |k: &str| {
                    attr(&e, k)
                        .and_then(|v| v.parse::<f32>().ok())
                        .unwrap_or(0.0)
                };
                positions.push(Vec3::new(f("x"), f("y"), f("z")) * scale);
            }
            b"triangle" => {
                let f = |k: &str| attr(&e, k).and_then(|v| v.parse::<u32>().ok());
                let (Some(a), Some(b), Some(c)) = (f("v1"), f("v2"), f("v3")) else {
                    bail!("Incomplete 3MF triangle");
                };
                triangles.push([a, b, c]);
                let key = f("p1").and_then(|p1| {
                    Some((
                        f("pid").or(cur_obj.as_ref()?.2.map(|(g, _)| g))?,
                        p1 as usize,
                    ))
                });
                if let Some(k) = key {
                    *tri_colors.entry(k).or_default() += 1;
                }
                tri_keys.push(key);
            }
            b"component" => {
                if let Some((_, def, _)) = cur_obj.as_mut() {
                    let oid = attr(&e, "objectid")
                        .and_then(|v| v.parse().ok())
                        .ok_or_else(|| anyhow!("component without objectid"))?;
                    let t = attr(&e, "transform")
                        .map(|t| parse_transform(&t))
                        .transpose()?
                        .unwrap_or(Mat4::IDENTITY);
                    def.components.push((oid, t));
                }
            }
            b"item" => {
                let oid = attr(&e, "objectid")
                    .and_then(|v| v.parse().ok())
                    .ok_or_else(|| anyhow!("item without objectid"))?;
                let t = attr(&e, "transform")
                    .map(|t| parse_transform(&t))
                    .transpose()?
                    .unwrap_or(Mat4::IDENTITY);
                build.push((oid, t));
            }
            _ => {}
        }
    }

    // Unit scale also applies to translations in transforms.
    let fix = |m: Mat4| {
        let mut m = m;
        m.w_axis = (m.w_axis.truncate() * scale).extend(1.0);
        m
    };
    let mut out = Vec::new();
    for (oid, t) in build {
        flatten(&objects, oid, fix(t), None, default_name, &fix, &mut out, 0)?;
    }
    for o in &out {
        if o.mesh
            .triangles
            .iter()
            .flatten()
            .any(|i| *i as usize >= o.mesh.positions.len())
        {
            bail!("Vertex index out of range in {}", o.name);
        }
    }
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
fn flatten(
    objects: &HashMap<u32, ObjDef>,
    id: u32,
    t: Mat4,
    inherited: Option<Rgba>,
    default_name: &str,
    fix: &dyn Fn(Mat4) -> Mat4,
    out: &mut Vec<ModelObject>,
    depth: usize,
) -> Result<()> {
    if depth > 32 {
        bail!("3MF components nest in a cycle");
    }
    let def = objects
        .get(&id)
        .ok_or_else(|| anyhow!("The build references object {id}, which does not exist"))?;
    let color = def.color.or(inherited);
    if let Some(mesh) = &def.mesh {
        out.push(ModelObject {
            name: def.name.clone().unwrap_or_else(|| default_name.to_string()),
            mesh: mesh.clone(),
            transform: t,
            color: color.unwrap_or(DEFAULT_COLOR),
        });
    }
    for (cid, ct) in &def.components {
        flatten(
            objects,
            *cid,
            t * fix(*ct),
            color,
            default_name,
            fix,
            out,
            depth + 1,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_original_autosave_layout() {
        // Shape of the autosave written by 3D Builder: a mesh wrapped in a components
        // object, placed by the build item transform (findings H37, H40).
        let xml = r##"<?xml version="1.0" encoding="UTF-8"?>
<model unit="millimeter" xml:lang="en-US" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02">
  <resources>
    <basematerials id="2"><base name="Turquoise" displaycolor="#00A0E8FF" /></basematerials>
    <object id="3" type="model" pid="2" pindex="0"><mesh><vertices>
      <vertex x="0" y="0" z="0" /><vertex x="1" y="0" z="0" /><vertex x="0" y="1" z="0" />
    </vertices><triangles><triangle v1="0" v2="1" v3="2" /></triangles></mesh></object>
    <object id="4" type="model"><components><component objectid="3" /></components></object>
  </resources>
  <build><item objectid="4" transform="2 0 0 0 2 0 0 0 2 -6.68493 46.1068 25.7285" /></build>
</model>"##;
        let objs = parse_model(xml, "x").unwrap();
        assert_eq!(objs.len(), 1);
        assert_eq!(objs[0].color, [0x00, 0xA0, 0xE8, 0xFF]);
        let p = objs[0].transform.transform_point3(Vec3::X);
        assert!((p - Vec3::new(-4.68493, 46.1068, 25.7285)).length() < 1e-4);
    }

    #[test]
    fn save_replaces_the_file_without_leaving_a_temp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scene.3mf");
        std::fs::write(&path, b"old").unwrap();
        write(&path, &[], Some("t")).unwrap();
        assert!(zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).is_ok());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn transform_string_round_trips() {
        let m = Mat4::from_scale_rotation_translation(
            Vec3::new(1.0, 2.0, 3.0),
            glam::Quat::from_rotation_z(0.3),
            Vec3::new(4.0, 5.0, 6.0),
        );
        let back = parse_transform(&format_transform(&m)).unwrap();
        assert!(m.abs_diff_eq(back, 1e-5));
    }

    #[test]
    fn centimeter_units_convert_to_mm() {
        let xml = r#"<model unit="centimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02"><resources>
<object id="1" type="model"><mesh><vertices><vertex x="1" y="0" z="0" /><vertex x="0" y="1" z="0" /><vertex x="0" y="0" z="1" /></vertices>
<triangles><triangle v1="0" v2="1" v3="2" /></triangles></mesh></object></resources>
<build><item objectid="1" transform="1 0 0 0 1 0 0 0 1 1 0 0" /></build></model>"#;
        let o = &parse_model(xml, "x").unwrap()[0];
        assert_eq!(o.mesh.positions[0], Vec3::new(10.0, 0.0, 0.0));
        assert_eq!(o.transform.w_axis.x, 10.0);
    }
}
