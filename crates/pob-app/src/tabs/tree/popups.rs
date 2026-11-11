//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/TreeTab.lua:1-1046 (spec manage, version convert, import/export, ModifyNodePopup).
//! Owner: U25.
//!
//! @pob-file Classes/TreeTab.lua @16de4b82d
//!
//! The TreeTab popups: OpenSpecManagePopup, OpenVersionConvertPopup, OpenVersionConvertAllPopup,
//! OpenImportPopup, OpenExportPopup, ModifyNodePopup, OpenMasteryPopup (TreeTab.lua:635-999,
//! 1020-1045). The popup's `controls` table is the popup node's children (ui doc §6.10); integer
//! keys (`controls[i]`, the array popups) are `CtrlKey::Idx`.
//!
//! Lua semantics:
//! * `modGroups` (ModifyNodePopup) is one table that the `modSelect` dropdown aliases as its
//!   list: `buildMods` refills it in place, so the dropdown shows the new groups without a
//!   `SetList` (DropDownControl DIVERGENCE(alias): the port assigns the dropdown's list after
//!   each `buildMods`).
//! * `nodeName:match(node.targetType:gsub("^Small ", ""))` passes gsub's two results to `match`
//!   (pattern, init = the replacement count 0 or 1): both inits start at the first byte.
//! * `#node.stats` of a tattoo node is the border of a string-keyed table (0).
//! * The shared `treeLink` upvalue of OpenExportPopup is an `Rc<RefCell<String>>`.
//!
//! DIVERGENCE(tattoo-alias): `addModifier` stores the shared `tree.tattoo.nodes[...]` table
//! itself in `hashOverrides` after writing its `id` (L886-889), so two nodes given the same
//! tattoo share one table whose `id` is the last node's. `Node::id` is not mutable in the model,
//! so the port stores a copy carrying the node's id (as PassiveSpec:Load does, PassiveSpec.lua:181
//! `copyTable(..., true)`); the shared tattoo data node is unchanged. CCR-0123 §3.
//! DIVERGENCE(poeurl-resolve): the `poeurl.com` import branch runs a curl sub-script that only
//! reads the redirect URL (`LaunchSubScript`, L769-784). The platform has no such request
//! (`Sys::net` downloads pages), so `LaunchSubScript` returns no id here and, as in Lua when it
//! returns nil, nothing follows "Resolving PoEURL link..." (CCR-0123 §4).
//!
//! ORDER:
//! * `pairs(self.build.spec.tree.tattoo.nodes)` (L856, string keys): the groups are sorted by
//!   label afterwards (L883); labels are unique (each holds the tattoo's name and stats), so the
//!   result does not depend on the order (c).
//! * `pairs(node.stats)` (L862): only decides whether some stat id starts with `local_display`
//!   (c).
//! * `pairs(self.build.spec.hashOverrides)` (L962): integer keys, LjIntMap order (a).
//! * `pairs(tattooSdMap)` (L980, string keys): the tattoo-count tooltip's lines are in PoB's
//!   per-process hash order; the port uses first-seen order (b: a scenario hovering that button
//!   needs a multiset bucket for the tooltip layer).
//! * `pairs(node.masteryEffects)` (L1027): a sequence, visited in index order (c).
//! * `pairs(self.controls)` in PopupDialog (anchor resolution) is order-free (ui doc §11.2).
//!
//! Props with side effects (ui doc §7.3): none.

#![allow(non_snake_case, clippy::too_many_lines)]

use std::cell::RefCell;
use std::rc::Rc;

use pob_lua::{n, pattern};
use pob_ui_core::controls::drop_down::DropTipArgs;
use pob_ui_core::controls::{
    ButtonControl, CheckBoxControl, DropDownControl, DropItem, EditArgs, EditControl,
    LabelControl,
};
use pob_ui_core::popups::{self, PopupOpts};
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::Tooltip;

use super::core::SPEC_UNDO;
use super::mastery::{MasteryEffect, PassiveMasteryControl};
use super::spec_list::PassiveSpecListControl;
use super::TreeTab;
use crate::app::CxApp;
use crate::model::{self, Name, NodeRef, Val};
// not re-exported by model.rs (U15; CCR-0115)
use pob_calc::classes::passive_spec::{copy_node, new_PassiveSpec}; // CCR-0123 §9

fn tree_tab_mut(cx: &mut Cx, me: CtrlId) -> &mut super::core::State {
    &mut cx.w_mut::<TreeTab>(me).core
}

/// `treeVersions[v].display`
fn display(v: &str) -> String {
    model::data()
        .gameVersions
        .treeVersions
        .get(&Name::from(v))
        .map(|t| t.display.to_string())
        .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"))
}

/// `controls.<key>` of a popup
fn pc(cx: &Cx, p: CtrlId, key: &'static str) -> CtrlId {
    cx.child(p, key)
        .unwrap_or_else(|| lua_error!("attempt to index field '{key}' (a nil value)"))
}

/// `ConPrintf(s)` / `print(s)`
fn con_print(cx: &mut Cx, s: &str) {
    cx.sys.platform.con_print(s);
}

/// `s:match("%S")` truthiness
fn has_non_space(s: &str) -> bool {
    pattern::find(s, "%S", None, false).is_some()
}

impl TreeTab {
    /// lua: TreeTab.lua:635-653 `TreeTabClass:OpenSpecManagePopup()`
    /// @pob Classes/TreeTab.lua:635-653 h=5790ffb2
    pub fn OpenSpecManagePopup(cx: &mut Cx, me: CtrlId) {
        // L645-652
        popups::open_popup(
            cx,
            370.0,
            290.0,
            "Manage Passive Trees",
            move |cx, p| {
                let p = p.id();
                // L636-639
                let import_tree = cx
                    .add(
                        p,
                        CtrlKey::Idx(2),
                        ButtonControl::new(Anchor::NONE, rect(-99, 259, 90, 20), "Import Tree", move |cx| {
                            // L638
                            Self::OpenImportPopup(cx, me);
                        }),
                    )
                    .id();
                // L640-643
                let export_tree = cx
                    .add(
                        p,
                        CtrlKey::Idx(3),
                        ButtonControl::new(
                            Anchor::new(Left, import_tree, Right),
                            rect(8, 0, 90, 20),
                            "Export Tree",
                            move |cx| {
                                // L642
                                Self::OpenExportPopup(cx, me);
                            },
                        ),
                    )
                    .id();
                // L646
                cx.add(
                    p,
                    CtrlKey::Idx(1),
                    PassiveSpecListControl::new(Anchor::NONE, rect(0, 50, 350, 200), me),
                );
                // L649-651
                cx.add(
                    p,
                    CtrlKey::Idx(4),
                    ButtonControl::new(Anchor::new(Left, export_tree, Right), rect(8, 0, 90, 20), "Done", |cx| {
                        // L650
                        popups::close_popup(cx);
                    }),
                );
            },
            PopupOpts::default(),
        );
    }

