//! `pob-sg-wgpu`: draws a [`pob_sg::Frame`] (ui doc §5.1). Owner: U6.
//!
//! Port of SimpleGraphic `engine/render/r_main.cpp` `AdjacentMergeStrategy` and
//! `r_layer_c::Render` plus the texture setup in `r_texture.cpp` `r_tex_c::Upload`
//! (research copy master @ 8694a07; U6 re-verifies at `3b1a346`):
//! - the sorted queues are replayed: `Viewport`/`Bind`/`Color` are state, every `Quad` becomes
//!   two triangles `0,1,2 / 0,2,3` of the TL,TR,BR,BL corners offset by the viewport origin;
//! - a new draw call starts whenever the scissor (viewport) or texture changes; each call is
//!   scissored to its viewport, clamped to the target;
//! - quads outside their viewport are culled (`r_drawCull` = 1);
//! - a texture that has no pixels yet binds `@black` (transparent), like SG's not-DONE textures;
//! - textures and the render target are RGBA8 *UNORM*, so blending happens in gamma space
//!   (`glBlendFunc(GL_SRC_ALPHA, GL_ONE_MINUS_SRC_ALPHA)` for colour and alpha);
//! - the scene goes into an Rgba8Unorm offscreen target at framebuffer size, cleared to opaque
//!   black, then a full-screen blit writes it to the (non-sRGB) output view;
//! - sampling: `CLAMP` -> clamp-to-edge else repeat; `NEAREST` -> nearest magnification else
//!   linear; minification linear, trilinear when the texture has mips.
//!
//! Moved from the pre-port `pob-sg/src/gpu/mod.rs` (which drew a flat `DrawCmd` list straight
//! into the target) by UI-U0 and adapted to the §5.1 surface. Works with the WebGL2 backend (no
//! storage buffers, one texture per bind group, u32 indices, 16-byte uniform).
//!
//! Deltas U6 closes: anisotropy (16 when all filters are linear) is not set yet; T5 against the
//! CPU raster.
#![allow(clippy::too_many_lines)]

use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};
use pob_sg::{Cmd, Frame, IRect, ImageData, ImageFlags, TexId};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    pos: [f32; 2],
    uv: [f32; 2],
    tint: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Globals {
    screen: [f32; 2],
    _pad: [f32; 2],
}

struct GpuTex {
    bind_group: wgpu::BindGroup,
    _texture: wgpu::Texture,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct SamplerKey {
    clamp: bool,
    nearest: bool,
    mips: bool,
}

struct Batch {
    tex: TexId,
    viewport: IRect,
    first_index: u32,
    index_count: u32,
}

struct RenderTarget {
    size: (u32, u32),
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    blit_bg: wgpu::BindGroup,
}

/// Statistics of the last prepared frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameStats {
    pub quads: usize,
    pub culled: usize,
    pub skipped_loading: usize,
    pub batches: usize,
}

/// The wgpu renderer: create once per device; `prepare` + `render` per frame.
pub struct SgRenderer {
    pipeline: wgpu::RenderPipeline,
    blit_pipeline: wgpu::RenderPipeline,
    tex_layout: wgpu::BindGroupLayout,
    blit_layout: wgpu::BindGroupLayout,
    blit_sampler: wgpu::Sampler,
    globals_buf: wgpu::Buffer,
    globals_bg: wgpu::BindGroup,
    samplers: HashMap<SamplerKey, wgpu::Sampler>,
    textures: HashMap<TexId, GpuTex>,
    vbuf: Option<wgpu::Buffer>,
    ibuf: Option<wgpu::Buffer>,
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
    batches: Vec<Batch>,
    rt: Option<RenderTarget>,
    last_digest: u64,
    max_tex_dim: u32,
    pub stats: FrameStats,
}

/// The Rgba8Unorm offscreen target format (D12: no sRGB anywhere).
pub const RT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

