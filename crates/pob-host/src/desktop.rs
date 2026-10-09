//! Desktop host: winit window + event loop driving an [`App`] with SimpleGraphic input semantics,
//! rendering through pob-sg-wgpu (ui doc §5.2 "Desktop host rules"). Owner: U7.
//!
//! Moved from the pre-port `pob-sg/src/host.rs` by UI-U0 and adapted to the §5.2 surface (the
//! App owns `Sg`/`Sys`; keys are [`Key`]s; rendering goes through `SgRenderer`). Behaviour ported
//! from SimpleGraphic (research copy master @ 8694a07): key names and extra chars (`input.rs`),
//! wheel = KeyDown+KeyUp, double click, swallowed F10 / Shift+Pause KeyUp, `GetCursorPos`
//! rounding (in `Sys`).
//!
//! Deltas U7 closes: Net completions are not delivered yet (no App callback in the §5.2 trait,
//! see CCR-0013); Restart/TakeScreenshot requests are ignored.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use pob_sg::{FontSet, ImageRegistry, Key, ScreenInfo, Sg, Sys};
use pob_sg_wgpu::SgRenderer;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key as WKey, PhysicalKey};
use winit::window::{Window, WindowId};

use crate::fs_native::{DirAssets, NativeFs};
use crate::input::{
    extra_char, key_from_winit, mouse_button_key, swallow_key_up, text_chars, DoubleClick,
};
use crate::loader_native::NativeLoader;
use crate::net_native::NativeNet;
use crate::pacing::Pacing;
use crate::platform_native::{HostRequests, NativePlatform};
use crate::{panic_message, App, HostConfig, PlatformParts};

/// Build `Sg` (fonts loaded, native loader installed) and `Sys` for `cfg`. Returns the queue of
/// window requests the platform writes to.
pub fn build_parts(
    cfg: &HostConfig,
    deterministic_images: bool,
) -> anyhow::Result<(PlatformParts, Rc<RefCell<HostRequests>>)> {
    let mut images = ImageRegistry::new(Box::new(NativeLoader::new(
        &cfg.pob_src,
        deterministic_images,
    )));
    let assets = DirAssets {
        src: PathBuf::from(&cfg.pob_src),
        runtime: PathBuf::from(&cfg.sg_runtime),
    };
    let fonts = FontSet::load(&assets, &mut images)
        .map_err(|e| anyhow::anyhow!("loading fonts from {}: {e}", cfg.sg_runtime))?;
    let sg = Sg::new(Rc::new(fonts), images);
    let platform = NativePlatform::new(cfg.pob_src.clone(), cfg.sg_runtime.clone());
    let requests = platform.requests.clone();
    let sys = Sys::new(Box::new(platform), Box::new(NativeFs), Box::new(NativeNet));
    Ok((PlatformParts { sg, sys }, requests))
}

struct Gfx {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    view_format: wgpu::TextureFormat,
    renderer: SgRenderer,
}

struct Host<A: App> {
    app: A,
    cfg: HostConfig,
    requests: Rc<RefCell<HostRequests>>,
    gfx: Option<Gfx>,
    dbl: DoubleClick,
    wheel_acc: f64,
    cursor_phys: Option<(f64, f64)>,
    focused: bool,
    hovered: bool,
    pacing: Pacing,
    error: Option<String>,
    inited: bool,
}

