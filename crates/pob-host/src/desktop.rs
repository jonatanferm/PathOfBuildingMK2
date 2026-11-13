//! Desktop host: winit window + event loop driving an [`App`] with SimpleGraphic input semantics,
//! rendering through pob-sg-wgpu (ui doc §5.2 "Desktop host rules"). Owner: U7.
//!
//! Port of PathOfBuilding-SimpleGraphic `3b1a346` `sys_main.cpp` `Run` (main loop: poll events,
//! `core->Frame()` until `exitFlag`), `ui_main.cpp:380-445` `Frame`, `:465-475` `CanExit`, the
//! GLFW callbacks of `sys_video.cpp:476-620` (through [`crate::input::WinitInput`]) and
//! `r_main.cpp` `DoScreenshot`. Moved from the pre-port `pob-sg/src/host.rs` by UI-U0.
//!
//! Loop: winit `about_to_wait` requests a redraw when the next frame is due; `RedrawRequested`
//! runs [`crate::Driver::frame`] (pacing test, [`crate::run_frame`], render, present); the
//! following frame is due at once or after the 100 ms idle sleep ([`crate::pacing`]).
//! Window requests made by Lua (title, `Exit`, `Restart`, `TakeScreenshot`) are applied after
//! the callback that made them.
//!
//! DIVERGENCE(restart): SG's `Restart()` re-runs the script in the same window
//! (`ScriptShutdown` + `ScriptInit`); `run_desktop` takes its app factory by `FnOnce`, so the
//! port runs `OnExit`, starts a new process with the same arguments and exits.
//! DIVERGENCE(screenshot time): the file name uses UTC (`MMDDYY_HHMMSS`), SG local time.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use pob_sg::{FontSet, ImageRegistry, Sg, Sys};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

use crate::fs_native::{dir_or_bundle_assets, NativeFs};
use crate::loader_native::NativeLoader;
use crate::net_native::NativeNet;
use crate::platform_native::{HostRequests, NativePlatform};
use crate::{App, Driver, FrameOutcome, Gfx, HostConfig, PlatformParts};

/// Build `Sg` (fonts loaded, native loader installed) and `Sys` for `cfg`. Returns the queue of
/// window requests the platform writes to.
pub fn build_parts(
    cfg: &HostConfig,
    deterministic_images: bool,
) -> anyhow::Result<(PlatformParts, Rc<RefCell<HostRequests>>)> {
    let mut images = ImageRegistry::new(Box::new(NativeLoader::new(
        &cfg.script_dir,
        deterministic_images,
    )));
    // VENDOR: the asset bundle's index when the script dir is the bundle
    let assets = dir_or_bundle_assets(Path::new(&cfg.script_dir), Path::new(&cfg.sg_runtime));
    let fonts = FontSet::load(&*assets, &mut images)
        .map_err(|e| anyhow::anyhow!("loading fonts from {}: {e}", cfg.sg_runtime))?;
    let sg = Sg::new(Rc::new(fonts), images);
    let platform = NativePlatform::new(cfg.script_dir.clone(), cfg.sg_runtime.clone());
    let requests = platform.requests.clone();
    let net = NativeNet::with_clipboard(platform.clipboard());
    let fs = NativeFs::new(&cfg.script_dir);
    let sys = Sys::new(Box::new(platform), Box::new(fs), Box::new(net));
    Ok((PlatformParts { sg, sys }, requests))
}

/// `{:%m%d%y_%H%M%S}` of a UNIX time (UTC).
pub fn screenshot_stamp(unix_secs: i64) -> String {
    let days = unix_secs.div_euclid(86_400);
    let secs = unix_secs.rem_euclid(86_400);
    // civil_from_days (Howard Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{m:02}{d:02}{:02}_{:02}{:02}{:02}",
        y.rem_euclid(100),
        secs / 3600,
        secs / 60 % 60,
        secs % 60
    )
}

/// Where screenshots go: SimpleGraphic's `<runtime>/SimpleGraphic/Screenshots` (`CFG_DATAPATH`);
/// VENDOR: with the asset bundle as the runtime dir (read-only, part of the install), the user
/// data folder's `Path of Building/Screenshots` instead.
pub fn screenshot_dir(sg_runtime: &Path) -> PathBuf {
    if sg_runtime.join(crate::bundle::INDEX_FILE).is_file() {
        if let Ok(user) = crate::platform_native::find_user_path() {
            return Path::new(&user).join("Path of Building/Screenshots");
        }
    }
    sg_runtime.join("Screenshots")
}

