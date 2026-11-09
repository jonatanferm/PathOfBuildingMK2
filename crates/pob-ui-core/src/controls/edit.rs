//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/EditControl.lua 1-761
//! (`newClass("EditControl", "ControlHost", "Control", "UndoHandler", "TooltipHost")`). Owner: U11.
//!
//! @pob-file Classes/EditControl.lua @16de4b82d
//!
//! # Surface (ui doc §6.8, plus the owner's amendments, CCR-0081)
//!
//! `new`/`state`/`init`, `set_text`, `insert`, `replace_sel`, `get_sel_text`, `select_all`,
//! `undo`, `redo`, `set_placeholder`, `set_protected`, `buf`, `caret`/`sel` (1-based byte
//! positions, as in Lua) and `change_func` are the frozen §6.8 surface. Additions:
//!
//! - `EditUndoState.caret` is an `Rc<Cell<usize>>` and the control keeps `last_undo_state`, so
//!   `self.lastUndoState.caret = self.caret` (L241, :324, :490, :507, :560, :574, :591, :601)
//!   writes through to the very state table Lua aliases (the top of the undo stack, or the state
//!   just restored), with Lua's table identity.
//! - `changeFunc(buf, placeholder)`: `SetPlaceholder(text, true)` calls `changeFunc(text, true)`
//!   (L114) and ConfigTab.lua:305/323/334 read that second argument. The frozen
//!   `change_func: Fn(&mut Cx, &str)` cannot carry it, so the `placeholder == true` call goes to
//!   [`EditControl::placeholder_change_func`] when it is set (else to `change_func`, which is what
//!   a one-argument Lua function sees). CCR-0081 item 1.
//! - The fields Lua reads that callers assign after construction: `enterFunc` (CompareTab.lua:1474,
//!   ImportTab.lua:672, PartyTab.lua:291, Export/Main.lua:333), `pasteFilter` (ImportTab.lua:342,
//!   ItemsTab.lua:2728), `inactiveText` (a string or a function of `buf`: ConfigTab.lua:86, :349),
//!   `borderFunc` (ConfigTab.lua:697, SkillsTab.lua:879), `numberInc` (Export/Main.lua:301, :312),
//!   `inactiveCol` (SkillsTab.lua:338, :1097, :1290), `placeholder`, `protected`, `font` and the
//!   other colours, `tooltipText`/`tooltipFunc` (in `tt`), `drag`, `isNumeric`, `blinkStart`.
//! - The `filter` argument nil means `main.unicode and "%c" or "^%w%p "` (L50). `state` has no
//!   `Cx`, so the default is resolved in `init` (which runs right after insertion, before any
//!   caller code can read the field); `filter`/`filter_pattern` are empty until then.
//!
//! # Hook set (ui doc §6.8 rule: every subclass and instance override, grepped at 16de4b82d)
//!
//! Subclasses: `GemSelectControl` (overrides IsMouseOver, Draw, OnFocusGained, OnFocusLost,
//! OnKeyDown, OnKeyUp and super-calls `self.EditControl:Draw/OnFocusGained/OnKeyDown/OnKeyUp`) and
//! `ResizableEditControl` (overrides Draw). Instance overrides of an edit's methods:
//! `TradeQuery.lua:404` (`fetchCountEdit:OnFocusLost`), a framework method (`Overrides`).
//! No subclass or instance overrides SetText, Insert, ReplaceSel, SelectAll, ScrollCaretIntoView,
//! UpdateScrollBars, ZoomText, MoveCaretVertically, CreateUndoState or RestoreUndoState, and every
//! EditControl method that calls them is reached either directly or through the class proxy
//! (`self.EditControl:Draw(..)`, whose `self:X()` resolves to EditControl's own `X`,
//! Common.lua:180-188). So EditControl needs no class hook table: its internal calls are static.
//! The virtual calls are exactly the Lua's: `self.Object:IsMouseOver()` (L457, :533) and
//! `self.Object:TabAdvance(..)` (L661) go through `cx`; `self:IsMouseOver()` in Draw (L250)
//! resolves to an instance override or EditControl's own method ([`EditControl::self_is_mouse_over`]),
//! never GemSelectControl's; the +/- buttons' `self:OnKeyUp(..)` (L72, :75) close over the object
//! and dispatch through `cx.on_key_up`.
//!
//! # Notes
//!
//! - DIVERGENCE (ui doc §8 rule 4): buffers are `String`. Pieces cut out of `buf` at Lua byte
//!   positions are drawn and measured through a lossy UTF-8 conversion; every edit of `buf` itself
//!   is byte-exact (`luastr::{splice, insert}`, [`LBuf`]). Input reaches `buf` only through the
//!   filters, `Paste` (bytes >= 128 become `?`, L539) and callers' `SetText`.
//! - L346 assigns the undeclared global `caretY` (`caretX, caretY = textX, textY`); here it is a
//!   local. No PoB code reads a global `caretY`.
//! - `GetTime()` values are kept as Lua numbers (`f64` of `Sys::time_ms`).
//! - Property closures that write fields (ui doc §7.3): none in this file.
//! - ORDER: no `pairs` loops; the children's draw/hit order is the creation order (D7).

use std::cell::Cell;
use std::rc::Rc;

use pob_lua::math::{m_max, m_min};
use pob_lua::pattern::match_bytes;
use pob_sg::{Align, Font, Key};

use crate::controls::button::ButtonControl;
use crate::controls::scroll_bar::{ScrollBarControl, ScrollDir};
use crate::luastr::{self, utf8, LBuf};
use crate::prelude::*;
use crate::tooltip::TooltipHost;
use crate::undo::{UndoOps, UndoStack};

// @pob-skip Classes/EditControl.lua:1-5 reason="file header comment"
// @pob-skip Classes/EditControl.lua:39-41 reason="class header (newClass); the struct below"
/// lua: `local protected_replace = "*"` (the other module locals are `math.*` and `lua-utf8`).
/// @pob Classes/EditControl.lua:6-10 h=ac70432d
const PROTECTED_REPLACE: &str = "*";

/// lua: EditControl.lua:12-23 lastLine (the bytes after the last "\n")
/// @pob Classes/EditControl.lua:12-23 h=32147fea
fn last_line(s: &[u8]) -> &[u8] {
    // L13-L21: the index after the last plain "\n" find
    let last_line_index = s.iter().rposition(|&b| b == b'\n').map_or(0, |p| p + 1);
    // L22
    &s[last_line_index..]
}

/// lua: EditControl.lua:25-37 newlineCount
/// @pob Classes/EditControl.lua:25-37 h=3e118bb8
#[allow(clippy::naive_bytecount)]
fn newline_count(s: &[u8]) -> f64 {
    s.iter().filter(|&&b| b == b'\n').count() as f64
}

/// `s:sub(i, j)` on bytes with Lua-number indices.
fn bsub(s: &str, i: f64, j: Option<f64>) -> &[u8] {
    luastr::sub_bytes(s.as_bytes(), i, j)
}

/// `s:match(p)` is not nil.
fn pmatch(s: &[u8], p: &[u8]) -> bool {
    match_bytes(s, p, None).is_some()
}

/// `#s:match(p)` (the whole match; a nil match is Lua's "attempt to get length of a nil value").
#[track_caller]
fn pmatch_len(s: &[u8], p: &[u8]) -> f64 {
    let caps = match_bytes(s, p, None)
        .unwrap_or_else(|| lua_error!("attempt to get length of a nil value"));
    caps.first().map_or(0.0, |c| c.as_bytes().len() as f64)
}

/// `#utf8.match(s, p)`
#[track_caller]
fn umatch_len(s: &[u8], p: &[u8]) -> f64 {
    utf8::match1(s, p)
        .unwrap_or_else(|| lua_error!("attempt to get length of a nil value"))
        .len() as f64
}

/// Bytes Lua holds as a string, as `&str` for the SG calls (ui doc §8 rule 4).
fn lossy(b: &[u8]) -> std::borrow::Cow<'_, str> {
    String::from_utf8_lossy(b)
}

/// `string.rep(protected_replace, n)` (`n <= 0` gives "").
fn rep_protected(n: f64) -> String {
    if n >= 1.0 {
        PROTECTED_REPLACE.repeat(luastr::lua_int(n) as usize)
    } else {
        String::new()
    }
}

/// A 1-based Lua byte position back into the field type (positions never go below 1 in
/// EditControl's arithmetic; 0 only from `utf8.next(..) or 0`, which L552 rules out).
fn pos(n: f64) -> usize {
    n as usize
}

/// EditControl(anchor, rect, init, prompt, filter, limit, changeFunc, lineHeight, allowZoom, clearable) (EditControl.lua:43)
#[derive(Default)]
pub struct EditArgs {
    pub init: Option<String>,
    pub prompt: Option<String>,
    /// nil => main.unicode and "%c" or "^%w%p "
    pub filter: Option<String>,
    pub limit: Option<usize>,
    pub change_func: Option<Rc<dyn Fn(&mut Cx, &str)>>,
    /// The `placeholder == true` call of a two-argument changeFunc (L114). CCR-0081 item 1.
    pub placeholder_change_func: Option<Rc<dyn Fn(&mut Cx, &str)>>,
    pub line_height: Option<f64>,
    pub allow_zoom: bool,
    pub clearable: bool,
}

/// CreateUndoState (EditControl.lua:743-750): `{ buf, caret }`. `caret` is shared with
/// `EditControl::last_undo_state` (Lua table identity, see the module notes).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EditUndoState {
    pub buf: String,
    pub caret: Rc<Cell<usize>>,
}

