//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/ConfigTab.lua:16-127
//! (`CustomModBlockControl`: one "Custom Modifiers" mod group). Owner: U41.
//!
//! @pob-file Classes/ConfigTab.lua @16de4b82d
//!
//! `newClass("CustomModBlockControl", "ControlHost", "Control")`: a host node (`New::host`) with
//! its own `GetSize`, `IsMouseOver`, `OnKeyDown` and `Draw`.
//!
//! Lua semantics:
//! * `blockData` is the model's group table (`configSet.customModsList[blockIndex]`, a calc
//!   `Table`), aliased exactly as in Lua: the edits write `blockData.title/text/enabled` into
//!   the model table, and BuildModList (calc:CO-C) reads it.
//! * The delete button's closure uses the constructor's `blockIndex` upvalue (not
//!   `self.blockIndex`), as the Lua does.
//! * `GetSize` writes `self.height` (L100). Every reader of the block's size goes through
//!   `GetSize` (the section height closure, ConfigTab:Draw, the hit tests), so the field is kept as
//!   [`CustomModBlockControl::height`], a `Cell` written by `get_size` (ui doc §7.3: a write from
//!   a `&Cx` method); `Base::height` keeps the constructor's 120, which nothing reads.
//! * The enable check's tooltip compares `CheckForUpdate(outputRevision, blockData)` with Lua
//!   table identity: the block keeps one `Rc` handle of `blockData` for its lifetime
//!   ([`CustomModBlockControl::block_key`]), so the identity is the block's table's (a new block
//!   control for the same table gets a new tooltip anyway).
//! * `configTab:UpdateCustomModsControls()` is U41's port in `ctor.rs` (CCR-0110 §2);
//!   `configTab:AddUndoState()` and `configModBrowser.OpenAddModPopup` are U42's
//!   (`ctor::ConfigHooks`, CCR-0110 §1).
//!
//! DIVERGENCE: none. ORDER: no `pairs` loops. Props with side effects (ui doc §7.3): none (the
//! `GetSize` write above is a method, not a prop).

use std::any::Any;
use std::cell::Cell;
use std::rc::Rc;

