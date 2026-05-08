//! wgpu-backed tree node renderer.
//!
//! Phase 8a: takes over node circle rendering from egui shapes. Edges, search
//! highlight rings (drawn into the node SDF directly via the state byte), and
//! tooltips stay on egui paths for now — see `tree_view.rs`.
//!
//! Lifecycle:
//! - `TreeRenderer::install` runs once at app boot, compiles the WGSL pipeline
//!   from `shaders/tree_nodes.wgsl`, and stashes the renderer in
//!   `egui_wgpu::CallbackResources` so per-frame paint callbacks can find it.
//! - Each frame `tree_view.rs` builds a `TreeNodeCallback` carrying the node
//!   instance buffer + uniforms for that frame and adds it to the painter.
//!   `prepare()` uploads the buffers; `paint()` issues the draw call.
//!
//! The instance count can grow over time (different tree versions have
//! different node counts), so we re-create the GPU buffer if it's smaller than
//! the frame's instance vector.

use bytemuck::{Pod, Zeroable};
use eframe::{egui_wgpu, wgpu};

/// Per-instance vertex data — must match `Instance` in `tree_nodes.wgsl`.
/// `vertex_attr_array!` packs attribute offsets back-to-back, so this struct
/// must do the same — no padding fields.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct NodeInstance {
    pub world_pos: [f32; 2],
    pub world_radius: f32,
    pub kind: u32,
    /// Bitfield: 0=allocated, 1=search-match, 2=hovered, 3=path.
    pub state: u32,
    /// Atlas UV rect in [0, 1]: (u, v, du, dv). Zero means no icon — the
    /// shader falls back to the flat kind-color circle.
    pub icon_uv: [f32; 4],
}

/// Per-instance vertex data for an edge — matches `Edge` in `tree_edges.wgsl`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct EdgeInstance {
    pub a: [f32; 2],
    pub b: [f32; 2],
    /// Bitfield: 0=both endpoints allocated, 1=path overlay.
    pub state: u32,
    pub _pad: u32,
}

/// Per-instance vertex data for a group-background quad. Sampled from
/// `group-background.png`. Matches `Instance` in `tree_groups.wgsl`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct GroupInstance {
    pub world_pos: [f32; 2],
    pub world_size: [f32; 2],
    pub uv_rect: [f32; 4],
}

/// State bits — keep in sync with the shader.
pub mod state_bits {
    pub const ALLOCATED: u32 = 1 << 0;
    pub const SEARCH: u32 = 1 << 1;
    pub const HOVERED: u32 = 1 << 2;
    pub const PATH: u32 = 1 << 3;
}

pub mod edge_state_bits {
    pub const ALLOCATED: u32 = 1 << 0;
    pub const PATH: u32 = 1 << 1;
}

/// Uniform block — must match the WGSL `Uniforms` struct.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    viewport_center: [f32; 2],
    zoom: f32,
    _pad0: f32,
    viewport_size: [f32; 2],
    pixels_per_point: f32,
    _pad1: f32,
}

const INITIAL_INSTANCE_CAPACITY: u64 = 4096;
const INITIAL_EDGE_CAPACITY: u64 = 8192;
const INITIAL_GROUP_CAPACITY: u64 = 1024;

pub struct TreeRenderer {
    node_pipeline: wgpu::RenderPipeline,
    edge_pipeline: wgpu::RenderPipeline,
    group_pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    group_bind_group_layout: wgpu::BindGroupLayout,
    uniform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    group_bind_group: wgpu::BindGroup,
    instance_buffer: wgpu::Buffer,
    instance_capacity: u64,
    edge_buffer: wgpu::Buffer,
    edge_capacity: u64,
    group_buffer: wgpu::Buffer,
    group_capacity: u64,
    /// Cached views + sampler so we can rebuild bind groups in `prepare`
    /// after a buffer reallocation (the bindings must reference the same
    /// objects; we keep them around).
    atlas_active_view: wgpu::TextureView,
    atlas_inactive_view: wgpu::TextureView,
    atlas_sampler: wgpu::Sampler,
    group_atlas_view: wgpu::TextureView,
}

/// Inputs to `TreeRenderer::install`: the two skill atlases plus the group-
/// background atlas, all as RGBA8 byte arrays + their dimensions.
pub struct AtlasInputs {
    pub active_rgba8: Vec<u8>,
    pub active_size: (u32, u32),
    pub inactive_rgba8: Vec<u8>,
    pub inactive_size: (u32, u32),
    pub group_rgba8: Vec<u8>,
    pub group_size: (u32, u32),
}

