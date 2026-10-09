//! Desktop host: winit window + event loop driving an [`App`] with
//! SimpleGraphic/PoB input semantics, rendering through [`crate::gpu`].
//!
//! Input behaviour is ported from SimpleGraphic (master @ 8694a07):
//! - key codes and names: `engine/system/win/sys_main.cpp`
//!   `sys_main_c::GlfwKeyToKey` (GLFW key -> SG key; printable keys are
//!   delivered as their lowercase character, others by name) and
//!   `ui_main.cpp` `ui_keyNameMap` / `NameForKey` / `KeyForName`;
//! - extra characters on key down: `sys_main_c::GlfwKeyExtraChar` (BACK ->
//!   0x08, TAB -> 0x09, RETURN -> 0x0d, ESCAPE -> 0x1b), also on auto-repeat;
//! - mouse buttons, double click (same button within 500 ms and 5 px of the
//!   previous click; the double click consumes it) and the wheel (each scroll
//!   event sends WHEELUP/WHEELDOWN as a key *down then up*):
//!   `engine/system/win/sys_video.cpp` mouse button / scroll callbacks;
//! - `ui_main.cpp` `ui_main_c::KeyEvent`: key-ups of F10 (debug UI toggle) and
//!   of PAUSE while SHIFT is held (profiling toggle) are not forwarded;
//! - `IsKeyDown(name)`: `l_IsKeyDown` + `KeyForName` (names are matched
//!   case-insensitively; single letters case-insensitively; digits and space
//!   as themselves; other single characters are not recognised);
//! - `GetCursorPos()`: `l_GetCursorPos`, `lround(floor(cursor) / scale)`;
//! - `GetTime()`: milliseconds since start (`sys_main_c::GetTime`).
//!
//! Divergence: SimpleGraphic's `CallKeyHandler` passes codepoints >= 128 to
//! `OnChar` through `NameForKey` (yielding "?" or, for 128..=172, a key name);
//! this host delivers the real character instead.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

use crate::font::FontSet;
use crate::gpu::{self, Renderer};
use crate::images::Images;
use crate::sg::Sg;

/// The PoB-side callbacks (`launch:OnInit`, `OnFrame`, `OnKeyDown`, ...).
pub trait App {
    fn on_init(&mut self, sg: &mut Sg);
    fn on_frame(&mut self, sg: &mut Sg, input: &mut InputState);
    fn on_key_down(&mut self, key: &str, double_click: bool);
    fn on_key_up(&mut self, key: &str);
    fn on_char(&mut self, ch: char);
    fn can_exit(&mut self) -> bool;
    /// `OnExit`, called once before the window closes.
    fn on_exit(&mut self) {}
}

/// Platform state and services available during `on_frame`.
pub struct InputState {
    held: HashSet<String>,
    cursor_phys: Option<(f64, f64)>,
    cursor_virtual: (i32, i32),
    start: Instant,
    clipboard: Option<arboard::Clipboard>,
    pending_title: Option<String>,
    exit_requested: bool,
}

impl Default for InputState {
    fn default() -> Self {
        Self::new()
    }
}

impl InputState {
    pub fn new() -> InputState {
        InputState {
            held: HashSet::new(),
            cursor_phys: None,
            cursor_virtual: (0, 0),
            start: Instant::now(),
            clipboard: None,
            pending_title: None,
            exit_requested: false,
        }
    }

    /// `IsKeyDown(keyName)`. Unrecognised names return false (SimpleGraphic
    /// raises "unrecognised key name").
    pub fn is_key_down(&self, name: &str) -> bool {
        canonical_key_name(name).is_some_and(|k| self.held.contains(&k))
    }

    /// `GetCursorPos()` in virtual pixels.
    pub fn cursor_pos(&self) -> (i32, i32) {
        self.cursor_virtual
    }

    fn update_cursor(&mut self, scale: f32) {
        if let Some((x, y)) = self.cursor_phys {
            // GetRelativeCursor floors, l_GetCursorPos lrounds after dividing.
            let s = f64::from(scale);
            self.cursor_virtual = ((x.floor() / s).round() as i32, (y.floor() / s).round() as i32);
        }
    }

