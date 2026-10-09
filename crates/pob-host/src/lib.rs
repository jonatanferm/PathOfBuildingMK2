//! `pob-host`: hosts for the PoB UI port (ui doc §5.2). Owner: U7 (loaders: U3).
//!
//! The [`App`] trait is what `pob-app` implements (`launch:OnInit`, `OnFrame`, the key callbacks).
//! Hosts: [`run_desktop`] (winit + pob-sg-wgpu), `run_web` (wasm32), [`run_headless`]
//! (deterministic, windowless; tests and `ui-parity`). Restructured by UI-U0 from the pre-port
//! `pob-sg/src/host.rs` (AMENDMENTS A4).
#![allow(clippy::too_many_lines)]

use pob_sg::Key;

#[cfg(not(target_arch = "wasm32"))]
pub mod desktop;
pub mod headless;
pub mod input;
pub mod pacing;
pub mod script;

#[cfg(not(target_arch = "wasm32"))]
pub mod fs_native;
#[cfg(not(target_arch = "wasm32"))]
pub mod loader_native;
#[cfg(not(target_arch = "wasm32"))]
pub mod net_native;
#[cfg(not(target_arch = "wasm32"))]
pub mod platform_native;

pub mod loader_headless;

#[cfg(target_arch = "wasm32")]
pub mod fs_web;
#[cfg(target_arch = "wasm32")]
pub mod loader_web;
#[cfg(target_arch = "wasm32")]
pub mod net_web;
#[cfg(target_arch = "wasm32")]
pub mod platform_web;
#[cfg(target_arch = "wasm32")]
pub mod web;

#[cfg(not(target_arch = "wasm32"))]
pub use desktop::run_desktop;
pub use headless::run_headless;
#[cfg(target_arch = "wasm32")]
pub use web::run_web;

/// The PoB side of the host (Launch.lua's callbacks).
pub trait App: 'static {
    fn sg(&mut self) -> &mut pob_sg::Sg;
    fn sys(&mut self) -> &mut pob_sg::Sys;
    /// launch:OnInit
    fn on_init(&mut self);
    /// launch:OnFrame (between sg.begin_frame/end_frame)
    fn on_frame(&mut self);
    fn on_key_down(&mut self, key: Key, double_click: bool);
    fn on_key_up(&mut self, key: Key);
    fn on_char(&mut self, ch: Key);
    fn can_exit(&mut self) -> bool {
        true
    }
    fn on_exit(&mut self) {}
    /// hasActiveCoroutine || hasSubscript (ui_main.cpp:389): keeps frames running while unfocused
    fn wants_frames(&self) -> bool {
        false
    }
    /// A callback panicked with a LuaError (D17): show Launch's error prompt (Launch.lua:381 ShowErrMsg).
    fn on_lua_error(&mut self, msg: String);
}

/// built by the host (fonts loaded, loader installed)
pub struct PlatformParts {
    pub sg: pob_sg::Sg,
    pub sys: pob_sg::Sys,
}

/// Window and asset configuration.
pub struct HostConfig {
    pub title: String,
    /// Initial inner size in logical px.
    pub size: (u32, u32),
    pub min_size: (u32, u32),
    /// PoB's `src/` (script dir; relative image paths resolve here).
    pub pob_src: String,
    /// `runtime/SimpleGraphic/` (fonts).
    pub sg_runtime: String,
}

/// The message of a panic payload (`lua_error!` payloads are `pob_lua::LuaError`).
pub fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(e) = payload.downcast_ref::<pob_lua::LuaError>() {
        e.0.clone()
    } else if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_owned()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_owned()
    }
}