fn upload_atlas(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    rgba: &[u8],
    size: (u32, u32),
    label: &str,
) -> wgpu::Texture {
    let extent = wgpu::Extent3d {
        width: size.0,
        height: size.1,
        depth_or_array_layers: 1,
    };
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::ImageCopyTexture {
            texture: &tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        rgba,
        wgpu::ImageDataLayout {
            offset: 0,
            bytes_per_row: Some(4 * size.0),
            rows_per_image: Some(size.1),
        },
        extent,
    );
    tex
}

impl TreeRenderer {
    /// Compile the pipeline and stash the renderer in `callback_resources`. Call
    /// once during app construction; subsequent frames look up the renderer by
    /// type-id from `callback_resources`.
    pub fn install(render_state: &egui_wgpu::RenderState, atlases: AtlasInputs) {
        let device = &render_state.device;
        let queue = &render_state.queue;

        let active_tex = upload_atlas(device, queue, &atlases.active_rgba8, atlases.active_size, "atlas_active");
        let inactive_tex = upload_atlas(device, queue, &atlases.inactive_rgba8, atlases.inactive_size, "atlas_inactive");
        let group_tex = upload_atlas(device, queue, &atlases.group_rgba8, atlases.group_size, "atlas_group");
        let active_view = active_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let inactive_view = inactive_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let group_view = group_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("tree.atlas_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("tree.bind_group_layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });

        let pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("tree.pipeline_layout"),
                bind_group_layouts: &[&bind_group_layout],
                push_constant_ranges: &[],
            });

        let node_pipeline = build_node_pipeline(device, &pipeline_layout, render_state.target_format);
        let edge_pipeline = build_edge_pipeline(device, &pipeline_layout, render_state.target_format);

        // Group-background pipeline uses a 3-binding layout: uniform + atlas
        // + sampler (the node pipeline binds two atlases; we don't need the
        // active/inactive split here).
        let group_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("tree.group_bind_group_layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });
        let group_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("tree.group_pipeline_layout"),
                bind_group_layouts: &[&group_bind_group_layout],
                push_constant_ranges: &[],
            });
        let group_pipeline = build_group_pipeline(
            device,
            &group_pipeline_layout,
            render_state.target_format,
        );

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("tree.uniform_buffer"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let instance_capacity = INITIAL_INSTANCE_CAPACITY;
        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("tree.node_instance_buffer"),
            size: instance_capacity * std::mem::size_of::<NodeInstance>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let edge_capacity = INITIAL_EDGE_CAPACITY;
        let edge_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("tree.edge_instance_buffer"),
            size: edge_capacity * std::mem::size_of::<EdgeInstance>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let group_capacity = INITIAL_GROUP_CAPACITY;
        let group_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("tree.group_instance_buffer"),
            size: group_capacity * std::mem::size_of::<GroupInstance>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("tree.bind_group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&active_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&inactive_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        let group_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("tree.group_bind_group"),
            layout: &group_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&group_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        let renderer = Self {
            node_pipeline,
            edge_pipeline,
            group_pipeline,
            bind_group_layout,
            group_bind_group_layout,
            uniform_buffer,
            bind_group,
            group_bind_group,
            instance_buffer,
            instance_capacity,
            edge_buffer,
            edge_capacity,
            group_buffer,
            group_capacity,
            atlas_active_view: active_view,
            atlas_inactive_view: inactive_view,
            atlas_sampler: sampler,
            group_atlas_view: group_view,
        };

        render_state
            .renderer
            .write()
            .callback_resources
            .insert(renderer);
    }

    fn ensure_node_capacity(&mut self, device: &wgpu::Device, needed: u64) {
        if needed <= self.instance_capacity {
            return;
        }
        let new_capacity = needed.next_power_of_two().max(self.instance_capacity * 2);
        self.instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("tree.node_instance_buffer"),
            size: new_capacity * std::mem::size_of::<NodeInstance>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.instance_capacity = new_capacity;
    }

    fn ensure_edge_capacity(&mut self, device: &wgpu::Device, needed: u64) {
        if needed <= self.edge_capacity {
            return;
        }
        let new_capacity = needed.next_power_of_two().max(self.edge_capacity * 2);
        self.edge_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("tree.edge_instance_buffer"),
            size: new_capacity * std::mem::size_of::<EdgeInstance>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.edge_capacity = new_capacity;
    }

    fn ensure_group_capacity(&mut self, device: &wgpu::Device, needed: u64) {
        if needed <= self.group_capacity {
            return;
        }
        let new_capacity = needed.next_power_of_two().max(self.group_capacity * 2);
        self.group_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("tree.group_instance_buffer"),
            size: new_capacity * std::mem::size_of::<GroupInstance>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.group_capacity = new_capacity;
    }

    fn write_uniforms(
        &self,
        queue: &wgpu::Queue,
        viewport_center: [f32; 2],
        zoom: f32,
        viewport_size: [f32; 2],
        pixels_per_point: f32,
    ) {
        let u = Uniforms {
            viewport_center,
            zoom,
            _pad0: 0.0,
            viewport_size,
            pixels_per_point,
            _pad1: 0.0,
        };
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&u));
    }

    fn write_instances(&self, queue: &wgpu::Queue, instances: &[NodeInstance]) {
        if instances.is_empty() {
            return;
        }
        queue.write_buffer(&self.instance_buffer, 0, bytemuck::cast_slice(instances));
    }

    fn write_edges(&self, queue: &wgpu::Queue, edges: &[EdgeInstance]) {
        if edges.is_empty() {
            return;
        }
        queue.write_buffer(&self.edge_buffer, 0, bytemuck::cast_slice(edges));
    }

    fn write_groups(&self, queue: &wgpu::Queue, groups: &[GroupInstance]) {
        if groups.is_empty() {
            return;
        }
        queue.write_buffer(&self.group_buffer, 0, bytemuck::cast_slice(groups));
    }
}