impl<A: App> Host<A> {
    fn init_gfx(&mut self, el: &ActiveEventLoop) -> Result<(), String> {
        let attrs = Window::default_attributes()
            .with_title(self.cfg.title.clone())
            .with_inner_size(LogicalSize::new(
                f64::from(self.cfg.size.0),
                f64::from(self.cfg.size.1),
            ))
            .with_min_inner_size(LogicalSize::new(
                f64::from(self.cfg.min_size.0),
                f64::from(self.cfg.min_size.1),
            ));
        let window = Arc::new(el.create_window(attrs).map_err(|e| e.to_string())?);
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle_from_env(
                Box::new(el.owned_display_handle()),
            ));
        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| e.to_string())?;
        let (adapter, device, queue) =
            pollster::block_on(pob_sg_wgpu::request_device(&instance, Some(&surface)))?;
        let caps = surface.get_capabilities(&adapter);
        let (format, view_format) = pob_sg_wgpu::pick_surface_format(&caps);
        let size = window.inner_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            color_space: Default::default(),
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps
                .alpha_modes
                .first()
                .copied()
                .unwrap_or(wgpu::CompositeAlphaMode::Auto),
            view_formats: if view_format == format {
                vec![]
            } else {
                vec![view_format]
            },
        };
        surface.configure(&device, &config);
        let renderer = SgRenderer::new(&device, &adapter.limits(), view_format);
        self.gfx = Some(Gfx {
            window,
            surface,
            device,
            queue,
            config,
            view_format,
            renderer,
        });
        Ok(())
    }

    /// D17: each callback runs inside catch_unwind; a panic goes to `App::on_lua_error`.
    fn guard(&mut self, f: impl FnOnce(&mut A)) {
        let app = &mut self.app;
        if let Err(p) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(app))) {
            let msg = panic_message(&*p);
            let app = &mut self.app;
            if let Err(p2) =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| app.on_lua_error(msg)))
            {
                self.error = Some(panic_message(&*p2));
            }
        }
    }

    fn key_down(&mut self, key: Key, dbl: bool) {
        self.app.sys().set_key(key, true);
        self.guard(|a| a.on_key_down(key, dbl));
    }

    fn key_up(&mut self, key: Key) {
        self.app.sys().set_key(key, false);
        let shift = self.app.sys().is_key_down(Key::Shift);
        if swallow_key_up(key, shift) {
            return;
        }
        self.guard(|a| a.on_key_up(key));
    }

    fn wheel(&mut self, up: bool) {
        let key = if up { Key::WheelUp } else { Key::WheelDown };
        self.guard(|a| a.on_key_down(key, false));
        self.guard(|a| a.on_key_up(key));
    }

    fn frame(&mut self, el: &ActiveEventLoop) {
        let Some(gfx) = self.gfx.as_mut() else { return };
        let size = gfx.window.inner_size();
        if size.width == 0 || size.height == 0 {
            return;
        }
        if size.width != gfx.config.width || size.height != gfx.config.height {
            gfx.config.width = size.width;
            gfx.config.height = size.height;
            gfx.surface.configure(&gfx.device, &gfx.config);
        }
        let screen = ScreenInfo {
            fb_w: size.width as i32,
            fb_h: size.height as i32,
            win_h: size.height as i32,
            dpi_scale: gfx.window.scale_factor() as f32,
        };
        let cursor = self
            .cursor_phys
            .map_or((0, 0), |(x, y)| (x.floor() as i32, y.floor() as i32));
        self.app.sg().begin_frame(screen);
        let scale = self.app.sg().scale();
        self.app.sys().begin_frame(cursor, scale);
        self.guard(A::on_frame);
        let frame = self.app.sg().end_frame();
        let gfx = self.gfx.as_mut().unwrap();
        gfx.renderer.prepare(&gfx.device, &gfx.queue, &frame);
        let tex = match gfx.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t)
            | wgpu::CurrentSurfaceTexture::Suboptimal(t) => Some(t),
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                gfx.surface.configure(&gfx.device, &gfx.config);
                None
            }
            _ => None,
        };
        if let Some(tex) = tex {
            let view = tex.texture.create_view(&wgpu::TextureViewDescriptor {
                format: Some(gfx.view_format),
                ..Default::default()
            });
            let mut enc = gfx
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("pob-host frame"),
                });
            gfx.renderer.render(&mut enc, &view);
            gfx.queue.submit(Some(enc.finish()));
            gfx.window.pre_present_notify();
            gfx.queue.present(tex);
        }
        self.pacing.frame_done();
        let req = std::mem::take(&mut *self.requests.borrow_mut());
        if let Some(t) = req.title {
            gfx.window.set_title(&t);
        }
        if req.exit || self.error.is_some() {
            self.app.on_exit();
            el.exit();
            return;
        }
        match self
            .pacing
            .next_wait(self.focused, self.hovered, self.app.wants_frames())
        {
            None => {
                el.set_control_flow(ControlFlow::Poll);
                gfx.window.request_redraw();
            }
            Some(d) => el.set_control_flow(ControlFlow::WaitUntil(Instant::now() + d)),
        }
    }
}

