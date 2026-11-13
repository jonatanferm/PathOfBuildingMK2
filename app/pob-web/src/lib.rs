//! Browser entry of the Rust UI port (ui doc §3): wasm32 + Trunk, `pob_host::run_web(...)`.
//! Owner: U7.
//!
//! The page runs PoB's app (`pob_app::PobApp`, with the calc in a Web Worker); U53's cut-over
//! made it the default (it was `?app=pob` before). `?app=testcard` runs the SimpleGraphic
//! [`TestCard`] instead. A panic is fatal on wasm32 (ui doc §13 item 6).
//!
//! Serving: Trunk serves `dist/`; PoB's files are expected under `pob/` next to `index.html`
//! (`pob/src/...`, `pob/runtime/SimpleGraphic/Fonts/...`, optionally `pob/web-prefetch.txt`,
//! see `pob_host::web`), e.g. a symlink to `.PathOfBuilding` in `dist/`.
//!
//! CCR-0134 §6 (U54): the app also needs `pob-dump compile`'s output under
//! `pob/dump/` (`data.bin`, `trees/<ver>.bin`; `?data=<url>` overrides), and the calc runs in a
//! Web Worker started from this same module (`pob_app::calc::worker_web`).

use pob_host::{App, PlatformParts};
use pob_sg::{Align, Font, Key, Sg, Sys};

/// The SimpleGraphic test card: every face and height, the palette and escape leak, layers and
/// viewports, alpha, the cursor (`GetCursorPos`), the clock, and the last input events (key
/// names as SG delivers them, double clicks, wheel, OnChar incl. `?` for non-ASCII).
pub struct TestCard {
    pub sg: Sg,
    pub sys: Sys,
    events: Vec<String>,
    typed: String,
    frames: u64,
}

impl TestCard {
    pub fn new(parts: PlatformParts) -> TestCard {
        TestCard {
            sg: parts.sg,
            sys: parts.sys,
            events: Vec::new(),
            typed: String::new(),
            frames: 0,
        }
    }

    fn event(&mut self, e: String) {
        self.events.push(e);
        if self.events.len() > 12 {
            self.events.remove(0);
        }
    }
}

impl App for TestCard {
    fn sg(&mut self) -> &mut Sg {
        &mut self.sg
    }
    fn sys(&mut self) -> &mut Sys {
        &mut self.sys
    }
    fn on_init(&mut self) {
        self.sys.platform.set_window_title("pob-sg test card");
    }
    fn on_frame(&mut self) {
        self.frames += 1;
        let (sw, sh) = self.sg.get_virtual_screen_size();
        let (cx, cy) = self.sys.cursor_pos();
        let t = self.sys.time_ms();
        let sg = &mut self.sg;

        // Background and title.
        sg.set_draw_layer(None, Some(-100));
        sg.set_draw_color(0.06, 0.06, 0.08);
        sg.draw_image(None, 0.0, 0.0, sw, sh);
        sg.set_draw_layer(None, Some(0));
        sg.set_draw_color(1.0, 1.0, 1.0);
        sg.draw_string(
            10.0,
            6.0,
            Align::Left,
            32.0,
            Font::FontinSc,
            "^xC8C8C8pob-host ^7test card",
        );

        // Every face at heights 10..32.
        let mut y = 46.0;
        for f in Font::ALL {
            sg.set_draw_color(0.7, 0.7, 0.7);
            sg.draw_string(10.0, y + 10.0, Align::Left, 14.0, Font::Var, f.lua_name());
            let mut x = 150.0;
            for h in (10..=32).step_by(2) {
                let s = format!("^{}Ag^7{h}", h / 2 % 10);
                sg.set_draw_color(1.0, 1.0, 1.0);
                sg.draw_string(x, y + f64::from(32 - h), Align::Left, f64::from(h), f, &s);
                x += sg.draw_string_width(f64::from(h), f, &s) + 6.0;
            }
            y += 36.0;
        }

        // Palette, then the escape leak: the quad after the string takes ^3's colour.
        sg.set_draw_color(1.0, 1.0, 1.0);
        sg.draw_string(
            10.0,
            y,
            Align::Left,
            18.0,
            Font::VarBold,
            "^7palette: ^00 ^11 ^22 ^33 ^44 ^55 ^66 ^77 ^88 ^99 ^3leak",
        );
        sg.draw_image(None, 520.0, y + 2.0, 14.0, 14.0);
        y += 28.0;

        // Alignment against the virtual screen width.
        sg.set_draw_color(1.0, 1.0, 1.0);
        for (a, label) in [
            (Align::Left, "LEFT"),
            (Align::Center, "CENTER"),
            (Align::Right, "RIGHT"),
        ] {
            sg.draw_string(0.0, y, a, 16.0, Font::Var, label);
        }
        y += 24.0;

        // Alpha and layers: a translucent box on a higher layer over the event log.
        sg.set_draw_layer(None, Some(10));
        sg.set_draw_color_a(0.2, 0.6, 1.0, 0.35);
        sg.draw_image(None, 5.0, y, 420.0, 16.0 * 13.0);
        sg.set_draw_layer(None, Some(0));

        // A clipped viewport.
        sg.set_viewport(450.0, y, 300.0, 60.0);
        sg.set_draw_color(0.3, 0.3, 0.3);
        sg.draw_image(None, 0.0, 0.0, 300.0, 60.0);
        sg.set_draw_color(1.0, 1.0, 1.0);
        sg.draw_string(
            -40.0,
            20.0,
            Align::Left,
            20.0,
            Font::Var,
            "clipped by the viewport, clipped by the viewport",
        );
        sg.reset_viewport();

        // Event log, typed text, status.
        sg.set_draw_color(1.0, 1.0, 1.0);
        let mut ly = y;
        for e in &self.events {
            sg.draw_string(10.0, ly, Align::Left, 16.0, Font::Fixed, e);
            ly += 16.0;
        }
        sg.draw_string(
            10.0,
            ly,
            Align::Left,
            16.0,
            Font::Fixed,
            &format!("typed: {}", self.typed),
        );
        let status = format!(
            "frame {}  t={} ms  cursor=({cx}, {cy})  screen={sw}x{sh}  scale={}",
            self.frames,
            t,
            sg.get_screen_scale()
        );
        sg.draw_string(6.0, sh - 19.0, Align::Left, 16.0, Font::Fixed, &status);

        // Cursor crosshair on top.
        sg.set_draw_layer(None, Some(100));
        sg.set_draw_color(1.0, 0.8, 0.2);
        sg.draw_image(None, cx - 8.0, cy, 17.0, 1.0);
        sg.draw_image(None, cx, cy - 8.0, 1.0, 17.0);
        sg.set_draw_layer(None, Some(0));
    }
    fn on_key_down(&mut self, key: Key, double_click: bool) {
        let ctrl = self.sys.is_key_down(Key::Ctrl);
        if ctrl && key == Key::Char(b'v') {
            if let Some(p) = self.sys.platform.paste() {
                self.typed.push_str(&p);
            }
        } else if ctrl && key == Key::Char(b'c') {
            let t = self.typed.clone();
            self.sys.platform.copy(&t);
        }
        self.event(format!(
            "OnKeyDown {:?}{}",
            key.name(),
            if double_click { " (double)" } else { "" }
        ));
    }
    fn on_key_up(&mut self, key: Key) {
        self.event(format!("OnKeyUp   {:?}", key.name()));
    }
    fn on_char(&mut self, ch: Key) {
        match ch {
            Key::Char(8) => {
                self.typed.pop();
            }
            Key::Char(b) if !b.is_ascii_control() => self.typed.push(char::from(b)),
            Key::Unknown => self.typed.push('?'),
            _ => {}
        }
        if self.typed.chars().count() > 40 {
            self.typed.remove(0);
        }
        self.event(format!("OnChar    {:?}", ch.name()));
    }
    fn on_lua_error(&mut self, msg: String) {
        self.event(format!("error: {msg}"));
    }
}

