//! wgpu viewport drawn inside egui through a paint callback. Runs on DX12, Vulkan and
//! Metal through wgpu (ADR-0001).

use crate::mesh::Mesh;
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
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ObjectUniform {
    model: [[f32; 4]; 4],
    normal: [[f32; 4]; 4],
    color: [f32; 4],
    /// x selected, y pass (0 object, 1 reflection, 2 shadow)
    extra: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    pos: [f32; 3],
    normal: [f32; 3],
    /// Painted colour, alpha 0 for the object's.
    color: [u8; 4],
    /// The normals of every corner at this position, averaged, so the selection outline
    /// pushes creases out without gaps.
    smooth: [f32; 3],
}

/// What one frame draws. Built by the UI, consumed by the callback.
#[derive(Clone)]
pub struct DrawItem {
    pub mesh: Arc<Mesh>,
    pub model: Mat4,
    pub color: [u8; 4],
    pub selected: bool,
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

pub struct FrameCallback {
    pub items: Vec<DrawItem>,
    pub view_proj: Mat4,
    pub eye: Vec3,
    pub flags: ViewFlags,
    /// Size of the viewport in pixels.
    pub viewport: glam::Vec2,
}

struct GpuMesh {
    _keep: Arc<Mesh>,
    vbuf: wgpu::Buffer,
    ibuf: wgpu::Buffer,
    index_count: u32,
    /// Drawn triangles, for the wireframe built on first use.
    triangles: Vec<[u32; 3]>,
    lines: Option<(wgpu::Buffer, u32)>,
}

pub struct Renderer {
    solid: wgpu::RenderPipeline,
    xray: wgpu::RenderPipeline,
    wire: wgpu::RenderPipeline,
    outline: wgpu::RenderPipeline,
    shadow: wgpu::RenderPipeline,
    floor: wgpu::RenderPipeline,
    globals_buf: wgpu::Buffer,
    globals_bg: wgpu::BindGroup,
    object_layout: wgpu::BindGroupLayout,
    object_buf: wgpu::Buffer,
    object_bg: wgpu::BindGroup,
    object_capacity: u64,
    meshes: HashMap<usize, GpuMesh>,
}

const SHADER: &str = r#"
struct Globals { view_proj: mat4x4<f32>, eye: vec4<f32>, light: vec4<f32>, flags: vec4<f32>, flags2: vec4<f32>, viewport: vec4<f32> };
struct Obj { model: mat4x4<f32>, normal: mat4x4<f32>, color: vec4<f32>, extra: vec4<f32> };
@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var<uniform> o: Obj;

struct VOut { @builtin(position) pos: vec4<f32>, @location(0) wpos: vec3<f32>, @location(1) n: vec3<f32>, @location(2) c: vec4<f32> };

@vertex fn vs(@location(0) p: vec3<f32>, @location(1) n: vec3<f32>, @location(2) c: vec4<f32>) -> VOut {
    let w = o.model * vec4<f32>(p, 1.0);
    var out: VOut;
    out.pos = g.view_proj * w;
    out.wpos = w.xyz;
    out.n = (o.normal * vec4<f32>(n, 0.0)).xyz;
    out.c = c;
    return out;
}

@fragment fn fs(in: VOut) -> @location(0) vec4<f32> {
    let flat_n = normalize(cross(dpdx(in.wpos), dpdy(in.wpos)));
    // Degenerate faces have no normal; use the screen-space one.
    var n = flat_n;
    if (length(in.n) > 1e-6) { n = normalize(in.n); }
    if (g.flags.y < 0.5) { n = flat_n; }
    let v = normalize(g.eye.xyz - in.wpos);
    if (dot(n, v) < 0.0) { n = -n; }
    var base = o.color.rgb;
    // See-through items (the scene behind Custom or an import) keep their one colour.
    if (in.c.a > 0.001 && o.color.a > 0.99) { base = in.c.rgb; }
    if (g.flags.w < 0.5) { base = vec3<f32>(0.82, 0.82, 0.82); }
    var c = base;
    if (g.flags.x > 0.5) {
        let l = normalize(g.light.xyz);
        let diff = max(dot(n, l), 0.0);
        let fill = max(dot(n, normalize(vec3<f32>(0.5, 0.6, 0.4))), 0.0) * 0.2;
        let h = normalize(l + v);
        let spec = pow(max(dot(n, h), 0.0), 48.0) * 0.12;
        c = base * (0.40 + 0.42 * diff + fill) + vec3<f32>(spec);
    }
    var a = o.color.a;
    if (g.flags.z > 0.5) { a = 0.35; }
    if (o.extra.y > 0.5) {
        // Reflection under the floor, fading with depth. Parts of the object below the
        // floor would mirror above it; they are not drawn.
        if (in.wpos.z > 0.0) { discard; }
        a = a * 0.5 * clamp(1.0 + in.wpos.z / 45.0, 0.0, 1.0);
    }
    return vec4<f32>(c, a);
}

// Selection outline: the back faces pushed out a few pixels on screen, so only a rim
// shows around the object, as the original's light blue contour.
@vertex fn vs_outline(@location(0) p: vec3<f32>, @location(3) sn: vec3<f32>) -> @builtin(position) vec4<f32> {
    var pos = g.view_proj * (o.model * vec4<f32>(p, 1.0));
    let dir = (g.view_proj * vec4<f32>((o.normal * vec4<f32>(sn, 0.0)).xyz, 0.0)).xy;
    if (length(dir) > 1e-6) {
        pos = vec4<f32>(pos.xy + normalize(dir) * 4.0 * 2.0 / g.viewport.xy * pos.w, pos.zw);
    }
    return pos;
}

@fragment fn fs_outline() -> @location(0) vec4<f32> {
    return vec4<f32>(0.62, 0.78, 0.92, 1.0);
}

struct SOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };

