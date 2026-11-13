//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/PassiveSpecListControl.lua 1-132.
//! Owner: U25.
//!
//! @pob-file Classes/PassiveSpecListControl.lua @16de4b82d
//!
//! `newClass("PassiveSpecListControl", "ListControl")`: the "Manage Passive Trees" list. The
//! list is `treeTab.specList` itself (L14; calc:BM-A's `model.treeTab.specList`, aliased through
//! [`SharedVecSource`]), so reordering and deleting write through to the model.
//!
//! ListControl hooks: `GetRowValue`, `OnOrderChange`, `OnSelClick`, `OnSelDelete`,
//! `OnSelKeyDown`.
//!
//! DIVERGENCE(items-spec-select): `self.treeTab.build.itemsTab.controls.specSelect` (L125) is
//! ItemsTab's control (U33): always there with the real ItemsTab, skipped when a test stand-in
//! has none (core.rs module docs).
//!
//! ORDER: no `pairs` loops. Props with side effects (ui doc §7.3): none.
#![allow(non_snake_case)]

use std::rc::Rc;

use pob_lua::math::m_max;
use pob_lua::pattern;
use pob_ui_core::controls::list::{ListControl, ListData, ListHooks, ListSource, ListValue};
use pob_ui_core::controls::{
    ButtonControl, DropDownControl, DropItem, EditArgs, EditControl, LabelControl,
};
use pob_ui_core::popups::{self, PopupOpts};
use pob_ui_core::prelude::*;

use super::core::items_spec_select;
use super::TreeTab;
use crate::app::CxApp;
use crate::build::init::{self, flush_window_titles};
use crate::model::{self, PassiveSpec, PassiveSpecRef};
// not re-exported by model.rs (U15; CCR-0115)
use pob_calc::classes::passive_spec::new_PassiveSpec; // CCR-0123 §9

// @pob Classes/PassiveSpecListControl.lua:1-12 h=d33c033a
// (header comment, local aliases, newClass: the struct and Widget impl below)

/// `PassiveSpecListControl`
pub struct PassiveSpecListControl {
    pub list: ListControl,
    /// `self.treeTab`
    pub tree_tab: CtrlId,
}

static HOOKS: ListHooks = ListHooks {
    get_row_value: PassiveSpecListControl::GetRowValue,
    on_order_change: Some(PassiveSpecListControl::OnOrderChange),
    on_sel_click: Some(PassiveSpecListControl::OnSelClick),
    on_sel_delete: Some(PassiveSpecListControl::OnSelDelete),
    on_sel_key_down: Some(PassiveSpecListControl::OnSelKeyDown),
    ..ListHooks::BASE
};

/// `self.list = treeTab.specList` (L14): the model's list, aliased (reorder/delete write
/// through; Lua table semantics as `SharedVecSource`, whose `T: Clone` a spec is not).
struct SpecListSource(pob_lua::Shared<Vec<PassiveSpecRef>>);

impl ListSource for SpecListSource {
    fn len(&self, _cx: &Cx) -> usize {
        self.0.len()
    }
    fn get(&self, _cx: &Cx, i: usize) -> Option<ListValue> {
        self.0.get(i).map(ListValue::of)
    }
    fn insert(&self, _cx: &mut Cx, i: usize, v: ListValue) {
        let spec = spec_of(&v);
        self.0.with_mut(|l| {
            // lj_tab insert: 1 <= i <= #t + 1
            if i == 0 || i > l.len() + 1 {
                lua_error!("bad argument #2 to 'insert' (position out of bounds)");
            }
            l.insert(i - 1, spec);
        });
    }
    fn remove(&self, _cx: &mut Cx, i: usize) -> ListValue {
        let spec = self.0.with_mut(|l| {
            if i == 0 || i > l.len() {
                lua_error!("t_remove: position out of range");
            }
            l.remove(i - 1)
        });
        ListValue::of(spec)
    }
}

