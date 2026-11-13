//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/CompareTab.lua:1244-1645 (ImportBuild,
//! ImportFromCode, RemoveBuild, ReimportPrimary, UpdateBuildSelector, GetActiveCompare,
//! CopyCompareSpecToPrimary, GetJewelComparisonSlots, CopyCompareItemToPrimary,
//! OpenImportPopup, OpenImportFolderPopup with the `buildList` instance overrides
//! `:1607-1628`). Owner: U46. The state and the seams are `init.rs`'s.
//!
//! @pob-file Classes/CompareTab.lua @16de4b82d
//!
//! # The folder popup's list host
//!
//! `listHost` (L1540-1565) is a minimal stand-in for BuildList's `listMode`, consumed by
//! BuildListControl and its PathControl. U18's `BuildListControl` takes a `ListMode` node for
//! its `listMode` (the fields and methods it uses are `subPath`, `list` and `BuildList`), so the
//! popup creates its own `ListMode` node (a free node of the popup; not `main.modes.LIST`) as
//! the host: `listHost.subPath`/`list`/`buildIndex` are that node's `sub_path`/`list`/
//! `build_index`, and the host's methods are BuildList.lua's `listMode:BuildList`/
//! `FilterBuildList`/`SortList` (U18), which do what L1545-1562 do: ScanFolder of `subPath`;
//! FilterList of `buildIndex` by `subPath` and the search text (`searchText`, kept in the
//! node's `filter_build_list`); SortList by the sort mode, keeping the selection by
//! `fullFileName`. The popup's `sortMode` is set together with `main.buildSortMode` (L1537,
//! L1594-1595) and BuildList's SortList reads `main.buildSortMode`, the same value. The node's
//! `controls.buildList` is the popup's list (BuildList's SortList reads it there).
//! `listHost:SelectControl` (L1563-1565, a no-op) has no caller in the popup (BuildListControl
//! calls `listMode:SelectControl` only from rename/new-folder, which the overrides disable).
//!
//! The instance overrides (L1607-1628): `OnSelKeyDown`, `OnSelCopy`, `OnSelCut`, `OnSelDelete`
//! are ListControl's per-instance hook slots (`ListOverrides`); `OnHoverKeyUp` and
//! `CanReceiveDrag` (the list's and the path control's) are framework `Overrides`. `LoadBuild`
//! (L1607-1613) has no override slot on U18's BuildListControl (ui doc D6 names one;
//! CCR-0136 §3): the class's `OnSelClick` (BuildListControl.lua:259-265) calls `self:LoadBuild`,
//! so the popup overrides `OnSelClick` with those lines calling the instance's `LoadBuild`
//! ([`popup_load_build`]), marked CCR-0136 §3.
//!
//! # Lua semantics
//!
//! * `Inflate(common.base64.decode(x:gsub("-","+"):gsub("_","/")))` is U43's
//!   `import::core::decode_build_code` (base64 with gsub's count as an ignored second argument;
//!   `Inflate` nil unless the zlib stream ends).
//! * `buf:match(site.matchURL)` is `pob_lua::pattern::r#match`.
//! * `tostring(codeData)` of the download error: the callback's string.
//! * `io.open(build.fullFileName, "r")` + `read("*a")`: `cx.sys.fs.read` (an empty file reads
//!   as `""`). DIVERGENCE(bytes), as in `tabs::import::core`: the XML is kept as UTF-8
//!   (`from_utf8_lossy`).
//! * `table.sort(result, function(a, b) return a.nodeId < b.nodeId end)`: node ids are unique,
//!   so any sort gives PoB's order.
//!
//! ORDER: `pairs(pSpec.jewels)` / `pairs(cSpec.jewels)` / `pairs(nodeIds)` in
//! GetJewelComparisonSlots (L1385, L1392, L1400) build a set and a list that is sorted by the
//! unique node id afterwards (order-independent, (c)).

#![allow(non_snake_case)]

use std::cell::RefCell;
use std::rc::Rc;

use pob_lua::{n, Name};
use pob_ui_core::controls::list::ListControl;
use pob_ui_core::controls::path::PathControl;
use pob_ui_core::controls::scroll_bar::ScrollBarControl;
use pob_ui_core::controls::{
    ButtonControl, CheckBoxControl, DropDownControl, DropItem, EditArgs, EditControl, LabelControl,
};
use pob_ui_core::popups::{self, PopupOpts};
use pob_ui_core::prelude::*;

use super::init::{at_num, compare_hooks, ctl, st, st_mut, CompareEntryRef};
use super::CompareTab;
use crate::app::CxApp;
use crate::build::init as build_init;
use crate::build_list::build_list_control::{entry_of, BuildListControl};
use crate::build_list::helpers::{build_sort_drop_list, Entry, SortDropItem};
use crate::build_list::ListMode;
use crate::model::{self, ItemRef, Val};
use crate::tabs::import::build_sites;
use crate::tabs::import::core::decode_build_code;
// not re-exported by model.rs (U15; CCR-0115, CCR-0123 §9)
use pob_calc::classes::item::new_Item;
use pob_calc::classes::passive_spec::new_PassiveSpec;

