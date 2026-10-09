//! UI-U0 acceptance (ui doc §12.2): the §9.1 (`ButtonControl`) and §9.2 (`NotesTab`) reference
//! bodies compile against the frozen pob-ui-core surface. Nothing here runs: the stubs' bodies
//! are `todo!()`; the test only proves the contracts fit the binding porting style.
//!
//! The bodies are copied verbatim from docs/port/ui-architecture.md §9.1/§9.2. The only edits,
//! each marked `// CCR-0013`, are forced by this file being a separate crate or by a mismatch
//! with the calc plan's frozen crates:
//! - `impl New<ButtonControl> { .. }` is an inherent impl on a foreign type here, so it becomes an
//!   extension trait (inside pob-ui-core, where U9 writes it, the inherent impl is legal);
//! - `pob_lua::{gsub, lmatch}` become `pob_ui_core::luastr::{gsub, lmatch}` (pob_lua's `gsub`
//!   returns `(String, count)` and its `match` returns captures);
//! - `XmlNode::children()` / `push_text()` come from a local extension trait over the calc plan's
//!   real `pob_calc::model::XmlNode` (which has a pub `children` field and no `push_text`);
//! - `crate::main_mode::background::draw_background` (pob-app, U17) is a local shim.
#![allow(dead_code, unused_imports, clippy::all, clippy::pedantic)]

/// `crate::prelude` / `crate::tooltip` as seen from inside pob-ui-core.
mod prelude {
    pub use pob_ui_core::prelude::*;
}
mod tooltip {
    pub use pob_ui_core::tooltip::*;
}

/// `crate::model::xml` as seen from inside pob-app (calc:BM-A's XmlNode).
mod model {
    pub mod xml {
        pub use pob_calc::model::{XmlChild, XmlNode};

        // CCR-0013: helpers the §9.2 body assumes.
        pub trait XmlNodeExt {
            fn children(&self) -> std::slice::Iter<'_, XmlChild>;
            fn push_text(&mut self, text: String);
        }
        impl XmlNodeExt for XmlNode {
            fn children(&self) -> std::slice::Iter<'_, XmlChild> {
                self.children.iter()
            }
            fn push_text(&mut self, text: String) {
                self.children.push(XmlChild::Text(text.into()));
            }
        }
    }
}

/// `crate::main_mode::background` as seen from inside pob-app (U17 owns the real one).
mod main_mode {
    pub mod background {
        use pob_ui_core::prelude::*;
        /// lua: Main.lua:1486-1496 main:DrawBackground(viewPort)
        pub fn draw_background(cx: &mut Cx, vp: &ViewPort) {
            let _ = (cx, vp);
            todo!("U17: main:DrawBackground")
        }
    }
}

// ---------------------------------------------------------------------------------------------
// ui doc §9.1 `ButtonControl` (complete)
// ---------------------------------------------------------------------------------------------
#[rustfmt::skip]
mod button_9_1 {
    //! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/ButtonControl.lua
    use std::rc::Rc;
    use pob_sg::{Align, Font, ImageFlags, ImageHandle, Key};
    use crate::{prelude::*, tooltip::TooltipHost};

    pub type ClickFn = Rc<dyn Fn(&mut Cx) -> Focus>;

    pub struct ButtonControl {
        pub tt: TooltipHost<()>,
        pub label: Prop<String>,
        pub on_click: ClickFn,
        pub on_hover: Option<Rc<dyn Fn(&mut Cx)>>,
        pub force_tooltip: bool,
        pub locked: Prop<bool>,                 // GetProperty("locked"); nil => false
        pub enter_func: Option<Rc<dyn Fn(&mut Cx)>>,
        pub image: Option<ImageHandle>,
        pub over_size_text: f64,                // `self.overSizeText or 0`
        pub clicked: bool,
    }

