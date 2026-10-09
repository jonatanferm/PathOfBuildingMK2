//! wgpu backend: draws a sorted [`DrawCmd`] list.
//!
//! Port of SimpleGraphic `engine/render/r_main.cpp` `AdjacentMergeStrategy`
//! and `r_layer_c::Render` (master @ 8694a07) plus the texture setup in
//! `engine/render/r_texture.cpp` `r_tex_c::Upload`:
//! - every quad becomes two triangles `0,1,2 / 0,2,3` of the TL,TR,BR,BL
//!   corners, offset by the viewport origin, tinted by the quad colour;
//! - adjacent quads with the same texture, viewport and blend mode are merged
//!   into one draw call; each call is scissored to its viewport;
//! - quads outside their viewport are culled (`r_drawCull` = 1);
//! - textures that are still loading are bound as the transparent `blackTex`
//!   in SimpleGraphic, so such quads are skipped here;
//! - textures are RGBA8 *UNORM* and the render target is UNORM too, so
//!   blending happens in gamma space, like the GL renderer (GL_RGB render
//!   target, `glBlendFunc(GL_SRC_ALPHA, GL_ONE_MINUS_SRC_ALPHA)`);
//! - sampling: `CLAMP` -> clamp-to-edge else repeat; `NEAREST` -> nearest
//!   magnification else linear; minification linear, trilinear
//!   (`GL_LINEAR_MIPMAP_LINEAR`) when the texture has mips.
//!
//! Works with the WebGL2 backend (no storage buffers, one texture per bind
//! group, u32 indices, 16-byte uniform).

use std::collections::HashMap;
use std::sync::Arc;

use bytemuck::{Pod, Zeroable};

use crate::font::{Font, FontSet};
use crate::images::{Images, LoadFlags, TextureData, TextureId};
use crate::sg::{BlendMode, DrawCmd, TexRef, Viewport};

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct Vertex {
    pos: [f32; 2],
    uv: [f32; 2],
    color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct Uniforms {
    screen_size: [f32; 2],
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
    tex: TexRef,
    viewport: Viewport,
    blend: BlendMode,
    first_index: u32,
    index_count: u32,
}

/// Statistics of the last rendered frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameStats {
    pub quads: usize,
    pub culled: usize,
    pub skipped_loading: usize,
    pub batches: usize,
}

/// The wgpu renderer. Create once per device; call [`Renderer::render`] per frame.
pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    images: Images,
    fonts: Option<Arc<FontSet>>,
    shader: wgpu::ShaderModule,
    pipeline_layout: wgpu::PipelineLayout,
    tex_layout: wgpu::BindGroupLayout,
    uniform_buf: wgpu::Buffer,
    uniform_bg: wgpu::BindGroup,
    pipelines: HashMap<(wgpu::TextureFormat, BlendMode), wgpu::RenderPipeline>,
    samplers: HashMap<SamplerKey, wgpu::Sampler>,
    textures: HashMap<TexRef, GpuTex>,
    failed_fonts: Vec<(Font, u8)>,
    vbuf: Option<wgpu::Buffer>,
    ibuf: Option<wgpu::Buffer>,
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
    batches: Vec<Batch>,
    pub stats: FrameStats,
}

fn blend_state(mode: BlendMode) -> wgpu::BlendState {
    use wgpu::{BlendComponent, BlendFactor, BlendOperation};
    let comp = |src, dst| BlendComponent { src_factor: src, dst_factor: dst, operation: BlendOperation::Add };
    // SimpleGraphic applies glBlendFunc to colour and alpha alike.
    let c = match mode {
        BlendMode::Alpha => comp(BlendFactor::SrcAlpha, BlendFactor::OneMinusSrcAlpha),
        BlendMode::PreAlpha => comp(BlendFactor::One, BlendFactor::OneMinusSrcAlpha),
        BlendMode::Additive => comp(BlendFactor::One, BlendFactor::One),
    };
    wgpu::BlendState { color: c, alpha: c }
}

