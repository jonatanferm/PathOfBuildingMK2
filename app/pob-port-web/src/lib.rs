//! Browser entry of the Rust UI port (ui doc §3): wasm32 + Trunk, `pob_host::run_web(...)`.
//! Owner: U7. UI-U0 stub.

#[cfg(target_arch = "wasm32")]
mod web {
    use wasm_bindgen::prelude::*;

    /// Trunk's entry point.
    #[wasm_bindgen(start)]
    pub fn start() {
        wasm_bindgen_futures::spawn_local(async {
            let cfg = pob_host::HostConfig {
                title: "Path of Building".into(),
                size: (1280, 800),
                min_size: (800, 600),
                pob_src: "pob/src".into(),
                sg_runtime: "pob/runtime/SimpleGraphic".into(),
            };
            if let Err(e) = pob_host::run_web("pob_canvas", cfg, pob_app::PobApp::new).await {
                web_sys_log(&format!("{e:?}"));
            }
        });
    }

    fn web_sys_log(msg: &str) {
        let _ = msg; // U7: console.error
    }
}
