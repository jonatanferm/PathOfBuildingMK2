//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/PathControl.lua 1-110
//! (`newClass("PathControl", "Control", "ControlHost", "UndoHandler")`). Owner: U12.
//!
//! @pob-file Classes/PathControl.lua @16de4b82d
//!
//! Owner's amendments to the UI-U0 stub (ui doc §13 item 12, CCR-0078 C):
//! - `sub_path` is `Option<String>`: it is nil until the constructor's first `SetSubPath`, whose
//!   `subPath == self.subPath` test must fail even for `""`.
//! - `new` takes `sub_path: Option<String>` (`subPath or ""`); `self.onChange` is assigned AFTER
//!   the first `SetSubPath` (L20), so the constructor's call never runs it.
//! - `folder_list` (`self.folderList`), `base_name`, and the drag-target fields
//!   `other_drag_source`/`other_drag_targeting` (written by ListControl, read by Draw L86;
//!   `BuildListControl.lua:31` writes `false`, which is `None`).
//! - The UndoHandler methods are [`UNDO_OPS`] (`Undo`/`Redo` via [`PathControl::undo`]/[`PathControl::redo`]).
//!
//! The buttons are `self.controls["folder"..i]`, created on demand and hidden (never removed) when
//! the path gets shorter, as in the Lua.

use std::rc::Rc;

use pob_sg::Key;

use crate::controls::button::ButtonControl;
use crate::draw::{draw_arrow, ArrowDir};
use crate::prelude::*;
use crate::undo::{UndoOps, UndoStack};

// @pob-skip Classes/PathControl.lua:1-11 reason="header comment, locals and newClass (the Widget impl below)"

/// One `self.folderList[i]` entry (`{ label, path, button }`).
#[derive(Clone)]
pub struct Folder {
    pub label: String,
    pub path: String,
    pub button: Option<Ctrl<ButtonControl>>,
}

pub struct PathControl {
    pub base_path: String,
    /// `self.baseName`
    pub base_name: String,
    /// `self.subPath` (nil before the constructor's SetSubPath)
    pub sub_path: Option<String>,
    /// `self.folderList`
    pub folder_list: Vec<Folder>,
    pub on_change: Option<Rc<dyn Fn(&mut Cx, &str)>>,
    pub undo: UndoStack<String>,
    /// `self.otherDragSource` (set by a dragging ListControl)
    pub other_drag_source: Option<CtrlId>,
    /// `self.otherDragTargeting`
    pub other_drag_targeting: bool,
}

fn undo_stack(cx: &mut Cx, me: CtrlId) -> &mut UndoStack<String> {
    &mut cx.w_mut::<PathControl>(me).undo
}

/// PathControl's UndoHandler "class" (CreateUndoState / RestoreUndoState, L103-L109).
pub static UNDO_OPS: UndoOps<String> = UndoOps {
    stack: undo_stack,
    create: PathControl::create_undo_state,
    restore: PathControl::restore_undo_state,
};

impl PathControl {
    /// lua: PathControl.lua:12-22 PathControl(anchor, rect, basePath, subPath, onChange)
    /// @pob Classes/PathControl.lua:12-22 h=00541a7b
    pub fn new(
        anchor: Anchor,
        rect: [Prop<f64>; 4],
        base_path: String,
        sub_path: Option<String>,
        on_change: Option<Rc<dyn Fn(&mut Cx, &str)>>,
    ) -> New<PathControl> {
        // L13-L14 (Control, ControlHost)
        let base = Base::new(anchor, rect);
        // L17
        let base_name = pob_lua::pattern::r#match(&base_path, "([^/]+)/$", None)
            .and_then(|c| c.first().map(|c| c.to_str().into_owned()))
            .unwrap_or_else(|| "Base".to_owned());
        New::new(
            base,
            PathControl {
                // L16
                base_path,
                base_name,
                sub_path: None,
                folder_list: Vec::new(),
                on_change: None,
                // L15 (UndoHandler)
                undo: UndoStack::default(),
                other_drag_source: None,
                other_drag_targeting: false,
            },
        )
        .host()
        .init(move |cx, me| {
            // L18
            PathControl::set_sub_path(cx, me.0, &sub_path.unwrap_or_default(), false);
            // L19
            UNDO_OPS.reset_undo(cx, me.0);
            // L20
            cx.get_mut(me).on_change = on_change;
        })
    }