impl Renderer {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, images: Images, fonts: Option<Arc<FontSet>>) -> Renderer {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("pob-sg quad"),
            source: wgpu::ShaderSource::Wgsl(include_str!("quad.wgsl").into()),
        });
        let uniform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("pob-sg uniforms"),
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
            entries: &[
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
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pob-sg"),
            bind_group_layouts: &[Some(&uniform_layout), Some(&tex_layout)],
            immediate_size: 0,
        });
        let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("pob-sg uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let uniform_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("pob-sg uniforms"),
            layout: &uniform_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniform_buf.as_entire_binding() }],
        });
        let mut r = Renderer {
            device,
            queue,
            images,
            fonts,
            shader,
            pipeline_layout,
            tex_layout,
            uniform_buf,
            uniform_bg,
            pipelines: HashMap::new(),
            samplers: HashMap::new(),
            textures: HashMap::new(),
            failed_fonts: Vec::new(),
            vbuf: None,
            ibuf: None,
            vertices: Vec::new(),
            indices: Vec::new(),
            batches: Vec::new(),
            stats: FrameStats::default(),
        };
        // whiteImage: 8x8 white, TF_NOMIPMAP, repeat, linear (r_texture.cpp t_whiteImage).
        let white = TextureData { width: 8, height: 8, levels: vec![vec![255u8; 8 * 8 * 4]], flags: LoadFlags::NONE };
        let gt = r.create_texture(&white, "white");
        r.textures.insert(TexRef::White, gt);
        r
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    fn pipeline(&mut self, format: wgpu::TextureFormat, blend: BlendMode) -> &wgpu::RenderPipeline {
        let key = (format, blend);
        if !self.pipelines.contains_key(&key) {
            let attrs = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4];
            let p = self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("pob-sg quad"),
                layout: Some(&self.pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &self.shader,
                    entry_point: Some("vs_main"),
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
                    module: &self.shader,
                    entry_point: Some("fs_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(blend_state(blend)),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            });
            self.pipelines.insert(key, p);
        }
        &self.pipelines[&key]
    }

    fn sampler(&mut self, key: SamplerKey) -> wgpu::Sampler {
        self.samplers
            .entry(key)
            .or_insert_with(|| {
                let addr = if key.clamp { wgpu::AddressMode::ClampToEdge } else { wgpu::AddressMode::Repeat };
                self.device.create_sampler(&wgpu::SamplerDescriptor {
                    label: Some("pob-sg"),
                    address_mode_u: addr,
                    address_mode_v: addr,
                    address_mode_w: addr,
                    mag_filter: if key.nearest { wgpu::FilterMode::Nearest } else { wgpu::FilterMode::Linear },
                    min_filter: wgpu::FilterMode::Linear,
                    mipmap_filter: if key.mips { wgpu::MipmapFilterMode::Linear } else { wgpu::MipmapFilterMode::Nearest },
                    lod_min_clamp: 0.0,
                    lod_max_clamp: if key.mips { 32.0 } else { 0.0 },
                    compare: None,
                    anisotropy_clamp: 1,
                    border_color: None,
                })
            })
            .clone()
    }

    fn create_texture(&mut self, data: &TextureData, label: &str) -> GpuTex {
        let max = self.device.limits().max_texture_dimension_2d;
        let fitted;
        let data = if data.width > max || data.height > max {
            // SimpleGraphic BuildMipSet shrinks to GL_MAX_TEXTURE_SIZE by
            // dropping mip levels; we resample the top level instead.
            fitted = fit_texture(data, max);
            &fitted
        } else {
            data
        };
        let levels = data.levels.len() as u32;
        let size = wgpu::Extent3d { width: data.width, height: data.height, depth_or_array_layers: 1 };
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size,
            mip_level_count: levels,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let (mut w, mut h) = (data.width, data.height);
        for (level, px) in data.levels.iter().enumerate() {
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: level as u32,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                px,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * w), rows_per_image: Some(h) },
                wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            );
            w = (w / 2).max(1);
            h = (h / 2).max(1);
        }
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = self.sampler(SamplerKey {
            clamp: data.flags.contains(LoadFlags::CLAMP),
            nearest: data.flags.contains(LoadFlags::NEAREST),
            mips: levels > 1,
        });
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout: &self.tex_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) },
            ],
        });
        GpuTex { bind_group, _texture: texture }
    }

    /// Upload newly decoded images and free released ones.
    pub fn sync_images(&mut self) {
        for id in self.images.drain_freed() {
            self.textures.remove(&TexRef::Image(id));
        }
        for (id, data) in self.images.take_uploads() {
            let gt = self.create_texture(&data, "pob-sg image");
            self.textures.insert(TexRef::Image(id), gt);
        }
    }

    /// Make sure a texture for `tex` exists (font atlases are decoded lazily).
    fn ensure(&mut self, tex: TexRef) -> bool {
        if self.textures.contains_key(&tex) {
            return true;
        }
        match tex {
            TexRef::Font { font, height_idx } => {
                if self.failed_fonts.contains(&(font, height_idx)) {
                    return false;
                }
                let Some(fonts) = self.fonts.clone() else { return false };
                match fonts.atlas_rgba(font, height_idx as usize) {
                    Ok(img) => {
                        // r_font.cpp loads atlases with TF_NOMIPMAP (repeat, linear).
                        let data = TextureData {
                            width: img.width,
                            height: img.height,
                            levels: vec![img.pixels],
                            flags: LoadFlags::NONE,
                        };
                        let gt = self.create_texture(&data, "pob-sg font");
                        self.textures.insert(tex, gt);
                        true
                    }
                    Err(e) => {
                        eprintln!("pob-sg: font atlas {font:?}/{height_idx}: {e}");
                        self.failed_fonts.push((font, height_idx));
                        false
                    }
                }
            }
            // Not uploaded yet: still loading (SimpleGraphic binds blackTex).
            TexRef::Image(_) | TexRef::White => false,
        }
    }

    /// Ensure every texture an image handle refers to is resident, if loaded.
    pub fn is_texture_resident(&self, id: TextureId) -> bool {
        self.textures.contains_key(&TexRef::Image(id))
    }

    fn build(&mut self, cmds: &[DrawCmd]) {
        self.vertices.clear();
        self.indices.clear();
        self.batches.clear();
        let mut stats = FrameStats::default();
        for c in cmds {
            stats.quads += 1;
            if c.is_culled() {
                stats.culled += 1;
                continue;
            }
            if !self.ensure(c.tex) {
                stats.skipped_loading += 1;
                continue;
            }
            let base = self.vertices.len() as u32;
            let (ox, oy) = (c.viewport.x as f32, c.viewport.y as f32);
            for v in 0..4 {
                self.vertices.push(Vertex {
                    pos: [c.pos[v * 2] + ox, c.pos[v * 2 + 1] + oy],
                    uv: [c.uv[v * 2], c.uv[v * 2 + 1]],
                    color: c.color,
                });
            }
            // 3-2
            // |/|
            // 0-1   indices { 0, 1, 2, 0, 2, 3 }
            let first = self.indices.len() as u32;
            self.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
            match self.batches.last_mut() {
                Some(b) if b.tex == c.tex && b.viewport == c.viewport && b.blend == c.blend => b.index_count += 6,
                _ => self.batches.push(Batch { tex: c.tex, viewport: c.viewport, blend: c.blend, first_index: first, index_count: 6 }),
            }
        }
        stats.batches = self.batches.len();
        self.stats = stats;
    }

    fn upload_geometry(&mut self) {
        let vbytes: &[u8] = bytemuck::cast_slice(&self.vertices);
        let ibytes: &[u8] = bytemuck::cast_slice(&self.indices);
        let need_v = (vbytes.len() as u64).max(64);
        let need_i = (ibytes.len() as u64).max(64);
        if self.vbuf.as_ref().map_or(true, |b| b.size() < need_v) {
            self.vbuf = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("pob-sg vertices"),
                size: need_v.next_power_of_two(),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        if self.ibuf.as_ref().map_or(true, |b| b.size() < need_i) {
            self.ibuf = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("pob-sg indices"),
                size: need_i.next_power_of_two(),
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        if !vbytes.is_empty() {
            self.queue.write_buffer(self.vbuf.as_ref().unwrap(), 0, vbytes);
            self.queue.write_buffer(self.ibuf.as_ref().unwrap(), 0, ibytes);
        }
    }

    /// Record and submit the frame into `target` (size `size`, format
    /// `format`, which should be a non-sRGB format). `clear` is the
    /// `SetClearColor` colour (default black).
    pub fn render(&mut self, cmds: &[DrawCmd], target: &wgpu::TextureView, format: wgpu::TextureFormat, size: (u32, u32), clear: [f32; 4]) {
        self.sync_images();
        self.build(cmds);
        self.upload_geometry();
        let u = Uniforms { screen_size: [size.0 as f32, size.1 as f32], _pad: [0.0; 2] };
        self.queue.write_buffer(&self.uniform_buf, 0, bytemuck::bytes_of(&u));
        let blends: Vec<BlendMode> = self.batches.iter().map(|b| b.blend).collect();
        for bm in blends {
            self.pipeline(format, bm);
        }
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("pob-sg") });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("pob-sg"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: f64::from(clear[0]),
                            g: f64::from(clear[1]),
                            b: f64::from(clear[2]),
                            a: f64::from(clear[3]),
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if !self.batches.is_empty() {
                pass.set_bind_group(0, &self.uniform_bg, &[]);
                pass.set_vertex_buffer(0, self.vbuf.as_ref().unwrap().slice(..));
                pass.set_index_buffer(self.ibuf.as_ref().unwrap().slice(..), wgpu::IndexFormat::Uint32);
                let mut cur_blend = None;
                for b in &self.batches {
                    // glScissor(lo.x, H - extent.y - lo.y, extent.x, extent.y)
                    // in GL's bottom-left space == (lo, extent) top-left here,
                    // clamped to the target as GL clamps to the framebuffer.
                    let x0 = b.viewport.x.clamp(0, size.0 as i32);
                    let y0 = b.viewport.y.clamp(0, size.1 as i32);
                    let x1 = (b.viewport.x.saturating_add(b.viewport.width)).clamp(0, size.0 as i32);
                    let y1 = (b.viewport.y.saturating_add(b.viewport.height)).clamp(0, size.1 as i32);
                    if x1 <= x0 || y1 <= y0 {
                        continue;
                    }
                    if cur_blend != Some(b.blend) {
                        pass.set_pipeline(&self.pipelines[&(format, b.blend)]);
                        cur_blend = Some(b.blend);
                    }
                    pass.set_scissor_rect(x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32);
                    pass.set_bind_group(1, &self.textures[&b.tex].bind_group, &[]);
                    pass.draw_indexed(b.first_index..b.first_index + b.index_count, 0, 0..1);
                }
            }
        }
        self.queue.submit(Some(encoder.finish()));
    }

    /// Render offscreen and read the result back as tightly packed RGBA8
    /// (alpha forced to 255, matching SimpleGraphic's GL_RGB render target).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn render_to_rgba(&mut self, cmds: &[DrawCmd], size: (u32, u32), clear: [f32; 4]) -> Vec<u8> {
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("pob-sg offscreen"),
            size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        self.render(cmds, &view, format, size, clear);
        let row = 4 * size.0;
        let padded = row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("pob-sg readback"),
            size: u64::from(padded) * u64::from(size.1),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("pob-sg readback") });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded), rows_per_image: Some(size.1) },
            },
            wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
        );
        self.queue.submit(Some(enc.finish()));
        let slice = buf.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
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
        out
    }
}