/// `self.inactiveText`: a string, or a function of `buf` (L305).
#[derive(Clone)]
pub enum InactiveText {
    Str(String),
    Fn(Rc<dyn Fn(&mut Cx, &str) -> String>),
}

pub struct EditControl {
    pub tt: TooltipHost<()>,
    pub undo: UndoStack<EditUndoState>,
    /// `self.lastUndoState`
    pub last_undo_state: Option<EditUndoState>,
    pub buf: String,
    /// 1-based byte position, as in Lua
    pub caret: usize,
    /// 1-based byte position, as in Lua
    pub sel: Option<usize>,
    pub prompt: Option<String>,
    /// `self.filter` (resolved by `init`)
    pub filter: String,
    /// `self.filterPattern` = `"[" .. filter .. "]"`
    pub filter_pattern: String,
    /// the constructor's `filter` argument, consumed by `init`
    filter_arg: Option<String>,
    pub limit: Option<usize>,
    pub change_func: Option<Rc<dyn Fn(&mut Cx, &str)>>,
    /// CCR-0081 item 1 (see the module notes)
    pub placeholder_change_func: Option<Rc<dyn Fn(&mut Cx, &str)>>,
    pub line_height: Option<f64>,
    pub default_line_height: Option<f64>,
    /// "VAR", or "FIXED" after SetProtected
    pub font: Font,
    pub text_col: String,
    pub inactive_col: String,
    pub disable_col: String,
    pub sel_col: String,
    pub sel_bg_col: String,
    pub blink_start: f64,
    pub allow_zoom: bool,
    pub clearable: bool,
    pub is_numeric: bool,
    pub protected: bool,
    pub placeholder: Option<String>,
    pub inactive_text: Option<InactiveText>,
    pub enter_func: Option<Rc<dyn Fn(&mut Cx, &str)>>,
    pub paste_filter: Option<Rc<dyn Fn(&mut Cx, &str) -> String>>,
    pub border_func: Option<Rc<dyn Fn(&Cx) -> (f64, f64, f64)>>,
    pub number_inc: Option<f64>,
    /// `self.drag` (nil until the first click)
    pub drag: bool,
}

fn undo_stack(cx: &mut Cx, me: CtrlId) -> &mut UndoStack<EditUndoState> {
    &mut cx.w_mut::<EditControl>(me).undo
}

/// EditControl's UndoHandler "class" (ui doc §6.12).
pub const UNDO: UndoOps<EditUndoState> = UndoOps {
    stack: undo_stack,
    create: EditControl::create_undo_state,
    restore: EditControl::restore_undo_state,
};

impl EditControl {
    /// lua: EditControl.lua:43-99
    pub fn new(anchor: Anchor, rect: [Prop<f64>; 4], args: EditArgs) -> New<EditControl> {
        New::new(Base::new(anchor, rect), EditControl::state(args))
            .host()
            .init(|cx, me| EditControl::init(cx, me.id()))
    }

    /// The constructor's field part (subclasses embed it: GemSelectControl, ResizableEditControl):
    /// EditControl.lua:44-63 except what needs `cx` (`main.unicode` for the default filter,
    /// `GetTime()`), which [`EditControl::init`] does.
    /// @pob Classes/EditControl.lua:43-99 h=fa4836d7
    pub fn state(args: EditArgs) -> EditControl {
        // L44 self:ControlHost() (New::host), L45 self:Control(anchor, rect) (Base::new)
        let mut w = EditControl {
            // L47 self:TooltipHost()
            tt: TooltipHost::new(None),
            // L46 self:UndoHandler()
            undo: UndoStack::default(),
            last_undo_state: None,
            buf: String::new(),
            caret: 1,
            sel: None,
            prompt: None,
            filter: String::new(),
            filter_pattern: String::new(),
            filter_arg: None,
            limit: None,
            change_func: None,
            placeholder_change_func: None,
            line_height: None,
            default_line_height: None,
            font: Font::Var,
            text_col: String::new(),
            inactive_col: String::new(),
            disable_col: String::new(),
            sel_col: String::new(),
            sel_bg_col: String::new(),
            blink_start: 0.0,
            allow_zoom: false,
            clearable: false,
            is_numeric: false,
            protected: false,
            placeholder: None,
            inactive_text: None,
            enter_func: None,
            paste_filter: None,
            border_func: None,
            number_inc: None,
            drag: false,
        };
        // L48 self:SetText(init or "") (no notify; ResetUndo creates the first state)
        w.buf = luastr::tostring(&args.init.unwrap_or_default());
        w.caret = w.buf.len() + 1;
        w.sel = None;
        w.undo.undo.clear();
        w.undo.redo.clear();
        let s = Self::new_undo_state(&mut w);
        w.undo.undo.push_front(s);
        // L49
        w.prompt = args.prompt;
        // L50-L51: resolved in `init`
        w.filter_arg = args.filter;
        // L52
        w.limit = args.limit;
        // L53
        w.change_func = args.change_func;
        w.placeholder_change_func = args.placeholder_change_func;
        // L54
        w.line_height = args.line_height;
        // L55
        w.default_line_height = args.line_height;
        // L56
        w.font = Font::Var;
        // L57
        w.text_col = "^7".to_owned();
        // L58
        w.inactive_col = "^8".to_owned();
        // L59
        w.disable_col = "^9".to_owned();
        // L60
        w.sel_col = "^0".to_owned();
        // L61
        w.sel_bg_col = "^xBBBBBB".to_owned();
        // L62: in `init`
        // L63
        w.allow_zoom = args.allow_zoom;
        w.clearable = args.clearable;
        w
    }

    /// The constructor's tail that needs the node (scrollbars and buttons anchored to `me`),
    /// plus L50-L51 (the default filter reads `main.unicode`) and L62 (`GetTime()`).
    pub fn init(cx: &mut Cx, me: CtrlId) {
        // L50
        let unicode = cx.main.unicode;
        let filter = cx
            .w_mut::<Self>(me)
            .filter_arg
            .take()
            .unwrap_or_else(|| if unicode { "%c" } else { "^%w%p " }.to_owned());
        // L51
        cx.w_mut::<Self>(me).filter_pattern = format!("[{filter}]");
        cx.w_mut::<Self>(me).filter = filter.clone();
        // L62
        let now = cx.sys.time_ms() as f64;
        cx.w_mut::<Self>(me).blink_start = now;
        // L64-L67
        let button_size = move |cx: &Cx, _: CtrlId| {
            let (_, height) = cx.size(me);
            height - 4.0
        };
        let bs = || Prop::f(button_size);
        let clearable = cx.w::<Self>(me).clearable;
        // L68
        if filter == "%D" || filter == "^%-%d" {
            // L70 Add +/- buttons for integer number edits
            cx.w_mut::<Self>(me).is_numeric = true;
            // L71-L73
            let button_down = cx.add(
                me,
                "buttonDown",
                ButtonControl::new(
                    Anchor::new(Right, me, Right),
                    [Prop::Val(-2.0), Prop::Val(0.0), bs(), bs()],
                    "-",
                    move |cx| {
                        cx.on_key_up(me, Key::Down);
                    },
                ),
            );
            // L74-L76
            cx.add(
                me,
                "buttonUp",
                ButtonControl::new(
                    Anchor::new(Right, button_down, Left),
                    [Prop::Val(-1.0), Prop::Val(0.0), bs(), bs()],
                    "+",
                    move |cx| {
                        cx.on_key_up(me, Key::Up);
                    },
                ),
            );
        } else if clearable {
            // L78-L80
            let button_clear = cx.add(
                me,
                "buttonClear",
                ButtonControl::new(
                    Anchor::new(Right, me, Right),
                    [Prop::Val(-2.0), Prop::Val(0.0), bs(), bs()],
                    "x",
                    move |cx| EditControl::set_text(cx, me, "", true),
                ),
            );
            // L81
            cx.base_mut(button_clear).shown = Prop::f(move |cx, _| {
                !cx.w::<EditControl>(me).buf.is_empty() && cx.is_mouse_in_bounds(me)
            });
        }
        // L83
        let line_height = cx.w::<Self>(me).line_height;
        let scroll_bar_h = cx.add(
            me,
            "scrollBarH",
            ScrollBarControl::new(
                Anchor::new(BottomLeft, me, BottomLeft),
                rect(1, -1, 0, 14),
                Some(60.0),
                Some(ScrollDir::Horizontal),
                true,
            ),
        );
        // L84-L87
        cx.base_mut(scroll_bar_h).width = Prop::f(move |cx, _| {
            let (width, _height) = cx.size(me);
            let v = cx.child_as::<ScrollBarControl>(me, "scrollBarV");
            width
                - if ScrollBarControl::enabled_raw(cx, v.id()) {
                    16.0
                } else {
                    2.0
                }
        });
        // L88
        let scroll_bar_v = cx.add(
            me,
            "scrollBarV",
            ScrollBarControl::new(
                Anchor::new(TopRight, me, TopRight),
                rect(-1, 1, 14, 0),
                Some(line_height.unwrap_or(0.0) * 3.0),
                Some(ScrollDir::Vertical),
                true,
            ),
        );
        // L89-L92
        cx.base_mut(scroll_bar_v).height = Prop::f(move |cx, _| {
            let (_width, height) = cx.size(me);
            let h = cx.child_as::<ScrollBarControl>(me, "scrollBarH");
            height
                - if ScrollBarControl::enabled_raw(cx, h.id()) {
                    16.0
                } else {
                    2.0
                }
        });
        // L93
        if line_height.is_none() {
            // L94
            cx.base_mut(scroll_bar_h).shown = Prop::Val(false);
            // L95
            cx.base_mut(scroll_bar_v).shown = Prop::Val(false);
        }
        // L97
        cx.w_mut::<Self>(me).protected = false;
    }

