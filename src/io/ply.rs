//! PLY: reads ASCII and binary little-endian; writes binary little-endian with
//! per-vertex RGBA, mm, baked transforms (H44).

use super::{baked, ModelObject};
use crate::mesh::Mesh;
use crate::scene::DEFAULT_COLOR;
use anyhow::{anyhow, bail, Context, Result};
use glam::{Mat4, Vec3};
use std::io::{BufWriter, Write};
use std::path::Path;

#[derive(Clone, Copy)]
enum Ty {
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    F32,
    F64,
}

impl Ty {
    fn parse(s: &str) -> Result<Ty> {
        Ok(match s {
            "char" | "int8" => Ty::I8,
            "uchar" | "uint8" => Ty::U8,
            "short" | "int16" => Ty::I16,
            "ushort" | "uint16" => Ty::U16,
            "int" | "int32" => Ty::I32,
            "uint" | "uint32" => Ty::U32,
            "float" | "float32" => Ty::F32,
            "double" | "float64" => Ty::F64,
            _ => bail!("Unknown PLY type: {s}"),
        })
    }
    fn size(self) -> usize {
        match self {
            Ty::I8 | Ty::U8 => 1,
            Ty::I16 | Ty::U16 => 2,
            Ty::I32 | Ty::U32 | Ty::F32 => 4,
            Ty::F64 => 8,
        }
    }
    fn read(self, b: &[u8]) -> f64 {
        match self {
            Ty::I8 => b[0] as i8 as f64,
            Ty::U8 => b[0] as f64,
            Ty::I16 => i16::from_le_bytes([b[0], b[1]]) as f64,
            Ty::U16 => u16::from_le_bytes([b[0], b[1]]) as f64,
            Ty::I32 => i32::from_le_bytes(b[..4].try_into().unwrap()) as f64,
            Ty::U32 => u32::from_le_bytes(b[..4].try_into().unwrap()) as f64,
            Ty::F32 => f32::from_le_bytes(b[..4].try_into().unwrap()) as f64,
            Ty::F64 => f64::from_le_bytes(b[..8].try_into().unwrap()),
        }
    }
}

enum Prop {
    Scalar(String, Ty),
    List(String, Ty, Ty),
}

/// Values in file order, from either encoding, so one loop reads both.
enum Source<'a> {
    Binary(&'a [u8]),
    Ascii(std::str::SplitWhitespace<'a>),
}

impl Source<'_> {
    fn next(&mut self, t: Ty) -> Result<f64> {
        match self {
            Source::Binary(b) => {
                let v = t.read(b.get(..t.size()).ok_or_else(|| anyhow!("Truncated PLY"))?);
                *b = &b[t.size()..];
                Ok(v)
            }
            Source::Ascii(it) => Ok(it.next().ok_or_else(|| anyhow!("Truncated PLY"))?.parse()?),
        }
    }
}

struct Element {
    name: String,
    count: usize,
    props: Vec<Prop>,
}

