//! The web host (ui doc §5.2 "Web host"; wasm32 only). Owner: U7.
//!
//! Same handler as desktop via winit's web backend: input goes through
//! [`crate::input::WinitInput`] and [`crate::deliver`], frames through [`crate::Driver::frame`]
//! ([`crate::run_frame`]: `Net` completions → `on_net`, then `on_frame`) and the shared
//! [`crate::Gfx`] (WebGL2 through wgpu). The canvas size is its CSS size × devicePixelRatio
//! (winit sizes the backing store and reports it as the inner size).
//!
//! Before `on_init`, [`run_web`] fetches into memory what SimpleGraphic would read
//! synchronously: the asset bundle's index (`cfg.script_dir` is the bundle, VENDOR:
//! [`crate::bundle`]), the 7 `.tgf` fonts and their atlases, plus every file listed in the
//! bundle's [`WEB_PREFETCH_FILE`] (bundle paths, one per line: the UI art, the top-level and
//! legion tree images and the latest tree version's images), and loads the IndexedDB-backed user
//! files ([`crate::fs_web::WebFs`]). The VFS holds them under their logical paths
//! (`Assets/ring.png`), the paths the app names.
//!
//! Clipboard: the canvas keeps the browser's default action for Ctrl/Cmd+C, +X and +V only
//! (everything else is `preventDefault`ed, as winit would). The KeyDown of Ctrl+V is held back
//! until the DOM `paste` event has put the text into the paste cache, then delivered; a paste
//! from the browser menu is delivered as Ctrl down, V down, V up, Ctrl up.
//!
//! Unwinding is unavailable on wasm32, so a panic (a Lua error) is fatal: the panic hook logs it
//! to the console.
//!
//! Images load through U3's [`crate::loader_web::WebLoader`] over the same VFS (CCR-0026 D): the
//! manifest is fetched by [`WebLoader::prefetch`], non-ASYNC loads read the VFS and ASYNC ones
//! are fetched lazily from the bundle (the platform divergences are listed in `loader_web`).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use pob_sg::{AssetSource, Font, FontSet, ImageRegistry, Key, Sg, Sys};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::platform::web::{EventLoopExtWebSys, WindowAttributesExtWebSys};
use winit::window::{Window, WindowId};

use crate::bundle::{Bundle, INDEX_FILE, WEB_PREFETCH_FILE};
use crate::fs_web::{Vfs, WebFs};
use crate::input::Input;
use crate::loader_web::WebLoader;
use crate::net_web::WebNet;
use crate::platform_web::{WebPlatform, USER_PATH};
use crate::{deliver, App, Driver, FrameOutcome, Gfx, HostConfig, PlatformParts};

fn js(e: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&e.to_string())
}

fn now_ms() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map_or_else(js_sys::Date::now, |p| p.now())
}

fn join_url(base: &str, rel: &str) -> String {
    if base.is_empty() {
        rel.to_owned()
    } else {
        format!("{}/{rel}", base.trim_end_matches('/'))
    }
}

/// The parent of a `/`-separated path (`"pob/runtime/SimpleGraphic"` → `"pob/runtime"`).
fn parent_path(p: &str) -> String {
    let t = p.trim_end_matches('/');
    t.rfind('/').map_or_else(String::new, |i| t[..i].to_owned())
}

/// Start fetching `url` (the request runs while other fetches are started).
fn start_fetch(url: &str) -> Option<JsFuture> {
    let w = web_sys::window()?;
    Some(JsFuture::from(w.fetch_with_str(url)))
}

/// The body of a started fetch, `None` on any failure or a non-2xx status.
async fn finish_fetch(f: Option<JsFuture>) -> Option<Vec<u8>> {
    let resp: web_sys::Response = f?.await.ok()?.dyn_into().ok()?;
    if !resp.ok() {
        return None;
    }
    let buf = JsFuture::from(resp.array_buffer().ok()?).await.ok()?;
    Some(js_sys::Uint8Array::new(&buf).to_vec())
}

/// Fetch the logical runtime paths `rels` (`Fonts/…`) from the bundle files that hold them into
/// `out` (keyed by the logical path); paths the index lacks and failed fetches are left out.
async fn fetch_runtime(bundle: &Bundle, rels: &[String], out: &mut HashMap<String, Vec<u8>>) {
    let started: Vec<_> = rels
        .iter()
        .filter_map(|r| {
            let f = bundle.runtime_file(r)?;
            Some((r.clone(), start_fetch(&join_url(&bundle.root, f))))
        })
        .collect();
    for (r, f) in started {
        if let Some(b) = finish_fetch(f).await {
            out.insert(r, b);
        }
    }
}