use pob_lua::n;
use pob_lua::pattern::{gmatch, r#match};
use pob_lua::string::StripEscapes;
use pob_ui_core::controls::edit::InactiveText;
use pob_ui_core::controls::{
    ButtonControl, CheckBoxControl, EditArgs, EditControl, ResizableEditControl,
};
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::{Tooltip, TtKey, TtObj};

use super::ctor::{self, config_hooks};
use super::ConfigTab;
use crate::app::CxApp;
use crate::model::{Table, Val};

/// @pob Classes/ConfigTab.lua:16-17 h=3300f6ff
pub struct CustomModBlockControl {
    /// `self.configTab`
    pub config_tab: CtrlId,
    /// `self.blockIndex` (1-based)
    pub block_index: usize,
    /// `self.blockData` (the model's group table)
    pub block_data: Table,
    /// `blockData`'s identity for `CheckForUpdate` (see the module notes)
    pub block_key: Rc<dyn Any>,
    /// `self.height` as `GetSize` writes it (L100); starts as the rect's height
    pub height: Cell<f64>,
}

/// `configTab:BuildModList()` (calc:CO-C) on the open build.
fn build_mod_list(cx: &Cx) {
    let m = cx.build().model.clone();
    m.configTab.BuildModList(&m);
}

/// The common tail of the block's edits: `configTab:AddUndoState()`, `configTab:BuildModList()`,
/// `configTab.build.buildFlag = true`.
fn changed(cx: &mut Cx, config_tab: CtrlId) {
    let h = config_hooks();
    (h.add_undo_state)(cx, config_tab);
    build_mod_list(cx);
    ConfigTab::pull_var_controls(cx, config_tab);
    cx.build().model.buildFlag.set(true);
}

impl CustomModBlockControl {
    /// lua: ConfigTab.lua:24-96 CustomModBlockControl(anchor, rect, configTab, blockIndex, blockData)
    /// @pob Classes/ConfigTab.lua:19-96 h=82c6ef6c
    pub fn new(
        anchor: Anchor,
        rect_: [Prop<f64>; 4],
        config_tab: CtrlId,
        block_index: usize,
        block_data: Table,
    ) -> New<CustomModBlockControl> {
        let height = rect_[3].raw();
        // L25 self:Control(anchor, rect); L26 self:ControlHost()
        New::new(
            Base::new(anchor, rect_),
            CustomModBlockControl {
                // L28
                config_tab,
                // L29
                block_index,
                // L30
                block_data: block_data.clone(),
                block_key: Rc::new(block_data.clone()),
                height: Cell::new(height),
            },
        )
        .host()
        .init(move |cx, me| {
            let me = me.id();
            // L32-42
            let delete_btn = cx.add(
                me,
                "deleteBtn",
                ButtonControl::new(
                    Anchor::new(TopLeft, me, TopLeft),
                    rect(0, 0, 20, 18),
                    "^1X",
                    move |cx| {
                        // L33
                        let custom_mods_list = custom_mods_list(cx);
                        // L34
                        custom_mods_list.remove(Some(block_index as i64));
                        // L35
                        if custom_mods_list.is_empty() {
                            // L36
                            let t = Table::new();
                            t.set(n!("title"), "Default");
                            t.set(n!("enabled"), true);
                            t.set(n!("text"), "");
                            custom_mods_list.push(t);
                        }
                        // L38
                        ConfigTab::update_custom_mods_controls(cx, config_tab);
                        // L39-41
                        changed(cx, config_tab);
                    },
                ),
            );

            // L44-49
            let bd = block_data.clone();
            let title_edit = cx.add(
                me,
                "titleEdit",
                EditControl::new(
                    Anchor::new(Left, delete_btn, Right),
                    rect(6, 0, 222, 18),
                    EditArgs {
                        init: Some(title_or_empty(&block_data)),
                        change_func: Some(Rc::new(move |cx: &mut Cx, buf: &str| {
                            // L45
                            bd.set(n!("title"), buf);
                            // L46-48
                            changed(cx, config_tab);
                        })),
                        ..Default::default()
                    },
                ),
            );

            // L51-53
            let bd = block_data.clone();
            cx.add(
                me,
                "addModBtn",
                ButtonControl::new(
                    Anchor::new(Left, title_edit, Right),
                    rect(6, 0, 58, 18),
                    "^7Add Mod",
                    move |cx| {
                        // L52
                        let f = config_hooks().open_add_mod_popup.clone();
                        f(cx, config_tab, &bd);
                    },
                ),
            );

            // L55-60
            let bd = block_data.clone();
            let enable_check = cx.add(
                me,
                "enableCheck",
                CheckBoxControl::new(
                    Anchor::new(TopRight, me, TopRight),
                    rect(0, 0, 18, 0),
                    "",
                    Some(Rc::new(move |cx: &mut Cx, state: bool| {
                        // L56
                        bd.set(n!("enabled"), state);
                        // L57-59
                        changed(cx, config_tab);
                    })),
                    None,
                    false,
                ),
            );
            // L61 self.controls.enableCheck.state = blockData.enabled ~= false
            cx.get_mut(enable_check).state =
                !matches!(block_data.get(n!("enabled")), Val::Bool(false));
            // L62-77
            let bd = block_data.clone();
            let key = cx.w::<Self>(me).block_key.clone();
            cx.get_mut(enable_check).tt.tooltip_func = Some(Rc::new(
                move |cx: &mut Cx, tooltip: &mut Tooltip, _state: bool| {
                    enable_check_tooltip(cx, tooltip, config_tab, &bd, &key);
                },
            ));

            // L79-84
            let bd = block_data.clone();
            let text_edit = cx.add(
                me,
                "textEdit",
                ResizableEditControl::new(
                    Anchor::new(TopLeft, me, TopLeft),
                    rect(0, 22, 344, 80),
                    EditArgs {
                        init: Some(text_or_empty(&block_data)),
                        filter: Some("^%C\t\n".into()),
                        change_func: Some(Rc::new(move |cx: &mut Cx, buf: &str| {
                            // L80
                            bd.set(n!("text"), buf);
                            // L81-83
                            changed(cx, config_tab);
                        })),
                        line_height: Some(16.0),
                        ..Default::default()
                    },
                    Some(344.0),
                    Some(40.0),
                    Some(344.0),
                    Some(600.0),
                ),
            );

            // L86-94
            cx.w_mut::<EditControl>(text_edit.id()).inactive_text =
                Some(InactiveText::Fn(Rc::new(|cx: &mut Cx, val: &str| {
                    inactive_text(cx, val)
                })));
            // L95 return self
        })
    }
}

/// `configTab.configSets[configTab.activeConfigSetId].customModsList` (nil: the Lua index of
/// `table.remove(nil, …)` raises).
fn custom_mods_list(cx: &Cx) -> Table {
    let set = cx.build().model.configTab.activeConfigSet();
    set.table(n!("customModsList"))
        .unwrap_or_else(|| lua_error!("bad argument #1 to 'remove' (table expected, got nil)"))
}

/// `blockData.title or ""` (EditControl's `SetText(init or "")` applies `tostring`).
fn title_or_empty(block_data: &Table) -> String {
    tostring_or_empty(&block_data.get(n!("title")))
}

/// `blockData.text or ""`.
fn text_or_empty(block_data: &Table) -> String {
    tostring_or_empty(&block_data.get(n!("text")))
}

fn tostring_or_empty(v: &Val) -> String {
    match v {
        Val::Nil | Val::Bool(false) => String::new(),
        other => ctor::tostring_val(other),
    }
}

/// lua: ConfigTab.lua:62-77 (the enable check's `tooltipFunc(tooltip)`).
fn enable_check_tooltip(
    cx: &mut Cx,
    tooltip: &mut Tooltip,
    config_tab: CtrlId,
    block_data: &Table,
    key: &Rc<dyn Any>,
) {
    let model = cx.build().model.clone();
    // L63
    let rev = model.outputRevision.get();
    if tooltip.check_for_update(cx, &[TtKey::Num(rev), TtKey::Obj(TtObj(key.clone()))]) {
        // L64 `if configTab.build.calcsTab then` (always set)
        // L65
        let misc = cx.build_mut().calc.misc_calculator(&model);
        // Poll::Pending (wasm only): no calcFunc yet, as `if calcFunc` below
        if let std::task::Poll::Ready((calc_func, calc_base)) = misc {
            // L66 if calcFunc then
            // L67
            let cur_state = !matches!(block_data.get(n!("enabled")), Val::Bool(false));
            // L68
            block_data.set(n!("enabled"), !cur_state);
            // L69
            model.configTab.BuildModList(&model);
            // L70
            let output = match calc_func.call(None, None) {
                std::task::Poll::Ready(o) => Some(o),
                std::task::Poll::Pending => None,
            };
            // L71
            block_data.set(n!("enabled"), cur_state);
            // L72
            model.configTab.BuildModList(&model);
            // L73
            if let Some(output) = output {
                let header = if cur_state {
                    "^7Disabling this group will give you:"
                } else {
                    "^7Enabling this group will give you:"
                };
                crate::build::stats::add_stat_compares_to_tooltip(
                    cx, tooltip, &calc_base, &output, header, None,
                );
            }
        }
    }
    let _ = config_tab;
}

/// lua: ConfigTab.lua:86-94 (`textEdit.inactiveText = function(val)`).
fn inactive_text(cx: &mut Cx, val: &str) -> String {
    // L87
    let mut inactive_text = String::new();
    // L88 for line in val:gmatch("([^\n]*)\n?") do
    let lines: Vec<String> = gmatch(val, "([^\n]*)\n?")
        .map(|c| c[0].to_str().into_owned())
        .collect();
    for line in lines {
        // L89 StripEscapes(line):match("^%s*(.-)%s*$")
        let stripped = StripEscapes(&line);
        let stripped_line = r#match(&stripped, "^%s*(.-)%s*$", None)
            .map(|c| c[0].to_str().into_owned())
            .unwrap_or_else(|| lua_error!("attempt to index a nil value"));
        // L90 local mods, extra = modLib.parseMod(strippedLine)
        let (mods, extra) = pob_modparser::parseMod(&stripped_line, false);
        // L91
        let colour = if mods.is_some() && extra.is_none() {
            cx.main.cc.MAGIC.clone()
        } else {
            cx.main.cc.UNSUPPORTED.clone()
        };
        let text = if cx.sys.is_key_down(Key::Alt) {
            &stripped_line
        } else {
            &line
        };
        inactive_text.push_str(&colour);
        inactive_text.push_str(text);
        inactive_text.push('\n');
    }
    // L93
    inactive_text
}

impl Widget for CustomModBlockControl {
    const NAME: &'static str = "CustomModBlockControl";
    const CAPS: Caps = Caps::DRAW.union(Caps::IS_MOUSE_OVER).union(Caps::KEY_DOWN);

    /// lua: ConfigTab.lua:98-102 GetSize
    /// @pob Classes/ConfigTab.lua:98-102 h=a92aba80
    fn get_size(cx: &Cx, me: CtrlId) -> (f64, f64) {
        // L99 `self.controls.textEdit and self.controls.textEdit.height or 80` (the raw field)
        let text_height = cx
            .child(me, "textEdit")
            .map_or(80.0, |t| cx.base(t).height.raw());
        // L100
        let w = cx.w::<Self>(me);
        w.height.set(22.0 + text_height + 4.0);
        // L101
        (344.0, w.height.get())
    }

    /// lua: ConfigTab.lua:104-109 IsMouseOver
    /// @pob Classes/ConfigTab.lua:104-109 h=7357db34
    fn is_mouse_over(cx: &Cx, me: CtrlId) -> bool {
        // L105
        if !cx.is_shown(me) {
            // L106
            return false;
        }
        // L108
        cx.is_mouse_in_bounds(me) || cx.get_mouse_over_control(me).is_some()
    }

    /// lua: ConfigTab.lua:111-119 OnKeyDown
    /// @pob Classes/ConfigTab.lua:111-119 h=01702ddd
    fn on_key_down(cx: &mut Cx, me: CtrlId, key: Key, double_click: bool) -> Focus {
        // L112
        if !cx.is_shown(me) || !cx.is_enabled(me) {
            // L113
            return None;
        }
        // L115
        let m_over_control = cx.get_mouse_over_control(me);
        // L116
        if let Some(m) = m_over_control.filter(|&m| cx.has(m, Caps::KEY_DOWN)) {
            // L117
            return cx.on_key_down(m, key, double_click);
        }
        None
    }

    /// lua: ConfigTab.lua:121-127 Draw
    /// @pob Classes/ConfigTab.lua:121-127 h=4d21abc8
    fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, _no_tooltip: bool) {
        // L122
        if !cx.is_shown(me) {
            // L123
            return;
        }
        // L125
        cx.size(me);
        // L126
        cx.draw_controls(me, vp, None);
    }
}