    impl ButtonControl {
        /// lua: ButtonControl.lua:9-17 ButtonControl(anchor, rect, label, onClick, onHover, forceTooltip)
        pub fn new(anchor: Anchor, rect: [Prop<f64>; 4], label: impl Into<Prop<String>>,
                   on_click: impl Fn(&mut Cx) + 'static) -> New<ButtonControl> {
            Self::new_ret(anchor, rect, label, move |cx| { on_click(cx); None })
        }
        /// For onClick functions whose return value Lua feeds into SelectControl (`return self.onClick()` in OnKeyUp).
        pub fn new_ret(anchor: Anchor, rect: [Prop<f64>; 4], label: impl Into<Prop<String>>,
                       on_click: impl Fn(&mut Cx) -> Focus + 'static) -> New<ButtonControl> {
            New::new(Base::new(anchor, rect), ButtonControl {
                tt: TooltipHost::new(None), label: label.into(), on_click: Rc::new(on_click), on_hover: None,
                force_tooltip: false, locked: Prop::Val(false), enter_func: None, image: None,
                over_size_text: 0.0, clicked: false,
            })
        }
        /// lua: ButtonControl.lua:25-32
        pub fn set_image(cx: &mut Cx, me: CtrlId, path: Option<&str>) {
            let img = path.map(|p| { let h = cx.sg.new_image_handle(); h.load(&mut cx.sg, p, ImageFlags::empty()); h });
            cx.w_mut::<Self>(me).image = img;
        }
    }
    // CCR-0013: `impl New<ButtonControl> { .. }` (foreign type here) as an extension trait.
    pub trait NewButtonControlExt {
        fn on_hover(self, f: impl Fn(&mut Cx) + 'static) -> Self;
        fn force_tooltip(self, v: bool) -> Self;
    }
    impl NewButtonControlExt for New<ButtonControl> {
        /// 5th and 6th constructor arguments.
        fn on_hover(mut self, f: impl Fn(&mut Cx) + 'static) -> Self { self.w.on_hover = Some(Rc::new(f)); self }
        fn force_tooltip(mut self, v: bool) -> Self { self.w.force_tooltip = v; self }
    }

    impl Widget for ButtonControl {
        const NAME: &'static str = "ButtonControl";
        const CAPS: Caps = Caps::DRAW.union(Caps::IS_MOUSE_OVER).union(Caps::KEY_DOWN).union(Caps::KEY_UP).union(Caps::CLICK);

        /// lua: ButtonControl.lua:19-23
        fn click(cx: &mut Cx, me: CtrlId) {
            if cx.is_shown(me) && cx.is_enabled(me) { let f = cx.w::<Self>(me).on_click.clone(); f(cx); }
        }

        /// lua: ButtonControl.lua:34-39
        fn is_mouse_over(cx: &Cx, me: CtrlId) -> bool {
            if !cx.is_shown(me) { return false; }
            cx.is_mouse_in_bounds(me)
        }

        /// lua: ButtonControl.lua:41-105
        fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, no_tooltip: bool) {
            let (x, y) = cx.pos(me);
            let (width, height) = cx.size(me);
            let enabled = cx.is_enabled(me);
            let m_over = cx.is_mouse_over(me);
            let locked = cx.prop::<Self, _>(me, |w| &w.locked);
            if !enabled { cx.sg.set_draw_color(0.33, 0.33, 0.33) }
            else if m_over || locked { cx.sg.set_draw_color(1.0, 1.0, 1.0) }
            else { cx.sg.set_draw_color(0.5, 0.5, 0.5) }
            cx.sg.draw_image(None, x, y, width, height);
            let (clicked, image) = { let w = cx.w::<Self>(me); (w.clicked, w.image.clone()) };
            if !enabled { cx.sg.set_draw_color(0.0, 0.0, 0.0) }
            else if clicked && m_over { cx.sg.set_draw_color(0.5, 0.5, 0.5) }
            else if m_over || locked { cx.sg.set_draw_color(0.33, 0.33, 0.33) }
            else { cx.sg.set_draw_color(0.0, 0.0, 0.0) }
            cx.sg.draw_image(None, x + 1.0, y + 1.0, width - 2.0, height - 2.0);
            if let Some(image) = image {
                if enabled { cx.sg.set_draw_color(1.0, 1.0, 1.0) } else { cx.sg.set_draw_color(0.33, 0.33, 0.33) }
                cx.sg.draw_image(Some(&image), x + 2.0, y + 2.0, width - 4.0, height - 4.0);
                if clicked && m_over {
                    cx.sg.set_draw_color_a(1.0, 1.0, 1.0, 0.5);
                    cx.sg.draw_image(None, x + 1.0, y + 1.0, width - 2.0, height - 2.0);
                }
            }
            if enabled { cx.sg.set_draw_color(1.0, 1.0, 1.0) } else { cx.sg.set_draw_color(0.33, 0.33, 0.33) }
            let label = cx.prop::<Self, _>(me, |w| &w.label);
            if label == "+" {
                cx.sg.draw_image(None, x + width * 0.2, y + height * 0.45, width * 0.6, height * 0.1);
                cx.sg.draw_image(None, x + width * 0.45, y + height * 0.2, width * 0.1, height * 0.6);
            } else if label == "-" {
                cx.sg.draw_image(None, x + width * 0.2, y + height * 0.45, width * 0.6, height * 0.1);
            } else if label == "x" {
                cx.sg.draw_image_quad(None, [x + width * 0.2, y + height * 0.3, x + width * 0.3, y + height * 0.2,
                                             x + width * 0.8, y + height * 0.7, x + width * 0.7, y + height * 0.8]);
                cx.sg.draw_image_quad(None, [x + width * 0.7, y + height * 0.2, x + width * 0.8, y + height * 0.3,
                                             x + width * 0.3, y + height * 0.8, x + width * 0.2, y + height * 0.7]);
            } else {
                let over_size = cx.w::<Self>(me).over_size_text;
                cx.sg.draw_string(x + width / 2.0, y + 2.0 - over_size, Align::CenterX, height - 4.0 + over_size * 2.0, Font::Var, &label);
            }
            if m_over {
                if !no_tooltip || cx.w::<Self>(me).force_tooltip {
                    cx.sg.set_draw_layer(None, Some(100));
                    TooltipHost::draw_tooltip::<Self>(cx, me, |w| &mut w.tt, x, y, width, height, vp, ());
                    cx.sg.set_draw_layer(None, Some(0));
                }
                if let Some(h) = cx.w::<Self>(me).on_hover.clone() { h(cx); }   // return value ignored by DrawControls
            }
        }