fn build_node_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    target_format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("tree_nodes.wgsl"),
        source: wgpu::ShaderSource::Wgsl(
            include_str!("../shaders/tree_nodes.wgsl").into(),
        ),
    });
    let attrs = wgpu::vertex_attr_array![
        0 => Float32x2, // world_pos          @ 0
        1 => Float32,   // world_radius       @ 8
        2 => Uint32,    // kind               @ 12
        3 => Uint32,    // state              @ 16
        4 => Float32x4, // icon_uv            @ 20
    ];
    let buffer_layout = wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<NodeInstance>() as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &attrs,
    };
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("tree_nodes.pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: "vs_main",
            buffers: &[buffer_layout],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: "fs_main",
            targets: &[Some(wgpu::ColorTargetState {
                format: target_format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
}

fn build_group_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    target_format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("tree_groups.wgsl"),
        source: wgpu::ShaderSource::Wgsl(
            include_str!("../shaders/tree_groups.wgsl").into(),
        ),
    });
    let attrs = wgpu::vertex_attr_array![
        0 => Float32x2, // world_pos     @ 0
        1 => Float32x2, // world_size    @ 8
        2 => Float32x4, // uv_rect       @ 16
    ];
    let buffer_layout = wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<GroupInstance>() as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &attrs,
    };
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("tree_groups.pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: "vs_main",
            buffers: &[buffer_layout],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: "fs_main",
            targets: &[Some(wgpu::ColorTargetState {
                format: target_format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
}

fn build_edge_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    target_format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("tree_edges.wgsl"),
        source: wgpu::ShaderSource::Wgsl(
            include_str!("../shaders/tree_edges.wgsl").into(),
        ),
    });
    // EdgeInstance: a (vec2), b (vec2), state (u32), _pad (u32). Shader uses
    // locations 0 (a), 1 (b), 2 (state); the trailing pad word doesn't get a
    // shader binding.
    let attrs = wgpu::vertex_attr_array![
        0 => Float32x2, // a
        1 => Float32x2, // b
        2 => Uint32,    // state
    ];
    let buffer_layout = wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<EdgeInstance>() as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &attrs,
    };
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("tree_edges.pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: "vs_main",
            buffers: &[buffer_layout],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: "fs_main",
            targets: &[Some(wgpu::ColorTargetState {
                format: target_format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
}

/// Per-frame paint callback. Carries just the inputs needed for one draw —
/// the static geometry decisions (which node is which) live in the instance
/// vector built fresh each frame, and the uniforms describe the current camera.
pub struct TreeNodeCallback {
    pub instances: Vec<NodeInstance>,
    pub viewport_center: [f32; 2],
    pub zoom: f32,
    pub viewport_size: [f32; 2],
    pub pixels_per_point: f32,
}

impl egui_wgpu::CallbackTrait for TreeNodeCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen_descriptor: &egui_wgpu::ScreenDescriptor,
        _egui_encoder: &mut wgpu::CommandEncoder,
        callback_resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let Some(renderer) = callback_resources.get_mut::<TreeRenderer>() else {
            return Vec::new();
        };
        renderer.ensure_node_capacity(device, self.instances.len() as u64);
        renderer.write_uniforms(
            queue,
            self.viewport_center,
            self.zoom,
            self.viewport_size,
            self.pixels_per_point,
        );
        renderer.write_instances(queue, &self.instances);
        // Rebind defensively in case any of the buffers got reallocated.
        renderer.bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("tree.bind_group"),
            layout: &renderer.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: renderer.uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&renderer.atlas_active_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&renderer.atlas_inactive_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&renderer.atlas_sampler),
                },
            ],
        });
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        callback_resources: &egui_wgpu::CallbackResources,
    ) {
        let Some(renderer) = callback_resources.get::<TreeRenderer>() else {
            return;
        };
        if self.instances.is_empty() {
            return;
        }
        render_pass.set_pipeline(&renderer.node_pipeline);
        render_pass.set_bind_group(0, &renderer.bind_group, &[]);
        render_pass.set_vertex_buffer(0, renderer.instance_buffer.slice(..));
        render_pass.draw(0..6, 0..self.instances.len() as u32);
    }
}

