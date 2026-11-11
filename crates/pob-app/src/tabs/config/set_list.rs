//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/ConfigSetListControl.lua 1-114
//! (`newClass("ConfigSetListControl", "ListControl")`). Owner: U42.
//!
//! @pob-file Classes/ConfigSetListControl.lua @16de4b82d
//!
//! The list is `configTab.configSetOrderList` itself (L14; calc:BM-B's
//! `build.configTab.configSetOrderList`, aliased through U32's [`IdListSource`], so a drag
//! reorder writes through); rows are config set ids. Config sets are the model's tables
//! (`build.configTab.configSets[id]`).
//!
//! ListControl hooks: `GetRowValue`, `OnOrderChange`, `OnSelClick`, `OnSelDelete`,
//! `OnSelKeyDown`.
//!
//! Lua semantics:
//! * `configTab.configSets[id]` with a nil or non-integral id reads nil; the set's fields are
//!   then indexed, which is the Lua index error.
//! * `self.configTab:SetActiveConfigSet(id)` is `ConfigTab::set_active_config_set` (core.rs).
//! * `self.configTab.modFlag = true` writes the tab's UndoHandler flag.
//!
//! DIVERGENCE: none. ORDER: no `pairs` loops (`copyTable` copies the set; the copy's key order
//! is unobservable: every reader indexes it by key).
//!
//! Props with side effects (ui doc §7.3): none.
#![allow(non_snake_case)]

use std::rc::Rc;

use pob_lua::math::m_max;
use pob_lua::{n, pattern};
use pob_ui_core::controls::list::{ListControl, ListData, ListHooks, ListValue};
use pob_ui_core::controls::{ButtonControl, EditArgs, EditControl, LabelControl};
use pob_ui_core::popups::{self, PopupOpts};
use pob_ui_core::prelude::*;

use super::ConfigTab;
use crate::app::CxApp;
use crate::model::{Shared, Table};
use crate::tabs::items::item_list::{id_value, value_id, IdListSource};

// @pob Classes/ConfigSetListControl.lua:1-12 h=5cd0c2be
// (header comment, local aliases and newClass: the struct and Widget impl below)

/// `ConfigSetListControl`: the build's config sets (the config set manage popup).
pub struct ConfigSetListControl {
    pub list: ListControl,
    /// `self.configTab`
    pub config_tab: CtrlId,
}

static HOOKS: ListHooks = ListHooks {
    get_row_value: ConfigSetListControl::GetRowValue,
    on_order_change: Some(ConfigSetListControl::OnOrderChange),
    on_sel_click: Some(ConfigSetListControl::OnSelClick),
    on_sel_delete: Some(ConfigSetListControl::OnSelDelete),
    on_sel_key_down: Some(ConfigSetListControl::OnSelKeyDown),
    ..ListHooks::BASE
};

/// `configTab.configSets[id]` key (integral ids; any other id reads nil).
fn set_key(id: f64) -> Option<u32> {
    (id.fract() == 0.0 && id >= 0.0 && id <= f64::from(u32::MAX)).then_some(id as u32)
}

/// `configTab.configSets[id]`
fn config_set(cx: &Cx, id: f64) -> Option<Table> {
    cx.build().model.configTab.configSet(id)
}

/// `configTab.configSets[id]` that Lua then indexes.
fn config_set_or_raise(cx: &Cx, id: f64) -> Table {
    config_set(cx, id)
        .unwrap_or_else(|| lua_error!("attempt to index local 'configSet' (a nil value)"))
}

/// `configSet.title or "Default"`
fn set_title(set: &Table) -> String {
    set.str(n!("title"))
        .map_or_else(|| "Default".to_owned(), |s| s.to_string())
}