#[cfg(target_arch = "wasm32")]
mod web {
    use wasm_bindgen::prelude::*;

    /// Trunk's entry point.
    #[wasm_bindgen(start)]
    pub fn start() {
        // CCR-0134 §6: this module also runs in the calc's Web Worker (pob_app's WorkerCalc,
        // calc doc D15), which must not start the app
        if pob_app::calc::worker_web::in_worker() {
            return;
        }
        wasm_bindgen_futures::spawn_local(async {
            let cfg = pob_host::HostConfig {
                title: "Path of Building".into(),
                size: (1280, 800),
                min_size: (800, 600),
                pob_src: "pob/src".into(),
                sg_runtime: "pob/runtime/SimpleGraphic".into(),
            };
            let search = web_sys::window()
                .and_then(|w| w.location().search().ok())
                .unwrap_or_default();
            // U53 (cut-over): PoB's app is the page; `?app=testcard` shows the test card
            let pob = !search.contains("app=testcard");
            let r = if pob {
                // CCR-0134 §6: the game data (`?data=<url>`, default `pob/dump`) and the calc in a
                // Web Worker (WorkerCalc, calc doc D15)
                use pob_app::calc::worker_web::{self, WebWorkerConfig, WorkerCalc};
                let data_base = search
                    .trim_start_matches('?')
                    .split('&')
                    .find_map(|kv| kv.strip_prefix("data="))
                    .map_or_else(|| worker_web::DEFAULT_DATA_BASE.to_owned(), str::to_owned);
                if let Err(e) = worker_web::init_web_data(&data_base).await {
                    web_sys::console::error_1(&e);
                    return;
                }
                let spawn = worker_web::web_spawner(WebWorkerConfig {
                    glue_url: None,
                    data_base: Some(data_base),
                });
                pob_host::run_web("pob_canvas", cfg, move |parts| {
                    pob_app::PobApp::with_calc(
                        parts,
                        std::rc::Rc::new(move || {
                            Box::new(WorkerCalc::new(spawn.clone()))
                                as Box<dyn pob_app::calc::CalcFacade>
                        }),
                    )
                })
                .await
            } else {
                pob_host::run_web("pob_canvas", cfg, super::TestCard::new).await
            };
            if let Err(e) = r {
                web_sys::console::error_1(&e);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The card runs headless: deterministic traces, input reflected in the log.
    #[test]
    fn headless_card() {
        let script = pob_host::script::Script::parse(
            "screen 1280 800 dpi 1.25\nframes 1\nmove 100 120\nclick LEFTBUTTON\nclick LEFTBUTTON\ntype \"hé\"\npaste-clipboard \"P\"\nkey-down CTRL ; key-down v ; key-up v ; key-up CTRL\nwheel up\nframes 1",
        )
        .unwrap();
        let run = || {
            let mut card = TestCard::new(pob_host::headless::parts_without_assets());
            let t = pob_host::run_headless(&mut card, &script);
            (serde_json::to_string(&t).unwrap(), card.events, card.typed)
        };
        let (a, events, typed) = run();
        let (b, ..) = run();
        assert_eq!(a, b);
        assert_eq!(typed, "h?P");
        assert!(events
            .iter()
            .any(|e| e == "OnKeyDown \"LEFTBUTTON\" (double)"));
        assert_eq!(
            events.last().map(String::as_str),
            Some("OnKeyUp   \"WHEELUP\"")
        );
    }
}
