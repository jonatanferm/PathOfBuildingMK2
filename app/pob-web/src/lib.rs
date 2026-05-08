//! Browser entry point for Path of Building MK2.
//!
//! Trunk picks this up as a cdylib via `<link data-trunk rel="rust" />` in
//! `index.html`. The exported `start` function is invoked by Trunk's bootstrap
//! script with the canvas element id.
//!
//! On non-wasm targets this crate compiles to an empty rlib so it stays in the
//! workspace `cargo check`.

#![cfg(target_arch = "wasm32")]

use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
    let _ = console_log::init_with_level(log::Level::Info);

    // Force the WebGL2 backend for wgpu. wgpu 22's WebGPU code-path requests
    // device limits (`maxInterStageShaderComponents`) the current browser
    // WebGPU spec doesn't recognise, and the request errors out. WebGL2 has
    // no such issue. Bumping wgpu would be the alternative fix.
    let mut wgpu_options = eframe::egui_wgpu::WgpuConfiguration::default();
    wgpu_options.supported_backends = eframe::wgpu::Backends::GL;
    let web_options = eframe::WebOptions {
        wgpu_options,
        ..Default::default()
    };

    wasm_bindgen_futures::spawn_local(async {
        let document = web_sys::window()
            .expect("window")
            .document()
            .expect("document");
        let canvas = document
            .get_element_by_id("pob_canvas")
            .expect("#pob_canvas")
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("element is a canvas");

        eframe::WebRunner::new()
            .start(
                canvas,
                web_options,
                Box::new(|cc| Ok(Box::new(pob_ui::PobApp::new(cc)))),
            )
            .await
            .expect("eframe WebRunner");
    });
}