        /// lua: ButtonControl.lua:107-117
        fn on_key_down(cx: &mut Cx, me: CtrlId, key: Key, _double_click: bool) -> Focus {
            if !cx.is_shown(me) || !cx.is_enabled(me) { return None; }
            if key == Key::LeftButton { cx.w_mut::<Self>(me).clicked = true; }
            else if let Some(f) = cx.w::<Self>(me).enter_func.clone() { f(cx); }
            Some(me)
        }

        /// lua: ButtonControl.lua:119-130
        fn on_key_up(cx: &mut Cx, me: CtrlId, key: Key) -> Focus {
            if !cx.is_shown(me) || !cx.is_enabled(me) { return None; }
            if key == Key::LeftButton && cx.w::<Self>(me).clicked {
                cx.w_mut::<Self>(me).clicked = false;
                if cx.is_mouse_over(me) { let f = cx.w::<Self>(me).on_click.clone(); return f(cx); }
            }
            cx.w_mut::<Self>(me).clicked = false;
            None
        }
    }
}

// ---------------------------------------------------------------------------------------------
// ui doc §9.2 `NotesTab` (complete)
// ---------------------------------------------------------------------------------------------
#[rustfmt::skip]
mod notes_9_2 {
    //! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/NotesTab.lua
    use pob_ui_core::luastr::{gsub, lmatch}; // CCR-0013: §9.2 says `use pob_lua::{gsub, lmatch};`
    use crate::model::xml::{XmlChild, XmlNode};   // calc:BM-A port of runtime/lua/xml.lua
    use crate::model::xml::XmlNodeExt; // CCR-0013
    use pob_ui_core::{prelude::*, color_codes::ColorCodes, controls::{ButtonControl, EditArgs, EditControl, LabelControl}};
    use crate::main_mode::background::draw_background;

    const NOTES_DESC: &str = "^7You can use Ctrl +/- (or Ctrl+Scroll) to zoom in and out and Ctrl+0 to reset.\n\
This field also supports different colors.  Using the caret symbol (^) followed by a Hex code or a number (0-9) will set the color.\n\
Below are some common color codes PoB uses:\t";

    pub struct NotesTab { pub build: CtrlId, pub last_content: String, pub show_color_codes: bool, pub mod_flag: bool }
    impl Widget for NotesTab { const NAME: &'static str = "NotesTab"; const CAPS: Caps = Caps::empty(); } // drawn by BuildMode

    type CcField = fn(&ColorCodes) -> &String;