fn alpha_blend() -> wgpu::BlendState {
    // SimpleGraphic applies glBlendFunc to colour and alpha alike.
    let c = wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::SrcAlpha,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    };
    wgpu::BlendState { color: c, alpha: c }
}

fn tex_layout_entries() -> [wgpu::BindGroupLayoutEntry; 2] {
    [
        wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
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
    ]
}

impl SgRenderer {
    pub fn new(
        device: &wgpu::Device,
        adapter_limits: &wgpu::Limits,
        output: wgpu::TextureFormat,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("pob-sg quad"),
            source: wgpu::ShaderSource::Wgsl(include_str!("quad.wgsl").into()),
        });
        let blit_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("pob-sg blit"),
            source: wgpu::ShaderSource::Wgsl(include_str!("blit.wgsl").into()),
        });
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("pob-sg globals"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let tex_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("pob-sg texture"),
            entries: &tex_layout_entries(),
        });
        let blit_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("pob-sg blit"),
            entries: &tex_layout_entries(),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pob-sg"),
            bind_group_layouts: &[Some(&globals_layout), Some(&tex_layout)],
            immediate_size: 0,
        });
        let blit_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pob-sg blit"),
            bind_group_layouts: &[Some(&blit_layout)],
            immediate_size: 0,
        });
        let attrs = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("pob-sg quad"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &attrs,
                })],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: RT_FORMAT,
                    blend: Some(alpha_blend()),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let blit_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("pob-sg blit"),
            layout: Some(&blit_pl),
            vertex: wgpu::VertexState {
                module: &blit_shader,
                entry_point: Some("vs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &blit_shader,
                entry_point: Some("fs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: output,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let globals_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("pob-sg globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("pob-sg globals"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals_buf.as_entire_binding(),
            }],
        });
        let blit_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("pob-sg blit"),
            ..Default::default()
        });
        SgRenderer {
            pipeline,
            blit_pipeline,
            tex_layout,
            blit_layout,
            blit_sampler,
            globals_buf,
            globals_bg,
            samplers: HashMap::new(),
            textures: HashMap::new(),
            vbuf: None,
            ibuf: None,
            vertices: Vec::new(),
            indices: Vec::new(),
            batches: Vec::new(),
            rt: None,
            last_digest: 0,
            max_tex_dim: adapter_limits.max_texture_dimension_2d,
            stats: FrameStats::default(),
        }
    }

    fn sampler(&mut self, device: &wgpu::Device, key: SamplerKey) -> wgpu::Sampler {
        self.samplers
            .entry(key)
            .or_insert_with(|| {
                let addr = if key.clamp {
                    wgpu::AddressMode::ClampToEdge
                } else {
                    wgpu::AddressMode::Repeat
                };
                device.create_sampler(&wgpu::SamplerDescriptor {
                    label: Some("pob-sg"),
                    address_mode_u: addr,
                    address_mode_v: addr,
                    address_mode_w: addr,
                    mag_filter: if key.nearest {
                        wgpu::FilterMode::Nearest
                    } else {
                        wgpu::FilterMode::Linear
                    },
                    min_filter: wgpu::FilterMode::Linear,
                    mipmap_filter: if key.mips {
                        wgpu::MipmapFilterMode::Linear
                    } else {
                        wgpu::MipmapFilterMode::Nearest
                    },
                    lod_min_clamp: 0.0,
                    lod_max_clamp: if key.mips { 32.0 } else { 0.0 },
                    compare: None,
                    anisotropy_clamp: 1,
                    border_color: None,
                })
            })
            .clone()
    }

    fn create_texture(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        data: &ImageData,
    ) -> GpuTex {
        let max = self.max_tex_dim;
        let fitted;
        let data = if data.width > max || data.height > max {
            // SimpleGraphic BuildMipSet shrinks to GL_MAX_TEXTURE_SIZE; we resample the top level.
            fitted = fit_texture(data, max);
            &fitted
        } else {
            data
        };
        let levels = data.levels.len() as u32;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("pob-sg texture"),
            size: wgpu::Extent3d {
                width: data.width,
                height: data.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: levels,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let (mut w, mut h) = (data.width, data.height);
        for (level, px) in data.levels.iter().enumerate() {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: level as u32,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                px,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4 * w),
                    rows_per_image: Some(h),
                },
                wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
            );
            w = (w / 2).max(1);
            h = (h / 2).max(1);
        }
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = self.sampler(
            device,
            SamplerKey {
                clamp: data.flags.contains(ImageFlags::CLAMP),
                nearest: data.flags.contains(ImageFlags::NEAREST),
                mips: levels > 1,
            },
        );
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("pob-sg texture"),
            layout: &self.tex_layout,
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
        GpuTex {
            bind_group,
            _texture: texture,
        }
    }

    fn ensure_rt(&mut self, device: &wgpu::Device, size: (u32, u32)) {
        if self.rt.as_ref().is_some_and(|rt| rt.size == size) {
            return;
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("pob-sg render target"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: RT_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let blit_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("pob-sg blit"),
            layout: &self.blit_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.blit_sampler),
                },
            ],
        });
        self.rt = Some(RenderTarget {
            size,
            texture,
            view,
            blit_bg,
        });
    }

    /// Uploads (downscaling above max_tex_dim), builds vertices and draw calls.
    pub fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, frame: &Frame) {
        for id in &frame.releases {
            self.textures.remove(id);
        }
        for (id, data) in &frame.uploads {
            let gt = self.create_texture(device, queue, data);
            self.textures.insert(*id, gt);
        }
        let size = (
            frame.screen.fb_w.max(1) as u32,
            frame.screen.fb_h.max(1) as u32,
        );
        self.ensure_rt(device, size);
        self.last_digest = frame.digest;
        self.vertices.clear();
        self.indices.clear();
        self.batches.clear();
        let mut stats = FrameStats::default();
        let full = IRect {
            x: 0,
            y: 0,
            w: frame.screen.fb_w,
            h: frame.screen.fb_h,
        };
        for q in &frame.layers {
            let (mut vp, mut tex, mut tint) = (full, TexId::WHITE, [1.0f32; 4]);
            for c in &q.cmds {
                match c {
                    Cmd::Viewport(v) => vp = *v,
                    Cmd::Bind(t) => tex = *t,
                    Cmd::Color(c) => tint = *c,
                    Cmd::Quad(quad) => {
                        stats.quads += 1;
                        if quad.is_culled(vp) {
                            stats.culled += 1;
                            continue;
                        }
                        // A texture without pixels yet binds @black (r_texture.cpp:340-347).
                        let bound = if self.textures.contains_key(&tex) {
                            tex
                        } else {
                            TexId::BLACK
                        };
                        if !self.textures.contains_key(&bound) {
                            stats.skipped_loading += 1;
                            continue;
                        }
                        let base = self.vertices.len() as u32;
                        let (ox, oy) = (vp.x as f32, vp.y as f32);
                        for v in 0..4 {
                            self.vertices.push(Vertex {
                                pos: [quad.x[v] + ox, quad.y[v] + oy],
                                uv: [quad.s[v], quad.t[v]],
                                tint,
                            });
                        }
                        let first = self.indices.len() as u32;
                        self.indices.extend_from_slice(&[
                            base,
                            base + 1,
                            base + 2,
                            base,
                            base + 2,
                            base + 3,
                        ]);
                        match self.batches.last_mut() {
                            Some(b) if b.tex == bound && b.viewport == vp => b.index_count += 6,
                            _ => self.batches.push(Batch {
                                tex: bound,
                                viewport: vp,
                                first_index: first,
                                index_count: 6,
                            }),
                        }
                    }
                }
            }
        }
        stats.batches = self.batches.len();
        self.stats = stats;
        let vbytes: &[u8] = bytemuck::cast_slice(&self.vertices);
        let ibytes: &[u8] = bytemuck::cast_slice(&self.indices);
        let need_v = (vbytes.len() as u64).max(64);
        let need_i = (ibytes.len() as u64).max(64);
        if self.vbuf.as_ref().map_or(true, |b| b.size() < need_v) {
            self.vbuf = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("pob-sg vertices"),
                size: need_v.next_power_of_two(),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        if self.ibuf.as_ref().map_or(true, |b| b.size() < need_i) {
            self.ibuf = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("pob-sg indices"),
                size: need_i.next_power_of_two(),
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        if !vbytes.is_empty() {
            queue.write_buffer(self.vbuf.as_ref().unwrap(), 0, vbytes);
            queue.write_buffer(self.ibuf.as_ref().unwrap(), 0, ibytes);
        }
        let g = Globals {
            screen: [size.0 as f32, size.1 as f32],
            _pad: [0.0; 2],
        };
        queue.write_buffer(&self.globals_buf, 0, bytemuck::bytes_of(&g));
    }

    /// Clears the RT to opaque black, renders, then blits to `output` (a non-sRGB view).
    pub fn render(&mut self, encoder: &mut wgpu::CommandEncoder, output: &wgpu::TextureView) {
        let Some(rt) = &self.rt else { return };
        let (w, h) = (rt.size.0 as i32, rt.size.1 as i32);
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("pob-sg scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &rt.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if !self.batches.is_empty() {
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &self.globals_bg, &[]);
                pass.set_vertex_buffer(0, self.vbuf.as_ref().unwrap().slice(..));
                pass.set_index_buffer(
                    self.ibuf.as_ref().unwrap().slice(..),
                    wgpu::IndexFormat::Uint32,
                );
                for b in &self.batches {
                    // glScissor(lo.x, H - extent.y - lo.y, extent.x, extent.y) in GL's bottom-left
                    // space == (lo, extent) top-left here, clamped to the target.
                    let x0 = b.viewport.x.clamp(0, w);
                    let y0 = b.viewport.y.clamp(0, h);
                    let x1 = b.viewport.x.saturating_add(b.viewport.w).clamp(0, w);
                    let y1 = b.viewport.y.saturating_add(b.viewport.h).clamp(0, h);
                    if x1 <= x0 || y1 <= y0 {
                        continue;
                    }
                    pass.set_scissor_rect(x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32);
                    pass.set_bind_group(1, &self.textures[&b.tex].bind_group, &[]);
                    pass.draw_indexed(b.first_index..b.first_index + b.index_count, 0, 0..1);
                }
            }
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("pob-sg blit"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: output,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.blit_pipeline);
        pass.set_bind_group(0, &rt.blit_bg, &[]);
        pass.draw(0..3, 0..1);
    }

    /// Reads the render target back as tightly packed RGBA8 (alpha forced to 255: SG's target is RGB).
    pub fn read_back(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> (u32, u32, Vec<u8>) {
        let Some(rt) = &self.rt else {
            return (0, 0, Vec::new());
        };
        let size = rt.size;
        let row = 4 * size.0;
        let padded =
            row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("pob-sg readback"),
            size: u64::from(padded) * u64::from(size.1),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("pob-sg readback"),
        });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &rt.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(size.1),
                },
            },
            wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(Some(enc.finish()));
        let slice = buf.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        let mut out = Vec::with_capacity((row * size.1) as usize);
        {
            let data = slice.get_mapped_range().expect("map readback buffer");
            for y in 0..size.1 {
                let s = (y * padded) as usize;
                out.extend_from_slice(&data[s..s + row as usize]);
            }
        }
        buf.unmap();
        for px in out.chunks_exact_mut(4) {
            px[3] = 255;
        }
        (size.0, size.1, out)
    }

    /// Digest of the last prepared frame (frame elision is optional, ui doc §2).
    pub fn last_digest(&self) -> u64 {
        self.last_digest
    }
}