/// One `GetJewelComparisonSlots` record (L1425-1434).
#[derive(Clone)]
pub struct JewelComparisonSlot {
    pub label: String,
    pub nodeId: i64,
    pub pItem: Option<ItemRef>,
    pub cItem: Option<ItemRef>,
    pub pSlotName: String,
    pub cSlotName: String,
    pub pNodeAllocated: bool,
    pub cNodeAllocated: bool,
}

/// `controls.<key>` of a popup
fn pc(cx: &Cx, p: CtrlId, key: &'static str) -> CtrlId {
    cx.child(p, key)
        .unwrap_or_else(|| lua_error!("attempt to index field '{key}' (a nil value)"))
}

/// `items[id]` of a build's ItemsTab (`id` a Lua value: nil/false and non-numbers give nil).
fn item_at(build: &model::BuildRef, id: &Val) -> Option<ItemRef> {
    let id = id.opt_num()?;
    if id.fract() != 0.0 || id < 0.0 || id > f64::from(u32::MAX) {
        return None;
    }
    build.itemsTab.items.with(|m| m.get(&(id as u32)).cloned())
}

impl CompareTab {
    /// lua: CompareTab.lua:1244-1258 `CompareTabClass:ImportBuild(xmlText, label)`
    /// Import a comparison build from XML text
    /// @pob Classes/CompareTab.lua:1244-1258 h=18757cca
    pub fn ImportBuild(
        cx: &mut Cx,
        me: CtrlId,
        xmlText: Option<&str>,
        label: Option<&str>,
    ) -> bool {
        // L1246
        let f = compare_hooks().new_compare_entry;
        let entry = f(cx, xmlText, label);
        // L1247
        if let Some(entry) = entry.filter(|e| e.model.calcsTab.mainOutput.get().is_some()) {
            let s = st_mut(cx, me);
            // L1248
            s.compareEntries.push(entry);
            // L1249
            s.activeCompareIndex = s.compareEntries.len();
            // L1250
            Self::UpdateBuildSelector(cx, me);
            // Restore primary build's window title
            // L1252 (`self.primaryBuild.spec` is always set)
            // L1253
            build_init::set_window_title_with_build_class(cx);
            // L1255
            return true;
        }
        // L1257
        false
    }

    /// lua: CompareTab.lua:1260-1270 `CompareTabClass:ImportFromCode(code)`
    /// Import a comparison build from a build code (base64-encoded)
    /// @pob Classes/CompareTab.lua:1260-1270 h=aafb5bdc
    pub fn ImportFromCode(cx: &mut Cx, me: CtrlId, code: &str) -> bool {
        // L1262
        let Some(xmlText) = decode_build_code(code) else {
            // L1263-1264
            return false;
        };
        // L1266
        if Self::ImportBuild(cx, me, Some(&xmlText), Some("Imported build")) {
            // L1267
            return true;
        }
        // L1269
        false
    }

    /// lua: CompareTab.lua:1272-1290 `CompareTabClass:RemoveBuild(index)`
    /// Remove a comparison build
    /// @pob Classes/CompareTab.lua:1272-1290 h=7d34a06a
    pub fn RemoveBuild(cx: &mut Cx, me: CtrlId, index: usize) {
        // L1274
        if index >= 1 && index <= st(cx, me).compareEntries.len() {
            let s = st_mut(cx, me);
            // L1275
            s.compareEntries.remove(index - 1);
            // L1276
            if s.activeCompareIndex > s.compareEntries.len() {
                // L1277
                s.activeCompareIndex = s.compareEntries.len();
            }
            // L1279
            if s.activeCompareIndex == 0 && !s.compareEntries.is_empty() {
                // L1280
                s.activeCompareIndex = 1;
            }
            // L1282
            if st(cx, me).compareEntries.is_empty() {
                // L1283-1284
                for key in [
                    "cmpMainSkill",
                    "cmpSkillPart",
                    "cmpStageCount",
                    "cmpMineCount",
                    "cmpMinion",
                    "cmpMinionSkill",
                ] {
                    // L1285
                    let c = ctl(cx, me, key);
                    cx.base_mut(c).shown = Prop::Val(false);
                }
            }
            // L1288
            Self::UpdateBuildSelector(cx, me);
        }
    }