    /// `Copy(text)`.
    pub fn copy(&mut self, text: &str) {
        if let Some(cb) = self.clipboard() {
            let _ = cb.set_text(text.to_owned());
        }
    }

    /// `Paste()`.
    pub fn paste(&mut self) -> Option<String> {
        self.clipboard()?.get_text().ok()
    }

    fn clipboard(&mut self) -> Option<&mut arboard::Clipboard> {
        if self.clipboard.is_none() {
            self.clipboard = arboard::Clipboard::new().ok();
        }
        self.clipboard.as_mut()
    }

    /// `SetWindowTitle(title)` (applied after the frame).
    pub fn set_window_title(&mut self, title: &str) {
        self.pending_title = Some(title.to_owned());
    }

    /// `OpenURL(url)`: hands the URL to the platform opener.
    pub fn open_url(&self, url: &str) -> Result<(), String> {
        open_url(url)
    }

    /// `GetTime()`: milliseconds since the host started.
    pub fn get_time(&self) -> i64 {
        self.start.elapsed().as_millis() as i64
    }

    /// `Exit()`: close after the current frame.
    pub fn exit(&mut self) {
        self.exit_requested = true;
    }
}

/// Open a URL with the desktop's default handler.
pub fn open_url(url: &str) -> Result<(), String> {
    let mut cmd = if cfg!(target_os = "windows") {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "start", "", url]);
        c
    } else if cfg!(target_os = "macos") {
        let mut c = std::process::Command::new("open");
        c.arg(url);
        c
    } else {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(url);
        c
    };
    cmd.spawn().map(|_| ()).map_err(|e| e.to_string())
}

/// SimpleGraphic key name for a physical key, as delivered to
/// `OnKeyDown`/`OnKeyUp` (`GlfwKeyToKey` + `NameForKey`; codes < 128 are sent
/// as the character). `logical` is the layout's character for the key, used
/// for letters like GLFW's translated key names.
pub fn key_name(code: KeyCode, logical: Option<char>) -> Option<String> {
    use KeyCode as K;
    let s: &str = match code {
        K::Backspace => "BACK",
        K::Tab => "TAB",
        K::Enter | K::NumpadEnter => "RETURN",
        K::ShiftLeft | K::ShiftRight => "SHIFT",
        K::ControlLeft | K::ControlRight => "CTRL",
        K::AltLeft | K::AltRight => "ALT",
        K::Pause => "PAUSE",
        K::Escape => "ESCAPE",
        K::Space => " ",
        K::PageUp => "PAGEUP",
        K::PageDown => "PAGEDOWN",
        K::End => "END",
        K::Home => "HOME",
        K::ArrowLeft => "LEFT",
        K::ArrowUp => "UP",
        K::ArrowRight => "RIGHT",
        K::ArrowDown => "DOWN",
        K::PrintScreen => "PRINTSCREEN",
        K::Insert => "INSERT",
        K::Delete => "DELETE",
        K::NumLock => "NUMLOCK",
        K::ScrollLock => "SCROLLLOCK",
        K::Semicolon => ";",
        // "GLFW defines no plus key": '=' is mapped to '+'.
        K::Equal => "+",
        K::Comma => ",",
        K::Minus => "-",
        K::Period => ".",
        K::Slash => "/",
        K::Backquote => "`",
        K::BracketLeft => "[",
        K::Backslash => "\\",
        K::BracketRight => "]",
        K::Quote => "'",
        K::Numpad0 => "0",
        K::NumpadSubtract => "-",
        K::NumpadAdd => "+",
        K::F1 => "F1",
        K::F2 => "F2",
        K::F3 => "F3",
        K::F4 => "F4",
        K::F5 => "F5",
        K::F6 => "F6",
        K::F7 => "F7",
        K::F8 => "F8",
        K::F9 => "F9",
        K::F10 => "F10",
        K::F11 => "F11",
        K::F12 => "F12",
        K::F13 => "F13",
        K::F14 => "F14",
        K::F15 => "F15",
        K::Digit0 => "0",
        K::Digit1 => "1",
        K::Digit2 => "2",
        K::Digit3 => "3",
        K::Digit4 => "4",
        K::Digit5 => "5",
        K::Digit6 => "6",
        K::Digit7 => "7",
        K::Digit8 => "8",
        K::Digit9 => "9",
        _ => {
            let phys = physical_letter(code)?;
            let ch = match logical {
                Some(c) if c.is_ascii_alphabetic() => c.to_ascii_lowercase(),
                _ => phys,
            };
            return Some(ch.to_string());
        }
    };
    Some(s.to_owned())
}