@vertex fn vs_shadow(@builtin(vertex_index) i: u32) -> SOut {
    var corners = array<vec2<f32>, 6>(vec2(-1.0, -1.0), vec2(1.0, -1.0), vec2(1.0, 1.0), vec2(-1.0, -1.0), vec2(1.0, 1.0), vec2(-1.0, 1.0));
    var out: SOut;
    out.pos = g.view_proj * (o.model * vec4<f32>(corners[i], 0.0, 1.0));
    out.uv = corners[i];
    return out;
}

@fragment fn fs_shadow(in: SOut) -> @location(0) vec4<f32> {
    // A soft rounded patch under the object.
    let d = length(max(abs(in.uv) - vec2<f32>(0.5), vec2<f32>(0.0))) / 0.5;
    return vec4<f32>(0.0, 0.0, 0.0, 0.2 * (1.0 - smoothstep(0.0, 1.0, d)));
}

@vertex fn vs_wire(@location(0) p: vec3<f32>, @location(1) n: vec3<f32>) -> @builtin(position) vec4<f32> {
    var pos = g.view_proj * (o.model * vec4<f32>(p, 1.0));
    pos.z = pos.z - 0.00002 * pos.w;
    return pos;
}

@fragment fn fs_wire() -> @location(0) vec4<f32> {
    return vec4<f32>(0.05, 0.08, 0.12, 0.85);
}

struct FOut { @builtin(position) pos: vec4<f32>, @location(0) w: vec2<f32> };

@vertex fn vs_floor(@builtin(vertex_index) i: u32) -> FOut {
    var corners = array<vec2<f32>, 6>(vec2(-1.0, -1.0), vec2(1.0, -1.0), vec2(1.0, 1.0), vec2(-1.0, -1.0), vec2(1.0, 1.0), vec2(-1.0, 1.0));
    let w = corners[i] * 3000.0;
    var out: FOut;
    out.pos = g.view_proj * vec4<f32>(w, -0.02, 1.0);
    out.w = w;
    return out;
}