    fn sb_h(cx: &Cx, me: CtrlId) -> Ctrl<ScrollBarControl> {
        cx.child_as::<ScrollBarControl>(me, "scrollBarH")
    }

    fn sb_v(cx: &Cx, me: CtrlId) -> Ctrl<ScrollBarControl> {
        cx.child_as::<ScrollBarControl>(me, "scrollBarV")
    }

    /// `self.changeFunc(self.buf)` when set (the Rc is cloned out first, ui doc §7.2).
    fn notify_change(cx: &mut Cx, me: CtrlId) {
        let w = cx.w::<Self>(me);
        if let Some(f) = w.change_func.clone() {
            let buf = w.buf.clone();
            f(cx, &buf);
        }
    }

    /// `self.lastUndoState.caret = self.caret`
    #[track_caller]
    fn set_last_undo_caret(cx: &mut Cx, me: CtrlId) {
        let w = cx.w::<Self>(me);
        let last = w
            .last_undo_state
            .as_ref()
            .unwrap_or_else(|| lua_error!("attempt to index field 'lastUndoState' (a nil value)"));
        last.caret.set(w.caret);
    }

    /// `self.blinkStart = GetTime()`
    fn reset_blink(cx: &mut Cx, me: CtrlId) {
        let now = cx.sys.time_ms() as f64;
        cx.w_mut::<Self>(me).blink_start = now;
    }

    /// `(GetTime() - self.blinkStart) % 1000 < 500`
    fn blink_on(cx: &Cx, me: CtrlId) -> bool {
        let now = cx.sys.time_ms() as f64;
        pob_lua::fmod_lua(now - cx.w::<Self>(me).blink_start, 1000.0) < 500.0
    }

    /// `self.sel = shift and (self.sel or self.caret) or nil`
    fn shift_sel(cx: &mut Cx, me: CtrlId, shift: bool) {
        let w = cx.w_mut::<Self>(me);
        w.sel = if shift {
            Some(w.sel.unwrap_or(w.caret))
        } else {
            None
        };
    }

    /// `m_min(self.caret, self.sel)`, `m_max(self.caret, self.sel)` (a nil `sel` is a Lua error).
    #[track_caller]
    fn sel_range(w: &EditControl) -> (f64, f64) {
        let sel = w
            .sel
            .unwrap_or_else(|| lua_error!("bad argument #2 to 'min' (number expected, got nil)"));
        let (c, s) = (w.caret as f64, sel as f64);
        (m_min(c, s), m_max(c, s))
    }

    /// lua: EditControl.lua:101-109 SetText(text, notify)
    /// @pob Classes/EditControl.lua:101-109 h=98e45446
    pub fn set_text(cx: &mut Cx, me: CtrlId, text: &str, notify: bool) {
        let w = cx.w_mut::<Self>(me);
        // L102
        w.buf = luastr::tostring(text);
        // L103
        w.caret = w.buf.len() + 1;
        // L104
        w.sel = None;
        // L105
        if notify {
            // L106
            Self::notify_change(cx, me);
        }
        // L108
        UNDO.reset_undo(cx, me);
    }

    /// lua: EditControl.lua:111-116 SetPlaceholder(text, notify)
    /// @pob Classes/EditControl.lua:111-116 h=7fc37851
    pub fn set_placeholder(cx: &mut Cx, me: CtrlId, text: &str, notify: bool) {
        // L112
        let placeholder = luastr::tostring(text);
        cx.w_mut::<Self>(me).placeholder = Some(placeholder.clone());
        // L113
        let w = cx.w::<Self>(me);
        if notify && w.change_func.is_some() {
            // L114 self.changeFunc(self.placeholder, true)  (CCR-0081 item 1)
            let f = w
                .placeholder_change_func
                .clone()
                .or_else(|| w.change_func.clone())
                .expect("change_func checked above");
            f(cx, &placeholder);
        }
    }

    /// lua: EditControl.lua:118-123 SetProtected(bool)
    /// @pob Classes/EditControl.lua:118-123 h=2d8643b7
    pub fn set_protected(cx: &mut Cx, me: CtrlId, protected: bool) {
        let w = cx.w_mut::<Self>(me);
        // L119 `self.protected = bool or true`: true whatever the argument
        let _ = protected;
        w.protected = true;
        // L122 set the font to be fixed to prevent strange spacing
        w.font = Font::Fixed;
    }

    /// `self:IsMouseOver()` from inside EditControl's own methods (Draw, L250): an instance
    /// override, else EditControl's method, also when the node is a subclass reached through the
    /// class proxy (`self.EditControl:Draw(..)`, Common.lua:180-188).
    pub fn self_is_mouse_over(cx: &Cx, me: CtrlId) -> bool {
        if let Some(f) = cx.ui.node(me).ov.is_mouse_over.clone() {
            return f(cx, me);
        }
        <Self as Widget>::is_mouse_over(cx, me)
    }

    /// lua: EditControl.lua:132-136
    /// @pob Classes/EditControl.lua:132-136 h=72ed91c0
    pub fn select_all(cx: &mut Cx, me: CtrlId) {
        let w = cx.w_mut::<Self>(me);
        // L133
        w.caret = w.buf.len() + 1;
        // L134
        w.sel = Some(1);
        // L135
        Self::scroll_caret_into_view(cx, me);
    }

    /// lua: EditControl.lua:138-143
    /// @pob Classes/EditControl.lua:138-143 h=181256ac
    pub fn get_sel_text(cx: &Cx, me: CtrlId) -> String {
        let w = cx.w::<Self>(me);
        // L139-L140
        let (left, right) = Self::sel_range(w);
        // L141
        let mut b = LBuf::new();
        b.push_sub(&w.buf, left, Some(right - 1.0));
        // L142
        b.into_string()
    }

    /// lua: EditControl.lua:145-165
    /// @pob Classes/EditControl.lua:145-165 h=fddb3330
    pub fn replace_sel(cx: &mut Cx, me: CtrlId, text: &str) {
        // L146
        let text = luastr::gsub(text, "\r", "");
        // L147
        if pob_lua::lmatch(&text, &cx.w::<Self>(me).filter_pattern, None).is_some() {
            // L148
            return;
        }
        let w = cx.w::<Self>(me);
        // L150-L151
        let (left, right) = Self::sel_range(w);
        // L152
        let new_buf = luastr::splice(&w.buf, left, right, &text);
        // L153
        if let Some(limit) = w.limit {
            if new_buf.len() > limit {
                // L154
                return;
            }
        }
        let w = cx.w_mut::<Self>(me);
        // L156
        w.buf = new_buf;
        // L157
        w.caret = pos(left + text.len() as f64);
        // L158
        w.sel = None;
        // L159
        Self::scroll_caret_into_view(cx, me);
        // L160
        Self::reset_blink(cx, me);
        // L161-L163
        Self::notify_change(cx, me);
        // L164
        UNDO.add_undo_state(cx, me, false);
    }

    /// lua: EditControl.lua:167-187
    /// @pob Classes/EditControl.lua:167-187 h=f8ad32b1
    pub fn insert(cx: &mut Cx, me: CtrlId, text: &str) {
        // L168
        let text = luastr::gsub(text, "\r", "");
        // L169 Remove any illegal chars from the "text" variable, to stop resulting in no text when an illegal character is found.
        // L170
        let text = luastr::gsub(&text, &cx.w::<Self>(me).filter_pattern, "");
        // L171
        if text.is_empty() {
            // L172
            return;
        }
        let w = cx.w::<Self>(me);
        // L174
        let new_buf = luastr::insert(&w.buf, w.caret as f64, &text);
        // L175
        if let Some(limit) = w.limit {
            if new_buf.len() > limit {
                // L176
                return;
            }
        }
        let w = cx.w_mut::<Self>(me);
        // L178
        w.buf = new_buf;
        // L179
        w.caret += text.len();
        // L180
        w.sel = None;
        // L181
        Self::scroll_caret_into_view(cx, me);
        // L182
        Self::reset_blink(cx, me);
        // L183-L185
        Self::notify_change(cx, me);
        // L186
        UNDO.add_undo_state(cx, me, false);
    }

    /// lua: EditControl.lua:189-210 ZoomText(zoom) (`zoom` is "+", "-" or "0")
    /// @pob Classes/EditControl.lua:189-210 h=0f4a88ff
    pub fn zoom_text(cx: &mut Cx, me: CtrlId, zoom: &str) {
        let w = cx.w_mut::<Self>(me);
        // L190
        if !w.allow_zoom || w.line_height.is_none() {
            // L191
            return;
        }
        // L194
        let mut text_height = w.line_height;
        // L195
        if zoom == "+" {
            // L196
            text_height = text_height.map(|h| h + 1.0);
        } else if zoom == "-" {
            // L198
            text_height = text_height.map(|h| h - 1.0);
        } else if zoom == "0" {
            // L200
            text_height = w.default_line_height;
        }
        // L203 (a nil defaultLineHeight cannot happen: lineHeight is non-nil, so it was given)
        let th = text_height.unwrap_or_else(|| lua_error!("attempt to compare nil with number"));
        let th = if th < 10.0 {
            // L204
            10.0
        } else if th > 100.0 {
            // L206
            100.0
        } else {
            th
        };
        // L209
        w.line_height = Some(th);
    }

