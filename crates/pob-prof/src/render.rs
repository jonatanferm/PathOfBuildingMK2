//! What happens to each frame the app produces. Owner: PROF1.
//!
//! - [`RenderMode::None`]: the frame (`pob_sg::Frame`: the sorted layer queues, texture uploads)
//!   is built by the app and dropped. This measures the app's own CPU work per frame and is the
//!   default for baselines (least noise, no GPU driver in the numbers).
//! - [`RenderMode::Cpu`]: `pob_sg::raster::Raster` draws it into an RGBA buffer. The rasteriser is
//!   the GPU backend's reference (T5), written for exactness, not speed: its cost is not what a
//!   user pays, so the mode is for pictures (`inspect --png`) and for a "pixels touched" sanity
//!   check, not for frame-time baselines.
//! - [`RenderMode::Wgpu`]: the shipping backend, `pob_sg_wgpu::SgRenderer`, on an offscreen
//!   target of the frame's size (prepare: uploads, vertices; render: scene + blit), submitted,
//!   then the CPU waits for the GPU (`device.poll(wait)`): the frame time includes the GPU work,
//!   as a present without vsync would. Needs an adapter; on a machine without one the run fails
//!   with the adapter error.

use std::time::Instant;

use anyhow::{anyhow, Result};
use pob_sg::Frame;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderMode {
    None,
    Cpu,
    Wgpu,
}

impl RenderMode {
    pub fn parse(s: &str) -> Result<RenderMode> {
        match s {
            "none" => Ok(RenderMode::None),
            "cpu" => Ok(RenderMode::Cpu),
            "wgpu" => Ok(RenderMode::Wgpu),
            _ => Err(anyhow!("render mode {s:?}: expected none, cpu or wgpu")),
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            RenderMode::None => "none",
            RenderMode::Cpu => "cpu",
            RenderMode::Wgpu => "wgpu",
        }
    }
}

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: pob_sg_wgpu::SgRenderer,
    output: Option<(u32, u32, wgpu::Texture, wgpu::TextureView)>,
    adapter: String,
}

/// The per-frame renderer of one run.
pub struct Renderer {
    mode: RenderMode,
    raster: Option<pob_sg::raster::Raster>,
    gpu: Option<Gpu>,
    /// Keep the textures of every frame for a later [`Renderer::snapshot_png`] (inspect).
    pub track_textures: bool,
    tracked: pob_sg::raster::Raster,
    last: Option<Frame>,
}

impl Renderer {
    pub fn new(mode: RenderMode) -> Result<Renderer> {
        let gpu = if mode == RenderMode::Wgpu {
            let instance = wgpu::Instance::new(
                wgpu::InstanceDescriptor::new_without_display_handle_from_env(),
            );
            let (a, device, queue) =
                pollster::block_on(pob_sg_wgpu::request_device(&instance, None))
                    .map_err(|e| anyhow!("render mode wgpu: {e}"))?;
            let info = a.get_info();
            let renderer =
                pob_sg_wgpu::SgRenderer::new(&device, &a.limits(), pob_sg_wgpu::RT_FORMAT);
            Some(Gpu {
                device,
                queue,
                renderer,
                output: None,
                adapter: format!("{} ({:?}, {})", info.name, info.backend, info.driver),
            })
        } else {
            None
        };
        Ok(Renderer {
            mode,
            raster: (mode == RenderMode::Cpu).then(pob_sg::raster::Raster::new),
            gpu,
            track_textures: false,
            tracked: pob_sg::raster::Raster::new(),
            last: None,
        })
    }

    pub fn mode(&self) -> RenderMode {
        self.mode
    }

    /// The GPU adapter (wgpu mode).
    pub fn adapter(&self) -> Option<String> {
        self.gpu.as_ref().map(|g| g.adapter.clone())
    }

    /// Render one frame; returns the time spent (ns).
    pub fn render(&mut self, frame: Frame) -> u64 {
        let t0 = Instant::now();
        match self.mode {
            RenderMode::None => {}
            RenderMode::Cpu => {
                if let Some(r) = self.raster.as_mut() {
                    let px = r.render(&frame);
                    std::hint::black_box(px);
                }
            }
            RenderMode::Wgpu => {
                if let Some(g) = self.gpu.as_mut() {
                    render_gpu(g, &frame);
                }
            }
        }
        let d = t0.elapsed().as_nanos() as u64;
        if self.track_textures {
            for id in &frame.releases {
                self.tracked.textures.remove(id);
            }
            for (id, data) in &frame.uploads {
                self.tracked.textures.insert(*id, data.clone());
            }
            self.last = Some(frame);
        }
        d
    }

    /// The last frame as a PNG (needs `track_textures` from the start of the run).
    pub fn snapshot_png(&mut self, path: &std::path::Path) -> Result<()> {
        let Some(f) = self.last.as_ref() else {
            return Err(anyhow!("no frame recorded"));
        };
        // the textures are already in `tracked`; render without re-applying the uploads
        let frame = Frame {
            screen: f.screen,
            layers: f.layers.clone(),
            uploads: Vec::new(),
            releases: Vec::new(),
            digest: f.digest,
        };
        let (w, h, px) = self.tracked.render(&frame);
        image::save_buffer(path, &px, w, h, image::ExtendedColorType::Rgba8)
            .map_err(|e| anyhow!("{}: {e}", path.display()))
    }
}

fn render_gpu(g: &mut Gpu, frame: &Frame) {
    let (w, h) = (
        frame.screen.fb_w.max(1) as u32,
        frame.screen.fb_h.max(1) as u32,
    );
    g.renderer.prepare(&g.device, &g.queue, frame);
    if g.output.as_ref().map_or(true, |o| (o.0, o.1) != (w, h)) {
        let tex = g.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("pob-prof output"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: pob_sg_wgpu::RT_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        g.output = Some((w, h, tex, view));
    }
    let Some((_, _, _, view)) = g.output.as_ref() else {
        return;
    };
    let mut enc = g
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("pob-prof frame"),
        });
    g.renderer.render(&mut enc, view);
    g.queue.submit(Some(enc.finish()));
    let _ = g.device.poll(wgpu::PollType::wait_indefinitely());
}
