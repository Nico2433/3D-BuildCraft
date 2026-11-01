//! Pipeline, layout and buffer creation for the viewport [`Renderer`].

use super::shader::SHADER;
use super::*;

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
        let texture_group = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("textures"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let textured_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("textured mesh"),
            bind_group_layouts: &[&globals_layout, &object_layout, &texture_group],
            push_constant_ranges: &[],
        });
        let (textures, texture_bg) = Self::texture_array(device, &texture_group);
        let floor_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("floor"),
            bind_group_layouts: &[&globals_layout],
            push_constant_ranges: &[],
        });

        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Unorm8x4, 3 => Float32x2, 4 => Uint32],
        };
        let pipeline = |label: &str,
                        layout: &wgpu::PipelineLayout,
                        vs: &str,
                        fs: &str,
                        buffers: &[wgpu::VertexBufferLayout],
                        topology: wgpu::PrimitiveTopology,
                        depth_write: bool,
                        depth_compare: Option<wgpu::CompareFunction>| {
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
                    ..Default::default()
                },
                depth_stencil: depth_compare.map(|compare| wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: depth_write,
                    depth_compare: compare,
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: if depth_compare.is_some() {
                        MSAA_SAMPLES
                    } else {
                        1
                    },
                    ..Default::default()
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: if depth_compare.is_some() {
                            target
                        } else {
                            wgpu::TextureFormat::R8Unorm
                        },
                        blend: depth_compare
                            .is_some()
                            .then_some(wgpu::BlendState::ALPHA_BLENDING),
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
            &textured_layout,
            "vs",
            "fs",
            std::slice::from_ref(&vertex_layout),
            tri,
            true,
            Some(wgpu::CompareFunction::LessEqual),
        );
        let xray = pipeline(
            "xray",
            &textured_layout,
            "vs",
            "fs",
            std::slice::from_ref(&vertex_layout),
            tri,
            false,
            Some(wgpu::CompareFunction::LessEqual),
        );
        let wire = pipeline(
            "wire",
            &mesh_layout,
            "vs_wire",
            "fs_wire",
            std::slice::from_ref(&vertex_layout),
            wgpu::PrimitiveTopology::LineList,
            false,
            Some(wgpu::CompareFunction::LessEqual),
        );
        let mask_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("selection texture"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let screen_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("selection screen"),
            bind_group_layouts: &[&mask_layout],
            push_constant_ranges: &[],
        });
        let mask_pipeline = pipeline(
            "selection mask",
            &mesh_layout,
            "vs_mask",
            "fs_mask",
            std::slice::from_ref(&vertex_layout),
            tri,
            false,
            None,
        );
        let dilate = pipeline(
            "selection dilation",
            &screen_layout,
            "vs_screen",
            "fs_dilate",
            &[],
            tri,
            false,
            None,
        );
        let outline = pipeline(
            "selection outline",
            &screen_layout,
            "vs_screen",
            "fs_outline",
            &[],
            tri,
            false,
            Some(wgpu::CompareFunction::Always),
        );
        let shadow = pipeline(
            "shadow",
            &mesh_layout,
            "vs_shadow",
            "fs_shadow",
            &[],
            tri,
            false,
            Some(wgpu::CompareFunction::LessEqual),
        );
        let floor = pipeline(
            "floor",
            &floor_layout,
            "vs_floor",
            "fs_floor",
            &[],
            tri,
            false,
            Some(wgpu::CompareFunction::LessEqual),
        );

        let slab = pipeline(
            "slab",
            &mesh_layout,
            "vs_slab",
            "fs_slab",
            &[],
            tri,
            false,
            Some(wgpu::CompareFunction::LessEqual),
        );

        let object_capacity = 64;
        let (object_buf, object_bg) = Self::object_buffer(device, &object_layout, object_capacity);
        Self {
            solid,
            xray,
            wire,
            outline,
            mask_pipeline,
            dilate,
            mask_layout,
            selection: None,
            shadow,
            floor,
            slab,
            globals_buf,
            globals_bg,
            object_layout,
            object_buf,
            object_bg,
            object_capacity,
            object_bytes: Vec::new(),
            meshes: HashMap::new(),
            textures,
            texture_bg,
            textures_loaded: 0,
        }
    }
}