@fragment fn fs_floor(in: FOut) -> @location(0) vec4<f32> {
    let cell = floor(in.w / 10.0);
    let odd = (i32(cell.x) + i32(cell.y)) & 1;
    var c = select(0.375, 0.445, odd == 1);
    // Light and dark tones from the grid properties.
    if (g.flags2.z > 1.5) { c = select(0.30, 0.34, odd == 1); }
    else if (g.flags2.z > 0.5) { c = select(0.56, 0.62, odd == 1); }
    let d = length(in.w - g.eye.xy);
    let fade = 1.0 - smoothstep(400.0, 2600.0, d);
    // Slightly see-through so reflections show under it.
    let opacity = select(1.0, 0.88, g.flags2.y > 0.5);
    return vec4<f32>(vec3<f32>(c), fade * opacity * g.flags2.x);
}
"#;

impl Renderer {
    pub fn new(device: &wgpu::Device, target: wgpu::TextureFormat) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("viewport"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let uniform_entry = |dynamic: bool, size: u64| wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: dynamic,
                min_binding_size: wgpu::BufferSize::new(size),
            },
            count: None,
        };
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals"),
            entries: &[uniform_entry(false, std::mem::size_of::<Globals>() as u64)],
        });
        let object_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("object"),
            entries: &[uniform_entry(
                true,
                std::mem::size_of::<ObjectUniform>() as u64,
            )],
        });
        let globals_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals_buf.as_entire_binding(),
            }],
        });
        let mesh_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mesh"),
            bind_group_layouts: &[&globals_layout, &object_layout],
            push_constant_ranges: &[],
        });
        let floor_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("floor"),
            bind_group_layouts: &[&globals_layout],
            push_constant_ranges: &[],
        });

        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Unorm8x4, 3 => Float32x3],
        };
        let pipeline = |label: &str,
                        layout: &wgpu::PipelineLayout,
                        vs: &str,
                        fs: &str,
                        buffers: &[wgpu::VertexBufferLayout],
                        topology: wgpu::PrimitiveTopology,
                        depth_write: bool,
                        cull_mode: Option<wgpu::Face>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some(vs),
                    compilation_options: Default::default(),
                    buffers,
                },
                primitive: wgpu::PrimitiveState {
                    topology,
                    cull_mode,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: depth_write,
                    depth_compare: wgpu::CompareFunction::LessEqual,
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: MSAA_SAMPLES,
                    ..Default::default()
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: target,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview: None,
                cache: None,
            })
        };
        let tri = wgpu::PrimitiveTopology::TriangleList;
        let solid = pipeline(
            "solid",
            &mesh_layout,
            "vs",
            "fs",
            std::slice::from_ref(&vertex_layout),
            tri,
            true,
            None,
        );
        let xray = pipeline(
            "xray",
            &mesh_layout,
            "vs",
            "fs",
            std::slice::from_ref(&vertex_layout),
            tri,
            false,
            None,
        );
        let wire = pipeline(
            "wire",
            &mesh_layout,
            "vs_wire",
            "fs_wire",
            std::slice::from_ref(&vertex_layout),
            wgpu::PrimitiveTopology::LineList,
            false,
            None,
        );
        let outline = pipeline(
            "outline",
            &mesh_layout,
            "vs_outline",
            "fs_outline",
            std::slice::from_ref(&vertex_layout),
            tri,
            true,
            Some(wgpu::Face::Front),
        );
        let shadow = pipeline(
            "shadow",
            &mesh_layout,
            "vs_shadow",
            "fs_shadow",
            &[],
            tri,
            false,
            None,
        );
        let floor = pipeline(
            "floor",
            &floor_layout,
            "vs_floor",
            "fs_floor",
            &[],
            tri,
            false,
            None,
        );

        let object_capacity = 64;
        let (object_buf, object_bg) = Self::object_buffer(device, &object_layout, object_capacity);
        Self {
            solid,
            xray,
            wire,
            outline,
            shadow,
            floor,
            globals_buf,
            globals_bg,
            object_layout,
            object_buf,
            object_bg,
            object_capacity,
            meshes: HashMap::new(),
        }
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
        let key = |p: &Vec3| p.to_array().map(f32::to_bits);
        let mut smooth: HashMap<[u32; 3], Vec3> = HashMap::new();
        for (p, n) in shading.positions.iter().zip(&shading.normals) {
            *smooth.entry(key(p)).or_default() += *n;
        }
        let verts: Vec<Vertex> = shading
            .positions
            .iter()
            .zip(&shading.normals)
            .zip(&shading.colors)
            .map(|((p, n), c)| Vertex {
                pos: p.to_array(),
                normal: n.to_array(),
                color: *c,
                smooth: smooth[&key(p)].normalize_or_zero().to_array(),
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

impl CallbackTrait for FrameCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let Some(r) = resources.get_mut::<Renderer>() else {
            return vec![];
        };
        let f = |b: bool| if b { 1.0 } else { 0.0 };
        // From the front left, as in the original.
        let light = (Vec3::new(-0.35, -0.55, 0.75).normalize()).extend(0.0);
        let globals = Globals {
            view_proj: mat(self.view_proj),
            eye: self.eye.extend(1.0).to_array(),
            light: light.to_array(),
            flags: [
                f(self.flags.shading),
                f(self.flags.smoothing),
                f(self.flags.xray),
                f(self.flags.colors),
            ],
            flags2: [
                f(self.flags.floor),
                f(self.flags.reflections),
                self.flags.floor_tone as f32,
                0.0,
            ],
            viewport: [self.viewport.x, self.viewport.y, 0.0, 0.0],
        };
        queue.write_buffer(&r.globals_buf, 0, bytemuck::bytes_of(&globals));

        // Three slots per item: the object, its reflection and its shadow.
        let n = self.items.len();
        if 3 * n as u64 > r.object_capacity {
            r.object_capacity = (3 * n as u64).next_power_of_two();
            let (b, bg) = Renderer::object_buffer(device, &r.object_layout, r.object_capacity);
            r.object_buf = b;
            r.object_bg = bg;
        }
        let mut bytes = vec![0u8; 3 * n * OBJECT_STRIDE as usize];
        let mut live = HashSet::new();
        for (i, it) in self.items.iter().enumerate() {
            let c = it.color;
            let color = Vec4::new(c[0] as f32, c[1] as f32, c[2] as f32, c[3] as f32)
                .to_array()
                .map(|x| x / 255.0);
            let mirrored = Mat4::from_scale(Vec3::new(1.0, 1.0, -1.0)) * it.model;
            // The shadow patch: the footprint, a little larger and pushed away from the light.
            let b = crate::scene::cached_aabb(&it.mesh, &it.model);
            let half = ((b.max - b.min) * 0.5).truncate() * 1.2 + glam::Vec2::splat(2.0);
            let centre = ((b.min + b.max) * 0.5).truncate()
                - light.truncate().truncate() * 0.15 * half.length();
            let patch =
                Mat4::from_translation(centre.extend(0.01)) * Mat4::from_scale(half.extend(1.0));
            for (slot, model, pass) in [
                (i, it.model, 0.0),
                (n + i, mirrored, 1.0),
                (2 * n + i, patch, 2.0),
            ] {
                let u = ObjectUniform {
                    model: mat(model),
                    normal: mat(model.inverse().transpose()),
                    color,
                    extra: [f(it.selected), pass, 0.0, 0.0],
                };
                let o = slot * OBJECT_STRIDE as usize;
                bytes[o..o + std::mem::size_of::<ObjectUniform>()]
                    .copy_from_slice(bytemuck::bytes_of(&u));
            }

            let key = Arc::as_ptr(&it.mesh) as usize;
            live.insert(key);
            let gm = r
                .meshes
                .entry(key)
                .or_insert_with(|| Renderer::upload_mesh(device, &it.mesh));
            if self.flags.wireframe && gm.lines.is_none() {
                let idx = Renderer::line_indices(&gm.triangles);
                let buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("mesh lines"),
                    contents: bytemuck::cast_slice(&idx),
                    usage: wgpu::BufferUsages::INDEX,
                });
                gm.lines = Some((buf, idx.len() as u32));
            }
        }
        if !bytes.is_empty() {
            queue.write_buffer(&r.object_buf, 0, &bytes);
        }
        r.meshes.retain(|k, _| live.contains(k));
        vec![]
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &CallbackResources,
    ) {
        let Some(r) = resources.get::<Renderer>() else {
            return;
        };
        pass.set_bind_group(0, &r.globals_bg, &[]);
        let n = self.items.len() as u64;
        let slot = |s: u64| [(s * OBJECT_STRIDE) as u32];
        if self.flags.floor && self.flags.reflections && !self.flags.xray {
            pass.set_pipeline(&r.solid);
            for (i, it) in self.items.iter().enumerate() {
                let Some(gm) = r.meshes.get(&(Arc::as_ptr(&it.mesh) as usize)) else {
                    continue;
                };
                if gm.index_count == 0 {
                    continue;
                }
                pass.set_bind_group(1, &r.object_bg, &slot(n + i as u64));
                pass.set_vertex_buffer(0, gm.vbuf.slice(..));
                pass.set_index_buffer(gm.ibuf.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..gm.index_count, 0, 0..1);
            }
        }
        if self.flags.floor {
            pass.set_pipeline(&r.floor);
            pass.draw(0..6, 0..1);
        }
        if self.flags.floor && self.flags.shadows {
            pass.set_pipeline(&r.shadow);
            for i in 0..n {
                pass.set_bind_group(1, &r.object_bg, &slot(2 * n + i));
                pass.draw(0..6, 0..1);
            }
        }
        pass.set_pipeline(if self.flags.xray { &r.xray } else { &r.solid });
        for (i, it) in self.items.iter().enumerate() {
            let Some(gm) = r.meshes.get(&(Arc::as_ptr(&it.mesh) as usize)) else {
                continue;
            };
            if gm.index_count == 0 {
                continue;
            }
            pass.set_bind_group(1, &r.object_bg, &[(i as u64 * OBJECT_STRIDE) as u32]);
            pass.set_vertex_buffer(0, gm.vbuf.slice(..));
            pass.set_index_buffer(gm.ibuf.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..gm.index_count, 0, 0..1);
        }
        if self.flags.wireframe {
            pass.set_pipeline(&r.wire);
            // Not on see-through items: the wire belongs to what is being made.
            for (i, it) in self
                .items
                .iter()
                .enumerate()
                .filter(|(_, it)| it.color[3] == 255)
            {
                let Some(gm) = r.meshes.get(&(Arc::as_ptr(&it.mesh) as usize)) else {
                    continue;
                };
                let Some((lines, count)) = &gm.lines else {
                    continue;
                };
                if *count == 0 {
                    continue;
                }
                pass.set_bind_group(1, &r.object_bg, &[(i as u64 * OBJECT_STRIDE) as u32]);
                pass.set_vertex_buffer(0, gm.vbuf.slice(..));
                pass.set_index_buffer(lines.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..*count, 0, 0..1);
            }
        }
        pass.set_pipeline(&r.outline);
        for (i, it) in self.items.iter().enumerate().filter(|(_, it)| it.selected) {
            let Some(gm) = r.meshes.get(&(Arc::as_ptr(&it.mesh) as usize)) else {
                continue;
            };
            if gm.index_count == 0 {
                continue;
            }
            pass.set_bind_group(1, &r.object_bg, &slot(i as u64));
            pass.set_vertex_buffer(0, gm.vbuf.slice(..));
            pass.set_index_buffer(gm.ibuf.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..gm.index_count, 0, 0..1);
        }
    }
}