/// lua: r_main.cpp `DoScreenshot`: the frame as RGB JPEG (`r_screenshotFormat` defaults to
/// `jpg`) at `<screenshot_dir>/<MMDDYY_HHMMSS>.jpg`.
fn save_screenshot(sg_runtime: &Path, w: u32, h: u32, rgba: &[u8]) -> Result<PathBuf, String> {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let dir = screenshot_dir(sg_runtime);
    std::fs::create_dir_all(&dir).map_err(|_| "Couldn't create screenshot folder!".to_owned())?;
    let path = dir.join(format!("{}.jpg", screenshot_stamp(secs)));
    let rgb: Vec<u8> = rgba
        .chunks_exact(4)
        .flat_map(|p| [p[0], p[1], p[2]])
        .collect();
    image::save_buffer(&path, &rgb, w, h, image::ColorType::Rgb8)
        .map_err(|_| "Couldn't write screenshot!".to_owned())?;
    Ok(path)
}

struct Host<A: App> {
    drv: Driver<A>,
    cfg: HostConfig,
    requests: Rc<RefCell<HostRequests>>,
    gfx: Option<Gfx>,
    start: Instant,
    next_frame: Instant,
    inited: bool,
    done: bool,
    /// A frame ran since the last screenshot check (SG shoots the frame it was asked in).
    rendered: bool,
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
        self.gfx = Some(Gfx::new(window, surface, &adapter, device, queue));
        Ok(())
    }

    fn now_ms(&self) -> f64 {
        self.start.elapsed().as_secs_f64() * 1000.0
    }

    /// Apply what Lua asked for during the last callbacks.
    fn apply_requests(&mut self, el: &ActiveEventLoop) {
        let req = std::mem::take(&mut *self.requests.borrow_mut());
        if let (Some(t), Some(g)) = (req.title, &self.gfx) {
            g.window.set_title(&t);
        }
        if req.screenshot && self.rendered {
            if let Some(g) = &self.gfx {
                let (w, h, px) = g.read_back();
                match save_screenshot(Path::new(&self.cfg.sg_runtime), w, h, &px) {
                    Ok(p) => println!("Wrote screenshot to {}", p.display()),
                    Err(e) => println!("{e}"),
                }
            }
        } else if req.screenshot {
            self.requests.borrow_mut().screenshot = true; // wait for the next rendered frame
        }
        if req.exit || req.restart {
            self.drv.shutdown();
            if req.restart && !req.exit {
                if let Ok(exe) = std::env::current_exe() {
                    let _ = std::process::Command::new(exe)
                        .args(std::env::args_os().skip(1))
                        .spawn();
                }
            }
            self.finish(el);
        }
        if self.drv.error.is_some() {
            self.finish(el);
        }
    }

    fn finish(&mut self, el: &ActiveEventLoop) {
        self.done = true;
        el.exit();
    }

    fn redraw(&mut self, el: &ActiveEventLoop) {
        let Some(gfx) = self.gfx.as_mut() else { return };
        let now = Instant::now();
        self.rendered = false;
        match self.drv.frame(gfx) {
            FrameOutcome::Skipped => self.next_frame = now + crate::pacing::IDLE_WAIT,
            FrameOutcome::Ran(sleep) => {
                self.rendered = true;
                self.next_frame = now + sleep.unwrap_or(Duration::ZERO);
            }
        }
        self.apply_requests(el);
    }
}

impl<A: App> ApplicationHandler for Host<A> {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.gfx.is_none() {
            if let Err(e) = self.init_gfx(el) {
                self.drv.error = Some(e);
                self.finish(el);
                return;
            }
        }
        if !self.inited {
            self.inited = true;
            self.drv.init();
            self.apply_requests(el);
        }
        if let Some(g) = &self.gfx {
            g.window.request_redraw();
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if self.done {
            return;
        }
        match event {
            WindowEvent::CloseRequested => {
                // glfwSetWindowCloseCallback: initialised && CanExit(); then Shutdown → OnExit.
                if self.drv.can_exit() {
                    self.drv.shutdown();
                    self.finish(el);
                }
            }
            WindowEvent::RedrawRequested => self.redraw(el),
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {}
            ev => {
                let scale = self.gfx.as_ref().map_or(1.0, |g| g.window.scale_factor());
                let now = self.now_ms();
                self.drv.window_event(&ev, now, scale);
                self.apply_requests(el);
            }
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        if self.done {
            return;
        }
        let Some(g) = &self.gfx else { return };
        if Instant::now() >= self.next_frame {
            g.window.request_redraw();
            el.set_control_flow(ControlFlow::Wait);
        } else {
            el.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
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
    el.set_control_flow(ControlFlow::Wait);
    let now = Instant::now();
    let mut host = Host {
        drv: Driver::new(app),
        cfg,
        requests,
        gfx: None,
        start: now,
        next_frame: now,
        inited: false,
        done: false,
        rendered: false,
    };
    el.run_app(&mut host)?;
    host.drv.error.map_or(Ok(()), |e| Err(anyhow::anyhow!(e)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamps() {
        assert_eq!(screenshot_stamp(0), "010170_000000");
        // 2026-10-10T13:45:07Z
        assert_eq!(screenshot_stamp(1_791_639_907), "101026_134507");
        // 2000-02-29T23:59:59Z
        assert_eq!(screenshot_stamp(951_868_799), "022900_235959");
    }
}