fn fit_texture(data: &TextureData, max: u32) -> TextureData {
    let scale = f64::from(max) / f64::from(data.width.max(data.height));
    let nw = ((f64::from(data.width) * scale).floor() as u32).clamp(1, max);
    let nh = ((f64::from(data.height) * scale).floor() as u32).clamp(1, max);
    let img = image::RgbaImage::from_raw(data.width, data.height, data.levels[0].clone()).expect("rgba size");
    let resized = image::imageops::resize(&img, nw, nh, image::imageops::FilterType::Triangle).into_raw();
    let levels = if data.levels.len() > 1 { crate::images::build_mips(nw, nh, resized) } else { vec![resized] };
    TextureData { width: nw, height: nh, levels, flags: data.flags }
}

/// Request an adapter + device suitable for this renderer. Limits are WebGL2
/// downlevel defaults raised to the adapter's texture size limits.
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
/// Returns `(configure_format, render_view_format)`.
pub fn pick_surface_format(caps: &wgpu::SurfaceCapabilities) -> (wgpu::TextureFormat, wgpu::TextureFormat) {
    if let Some(f) = caps.formats.iter().copied().find(|f| !f.is_srgb()) {
        return (f, f);
    }
    let f = caps.formats.first().copied().unwrap_or(wgpu::TextureFormat::Bgra8Unorm);
    (f, f.remove_srgb_suffix())
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::sg::Sg;

    fn renderer(images: &Images) -> Option<Renderer> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        match pollster::block_on(request_device(&instance, None)) {
            Ok((_, d, q)) => Some(Renderer::new(d, q, images.clone(), None)),
            Err(e) => {
                eprintln!("SKIP: no GPU adapter: {e}");
                None
            }
        }
    }

    fn px(img: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * w + x) * 4) as usize;
        [img[i], img[i + 1], img[i + 2], img[i + 3]]
    }

    #[test]
    fn solid_quads_gamma_blend_and_scissor() {
        let images = Images::new();
        let Some(mut r) = renderer(&images) else { return };
        let mut sg = Sg::new(images.clone(), None);
        sg.begin_frame(32, 32, 1.0);
        sg.set_draw_color(1.0, 1.0, 1.0, 1.0);
        sg.draw_image(None, 0.0, 0.0, 16.0, 32.0, None);
        // Drawn before the white quad's layer-0 successors but on layer 1: ends on top.
        sg.set_draw_layer(Some(1), None);
        sg.set_draw_color(1.0, 0.0, 0.0, 0.5);
        sg.draw_image(None, 0.0, 0.0, 32.0, 8.0, None);
        sg.set_draw_layer(Some(0), None);
        // Clipped to a viewport: only x in 20..24 is visible.
        sg.set_viewport(Some((20.0, 16.0, 4.0, 16.0)));
        sg.set_draw_color(0.0, 1.0, 0.0, 1.0);
        sg.draw_image(None, -10.0, 0.0, 40.0, 16.0, None);
        sg.set_viewport(None);
        let cmds = sg.end_frame();
        let img = r.render_to_rgba(&cmds, (32, 32), [0.0, 0.0, 0.0, 1.0]);
        // Red 50% over white, blended in gamma space: (255, 127.5, 127.5).
        let p = px(&img, 32, 4, 4);
        assert_eq!(p[0], 255);
        assert!((127..=128).contains(&p[1]) && (127..=128).contains(&p[2]), "{p:?}");
        // Red 50% over the black clear colour: (127.5, 0, 0).
        let p = px(&img, 32, 24, 4);
        assert!((127..=128).contains(&p[0]) && p[1] == 0 && p[2] == 0, "{p:?}");
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
        let images = Images::new();
        let Some(mut r) = renderer(&images) else { return };
        // 2x1 texture: left red, right blue; NEAREST so samples are exact.
        let png = {
            let img = image::RgbaImage::from_fn(2, 1, |x, _| {
                if x == 0 {
                    image::Rgba([255, 0, 0, 255])
                } else {
                    image::Rgba([0, 0, 255, 255])
                }
            });
            let mut out = std::io::Cursor::new(Vec::new());
            img.write_to(&mut out, image::ImageFormat::Png).unwrap();
            out.into_inner()
        };
        let h = images.new_handle();
        images.load_from_memory(h, "t.png", png, LoadFlags::NEAREST);
        let mut sg = Sg::new(images.clone(), None);
        sg.begin_frame(8, 2, 1.0);
        // tc 0..2 across 8 px with REPEAT: R R B B R R B B (texel centres).
        sg.draw_image(Some(h), 0.0, 0.0, 8.0, 2.0, Some([0.0, 0.0, 2.0, 1.0]));
        let cmds = sg.end_frame();
        let img = r.render_to_rgba(&cmds, (8, 2), [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(px(&img, 8, 0, 0), [255, 0, 0, 255]);
        assert_eq!(px(&img, 8, 3, 0), [0, 0, 255, 255]);
        assert_eq!(px(&img, 8, 4, 0), [255, 0, 0, 255]);
        assert_eq!(px(&img, 8, 7, 0), [0, 0, 255, 255]);
    }
}