/// The fonts `FontSet::load` reads: every face's `.tgf`, then the atlas of each height it lists.
async fn prefetch_fonts(bundle: &Bundle) -> HashMap<String, Vec<u8>> {
    let mut files = HashMap::new();
    let tgfs: Vec<String> = Font::ALL
        .iter()
        .map(|f| format!("Fonts/{}.tgf", f.file_stem()))
        .collect();
    fetch_runtime(bundle, &tgfs, &mut files).await;
    let mut atlases = Vec::new();
    for f in Font::ALL {
        let stem = f.file_stem();
        if let Some(raw) = files.get(&format!("Fonts/{stem}.tgf")) {
            for th in pob_sg::font::tgf::parse(raw) {
                atlases.push(format!("Fonts/{stem}.{}.tga", th.height));
            }
        }
    }
    fetch_runtime(bundle, &atlases, &mut files).await;
    files
}

/// The prefetched files as the script and runtime directories (logical paths).
#[derive(Clone)]
struct VfsAssets {
    src: Vfs,
    runtime: Rc<HashMap<String, Vec<u8>>>,
}

fn missing(path: &str) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::NotFound,
        format!("{path}: not prefetched"),
    )
}

impl AssetSource for VfsAssets {
    fn read_src(&self, path: &str) -> std::io::Result<Vec<u8>> {
        let key = path.replace('\\', "/");
        self.src
            .borrow()
            .get(&key)
            .cloned()
            .ok_or_else(|| missing(path))
    }
    fn read_runtime(&self, path: &str) -> std::io::Result<Vec<u8>> {
        self.runtime
            .get(&path.replace('\\', "/"))
            .cloned()
            .ok_or_else(|| missing(path))
    }
}

/// Add a DOM event listener that lives as long as the page.
fn listen<E: wasm_bindgen::convert::FromWasmAbi + 'static>(
    target: &web_sys::EventTarget,
    name: &str,
    passive: Option<bool>,
    f: impl FnMut(E) + 'static,
) -> Result<(), JsValue> {
    let cb = Closure::<dyn FnMut(E)>::new(f);
    match passive {
        Some(p) => {
            let opts = web_sys::AddEventListenerOptions::new();
            opts.set_passive(p);
            target.add_event_listener_with_callback_and_add_event_listener_options(
                name,
                cb.as_ref().unchecked_ref(),
                &opts,
            )?;
        }
        None => target.add_event_listener_with_callback(name, cb.as_ref().unchecked_ref())?,
    }
    cb.forget();
    Ok(())
}

/// What the page tells the event loop.
#[derive(Debug)]
enum PageEvent {
    /// A `paste` event put text into the paste cache.
    Pasted,
    /// The GPU is ready (or failed).
    GfxReady,
}

struct WebHost<A: App> {
    drv: Driver<A>,
    canvas: web_sys::HtmlCanvasElement,
    proxy: EventLoopProxy<PageEvent>,
    gfx: Rc<RefCell<Option<Result<Gfx, String>>>>,
    window: Option<Arc<Window>>,
    inited: bool,
    done: bool,
    start: f64,
    next_due: f64,
    /// The KeyDown of Ctrl+V waits for the paste event.
    held_paste: bool,
    exit_requested: Rc<Cell<bool>>,
}

impl<A: App> WebHost<A> {
    fn fail(&mut self, r: Result<(), String>) {
        if let Err(e) = r {
            self.drv.error.get_or_insert(e);
        }
    }

    fn deliver_all(&mut self, inputs: Vec<Input>) {
        for i in inputs {
            let r = deliver(&mut self.drv.app, i);
            self.fail(r);
            if self.drv.error.is_some() {
                return;
            }
        }
    }

    /// Stop on `Exit()` or a fatal error.
    fn check_exit(&mut self, el: &ActiveEventLoop) {
        if self.exit_requested.get() {
            self.drv.shutdown();
            self.done = true;
        }
        if let Some(e) = &self.drv.error {
            web_sys::console::error_1(&e.as_str().into());
            self.done = true;
        }
        if self.done {
            el.exit();
        }
    }