pub fn read(path: &Path, name: &str) -> Result<ModelObject> {
    let data = std::fs::read(path).with_context(|| format!("Could not read {}", path.display()))?;
    let end = find(&data, b"end_header").ok_or_else(|| anyhow!("PLY without end_header"))?;
    let header = std::str::from_utf8(&data[..end])?;
    let mut body = end + "end_header".len();
    if data.get(body) == Some(&b'\r') {
        body += 1;
    }
    if data.get(body) == Some(&b'\n') {
        body += 1;
    }

    let mut format = "";
    let mut elements: Vec<Element> = Vec::new();
    for line in header.lines() {
        let tok: Vec<&str> = line.split_whitespace().collect();
        match tok.as_slice() {
            ["format", f, ..] => format = f,
            ["element", n, c] => elements.push(Element {
                name: n.to_string(),
                count: c.parse()?,
                props: vec![],
            }),
            ["property", "list", ct, it, n] => {
                let e = elements
                    .last_mut()
                    .ok_or_else(|| anyhow!("property before element"))?;
                e.props
                    .push(Prop::List(n.to_string(), Ty::parse(ct)?, Ty::parse(it)?));
            }
            ["property", t, n] => {
                let e = elements
                    .last_mut()
                    .ok_or_else(|| anyhow!("property before element"))?;
                e.props.push(Prop::Scalar(n.to_string(), Ty::parse(t)?));
            }
            _ => {}
        }
    }

    let mut src = match format {
        "binary_little_endian" => Source::Binary(&data[body..]),
        "ascii" => Source::Ascii(std::str::from_utf8(&data[body..])?.split_whitespace()),
        other => bail!("Unsupported PLY format: {other}"),
    };
    let mut positions = Vec::new();
    let mut triangles = Vec::new();
    let mut color = None;
    for e in &elements {
        for _ in 0..e.count {
            let mut xyz = [0f64; 3];
            let mut rgb = [None::<f64>; 3];
            for p in &e.props {
                match p {
                    Prop::Scalar(n, t) => assign(n, src.next(*t)?, &mut xyz, &mut rgb),
                    Prop::List(n, ct, it) => {
                        let k = src.next(*ct)? as usize;
                        let idx = (0..k)
                            .map(|_| src.next(*it).map(|v| v as u32))
                            .collect::<Result<Vec<_>>>()?;
                        if e.name == "face" && (n == "vertex_indices" || n == "vertex_index") {
                            super::fan(&idx, &mut triangles);
                        }
                    }
                }
            }
            if e.name == "vertex" {
                push_vertex(xyz, rgb, &mut positions, &mut color);
            }
        }
    }
    if triangles
        .iter()
        .flatten()
        .any(|i| *i as usize >= positions.len())
    {
        bail!("Face index out of range");
    }
    Ok(ModelObject {
        name: name.to_string(),
        mesh: Mesh::new(positions, triangles),
        transform: Mat4::IDENTITY,
        color: color.unwrap_or(DEFAULT_COLOR),
    })
}

fn assign(name: &str, v: f64, xyz: &mut [f64; 3], rgb: &mut [Option<f64>; 3]) {
    match name {
        "x" => xyz[0] = v,
        "y" => xyz[1] = v,
        "z" => xyz[2] = v,
        "red" => rgb[0] = Some(v),
        "green" => rgb[1] = Some(v),
        "blue" => rgb[2] = Some(v),
        _ => {}
    }
}

fn push_vertex(
    xyz: [f64; 3],
    rgb: [Option<f64>; 3],
    positions: &mut Vec<Vec3>,
    color: &mut Option<[u8; 4]>,
) {
    positions.push(Vec3::new(xyz[0] as f32, xyz[1] as f32, xyz[2] as f32));
    if color.is_none() {
        if let [Some(r), Some(g), Some(b)] = rgb {
            *color = Some([r as u8, g as u8, b as u8, 255]);
        }
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

pub fn write(path: &Path, objects: &[ModelObject]) -> Result<()> {
    let meshes = baked(objects);
    let nv: usize = meshes.iter().map(|m| m.positions.len()).sum();
    let nf: usize = meshes.iter().map(|m| m.triangles.len()).sum();
    let mut w = BufWriter::new(std::fs::File::create(path)?);
    write!(
        w,
        "ply\nformat binary_little_endian 1.0\nelement vertex {nv}\nproperty float x\nproperty float y\nproperty float z\n\
         property uchar red\nproperty uchar green\nproperty uchar blue\nproperty uchar alpha\n\
         element face {nf}\nproperty list uchar int vertex_indices\nend_header\n"
    )?;
    for (o, m) in objects.iter().zip(&meshes) {
        for p in &m.positions {
            for x in p.to_array() {
                w.write_all(&x.to_le_bytes())?;
            }
            w.write_all(&o.color)?;
        }
    }
    let mut base = 0i32;
    for m in &meshes {
        for t in &m.triangles {
            w.write_all(&[3])?;
            for i in t {
                w.write_all(&(*i as i32 + base).to_le_bytes())?;
            }
        }
        base += m.positions.len() as i32;
    }
    w.flush()?;
    Ok(())
}
