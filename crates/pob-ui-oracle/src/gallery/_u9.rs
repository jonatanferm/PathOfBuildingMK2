//! Support module of the U9 galleries (`u9_*`): a `Cx`-owning `pob_host::App` that stands in for
//! `lua/gallery_launch.lua` + a gallery's `G:Init`/`G:OnFrame`. Owner: U9.
//!
//! The Lua galleries build `self.host = new("ControlHost"):ControlHost()` and per frame call
//! `self.host:ProcessControlsInput(inputEvents, viewPort)`, then draw, then
//! `self.host:DrawControls(viewPort)`; `launch:OnFrame` sets `main.screenW/H` and `main.viewPort`
//! first and wipes `main.inputEvents` after. [`Gallery`] does the same on the Rust side, with a
//! root node whose class name is "ControlHost" (the control-path markers print it).
#![allow(clippy::type_complexity, clippy::semicolon_if_nothing_returned)]

use pob_host::{App, PlatformParts};
use pob_sg::{Key, Sg, Sys};
use pob_ui_core::main_core::MainCore;
use pob_ui_core::prelude::*;

/// Stand-in for the module-level code of `Classes/Tooltip.lua:19-41` (U10's file), which runs
/// when the Lua side first constructs a TooltipHost (`new("Tooltip")` loads the class): the 14
/// recipe (oil) images are loaded with `"CLAMP"`, in array order (`pairs` over an array part).
/// The galleries call it where the Lua would load the class, so the frame-0 `ImageLoad` side
/// effects match. Remove once U10 ports the preload.
pub fn tooltip_module_load(cx: &mut Cx) {
    const RECIPE_NAMES: [&str; 14] = [
        "AmberOil",
        "AzureOil",
        "BlackOil",
        "ClearOil",
        "CrimsonOil",
        "GoldenOil",
        "IndigoOil",
        "OpalescentOil",
        "PrismaticOil",
        "SepiaOil",
        "SilverOil",
        "TealOil",
        "VerdantOil",
        "VioletOil",
    ];
    for name in RECIPE_NAMES {
        let h = cx.sg.new_image_handle();
        let path = format!("TreeData/{name}.png");
        let before = effects_len(cx);
        h.load(&mut cx.sg, &path, pob_sg::ImageFlags::CLAMP);
        record_image_load(cx, before, &path, pob_sg::ImageFlags::CLAMP);
    }
}

/// The `ImageLoad` side effect of an `imgHandle:Load` the Rust side just made. `pob-sg` does
/// not record side effects yet (CCR-0028 item 2, proposed for U1/U3); the oracle's bridge
/// records the Lua side's loads itself (`sg_bridge.rs` "record unless Sg did"), so a gallery
/// does the same after each load, unless `Sg` already recorded one. Remove with CCR-0028 item 2.
pub fn record_image_load(cx: &mut Cx, before: usize, path: &str, flags: pob_sg::ImageFlags) {
    use pob_sg::trace::{ImgRef, SideEffect};
    if let Some(t) = &mut cx.sg.trace {
        if t.cur.effects.len() == before {
            t.effect(SideEffect::ImageLoad(ImgRef {
                path: path.to_owned(),
                flags,
            }));
        }
    }
}

/// The number of side effects recorded so far in the current frame.
pub fn effects_len(cx: &Cx) -> usize {
    cx.sg.trace.as_ref().map_or(0, |t| t.cur.effects.len())
}

/// `control:SetImage(path)` (Button/Dragger) followed by [`record_image_load`].
pub fn set_image_traced(cx: &mut Cx, path: &str, set: impl FnOnce(&mut Cx)) {
    let before = effects_len(cx);
    set(cx);
    record_image_load(cx, before, path, pob_sg::ImageFlags::empty());
}

/// `new("ControlHost"):ControlHost()`: a bare host node.
pub struct ControlHost;
impl Widget for ControlHost {
    const NAME: &'static str = "ControlHost";
    const CAPS: Caps = Caps::empty();
}

type InitFn = Box<dyn FnOnce(&mut Cx, CtrlId)>;
type FrameFn = Box<dyn FnMut(&mut Cx, CtrlId, &mut InputEvents, &ViewPort)>;

/// A gallery: `init` is `G:Init` (after the host exists), `frame` is `G:OnFrame`.
pub struct Gallery {
    cx: Cx,
    host: CtrlId,
    events: InputEvents,
    init: Option<InitFn>,
    frame: FrameFn,
}

/// The default `G:OnFrame`: `ProcessControlsInput`, then `DrawControls`.
pub fn default_frame(cx: &mut Cx, host: CtrlId, ev: &mut InputEvents, vp: &ViewPort) {
    cx.process_controls_input(host, ev, vp);
    cx.draw_controls(host, vp, None);
}

impl Gallery {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        parts: PlatformParts,
        init: impl FnOnce(&mut Cx, CtrlId) + 'static,
        frame: impl FnMut(&mut Cx, CtrlId, &mut InputEvents, &ViewPort) + 'static,
    ) -> Box<dyn App> {
        let mut cx = Cx {
            ui: pob_ui_core::arena::Ui::default(),
            sg: parts.sg,
            sys: parts.sys,
            main: MainCore::new(CtrlId::default()),
            ext: Box::new(()),
        };
        let host = cx
            .add_free(
                None,
                New::new(Base::new(Anchor::NONE, rect(0, 0, 0, 0)), ControlHost).host(),
            )
            .id();
        Box::new(Gallery {
            cx,
            host,
            events: InputEvents::default(),
            init: Some(Box::new(init)),
            frame: Box::new(frame),
        })
    }
}

impl App for Gallery {
    fn sg(&mut self) -> &mut Sg {
        &mut self.cx.sg
    }
    fn sys(&mut self) -> &mut Sys {
        &mut self.cx.sys
    }
    fn on_init(&mut self) {
        if let Some(f) = self.init.take() {
            f(&mut self.cx, self.host);
        }
    }
    fn on_frame(&mut self) {
        // launch:OnFrame
        let (w, h) = self.cx.sg.get_virtual_screen_size();
        self.cx.main.screen_w = w;
        self.cx.main.screen_h = h;
        let vp = ViewPort {
            x: 0.0,
            y: 0.0,
            width: w,
            height: h,
        };
        self.cx.main.view_port = vp;
        (self.frame)(&mut self.cx, self.host, &mut self.events, &vp);
        self.events.wipe();
        self.cx.end_frame_gc();
    }
    fn on_key_down(&mut self, key: Key, double_click: bool) {
        self.events.push(InputEvent {
            kind: EventKind::KeyDown { double_click },
            key,
        });
    }
    fn on_key_up(&mut self, key: Key) {
        self.events.push(InputEvent {
            kind: EventKind::KeyUp,
            key,
        });
    }
    fn on_char(&mut self, ch: Key) {
        self.events.push(InputEvent {
            kind: EventKind::Char,
            key: ch,
        });
    }
    fn on_lua_error(&mut self, msg: String) {
        panic!("u9 gallery: {msg}");
    }
}