impl ConfigSetListControl {
    /// lua: ConfigSetListControl.lua:13-45 `ConfigSetListClass:ConfigSetListControl(anchor,
    /// rect, configTab)`
    /// @pob Classes/ConfigSetListControl.lua:13-45 h=c7161580
    /// `order_list` is `configTab.configSetOrderList` (the model's, aliased).
    pub fn new(
        anchor: Anchor,
        r: [Prop<f64>; 4],
        config_tab: CtrlId,
        order_list: Shared<Vec<u32>>,
    ) -> New<ConfigSetListControl> {
        // L14
        let l = ListControl::state(
            &HOOKS,
            16.0,
            true,
            true,
            ListData::Source(Rc::new(IdListSource(order_list))),
            false,
        );
        New::new(
            Base::new(anchor, r),
            ConfigSetListControl {
                list: l,
                // L15
                config_tab,
            },
        )
        .host()
        .init(|cx, me| ListControl::init(cx, me.0))
        .init(move |cx, me| {
            let me = me.0;
            let sel_value = move |cx: &Cx| cx.w::<ListControl>(me).sel_value.as_ref().map(value_id);
            // L16-25
            let copy = cx.add(
                me,
                "copy",
                ButtonControl::new(
                    Anchor::new(BottomLeft, me, Top),
                    rect(2, -4, 60, 18),
                    "Copy",
                    move |cx| {
                        // L17
                        let configSet = sel_value(cx)
                            .and_then(|id| config_set(cx, f64::from(id)))
                            .unwrap_or_else(|| {
                                lua_error!("bad argument #1 to 'pairs' (table expected, got nil)")
                            });
                        // L18
                        let newConfigSet = configSet.copyTable(false);
                        // L19
                        newConfigSet.set(n!("id"), 1.0);
                        // L20
                        while config_set(cx, newConfigSet.num(n!("id"))).is_some() {
                            // L21
                            let id = newConfigSet.num(n!("id"));
                            newConfigSet.set(n!("id"), id + 1.0);
                        }
                        // L23
                        let k = set_key(newConfigSet.num(n!("id"))).expect("an integer id");
                        cx.build()
                            .model
                            .configTab
                            .configSets
                            .with_mut(|m| m.insert(k, newConfigSet.clone()));
                        // L24
                        ConfigSetListControl::RenameSet(cx, me, &newConfigSet, true);
                    },
                ),
            );
            // L26-28
            cx.base_mut(copy).enabled = Prop::f(move |cx, _| {
                // L27
                sel_value(cx).is_some()
            });
            // L29-31
            let delete = cx.add(
                me,
                "delete",
                ButtonControl::new(
                    Anchor::new(Left, copy, Right),
                    rect(4, 0, 60, 18),
                    "Delete",
                    move |cx| {
                        // L30
                        let (index, value) = {
                            let w = cx.w::<ListControl>(me);
                            (w.sel_index, w.sel_value.clone())
                        };
                        let value = value.unwrap_or_else(|| {
                            lua_error!("attempt to index local 'configSet' (a nil value)")
                        });
                        ConfigSetListControl::OnSelDelete(cx, me, index.unwrap_or(0), &value);
                    },
                ),
            );
            // L32-34
            cx.base_mut(delete).enabled = Prop::f(move |cx, _| {
                // L33
                sel_value(cx).is_some() && ListControl::list_len(cx, me) > 1
            });
            // L35-37
            let rename = cx.add(
                me,
                "rename",
                ButtonControl::new(
                    Anchor::new(BottomRight, me, Top),
                    rect(-2, -4, 60, 18),
                    "Rename",
                    move |cx| {
                        // L36
                        let set = sel_value(cx)
                            .and_then(|id| config_set(cx, f64::from(id)))
                            .unwrap_or_else(|| {
                                lua_error!("attempt to index local 'configSet' (a nil value)")
                            });
                        ConfigSetListControl::RenameSet(cx, me, &set, false);
                    },
                ),
            );
            // L38-40
            cx.base_mut(rename).enabled = Prop::f(move |cx, _| {
                // L39
                sel_value(cx).is_some()
            });
            // L41-43
            cx.add(
                me,
                "new",
                ButtonControl::new(
                    Anchor::new(Right, rename, Left),
                    rect(-4, 0, 60, 18),
                    "New",
                    move |cx| {
                        // L42
                        let set = cx.build().model.configTab.NewConfigSet(None, None);
                        ConfigSetListControl::RenameSet(cx, me, &set, true);
                    },
                ),
            );
            // L44 return self
        })
    }