fn physical_letter(code: KeyCode) -> Option<char> {
    use KeyCode as K;
    let letters = [
        (K::KeyA, 'a'),
        (K::KeyB, 'b'),
        (K::KeyC, 'c'),
        (K::KeyD, 'd'),
        (K::KeyE, 'e'),
        (K::KeyF, 'f'),
        (K::KeyG, 'g'),
        (K::KeyH, 'h'),
        (K::KeyI, 'i'),
        (K::KeyJ, 'j'),
        (K::KeyK, 'k'),
        (K::KeyL, 'l'),
        (K::KeyM, 'm'),
        (K::KeyN, 'n'),
        (K::KeyO, 'o'),
        (K::KeyP, 'p'),
        (K::KeyQ, 'q'),
        (K::KeyR, 'r'),
        (K::KeyS, 's'),
        (K::KeyT, 't'),
        (K::KeyU, 'u'),
        (K::KeyV, 'v'),
        (K::KeyW, 'w'),
        (K::KeyX, 'x'),
        (K::KeyY, 'y'),
        (K::KeyZ, 'z'),
    ];
    letters.iter().find(|(k, _)| *k == code).map(|(_, c)| *c)
}

/// `GlfwKeyExtraChar`: characters sent to `OnChar` on key down (and repeat).
pub fn extra_char(code: KeyCode) -> Option<char> {
    match code {
        KeyCode::Backspace => Some('\u{8}'),
        KeyCode::Tab => Some('\t'),
        KeyCode::Enter => Some('\r'),
        KeyCode::Escape => Some('\u{1b}'),
        _ => None,
    }
}

/// Mouse button names (`ui_keyNameMap`).
pub fn mouse_button_name(b: MouseButton) -> Option<&'static str> {
    Some(match b {
        MouseButton::Left => "LEFTBUTTON",
        MouseButton::Middle => "MIDDLEBUTTON",
        MouseButton::Right => "RIGHTBUTTON",
        MouseButton::Back => "MOUSE4",
        MouseButton::Forward => "MOUSE5",
        MouseButton::Other(_) => return None,
    })
}

const KEY_NAMES: &[&str] = &[
    "LEFTBUTTON", "MIDDLEBUTTON", "RIGHTBUTTON", "MOUSE4", "MOUSE5", "WHEELUP", "WHEELDOWN", "BACK", "TAB",
    "RETURN", "ESCAPE", "SHIFT", "CTRL", "ALT", "PAUSE", "PAGEUP", "PAGEDOWN", "END", "HOME", "PRINTSCREEN",
    "INSERT", "DELETE", "UP", "DOWN", "LEFT", "RIGHT", "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9",
    "F10", "F11", "F12", "F13", "F14", "F15", "NUMLOCK", "SCROLLLOCK",
];

/// `ui_main_c::KeyForName`: the canonical held-key name for an
/// `IsKeyDown` query, or `None` if SimpleGraphic would not recognise it.
pub fn canonical_key_name(name: &str) -> Option<String> {
    let mut chars = name.chars();
    let first = chars.next()?;
    if chars.next().is_some() {
        KEY_NAMES.iter().find(|k| k.eq_ignore_ascii_case(name)).map(|k| (*k).to_owned())
    } else if first.is_ascii_alphabetic() {
        Some(first.to_ascii_lowercase().to_string())
    } else if first == ' ' || first.is_ascii_digit() {
        Some(first.to_string())
    } else {
        None
    }
}