fn fit_texture(data: &ImageData, max: u32) -> ImageData {
    let scale = f64::from(max) / f64::from(data.width.max(data.height));
    let nw = ((f64::from(data.width) * scale).floor() as u32).clamp(1, max);
    let nh = ((f64::from(data.height) * scale).floor() as u32).clamp(1, max);
    let img = image::RgbaImage::from_raw(data.width, data.height, data.levels[0].clone())
        .expect("rgba size");
    let resized =
        image::imageops::resize(&img, nw, nh, image::imageops::FilterType::Triangle).into_raw();
    let levels = if data.levels.len() > 1 {
        pob_sg::image::build_mips(nw, nh, resized)
    } else {
        vec![resized]
    };
    ImageData {
        width: nw,
        height: nh,
        levels,
        flags: data.flags,
    }
}

/// Request an adapter + device suitable for this renderer: WebGL2 downlevel limits raised to the
/// adapter's texture size limits. Returns the adapter limits too (for [`SgRenderer::new`]).
pub async fn request_device(
    instance: &wgpu::Instance,
    surface: Option<&wgpu::Surface<'_>>,
) -> Result<(wgpu::Adapter, wgpu::Device, wgpu::Queue), String> {
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            force_fallback_adapter: false,
            compatible_surface: surface,
            apply_limit_buckets: false,
        })
        .await
        .map_err(|e| format!("no suitable GPU adapter: {e}"))?;
    let limits = wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits());
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("pob-sg"),
            required_features: wgpu::Features::empty(),
            required_limits: limits,
            ..Default::default()
        })
        .await
        .map_err(|e| format!("request_device: {e}"))?;
    Ok((adapter, device, queue))
}