    fn start_gfx(&mut self, el: &ActiveEventLoop) -> Result<(), String> {
        let attrs = Window::default_attributes()
            .with_canvas(Some(self.canvas.clone()))
            .with_prevent_default(false)
            .with_focusable(true);
        let window = Arc::new(el.create_window(attrs).map_err(|e| e.to_string())?);
        self.window = Some(window.clone());
        let slot = self.gfx.clone();
        let proxy = self.proxy.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let r = async {
                // Fall back to WebGL2 when the browser exposes `navigator.gpu` but has no WebGPU
                // adapter (e.g. Chrome on Linux): `Instance::new` alone would pick WebGPU and fail.
                let instance = wgpu::util::new_instance_with_webgpu_detection(
                    wgpu::InstanceDescriptor::new_without_display_handle_from_env(),
                )
                .await;
                let surface = instance
                    .create_surface(window.clone())
                    .map_err(|e| e.to_string())?;
                let (adapter, device, queue) =
                    pob_sg_wgpu::request_device(&instance, Some(&surface)).await?;
                Ok::<_, String>(Gfx::new(window, surface, &adapter, device, queue))
            }
            .await;
            *slot.borrow_mut() = Some(r);
            let _ = proxy.send_event(PageEvent::GfxReady);
        });
        Ok(())
    }

    fn redraw(&mut self, el: &ActiveEventLoop) {
        let slot = self.gfx.clone();
        let mut g = slot.borrow_mut();
        let Some(Ok(gfx)) = g.as_mut() else { return };
        let now = now_ms();
        match self.drv.frame(gfx) {
            FrameOutcome::Skipped => {
                self.next_due = now + crate::pacing::IDLE_WAIT.as_secs_f64() * 1000.0;
            }
            FrameOutcome::Ran(sleep) => {
                self.next_due = now + sleep.map_or(0.0, |d| d.as_secs_f64() * 1000.0);
            }
        }
        drop(g);
        self.check_exit(el);
    }
}