    /// lua: PathControl.lua:24-64 SetSubPath
    /// @pob Classes/PathControl.lua:24-64 h=015b8ff8
    pub fn set_sub_path(cx: &mut Cx, me: CtrlId, sub_path: &str, no_undo: bool) {
        // L25
        if cx.w::<Self>(me).sub_path.as_deref() == Some(sub_path) {
            // L26
            return;
        }
        // L28
        cx.w_mut::<Self>(me).sub_path = Some(sub_path.to_owned());
        // L29
        let mut folder_list = vec![Folder {
            label: cx.w::<Self>(me).base_name.clone(),
            path: String::new(),
            button: None,
        }];
        // L32
        for caps in pob_lua::pattern::gmatch(sub_path, "([^/]+)()") {
            let folder = caps[0].to_str().into_owned();
            let end_index = caps[1].num().expect("position capture");
            // L33
            folder_list.push(Folder {
                label: folder,
                path: crate::luastr::sub(sub_path, 1.0, Some(end_index)).into_owned(),
                button: None,
            });
        }
        cx.w_mut::<Self>(me).folder_list = folder_list;
        // L35
        let mut x = 2.0;
        // L36
        let mut i = 1;
        // L37
        let mut index = 1;
        while let Some(folder) = cx.w::<Self>(me).folder_list.get(index - 1).cloned() {
            let key = format!("folder{i}");
            // L38
            let mut button = cx.child(me, key.clone());
            // L39
            if button.is_none() {
                // L40
                let height = cx.base(me).height.raw();
                let b = cx.add(
                    me,
                    key,
                    ButtonControl::new(
                        Anchor::new(Left, me, Left),
                        rect(0, 0, 0, height - 4.0),
                        "",
                        |_| {},
                    ),
                );
                // L41 (the store is the last step of the assignment in `cx.add`)
                button = Some(b.id());
            }
            let button = Ctrl::<ButtonControl>::from_raw(button.expect("set above"));
            {
                let b = cx.base_mut(button);
                // L43
                b.shown = Prop::Val(true);
                // L44
                b.x = Prop::Val(x);
            }
            // L45
            cx.get_mut(button).label = Prop::Val(folder.label.clone());
            // L46
            let height = cx.base(me).height.raw();
            let width = cx
                .sg
                .draw_string_width(height - 8.0, pob_sg::Font::Var, &folder.label)
                + 10.0;
            cx.base_mut(button).width = Prop::Val(width);
            // L47
            let path = folder.path.clone();
            cx.get_mut(button).on_click = Rc::new(move |cx| {
                // L48
                PathControl::set_sub_path(cx, me, &path, false);
                None
            });
            // L50
            cx.w_mut::<Self>(me).folder_list[index - 1].button = Some(button);
            // L51
            x = x + cx.base(button).width.raw() + 12.0;
            // L52
            i += 1;
            index += 1;
        }
        // L54
        while let Some(b) = cx.child(me, format!("folder{i}")) {
            // L55
            cx.base_mut(b).shown = Prop::Val(false);
            // L56
            i += 1;
        }
        // L58
        if let Some(f) = cx.w::<Self>(me).on_change.clone() {
            // L59
            f(cx, sub_path);
        }
        // L61
        if !no_undo {
            // L62
            UNDO_OPS.add_undo_state(cx, me, false);
        }
    }