    impl NotesTab {
        /// lua: NotesTab.lua:12-51
        pub fn new(build: CtrlId) -> New<NotesTab> {
            New::new(Base::new(Anchor::NONE, rect(0, 0, 0, 0)),
                     NotesTab { build, last_content: String::new(), show_color_codes: false, mod_flag: false })
                .host()
                .init(|cx, me| {
                    let me = me.id();
                    let notes_desc = cx.add(me, "notesDesc", LabelControl::new(Anchor::new(TopLeft, me, TopLeft), rect(8, 8, 150, 16), NOTES_DESC));
                    // The label is read at construction; the click handler reads colorCodes at click time, as the Lua does.
                    let btn = |cx: &mut Cx, key: &'static str, to: CtrlId, x: i32, y: i32, cc: CcField, name: &str| {
                        let label = format!("{}{}", cc(&cx.main.cc), name);
                        cx.add(me, key, ButtonControl::new(Anchor::new(TopLeft, to, TopLeft), rect(x, y, 100, 18), label,
                            move |cx| { let c = cc(&cx.main.cc).clone(); NotesTab::set_color(cx, me, &c) })).id()
                    };
                    let normal = btn(cx, "normal", notes_desc.id(), 0, 48, |c| &c.NORMAL, "NORMAL");
                    let magic = btn(cx, "magic", normal, 120, 0, |c| &c.MAGIC, "MAGIC");
                    let rare = btn(cx, "rare", magic, 120, 0, |c| &c.RARE, "RARE");
                    btn(cx, "unique", rare, 120, 0, |c| &c.UNIQUE, "UNIQUE");
                    let fire = btn(cx, "fire", normal, 0, 18, |c| &c.FIRE, "FIRE");
                    let cold = btn(cx, "cold", fire, 120, 0, |c| &c.COLD, "COLD");
                    let lightning = btn(cx, "lightning", cold, 120, 0, |c| &c.LIGHTNING, "LIGHTNING");
                    btn(cx, "chaos", lightning, 120, 0, |c| &c.CHAOS, "CHAOS");
                    let strength = btn(cx, "strength", fire, 0, 18, |c| &c.STRENGTH, "STRENGTH");
                    let dexterity = btn(cx, "dexterity", strength, 120, 0, |c| &c.DEXTERITY, "DEXTERITY");
                    let intelligence = btn(cx, "intelligence", dexterity, 120, 0, |c| &c.INTELLIGENCE, "INTELLIGENCE");
                    cx.add(me, "default", ButtonControl::new(Anchor::new(TopLeft, intelligence, TopLeft), rect(120, 0, 100, 18),
                        "^7DEFAULT", move |cx| NotesTab::set_color(cx, me, "^7")));
                    // EditControl(anchor, rect, init, prompt, filter, limit, changeFunc, lineHeight, allowZoom)
                    let edit = cx.add(me, "edit", EditControl::new(Anchor::new(TopLeft, fire, TopLeft), rect(0, 48, 0, 0),
                        EditArgs { init: Some(String::new()), filter: Some("^%C\t\n".into()), line_height: Some(16.0),
                                   allow_zoom: true, ..Default::default() }));
                    cx.base_mut(edit).width = Prop::f(move |cx, _| cx.base(me).width.raw() - 16.0);   // `self.width - 16` (raw)
                    cx.base_mut(edit).height = Prop::f(move |cx, _| cx.base(me).height.raw() - 128.0);
                    cx.add(me, "toggleColorCodes", ButtonControl::new(Anchor::new(TopRight, me, TopRight), rect(-10, 70, 160, 20),
                        "Show Color Codes", move |cx| {
                            let s = !cx.w::<NotesTab>(me).show_color_codes;
                            cx.w_mut::<NotesTab>(me).show_color_codes = s;
                            NotesTab::set_show_color_codes(cx, me, s);
                        }));
                    cx.select_control(me, Some(edit.id()));
                })
        }

        /// lua: NotesTab.lua:53-62
        pub fn set_show_color_codes(cx: &mut Cx, me: CtrlId, setting: bool) {
            cx.w_mut::<NotesTab>(me).show_color_codes = setting;
            let toggle = cx.child_as::<ButtonControl>(me, "toggleColorCodes");
            let edit = cx.child_as::<EditControl>(me, "edit");
            if setting {
                cx.get_mut(toggle).label = "Hide Color Codes".into();
                let buf = gsub(&gsub(&cx.get(edit).buf, "%^x(%x%x%x%x%x%x)", "^_x%1"), "%^(%d)", "^_%1");
                cx.get_mut(edit).buf = buf;
            } else {
                cx.get_mut(toggle).label = "Show Color Codes".into();
                let buf = gsub(&gsub(&cx.get(edit).buf, "%^_x(%x%x%x%x%x%x)", "^x%1"), "%^_(%d)", "^%1");
                cx.get_mut(edit).buf = buf;
            }
        }