    /// lua: TreeTab.lua:655-672 `TreeTabClass:OpenVersionConvertPopup(version, ignoreTreeSubType)`
    /// @pob Classes/TreeTab.lua:655-672 h=5244437b
    pub fn OpenVersionConvertPopup(cx: &mut Cx, me: CtrlId, version: Name, ignore_tree_sub_type: bool) {
        // L671 (the title is evaluated after the constructors, which draw nothing)
        let title = format!("Convert to Version {}", display(&version));
        popups::open_popup(
            cx,
            570.0,
            140.0,
            title,
            move |cx, p| {
                let p = p.id();
                // L656 local controls = { }
                // L657-658
                cx.add(
                    p,
                    "warningLabel",
                    LabelControl::new(
                        Anchor::NONE,
                        rect(0, 20, 0, 16),
                        "^7Warning: some or all of the passives may be de-allocated due to changes in the tree.\n\nConvert will replace your current tree.\nCopy + Convert will backup your current tree.\n",
                    ),
                );
                // L659-662
                cx.add(
                    p,
                    "convert",
                    ButtonControl::new(Anchor::NONE, rect(-125, 105, 100, 20), "Convert", move |cx| {
                        // L660
                        Self::ConvertToVersion(cx, me, version, true, false, ignore_tree_sub_type);
                        // L661
                        popups::close_popup(cx);
                    }),
                );
                // L663-666
                cx.add(
                    p,
                    "convertCopy",
                    ButtonControl::new(Anchor::NONE, rect(0, 105, 125, 20), "Copy + Convert", move |cx| {
                        // L664
                        Self::ConvertToVersion(cx, me, version, false, false, ignore_tree_sub_type);
                        // L665
                        popups::close_popup(cx);
                    }),
                );
                // L667-670
                cx.add(
                    p,
                    "cancel",
                    ButtonControl::new(Anchor::NONE, rect(125, 105, 100, 20), "Cancel", move |cx| {
                        // L668
                        let vs = super::core::ctl(cx, me, "versionSelect");
                        let tv = cx.build().model.spec.get().treeVersion.get();
                        Self::sel_version(cx, vs, &tv);
                        // L669
                        popups::close_popup(cx);
                    }),
                );
            },
            PopupOpts {
                enter: Some("convert".into()),
                default: Some("edit".into()),
                ..PopupOpts::default()
            },
        );
    }

    /// lua: TreeTab.lua:674-686 `TreeTabClass:OpenVersionConvertAllPopup(version)`
    /// @pob Classes/TreeTab.lua:674-686 h=f0177c9a
    pub fn OpenVersionConvertAllPopup(cx: &mut Cx, me: CtrlId, version: Name) {
        // L676-677 (the label) / L685 (the title)
        let shown = display(&version);
        let label = format!(
            "^7Warning: some or all of the passives may be de-allocated due to changes in the tree.\n\nConvert will replace all trees that are not Version {shown}.\nThis action cannot be undone.\n"
        );
        let title = format!("Convert all to Version {shown}");
        popups::open_popup(
            cx,
            570.0,
            140.0,
            title,
            move |cx, p| {
                let p = p.id();
                // L675 local controls = { }
                // L676
                cx.add(p, "warningLabel", LabelControl::new(Anchor::NONE, rect(0, 20, 0, 16), label));
                // L678-681
                cx.add(
                    p,
                    "convert",
                    ButtonControl::new(Anchor::NONE, rect(-58, 105, 100, 20), "Convert", move |cx| {
                        // L679
                        Self::ConvertAllToVersion(cx, me, version);
                        // L680
                        popups::close_popup(cx);
                    }),
                );
                // L682-684
                cx.add(
                    p,
                    "cancel",
                    ButtonControl::new(Anchor::NONE, rect(58, 105, 100, 20), "Cancel", |cx| {
                        // L683
                        popups::close_popup(cx);
                    }),
                );
            },
            PopupOpts {
                enter: Some("convert".into()),
                default: Some("edit".into()),
                ..PopupOpts::default()
            },
        );
    }

