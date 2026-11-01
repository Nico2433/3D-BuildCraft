//! wgpu viewport drawn inside egui through a paint callback. Runs on DX12, Vulkan and
//! Metal through wgpu (ADR-0001).

mod callback;
mod pipelines;
mod selection;
mod shader;

use crate::mesh::Mesh;
use crate::scene::Material;
use bytemuck::{Pod, Zeroable};
use eframe::egui;
use eframe::egui_wgpu::{self, wgpu, CallbackResources, CallbackTrait};
use glam::{Mat4, Vec3, Vec4};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use wgpu::util::DeviceExt;

/// Must match `NativeOptions::multisampling` and `depth_buffer` in main.rs.
pub const MSAA_SAMPLES: u32 = 4;
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth24Plus;

const OBJECT_STRIDE: u64 = 256;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Globals {
    view_proj: [[f32; 4]; 4],
    eye: [f32; 4],
    light: [f32; 4],
    /// x shading, y smoothing, z x-ray, w colours
    flags: [f32; 4],
    /// x show floor, y reflections, z floor tone (0 default, 1 light, 2 dark)
    flags2: [f32; 4],
    /// Viewport size in pixels (xy), for the selection outline's width.
    viewport: [f32; 4],
    /// Plane (normal xyz, distance w): what lies on its positive side is not drawn. A zero
    /// normal clips nothing.
    clip: [f32; 4],
    /// World to (u, v, height) of the live sticker preview.
    decal: [[f32; 4]; 4],
    /// Sticker normal (xyz) and how far from its plane it still shows (w).
    decal_n: [f32; 4],
    /// x: the sticker's texture layer + 1, 0 for no preview.
    decal_p: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ObjectUniform {
    model: [[f32; 4]; 4],
    normal: [[f32; 4]; 4],
    color: [f32; 4],
    /// x selected, y pass (0 object, 1 reflection, 2 shadow), z realistic, w takes the sticker preview
    extra: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    pos: [f32; 3],
    normal: [f32; 3],
    /// Painted colour, alpha 0 for the object's.
    color: [u8; 4],
    uv: [f32; 2],
    /// [`crate::texture::FaceTex::pack`]
    tex: u32,
}

/// What one frame draws. Built by the UI, consumed by the callback.
#[derive(Clone)]
pub struct DrawItem {
    pub mesh: Arc<Mesh>,
    pub model: Mat4,
    pub color: [u8; 4],
    pub selected: bool,
    pub material: Material,
}

#[derive(Clone, Copy)]
pub struct ViewFlags {
    pub shading: bool,
    pub smoothing: bool,
    pub xray: bool,
    pub colors: bool,
    pub wireframe: bool,
    pub floor: bool,
    /// 0 the original's default checker, 1 light, 2 dark.
    pub floor_tone: u8,
    pub shadows: bool,
    pub reflections: bool,
}

/// A sticker shown live on one item, before it is baked into the mesh.
#[derive(Clone, Copy)]
pub struct DecalPreview {
    /// Index into the frame's items.
    pub item: usize,
    pub decal: crate::texture::Decal,
    pub layer: u16,
}

pub struct FrameCallback {
    pub items: Vec<DrawItem>,
    pub decal: Option<DecalPreview>,
    pub view_proj: Mat4,
    pub eye: Vec3,
    pub flags: ViewFlags,
    /// Size of the viewport in pixels.
    pub viewport: glam::Vec2,
    /// A plane (normal, distance) whose positive side is cut away, as in the Split preview.
    pub clip: Option<(Vec3, f32)>,
    /// A translucent square (the Split plane): the unit square -1..1 in XY under this
    /// matrix, with its colour. Depth tested, so objects hide it.
    pub slab: Option<(Mat4, [f32; 4])>,
}

struct GpuMesh {
    _keep: Arc<Mesh>,
    vbuf: wgpu::Buffer,
    ibuf: wgpu::Buffer,
    index_count: u32,
    bounds: crate::mesh::Aabb,
    /// Drawn triangles, for the wireframe built on first use.
    triangles: Vec<[u32; 3]>,
    lines: Option<(wgpu::Buffer, u32)>,
}

#[derive(PartialEq)]
struct SelectionKey {
    view_proj: Mat4,
    size: [u32; 2],
    items: Vec<(usize, Mat4)>,
}

struct SelectionMask {
    key: SelectionKey,
    mask: wgpu::TextureView,
    mask_bg: wgpu::BindGroup,
    outline: wgpu::TextureView,
    outline_bg: wgpu::BindGroup,
}

pub struct Renderer {
    solid: wgpu::RenderPipeline,
    xray: wgpu::RenderPipeline,
    wire: wgpu::RenderPipeline,
    outline: wgpu::RenderPipeline,
    mask_pipeline: wgpu::RenderPipeline,
    dilate: wgpu::RenderPipeline,
    mask_layout: wgpu::BindGroupLayout,
    selection: Option<SelectionMask>,
    shadow: wgpu::RenderPipeline,
    floor: wgpu::RenderPipeline,
    slab: wgpu::RenderPipeline,
    globals_buf: wgpu::Buffer,
    globals_bg: wgpu::BindGroup,
    object_layout: wgpu::BindGroupLayout,
    object_buf: wgpu::Buffer,
    object_bg: wgpu::BindGroup,
    object_capacity: u64,
    /// Scratch for the per-frame object uniforms, kept to avoid reallocating.
    object_bytes: Vec<u8>,
    meshes: HashMap<usize, GpuMesh>,
    /// The texture array and how many registry pictures it already holds.
    textures: wgpu::Texture,
    texture_bg: wgpu::BindGroup,
    textures_loaded: usize,
}

impl Renderer {
    /// Mesh uploads and wireframe copies are rebuilt on the next draw.
    pub fn release_meshes(&mut self) {
        let bytes: u64 = self
            .meshes
            .values()
            .map(|m| m.vbuf.size() + m.ibuf.size() + m.lines.as_ref().map_or(0, |(b, _)| b.size()))
            .sum();
        log::info!(
            "power: releasing {} GPU meshes, {} buffer bytes",
            self.meshes.len(),
            bytes
        );
        self.meshes = HashMap::new();
        self.selection = None;
    }

    /// The array every picture of the registry is copied into, with its mip levels.
    fn texture_array(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
    ) -> (wgpu::Texture, wgpu::BindGroup) {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("textures"),
            size: wgpu::Extent3d {
                width: crate::texture::SIZE,
                height: crate::texture::SIZE,
                depth_or_array_layers: crate::texture::LAYERS as u32,
            },
            mip_level_count: crate::texture::SIZE.ilog2() + 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("textures"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        (texture, bg)
    }

    /// Copies pictures added to the registry since the last frame, with their mips.
    fn upload_textures(&mut self, queue: &wgpu::Queue) {
        for (k, image) in crate::texture::images_from(self.textures_loaded)
            .into_iter()
            .enumerate()
        {
            let layer = (self.textures_loaded + k) as u32;
            let mut level = (*image).clone();
            for mip in 0.. {
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &self.textures,
                        mip_level: mip,
                        origin: wgpu::Origin3d {
                            x: 0,
                            y: 0,
                            z: layer,
                        },
                        aspect: wgpu::TextureAspect::All,
                    },
                    &level,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(4 * level.width()),
                        rows_per_image: Some(level.height()),
                    },
                    wgpu::Extent3d {
                        width: level.width(),
                        height: level.height(),
                        depth_or_array_layers: 1,
                    },
                );
                if level.width() == 1 {
                    break;
                }
                level = image::imageops::resize(
                    &level,
                    level.width() / 2,
                    level.height() / 2,
                    image::imageops::FilterType::Triangle,
                );
            }
        }
        self.textures_loaded += crate::texture::images_from(self.textures_loaded).len();
    }

    fn object_buffer(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        capacity: u64,
    ) -> (wgpu::Buffer, wgpu::BindGroup) {
        let buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("objects"),
            size: capacity * OBJECT_STRIDE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("objects"),
            layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &buf,
                    offset: 0,
                    size: wgpu::BufferSize::new(std::mem::size_of::<ObjectUniform>() as u64),
                }),
            }],
        });
        (buf, bg)
    }

    fn upload_mesh(device: &wgpu::Device, mesh: &Arc<Mesh>) -> GpuMesh {
        let shading = mesh.shading(40.0);
        let verts: Vec<Vertex> = shading
            .positions
            .iter()
            .zip(&shading.normals)
            .zip(&shading.colors)
            .zip(shading.uvs.iter().zip(&shading.tex))
            .map(|(((p, n), c), (uv, tex))| Vertex {
                pos: p.to_array(),
                normal: n.to_array(),
                color: *c,
                uv: *uv,
                tex: *tex,
            })
            .collect();
        let vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh vertices"),
            contents: bytemuck::cast_slice(&verts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let ibuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh indices"),
            contents: bytemuck::cast_slice(&shading.triangles),
            usage: wgpu::BufferUsages::INDEX,
        });
        GpuMesh {
            _keep: mesh.clone(),
            vbuf,
            ibuf,
            index_count: (shading.triangles.len() * 3) as u32,
            bounds: mesh.aabb(),
            lines: None,
            triangles: shading.triangles,
        }
    }

    fn line_indices(triangles: &[[u32; 3]]) -> Vec<u32> {
        let mut seen = HashSet::with_capacity(triangles.len() * 2);
        let mut out = Vec::with_capacity(triangles.len() * 3);
        for t in triangles {
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                if seen.insert((a.min(b), a.max(b))) {
                    out.extend([a, b]);
                }
            }
        }
        out
    }
}

fn mat(m: Mat4) -> [[f32; 4]; 4] {
    m.to_cols_array_2d()
}