    /// lua: EditControl.lua:212-221
    /// @pob Classes/EditControl.lua:212-221 h=bec0e0b1
    pub fn update_scroll_bars(cx: &mut Cx, me: CtrlId) {
        // L213
        let (width, height) = cx.size(me);
        let (line_height, font, buf, prompt) = {
            let w = cx.w::<Self>(me);
            (w.line_height, w.font, w.buf.clone(), w.prompt.clone())
        };
        // L214
        let text_height = line_height.unwrap_or(height - 4.0);
        let h = Self::sb_h(cx, me);
        // L215
        if line_height.is_some() {
            // L216
            let con = cx.sg.draw_string_width(text_height, font, &buf) + 2.0;
            ScrollBarControl::set_content_dimension(cx, h.id(), con, width - 18.0);
            // L217
            let v = Self::sb_v(cx, me);
            let mut b = LBuf::new();
            b.push_str(&buf).push_str("\n");
            let con = newline_count(&b.0) * text_height;
            let view = height
                - if ScrollBarControl::enabled_raw(cx, h.id()) {
                    18.0
                } else {
                    4.0
                };
            ScrollBarControl::set_content_dimension(cx, v.id(), con, view);
        } else {
            // L219
            let con = cx.sg.draw_string_width(text_height, font, &buf) + 2.0;
            let prompt_w = match &prompt {
                Some(p) => cx.sg.draw_string_width(text_height, font, p) + text_height / 2.0,
                None => 0.0,
            };
            ScrollBarControl::set_content_dimension(cx, h.id(), con, width - 4.0 - prompt_w);
        }
    }

    /// lua: EditControl.lua:223-234
    /// @pob Classes/EditControl.lua:223-234 h=7e4bd827
    pub fn scroll_caret_into_view(cx: &mut Cx, me: CtrlId) {
        // L224
        let (_width, height) = cx.size(me);
        let (line_height, font, pre) = {
            let w = cx.w::<Self>(me);
            // L226
            let pre = bsub(&w.buf, 1.0, Some(w.caret as f64 - 1.0)).to_vec();
            (w.line_height, w.font, pre)
        };
        // L225
        let text_height = line_height.unwrap_or(height - 4.0);
        // L227
        let caret_x = cx
            .sg
            .draw_string_width(text_height, font, &lossy(last_line(&pre)));
        // L228
        Self::update_scroll_bars(cx, me);
        // L229
        let h = Self::sb_h(cx, me);
        ScrollBarControl::scroll_into_view(cx, h.id(), caret_x - text_height, text_height * 2.0);
        // L230
        if line_height.is_some() {
            // L231
            let caret_y = newline_count(&pre) * text_height;
            // L232
            let v = Self::sb_v(cx, me);
            ScrollBarControl::scroll_into_view(cx, v.id(), caret_y, text_height);
        }
    }

    /// lua: EditControl.lua:236-244 (only called with a lineHeight)
    /// @pob Classes/EditControl.lua:236-244 h=b71c3c90
    pub fn move_caret_vertically(cx: &mut Cx, me: CtrlId, offset: f64) {
        let (lh, font, buf, pre) = {
            let w = cx.w::<Self>(me);
            // L237
            let pre = bsub(&w.buf, 1.0, Some(w.caret as f64 - 1.0)).to_vec();
            let lh = w.line_height.unwrap_or_else(|| {
                lua_error!("attempt to perform arithmetic on field 'lineHeight' (a nil value)")
            });
            (lh, w.font, w.buf.clone(), pre)
        };
        // L238
        let caret_x = cx.sg.draw_string_width(lh, font, &lossy(last_line(&pre)));
        // L239
        let caret_y = newline_count(&pre) * lh;
        // L240
        let caret = cx.sg.draw_string_cursor_index(
            lh,
            font,
            &buf,
            caret_x + 1.0,
            caret_y + lh / 2.0 + offset,
        );
        cx.w_mut::<Self>(me).caret = caret;
        // L241
        Self::set_last_undo_caret(cx, me);
        // L242
        Self::scroll_caret_into_view(cx, me);
        // L243
        Self::reset_blink(cx, me);
    }

    /// The new state table of CreateUndoState (L744-L748) on the struct.
    fn new_undo_state(w: &mut EditControl) -> EditUndoState {
        // L744-L747
        let state = EditUndoState {
            buf: w.buf.clone(),
            caret: Rc::new(Cell::new(w.caret)),
        };
        // L748
        w.last_undo_state = Some(state.clone());
        state
    }

    /// lua: EditControl.lua:743-750 CreateUndoState
    /// @pob Classes/EditControl.lua:743-750 h=a3af304b
    pub fn create_undo_state(cx: &mut Cx, me: CtrlId) -> EditUndoState {
        // L749
        Self::new_undo_state(cx.w_mut::<Self>(me))
    }

    /// lua: EditControl.lua:752-761 RestoreUndoState
    /// @pob Classes/EditControl.lua:752-761 h=d2105a0c
    pub fn restore_undo_state(cx: &mut Cx, me: CtrlId, state: EditUndoState) {
        let w = cx.w_mut::<Self>(me);
        // L753
        w.buf = state.buf.clone();
        // L754
        w.caret = state.caret.get();
        // L755
        w.sel = None;
        // L756
        Self::scroll_caret_into_view(cx, me);
        // L757-L759
        Self::notify_change(cx, me);
        // L760
        cx.w_mut::<Self>(me).last_undo_state = Some(state);
    }

    /// lua: UndoHandler.lua:44-50 (EditControl's undo state: EditControl.lua:743-761)
    pub fn undo(cx: &mut Cx, me: CtrlId) {
        UNDO.undo(cx, me);
    }

    /// lua: UndoHandler.lua:53-58 (EditControl's undo state: EditControl.lua:743-761)
    pub fn redo(cx: &mut Cx, me: CtrlId) {
        UNDO.redo(cx, me);
    }

    /// The draw of the multi-line focused text (L329-L378).
    #[allow(unused_assignments)] // L368 advances textX after the last piece, as the Lua does
    fn draw_lines(cx: &mut Cx, me: CtrlId, text_height: f64) {
        let h_off = cx.get(Self::sb_h(cx, me)).offset;
        let v_off = cx.get(Self::sb_v(cx, me)).offset;
        let (buf, caret, sel, font, text_col, sel_col, sel_bg_col) = {
            let w = cx.w::<Self>(me);
            (
                w.buf.clone(),
                w.caret as i64,
                w.sel.map(|s| s as i64),
                w.font,
                w.text_col.clone(),
                w.sel_col.clone(),
                w.sel_bg_col.clone(),
            )
        };
        // L327-L328
        let mut text_y = -v_off;
        // L330
        let left = caret.min(sel.unwrap_or(caret));
        // L331
        let right = caret.max(sel.unwrap_or(caret));
        // L332
        let mut caret_xy: Option<(f64, f64)> = None;
        // L333
        cx.sg.set_draw_color_esc(&text_col);
        // L334 for s, line, e in (self.buf.."\n"):gmatch("()([^\n]*)\n()") do
        let mut all = LBuf::new();
        all.push_str(&buf).push_str("\n");
        let all = all.0;
        let mut p0 = 0usize;
        while p0 < all.len() {
            let nl = p0
                + all[p0..]
                    .iter()
                    .position(|&b| b == b'\n')
                    .expect("the text ends with a newline");
            let s = p0 as i64 + 1;
            let line = &all[p0..nl];
            let e = nl as i64 + 2;
            p0 = nl + 1;
            let sub =
                |i: i64, j: Option<i64>| luastr::sub_bytes(line, i as f64, j.map(|j| j as f64));
            // L335
            let mut text_x = -h_off;
            // L336
            if left >= e || right <= s {
                // L337
                cx.sg
                    .draw_string(text_x, text_y, Align::Left, text_height, font, &lossy(line));
            }
            // L339
            if left < e {
                // L340
                if left > s {
                    // L341
                    let pre = lossy(sub(1, Some(left - s))).into_owned();
                    // L342
                    cx.sg
                        .draw_string(text_x, text_y, Align::Left, text_height, font, &pre);
                    // L343
                    text_x += cx.sg.draw_string_width(text_height, font, &pre);
                }
                // L345
                if left >= s && left == caret {
                    // L346
                    caret_xy = Some((text_x, text_y));
                }
            }
            // L349
            if left != right && left < e && right > s {
                // L350
                let piece = sub(
                    (left - s + 1).max(1),
                    Some((line.len() as i64).min(right - s)),
                );
                let mut sel_s = format!("{sel_col}{}", pob_sg::strip_escapes(&lossy(piece)));
                // L351
                if right >= e {
                    // L352
                    sel_s.push_str("  ");
                }
                // L354
                let sel_width = cx.sg.draw_string_width(text_height, font, &sel_s);
                // L355
                cx.sg.set_draw_color_esc(&sel_bg_col);
                // L356
                cx.sg
                    .draw_image(None, text_x, text_y, sel_width, text_height);
                // L357
                cx.sg
                    .draw_string(text_x, text_y, Align::Left, text_height, font, &sel_s);
                // L358
                cx.sg.set_draw_color_esc(&text_col);
                // L359
                text_x += sel_width;
            }
            // L361
            if right >= s && right < e && right == caret {
                // L362
                caret_xy = Some((text_x, text_y));
            }
            // L364
            if right > s {
                // L365
                if right < e {
                    // L366
                    let post = lossy(sub(right - s + 1, None)).into_owned();
                    // L367
                    cx.sg
                        .draw_string(text_x, text_y, Align::Left, text_height, font, &post);
                    // L368
                    text_x += cx.sg.draw_string_width(text_height, font, &post);
                }
            }
            // L371
            text_y += text_height;
        }
        // L373
        if let Some((caret_x, caret_y)) = caret_xy {
            // L374
            if Self::blink_on(cx, me) {
                // L375
                cx.sg.set_draw_color_esc(&text_col);
                // L376
                cx.sg.draw_image(None, caret_x, caret_y, 1.0, text_height);
            }
        }
    }

