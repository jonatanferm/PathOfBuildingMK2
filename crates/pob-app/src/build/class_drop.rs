//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Modules/Build.lua:771-1617 (UpdateSecondaryAscendancyDropdown, class dropdowns).
//! Owner: U20.
//!
//! @pob-file Modules/Build.lua @16de4b82d
//!
//! `main.tree[treeVersion]` (Build.lua:1554) is read through [`main_tree`]: PoB indexes the
//! `main.tree` cache directly (no load, no `data.setJewelRadiiGlobally`), and pob-calc's cache
//! (`passive_spec::main_LoadTree`) has no peek that leaves the jewel radii alone (CCR-0104 §4).
//! The trees UpdateClassDropdowns is called with are the latest tree (`build.latestTree`, loaded
//! by Init) and the tree of a spec of the build, so those are used; any other version falls back
//! to `main_LoadTree`.
//!
//! ORDER: `pairs(classes)` (L1557) and `pairs(altAscendancies)` (L1136) feed lists that are
//! sorted by label right after (`table.sort`, L1146 / L1573); the labels are unique, so the
//! order of the loop does not matter (ui doc §8 rule 6 (c)).
//!
//! The `secondaryAscendDrop*` fields of the buildMode object live in `on_frame::State` (the
//! split struct has one field per unit file group, ui doc §12.1).

use std::rc::Rc;

use pob_lua::n;
use pob_ui_core::controls::{DropDownControl, DropItem};
use pob_ui_core::prelude::*;

use super::init::{AscendListValue, ClassListValue};
use super::BuildMode;
use crate::app::CxApp;
use crate::model;

/// `local legacyAlternateAscendancyIds = { Warden = true, Warlock = true, Primalist = true }`
fn is_legacy_alternate_ascendancy(id: &str) -> bool {
    matches!(id, "Warden" | "Warlock" | "Primalist")
}

/// `main.tree[treeVersion]` (see the module doc; CCR-0104 §4).
pub fn main_tree(cx: &Cx, tree_version: model::Name) -> Option<model::PassiveTreeRef> {
    let m = &cx.build().model;
    if tree_version == m.data.gameVersions.latestTreeVersion {
        if let Some(t) = m.latestTree.get() {
            return Some(t);
        }
    }
    let spec = m.spec.get();
    if spec.treeVersion.get() == tree_version {
        return Some(spec.tree.get());
    }
    for s in m.treeTab.specList.with(Clone::clone) {
        if s.treeVersion.get() == tree_version {
            return Some(s.tree.get());
        }
    }
    // CCR-0104 §4
    pob_calc::classes::passive_spec::main_LoadTree(tree_version)
}

/// lua: Build.lua:1113-1158 `buildMode:UpdateSecondaryAscendancyDropdown(forceListUpdate)`
/// @pob Modules/Build.lua:1113-1158 h=b22b4701
pub fn update_secondary_ascendancy_dropdown(cx: &mut Cx, force_list_update: bool) {
    let me = cx.build().node.id();
    // L1114
    let Some(secondary_drop) = cx.child(me, "secondaryAscendDrop") else {
        // L1116
        return;
    };
    // L1118-1122 legacyAlternateAscendancyIds: see is_legacy_alternate_ascendancy
    // L1123 (the build always has a spec; a nil curSecondaryAscendClassId is NaN)
    let spec = cx.build().model.spec.get();
    let selection = {
        let v = spec.curSecondaryAscendClassId.get();
        if v.is_nan() {
            0.0
        } else {
            v
        }
    };
    // L1124
    let alt_ascendancies = spec.tree.get().alternate_ascendancies.get();
    // L1125 (a non-integer selection is never a key of the table)
    let selected_ascendancy = alt_ascendancies.as_ref().and_then(|t| {
        if selection.fract() == 0.0 {
            Some(t.get(selection)).filter(Val::truthy)
        } else {
            None
        }
    });
    // L1126
    let selected_legacy_ascendancy = selected_ascendancy
        .as_ref()
        .and_then(|a| a.get(n!("id")).str())
        .filter(|id| is_legacy_alternate_ascendancy(id))
        .map(|id| id.to_string());
    // L1127-1129
    let changed = {
        let st = &cx.w::<BuildMode>(me).on_frame;
        let same_alt = match (
            &st.secondary_ascend_drop_alt_ascendancies,
            &alt_ascendancies,
        ) {
            (None, None) => true,
            (Some(a), Some(b)) => a.ptr_eq(b),
            _ => false,
        };
        !same_alt || st.secondary_ascend_drop_legacy_selection != selected_legacy_ascendancy
    };
    if force_list_update || changed {
        // L1131-1133
        let mut entries = vec![ascend_item("None", 0.0)];
        // L1134
        if let Some(alt) = &alt_ascendancies {
            // L1135
            let mut sortable: Vec<DropItem> = Vec::new();
            // L1136 ORDER: sorted below (module doc)
            for (ascend_class_id, ascend_class) in alt.pairs() {
                let ascend_class_id = ascend_class_id.to_val().num();
                // L1137
                let id = if ascend_class.truthy() {
                    ascend_class.get(n!("id"))
                } else {
                    Val::Nil
                };
                if id.truthy() {
                    // L1138 (`legacyAlternateAscendancyIds[id]` for a non-string id is nil)
                    let legacy = id.str().is_some_and(|s| is_legacy_alternate_ascendancy(&s));
                    if !legacy || ascend_class_id == selection {
                        // L1139
                        let label = ascend_class.get(n!("name")).str().map_or_else(
                            || lua_error!("attempt to compare nil with string"),
                            |s| s.to_string(),
                        );
                        sortable.push(ascend_item(&label, ascend_class_id));
                    }
                }
            }
            // L1143-1145
            pob_lua::sort(&mut sortable, |a, b| a.label < b.label);
            // L1146
            for entry in sortable {
                // L1147
                entries.push(entry);
            }
        }
        let count = entries.len() as f64;
        // L1150
        DropDownControl::set_list(cx, secondary_drop, entries);
        // L1151
        DropDownControl::check_dropped_width(cx, secondary_drop, true);
        let st = &mut cx.w_mut::<BuildMode>(me).on_frame;
        // L1152
        st.secondary_ascend_drop_entry_count = Some(count);
        // L1153
        st.secondary_ascend_drop_alt_ascendancies = alt_ascendancies;
        // L1154
        st.secondary_ascend_drop_legacy_selection = selected_legacy_ascendancy;
    }
    // L1156
    DropDownControl::sel_by_value(cx, secondary_drop, &|it: &DropItem| {
        it.get::<AscendListValue>()
            .is_some_and(|v| v.ascend_class_id == selection)
    });
    // L1157 (`self.spec ~= nil` is always true)
    let count = cx
        .w::<BuildMode>(me)
        .on_frame
        .secondary_ascend_drop_entry_count
        .unwrap_or(1.0);
    cx.base_mut(secondary_drop).enabled = Prop::Val(count > 1.0);
}