impl<A: App> ApplicationHandler<PageEvent> for WebHost<A> {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_none() {
            if let Err(e) = self.start_gfx(el) {
                self.drv.error = Some(e);
                self.check_exit(el);
                return;
            }
        }
        if !self.inited {
            self.inited = true;
            self.drv.init();
            self.check_exit(el);
        }
    }

    fn user_event(&mut self, el: &ActiveEventLoop, ev: PageEvent) {
        if self.done {
            return;
        }
        match ev {
            PageEvent::GfxReady => {
                if let Some(Err(e)) = &*self.gfx.borrow() {
                    self.drv.error.get_or_insert(e.clone());
                }
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            PageEvent::Pasted => {
                let v_down = Input::Key {
                    key: Key::Char(b'v'),
                    down: true,
                    double_click: false,
                };
                if std::mem::take(&mut self.held_paste) {
                    self.deliver_all(vec![v_down]);
                } else {
                    let k = |key, down| Input::Key {
                        key,
                        down,
                        double_click: false,
                    };
                    self.deliver_all(vec![
                        k(Key::Ctrl, true),
                        v_down,
                        k(Key::Char(b'v'), false),
                        k(Key::Ctrl, false),
                    ]);
                }
            }
        }
        self.check_exit(el);
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if self.done {
            return;
        }
        match event {
            WindowEvent::RedrawRequested => self.redraw(el),
            WindowEvent::CloseRequested
            | WindowEvent::Resized(_)
            | WindowEvent::ScaleFactorChanged { .. } => {}
            ev => {
                let scale = self.window.as_ref().map_or(1.0, |w| w.scale_factor());
                let now = now_ms() - self.start;
                let inputs = self.drv.input.translate(&ev, now, scale);
                let mut out = Vec::with_capacity(inputs.len());
                for i in inputs {
                    match i {
                        Input::Key {
                            key: Key::Char(b'v'),
                            down: true,
                            ..
                        } if self.drv.app.sys().is_key_down(Key::Ctrl) => {
                            // Wait for the paste event (PageEvent::Pasted).
                            self.held_paste = true;
                        }
                        Input::Key {
                            key: Key::Char(b'v'),
                            down: false,
                            ..
                        } if std::mem::take(&mut self.held_paste) => {
                            // No paste event came (no text on the clipboard): deliver late.
                            out.push(Input::Key {
                                key: Key::Char(b'v'),
                                down: true,
                                double_click: false,
                            });
                            out.push(i);
                        }
                        i => out.push(i),
                    }
                }
                self.deliver_all(out);
                self.check_exit(el);
            }
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        if self.done {
            return;
        }
        let ready = matches!(&*self.gfx.borrow(), Some(Ok(_)));
        let Some(w) = self.window.as_ref().filter(|_| ready) else {
            el.set_control_flow(ControlFlow::Wait);
            return;
        };
        let now = now_ms();
        if now >= self.next_due {
            w.request_redraw();
            el.set_control_flow(ControlFlow::Wait);
        } else {
            let ms = (self.next_due - now).ceil() as u64;
            el.set_control_flow(ControlFlow::wait_duration(Duration::from_millis(ms)));
        }
    }
}

/// Run the app made by `make` on the canvas `canvas_id` (see the module docs). Returns once the
/// event loop is running (it lives on in the browser's event handlers).
pub async fn run_web<A: App>(
    canvas_id: &str,
    cfg: HostConfig,
    make: impl FnOnce(PlatformParts) -> A,
) -> Result<(), wasm_bindgen::JsValue> {
    std::panic::set_hook(Box::new(|info| {
        web_sys::console::error_1(&format!("--- SCRIPT ERROR ---\n{info}").into());
    }));
    let window = web_sys::window().ok_or_else(|| js("no window"))?;
    let document = window.document().ok_or_else(|| js("no document"))?;
    document.set_title(&cfg.title);
    let canvas: web_sys::HtmlCanvasElement = document
        .get_element_by_id(canvas_id)
        .ok_or_else(|| js(format!("no element #{canvas_id}")))?
        .dyn_into()?;

    // Prefetch: the bundle's index, the fonts, then the bundle's prefetch list (VENDOR).
    let index_url = join_url(&cfg.script_dir, INDEX_FILE);
    let bundle = match finish_fetch(start_fetch(&index_url)).await {
        Some(t) => Bundle::parse(&cfg.script_dir, &String::from_utf8_lossy(&t)).map_err(js)?,
        None => {
            return Err(js(format!(
                "{index_url}: the asset bundle's index is missing"
            )))
        }
    };
    let bundle = Rc::new(bundle);
    let runtime = prefetch_fonts(&bundle).await;
    let vfs: Vfs = Rc::default();
    // `WebLoader` appends the bundle path to its base: the bundle root with a trailing slash
    // ("" = page-relative).
    let src_base = if cfg.script_dir.is_empty() {
        String::new()
    } else {
        format!("{}/", cfg.script_dir.trim_end_matches('/'))
    };
    let mut loader = WebLoader::with_vfs(src_base, vfs.clone());
    loader.set_bundle(bundle.clone());
    let list_url = join_url(&cfg.script_dir, WEB_PREFETCH_FILE);
    if let Some(m) = finish_fetch(start_fetch(&list_url)).await {
        let list = crate::bundle::prefetch_list(&String::from_utf8_lossy(&m));
        loader.prefetch_bundle_files(&list).await;
    }
    let assets = VfsAssets {
        src: vfs.clone(),
        runtime: Rc::new(runtime),
    };
    let mut images = ImageRegistry::new(Box::new(loader));
    let fonts = FontSet::load(&assets, &mut images).unwrap_or_else(|e| {
        web_sys::console::error_1(&format!("fonts from {}: {e}", bundle.root).into());
        FontSet::empty()
    });
    let sg = Sg::new(Rc::new(fonts), images);
    let platform = WebPlatform::new(cfg.script_dir.clone(), parent_path(&cfg.sg_runtime));
    let paste_cache = platform.paste_cache.clone();
    let exit_requested = platform.exit_requested.clone();
    let fs = WebFs::open(&cfg.script_dir, USER_PATH, vfs).await;
    let sys = Sys::new(
        Box::new(platform),
        Box::new(fs),
        Box::new(WebNet::default()),
    );
    let app = make(PlatformParts { sg, sys });

    let el = EventLoop::<PageEvent>::with_user_event()
        .build()
        .map_err(js)?;
    let proxy = el.create_proxy();

    // The clipboard chords keep their default action (the paste event); nothing else does.
    let target: &web_sys::EventTarget = canvas.as_ref();
    listen(target, "keydown", None, |e: web_sys::KeyboardEvent| {
        let k = e.key().to_ascii_lowercase();
        let chord = (e.ctrl_key() || e.meta_key()) && matches!(k.as_str(), "c" | "v" | "x");
        if !chord {
            e.prevent_default();
        }
    })?;
    listen(target, "keyup", None, |e: web_sys::KeyboardEvent| {
        e.prevent_default();
    })?;
    listen(target, "contextmenu", None, |e: web_sys::Event| {
        e.prevent_default();
    })?;
    listen(target, "wheel", Some(false), |e: web_sys::Event| {
        e.prevent_default();
    })?;
    let paste_proxy = proxy.clone();
    listen(
        document.as_ref(),
        "paste",
        None,
        move |e: web_sys::ClipboardEvent| {
            let text = e.clipboard_data().and_then(|d| d.get_data("text").ok());
            e.prevent_default();
            *paste_cache.borrow_mut() = text;
            let _ = paste_proxy.send_event(PageEvent::Pasted);
        },
    )?;

    let start = now_ms();
    el.spawn_app(WebHost {
        drv: Driver::new(app),
        canvas,
        proxy,
        gfx: Rc::default(),
        window: None,
        inited: false,
        done: false,
        start,
        next_due: start,
        held_paste: false,
        exit_requested,
    });
    Ok(())
}