/// The spec of a row.
fn spec_of(v: &ListValue) -> PassiveSpecRef {
    v.rc::<PassiveSpec>()
        .unwrap_or_else(|| lua_error!("PassiveSpecListControl: a row that is not a spec"))
}

/// `self.selValue`
fn sel_value(cx: &Cx, me: CtrlId) -> Option<PassiveSpecRef> {
    cx.w::<ListControl>(me).sel_value.as_ref().map(spec_of)
}

/// `isValueInArray(self.list, self.treeTab.build.spec)` (Common.lua:569-575; nil is NaN in
/// the model's `activeSpec`)
fn index_of_build_spec(cx: &Cx) -> f64 {
    let m = &cx.build().model;
    let spec = m.spec.get();
    m.treeTab
        .specList
        .snapshot()
        .iter()
        .position(|s| Rc::ptr_eq(s, &spec))
        .map_or(f64::NAN, |i| (i + 1) as f64)
}

impl PassiveSpecListControl {
    /// lua: PassiveSpecListControl.lua:13-48 `PassiveSpecListClass:PassiveSpecListControl(anchor,
    /// rect, treeTab)`
    /// @pob Classes/PassiveSpecListControl.lua:13-48 h=33a617b1
    pub fn new(anchor: Anchor, r: [Prop<f64>; 4], tree_tab: CtrlId) -> New<PassiveSpecListControl> {
        New::new(
            Base::new(anchor, r),
            PassiveSpecListControl {
                // L14 (filled in `init`: the list is the model's)
                list: ListControl::state(&HOOKS, 16.0, true, true, ListData::default(), false),
                // L15
                tree_tab,
            },
        )
        .host()
        .init(|cx, me| {
            let spec_list = cx.build().model.treeTab.specList.clone();
            cx.w_mut::<ListControl>(me.0).list =
                ListData::Source(Rc::new(SpecListSource(spec_list)));
            ListControl::init(cx, me.0);
        })
        .init(|cx, me| {
            let me = me.0;
            // L16-23
            let copy = cx.add(
                me,
                "copy",
                ButtonControl::new(
                    Anchor::new(BottomLeft, me, Top),
                    rect(2, -4, 60, 18),
                    "Copy",
                    move |cx| {
                        let m = cx.build().model.clone();
                        let sel = sel_value(cx, me).unwrap_or_else(|| {
                            lua_error!("attempt to index field 'selValue' (a nil value)")
                        });
                        // L17
                        let new_spec = new_PassiveSpec(&m, sel.treeVersion.get(), false);
                        // L18
                        new_spec.title.set(sel.title.get());
                        // L19
                        let jewels = sel.jewels.with(Clone::clone);
                        new_spec.jewels.with_mut(|j| *j = jewels);
                        // L20
                        let state = sel.CreateUndoState();
                        new_spec.RestoreUndoState(&state, None);
                        flush_window_titles(cx);
                        // L21
                        new_spec.BuildClusterJewelGraphs();
                        // L22
                        Self::RenameSpec(cx, me, &new_spec, "Copy Tree", true);
                    },
                ),
            );
            // L24-26
            cx.base_mut(copy).enabled = Prop::f(move |cx, _| {
                // L25
                sel_value(cx, me).is_some()
            });
            // L27-29
            let delete = cx.add(
                me,
                "delete",
                ButtonControl::new(
                    Anchor::new(Left, copy, Right),
                    rect(4, 0, 60, 18),
                    "Delete",
                    move |cx| {
                        // L28
                        let (index, value) = {
                            let w = cx.w::<ListControl>(me);
                            (w.sel_index, w.sel_value.clone())
                        };
                        let value = value.unwrap_or_else(|| {
                            lua_error!("attempt to index local 'spec' (a nil value)")
                        });
                        Self::OnSelDelete(cx, me, index.unwrap_or(0), &value);
                    },
                ),
            );
            // L30-32
            cx.base_mut(delete).enabled = Prop::f(move |cx, _| {
                // L31
                sel_value(cx, me).is_some() && ListControl::list_len(cx, me) > 1
            });
            // L33-35
            let rename = cx.add(
                me,
                "rename",
                ButtonControl::new(
                    Anchor::new(BottomRight, me, Top),
                    rect(-2, -4, 60, 18),
                    "Rename",
                    move |cx| {
                        // L34
                        let sel = sel_value(cx, me).unwrap_or_else(|| {
                            lua_error!("attempt to index local 'spec' (a nil value)")
                        });
                        Self::RenameSpec(cx, me, &sel, "Rename Tree", false);
                    },
                ),
            );
            // L36-38
            cx.base_mut(rename).enabled = Prop::f(move |cx, _| {
                // L37
                sel_value(cx, me).is_some()
            });
            // L39-45
            cx.add(
                me,
                "new",
                ButtonControl::new(
                    Anchor::new(Right, rename, Left),
                    rect(-4, 0, 60, 18),
                    "New",
                    move |cx| {
                        let m = cx.build().model.clone();
                        let build_spec = m.spec.get();
                        // L40
                        let new_spec = new_PassiveSpec(
                            &m,
                            model::data().gameVersions.latestTreeVersion,
                            false,
                        );
                        // L41
                        new_spec.SelectClass(build_spec.curClassId.get());
                        // L42
                        new_spec.SelectAscendClass(build_spec.curAscendClassId.get());
                        // L43
                        new_spec
                            .SelectSecondaryAscendClass(build_spec.curSecondaryAscendClassId.get());
                        // L44
                        Self::RenameSpec(cx, me, &new_spec, "New Tree", true);
                    },
                ),
            );
            // L46
            Self::UpdateItemsTabPassiveTreeDropdown(cx, me);
            // L47 return self
        })
    }