        /// lua: NotesTab.lua:64-73
        pub fn set_color(cx: &mut Cx, me: CtrlId, color: &str) {
            let show = cx.w::<NotesTab>(me).show_color_codes;
            let mut text = color.to_owned();
            if show { text = gsub(&gsub(color, "%^x(%x%x%x%x%x%x)", "^_x%1"), "%^(%d)", "^_%1"); }
            let edit = cx.child_as::<EditControl>(me, "edit");
            let (sel, caret) = { let e = cx.get(edit); (e.sel, e.caret) };
            if sel.is_none() || sel == Some(caret) {
                EditControl::insert(cx, edit.id(), &text);
            } else {
                let last_color = lmatch(&EditControl::get_sel_text(cx, edit.id()),
                                        if show { "^.*(%^_x%x%x%x%x%x%x)" } else { "^.*(%^x%x%x%x%x%x%x)" })
                    .unwrap_or_else(|| "^7".to_owned());
                let stripped = gsub(&EditControl::get_sel_text(cx, edit.id()),
                                    if show { "%^_x%x%x%x%x%x%x" } else { "%^x%x%x%x%x%x%x" }, "");
                EditControl::replace_sel(cx, edit.id(), &format!("{text}{stripped}{last_color}"));
            }
        }

        /// lua: NotesTab.lua:75-82
        pub fn load(cx: &mut Cx, me: CtrlId, xml: &XmlNode, _file_name: &str) {
            let edit = cx.child_as::<EditControl>(me, "edit");
            for node in xml.children() {
                if let XmlChild::Text(t) = node { EditControl::set_text(cx, edit.id(), t, false); }
            }
            let buf = cx.get(edit).buf.clone();
            cx.w_mut::<NotesTab>(me).last_content = buf;
        }

        /// lua: NotesTab.lua:84-88
        pub fn save(cx: &mut Cx, me: CtrlId, xml: &mut XmlNode) {
            Self::set_show_color_codes(cx, me, false);
            let buf = cx.get(cx.child_as::<EditControl>(me, "edit")).buf.clone();
            xml.push_text(buf.clone());
            cx.w_mut::<NotesTab>(me).last_content = buf;
        }

        /// lua: NotesTab.lua:90-112
        pub fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, ev: &mut InputEvents) {
            {
                let b = cx.base_mut(me);
                b.x = vp.x.into(); b.y = vp.y.into(); b.width = vp.width.into(); b.height = vp.height.into();
            }
            let mut i = 0;
            while let Some(e) = ev.next_ipairs(&mut i) {
                if let EventKind::KeyDown { .. } = e.kind {
                    if e.key == Key::Char(b'z') && cx.sys.is_key_down(Key::Ctrl) {
                        let edit = cx.child_as::<EditControl>(me, "edit"); EditControl::undo(cx, edit.id());
                    } else if e.key == Key::Char(b'y') && cx.sys.is_key_down(Key::Ctrl) {
                        let edit = cx.child_as::<EditControl>(me, "edit"); EditControl::redo(cx, edit.id());
                    }
                }
            }
            cx.process_controls_input(me, ev, vp);
            draw_background(cx, vp);
            cx.draw_controls(me, vp, None);
            let edit = cx.child_as::<EditControl>(me, "edit");
            let mod_flag = cx.w::<NotesTab>(me).last_content != cx.get(edit).buf;
            cx.w_mut::<NotesTab>(me).mod_flag = mod_flag;
        }
    }
}

/// The shapes are real: the vtables build and the widget metadata is what §9 declares.
#[test]
fn reference_ports_have_their_shape() {
    use pob_ui_core::prelude::*;
    let vt = vtable_of::<button_9_1::ButtonControl>();
    assert_eq!(vt.name, "ButtonControl");
    assert!(vt
        .caps
        .contains(Caps::DRAW | Caps::IS_MOUSE_OVER | Caps::KEY_DOWN | Caps::KEY_UP | Caps::CLICK));
    assert_eq!(vtable_of::<notes_9_2::NotesTab>().caps, Caps::empty());
    // The stub and the reference agree on the class metadata.
    assert_eq!(
        <pob_ui_core::controls::ButtonControl as Widget>::CAPS,
        <button_9_1::ButtonControl as Widget>::CAPS
    );
}