/// `{ label = label, ascendClassId = id }`
fn ascend_item(label: &str, ascend_class_id: f64) -> DropItem {
    DropItem::new(
        label,
        Rc::new(AscendListValue {
            ascend_class_id,
            extra: None,
        }),
    )
}

/// lua: Build.lua:1553-1574 `buildMode:UpdateClassDropdowns(treeVersion)`
/// @pob Modules/Build.lua:1553-1574 h=fb549f34
pub fn update_class_dropdowns(cx: &mut Cx, tree_version: Option<model::Name>) {
    let me = cx.build().node.id();
    // L1554
    let tv = tree_version.unwrap_or(cx.build().model.data.gameVersions.latestTreeVersion);
    let tree =
        main_tree(cx, tv).unwrap_or_else(|| lua_error!("attempt to index a nil value (field '?')"));
    let classes = tree.classes.clone();
    let class_drop = cx
        .child(me, "classDrop")
        .unwrap_or_else(|| lua_error!("attempt to index field 'classDrop' (a nil value)"));
    // L1555 wipeTable(self.controls.classDrop.list)
    let mut list: Vec<DropItem> = Vec::new();
    // Initialise class dropdown
    // L1557 ORDER: sorted below (module doc)
    for (class_id, class) in classes.pairs() {
        let class_id = class_id.to_val().num();
        // L1558
        let mut ascendancies: Vec<DropItem> = Vec::new();
        // Initialise ascendancy dropdown
        let class_classes = class.get(n!("classes"));
        let n_asc = class_classes.table().map_or_else(
            || lua_error!("attempt to get length of field 'classes' (a nil value)"),
            |t| t.len(),
        );
        // L1560
        for i in 0..=n_asc {
            // L1561
            let ascend_class = class_classes.get(i as f64);
            // L1562-1565 (`ascendClass.name` on a nil entry raises, as in Lua)
            if !ascend_class.truthy() {
                lua_error!("attempt to index local 'ascendClass' (a nil value)");
            }
            // DIVERGENCE(nil-label): a nil `ascendClass.name` is a nil label in Lua; `DropItem`
            // labels are strings, so it reads "" here (real tree data always names them).
            let label = ascend_class.get(n!("name")).str().map(|s| s.to_string());
            ascendancies.push(DropItem::new(
                label.unwrap_or_default(),
                Rc::new(AscendListValue {
                    ascend_class_id: i as f64,
                    extra: None,
                }),
            ));
        }
        // L1567-1571
        let label = class.get(n!("name")).str().map_or_else(
            || lua_error!("attempt to compare nil with string"),
            |s| s.to_string(),
        );
        list.push(DropItem::new(
            label,
            Rc::new(ClassListValue {
                class_id,
                ascendancies,
            }),
        ));
    }
    // L1573
    pob_lua::sort(&mut list, |a, b| a.label < b.label);
    cx.w_mut::<DropDownControl>(class_drop).list = list;
}

use model::Val;