    /// The draw of the single-line focused text with a selection (L379-L408).
    fn draw_sel(cx: &mut Cx, me: CtrlId, mut text_x: f64, text_y: f64, text_height: f64) {
        let w = cx.w::<Self>(me);
        let (font, protected) = (w.font, w.protected);
        let (text_col, sel_col, sel_bg_col) =
            (w.text_col.clone(), w.sel_col.clone(), w.sel_bg_col.clone());
        // L380-L381
        let (left, right) = Self::sel_range(w);
        let (caret, sel) = (w.caret, w.sel.expect("sel checked by the caller"));
        // L382
        let pre = format!("{text_col}{}", lossy(bsub(&w.buf, 1.0, Some(left - 1.0))));
        // L383
        let sel_s = format!(
            "{sel_col}{}",
            pob_sg::strip_escapes(&lossy(bsub(&w.buf, left, Some(right - 1.0))))
        );
        // L384
        let post = format!("{text_col}{}", lossy(bsub(&w.buf, right, None)));
        // L385
        if protected {
            // L386
            let s = format!(
                "{text_col}{}",
                rep_protected(pre.len() as f64 - text_col.len() as f64)
            );
            cx.sg
                .draw_string(text_x, text_y, Align::Left, text_height, font, &s);
        } else {
            // L388
            cx.sg
                .draw_string(text_x, text_y, Align::Left, text_height, font, &pre);
        }
        // L390
        text_x += cx.sg.draw_string_width(text_height, font, &pre);
        // L391
        let sel_width = cx.sg.draw_string_width(text_height, font, &sel_s);
        // L392
        cx.sg.set_draw_color_esc(&sel_bg_col);
        // L393
        cx.sg
            .draw_image(None, text_x, text_y, sel_width, text_height);
        // L394
        if protected {
            // L395
            let s = format!(
                "{sel_col}{}",
                rep_protected(sel_s.len() as f64 - sel_col.len() as f64)
            );
            cx.sg
                .draw_string(text_x, text_y, Align::Left, text_height, font, &s);
        } else {
            // L397
            cx.sg
                .draw_string(text_x, text_y, Align::Left, text_height, font, &sel_s);
        }
        // L399
        if protected && !post.is_empty() {
            // L400
            let s = format!(
                "{text_col}{}",
                rep_protected(post.len() as f64 - text_col.len() as f64)
            );
            cx.sg.draw_string(
                text_x + sel_width,
                text_y,
                Align::Left,
                text_height,
                font,
                &s,
            );
        } else {
            // L402
            cx.sg.draw_string(
                text_x + sel_width,
                text_y,
                Align::Left,
                text_height,
                font,
                &post,
            );
        }
        // L404
        if Self::blink_on(cx, me) {
            // L405
            let caret_x = if caret > sel {
                text_x + sel_width
            } else {
                text_x
            };
            // L406
            cx.sg.set_draw_color_esc(&text_col);
            // L407
            cx.sg.draw_image(None, caret_x, text_y, 1.0, text_height);
        }
    }

    /// The draw of the single-line focused text without a selection (L409-L431).
    fn draw_plain(cx: &mut Cx, me: CtrlId, mut text_x: f64, text_y: f64, text_height: f64) {
        let w = cx.w::<Self>(me);
        let (font, protected) = (w.font, w.protected);
        let (text_col, disable_col) = (w.text_col.clone(), w.disable_col.clone());
        let placeholder = w.placeholder.clone();
        let buf_empty = w.buf.is_empty();
        // L414
        let pre = format!(
            "{text_col}{}",
            lossy(bsub(&w.buf, 1.0, Some(w.caret as f64 - 1.0)))
        );
        // L415
        let post = lossy(bsub(&w.buf, w.caret as f64, None)).into_owned();
        // L410
        if buf_empty {
            if let Some(placeholder) = placeholder {
                // L411
                cx.sg.set_draw_color_esc(&disable_col);
                // L412
                cx.sg
                    .draw_string(text_x, text_y, Align::Left, text_height, font, &placeholder);
            }
        }
        // L416
        if protected {
            // L417
            let s = format!(
                "{text_col}{}",
                rep_protected(pre.len() as f64 - text_col.len() as f64)
            );
            cx.sg
                .draw_string(text_x, text_y, Align::Left, text_height, font, &s);
        } else {
            // L419
            cx.sg
                .draw_string(text_x, text_y, Align::Left, text_height, font, &pre);
        }
        // L421
        text_x += cx.sg.draw_string_width(text_height, font, &pre);
        // L422
        if protected && !post.is_empty() {
            // L423
            let s = rep_protected(post.len() as f64);
            cx.sg
                .draw_string(text_x, text_y, Align::Left, text_height, font, &s);
        } else {
            // L425
            cx.sg
                .draw_string(text_x, text_y, Align::Left, text_height, font, &post);
        }
        // L427
        if Self::blink_on(cx, me) {
            // L428
            cx.sg.set_draw_color_esc(&text_col);
            // L429
            cx.sg.draw_image(None, text_x, text_y, 1.0, text_height);
        }
    }

    /// The double click of a multi-line edit (L461-L485): select the word (or the run of one
    /// non-space character) at the caret.
    fn select_word(cx: &mut Cx, me: CtrlId) {
        let w = cx.w_mut::<Self>(me);
        let buf = w.buf.clone();
        let mut caret = w.caret as f64;
        // L462
        if pmatch(bsub(&buf, caret - 1.0, Some(caret)), b"^%C\n$") {
            // L463
            caret -= 1.0;
        }
        // L465
        while pmatch(bsub(&buf, caret - 1.0, Some(caret)), b"[^\n][ \t]") {
            // L466
            caret -= 1.0;
        }
        // L468
        let caret_char = bsub(&buf, caret, Some(caret)).to_vec();
        let mut sel = w.sel.map(|s| s as f64);
        // L469
        if pmatch(&caret_char, b"%w") {
            // L470
            let mut s = caret;
            // L471
            while pmatch(bsub(&buf, s - 1.0, Some(s - 1.0)), b"%w") {
                // L472
                s -= 1.0;
            }
            sel = Some(s);
            // L474
            while pmatch(bsub(&buf, caret, Some(caret)), b"%w") {
                // L475
                caret += 1.0;
            }
        } else if pmatch(&caret_char, b"%S") {
            // L478
            let mut s = caret;
            // L479
            while bsub(&buf, s - 1.0, Some(s - 1.0)) == caret_char.as_slice() {
                // L480
                s -= 1.0;
            }
            sel = Some(s);
            // L482
            while bsub(&buf, caret, Some(caret)) == caret_char.as_slice() {
                // L483
                caret += 1.0;
            }
        }
        w.caret = pos(caret);
        w.sel = sel.map(pos);
    }
}

impl Widget for EditControl {
    const NAME: &'static str = "EditControl";
    const CAPS: Caps = Caps::DRAW
        .union(Caps::IS_MOUSE_OVER)
        .union(Caps::KEY_DOWN)
        .union(Caps::KEY_UP)
        .union(Caps::CHAR)
        .union(Caps::FOCUS_GAINED);

    /// lua: EditControl.lua:125-130
    /// @pob Classes/EditControl.lua:125-130 h=7ebe2091
    fn is_mouse_over(cx: &Cx, me: CtrlId) -> bool {
        // L126
        if !cx.is_shown(me) {
            // L127
            return false;
        }
        // L129
        cx.is_mouse_in_bounds(me) || cx.get_mouse_over_control(me).is_some()
    }