    /// lua: PassiveSpecListControl.lua:50-74 `PassiveSpecListClass:RenameSpec(spec, title,
    /// addOnName)`
    /// @pob Classes/PassiveSpecListControl.lua:50-74 h=0b772a22
    pub fn RenameSpec(
        cx: &mut Cx,
        me: CtrlId,
        spec: &PassiveSpecRef,
        title: &str,
        add_on_name: bool,
    ) {
        let spec_title = spec.title.get().map(|s| s.to_string());
        let spec = spec.clone();
        // L72 -- main:OpenPopup(370, 100, spec.title and "Rename" or "Set Name", controls, "save", "edit")
        // L73
        popups::open_popup(
            cx,
            370.0,
            100.0,
            title,
            move |cx, p| {
                let p = p.id();
                // L51 local controls = { }
                // L52
                cx.add(
                    p,
                    "label",
                    LabelControl::new(
                        Anchor::NONE,
                        rect(0, 20, 0, 16),
                        "^7Enter name for this passive tree:",
                    ),
                );
                // L53-55
                cx.add(
                    p,
                    "edit",
                    EditControl::new(
                        Anchor::NONE,
                        rect(0, 40, 350, 20),
                        EditArgs {
                            init: spec_title,
                            limit: Some(100),
                            change_func: Some(Rc::new(move |cx: &mut Cx, buf: &str| {
                                // L54
                                let save = cx.child_as::<ButtonControl>(p, "save");
                                let ok = pattern::find(buf, "%S", None, false).is_some();
                                cx.base_mut(save).enabled = Prop::Val(ok);
                            })),
                            ..EditArgs::default()
                        },
                    ),
                );
                // L56-67
                let save = cx.add(
                    p,
                    "save",
                    ButtonControl::new(Anchor::NONE, rect(-45, 70, 80, 20), "Save", move |cx| {
                        let edit = cx.child_as::<EditControl>(p, "edit");
                        // L57
                        let buf = cx.get(edit).buf.clone();
                        spec.title.set(Some(buf.as_str().into()));
                        // L58
                        let tree_tab = cx.w::<PassiveSpecListControl>(me).tree_tab;
                        cx.w_mut::<TreeTab>(tree_tab).core.mod_flag = true;
                        // L59
                        if add_on_name {
                            // L60
                            let n = ListControl::list_len(cx, me);
                            ListControl::list_insert(cx, me, n + 1, ListValue::of(spec.clone()));
                            // L61-62
                            let n = ListControl::list_len(cx, me);
                            let w = cx.w_mut::<ListControl>(me);
                            w.sel_index = Some(n);
                            w.sel_value = Some(ListValue::of(spec.clone()));
                        }
                        // L64
                        Self::UpdateItemsTabPassiveTreeDropdown(cx, me);
                        // L65
                        let f = init::hooks(cx).sync_loadouts;
                        f(cx);
                        // L66
                        popups::close_popup(cx);
                    }),
                );
                // L68
                cx.base_mut(save).enabled = Prop::Val(false);
                // L69-71
                cx.add(
                    p,
                    "cancel",
                    ButtonControl::new(Anchor::NONE, rect(45, 70, 80, 20), "Cancel", |cx| {
                        // L70
                        popups::close_popup(cx);
                    }),
                );
            },
            PopupOpts {
                enter: Some("save".into()),
                default: Some("edit".into()),
                ..PopupOpts::default()
            },
        );
    }