    /// lua: CompareTab.lua:1292-1313 `CompareTabClass:ReimportPrimary()`
    /// Re-import primary build using character import (same as Import/Export tab)
    /// @pob Classes/CompareTab.lua:1292-1313 h=486be34b
    pub fn ReimportPrimary(cx: &mut Cx, _me: CtrlId) {
        // L1294
        let importTab = crate::tabs::import::core::tab(cx)
            .unwrap_or_else(|| lua_error!("attempt to index local 'importTab' (a nil value)"));
        let check = |cx: &mut Cx, key: &'static str| {
            let c = ctl(cx, importTab, key);
            cx.w_mut::<CheckBoxControl>(c).state = true;
        };

        // oauth import, if previously selected
        // L1297
        if DropDownControl::get_sel_value(cx, ctl(cx, importTab, "charSelect")).is_some() {
            // Set clear checkboxes to true (delete existing jewels, skills, equipment)
            // L1299
            check(cx, "charImportTreeClearJewels");
            // L1300
            check(cx, "charImportItemsClearSkills");
            // L1301
            check(cx, "charImportItemsClearItems");
            // Trigger both async imports (passive tree + items/skills)
            // L1303
            let b = ctl(cx, importTab, "charImportTree");
            cx.click(b);
            // L1304
            let b = ctl(cx, importTab, "charImportItems");
            cx.click(b);
        // account name import
        // L1306
        } else if DropDownControl::get_sel_value(cx, ctl(cx, importTab, "siteCharSelect")).is_some()
        {
            // L1307
            check(cx, "siteCharImportTreeClearJewels");
            // L1308
            check(cx, "siteCharImportItemsClearSkills");
            // L1309
            check(cx, "siteCharImportItemsClearItems");
            // L1310
            let b = ctl(cx, importTab, "siteCharImportTree");
            cx.click(b);
            // L1311
            let b = ctl(cx, importTab, "siteCharImportItems");
            cx.click(b);
        }
    }

    /// lua: CompareTab.lua:1315-1325 `CompareTabClass:UpdateBuildSelector()`
    /// Update the build selector dropdown
    /// @pob Classes/CompareTab.lua:1315-1325 h=8179adf1
    pub fn UpdateBuildSelector(cx: &mut Cx, me: CtrlId) {
        // L1317
        let mut list: Vec<DropItem> = Vec::new();
        // L1318
        for entry in &st(cx, me).compareEntries {
            // L1319 (`entry.label` is always set)
            list.push(DropItem::str(entry.label.clone()));
        }
        let len = list.len();
        let sel = ctl(cx, me, "compareBuildSelect");
        // L1321
        cx.w_mut::<DropDownControl>(sel).list = list;
        let active = st(cx, me).activeCompareIndex;
        // L1322
        if active > 0 && active <= len {
            // L1323
            cx.w_mut::<DropDownControl>(sel).sel_index = active;
        }
    }

    /// lua: CompareTab.lua:1327-1333 `CompareTabClass:GetActiveCompare()`
    /// Get the active comparison entry
    /// @pob Classes/CompareTab.lua:1327-1333 h=c952c421
    pub fn GetActiveCompare(cx: &Cx, me: CtrlId) -> Option<CompareEntryRef> {
        let s = st(cx, me);
        // L1329
        if s.activeCompareIndex > 0 && s.activeCompareIndex <= s.compareEntries.len() {
            // L1330
            return Some(s.compareEntries[s.activeCompareIndex - 1].clone());
        }
        // L1332
        None
    }

    /// lua: CompareTab.lua:1335-1373 `CompareTabClass:CopyCompareSpecToPrimary(andUse)`
    /// Copy the compared build's currently selected tree spec into the primary build
    /// @pob Classes/CompareTab.lua:1335-1373 h=120b0aa4
    pub fn CopyCompareSpecToPrimary(cx: &mut Cx, me: CtrlId, andUse: bool) {
        // L1337
        // L1338 (`entry.treeTab` always exists)
        let Some(entry) = Self::GetActiveCompare(cx, me) else {
            return;
        };
        let tt = &entry.model.treeTab;
        // L1339
        let sourceSpec = tt.specList.with(|l| at_num(l, tt.activeSpec.get()));
        // L1340
        let Some(sourceSpec) = sourceSpec else { return };

        // L1342
        let p = cx.build().model.clone();

        // Create new spec from source (same pattern as PassiveSpecListControl Copy)
        // Note: we don't copy jewels because they reference item IDs in the compared
        // build's itemsTab which don't exist in the primary build
        // L1347
        let newSpec = new_PassiveSpec(&p, sourceSpec.treeVersion.get(), false);
        // L1348
        let title = sourceSpec
            .title
            .get()
            .map_or_else(|| "Default".to_owned(), |t| t.to_string());
        newSpec
            .title
            .set(Some(format!("{title} (Compared)").as_str().into()));
        // L1349
        let state = sourceSpec.CreateUndoState();
        newSpec.RestoreUndoState(&state, None);
        build_init::flush_window_titles(cx);
        // L1350
        newSpec.BuildClusterJewelGraphs();

        // Add to primary build's spec list
        // L1353
        p.treeTab.specList.push(newSpec);

        // L1355
        if andUse {
            // L1356
            let n_specs = p.treeTab.specList.len();
            let f = build_init::hooks(cx).tree_set_active_spec;
            f(cx, n_specs as f64);
            // Restore primary build's window title
            // L1358 (`self.primaryBuild.spec` is always set)
            // L1359
            build_init::set_window_title_with_build_class(cx);
        }

        // Update items tab passive tree dropdown (same pattern as PassiveSpecListControl)
        // L1364
        let items = crate::tabs::items::ctor::tab(cx)
            .unwrap_or_else(|| lua_error!("attempt to index field 'itemsTab' (a nil value)"));
        let itemsSpecSelect = ctl(cx, items, "specSelect");
        // L1365
        let mut newSpecList: Vec<DropItem> = Vec::new();
        // L1366
        for spec in p.treeTab.specList.with(Clone::clone) {
            // L1367
            let t = spec
                .title
                .get()
                .map_or_else(|| "Default".to_owned(), |t| t.to_string());
            newSpecList.push(DropItem::str(t));
        }
        // L1369
        DropDownControl::set_list(cx, itemsSpecSelect, newSpecList);
        // L1370
        let active = p.treeTab.activeSpec.get();
        cx.w_mut::<DropDownControl>(itemsSpecSelect).sel_index = active as usize;

        // L1372
        p.buildFlag.set(true);
    }

    /// lua: CompareTab.lua:1375-1441 `CompareTabClass:GetJewelComparisonSlots(compareEntry)`
    /// Build a list of jewel comparison entries between the primary and compare builds.
    /// Returns a sorted list of { label, nodeId, pItem, cItem, pSlotName, cSlotName } records.
    /// @pob Classes/CompareTab.lua:1375-1441 h=10957063
    pub fn GetJewelComparisonSlots(
        cx: &Cx,
        compareEntry: &CompareEntryRef,
    ) -> Vec<JewelComparisonSlot> {
        let p = cx.build().model.clone();
        let c = compareEntry.model.clone();
        // L1378
        let pSpec = p.spec.get();
        // L1379
        let cSpec = c.spec.get();
        // L1380 (both specs are always set)

        // Collect union of all socket nodeIds that have a jewel equipped in either build
        // L1383
        let mut nodeIds: Vec<i64> = Vec::new();
        // L1384 ORDER: see the module notes
        for (spec, _) in [(&pSpec, 0), (&cSpec, 1)] {
            // L1385 / L1392
            for (nodeId, itemId) in spec.jewels.with(|j| j.pairs()) {
                // L1386 / L1393
                if itemId > 0.0 && !nodeIds.contains(&nodeId) {
                    // L1387 / L1394
                    nodeIds.push(nodeId);
                }
            }
        }

        // L1399
        let mut result: Vec<JewelComparisonSlot> = Vec::new();
        // L1400
        for nodeId in nodeIds {
            // L1401
            let pItemId = pSpec.jewels.with(|j| j.get(nodeId));
            // L1402
            let cItemId = cSpec.jewels.with(|j| j.get(nodeId));
            // L1403
            let pItem = pItemId.and_then(|id| item_at(&p, &Val::Num(id)));
            // L1404
            let cItem = cItemId.and_then(|id| item_at(&c, &Val::Num(id)));

            // Skip if neither build actually has a jewel here
            // L1407
            if pItem.is_some() || cItem.is_some() {
                // L1408
                let slotName = format!("Jewel {}", pob_lua::tostring_num(nodeId as f64));
                // Derive a friendly label from the primary build's socket control if available
                // L1410
                let mut label = slotName.clone();
                // L1411
                let pSocket = p.itemsTab.sockets.with(|s| s.get(nodeId));
                // L1412
                if let Some(l) = pSocket.as_ref().and_then(|s| s.str(n!("label"))) {
                    // L1413
                    label = l.to_string();
                } else {
                    // L1415
                    let cSocket = c.itemsTab.sockets.with(|s| s.get(nodeId));
                    // L1416
                    if let Some(l) = cSocket.as_ref().and_then(|s| s.str(n!("label"))) {
                        // L1417
                        label = l.to_string();
                    }
                }

                // Check if the socket node is allocated in each build's current tree
                // L1422
                let pNodeAllocated = pSpec.allocNodes.with(|a| a.contains_key(nodeId));
                // L1423
                let cNodeAllocated = cSpec.allocNodes.with(|a| a.contains_key(nodeId));

                // L1425-1434
                result.push(JewelComparisonSlot {
                    label,
                    nodeId,
                    pItem,
                    cItem,
                    pSlotName: slotName.clone(),
                    cSlotName: slotName,
                    pNodeAllocated,
                    cNodeAllocated,
                });
            }
        }

        // Sort by nodeId for stable ordering
        // L1439
        result.sort_by_key(|r| r.nodeId);
        // L1440
        result
    }

    /// lua: CompareTab.lua:1443-1464 `CompareTabClass:CopyCompareItemToPrimary(slotName,
    /// compareEntry, andUse)`
    /// Copy a compared build's item into the primary build
    /// @pob Classes/CompareTab.lua:1443-1464 h=c2775f37
    pub fn CopyCompareItemToPrimary(
        cx: &mut Cx,
        slotName: &str,
        compareEntry: &CompareEntryRef,
        andUse: bool,
    ) {
        let c = compareEntry.model.clone();
        // L1445
        let cSlot = c
            .itemsTab
            .slots
            .with(|s| s.get(&Name::from(slotName)).cloned());
        // L1446
        let cItem = cSlot.and_then(|slot| item_at(&c, &slot.get(n!("selItemId"))));
        // L1447 (`cItem.raw` is always a string)
        let Some(cItem) = cItem else { return };

        // L1449
        let raw = cItem.raw.get().to_string();
        let newItem = new_Item(Some(&raw), None, false);
        // L1450
        newItem.NormaliseQuality();
        // L1451
        let h = crate::tabs::items::slot_helper::items_hooks();
        // L1452 true = noAutoEquip
        (h.add_item)(cx, &newItem, true, None);

        // L1454
        if andUse {
            let p = cx.build().model.clone();
            // L1455
            let pSlot = p
                .itemsTab
                .slots
                .with(|s| s.get(&Name::from(slotName)).cloned());
            // L1456
            if let Some(pSlot) = pSlot {
                // L1457
                let id = newItem
                    .id
                    .get()
                    .unwrap_or_else(|| lua_error!("bad argument (number expected, got nil)"));
                crate::tabs::items::slot::set_sel_item_id(&p, &pSlot, id);
            }
        }

        // L1461
        (h.populate_slots)(cx);
        // L1462
        (h.add_undo_state)(cx);
        // L1463
        cx.build().model.buildFlag.set(true);
    }

    /// lua: CompareTab.lua:1466-1530 `CompareTabClass:OpenImportPopup()`
    /// Open the import popup for adding a comparison build
    /// @pob Classes/CompareTab.lua:1466-1530 h=d8cf32c8
    pub fn OpenImportPopup(cx: &mut Cx, me: CtrlId) {
        // L1468 local controls = {}
        // Use a local variable for state text so it doesn't go into the controls table
        // (PopupDialog iterates all controls table entries and expects them to be control objects)
        // L1471
        let stateText = Rc::new(RefCell::new(String::new()));
        // L1529
        popups::open_popup(
            cx,
            500.0,
            160.0,
            "Import Comparison Build",
            move |cx, p| {
                let p = p.id();
                // L1472
                cx.add(
                    p,
                    "label",
                    LabelControl::new(
                        Anchor::NONE,
                        rect(0, 20, 0, 16),
                        "^7Paste a build code or URL to import as comparison:",
                    ),
                );
                // L1473
                let input = cx
                    .add(
                        p,
                        "input",
                        EditControl::new(
                            Anchor::NONE,
                            rect(0, 50, 450, 20),
                            EditArgs {
                                init: Some(String::new()),
                                clearable: true,
                                ..EditArgs::default()
                            },
                        ),
                    )
                    .id();
                // L1474-1478
                cx.w_mut::<EditControl>(input).enter_func = Some(Rc::new(move |cx: &mut Cx, _| {
                    // L1475
                    if !cx.w::<EditControl>(pc(cx, p, "input")).buf.is_empty() {
                        // L1476
                        let go = pc(cx, p, "go");
                        let f = cx.w::<ButtonControl>(go).on_click.clone();
                        f(cx);
                    }
                }));

                // L1480
                let name = cx
                    .add(
                        p,
                        "name",
                        EditControl::new(
                            Anchor::NONE,
                            rect(0, 80, 450, 20),
                            EditArgs {
                                init: Some(String::new()),
                                prompt: Some("Name (optional)".to_owned()),
                                limit: Some(100),
                                ..EditArgs::default()
                            },
                        ),
                    )
                    .id();
                // L1481
                let state = cx
                    .add(
                        p,
                        "state",
                        LabelControl::new(
                            Anchor::new(TopLeft, name, BottomLeft),
                            rect(0, 4, 0, 16),
                            "",
                        ),
                    )
                    .id();
                // L1482-1484
                let st_text = stateText.clone();
                cx.w_mut::<LabelControl>(state).label =
                    Prop::f(move |_, _| st_text.borrow().clone());
                // L1485-1521
                let st_text = stateText.clone();
                cx.add(
                    p,
                    "go",
                    ButtonControl::new(
                        Anchor::NONE,
                        rect(-118, 130, 80, 20),
                        "Import",
                        move |cx| {
                            Self::import_popup_go(cx, me, p, &st_text);
                        },
                    ),
                );
                // L1522-1525
                cx.add(
                    p,
                    "importFolder",
                    ButtonControl::new(
                        Anchor::NONE,
                        rect(0, 130, 140, 20),
                        "Import from Folder",
                        move |cx| {
                            // L1523
                            popups::close_popup(cx);
                            // L1524
                            Self::OpenImportFolderPopup(cx, me);
                        },
                    ),
                );
                // L1526-1528
                cx.add(
                    p,
                    "cancel",
                    ButtonControl::new(Anchor::NONE, rect(118, 130, 80, 20), "Cancel", |cx| {
                        // L1527
                        popups::close_popup(cx);
                    }),
                );
            },
            PopupOpts {
                enter: Some("go".into()),
                default: Some("input".into()),
                escape: Some("cancel".into()),
                ..PopupOpts::default()
            },
        );
    }

    /// The Import button's `onClick` of the import popup (L1485-1521).
    fn import_popup_go(cx: &mut Cx, me: CtrlId, p: CtrlId, stateText: &Rc<RefCell<String>>) {
        // L1486
        let buf = cx.w::<EditControl>(pc(cx, p, "input")).buf.clone();
        // L1487
        if buf.is_empty() {
            // L1488
            return;
        }
        // L1490
        let name_buf = cx.w::<EditControl>(pc(cx, p, "name")).buf.clone();
        let customName = if name_buf.is_empty() {
            None
        } else {
            Some(name_buf)
        };

        // Check if it's a URL
        // L1493
        for site in &build_sites::WEBSITE_LIST {
            // L1494
            if pob_lua::pattern::r#match(&buf, site.match_url, None).is_some() {
                // L1495
                *stateText.borrow_mut() = format!("{}Downloading...", cx.main.cc.WARNING);
                let stateText = stateText.clone();
                let customName = customName.clone();
                let label = site.label;
                // L1496-1508
                build_sites::download_build(
                    cx,
                    &buf,
                    Some(site),
                    Rc::new(
                        move |cx: &mut Cx,
                              isSuccess: bool,
                              codeData: String,
                              _url: Option<String>| {
                            // L1497
                            if isSuccess {
                                // L1498
                                let xmlText = decode_build_code(&codeData);
                                // L1499
                                if let Some(xmlText) = xmlText {
                                    // L1500
                                    let label = customName
                                        .clone()
                                        .unwrap_or_else(|| format!("Imported from {label}"));
                                    Self::ImportBuild(cx, me, Some(&xmlText), Some(&label));
                                    // L1501
                                    popups::close_popup(cx);
                                } else {
                                    // L1503
                                    *stateText.borrow_mut() = format!(
                                        "{}Failed to decode build data",
                                        cx.main.cc.NEGATIVE
                                    );
                                }
                            } else {
                                // L1506
                                *stateText.borrow_mut() =
                                    format!("{}{codeData}", cx.main.cc.NEGATIVE);
                            }
                        },
                    ),
                );
                // L1509
                return;
            }
        }

        // Try as a build code
        // L1514
        let xmlText = decode_build_code(&buf);
        // L1515
        if let Some(xmlText) = xmlText {
            // L1516
            let label = customName.unwrap_or_else(|| "Imported build".to_owned());
            Self::ImportBuild(cx, me, Some(&xmlText), Some(&label));
            // L1517
            popups::close_popup(cx);
        } else {
            // L1519
            *stateText.borrow_mut() = format!("{}Invalid build code", cx.main.cc.NEGATIVE);
        }
    }

    /// lua: CompareTab.lua:1532-1645 `CompareTabClass:OpenImportFolderPopup()`
    /// Open the "Import from Folder" popup: browse the user's local builds folder and
    /// import the selected build file as a comparison.
    /// @pob Classes/CompareTab.lua:1532-1645 h=a28eae76
    pub fn OpenImportFolderPopup(cx: &mut Cx, me: CtrlId) {
        // L1535 local controls = {}
        // L1536 local searchText = "" (the host node's `filter_build_list`, module notes)
        // L1537 local sortMode = main.buildSortMode
        let sortMode = cx.app().main.build_sort_mode.clone();
        // L1644
        popups::open_popup(
            cx,
            700.0,
            500.0,
            "Import from Folder",
            move |cx, p| {
                let p = p.id();
                // Minimal listMode-like host consumed by BuildListControl/PathControl.
                // L1540-1544 (see the module notes)
                let listHost = cx.add_free(
                    Some(p),
                    New::new(
                        Base::new(Anchor::NONE, rect(0, 0, 0, 0)),
                        ListMode {
                            sub_path: Some(String::new()),
                            filter_build_list: Some(String::new()),
                            ..ListMode::default()
                        },
                    )
                    .host(),
                );
                // L1545-1565 listHost:BuildList / FilterBuildList / SortList / SelectControl:
                // `ListMode::{build_list, filter_build_list, sort_list}` (module notes)

                // L1568-1585 importBuildEntry: [`import_build_entry`]

                // Search box and sort dropdown sit above the build list.
                // L1588-1591
                let searchText = cx
                    .add(
                        p,
                        "searchText",
                        EditControl::new(
                            Anchor::nil(TopLeft, TopLeft),
                            rect(15, 25, 450, 20),
                            EditArgs {
                                init: Some(String::new()),
                                prompt: Some("Search".to_owned()),
                                filter: Some("%c%(%)".to_owned()),
                                limit: Some(100),
                                change_func: Some(Rc::new(move |cx: &mut Cx, buf: &str| {
                                    // L1589
                                    cx.get_mut(listHost).filter_build_list = Some(buf.to_owned());
                                    // L1590
                                    ListMode::filter_build_list(cx, listHost.id());
                                })),
                                clearable: true,
                                ..EditArgs::default()
                            },
                        ),
                    )
                    .id();
                // L1592
                EditControl::set_placeholder(
                    cx,
                    searchText,
                    "(e.g. class:assassin myfilename)",
                    false,
                );
                // L1593-1597
                let sort = cx
                    .add(
                        p,
                        "sort",
                        DropDownControl::new(
                            Anchor::nil(TopLeft, TopLeft),
                            rect(475, 25, 210, 20),
                            build_sort_drop_list(),
                            Some(Rc::new(
                                move |cx: &mut Cx, _index: usize, value: Option<&DropItem>| {
                                    let value = value
                                        .and_then(|v| v.get::<SortDropItem>())
                                        .unwrap_or_else(|| {
                                            lua_error!(
                                                "attempt to index local 'value' (a nil value)"
                                            )
                                        });
                                    // L1594 sortMode = value.sortMode (the popup's sort mode is
                                    // `main.buildSortMode`, module notes)
                                    // L1595
                                    cx.app_mut().main.build_sort_mode = value.sort_mode.to_owned();
                                    // L1596
                                    ListMode::sort_list(cx, listHost.id());
                                },
                            )),
                            None,
                            false,
                        ),
                    )
                    .id();
                // L1598
                DropDownControl::sel_by_value(cx, sort, &|item: &DropItem| {
                    item.get::<SortDropItem>()
                        .is_some_and(|v| v.sort_mode == sortMode)
                });

                // Build list itself. Reuses BuildListControl (which provides the PathControl
                // breadcrumbs)
                // L1601
                let list = cx.get(listHost).list.clone();
                let buildList = cx
                    .add(
                        p,
                        "buildList",
                        BuildListControl::new(
                            Anchor::nil(TopLeft, TopLeft),
                            rect(15, 75, 0, 0),
                            listHost,
                            list,
                            Some(String::new()),
                        ),
                    )
                    .id();
                // the host's `controls.buildList` (BuildList.lua's SortList reads it)
                cx.put(listHost, "buildList", buildList);
                // L1602
                cx.base_mut(buildList).width = Prop::f(|_, _| 670.0);
                // L1603
                cx.base_mut(buildList).height = Prop::f(|_, _| 355.0);

                // Override instance methods on the BuildListControl to tailor it for the popup:
                // navigate folders, import builds, and suppress rename/delete/drag behaviors.
                {
                    let ov = &mut cx.w_mut::<ListControl>(buildList).ov;
                    // L1607-1613 LoadBuild: [`popup_load_build`]; BuildListControl.lua:259-265
                    // OnSelClick calls it (CCR-0136 §3)
                    ov.on_sel_click = Some(Rc::new(
                        move |cx: &mut Cx,
                              list: CtrlId,
                              _index: usize,
                              v: &pob_ui_core::controls::ListValue,
                              doubleClick: bool| {
                            // BuildListControl.lua:L260
                            if doubleClick {
                                // L261
                                popup_load_build(cx, me, list, &entry_of(v));
                                let l = cx.w_mut::<ListControl>(list);
                                // L262
                                l.sel_dragging = false;
                                // L263
                                l.sel_drag_active = false;
                            }
                            None
                        },
                    ));
                    // L1614-1618
                    ov.on_sel_key_down = Some(Rc::new(
                        move |cx: &mut Cx,
                              list: CtrlId,
                              _index: usize,
                              v: &pob_ui_core::controls::ListValue,
                              key: Key| {
                            // L1615
                            if key == Key::Return {
                                // L1616
                                popup_load_build(cx, me, list, &entry_of(v));
                            }
                        },
                    ));
                    // L1625
                    ov.on_sel_copy = Some(Rc::new(
                        |_cx: &mut Cx,
                         _list: CtrlId,
                         _index: usize,
                         _v: &pob_ui_core::controls::ListValue| {},
                    ));
                    // L1626
                    ov.on_sel_cut = Some(Rc::new(
                        |_cx: &mut Cx,
                         _list: CtrlId,
                         _index: usize,
                         _v: &pob_ui_core::controls::ListValue| {},
                    ));
                    // L1627
                    ov.on_sel_delete = Some(Rc::new(
                        |_cx: &mut Cx,
                         _list: CtrlId,
                         _index: usize,
                         _v: &pob_ui_core::controls::ListValue| {},
                    ));
                }
                cx.set_ov(buildList, |ov| {
                    // L1619-1623
                    ov.on_hover_key_up = Some(Rc::new(|cx: &mut Cx, list: CtrlId, key: Key| {
                        // L1620
                        let sb = cx.child(list, "scrollBarV").unwrap_or_else(|| {
                            lua_error!("attempt to index field 'scrollBarV' (a nil value)")
                        });
                        if ScrollBarControl::is_scroll_down_key(cx, sb, key)
                            || ScrollBarControl::is_scroll_up_key(cx, sb, key)
                        {
                            // L1621
                            cx.on_key_up(list, key);
                        }
                    }));
                    // L1624
                    ov.can_receive_drag = Some(Rc::new(
                        |_cx: &Cx, _list: CtrlId, _ty: &str, _v: &DragValue| false,
                    ));
                });
                // L1628
                let path = cx.child_as::<PathControl>(buildList, "path");
                cx.set_ov(path, |ov| {
                    ov.can_receive_drag = Some(Rc::new(
                        |_cx: &Cx, _path: CtrlId, _ty: &str, _v: &DragValue| false,
                    ));
                });

                // Populate the initial list now that the control (and its path control) exist.
                // L1631
                ListMode::build_list(cx, listHost.id());

                // L1633-1638
                let open = cx
                    .add(
                        p,
                        "open",
                        ButtonControl::new(
                            Anchor::nil(TopLeft, TopLeft),
                            rect(255, 465, 80, 20),
                            "Open",
                            move |cx| {
                                // L1634
                                let sel = BuildListControl::sel_value(cx, buildList);
                                // L1635
                                if let Some(sel) = sel {
                                    // L1636
                                    popup_load_build(cx, me, buildList, &sel);
                                }
                            },
                        ),
                    )
                    .id();
                // L1639
                cx.base_mut(open).enabled =
                    Prop::f(move |cx, _| cx.w::<ListControl>(buildList).sel_value.is_some());
                // L1640-1642
                cx.add(
                    p,
                    "close",
                    ButtonControl::new(
                        Anchor::nil(TopLeft, TopLeft),
                        rect(365, 465, 80, 20),
                        "Close",
                        |cx| {
                            // L1641
                            popups::close_popup(cx);
                        },
                    ),
                );
            },
            PopupOpts {
                enter: Some("open".into()),
                default: Some("searchText".into()),
                escape: Some("close".into()),
                ..PopupOpts::default()
            },
        );
    }
}