    /// lua: PathControl.lua:103-105 CreateUndoState
    /// @pob Classes/PathControl.lua:103-105 h=4d77cae1
    pub fn create_undo_state(cx: &mut Cx, me: CtrlId) -> String {
        // L104 (`self.subPath` is a string once the constructor ran SetSubPath)
        cx.w::<Self>(me).sub_path.clone().unwrap_or_default()
    }

    /// lua: PathControl.lua:107-109 RestoreUndoState
    /// @pob Classes/PathControl.lua:107-109 h=e2ace391
    pub fn restore_undo_state(cx: &mut Cx, me: CtrlId, state: String) {
        // L108
        Self::set_sub_path(cx, me, &state, true);
    }

    /// UndoHandler:Undo()
    pub fn undo(cx: &mut Cx, me: CtrlId) {
        UNDO_OPS.undo(cx, me);
    }

    /// UndoHandler:Redo()
    pub fn redo(cx: &mut Cx, me: CtrlId) {
        UNDO_OPS.redo(cx, me);
    }
}

impl Widget for PathControl {
    const NAME: &'static str = "PathControl";
    const CAPS: Caps = Caps::DRAW.union(Caps::IS_MOUSE_OVER).union(Caps::KEY_DOWN);

    /// lua: PathControl.lua:66-71
    /// @pob Classes/PathControl.lua:66-71 h=30401b48
    fn is_mouse_over(cx: &Cx, me: CtrlId) -> bool {
        // L67
        if !cx.is_shown(me) {
            // L68
            return false;
        }
        // L70
        cx.is_mouse_in_bounds(me) || cx.get_mouse_over_control(me).is_some()
    }

    /// lua: PathControl.lua:73-91
    /// @pob Classes/PathControl.lua:73-91 h=3dc5ca87
    fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, _no_tooltip: bool) {
        // L74
        let (x, y) = cx.pos(me);
        // L75
        let (width, height) = cx.size(me);
        // L76
        cx.sg.set_draw_color(0.5, 0.5, 0.5);
        // L77
        cx.sg.draw_image(None, x, y, width, height);
        // L78
        cx.sg.set_draw_color(0.0, 0.0, 0.0);
        // L79
        cx.sg
            .draw_image(None, x + 1.0, y + 1.0, width - 2.0, height - 2.0);
        // L80
        cx.draw_controls(me, vp, None);
        // L81
        let mut index = 1;
        while let Some(folder) = cx.w::<Self>(me).folder_list.get(index - 1).cloned() {
            let button = folder.button.unwrap_or_else(|| {
                crate::lua_error!("attempt to index field 'button' (a nil value)")
            });
            // L82
            let (button_x, button_y) = cx.pos(button);
            // L83
            let (button_w, button_h) = cx.size(button);
            // L84
            cx.sg.set_draw_color(1.0, 1.0, 1.0);
            // L85
            draw_arrow(
                cx,
                button_x + button_w + 6.0,
                y + height / 2.0,
                8.0,
                8.0,
                ArrowDir::Right,
            );
            // L86
            let n = cx.w::<Self>(me).folder_list.len();
            if cx.w::<Self>(me).other_drag_source.is_some() && index < n {
                // L87
                cx.sg.set_draw_color_a(0.0, 1.0, 0.0, 0.25);
                // L88
                cx.sg
                    .draw_image(None, button_x, button_y, button_w, button_h);
            }
            index += 1;
        }
    }

    /// lua: PathControl.lua:93-101
    /// @pob Classes/PathControl.lua:93-101 h=8e9f0fd5
    fn on_key_down(cx: &mut Cx, me: CtrlId, key: Key, _double_click: bool) -> Focus {
        // L94
        if !cx.is_shown(me) || !cx.is_enabled(me) {
            // L95
            return None;
        }
        // L97
        let m_over_control = cx.get_mouse_over_control(me);
        // L98
        if let Some(m) = m_over_control {
            if cx.has(m, Caps::KEY_DOWN) {
                // L99 (OnKeyDown(key): doubleClick nil)
                return cx.on_key_down(m, key, false);
            }
        }
        None
    }
}