/// Double-click detection (`sys_video.cpp` mouse button callback).
#[derive(Default, Debug)]
pub struct DoubleClick {
    last: Option<(Instant, (i32, i32), &'static str)>,
}

impl DoubleClick {
    pub const DELAY: Duration = Duration::from_millis(500);
    pub const RANGE: i32 = 5;

    /// Register a button press at `pos` (window pixels; `None` when the
    /// cursor position is unknown, which disables detection). Returns whether
    /// it is a double click.
    pub fn press(&mut self, button: &'static str, pos: Option<(i32, i32)>, now: Instant) -> bool {
        let Some(pos) = pos else { return false };
        let dbl = matches!(self.last, Some((t, p, b)) if b == button
            && now.duration_since(t) <= Self::DELAY
            && (p.0 - pos.0).abs() <= Self::RANGE
            && (p.1 - pos.1).abs() <= Self::RANGE);
        self.last = if dbl { None } else { Some((now, pos, button)) };
        dbl
    }
}

/// Window configuration.
pub struct HostConfig {
    pub title: String,
    /// Initial inner size in logical pixels.
    pub width: u32,
    pub height: u32,
    pub images: Images,
    pub fonts: Option<Arc<FontSet>>,
}

struct Gfx {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    view_format: wgpu::TextureFormat,
    renderer: Renderer,
}

struct Host<A: App> {
    app: A,
    cfg: HostConfig,
    sg: Sg,
    input: InputState,
    gfx: Option<Gfx>,
    dbl: DoubleClick,
    wheel_acc: f64,
    error: Option<String>,
    inited: bool,
}

impl<A: App> Host<A> {
    fn init_gfx(&mut self, el: &ActiveEventLoop) -> Result<(), String> {
        let attrs = Window::default_attributes()
            .with_title(self.cfg.title.clone())
            .with_inner_size(LogicalSize::new(f64::from(self.cfg.width), f64::from(self.cfg.height)));
        let window = Arc::new(el.create_window(attrs).map_err(|e| e.to_string())?);
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle_from_env(Box::new(
            el.owned_display_handle(),
        )));
        let surface = instance.create_surface(window.clone()).map_err(|e| e.to_string())?;
        let (adapter, device, queue) = pollster::block_on(gpu::request_device(&instance, Some(&surface)))?;
        let caps = surface.get_capabilities(&adapter);
        let (format, view_format) = gpu::pick_surface_format(&caps);
        let size = window.inner_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            color_space: Default::default(),
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes.first().copied().unwrap_or(wgpu::CompositeAlphaMode::Auto),
            view_formats: if view_format == format { vec![] } else { vec![view_format] },
        };
        surface.configure(&device, &config);
        let renderer = Renderer::new(device, queue, self.cfg.images.clone(), self.cfg.fonts.clone());
        self.gfx = Some(Gfx { window, surface, config, view_format, renderer });
        Ok(())
    }

    fn key_down(&mut self, name: &str, dbl: bool) {
        self.input.held.insert(name.to_owned());
        self.app.on_key_down(name, dbl);
    }

    fn key_up(&mut self, name: &str) {
        self.input.held.remove(name);
        // ui_main_c::KeyEvent: F10 toggles the debug UI; SHIFT+PAUSE toggles profiling.
        if name == "F10" || (name == "PAUSE" && self.input.held.contains("SHIFT")) {
            return;
        }
        self.app.on_key_up(name);
    }

    fn wheel(&mut self, up: bool) {
        let name = if up { "WHEELUP" } else { "WHEELDOWN" };
        self.app.on_key_down(name, false);
        self.app.on_key_up(name);
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
            gfx.surface.configure(gfx.renderer.device(), &gfx.config);
        }
        let scale = gfx.window.scale_factor() as f32;
        self.sg.begin_frame(size.width, size.height, scale);
        self.input.update_cursor(self.sg.screen_scale());
        self.app.on_frame(&mut self.sg, &mut self.input);
        let cmds = self.sg.end_frame();
        let clear = self.sg.clear_color();
        let gfx = self.gfx.as_mut().unwrap();
        let frame = match gfx.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => Some(t),
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                gfx.surface.configure(gfx.renderer.device(), &gfx.config);
                None
            }
            _ => None,
        };
        if let Some(frame) = frame {
            let view = frame.texture.create_view(&wgpu::TextureViewDescriptor {
                format: Some(gfx.view_format),
                ..Default::default()
            });
            gfx.renderer.render(&cmds, &view, gfx.view_format, (size.width, size.height), clear);
            gfx.window.pre_present_notify();
            gfx.renderer.queue().present(frame);
        }
        if let Some(t) = self.input.pending_title.take() {
            gfx.window.set_title(&t);
        }
        if self.input.exit_requested {
            self.app.on_exit();
            el.exit();
            return;
        }
        gfx.window.request_redraw();
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
            self.app.on_init(&mut self.sg);
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
            WindowEvent::Resized(_) => {
                if let Some(g) = &self.gfx {
                    g.window.request_redraw();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.input.cursor_phys = Some((position.x, position.y));
            }
            WindowEvent::CursorLeft { .. } => {
                // glfwSetCursorEnterCallback: lastCursorPos.reset() on leave.
                self.input.cursor_phys = None;
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let Some(name) = mouse_button_name(button) else { return };
                match state {
                    ElementState::Pressed => {
                        let pos = self.input.cursor_phys.map(|(x, y)| (x as i32, y as i32));
                        let dbl = self.dbl.press(name, pos, Instant::now());
                        self.key_down(name, dbl);
                    }
                    ElementState::Released => self.key_up(name),
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
                    // Touchpads report many small pixel deltas; GLFW gets one
                    // callback per event, but to keep zoom steps sane we emit
                    // one wheel event per 50 physical pixels.
                    self.wheel_acc += p.y;
                    while self.wheel_acc >= 50.0 {
                        self.wheel_acc -= 50.0;
                        self.wheel(true);
                    }
                    while self.wheel_acc <= -50.0 {
                        self.wheel_acc += 50.0;
                        self.wheel(false);
                    }
                }
            },
            WindowEvent::KeyboardInput { event, .. } => {
                let PhysicalKey::Code(code) = event.physical_key else { return };
                let logical = match &event.logical_key {
                    Key::Character(s) => s.chars().next(),
                    _ => None,
                };
                let name = key_name(code, logical);
                match event.state {
                    ElementState::Pressed => {
                        if let Some(n) = &name {
                            self.key_down(n, false);
                        }
                        if let Some(c) = extra_char(code) {
                            self.app.on_char(c);
                        } else if let Some(text) = event.text.as_ref().filter(|_| {
                            // GLFW only delivers chars for "plain" input
                            // (no CTRL/ALT held), so Ctrl+V is a key, not a 'v'.
                            !self.input.held.contains("CTRL") && !self.input.held.contains("ALT")
                        }) {
                            for c in text.chars().filter(|c| !c.is_control()) {
                                self.app.on_char(c);
                            }
                        }
                    }
                    ElementState::Released => {
                        if let Some(n) = &name {
                            self.key_up(n);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _el: &ActiveEventLoop) {}
}

/// Run `app` in a window until it exits.
pub fn run<A: App>(app: A, cfg: HostConfig) -> Result<(), String> {
    let el = EventLoop::new().map_err(|e| e.to_string())?;
    el.set_control_flow(ControlFlow::Wait);
    let sg = Sg::new(cfg.images.clone(), cfg.fonts.clone());
    let mut host = Host {
        app,
        cfg,
        sg,
        input: InputState::new(),
        gfx: None,
        dbl: DoubleClick::default(),
        wheel_acc: 0.0,
        error: None,
        inited: false,
    };
    el.run_app(&mut host).map_err(|e| e.to_string())?;
    host.error.map_or(Ok(()), Err)
}

/// Run `app` without a window for `frames` frames at `width`x`height`
/// physical pixels and DPI `scale`, rendering offscreen, and return the last
/// frame as RGBA8. Image loads are waited for before the last frame.
pub fn run_headless<A: App>(mut app: A, images: Images, fonts: Option<Arc<FontSet>>, width: u32, height: u32, scale: f32, frames: u32) -> Result<Vec<u8>, String> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let (_adapter, device, queue) = pollster::block_on(gpu::request_device(&instance, None))?;
    let mut renderer = Renderer::new(device, queue, images.clone(), fonts.clone());
    let mut sg = Sg::new(images.clone(), fonts);
    let mut input = InputState::new();
    app.on_init(&mut sg);
    let frames = frames.max(1);
    let mut out = Vec::new();
    for i in 0..frames {
        if i + 1 == frames {
            images.wait_idle();
        }
        sg.begin_frame(width, height, scale);
        app.on_frame(&mut sg, &mut input);
        let cmds = sg.end_frame();
        if i + 1 == frames {
            out = renderer.render_to_rgba(&cmds, (width, height), sg.clear_color());
        } else {
            renderer.sync_images();
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_names() {
        assert_eq!(key_name(KeyCode::KeyA, Some('A')).as_deref(), Some("a"));
        assert_eq!(key_name(KeyCode::KeyQ, Some('a')).as_deref(), Some("a")); // AZERTY: layout letter
        assert_eq!(key_name(KeyCode::KeyZ, None).as_deref(), Some("z"));
        assert_eq!(key_name(KeyCode::Digit7, Some('/')).as_deref(), Some("7"));
        assert_eq!(key_name(KeyCode::Equal, None).as_deref(), Some("+"));
        assert_eq!(key_name(KeyCode::Enter, None).as_deref(), Some("RETURN"));
        assert_eq!(key_name(KeyCode::NumpadEnter, None).as_deref(), Some("RETURN"));
        assert_eq!(key_name(KeyCode::ControlRight, None).as_deref(), Some("CTRL"));
        assert_eq!(key_name(KeyCode::F12, None).as_deref(), Some("F12"));
        assert_eq!(key_name(KeyCode::Space, None).as_deref(), Some(" "));
        assert_eq!(key_name(KeyCode::Numpad5, None), None); // GLFW_KEY_KP_5 is unmapped
        assert_eq!(extra_char(KeyCode::Backspace), Some('\u{8}'));
        assert_eq!(extra_char(KeyCode::NumpadEnter), None);
    }

    #[test]
    fn key_for_name() {
        assert_eq!(canonical_key_name("ctrl").as_deref(), Some("CTRL"));
        assert_eq!(canonical_key_name("LEFTBUTTON").as_deref(), Some("LEFTBUTTON"));
        assert_eq!(canonical_key_name("A").as_deref(), Some("a"));
        assert_eq!(canonical_key_name("5").as_deref(), Some("5"));
        assert_eq!(canonical_key_name(";"), None);
        assert_eq!(canonical_key_name("BOGUS"), None);
        assert_eq!(canonical_key_name(""), None);
        let mut st = InputState::new();
        st.held.insert("CTRL".into());
        st.held.insert("z".into());
        assert!(st.is_key_down("CTRL") && st.is_key_down("Ctrl") && st.is_key_down("Z"));
        assert!(!st.is_key_down("SHIFT"));
    }

    #[test]
    fn double_click() {
        let mut d = DoubleClick::default();
        let t0 = Instant::now();
        assert!(!d.press("LEFTBUTTON", Some((10, 10)), t0));
        assert!(d.press("LEFTBUTTON", Some((14, 6)), t0 + Duration::from_millis(400)));
        // consumed: a third click is a fresh single click
        assert!(!d.press("LEFTBUTTON", Some((14, 6)), t0 + Duration::from_millis(450)));
        // other button, too far, too late
        assert!(!d.press("RIGHTBUTTON", Some((14, 6)), t0 + Duration::from_millis(500)));
        assert!(!d.press("RIGHTBUTTON", Some((20, 6)), t0 + Duration::from_millis(600)));
        assert!(!d.press("RIGHTBUTTON", Some((20, 6)), t0 + Duration::from_millis(1200)));
        assert!(!d.press("LEFTBUTTON", None, t0));
    }

    #[test]
    fn cursor_virtual() {
        let mut st = InputState::new();
        st.cursor_phys = Some((301.7, 99.2));
        st.update_cursor(2.0);
        assert_eq!(st.cursor_pos(), (151, 50)); // lround(301/2)=151 (150.5 rounds away), lround(99/2)=50
    }
}