/// Pick a non-sRGB surface format (gamma-space blending, like SimpleGraphic).
/// Returns `(configure_format, render_view_format)`: if only sRGB formats are offered, configure
/// `view_formats = [fmt.remove_srgb_suffix()]` and render into that view (ui doc §5.1).
pub fn pick_surface_format(
    caps: &wgpu::SurfaceCapabilities,
) -> (wgpu::TextureFormat, wgpu::TextureFormat) {
    if let Some(f) = caps.formats.iter().copied().find(|f| !f.is_srgb()) {
        return (f, f);
    }
    let f = caps
        .formats
        .first()
        .copied()
        .unwrap_or(wgpu::TextureFormat::Bgra8Unorm);
    (f, f.remove_srgb_suffix())
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use pob_sg::{FontSet, ImageHandle, ImageLoader, ImageRegistry, LoadEvent, ScreenInfo, Sg};
    use std::rc::Rc;

    /// Loader serving one 2x1 NEAREST test texture synchronously (deterministic mode).
    struct TestLoader;
    impl ImageLoader for TestLoader {
        fn request(
            &mut self,
            id: TexId,
            _path: &str,
            flags: ImageFlags,
            _sync: bool,
            _priority: i32,
        ) -> Option<LoadEvent> {
            // Left red, right blue.
            Some(LoadEvent::Done(
                id,
                ImageData {
                    width: 2,
                    height: 1,
                    levels: vec![vec![255, 0, 0, 255, 0, 0, 255, 255]],
                    flags,
                },
            ))
        }
        fn read_size(&mut self, _id: TexId, _path: &str) -> Option<(u32, u32)> {
            Some((2, 1))
        }
        fn set_priority(&mut self, _id: TexId, _priority: i32) {}
        fn poll(&mut self) -> Vec<LoadEvent> {
            Vec::new()
        }
    }

    fn gpu() -> Option<(wgpu::Device, wgpu::Queue, SgRenderer)> {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        match pollster::block_on(request_device(&instance, None)) {
            Ok((a, d, q)) => {
                let r = SgRenderer::new(&d, &a.limits(), RT_FORMAT);
                Some((d, q, r))
            }
            Err(e) => {
                eprintln!("SKIP: no GPU adapter: {e}");
                None
            }
        }
    }

    fn draw(d: &wgpu::Device, q: &wgpu::Queue, r: &mut SgRenderer, frame: &Frame) -> Vec<u8> {
        r.prepare(d, q, frame);
        let out = d.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: frame.screen.fb_w as u32,
                height: frame.screen.fb_h as u32,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: RT_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = out.create_view(&wgpu::TextureViewDescriptor::default());
        let mut enc = d.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        r.render(&mut enc, &view);
        q.submit(Some(enc.finish()));
        r.read_back(d, q).2
    }

    fn px(img: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * w + x) * 4) as usize;
        [img[i], img[i + 1], img[i + 2], img[i + 3]]
    }

    fn sg() -> Sg {
        Sg::new(
            Rc::new(FontSet::empty()),
            ImageRegistry::new(Box::new(TestLoader)),
        )
    }

    fn screen(w: i32, h: i32) -> ScreenInfo {
        ScreenInfo {
            fb_w: w,
            fb_h: h,
            win_h: h,
            dpi_scale: 1.0,
        }
    }

    #[test]
    fn solid_quads_gamma_blend_and_scissor() {
        let Some((d, q, mut r)) = gpu() else { return };
        let mut sg = sg();
        sg.begin_frame(screen(32, 32));
        sg.set_draw_color(1.0, 1.0, 1.0);
        sg.draw_image(None, 0.0, 0.0, 16.0, 32.0);
        // Drawn before the white quad's layer-0 successors but on layer 1: ends on top.
        sg.set_draw_layer(Some(1), None);
        sg.set_draw_color_a(1.0, 0.0, 0.0, 0.5);
        sg.draw_image(None, 0.0, 0.0, 32.0, 8.0);
        sg.set_draw_layer(Some(0), None);
        // Clipped to a viewport: only x in 20..24 is visible.
        sg.set_viewport(20.0, 16.0, 4.0, 16.0);
        sg.set_draw_color(0.0, 1.0, 0.0);
        sg.draw_image(None, -10.0, 0.0, 40.0, 16.0);
        sg.reset_viewport();
        let frame = sg.end_frame();
        let img = draw(&d, &q, &mut r, &frame);
        // Red 50% over white, blended in gamma space: (255, 127.5, 127.5).
        let p = px(&img, 32, 4, 4);
        assert_eq!(p[0], 255);
        assert!(
            (127..=128).contains(&p[1]) && (127..=128).contains(&p[2]),
            "{p:?}"
        );
        // Red 50% over the opaque black clear: (127.5, 0, 0).
        let p = px(&img, 32, 24, 4);
        assert!(
            (127..=128).contains(&p[0]) && p[1] == 0 && p[2] == 0,
            "{p:?}"
        );
        // White below the red strip.
        assert_eq!(px(&img, 32, 4, 12), [255, 255, 255, 255]);
        // Viewport clip.
        assert_eq!(px(&img, 32, 21, 20), [0, 255, 0, 255]);
        assert_eq!(px(&img, 32, 19, 20), [0, 0, 0, 255]);
        assert_eq!(px(&img, 32, 24, 20), [0, 0, 0, 255]);
        assert_eq!(r.stats.batches, 3);
    }

    #[test]
    fn textured_quad_repeat() {
        let Some((d, q, mut r)) = gpu() else { return };
        let mut sg = sg();
        // 2x1 texture: left red, right blue; NEAREST so samples are exact.
        let h = ImageHandle::default();
        h.load(&mut sg, "t.png", ImageFlags::NEAREST);
        sg.begin_frame(screen(8, 2));
        // tc 0..2 across 8 px with REPEAT: R R B B R R B B (texel centres).
        sg.draw_image_tc(Some(&h), 0.0, 0.0, 8.0, 2.0, 0.0, 0.0, 2.0, 1.0);
        let frame = sg.end_frame();
        let img = draw(&d, &q, &mut r, &frame);
        assert_eq!(px(&img, 8, 0, 0), [255, 0, 0, 255]);
        assert_eq!(px(&img, 8, 3, 0), [0, 0, 255, 255]);
        assert_eq!(px(&img, 8, 4, 0), [255, 0, 0, 255]);
        assert_eq!(px(&img, 8, 7, 0), [0, 0, 255, 255]);
    }
}