/// Group-background paint callback. Drawn FIRST so it sits beneath edges
/// + nodes — like PoB's `renderGroup` pass.
pub struct TreeGroupCallback {
    pub groups: Vec<GroupInstance>,
    pub viewport_center: [f32; 2],
    pub zoom: f32,
    pub viewport_size: [f32; 2],
    pub pixels_per_point: f32,
}

impl egui_wgpu::CallbackTrait for TreeGroupCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen_descriptor: &egui_wgpu::ScreenDescriptor,
        _egui_encoder: &mut wgpu::CommandEncoder,
        callback_resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let Some(renderer) = callback_resources.get_mut::<TreeRenderer>() else {
            return Vec::new();
        };
        renderer.ensure_group_capacity(device, self.groups.len() as u64);
        renderer.write_uniforms(
            queue,
            self.viewport_center,
            self.zoom,
            self.viewport_size,
            self.pixels_per_point,
        );
        renderer.write_groups(queue, &self.groups);
        renderer.group_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("tree.group_bind_group"),
            layout: &renderer.group_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: renderer.uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&renderer.group_atlas_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&renderer.atlas_sampler),
                },
            ],
        });
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        callback_resources: &egui_wgpu::CallbackResources,
    ) {
        let Some(renderer) = callback_resources.get::<TreeRenderer>() else {
            return;
        };
        if self.groups.is_empty() {
            return;
        }
        render_pass.set_pipeline(&renderer.group_pipeline);
        render_pass.set_bind_group(0, &renderer.group_bind_group, &[]);
        render_pass.set_vertex_buffer(0, renderer.group_buffer.slice(..));
        render_pass.draw(0..6, 0..self.groups.len() as u32);
    }
}

/// Edge paint callback. Drawn before nodes so node SDFs cover edge endpoints.
pub struct TreeEdgeCallback {
    pub edges: Vec<EdgeInstance>,
    pub viewport_center: [f32; 2],
    pub zoom: f32,
    pub viewport_size: [f32; 2],
    pub pixels_per_point: f32,
}

impl egui_wgpu::CallbackTrait for TreeEdgeCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen_descriptor: &egui_wgpu::ScreenDescriptor,
        _egui_encoder: &mut wgpu::CommandEncoder,
        callback_resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let Some(renderer) = callback_resources.get_mut::<TreeRenderer>() else {
            return Vec::new();
        };
        renderer.ensure_edge_capacity(device, self.edges.len() as u64);
        renderer.write_uniforms(
            queue,
            self.viewport_center,
            self.zoom,
            self.viewport_size,
            self.pixels_per_point,
        );
        renderer.write_edges(queue, &self.edges);
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        callback_resources: &egui_wgpu::CallbackResources,
    ) {
        let Some(renderer) = callback_resources.get::<TreeRenderer>() else {
            return;
        };
        if self.edges.is_empty() {
            return;
        }
        render_pass.set_pipeline(&renderer.edge_pipeline);
        render_pass.set_bind_group(0, &renderer.bind_group, &[]);
        render_pass.set_vertex_buffer(0, renderer.edge_buffer.slice(..));
        render_pass.draw(0..6, 0..self.edges.len() as u32);
    }
}

/// Map a `pob_data::NodeKind` to the integer the WGSL fragment shader expects.
pub fn kind_to_u32(kind: pob_data::NodeKind) -> u32 {
    match kind {
        pob_data::NodeKind::Normal => 0,
        pob_data::NodeKind::Notable => 1,
        pob_data::NodeKind::Keystone => 2,
        pob_data::NodeKind::Mastery => 3,
        pob_data::NodeKind::JewelSocket => 4,
        pob_data::NodeKind::Root => 5,
        pob_data::NodeKind::ClassStart => 6,
        pob_data::NodeKind::AscendancyStart => 7,
        pob_data::NodeKind::Tattoo => 8,
        pob_data::NodeKind::Blighted => 9,
    }
}