    /// lua: TreeTab.lua:688-816 `TreeTabClass:OpenImportPopup()`
    /// @pob Classes/TreeTab.lua:688-816 h=cd1d9c72
    pub fn OpenImportPopup(cx: &mut Cx, me: CtrlId) {
        // L689
        let version_lookup = "tree/([0-9]+)%.([0-9]+)%.([0-9]+)/";
        // L815
        popups::open_popup(
            cx,
            580.0,
            115.0,
            "Import Tree",
            move |cx, p| {
                let p = p.id();
                // L690 local controls = { }
                // L749
                cx.add(
                    p,
                    "nameLabel",
                    LabelControl::new(Anchor::NONE, rect(-180, 20, 0, 16), "Enter name for this passive tree:"),
                );
                // L750-753
                cx.add(
                    p,
                    "name",
                    EditControl::new(
                        Anchor::NONE,
                        rect(100, 20, 350, 18),
                        EditArgs {
                            init: Some(String::new()),
                            change_func: Some(Rc::new(move |cx: &mut Cx, buf: &str| {
                                // L751
                                let msg = pc(cx, p, "msg");
                                cx.w_mut::<LabelControl>(msg).label = Prop::Val(String::new());
                                // L752
                                let edit = pc(cx, p, "edit");
                                let ok = has_non_space(buf) && has_non_space(&cx.w::<EditControl>(edit).buf);
                                let import = pc(cx, p, "import");
                                cx.base_mut(import).enabled = Prop::Val(ok);
                            })),
                            ..EditArgs::default()
                        },
                    ),
                );
                // L754
                cx.add(
                    p,
                    "editLabel",
                    LabelControl::new(Anchor::NONE, rect(-150, 45, 0, 16), "Enter passive tree link:"),
                );
                // L755-758
                cx.add(
                    p,
                    "edit",
                    EditControl::new(
                        Anchor::NONE,
                        rect(100, 45, 350, 18),
                        EditArgs {
                            init: Some(String::new()),
                            change_func: Some(Rc::new(move |cx: &mut Cx, buf: &str| {
                                // L756
                                let msg = pc(cx, p, "msg");
                                cx.w_mut::<LabelControl>(msg).label = Prop::Val(String::new());
                                // L757
                                let name = pc(cx, p, "name");
                                let ok = has_non_space(buf) && has_non_space(&cx.w::<EditControl>(name).buf);
                                let import = pc(cx, p, "import");
                                cx.base_mut(import).enabled = Prop::Val(ok);
                            })),
                            ..EditArgs::default()
                        },
                    ),
                );
                // L759
                cx.add(p, "msg", LabelControl::new(Anchor::NONE, rect(0, 65, 0, 16), ""));
                // L760-810
                let import = cx
                    .add(
                        p,
                        "import",
                        ButtonControl::new(Anchor::NONE, rect(-45, 85, 80, 20), "Import", move |cx| {
                            Self::import_click(cx, me, p, version_lookup);
                        }),
                    )
                    .id();
                // L811
                cx.base_mut(import).enabled = Prop::Val(false);
                // L812-814
                cx.add(
                    p,
                    "cancel",
                    ButtonControl::new(Anchor::NONE, rect(45, 85, 80, 20), "Cancel", |cx| {
                        // L813
                        popups::close_popup(cx);
                    }),
                );
            },
            PopupOpts {
                enter: Some("import".into()),
                default: Some("name".into()),
                ..PopupOpts::default()
            },
        );
    }

    /// The import button's onClick (L760-810).
    fn import_click(cx: &mut Cx, me: CtrlId, p: CtrlId, version_lookup: &str) {
        let edit = pc(cx, p, "edit");
        // L761
        let tree_link = cx.w::<EditControl>(edit).buf.clone();
        // L762
        if tree_link.is_empty() {
            // L763
            return;
        }
        // EG: http://poeurl.com/dABz
        // L766
        if pattern::find(&tree_link, "poeurl%.com/", None, false).is_some() {
            // L767
            let import = pc(cx, p, "import");
            cx.base_mut(import).enabled = Prop::Val(false);
            // L768
            let msg = pc(cx, p, "msg");
            cx.w_mut::<LabelControl>(msg).label = Prop::Val("Resolving PoEURL link...".to_owned());
            // L769-784 LaunchSubScript(...): DIVERGENCE(poeurl-resolve), no id // CCR-0123 §4
            // L785-795 (`if id then`: not reached)
        } else if pattern::find(&tree_link, "poeplanner.com/", None, false).is_some() {
            // L797
            let link = pob_ui_core::luastr::gsub(&tree_link, "/%?v=.+#", "/");
            decode_poe_planner_tree_link(cx, me, p, &link);
        } else if pattern::find(&tree_link, "poeskilltree.com/", None, false).is_some() {
            // L799
            let old_style_version_lookup = "/%?v=([0-9]+)%.([0-9]+)%.([0-9]+)%-?%w?%-?%w?#";
            // Strip the version from the tree : https://poeskilltree.com/?v=3.6.0#AAAABAMAABEtfIOFMo6-ksHfsOvu -> https://poeskilltree.com/AAAABAMAABEtfIOFMo6-ksHfsOvu
            // L801
            let link = pob_ui_core::luastr::gsub(&tree_link, "/%?v=.+#", "/");
            let alt = pob_ui_core::luastr::lmatch(&tree_link, "%-(%l+%-?%l*)#");
            let (major, minor) = version_caps(&tree_link, old_style_version_lookup);
            let v = validate_tree_version(cx, alt.as_deref(), major.as_deref(), minor.as_deref());
            decode_tree_link(cx, me, p, &link, &v);
        } else {
            // EG: https://www.pathofexile.com/passive-skill-tree/3.15.0/AAAABgMADI6-HwKSwQQHLJwtH9-wTLNfKoP3ES3r5AAA
            // EG: https://www.pathofexile.com/fullscreen-passive-skill-tree/3.15.0/AAAABgMADAQHES0fAiycLR9Ms18qg_eOvpLB37Dr5AAA
            // EG: https://www.pathofexile.com/passive-skill-tree/ruthless/AAAABgAAAAAA (Ruthless doesn't have versions)
            // EG: https://www.pathofexile.com/passive-skill-tree/ruthless-alternate/AAAABgAAAAAA
            // EG: https://www.pathofexile.com/passive-skill-tree/alternate/AAAABgAAAAAA
            // L808
            let alt = pob_ui_core::luastr::lmatch(&tree_link, "tree/(%l+%-?%l*)");
            let (major, minor) = version_caps(&tree_link, version_lookup);
            let v = validate_tree_version(cx, alt.as_deref(), major.as_deref(), minor.as_deref());
            decode_tree_link(cx, me, p, &tree_link, &v);
        }
    }