impl<A: App> ApplicationHandler for Host<A> {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.gfx.is_none() {
            if let Err(e) = self.init_gfx(el) {
                self.error = Some(e);
                el.exit();
                return;
            }
        }
        if !self.inited {
            self.inited = true;
            self.guard(A::on_init);
        }
        if let Some(g) = &self.gfx {
            g.window.request_redraw();
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                if self.app.can_exit() {
                    self.app.on_exit();
                    el.exit();
                }
            }
            WindowEvent::RedrawRequested => self.frame(el),
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                if let Some(g) = &self.gfx {
                    g.window.request_redraw();
                }
            }
            WindowEvent::Focused(f) => self.focused = f,
            WindowEvent::CursorEntered { .. } => self.hovered = true,
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_phys = Some((position.x, position.y));
            }
            WindowEvent::CursorLeft { .. } => {
                // glfwSetCursorEnterCallback: lastCursorPos.reset() on leave.
                self.cursor_phys = None;
                self.hovered = false;
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let Some(key) = mouse_button_key(button) else {
                    return;
                };
                match state {
                    ElementState::Pressed => {
                        let pos = self.cursor_phys.map(|(x, y)| (x as i32, y as i32));
                        let dbl = self.dbl.press(key, pos, Instant::now());
                        self.key_down(key, dbl);
                    }
                    ElementState::Released => self.key_up(key),
                }
            }
            WindowEvent::MouseWheel { delta, .. } => match delta {
                MouseScrollDelta::LineDelta(_, y) => {
                    if y > 0.0 {
                        self.wheel(true);
                    } else if y < 0.0 {
                        self.wheel(false);
                    }
                }
                MouseScrollDelta::PixelDelta(p) => {
                    // §5.2: PixelDelta accumulates and gives one pair per 40 logical px.
                    let step = 40.0 * self.gfx.as_ref().map_or(1.0, |g| g.window.scale_factor());
                    self.wheel_acc += p.y;
                    while self.wheel_acc >= step {
                        self.wheel_acc -= step;
                        self.wheel(true);
                    }
                    while self.wheel_acc <= -step {
                        self.wheel_acc += step;
                        self.wheel(false);
                    }
                }
            },
            WindowEvent::KeyboardInput { event, .. } => {
                let PhysicalKey::Code(code) = event.physical_key else {
                    return;
                };
                let logical = match &event.logical_key {
                    WKey::Character(s) => s.chars().next(),
                    _ => None,
                };
                let key = key_from_winit(code, logical);
                match event.state {
                    ElementState::Pressed => {
                        if let Some(k) = key {
                            self.key_down(k, false);
                        }
                        if let Some(c) = extra_char(code) {
                            self.guard(|a| a.on_char(c));
                        } else if let Some(text) = event.text.as_ref() {
                            // GLFW only delivers chars for "plain" input (no CTRL/ALT held), so
                            // Ctrl+V is a key, not a 'v'.
                            let held = self.app.sys().is_key_down(Key::Ctrl)
                                || self.app.sys().is_key_down(Key::Alt);
                            if !held {
                                let chars: Vec<Key> = text_chars(text).collect();
                                for c in chars {
                                    self.guard(|a| a.on_char(c));
                                }
                            }
                        }
                    }
                    ElementState::Released => {
                        if let Some(k) = key {
                            self.key_up(k);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

/// Run the app made by `make` in a window until it exits.
pub fn run_desktop<A: App>(
    cfg: HostConfig,
    make: impl FnOnce(PlatformParts) -> A,
) -> anyhow::Result<()> {
    let (parts, requests) = build_parts(&cfg, false)?;
    let app = make(parts);
    let el = EventLoop::new()?;
    el.set_control_flow(ControlFlow::Poll);
    let mut host = Host {
        app,
        cfg,
        requests,
        gfx: None,
        dbl: DoubleClick::default(),
        wheel_acc: 0.0,
        cursor_phys: None,
        focused: true,
        hovered: false,
        pacing: Pacing::default(),
        error: None,
        inited: false,
    };
    el.run_app(&mut host)?;
    host.error.map_or(Ok(()), |e| Err(anyhow::anyhow!(e)))
}
