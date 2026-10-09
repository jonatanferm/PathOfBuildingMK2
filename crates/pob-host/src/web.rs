//! The web host (ui doc §5.2 "Web host"; wasm32 only). Owner: U7.
//!
//! Same handler as desktop via winit's web backend; canvas size = CSS size x devicePixelRatio;
//! VFS prefetch of synchronously loaded assets before `on_init` (§4.5); paste captured from the
//! DOM `paste` event; copy through `navigator.clipboard.writeText`; a panic is fatal.

use crate::{App, HostConfig, PlatformParts};

pub async fn run_web<A: App>(
    canvas_id: &str,
    cfg: HostConfig,
    make: impl FnOnce(PlatformParts) -> A,
) -> Result<(), wasm_bindgen::JsValue> {
    let _ = (canvas_id, cfg, make);
    todo!("U7: web host")
}
