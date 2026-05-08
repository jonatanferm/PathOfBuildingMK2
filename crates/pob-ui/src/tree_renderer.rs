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
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct NodeInstance {
    pub world_pos: [f32; 2],
    pub world_radius: f32,
    pub kind: u32,
    /// Bitfield: 0=allocated, 1=search-match, 2=hovered, 3=path.
    pub state: u32,
    /// Padding to a 16-byte aligned size keeps the buffer layout identical
    /// between bytemuck slicing and `wgpu::vertex_attr_array!`.
    pub _pad: u32,
}

/// State bits — keep in sync with the shader.
pub mod state_bits {
    pub const ALLOCATED: u32 = 1 << 0;
    pub const SEARCH: u32 = 1 << 1;
    pub const HOVERED: u32 = 1 << 2;
    pub const PATH: u32 = 1 << 3;
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

pub struct TreeRenderer {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    uniform_buffer: wgpu::Buffer,
    instance_buffer: wgpu::Buffer,
    instance_capacity: u64,
    bind_group: wgpu::BindGroup,
}

impl TreeRenderer {
    /// Compile the pipeline and stash the renderer in `callback_resources`. Call
    /// once during app construction; subsequent frames look up the renderer by
    /// type-id from `callback_resources`.
    pub fn install(render_state: &egui_wgpu::RenderState) {
        let device = &render_state.device;

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("tree_nodes.wgsl"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../shaders/tree_nodes.wgsl").into(),
            ),
        });

        let bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("tree_nodes.bind_group_layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        let pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("tree_nodes.pipeline_layout"),
                bind_group_layouts: &[&bind_group_layout],
                push_constant_ranges: &[],
            });

        // Match the shader's Instance struct; offsets are explicit so we don't
        // accidentally drift if the struct grows.
        let instance_attrs = wgpu::vertex_attr_array![
            0 => Float32x2, // world_pos
            1 => Float32,   // world_radius
            2 => Uint32,    // kind
            3 => Uint32,    // state
        ];
        let instance_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<NodeInstance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &instance_attrs,
        };

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("tree_nodes.pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[instance_layout],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: render_state.target_format,
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
        });

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("tree_nodes.uniform_buffer"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let instance_capacity = INITIAL_INSTANCE_CAPACITY;
        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("tree_nodes.instance_buffer"),
            size: instance_capacity * std::mem::size_of::<NodeInstance>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("tree_nodes.bind_group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        let renderer = Self {
            pipeline,
            bind_group_layout,
            uniform_buffer,
            instance_buffer,
            instance_capacity,
            bind_group,
        };

        render_state
            .renderer
            .write()
            .callback_resources
            .insert(renderer);
    }

    fn ensure_capacity(&mut self, device: &wgpu::Device, needed: u64) {
        if needed <= self.instance_capacity {
            return;
        }
        // Grow geometrically so frequent re-allocations don't churn.
        let new_capacity = needed.next_power_of_two().max(self.instance_capacity * 2);
        self.instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("tree_nodes.instance_buffer"),
            size: new_capacity * std::mem::size_of::<NodeInstance>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.instance_capacity = new_capacity;
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
        renderer.ensure_capacity(device, self.instances.len() as u64);
        renderer.write_uniforms(
            queue,
            self.viewport_center,
            self.zoom,
            self.viewport_size,
            self.pixels_per_point,
        );
        renderer.write_instances(queue, &self.instances);
        // Rebind in case ensure_capacity replaced the instance buffer (the
        // bind group only references the uniform buffer, so it stays valid;
        // but we recreate it defensively).
        renderer.bind_group =
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("tree_nodes.bind_group"),
                layout: &renderer.bind_group_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: renderer.uniform_buffer.as_entire_binding(),
                }],
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
        render_pass.set_pipeline(&renderer.pipeline);
        render_pass.set_bind_group(0, &renderer.bind_group, &[]);
        render_pass.set_vertex_buffer(0, renderer.instance_buffer.slice(..));
        // 6 vertices (two triangles) per instance.
        render_pass.draw(0..6, 0..self.instances.len() as u32);
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