    /// lua: TreeTab.lua:818-845 `TreeTabClass:OpenExportPopup()`
    /// @pob Classes/TreeTab.lua:818-845 h=fdfbae12
    pub fn OpenExportPopup(cx: &mut Cx, _me: CtrlId) {
        let spec = cx.build().model.spec.get();
        // L819
        let url = model::data()
            .gameVersions
            .treeVersions
            .get(&spec.treeVersion.get())
            .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"))
            .url;
        let tree_link = Rc::new(RefCell::new(spec.EncodeURL(url.as_deref())));
        // L820 local popup
        // L844
        popups::open_popup(
            cx,
            380.0,
            100.0,
            "Export Tree",
            move |cx, p| {
                let p = p.id();
                // L821 local controls = { }
                // L822
                cx.add(p, "label", LabelControl::new(Anchor::NONE, rect(0, 20, 0, 16), "Passive tree link:"));
                // L823
                let init = tree_link.borrow().clone();
                cx.add(
                    p,
                    "edit",
                    EditControl::new(
                        Anchor::NONE,
                        rect(0, 40, 350, 18),
                        EditArgs {
                            init: Some(init),
                            filter: Some("%Z".to_owned()),
                            ..EditArgs::default()
                        },
                    ),
                );
                // L824-837
                let tl = tree_link.clone();
                cx.add(
                    p,
                    "shrink",
                    ButtonControl::new(Anchor::NONE, rect(-90, 70, 140, 20), "Shrink with PoEURL", move |cx| {
                        let shrink = pc(cx, p, "shrink");
                        // L825
                        cx.base_mut(shrink).enabled = Prop::Val(false);
                        // L826
                        cx.w_mut::<ButtonControl>(shrink).label = Prop::Val("Shrinking...".to_owned());
                        // L827-836
                        let url = format!("http://poeurl.com/shrink.php?url={}", tl.borrow());
                        let tl2 = tl.clone();
                        crate::platform::net::download_page(
                            cx,
                            &url,
                            Rc::new(move |cx, response, err_msg| {
                                // L828
                                let shrink = pc(cx, p, "shrink");
                                cx.w_mut::<ButtonControl>(shrink).label = Prop::Val("Done".to_owned());
                                // L829
                                let body_ok = err_msg.is_none()
                                    && has_non_space(response.body.as_deref().unwrap_or_else(|| {
                                        lua_error!("attempt to index field 'body' (a nil value)")
                                    }));
                                if !body_ok {
                                    // L830
                                    popups::open_message_popup(
                                        cx,
                                        "PoEURL Shortener",
                                        "Failed to get PoEURL link. Try again later.",
                                    );
                                } else {
                                    // L832
                                    let link = format!("http://poeurl.com/{}", response.body.unwrap_or_default());
                                    *tl2.borrow_mut() = link.clone();
                                    // L833
                                    let edit = pc(cx, p, "edit");
                                    EditControl::set_text(cx, edit, &link, false);
                                    // L834
                                    cx.select_control(p, Some(edit));
                                }
                            }),
                            None,
                        );
                    }),
                );
                // L838-840
                let tl = tree_link.clone();
                cx.add(
                    p,
                    "copy",
                    ButtonControl::new(Anchor::NONE, rect(30, 70, 80, 20), "Copy", move |cx| {
                        // L839
                        let s = tl.borrow().clone();
                        cx.sys.platform.copy(&s);
                    }),
                );
                // L841-843
                cx.add(
                    p,
                    "done",
                    ButtonControl::new(Anchor::NONE, rect(120, 70, 80, 20), "Done", |cx| {
                        // L842
                        popups::close_popup(cx);
                    }),
                );
            },
            PopupOpts {
                enter: Some("done".into()),
                default: Some("edit".into()),
                ..PopupOpts::default()
            },
        );
    }

    /// lua: TreeTab.lua:847-999 `TreeTabClass:ModifyNodePopup(selectedNode)`
    /// @pob Classes/TreeTab.lua:847-999 h=e68457c1
    pub fn ModifyNodePopup(cx: &mut Cx, me: CtrlId, selected_node: &NodeRef) {
        let spec = cx.build().model.spec.get();
        // L848 local controls = { }
        // L849
        let mod_groups: Rc<RefCell<Vec<ModGroup>>> = Rc::new(RefCell::new(Vec::new()));
        // L850
        let tree = spec.tree.get();
        // L851
        let tree_node = tree
            .nodes
            .get(selected_node.id as i64)
            .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
        let node_name = tree_node.dn.get().to_string();

        // L923
        build_mods(cx, me, selected_node, &mod_groups);
        let default_tattoo = default_tattoo(cx, me, &node_name);
        let sel = selected_node.clone();
        let groups = mod_groups.clone();
        let name = node_name.clone();
        // L990
        popups::open_popup(
            cx,
            600.0,
            105.0,
            "Replace Modifier of Node",
            move |cx, p| {
                let p = p.id();
                // L924
                cx.add(
                    p,
                    "modSelectLabel",
                    LabelControl::new(Anchor::nil(TopRight, TopLeft), rect(170, 25, 0, 16), "^7Modifier:"),
                );
                // L925
                let g = groups.clone();
                let mod_select = cx
                    .add(
                        p,
                        "modSelect",
                        DropDownControl::new(
                            Anchor::nil(TopLeft, TopLeft),
                            rect(175, 25, 250, 18),
                            drop_items(&groups.borrow()),
                            Some(Rc::new(move |cx: &mut Cx, idx: usize, _v: Option<&DropItem>| {
                                let g = g.borrow().get(idx.wrapping_sub(1)).cloned();
                                construct_ui(cx, p, g.as_ref());
                            })),
                            None,
                            false,
                        ),
                    )
                    .id();
                // L926
                cx.w_mut::<DropDownControl>(mod_select).sel_index = default_tattoo.unwrap_or(1.0) as usize;
                // L927-934
                cx.w_mut::<DropDownControl>(mod_select).tt.tooltip_func = Some(Rc::new(
                    |cx: &mut Cx, tooltip: &mut Tooltip, (mode, _index, value): DropTipArgs| {
                        // L928
                        tooltip.clear(false);
                        // L929
                        if mode != pob_ui_core::controls::drop_down::DropTipMode::Out {
                            if let Some(group) = value.as_ref().and_then(|v| v.get::<ModGroup>()) {
                                // L930
                                for line in &group.descriptions {
                                    // L931
                                    tooltip.add_line(cx, 16.0, &format!("^7{line}"));
                                }
                            }
                        }
                    },
                ));
                // L935-942
                let (s1, g1, n1) = (sel.clone(), groups.clone(), name.clone());
                cx.add(
                    p,
                    "save",
                    ButtonControl::new(Anchor::NONE, rect(-90, 75, 80, 20), "Add", move |cx| {
                        // L936
                        add_modifier(cx, p, &s1, &g1);
                        // L937
                        SPEC_UNDO.add_undo_state(cx, me, false);
                        // L938
                        tree_tab_mut(cx, me).mod_flag = true;
                        // L939
                        cx.build().model.buildFlag.set(true);
                        // L940
                        let ms = pc(cx, p, "modSelect");
                        let idx = cx.w::<DropDownControl>(ms).sel_index as f64;
                        set_default_tattoo(cx, me, &n1, Some(idx));
                        // L941
                        popups::close_popup(cx);
                    }),
                );
                // L943-950
                let (s2, n2) = (sel.clone(), name.clone());
                cx.add(
                    p,
                    "reset",
                    ButtonControl::new(Anchor::NONE, rect(0, 75, 80, 20), "Reset Node", move |cx| {
                        // L944
                        Self::RemoveTattooFromNode(cx, me, &s2);
                        // L945
                        SPEC_UNDO.add_undo_state(cx, me, false);
                        // L946
                        tree_tab_mut(cx, me).mod_flag = true;
                        // L947
                        cx.build().model.buildFlag.set(true);
                        // L948
                        set_default_tattoo(cx, me, &n2, None);
                        // L949
                        popups::close_popup(cx);
                    }),
                );
                // L951-953
                cx.add(
                    p,
                    "close",
                    ButtonControl::new(Anchor::NONE, rect(90, 75, 80, 20), "Cancel", |cx| {
                        // L952
                        popups::close_popup(cx);
                    }),
                );
                // L988
                let count = get_tattoo_count(cx, None);
                let total = cx
                    .add(
                        p,
                        "totalTattoos",
                        ButtonControl::new(
                            Anchor::NONE,
                            rect(0, 95, 145, 20),
                            format!("^7Tattoo Count: {count}/50"),
                            |_cx| {},
                        ),
                    )
                    .id();
                // L989
                cx.w_mut::<ButtonControl>(total).tt.tooltip_func = Some(Rc::new(
                    |cx: &mut Cx, tooltip: &mut Tooltip, ()| {
                        get_tattoo_count(cx, Some(tooltip));
                    },
                ));
            },
            PopupOpts {
                enter: Some("save".into()),
                ..PopupOpts::default()
            },
        );
        let p = cx
            .main
            .popups
            .first()
            .map(|p| p.id())
            .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
        // L991
        let first = mod_groups
            .borrow()
            .get(default_tattoo.unwrap_or(1.0) as usize - 1)
            .cloned();
        construct_ui(cx, p, first.as_ref());

        // Show Legacy Tattoos
        // L994-997
        let total = pc(cx, p, "totalTattoos");
        let (sel, groups) = (selected_node.clone(), mod_groups.clone());
        let legacy = cx
            .add(
                p,
                "showLegacyTattoo",
                CheckBoxControl::new(
                    Anchor::new(Left, total, Right),
                    rect(195, 0, 20, 0),
                    Some("Show Legacy Tattoos:".to_owned()),
                    Some(Rc::new(move |cx: &mut Cx, state: bool| {
                        // L995
                        tree_tab_mut(cx, me).show_legacy_tattoo = Some(state);
                        // L996
                        build_mods(cx, me, &sel, &groups);
                        let ms = pc(cx, p, "modSelect");
                        cx.w_mut::<DropDownControl>(ms).list = drop_items(&groups.borrow());
                    })),
                    None,
                    false,
                ),
            )
            .id();
        // L998
        let show = cx.w::<TreeTab>(me).core.show_legacy_tattoo;
        cx.w_mut::<CheckBoxControl>(legacy).state = show.unwrap_or(false);
    }