/// `function controls.buildList:LoadBuild(build)` (L1607-1613), the popup's instance override.
fn popup_load_build(cx: &mut Cx, me: CtrlId, list: CtrlId, build: &Entry) {
    // L1608
    if let Some(folderName) = &build.folder_name {
        // L1609
        let path = cx.child_as::<PathControl>(list, "path");
        PathControl::set_sub_path(
            cx,
            path.id(),
            &format!("{}{folderName}/", build.sub_path),
            false,
        );
    } else {
        // L1611
        import_build_entry(cx, me, build);
    }
}

/// `local function importBuildEntry(build)` (L1567-1585): import the given build entry (xml
/// file on disk) as a comparison.
fn import_build_entry(cx: &mut Cx, me: CtrlId, build: &Entry) {
    // L1569
    let fileHnd = cx.sys.fs.read(&build.full_file_name);
    // L1570
    let Ok(bytes) = fileHnd else {
        // L1571
        popups::open_message_popup(
            cx,
            "Import Error",
            &format!("Couldn't open '{}'.", build.full_file_name),
        );
        // L1572
        return;
    };
    // L1574-1575
    let xmlText = String::from_utf8_lossy(&bytes).into_owned();
    // L1576
    if xmlText.is_empty() {
        // L1577
        popups::open_message_popup(cx, "Import Error", "Build file is empty or unreadable.");
        // L1578
        return;
    }
    // L1580
    if CompareTab::ImportBuild(cx, me, Some(&xmlText), build.build_name.as_deref()) {
        // L1581
        popups::close_popup(cx);
    } else {
        // L1583
        popups::open_message_popup(cx, "Import Error", "Failed to import build for comparison.");
    }
}
