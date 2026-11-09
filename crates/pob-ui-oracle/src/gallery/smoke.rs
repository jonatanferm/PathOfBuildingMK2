//! Gallery "smoke" (U8): the Rust side of `lua/gallery/smoke.lua`, drawing directly through
//! `pob_sg::Sg` (no ported control needed). The mini-launch's frame (`lua/gallery_launch.lua`
//! `launch:OnFrame`) and input collection (`launch:OnKeyDown/OnKeyUp/OnChar`) are inlined.

use pob_host::{App, PlatformParts};
use pob_sg::{Align, Font, Key, Sg, Sys};

enum Ev {
    KeyDown(Key, bool),
    #[allow(dead_code)]
    KeyUp(Key),
    Char(Key),
}

struct Smoke {
    sg: Sg,
    sys: Sys,
    /// main.inputEvents
    events: Vec<Ev>,
    typed: String,
    clicks: i64,
    double_clicks: i64,
    wheel: i64,
}

/// The gallery app.
pub fn app(parts: PlatformParts) -> Box<dyn App> {
    Box::new(Smoke {
        sg: parts.sg,
        sys: parts.sys,
        events: Vec::new(),
        typed: String::new(),
        clicks: 0,
        double_clicks: 0,
        wheel: 0,
    })
}

impl App for Smoke {
    fn sg(&mut self) -> &mut Sg {
        &mut self.sg
    }
    fn sys(&mut self) -> &mut Sys {
        &mut self.sys
    }
    /// G:Init (the ControlHost has no controls: nothing to build)
    fn on_init(&mut self) {}
    /// gallery_launch.lua launch:OnFrame + smoke.lua G:OnFrame
    fn on_frame(&mut self) {
        let (w, h) = self.sg.get_virtual_screen_size();
        for ev in &self.events {
            match ev {
                Ev::Char(k) => self.typed.push_str(&k.name()),
                Ev::KeyDown(k, dbl) => match k {
                    Key::LeftButton => {
                        self.clicks += 1;
                        if *dbl {
                            self.double_clicks += 1;
                        }
                    }
                    Key::WheelUp => self.wheel += 1,
                    Key::WheelDown => self.wheel -= 1,
                    _ => {}
                },
                Ev::KeyUp(_) => {}
            }
        }
        // host:ProcessControlsInput on an empty host: no effect.
        let sg = &mut self.sg;
        sg.set_draw_color(0.1, 0.1, 0.1);
        sg.draw_image(None, 0.0, 0.0, w, h);
        sg.set_draw_layer(None, Some(5));
        sg.set_draw_color(1.0, 1.0, 1.0);
        sg.draw_string(10.0, 10.0, Align::Left, 16.0, Font::Var, "^7Smoke ^xFF8000gallery");
        let width = sg.draw_string_width(16.0, Font::Var, "Smoke gallery");
        sg.draw_image(None, 10.0, 30.0, width, 2.0);
        sg.set_draw_color_esc("^3");
        let status = format!(
            "clicks {} (double {})  wheel {}  typed '{}'",
            self.clicks, self.double_clicks, self.wheel, self.typed
        );
        sg.draw_string(w / 2.0, 40.0, Align::CenterX, 14.0, Font::VarBold, &status);
        sg.draw_string(0.0, 60.0, Align::Right, 12.0, Font::Fixed, "right aligned");
        let (cursor_x, cursor_y) = self.sys.cursor_pos();
        sg.set_viewport(100.0, 100.0, 200.0, 100.0);
        sg.set_draw_color_a(0.0, 1.0, 0.0, 0.5);
        sg.draw_image(None, cursor_x - 104.0, cursor_y - 104.0, 8.0, 8.0);
        sg.draw_image_quad(None, [0.0, 0.0, 50.0, 10.0, 60.0, 60.0, 5.0, 40.0]);
        sg.reset_viewport();
        sg.set_draw_layer(Some(2), None);
        sg.set_draw_color(1.0, 0.0, 0.0);
        sg.draw_image_tc(None, w - 20.0, h - 20.0, 10.0, 10.0, 0.0, 0.0, 1.0, 1.0);
        sg.set_draw_layer(Some(0), None);
        // host:DrawControls on an empty host: no draws.
        self.events.clear();
    }
    fn on_key_down(&mut self, key: Key, double_click: bool) {
        self.events.push(Ev::KeyDown(key, double_click));
    }
    fn on_key_up(&mut self, key: Key) {
        self.events.push(Ev::KeyUp(key));
    }
    fn on_char(&mut self, ch: Key) {
        self.events.push(Ev::Char(ch));
    }
    fn on_lua_error(&mut self, msg: String) {
        panic!("smoke gallery: {msg}");
    }
}