    /// lua: ConfigSetListControl.lua:47-73 `ConfigSetListClass:RenameSet(configSet, addOnName)`
    /// @pob Classes/ConfigSetListControl.lua:47-73 h=c9f8c59e
    pub fn RenameSet(cx: &mut Cx, me: CtrlId, configSet: &Table, addOnName: bool) {
        let title = configSet.str(n!("title")).map(|s| s.to_string());
        // L72 (the arguments are evaluated after the constructors; these draw nothing)
        let popup_title = if title.is_some() {
            "Rename"
        } else {
            "Set Name"
        };
        let set = configSet.clone();
        popups::open_popup(
            cx,
            370.0,
            100.0,
            popup_title,
            move |cx, p| {
                let p = p.id();
                // L48 local controls = { }
                // L49
                cx.add(
                    p,
                    "label",
                    LabelControl::new(
                        Anchor::NONE,
                        rect(0, 20, 0, 16),
                        "^7Enter name for this config set:",
                    ),
                );
                // L50-52
                cx.add(
                    p,
                    "edit",
                    EditControl::new(
                        Anchor::NONE,
                        rect(0, 40, 350, 20),
                        EditArgs {
                            init: title,
                            limit: Some(100),
                            change_func: Some(Rc::new(move |cx, buf| {
                                // L51
                                let save = cx.child_as::<ButtonControl>(p, "save");
                                let ok = pattern::find(buf, "%S", None, false).is_some();
                                cx.base_mut(save).enabled = Prop::Val(ok);
                            })),
                            ..EditArgs::default()
                        },
                    ),
                );
                // L53-64
                let set_s = set.clone();
                let save = cx.add(
                    p,
                    "save",
                    ButtonControl::new(Anchor::NONE, rect(-45, 70, 80, 20), "Save", move |cx| {
                        let edit = cx.child_as::<EditControl>(p, "edit");
                        // L54
                        let buf = cx.get(edit).buf.clone();
                        set_s.set(n!("title"), buf.as_str());
                        let config_tab = cx.w::<ConfigSetListControl>(me).config_tab;
                        // L55
                        ConfigTab::set_mod_flag_of(cx, config_tab, true);
                        // L56
                        if addOnName {
                            // L57
                            let id = set_s.num(n!("id"));
                            let n = ListControl::list_len(cx, me);
                            ListControl::list_insert(cx, me, n + 1, id_value(id as u32));
                            // L58-59
                            let n = ListControl::list_len(cx, me);
                            let w = cx.w_mut::<ListControl>(me);
                            w.sel_index = Some(n);
                            w.sel_value = Some(id_value(id as u32));
                        }
                        // L61
                        ConfigTab::add_undo_state(cx, config_tab);
                        // L62
                        let f = crate::build::init::hooks(cx).sync_loadouts;
                        f(cx);
                        // L63
                        popups::close_popup(cx);
                    }),
                );
                // L65
                cx.base_mut(save).enabled = Prop::Val(false);
                // L66-71
                let set_c = set.clone();
                cx.add(
                    p,
                    "cancel",
                    ButtonControl::new(Anchor::NONE, rect(45, 70, 80, 20), "Cancel", move |cx| {
                        // L67
                        if addOnName {
                            // L68
                            if let Some(k) = set_key(set_c.num(n!("id"))) {
                                cx.build()
                                    .model
                                    .configTab
                                    .configSets
                                    .with_mut(|m| m.shift_remove(&k));
                            }
                        }
                        // L70
                        popups::close_popup(cx);
                    }),
                );
            },
            // L72
            PopupOpts {
                enter: Some("save".into()),
                default: Some("edit".into()),
                escape: Some("cancel".into()),
                ..PopupOpts::default()
            },
        );
    }

    /// lua: ConfigSetListControl.lua:75-80 `ConfigSetListClass:GetRowValue(column, index,
    /// configSetId)`
    /// @pob Classes/ConfigSetListControl.lua:75-80 h=6d8a08e3
    fn GetRowValue(cx: &Cx, _me: CtrlId, column: usize, _index: usize, v: &ListValue) -> String {
        let id = f64::from(value_id(v));
        // L76
        let configSet = config_set(cx, id);
        // L77
        if column == 1 {
            let configSet = configSet
                .unwrap_or_else(|| lua_error!("attempt to index local 'configSet' (a nil value)"));
            // L78
            let current = if id == cx.build().model.configTab.activeConfigSetId.get() {
                "  ^9(Current)"
            } else {
                ""
            };
            return format!("{}{current}", set_title(&configSet));
        }
        String::new()
    }

    /// lua: ConfigSetListControl.lua:82-84 `ConfigSetListClass:OnOrderChange()`
    /// @pob Classes/ConfigSetListControl.lua:82-84 h=1421c929
    fn OnOrderChange(cx: &mut Cx, me: CtrlId, _sel_index: usize, _sel_drag_index: usize) {
        // L83
        let config_tab = cx.w::<ConfigSetListControl>(me).config_tab;
        ConfigTab::set_mod_flag_of(cx, config_tab, true);
    }