    /// lua: EditControl.lua:246-434
    /// @pob Classes/EditControl.lua:246-434 h=91a2278f
    #[allow(clippy::many_single_char_names)] // Lua's x, y, r, g, b
    fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, no_tooltip: bool) {
        // L247
        let (x, y) = cx.pos(me);
        // L248
        let (width, height) = cx.size(me);
        // L249
        let enabled = cx.is_enabled(me);
        // L250
        let m_over = Self::self_is_mouse_over(cx, me);
        // L251
        if !enabled {
            // L252
            cx.sg.set_draw_color(0.33, 0.33, 0.33);
        } else if m_over {
            // L254
            cx.sg.set_draw_color(1.0, 1.0, 1.0);
        } else if let Some(f) = cx.w::<Self>(me).border_func.clone() {
            // L256
            let (r, g, b) = f(cx);
            // L257
            cx.sg.set_draw_color(r, g, b);
        } else {
            // L259
            cx.sg.set_draw_color(0.5, 0.5, 0.5);
        }
        // L261
        cx.sg.draw_image(None, x, y, width, height);
        let line_height = cx.w::<Self>(me).line_height;
        // L262
        if !enabled {
            // L263
            cx.sg.set_draw_color(0.0, 0.0, 0.0);
        } else if cx.base(me).has_focus == Some(true) || m_over {
            // L265
            if line_height.is_some() {
                // L266
                cx.sg.set_draw_color(0.1, 0.1, 0.1);
            } else {
                // L268
                cx.sg.set_draw_color(0.15, 0.15, 0.15);
            }
        } else {
            // L271
            cx.sg.set_draw_color(0.0, 0.0, 0.0);
        }
        // L273
        cx.sg
            .draw_image(None, x + 1.0, y + 1.0, width - 2.0, height - 2.0);
        // L274
        let mut text_x = x + 2.0;
        // L275
        let mut text_y = y + 2.0;
        // L276
        let text_height = line_height.unwrap_or(height - 4.0);
        let (font, prompt) = {
            let w = cx.w::<Self>(me);
            (w.font, w.prompt.clone())
        };
        // L277
        if let Some(prompt) = prompt {
            let w = cx.ui.w::<Self>(me);
            // L278
            if !enabled {
                // L279
                let s = format!("{}{prompt}", w.disable_col);
                cx.sg
                    .draw_string(text_x, text_y, Align::Left, text_height, font, &s);
            } else {
                // L281
                let s = format!("{}{prompt}:", w.text_col);
                cx.sg
                    .draw_string(text_x, text_y, Align::Left, text_height, font, &s);
            }
            // L283
            text_x += cx.sg.draw_string_width(text_height, font, &prompt) + text_height / 2.0;
        }
        // L285
        if !enabled {
            // L286
            return;
        }
        // L288
        if m_over && !no_tooltip {
            // L289
            cx.sg.set_draw_layer(None, Some(100));
            // L290
            TooltipHost::draw_tooltip::<Self>(cx, me, |w| &mut w.tt, x, y, width, height, vp, ());
            // L291
            cx.sg.set_draw_layer(None, Some(0));
        }
        // L293
        Self::update_scroll_bars(cx, me);
        // L294
        let margin_l = text_x - x - 2.0;
        // L295
        let margin_r = if cx.is_shown(Self::sb_v(cx, me)) {
            14.0
        } else {
            0.0
        };
        // L296
        let margin_b = if cx.is_shown(Self::sb_h(cx, me)) {
            14.0
        } else {
            0.0
        };
        // L297
        cx.sg.set_viewport(
            text_x,
            text_y,
            width - 4.0 - margin_l - margin_r,
            height - 4.0 - margin_b,
        );
        let draw_controls_sel = if no_tooltip { Some(me) } else { None };
        // L298
        if cx.base(me).has_focus != Some(true) {
            let h_off = cx.get(Self::sb_h(cx, me)).offset;
            let v_off = cx.get(Self::sb_v(cx, me)).offset;
            let w = cx.w::<Self>(me);
            // L299
            if w.buf.is_empty() && w.placeholder.is_some() {
                let (disable_col, placeholder) = (
                    w.disable_col.clone(),
                    w.placeholder.clone().unwrap_or_default(),
                );
                // L300
                cx.sg.set_draw_color_esc(&disable_col);
                // L301
                cx.sg
                    .draw_string(-h_off, -v_off, Align::Left, text_height, font, &placeholder);
            } else {
                let inactive_col = w.inactive_col.clone();
                let inactive_text = w.inactive_text.clone();
                let protected = w.protected;
                let buf = w.buf.clone();
                // L303
                cx.sg.set_draw_color_esc(&inactive_col);
                // L304
                if let Some(it) = inactive_text {
                    // L305
                    let inactive_text = match it {
                        InactiveText::Str(s) => s,
                        InactiveText::Fn(f) => f(cx, &buf),
                    };
                    // L307
                    cx.sg.draw_string(
                        -h_off,
                        -v_off,
                        Align::Left,
                        text_height,
                        font,
                        &inactive_text,
                    );
                } else if protected {
                    // L309
                    let s = rep_protected(buf.len() as f64);
                    cx.sg
                        .draw_string(-h_off, -v_off, Align::Left, text_height, font, &s);
                } else {
                    // L311
                    cx.sg
                        .draw_string(-h_off, -v_off, Align::Left, text_height, font, &buf);
                }
            }
            // L314
            cx.sg.reset_viewport();
            // L315
            cx.draw_controls(me, vp, draw_controls_sel);
            // L316
            return;
        }
        // L318
        if !cx.sys.is_key_down(Key::LeftButton) {
            // L319
            cx.w_mut::<Self>(me).drag = false;
        }
        // L321
        if cx.w::<Self>(me).drag {
            // L322
            let (cursor_x, cursor_y) = cx.sys.cursor_pos();
            // L323
            let h_off = cx.get(Self::sb_h(cx, me)).offset;
            let v_off = cx.get(Self::sb_v(cx, me)).offset;
            let caret = cx.sg.draw_string_cursor_index(
                text_height,
                font,
                &cx.w::<Self>(me).buf,
                cursor_x - text_x + h_off,
                cursor_y - text_y + v_off,
            );
            cx.w_mut::<Self>(me).caret = caret;
            // L324
            Self::set_last_undo_caret(cx, me);
            // L325
            Self::scroll_caret_into_view(cx, me);
        }
        // L327
        text_x = -cx.get(Self::sb_h(cx, me)).offset;
        // L328
        text_y = -cx.get(Self::sb_v(cx, me)).offset;
        let (sel, caret) = {
            let w = cx.w::<Self>(me);
            (w.sel, w.caret)
        };
        // L329
        if line_height.is_some() {
            Self::draw_lines(cx, me, text_height);
        } else if sel.is_some() && sel != Some(caret) {
            // L379
            Self::draw_sel(cx, me, text_x, text_y, text_height);
        } else {
            // L409
            Self::draw_plain(cx, me, text_x, text_y, text_height);
        }
        // L432
        cx.sg.reset_viewport();
        // L433
        cx.draw_controls(me, vp, draw_controls_sel);
    }

    /// lua: EditControl.lua:436-441
    /// @pob Classes/EditControl.lua:436-441 h=e4f15ab0
    fn on_focus_gained(cx: &mut Cx, me: CtrlId) {
        // L437
        Self::reset_blink(cx, me);
        let w = cx.w::<Self>(me);
        // L438
        if !w.drag && cx.host(me).sel_control.is_none() && w.line_height.is_none() {
            // L439
            Self::select_all(cx, me);
        }
    }

    /// lua: EditControl.lua:443-666
    /// @pob Classes/EditControl.lua:443-666 h=c0dcf470
    fn on_key_down(cx: &mut Cx, me: CtrlId, key: Key, double_click: bool) -> Focus {
        // L444
        if !cx.is_shown(me) || !cx.is_enabled(me) {
            // L445
            return None;
        }
        // L447
        let m_over_control = cx.get_mouse_over_control(me);
        // L448
        if let Some(m) = m_over_control.filter(|&m| cx.has(m, Caps::KEY_DOWN)) {
            // L449
            cx.host_mut(me).sel_control = Some(m);
            // L450
            return cx.on_key_down(m, key, false).map(|_| me);
        }
        // L452
        cx.host_mut(me).sel_control = None;
        // L454
        let shift = cx.sys.is_key_down(Key::Shift);
        // L455
        let ctrl = cx.sys.is_key_down(Key::Ctrl);
        let line_height = cx.w::<Self>(me).line_height;
        let protected = cx.w::<Self>(me).protected;
        // L456
        if key == Key::LeftButton {
            // L457
            if !cx.is_mouse_over(me) {
                // L458
                return None;
            }
            // L460
            if double_click {
                // L461
                if line_height.is_some() {
                    Self::select_word(cx, me);
                } else {
                    let w = cx.w_mut::<Self>(me);
                    // L487
                    w.sel = Some(1);
                    // L488
                    w.caret = w.buf.len() + 1;
                }
                // L490
                Self::set_last_undo_caret(cx, me);
                // L491
                Self::scroll_caret_into_view(cx, me);
            } else if ctrl && pmatch(cx.w::<Self>(me).buf.as_bytes(), b"[a-z]*://[^ >,;]*") {
                // L493
                let buf = cx.w::<Self>(me).buf.clone();
                cx.sys.platform.open_url(&buf);
            } else {
                // L495
                cx.w_mut::<Self>(me).drag = true;
                // L496
                let (x, y) = cx.pos(me);
                // L497
                let (_width, height) = cx.size(me);
                // L498
                let mut text_x = x + 2.0;
                // L499
                let text_y = y + 2.0;
                // L500
                let text_height = line_height.unwrap_or(height - 4.0);
                let (font, prompt) = {
                    let w = cx.w::<Self>(me);
                    (w.font, w.prompt.clone())
                };
                // L501
                if let Some(prompt) = prompt {
                    // L502
                    text_x +=
                        cx.sg.draw_string_width(text_height, font, &prompt) + text_height / 2.0;
                }
                // L504
                let (cursor_x, cursor_y) = cx.sys.cursor_pos();
                // L505
                let h_off = cx.get(Self::sb_h(cx, me)).offset;
                let v_off = cx.get(Self::sb_v(cx, me)).offset;
                let caret = cx.sg.draw_string_cursor_index(
                    text_height,
                    font,
                    &cx.w::<Self>(me).buf,
                    cursor_x - text_x + h_off,
                    cursor_y - text_y + v_off,
                );
                let w = cx.w_mut::<Self>(me);
                w.caret = caret;
                // L506
                w.sel = Some(caret);
                // L507
                Self::set_last_undo_caret(cx, me);
                // L508
                Self::scroll_caret_into_view(cx, me);
                // L509
                Self::reset_blink(cx, me);
            }
        } else if key == Key::Escape {
            // L512
            return None;
        } else if key == Key::Return {
            // L514
            if line_height.is_some() {
                // L515
                Self::insert(cx, me, "\n");
            } else {
                // L517
                let w = cx.w::<Self>(me);
                if let Some(f) = w.enter_func.clone() {
                    // L518
                    let buf = w.buf.clone();
                    f(cx, &buf);
                }
                // L520
                return None;
            }
        } else if key == Key::Char(b'a') && ctrl {
            // L523
            Self::select_all(cx, me);
        } else if (key == Key::Char(b'c') || key == Key::Char(b'x')) && ctrl && !protected {
            let w = cx.w::<Self>(me);
            // L525
            if w.sel.is_some() && w.sel != Some(w.caret) {
                // L526-L527
                let (left, right) = Self::sel_range(w);
                // L528
                let text = LBuf(bsub(&w.buf, left, Some(right - 1.0)).to_vec()).into_string();
                cx.sys.platform.copy(&text);
                // L529
                if key == Key::Char(b'x') {
                    // L530
                    Self::replace_sel(cx, me, "");
                }
            }
        } else if (key == Key::Char(b'v') && ctrl)
            || (key == Key::RightButton && cx.is_mouse_over(me))
        {
            // L534
            if let Some(text) = cx.sys.platform.paste() {
                let mut text = text;
                // L536
                if let Some(f) = cx.w::<Self>(me).paste_filter.clone() {
                    // L537
                    text = f(cx, &text);
                }
                // L539
                let text = luastr::gsub_bytes_pat(&text, b"[\x80-\xff]", "?");
                let w = cx.w::<Self>(me);
                // L540
                if w.sel.is_some() && w.sel != Some(w.caret) {
                    // L541
                    Self::replace_sel(cx, me, &text);
                } else {
                    // L543
                    Self::insert(cx, me, &text);
                }
            }
        } else if key == Key::Char(b'z') && ctrl {
            // L547
            Self::undo(cx, me);
        } else if key == Key::Char(b'y') && ctrl {
            // L549
            Self::redo(cx, me);
        } else if key == Key::Left {
            // L551
            Self::shift_sel(cx, me, shift);
            // L552
            if cx.w::<Self>(me).caret > 1 {
                let w = cx.w_mut::<Self>(me);
                let mut caret = w.caret as f64;
                // L553
                if ctrl {
                    // L554 Skip leading space, then jump word
                    // L555
                    caret -= umatch_len(bsub(&w.buf, 1.0, Some(caret - 1.0)), b"[%s%p]*$");
                    // L556
                    caret -= umatch_len(bsub(&w.buf, 1.0, Some(caret - 1.0)), b"%w*$");
                } else {
                    // L558
                    caret = utf8::next(w.buf.as_bytes(), Some(caret), Some(-1.0))
                        .map_or(0.0, |(p, _)| p);
                }
                w.caret = pos(caret);
                // L560
                Self::set_last_undo_caret(cx, me);
                // L561
                Self::scroll_caret_into_view(cx, me);
                // L562
                Self::reset_blink(cx, me);
            }
        } else if key == Key::Right {
            // L565
            Self::shift_sel(cx, me, shift);
            // L566
            let w = cx.w::<Self>(me);
            if w.caret <= w.buf.len() {
                let w = cx.w_mut::<Self>(me);
                let mut caret = w.caret as f64;
                // L567
                if ctrl {
                    // L568 Jump word, then skip trailing space,
                    // L569
                    caret += umatch_len(bsub(&w.buf, caret, None), b"^%w*");
                    // L570
                    caret += umatch_len(bsub(&w.buf, caret, None), b"^[%s%p]*");
                } else {
                    // L572
                    caret = utf8::next(w.buf.as_bytes(), Some(caret), Some(1.0))
                        .map_or(w.buf.len() as f64 + 1.0, |(p, _)| p);
                }
                w.caret = pos(caret);
                // L574
                Self::set_last_undo_caret(cx, me);
                // L575
                Self::scroll_caret_into_view(cx, me);
                // L576
                Self::reset_blink(cx, me);
            }
        } else if key == Key::Up && line_height.is_some() {
            // L579
            Self::shift_sel(cx, me, shift);
            // L580
            Self::move_caret_vertically(cx, me, -line_height.unwrap_or_default());
        } else if key == Key::Down && line_height.is_some() {
            // L582
            Self::shift_sel(cx, me, shift);
            // L583
            Self::move_caret_vertically(cx, me, line_height.unwrap_or_default());
        } else if key == Key::Home {
            // L585
            Self::shift_sel(cx, me, shift);
            let w = cx.w_mut::<Self>(me);
            // L586
            if line_height.is_some() && !ctrl {
                // L587
                let n = last_line(bsub(&w.buf, 1.0, Some(w.caret as f64 - 1.0))).len();
                w.caret -= n;
            } else {
                // L589
                w.caret = 1;
            }
            // L591
            Self::set_last_undo_caret(cx, me);
            // L592
            Self::scroll_caret_into_view(cx, me);
            // L593
            Self::reset_blink(cx, me);
        } else if key == Key::End {
            // L595
            Self::shift_sel(cx, me, shift);
            let w = cx.w_mut::<Self>(me);
            // L596
            if line_height.is_some() && !ctrl {
                // L597
                let n = pmatch_len(bsub(&w.buf, w.caret as f64, Some(-1.0)), b"[^\n]*");
                w.caret = pos(w.caret as f64 + n);
            } else {
                // L599
                w.caret = w.buf.len() + 1;
            }
            // L601
            Self::set_last_undo_caret(cx, me);
            // L602
            Self::scroll_caret_into_view(cx, me);
            // L603
            Self::reset_blink(cx, me);
        } else if key == Key::PageUp && line_height.is_some() {
            // L605
            Self::shift_sel(cx, me, shift);
            // L606
            let (_width, height) = cx.size(me);
            // L607
            Self::move_caret_vertically(cx, me, -height + 18.0);
        } else if key == Key::PageDown && line_height.is_some() {
            // L609
            Self::shift_sel(cx, me, shift);
            // L610
            let (_width, height) = cx.size(me);
            // L611
            Self::move_caret_vertically(cx, me, height - 18.0);
        } else if key == Key::Back {
            let w = cx.w::<Self>(me);
            // L613
            if w.sel.is_some() && w.sel != Some(w.caret) {
                // L614
                Self::replace_sel(cx, me, "");
            } else if w.caret > 1 {
                let caret = w.caret as f64;
                let byte = |i: f64| bsub(&w.buf, i, Some(i));
                // L616
                let mut len = 1.0;
                // L617
                if cx.sys.is_key_down(Key::Ctrl) {
                    // L618
                    while caret - len > 1.0
                        && pmatch(byte(caret - len), b"%s")
                        && !pmatch(byte(caret - len - 1.0), b"\n")
                    {
                        // L619
                        len += 1.0;
                    }
                    // L621
                    if pmatch(byte(caret - len), b"%w") {
                        // L622
                        while caret - len > 1.0 && pmatch(byte(caret - len - 1.0), b"%w") {
                            // L623
                            len += 1.0;
                        }
                    }
                }
                // L627
                let new_buf = luastr::splice(&w.buf, caret - len, caret, "");
                let w = cx.w_mut::<Self>(me);
                w.buf = new_buf;
                // L628
                w.caret = pos(caret - len);
                // L629
                w.sel = None;
                // L630
                Self::scroll_caret_into_view(cx, me);
                // L631
                Self::reset_blink(cx, me);
                // L632-L634
                Self::notify_change(cx, me);
                // L635
                UNDO.add_undo_state(cx, me, false);
            }
        } else if key == Key::Delete {
            let w = cx.w::<Self>(me);
            // L638
            if w.sel.is_some() && w.sel != Some(w.caret) {
                // L639
                Self::replace_sel(cx, me, "");
            } else if w.caret <= w.buf.len() {
                let caret = w.caret as f64;
                let n = w.buf.len() as f64;
                let byte = |i: f64| bsub(&w.buf, i, Some(i));
                // L641
                let mut len = 1.0;
                // L642
                if cx.sys.is_key_down(Key::Ctrl) {
                    // L643
                    while caret + len <= n
                        && pmatch(byte(caret + len - 1.0), b"%s")
                        && !pmatch(byte(caret + len), b"\n")
                    {
                        // L644
                        len += 1.0;
                    }
                    // L646
                    if pmatch(byte(caret + len - 1.0), b"%w") {
                        // L647
                        while caret + len <= n && pmatch(byte(caret + len), b"%w") {
                            // L648
                            len += 1.0;
                        }
                    }
                }
                // L652
                let new_buf = luastr::splice(&w.buf, caret, caret + len, "");
                let w = cx.w_mut::<Self>(me);
                w.buf = new_buf;
                // L653
                w.sel = None;
                // L654
                Self::reset_blink(cx, me);
                // L655-L657
                Self::notify_change(cx, me);
                // L658
                UNDO.add_undo_state(cx, me, false);
            }
        } else if key == Key::Tab {
            // L661
            return Some(cx.tab_advance(me, if shift { -1 } else { 1 }));
        } else if (key == Key::Char(b'+') || key == Key::Char(b'-') || key == Key::Char(b'0'))
            && ctrl
            && cx.w::<Self>(me).allow_zoom
        {
            // L663
            Self::zoom_text(cx, me, &key.name());
        }
        // L665
        Some(me)
    }

    /// lua: EditControl.lua:668-727
    /// @pob Classes/EditControl.lua:668-727 h=12233d77
    fn on_key_up(cx: &mut Cx, me: CtrlId, key: Key) -> Focus {
        // L669
        if !cx.is_shown(me) || !cx.is_enabled(me) {
            // L670
            return None;
        }
        // L672
        if let Some(sel) = cx.host(me).sel_control {
            // L673
            let new_sel = cx.on_key_up(sel, key);
            // L674
            if new_sel.is_some() {
                // L675
                return Some(me);
            }
            // L677
            cx.host_mut(me).sel_control = None;
        }
        // L681
        let ctrl = cx.sys.is_key_down(Key::Ctrl);
        let v = Self::sb_v(cx, me);
        // L682
        if key == Key::LeftButton {
            // L683-L685
            if cx.w::<Self>(me).drag {
                cx.w_mut::<Self>(me).drag = false;
            }
        } else if cx.w::<Self>(me).is_numeric {
            let w = cx.w::<Self>(me);
            // L687
            let cur = pob_lua::tonumber(&w.buf);
            let inc = w.number_inc.unwrap_or(1.0);
            let placeholder = w.placeholder.clone();
            let filter_is_d = w.filter == "%D";
            let no_wheel = cx.main.disable_scroll_control_interaction;
            // L688
            if (!no_wheel && key == Key::WheelUp) || key == Key::Up {
                // L689
                if let Some(cur) = cur {
                    // L690
                    Self::set_text(cx, me, &pob_lua::tostring(cur + inc), true);
                } else if let Some(placeholder) = placeholder {
                    // L692-L693
                    let base = pob_lua::tonumber(&placeholder).unwrap_or(0.0);
                    Self::set_text(cx, me, &pob_lua::tostring(base + inc), true);
                } else {
                    // L695
                    Self::set_text(cx, me, "1", true);
                }
            } else if (!no_wheel && key == Key::WheelDown) || key == Key::Down {
                // L699
                if let Some(cur) = cur.filter(|&c| !filter_is_d || c > 0.0) {
                    // L700
                    Self::set_text(cx, me, &pob_lua::tostring(cur - inc), true);
                } else if let Some(placeholder) = placeholder {
                    // L702-L703
                    let base = pob_lua::tonumber(&placeholder).unwrap_or(0.0);
                    Self::set_text(cx, me, &pob_lua::tostring(base - inc), true);
                } else {
                    // L705
                    Self::set_text(cx, me, "0", true);
                }
            }
        } else if ScrollBarControl::is_scroll_up_key(cx, v.id(), key) {
            // L710
            if ctrl && cx.w::<Self>(me).allow_zoom {
                // L711
                Self::zoom_text(cx, me, "+");
            } else if ScrollBarControl::enabled_raw(cx, v.id()) {
                // L713
                ScrollBarControl::scroll(cx, v.id(), -1.0);
            } else {
                // L715
                let h = Self::sb_h(cx, me);
                ScrollBarControl::scroll(cx, h.id(), -1.0);
            }
        } else if ScrollBarControl::is_scroll_down_key(cx, v.id(), key) {
            // L718
            if ctrl && cx.w::<Self>(me).allow_zoom {
                // L719
                Self::zoom_text(cx, me, "-");
            } else if ScrollBarControl::enabled_raw(cx, v.id()) {
                // L721
                ScrollBarControl::scroll(cx, v.id(), 1.0);
            } else {
                // L723
                let h = Self::sb_h(cx, me);
                ScrollBarControl::scroll(cx, h.id(), 1.0);
            }
        }
        // L726
        if cx.base(me).has_focus == Some(true) {
            Some(me)
        } else {
            None
        }
    }

    /// lua: EditControl.lua:729-741
    /// @pob Classes/EditControl.lua:729-741 h=e96cd331
    fn on_char(cx: &mut Cx, me: CtrlId, ch: Key) -> Focus {
        // L730
        if !cx.is_shown(me) || !cx.is_enabled(me) {
            // L731
            return None;
        }
        // L733
        if ch != Key::Char(b'\x08') {
            let key = ch.name();
            let w = cx.w::<Self>(me);
            // L734
            if w.sel.is_some() && w.sel != Some(w.caret) {
                // L735
                Self::replace_sel(cx, me, &key);
            } else {
                // L737
                Self::insert(cx, me, &key);
            }
        }
        // L740
        Some(me)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(cx: &mut Cx, args: EditArgs) -> CtrlId {
        cx.add_free(
            None,
            EditControl::new(Anchor::NONE, rect(0, 0, 200, 20), args),
        )
        .id()
    }

    #[test]
    fn default_filter_follows_main_unicode() {
        let mut cx = Cx::headless();
        let a = edit(&mut cx, EditArgs::default());
        assert_eq!(cx.w::<EditControl>(a).filter, "^%w%p ");
        assert_eq!(cx.w::<EditControl>(a).filter_pattern, "[^%w%p ]");
        cx.main.unicode = true;
        let b = edit(&mut cx, EditArgs::default());
        assert_eq!(cx.w::<EditControl>(b).filter, "%c");
        // numeric filters add the +/- buttons, `clearable` the x button
        let n = edit(
            &mut cx,
            EditArgs {
                filter: Some("%D".into()),
                ..Default::default()
            },
        );
        assert!(cx.w::<EditControl>(n).is_numeric);
        assert!(cx.child(n, "buttonUp").is_some() && cx.child(n, "buttonDown").is_some());
        let c = edit(
            &mut cx,
            EditArgs {
                clearable: true,
                ..Default::default()
            },
        );
        assert!(cx.child(c, "buttonClear").is_some());
    }

    #[test]
    fn insert_filters_replace_sel_rejects() {
        let mut cx = Cx::headless();
        let e = edit(
            &mut cx,
            EditArgs {
                init: Some("ab".into()),
                filter: Some("%D".into()),
                limit: Some(4),
                ..Default::default()
            },
        );
        // EditControl.lua:170: illegal chars are removed, the rest goes in
        EditControl::insert(&mut cx, e, "1x2\r");
        let w = cx.w::<EditControl>(e);
        assert_eq!((w.buf.as_str(), w.caret), ("ab12", 5));
        // :175 over the limit: nothing
        EditControl::insert(&mut cx, e, "3");
        assert_eq!(cx.w::<EditControl>(e).buf, "ab12");
        // :147 ReplaceSel rejects the whole text when one char is illegal
        cx.w_mut::<EditControl>(e).sel = Some(1);
        EditControl::replace_sel(&mut cx, e, "9x");
        assert_eq!(cx.w::<EditControl>(e).buf, "ab12");
        EditControl::replace_sel(&mut cx, e, "9");
        let w = cx.w::<EditControl>(e);
        assert_eq!((w.buf.as_str(), w.caret, w.sel), ("9", 2, None));
    }

    #[test]
    fn caret_moves_write_through_last_undo_state() {
        let mut cx = Cx::headless();
        let e = edit(
            &mut cx,
            EditArgs {
                init: Some("ab".into()),
                ..Default::default()
            },
        );
        EditControl::insert(&mut cx, e, "c");
        assert_eq!(cx.w::<EditControl>(e).caret, 4);
        // LEFT: `self.lastUndoState.caret = self.caret` (L560) writes into undo[1]
        <EditControl as Widget>::on_key_down(&mut cx, e, Key::Left, false);
        assert_eq!(cx.w::<EditControl>(e).caret, 3);
        assert_eq!(cx.w::<EditControl>(e).undo.undo[0].caret.get(), 3);
        EditControl::undo(&mut cx, e);
        let w = cx.w::<EditControl>(e);
        assert_eq!((w.buf.as_str(), w.caret), ("ab", 3));
        // the redo state is the aliased table: its caret is the moved one, not 4
        EditControl::redo(&mut cx, e);
        let w = cx.w::<EditControl>(e);
        assert_eq!((w.buf.as_str(), w.caret), ("abc", 3));
    }

    #[test]
    fn on_char_inserts_the_key_name() {
        let mut cx = Cx::headless();
        let e = edit(&mut cx, EditArgs::default());
        <EditControl as Widget>::on_char(&mut cx, e, Key::Char(b'a'));
        <EditControl as Widget>::on_char(&mut cx, e, Key::Char(b'\x08')); // '\b' is ignored
        <EditControl as Widget>::on_char(&mut cx, e, Key::Unknown); // non-ASCII arrives as "?"
        <EditControl as Widget>::on_char(&mut cx, e, Key::Char(b'\t')); // filtered by "^%w%p "
        assert_eq!(cx.w::<EditControl>(e).buf, "a?");
        // L119: SetProtected(false) still protects
        EditControl::set_protected(&mut cx, e, false);
        assert!(cx.w::<EditControl>(e).protected);
        assert_eq!(cx.w::<EditControl>(e).font, Font::Fixed);
    }

    #[test]
    fn helpers_match_lua() {
        assert_eq!(last_line(b"ab\ncd\nef"), b"ef");
        assert_eq!(last_line(b"ab\n"), b"");
        assert_eq!(last_line(b"ab"), b"ab");
        assert_eq!(newline_count(b"a\n\nb\n"), 3.0);
        assert_eq!(rep_protected(-2.0), "");
        assert_eq!(rep_protected(3.0), "***");
    }
}