    /// lua: TreeTab.lua:1020-1045 `TreeTabClass:OpenMasteryPopup(node, viewPort)`
    /// @pob Classes/TreeTab.lua:1020-1045 h=e43ae2c5
    pub fn OpenMasteryPopup(cx: &mut Cx, me: CtrlId, node: &NodeRef, _view_port: &ViewPort) {
        let spec = cx.build().model.spec.get();
        // L1021 local controls = { }
        // L1022
        let mut effects: Vec<MasteryEffect> = Vec::new();
        // L1023
        let cached_sd = node.sd.get();
        // L1024
        let cached_all_mastery_option = node.allMasteryOptions.get();

        // L1026 wipeTable(effects)
        // L1027 (ORDER: a sequence, module docs)
        let mastery_effects = node
            .masteryEffects
            .get()
            .unwrap_or_else(|| lua_error!("bad argument #1 to 'pairs' (table expected, got nil)"));
        for (_, effect) in mastery_effects.pairs() {
            let effect = effect.table().expect("mastery effect");
            let effect_id = effect.get(n!("effect"));
            // L1028 isValueInTable(self.build.spec.masterySelections, effect.effect)
            let assigned_node_id = spec.masterySelections.with(|s| {
                s.pairs()
                    .into_iter()
                    .find(|(_, v)| effect_id.opt_num() == Some(*v))
                    .map(|(k, _)| k as f64)
            });
            // L1029
            if assigned_node_id.map_or(true, |a| a == node.id) {
                // L1030
                let stats: Vec<String> = effect
                    .table(n!("stats"))
                    .map(|t| {
                        t.ipairs()
                            .into_iter()
                            .map(|(_, v)| {
                                v.str().map(|s| s.to_string()).unwrap_or_else(|| {
                                    lua_error!("invalid value (at index 1) in table for 'concat'")
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                effects.push(MasteryEffect {
                    label: stats.join(" / "),
                    id: effect_id.opt_num().unwrap_or(f64::NAN),
                });
            }
        }
        //Check to make sure that the effects list has a potential mod to apply to a mastery
        // L1034
        if effects.is_empty() {
            return;
        }
        // L1035
        let passive_mastery_control_height = (effects.len() as f64 + 1.0) * 14.0 + 2.0;
        // L1036-1041 / L1042: the controls are created inside the popup below; their sizes
        // (L1043) come from the effect list, as PassiveMasteryControl's constructor computes them
        let width = PassiveMasteryControl::auto_width(cx, &effects, 0.0);
        let node_c = node.clone();
        let title = node
            .extra
            .str(n!("name"))
            .map(|s| s.to_string())
            .unwrap_or_else(|| lua_error!("bad argument #3 to 'OpenPopup' (string expected, got nil)"));
        // L1043
        popups::open_popup(
            cx,
            width + 12.0,
            passive_mastery_control_height + 60.0,
            title,
            move |cx, p| {
                let p = p.id();
                let n1 = node_c.clone();
                let sd = cached_sd.clone();
                // L1036-1041
                cx.add(
                    p,
                    "close",
                    ButtonControl::new(
                        Anchor::NONE,
                        [0.0.into(), (30.0 + passive_mastery_control_height).into(), 90.0.into(), 20.0.into()],
                        "Cancel",
                        move |cx| {
                            // L1037
                            n1.sd.set(sd.clone());
                            // L1038
                            n1.allMasteryOptions.set(cached_all_mastery_option);
                            // L1039
                            let tree = cx.build().model.spec.get().tree.get();
                            tree.ProcessStats(&n1, None);
                            // L1040
                            popups::close_popup(cx);
                        },
                    ),
                );
                // L1042 (`controls.save` is nil)
                cx.add(
                    p,
                    "effect",
                    PassiveMasteryControl::new(
                        Anchor::nil(TopLeft, TopLeft),
                        [6.0.into(), 25.0.into(), 0.0.into(), passive_mastery_control_height.into()],
                        effects,
                        me,
                        node_c.clone(),
                    ),
                );
            },
            PopupOpts {
                escape: Some("close".into()),
                ..PopupOpts::default()
            },
        );
    }
}

/// `{ label, descriptions, id }` of `modGroups`
#[derive(Clone, Debug)]
pub struct ModGroup {
    pub label: String,
    pub descriptions: Vec<String>,
    /// the `tree.tattoo.nodes` key
    pub id: String,
}

fn drop_items(groups: &[ModGroup]) -> Vec<DropItem> {
    groups
        .iter()
        .map(|g| DropItem::new(g.label.clone(), Rc::new(g.clone())))
        .collect()
}

/// `self.defaultTattoo[nodeName]`
fn default_tattoo(cx: &Cx, me: CtrlId, node_name: &str) -> Option<f64> {
    cx.w::<TreeTab>(me)
        .core
        .default_tattoo
        .iter()
        .find(|(k, _)| k == node_name)
        .map(|(_, v)| *v)
}

/// `self.defaultTattoo[nodeName] = v`
fn set_default_tattoo(cx: &mut Cx, me: CtrlId, node_name: &str, v: Option<f64>) {
    let d = &mut tree_tab_mut(cx, me).default_tattoo;
    d.retain(|(k, _)| k != node_name);
    if let Some(v) = v {
        d.push((node_name.to_owned(), v));
    }
}

/// `tattoo.nodes` (string key, node)
fn tattoo_nodes(cx: &Cx) -> Vec<(String, NodeRef)> {
    let tree = cx.build().model.spec.get().tree.get();
    let tattoo = tree
        .tattoo
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index field 'tattoo' (a nil value)"));
    let nodes = tattoo
        .table(n!("nodes"))
        .unwrap_or_else(|| lua_error!("attempt to index field 'nodes' (a nil value)"));
    nodes
        .pairs()
        .filter_map(|(k, v)| {
            let key = match k {
                model::Key::S(s) => s.to_string(),
                other => format!("{other:?}"),
            };
            v.obj::<model::Node>().map(|n| (key, n))
        })
        .collect()
}

/// `table.concat(sd, sep)`
fn concat_sd(node: &model::Node, sep: &str) -> String {
    node.sd.get().with(|v| v.iter().map(|s| s.to_string()).collect::<Vec<_>>().join(sep))
}

/// `local function buildMods(selectedNode)` (L852-884)
fn build_mods(cx: &mut Cx, me: CtrlId, selected_node: &NodeRef, mod_groups: &Rc<RefCell<Vec<ModGroup>>>) {
    let spec = cx.build().model.spec.get();
    let tree = spec.tree.get();
    let tree_node = tree
        .nodes
        .get(selected_node.id as i64)
        .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
    let node_name = tree_node.dn.get().to_string();
    let show_legacy = cx.w::<TreeTab>(me).core.show_legacy_tattoo;
    // L853
    mod_groups.borrow_mut().clear();
    // L854
    let num_linked_nodes = selected_node.linkedId.get().len() as f64;
    // L855
    let node_value = tree_node
        .sd
        .get()
        .get(1)
        .map_or_else(String::new, |s| s.to_string());
    // L856 (ORDER: sorted below, module docs)
    for (id, node) in tattoo_nodes(cx) {
        let target_type = node.extra.str(n!("targetType")).map(|s| s.to_string()).unwrap_or_else(|| {
            lua_error!("attempt to index field 'targetType' (a nil value)")
        });
        let target_value = node.extra.str(n!("targetValue")).map(|s| s.to_string());
        let minimum_connected = node.extra.num(n!("MinimumConnected"));
        let legacy = node.extra.get(n!("legacy"));
        // L857 (`targetType:gsub("^Small ", "")` passes the count as `init`; module docs)
        let pat = pob_ui_core::luastr::gsub(&target_type, "^Small ", "");
        let m1 = pattern::find(&node_name, &pat, None, false).is_some();
        // `or` short-circuits: the nil-targetValue error is raised only when m1 is false
        let m2 = !m1
            && match &target_value {
                Some(tv) if !tv.is_empty() => pattern::find(&node_value, tv, None, false).is_some(),
                Some(_) => false,
                None => lua_error!("bad argument #1 to 'match' (string expected, got nil)"),
            };
        // L858
        let m3 = target_type == "Small Attribute"
            && (node_name == "Intelligence" || node_name == "Strength" || node_name == "Dexterity");
        // L859
        let m4 = target_type == "Keystone" && *tree_node.r#type.get() == *target_type;
        // L860
        let legacy_ok = match legacy {
            Val::Nil | Val::Bool(false) => true,
            Val::Bool(b) => show_legacy == Some(b),
            _ => false,
        };
        if (m1 || m2 || m3 || m4) && minimum_connected <= num_linked_nodes && legacy_ok {
            // L861
            let mut combine = false;
            // L862 (ORDER: module docs)
            let stats = node.extra.table(n!("stats"));
            if let Some(stats) = &stats {
                for (k, _desc) in stats.pairs() {
                    let model::Key::S(sid) = &k else { continue };
                    // L863
                    combine = (pattern::find(sid, "^local_display.*", None, false).is_some()
                        && stats.len() as f64 == (node.sd.get().len() as f64 - 1.0))
                        || combine;
                    // L864
                    if combine {
                        break;
                    }
                }
            }
            let sd: Vec<String> = node.sd.get().with(|v| v.iter().map(|s| s.to_string()).collect());
            // L866-871
            if combine {
                // L868-870: the copy's first two lines become one (`node.sd[2]` must exist)
                let second = sd.get(1).cloned().unwrap_or_else(|| {
                    lua_error!("attempt to concatenate field '?' (a nil value)")
                });
                let _ = format!("{} {}", sd[0], second);
            }
            // L872
            let mut descriptions_and_reminders: Vec<String> = if combine { vec![sd.join(" ")] } else { sd.clone() };
            // L873
            if let Some(rt) = node.extra.table(n!("reminderText")) {
                // L874
                if let Some(first) = rt.str(1.0) {
                    descriptions_and_reminders.push(first.to_string());
                }
            }
            // L876-880
            mod_groups.borrow_mut().push(ModGroup {
                label: format!(
                    "{}                                                {}",
                    node.dn.get(),
                    concat_sd(&node, ",")
                ),
                descriptions: descriptions_and_reminders,
                id,
            });
        }
    }
    // L883
    pob_lua::sort(&mut mod_groups.borrow_mut(), |a, b| a.label.as_bytes() < b.label.as_bytes());
}

/// `local function addModifier(selectedNode)` (L885-894; DIVERGENCE(tattoo-alias))
fn add_modifier(cx: &mut Cx, p: CtrlId, selected_node: &NodeRef, mod_groups: &Rc<RefCell<Vec<ModGroup>>>) {
    let spec = cx.build().model.spec.get();
    let ms = pc(cx, p, "modSelect");
    let sel = cx.w::<DropDownControl>(ms).sel_index;
    // L886
    let group = mod_groups
        .borrow()
        .get(sel.wrapping_sub(1))
        .cloned()
        .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
    let src = tattoo_nodes(cx)
        .into_iter()
        .find(|(k, _)| *k == group.id)
        .map(|(_, n)| n)
        .unwrap_or_else(|| lua_error!("attempt to index local 'newTattooNode' (a nil value)"));
    // L887 (DIVERGENCE(tattoo-alias)) // CCR-0123 §3
    let new_tattoo_node = Rc::new(model::Node {
        id: selected_node.id,
        ..copy_node(&src)
    });
    // L888
    spec.hashOverrides
        .with_mut(|h| h.set(selected_node.id as i64, Some(new_tattoo_node.clone())));
    // L889
    spec.ReplaceNode(selected_node, &new_tattoo_node);
    // L890
    if &*selected_node.r#type.get() == "Mastery" {
        // L891
        selected_node.allMasteryOptions.set(false);
    }
    // L893
    spec.BuildAllDependsAndPaths();
}

/// `local function constructUI(modGroup)` (L896-921)
fn construct_ui(cx: &mut Cx, p: CtrlId, mod_group: Option<&ModGroup>) {
    let mod_group =
        mod_group.unwrap_or_else(|| lua_error!("attempt to index local 'modGroup' (a nil value)"));
    // L897-898
    let mut total_height = 43.0;
    let max_width = 375.0;
    // L899-903
    let mut i = 1;
    while cx.child(p, CtrlKey::Idx(i)).is_some() {
        // L901
        cx.remove_child(p, &CtrlKey::Idx(i));
        // L902
        i += 1;
    }

    // L905
    let mut wrap_table: Vec<String> = Vec::new();
    // L906
    for desc in &mod_group.descriptions {
        // L907
        for wrapped_desc in pob_ui_core::wrap::wrap_string(cx, desc, 16.0, max_width) {
            // L908
            wrap_table.push(wrapped_desc);
        }
    }
    // L911
    for (idx0, desc) in wrap_table.iter().enumerate() {
        let idx = idx0 as i64 + 1;
        // L912
        let other = if idx > 1 {
            cx.child(p, CtrlKey::Idx(idx - 1))
        } else {
            None
        }
        .unwrap_or_else(|| pc(cx, p, "modSelect"));
        cx.add(
            p,
            CtrlKey::Idx(idx),
            LabelControl::new(Anchor::new(TopLeft, other, TopLeft), rect(0, 20, 600, 16), format!("^7{desc}")),
        );
        // L913
        total_height += 20.0;
    }
    // L915
    let popup = cx
        .main
        .popups
        .first()
        .map(|p| p.id())
        .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
    cx.base_mut(popup).height = Prop::Val(total_height + 75.0);
    // L916
    let button_height = total_height + 15.0;
    // L917-920
    for key in ["save", "reset", "close"] {
        let id = pc(cx, p, key);
        cx.base_mut(id).y = Prop::Val(button_height);
    }
    let total = pc(cx, p, "totalTattoos");
    cx.base_mut(total).y = Prop::Val(button_height + 30.0);
}

/// `local function getTattooCount(tooltip)` (L955-987): the count, or the coloured count
/// string when it is over 50.
fn get_tattoo_count(cx: &Cx, mut tooltip: Option<&mut Tooltip>) -> String {
    // L956
    if let Some(t) = tooltip.as_deref_mut() {
        // L957
        t.clear(false);
    }
    // L959
    let mut count = 0.0;
    // L960 (first-seen order; ORDER (b), module docs)
    let mut tattoo_sd_map: Vec<(String, f64)> = Vec::new();

    let spec = cx.build().model.spec.get();
    // L962
    for (_, node) in spec.hashOverrides.with(pob_lua::LjIntMap::pairs) {
        // L963
        if node.isTattoo.get() && pattern::find(&node.dn.get(), "Runegraft", None, false).is_none() {
            // L964
            if tooltip.is_some() {
                // L965
                let mut combined = String::new();
                // L966
                for line in node.sd.get().snapshot() {
                    // clean up some legacy tattoo lines
                    // L967
                    if !(pattern::find(&line, "Limited", None, false).is_some()
                        || pattern::find(&line, "Requires", None, false).is_some())
                    {
                        // L968
                        combined = format!("{combined} {line}");
                    }
                }
                // L971-975
                if let Some(e) = tattoo_sd_map.iter_mut().find(|(k, _)| *k == combined) {
                    e.1 += 1.0;
                } else {
                    tattoo_sd_map.push((combined, 1.0));
                }
            }
            // L977
            count += 1.0;
        }
    }
    // L980
    for (line, mult) in tattoo_sd_map {
        // L981
        let t = tooltip
            .as_deref_mut()
            .unwrap_or_else(|| lua_error!("attempt to index local 'tooltip' (a nil value)"));
        t.add_line(
            cx,
            16.0,
            &format!("{}({}) ^7{}", cx.main.cc.COLD, pob_lua::tostring_num(mult), line),
        );
    }
    // L983
    if count > 50.0 {
        // L984
        return format!("{}{}", cx.main.cc.NEGATIVE, pob_lua::tostring_num(count));
    }
    // L986
    pob_lua::tostring_num(count)
}

/// `treeLink:match(lookup)`'s first two captures (`major`, `minor`; nil when it does not match).
fn version_caps(s: &str, lookup: &str) -> (Option<String>, Option<String>) {
    match pob_lua::lmatch(s, lookup, None) {
        Some(c) if c.len() >= 2 => (
            Some(c[0].to_str().into_owned()),
            Some(c[1].to_str().into_owned()),
        ),
        _ => (None, None),
    }
}

/// `local function validateTreeVersion(alternateType, major, minor)` (L733-747)
fn validate_tree_version(
    cx: &mut Cx,
    alternate_type: Option<&str>,
    major: Option<&str>,
    minor: Option<&str>,
) -> String {
    let gv = &model::data().gameVersions;
    // Take the Major and Minor version numbers and confirm it is a valid tree version. The point release is also passed in but it is not used
    // Return: the passed in tree version as text or latestTreeVersion
    let alt = |a: Option<&str>| {
        a.map_or_else(String::new, |a| format!("_{}", pob_ui_core::luastr::gsub(a, "-", "_")))
    };
    // L736
    if let (Some(major), Some(minor)) = (major, minor) {
        //need leading 0 here
        // L738 (`%d` coerces the numeric capture strings)
        let (maj, min) = (
            pob_lua::tonumber(major).unwrap_or(f64::NAN),
            pob_lua::tonumber(minor).unwrap_or(f64::NAN),
        );
        let new_tree_version_num = pob_lua::tonumber(&pob_lua::lfmt!("%d.%02d", maj, min));
        let num = |v: &Name| gv.treeVersions.get(v).map_or(f64::NAN, |t| t.num);
        // L739
        let n = new_tree_version_num.unwrap_or_else(|| {
            lua_error!("attempt to compare nil with number")
        });
        if n >= num(&gv.defaultTreeVersion) && n <= num(&gv.latestTreeVersion) {
            // no leading 0 here
            // L741
            return format!("{major}_{minor}{}", alt(alternate_type));
        }
        // L743
        let s = pob_lua::lfmt!("Version '%d_%02d' is out of bounds", maj, min);
        con_print(cx, &s);
    }
    // L746
    format!("{}{}", gv.latestTreeVersion, alt(alternate_type))
}

/// `local function decodePoePlannerTreeLink(treeLink)` (L691-712)
fn decode_poe_planner_tree_link(cx: &mut Cx, me: CtrlId, p: CtrlId, tree_link: &str) {
    let m = cx.build().model.clone();
    // treeVersion is not known at this point. We need to decode the URL to get it.
    // L693
    let tmp_spec = new_PassiveSpec(&m, model::data().gameVersions.latestTreeVersion, false);
    // L694
    let new_tree_version_or_err_msg = tmp_spec
        .DecodePoePlannerURL(tree_link, true)
        .unwrap_or_else(|| lua_error!("bad argument #1 to 'find' (string expected, got nil)"));
    // Check for an error message
    // L696
    if pattern::find(&new_tree_version_or_err_msg, "Invalid", None, false).is_some() {
        // L697
        let msg = pc(cx, p, "msg");
        cx.w_mut::<LabelControl>(msg).label = Prop::Val(format!("^1{new_tree_version_or_err_msg}"));
        // L698
        return;
    }

    // 20230908. We always create a new Spec()
    // L702
    let new_spec = new_PassiveSpec(&m, Name::from(new_tree_version_or_err_msg.as_str()), false);
    // L703
    let name = pc(cx, p, "name");
    let title = cx.w::<EditControl>(name).buf.clone();
    new_spec.title.set(Some(title.as_str().into()));
    // L704 DecodePoePlannerURL was used above and URL proven correct.
    new_spec.DecodePoePlannerURL(tree_link, false);
    // L705
    let n_specs = m.treeTab.specList.with_mut(|l| {
        l.push(new_spec);
        l.len()
    });
    // trigger all the things that go with changing a spec
    // L707
    TreeTab::SetActiveSpec(cx, me, n_specs as f64);
    // L708
    tree_tab_mut(cx, me).mod_flag = true;
    // L709
    SPEC_UNDO.add_undo_state(cx, me, false);
    // L710
    m.buildFlag.set(true);
    // L711
    popups::close_popup(cx);
}

/// `local function decodeTreeLink(treeLink, newTreeVersion)` (L714-732)
fn decode_tree_link(cx: &mut Cx, me: CtrlId, p: CtrlId, tree_link: &str, new_tree_version: &str) {
    let m = cx.build().model.clone();
    // newTreeVersion is passed in as an output of validateTreeVersion(). It will always be a valid tree version text string
    // 20230908. We always create a new Spec()
    // L717
    con_print(cx, &format!("Tree version: {new_tree_version}"));
    // L718
    let new_spec = new_PassiveSpec(&m, Name::from(new_tree_version), false);
    // L719
    let name = pc(cx, p, "name");
    let title = cx.w::<EditControl>(name).buf.clone();
    new_spec.title.set(Some(title.as_str().into()));
    // L720
    let err_msg = new_spec.DecodeURL(tree_link);
    // L721
    if let Some(err_msg) = err_msg {
        // L722
        let msg = pc(cx, p, "msg");
        cx.w_mut::<LabelControl>(msg).label = Prop::Val(format!("^1{err_msg}^7"));
    } else {
        // L724
        let n_specs = m.treeTab.specList.with_mut(|l| {
            l.push(new_spec);
            l.len()
        });
        // trigger all the things that go with changing a spec
        // L726
        TreeTab::SetActiveSpec(cx, me, n_specs as f64);
        // L727
        tree_tab_mut(cx, me).mod_flag = true;
        // L728
        SPEC_UNDO.add_undo_state(cx, me, false);
        // L729
        m.buildFlag.set(true);
        // L730
        popups::close_popup(cx);
    }
}