    /// lua: PassiveSpecListControl.lua:76-84 `PassiveSpecListClass:GetRowValue(column, index,
    /// spec)`
    /// @pob Classes/PassiveSpecListControl.lua:76-84 h=f097b42f
    fn GetRowValue(cx: &Cx, _me: CtrlId, column: usize, index: usize, v: &ListValue) -> String {
        // L77
        if column == 1 {
            let spec = spec_of(v);
            let gv = &model::data().gameVersions;
            // L78
            let (used, ..) = spec.CountAllocNodes();
            let tv = spec.treeVersion.get();
            // L79
            let prefix = if tv != gv.latestTreeVersion {
                let display = gv
                    .treeVersions
                    .get(&tv)
                    .map(|t| t.display.to_string())
                    .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
                format!("[{display}] ")
            } else {
                String::new()
            };
            // L80
            let title = spec
                .title
                .get()
                .map_or_else(|| "Default".to_owned(), |s| s.to_string());
            // L81
            let ascend = spec.curAscendClassName.get();
            let class = match ascend {
                Some(a) if &*a != "None" => a.to_string(),
                _ => spec
                    .curClassName
                    .get()
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| {
                        lua_error!("attempt to concatenate field 'curClassName' (a nil value)")
                    }),
            };
            // L82
            let current = if index as f64 == cx.build().model.treeTab.activeSpec.get() {
                "  ^9(Current)"
            } else {
                ""
            };
            return format!(
                "{prefix}{title} ({class}, {} points){current}",
                pob_lua::tostring_num(used)
            );
        }
        String::new()
    }

    /// lua: PassiveSpecListControl.lua:86-91 `PassiveSpecListClass:OnOrderChange()`
    /// @pob Classes/PassiveSpecListControl.lua:86-91 h=169bd81e
    fn OnOrderChange(cx: &mut Cx, me: CtrlId, _sel_index: usize, _sel_drag_index: usize) {
        // L87
        let a = index_of_build_spec(cx);
        cx.build().model.treeTab.activeSpec.set(a);
        // L88
        let tree_tab = cx.w::<PassiveSpecListControl>(me).tree_tab;
        cx.w_mut::<TreeTab>(tree_tab).core.mod_flag = true;
        // L89
        Self::UpdateItemsTabPassiveTreeDropdown(cx, me);
        // L90
        let f = init::hooks(cx).sync_loadouts;
        f(cx);
    }

    /// lua: PassiveSpecListControl.lua:93-97 `PassiveSpecListClass:OnSelClick(index, spec,
    /// doubleClick)`
    /// @pob Classes/PassiveSpecListControl.lua:93-97 h=13a087b8
    fn OnSelClick(
        cx: &mut Cx,
        me: CtrlId,
        index: usize,
        _v: &ListValue,
        double_click: bool,
    ) -> Option<bool> {
        // L94
        if double_click && index as f64 != cx.build().model.treeTab.activeSpec.get() {
            // L95
            let tree_tab = cx.w::<PassiveSpecListControl>(me).tree_tab;
            TreeTab::SetActiveSpec(cx, tree_tab, index as f64);
        }
        None
    }

    /// lua: PassiveSpecListControl.lua:99-115 `PassiveSpecListClass:OnSelDelete(index, spec)`
    /// @pob Classes/PassiveSpecListControl.lua:99-115 h=cd4ad588
    fn OnSelDelete(cx: &mut Cx, me: CtrlId, index: usize, v: &ListValue) {
        let spec = spec_of(v);
        // L100
        if ListControl::list_len(cx, me) > 1 {
            let title = spec
                .title
                .get()
                .map_or_else(|| "Default".to_owned(), |s| s.to_string());
            // L101-113
            popups::open_confirm_popup(
                cx,
                "Delete Tree",
                &format!("Are you sure you want to delete '{title}'?"),
                "Delete",
                move |cx| {
                    // L102
                    ListControl::list_remove(cx, me, index);
                    // L103-104
                    {
                        let w = cx.w_mut::<ListControl>(me);
                        w.sel_index = None;
                        w.sel_value = None;
                    }
                    let tree_tab = cx.w::<PassiveSpecListControl>(me).tree_tab;
                    // L105
                    if index as f64 == cx.build().model.treeTab.activeSpec.get() {
                        // L106
                        TreeTab::SetActiveSpec(cx, tree_tab, m_max(1.0, index as f64 - 1.0));
                    } else {
                        // L108
                        let a = index_of_build_spec(cx);
                        cx.build().model.treeTab.activeSpec.set(a);
                    }
                    // L110
                    cx.w_mut::<TreeTab>(tree_tab).core.mod_flag = true;
                    // L111
                    Self::UpdateItemsTabPassiveTreeDropdown(cx, me);
                    // L112
                    let f = init::hooks(cx).sync_loadouts;
                    f(cx);
                },
                None,
            );
        }
    }

    /// lua: PassiveSpecListControl.lua:117-121 `PassiveSpecListClass:OnSelKeyDown(index, spec,
    /// key)`
    /// @pob Classes/PassiveSpecListControl.lua:117-121 h=648462c6
    fn OnSelKeyDown(cx: &mut Cx, me: CtrlId, _index: usize, v: &ListValue, key: Key) {
        // L118
        if key == Key::F(2) {
            // L119
            Self::RenameSpec(cx, me, &spec_of(v), "Rename Tree", false);
        }
    }

    /// lua: PassiveSpecListControl.lua:123-132 `PassiveSpecListClass:UpdateItemsTabPassiveTreeDropdown()`
    /// Update the passive tree dropdown control in itemsTab
    /// @pob Classes/PassiveSpecListControl.lua:123-132 h=9d8804b8
    pub fn UpdateItemsTabPassiveTreeDropdown(cx: &mut Cx, _me: CtrlId) {
        // L125 (DIVERGENCE(items-spec-select))
        let Some(secondary_spec_list) = items_spec_select(cx) else {
            return;
        };
        // L126-129
        let new_spec_list: Vec<DropItem> = cx
            .build()
            .model
            .treeTab
            .specList
            .snapshot()
            .iter()
            .map(|s| {
                DropItem::str(
                    s.title
                        .get()
                        .map_or_else(|| "Default".to_owned(), |t| t.to_string()),
                )
            })
            .collect();
        // L130
        DropDownControl::set_list(cx, secondary_spec_list, new_spec_list);
        // L131
        let a = cx.build().model.treeTab.activeSpec.get();
        cx.w_mut::<DropDownControl>(secondary_spec_list).sel_index = a as usize;
    }
}

impl Widget for PassiveSpecListControl {
    const NAME: &'static str = "PassiveSpecListControl";
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

    pob_ui_core::impl_parts!(PassiveSpecListControl: list => ListControl);
}