    /// lua: ConfigSetListControl.lua:86-91 `ConfigSetListClass:OnSelClick(index, configSetId,
    /// doubleClick)`
    /// @pob Classes/ConfigSetListControl.lua:86-91 h=ea02993b
    fn OnSelClick(
        cx: &mut Cx,
        me: CtrlId,
        _index: usize,
        v: &ListValue,
        doubleClick: bool,
    ) -> Option<bool> {
        let id = f64::from(value_id(v));
        let config_tab = cx.w::<ConfigSetListControl>(me).config_tab;
        // L87
        if doubleClick && id != cx.build().model.configTab.activeConfigSetId.get() {
            // L88
            ConfigTab::set_active_config_set(cx, Some(config_tab), Some(id));
            // L89
            ConfigTab::add_undo_state(cx, config_tab);
        }
        None
    }

    /// lua: ConfigSetListControl.lua:93-108 `ConfigSetListClass:OnSelDelete(index, configSetId)`
    /// @pob Classes/ConfigSetListControl.lua:93-108 h=9e418cb8
    fn OnSelDelete(cx: &mut Cx, me: CtrlId, index: usize, v: &ListValue) {
        let id = value_id(v);
        // L94
        let configSet = config_set(cx, f64::from(id));
        // L95
        if ListControl::list_len(cx, me) > 1 {
            let configSet = configSet
                .unwrap_or_else(|| lua_error!("attempt to index local 'configSet' (a nil value)"));
            // L96-106
            popups::open_confirm_popup(
                cx,
                "Delete Config Set",
                &format!(
                    "Are you sure you want to delete '{}'?",
                    set_title(&configSet)
                ),
                "Delete",
                move |cx| {
                    let config_tab = cx.w::<ConfigSetListControl>(me).config_tab;
                    // L97
                    ListControl::list_remove(cx, me, index);
                    // L98
                    cx.build()
                        .model
                        .configTab
                        .configSets
                        .with_mut(|m| m.shift_remove(&id));
                    // L99-100
                    {
                        let w = cx.w_mut::<ListControl>(me);
                        w.sel_index = None;
                        w.sel_value = None;
                    }
                    // L101
                    if f64::from(id) == cx.build().model.configTab.activeConfigSetId.get() {
                        // L102
                        let i = m_max(1.0, index as f64 - 1.0) as usize;
                        let next =
                            ListControl::list_get(cx, me, i).map(|v| f64::from(value_id(&v)));
                        ConfigTab::set_active_config_set(cx, Some(config_tab), next);
                    }
                    // L104
                    ConfigTab::add_undo_state(cx, config_tab);
                    // L105
                    let f = crate::build::init::hooks(cx).sync_loadouts;
                    f(cx);
                },
                None,
            );
        }
    }

    /// lua: ConfigSetListControl.lua:110-114 `ConfigSetListClass:OnSelKeyDown(index,
    /// configSetId, key)`
    /// @pob Classes/ConfigSetListControl.lua:110-114 h=eb794a70
    fn OnSelKeyDown(cx: &mut Cx, me: CtrlId, _index: usize, v: &ListValue, key: Key) {
        // L111
        if key == Key::F(2) {
            // L112
            let set = config_set_or_raise(cx, f64::from(value_id(v)));
            Self::RenameSet(cx, me, &set, false);
        }
    }
}

impl Widget for ConfigSetListControl {
    const NAME: &'static str = "ConfigSetListControl";
    const CAPS: Caps = <ListControl as Widget>::CAPS;

    fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, no_tooltip: bool) {
        ListControl::draw(cx, me, vp, no_tooltip);
    }
    fn is_mouse_over(cx: &Cx, me: CtrlId) -> bool {
        ListControl::is_mouse_over(cx, me)
    }
    fn on_key_down(cx: &mut Cx, me: CtrlId, key: Key, double_click: bool) -> Focus {
        ListControl::on_key_down(cx, me, key, double_click)
    }
    fn on_key_up(cx: &mut Cx, me: CtrlId, key: Key) -> Focus {
        ListControl::on_key_up(cx, me, key)
    }
    fn duck(cx: &Cx, me: CtrlId, q: Duck) -> bool {
        ListControl::duck(cx, me, q)
    }

    pob_ui_core::impl_parts!(ConfigSetListControl: list => ListControl);
}
