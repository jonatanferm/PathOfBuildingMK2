//! @pob-file Classes/SkillsTab.lua @16de4b82d
//! SkillsTab model (Classes/SkillsTab.lua 441-630, 1129-1274, 1478-1502).
//!
//! Owner: BM-B. The model halves of the skills tab: loading and saving the
//! `<Skills>` section, gem processing (`ProcessSocketGroup`, `ProcessGemLevel`,
//! `UpdateSocketGroups`) and skill-set switching. UI statements (controls,
//! undo, loadout dropdowns) are left to the UI port (ui doc U30) and marked
//! `// UI:` where they sit in the Lua.
//!
//! CCR-0019 (docs/port/ccr/0019-bm-b-tab-model-contracts.md) lists the frozen
//! contract gaps this file works around locally:
//! * the helpers the model needs from outside the unit's ranges
//!   (`FindSkillGem` 1103-1127, the model half of `SetDisplayGroup`
//!   1275-1278, `NewSkillSet` 1452-1462, `RebuildImbuedSupportBySlot`
//!   1464-1475, the imbued-support `gemChangeFunc` 323-349) are ported here;
//! * `SkillsTab` has no slot for the saved UI settings (`sortGemsByDPS`,
//!   `sortGemsByDPSField`, `showSupportGemTypes`, `showLegacyGems`) nor for
//!   the dropdown selections Load goes through: they live in a per-tab side
//!   table ([`SkillsTab::ui_state`]);
//! * Lua `nil` for `socketGroup.label`, `socketGroup.includeInFullDPS`,
//!   `gemInstance.level` and `gemInstance.quality` (the typed fields are not
//!   `Option`): a nil label is `""` plus `extra["$labelNil"] = true`, a nil
//!   includeInFullDPS is `false` plus `extra["$includeInFullDPSNil"] = true`
//!   (Save writes `tostring(nil)`), a nil level/quality is NaN;
//! * `item.sockets.colourAlwaysMatches` (a string key on a list) is read from
//!   `item.extra.colourAlwaysMatches`;
//! * `grantedEffect.levels[level].cost = {}` (SkillsTab.lua:1188, a write into
//!   the immutable `data`) is recorded in a thread-local set
//!   ([`levelCostCleared`]).
#![allow(non_snake_case, clippy::all, clippy::pedantic)]

use std::cell::RefCell;

use crate::model::{XmlChild, XmlNode};
use crate::prelude::*;

// ---- local helpers (Rust-only plumbing) ---------------------------------------------------

/// `node.attrib[k]`.
fn attr<'a>(node: &'a XmlNode, k: &str) -> Option<&'a str> {
    node.attrib.get(k).map(|v| &**v)
}

/// `tonumber(s)` (nil → nil).
fn tonum(s: Option<&str>) -> Option<f64> {
    s.and_then(pob_lua::tonumber)
}

/// `colorCodes[name]`.
fn colorCode(name: &str) -> LStr {
    pob_gamedata::colorCodes
        .iter()
        .find(|(k, _)| *k == name)
        .map(|(_, v)| LStr::from(*v))
        .unwrap_or_else(|| lua_error!("colorCodes.{name} is nil"))
}

/// A Lua table key from a number (`self.skillSets[id]`): integral ids only.
fn set_key(id: f64) -> Option<u32> {
    (id.fract() == 0.0 && id >= 0.0 && id <= f64::from(u32::MAX)).then_some(id as u32)
}

/// `tostring(n)` of a number field that may be Lua nil (NaN here, CCR-0019).
fn tostring_opt_nan(v: f64) -> String {
    if v.is_nan() {
        "nil".to_owned()
    } else {
        tostring_num(v)
    }
}

/// `tostring(b)`.
fn tostring_bool(b: bool) -> String {
    if b { "true" } else { "false" }.to_owned()
}

fn new_xml(elem: &str) -> XmlNode {
    XmlNode {
        elem: LStr::from(elem),
        attrib: indexmap::IndexMap::new(),
        children: Vec::new(),
    }
}

/// `attrib[k] = v` for a value that may be nil/false (composeNode skips those).
fn set_attr(node: &mut XmlNode, k: &str, v: Option<String>) {
    if let Some(v) = v {
        node.attrib.insert(LStr::from(k), LStr::from(v));
    }
}

/// The `$labelNil` marker key on `socketGroup.extra` (CCR-0019).
pub const LABEL_NIL: &str = "$labelNil";

/// `socketGroup.label` with Lua nil (CCR-0019).
pub fn socketGroup_label(socketGroup: &SocketGroup) -> Option<LStr> {
    if socketGroup.extra.truthy(n!("$labelNil")) {
        None
    } else {
        Some(socketGroup.label.get())
    }
}

/// `socketGroup.label = v` with Lua nil (CCR-0019).
pub fn set_socketGroup_label(socketGroup: &SocketGroup, v: Option<&str>) {
    match v {
        Some(s) => {
            socketGroup.label.set(LStr::from(s));
            socketGroup.extra.set(n!("$labelNil"), Val::Nil);
        }
        None => {
            socketGroup.label.set(LStr::from(""));
            socketGroup.extra.set(n!("$labelNil"), true);
        }
    }
}

/// `socketGroup.includeInFullDPS` with Lua nil (absent attribute on load, CCR-0019).
pub fn socketGroup_includeInFullDPS(socketGroup: &SocketGroup) -> Val {
    let v = socketGroup.includeInFullDPS.get();
    if !v && socketGroup.extra.truthy(n!("$includeInFullDPSNil")) {
        Val::Nil
    } else {
        Val::Bool(v)
    }
}

/// `skillSet.socketGroupList` of a skill-set table (`{ id, title, socketGroupList }`):
/// the list is stored as `Val::Obj(Rc<Shared<Vec<SocketGroupRef>>>)`.
pub fn skillSet_socketGroupList(skillSet: &Table) -> Shared<Vec<SocketGroupRef>> {
    match skillSet.obj::<Shared<Vec<SocketGroupRef>>>(n!("socketGroupList")) {
        Some(l) => (*l).clone(),
        None => lua_error!("attempt to index field 'socketGroupList' (a nil value)"),
    }
}

fn new_socketGroupList_val(l: &Shared<Vec<SocketGroupRef>>) -> Val {
    Val::Obj(Rc::new(l.clone()))
}

thread_local! {
    /// `grantedEffect.levels[level].cost = {}` writes (SkillsTab.lua:1188): (granted effect id, level).
    static LEVEL_COST_CLEARED: RefCell<ahash::AHashSet<(Name, u64)>> = RefCell::new(ahash::AHashSet::new());
    /// Per-tab UI settings the frozen `SkillsTab` has no field for (CCR-0019), keyed by the tab's
    /// address. Each entry also holds the tab's `imbuedSupportBySlot` table: while the entry keeps
    /// it alive no other table can take its address, so `ptr_eq` tells a live tab from a new one
    /// that reuses a dropped tab's address (the cost is two small tables per tab ever used).
    static SKILLS_TAB_UI: RefCell<ahash::AHashMap<usize, (Table, Table)>> = RefCell::new(ahash::AHashMap::new());
}

/// Whether ProcessSocketGroup replaced `data.skills[id].levels[level].cost` with `{}` in this
/// process (SkillsTab.lua:1188; PoB mutates its global `data`, so this persists, CCR-0019).
pub fn levelCostCleared(grantedEffectId: Name, level: f64) -> bool {
    LEVEL_COST_CLEARED.with(|s| s.borrow().contains(&(grantedEffectId, level.to_bits())))
}

/// `defaultGemLevelList` (SkillsTab.lua:30-57): the `gemLevel` keys, in order.
const defaultGemLevelList: [&str; 5] = [
    "normalMaximum",
    "corruptedMaximum",
    "awakenedMaximum",
    "characterLevel",
    "levelOne",
];
/// `showSupportGemTypeList` (SkillsTab.lua:59-63): the `show` keys.
const showSupportGemTypeList: [&str; 3] = ["ALL", "NORMAL", "EXCEPTIONAL"];
/// `sortGemTypeList` (SkillsTab.lua:65-75): the `type` keys.
const sortGemTypeList: [&str; 9] = [
    "FullDPS",
    "CombinedDPS",
    "TotalDPS",
    "AverageDamage",
    "TotalDot",
    "BleedDPS",
    "IgniteDPS",
    "TotalPoisonDPS",
    "TotalEHP",
];

/// `DropDownControl:SelByValue(value, key)` over one of the lists above: the
/// new `selIndex` (unchanged when nothing matches, DropDownControl.lua:136-150).
fn selByValue(list: &[&str], selIndex: f64, value: Option<&str>) -> f64 {
    for (i, v) in list.iter().enumerate() {
        if Some(*v) == value {
            return (i + 1) as f64;
        }
    }
    selIndex
}

impl SkillsTab {
    /// The UI settings Load/Save read and write that the frozen struct has no field for
    /// (CCR-0019): `sortGemsByDPS`, `sortGemsByDPSField`, `showSupportGemTypes`,
    /// `showLegacyGems`, and the `selIndex` of the `defaultLevel`, `showSupportGemTypes`
    /// and `sortGemsByDPSFieldControl` dropdowns. Defaults are the constructor's
    /// (SkillsTab.lua:91-96; dropdowns start at index 1).
    pub fn ui_state(&self) -> Table {
        let key = self as *const SkillsTab as usize;
        SKILLS_TAB_UI.with(|m| {
            let mut m = m.borrow_mut();
            if m
                .get(&key)
                .is_some_and(|(owner, _)| !owner.ptr_eq(&self.imbuedSupportBySlot))
            {
                m.remove(&key);
            }
            m.entry(key)
                .or_insert_with(|| {
                    let t = Table::new();
                    t.set(n!("sortGemsByDPS"), true);
                    t.set(n!("sortGemsByDPSField"), "CombinedDPS");
                    t.set(n!("showSupportGemTypes"), "ALL");
                    t.set(n!("showLegacyGems"), false);
                    t.set(n!("defaultLevelSelIndex"), 1.0);
                    t.set(n!("showSupportGemTypesSelIndex"), 1.0);
                    t.set(n!("sortGemsByDPSFieldSelIndex"), 1.0);
                    (self.imbuedSupportBySlot.clone(), t)
                })
                .1
                .clone()
        })
    }

    /// `self.skillSets[id]`.
    fn skillSet(&self, id: f64) -> Option<Table> {
        let k = set_key(id)?;
        self.skillSets.with(|m| m.get(&k).cloned())
    }
}

// ---- SkillsTab.lua:323-349 (the imbued-support GemSelectControl change function) ----------

/// @pob Classes/SkillsTab.lua:323-349 h=ca99f4f1
impl SkillsTab {
    /// The model half of `self.controls.imbuedSupport.gemChangeFunc(gemData, _, _, gemMatch, slotName)`
    /// (CCR-0019: LoadSkill calls it). `gemData` is a gem record (or nil).
    pub fn imbuedSupport_gemChangeFunc(
        &self,
        build: &BuildRef,
        gemData: Option<GemDataRef>,
        gemMatch: bool,
        slotName: Option<Name>,
    ) {
        let mut gemData = gemData;
        // L323
        let targetSlot = slotName.or_else(|| self.displayGroup.get().and_then(|g| g.slot.get()));
        // L324
        let Some(targetSlot) = targetSlot else {
            // L325
            return;
        };
        // L327
        let updateDisplayGroup = self
            .displayGroup
            .get()
            .is_some_and(|g| Some(targetSlot) == g.slot.get());
        // L328 UI: self:AddUndoState()
        // L329
        if !gemMatch {
            // L330
            gemData = None;
        }
        // L332 (`type(gemData) == "string" or gemData.id`: a gem record always has an id)
        if let Some(gemData) = gemData {
            // L333
            let gem = build
                .data
                .gems
                .get(&gemData.id)
                .cloned()
                .unwrap_or_else(|| lua_error!("attempt to index local 'gem' (a nil value)"));
            // L334
            self.imbuedSupportBySlot.set(
                targetSlot,
                Val::Obj(Rc::new(gem.grantedEffect.clone()) as Rc<dyn std::any::Any>),
            );
            // L335
            if updateDisplayGroup {
                // L336
                if let Some(g) = self.displayGroup.get() {
                    g.extra.set(n!("imbuedSupport"), gem.name);
                }
            }
            // L338 UI: self.controls.imbuedSupport.inactiveCol = data.skillColorMap[…]
            // L339
            build.buildFlag.set(true);
        } else {
            // L341
            let cleanUp = self.imbuedSupportBySlot.get(targetSlot);
            // L342
            self.imbuedSupportBySlot.set(targetSlot, Val::Nil);
            // L343
            if updateDisplayGroup {
                // L344
                if let Some(g) = self.displayGroup.get() {
                    g.extra.set(n!("imbuedSupport"), Val::Nil);
                }
            }
            // L346
            if cleanUp.truthy() {
                // an imbued existed before clearing it, so reprocess
                // L347
                build.buildFlag.set(true);
            }
        }
    }
}

/// @pob Classes/SkillsTab.lua:441-521 h=c564f2d7
impl SkillsTab {
    pub fn LoadSkill(&self, build: &BuildRef, node: &crate::model::XmlNode, skillSetId: f64) {
        // L442
        if &*node.elem != "Skill" {
            // L443
            return;
        }

        // L446
        let socketGroup: SocketGroupRef = Rc::new(SocketGroup::default());
        // L447
        socketGroup
            .enabled
            .set(attr(node, "active") == Some("true") || attr(node, "enabled") == Some("true"));
        // L448 (`attrib and attrib == "true"`: nil when absent, kept as a marker for Save, CCR-0019)
        socketGroup
            .includeInFullDPS
            .set(attr(node, "includeInFullDPS").is_some_and(|v| v == "true"));
        if attr(node, "includeInFullDPS").is_none() {
            socketGroup.extra.set(n!("$includeInFullDPSNil"), true);
        }
        // L449
        socketGroup.groupCount.set(tonum(attr(node, "groupCount")));
        // L450
        set_socketGroup_label(&socketGroup, attr(node, "label"));
        // L451
        socketGroup.slot.set(attr(node, "slot").map(Name::from));
        // L452
        socketGroup.source.set(attr(node, "source").map(LStr::from));
        // L453
        socketGroup
            .mainActiveSkill
            .set(tonum(attr(node, "mainActiveSkill")).unwrap_or(1.0));
        // L454
        socketGroup
            .mainActiveSkillCalcs
            .set(tonum(attr(node, "mainActiveSkillCalcs")).unwrap_or(1.0));
        // L455
        socketGroup.gemList.set(Shared::new(Vec::new()));
        // L456
        if let (Some(imbuedSupport), Some(slot)) = (attr(node, "imbuedSupport"), attr(node, "slot"))
        {
            // L457
            socketGroup.extra.set(n!("imbuedSupport"), imbuedSupport);
            // L458
            let key = format!("{} support", pob_lua::string::lower(imbuedSupport));
            let gemData = build
                .data
                .gemForBaseName
                .get(key.as_str())
                .and_then(|id| build.data.gems.get(id))
                .cloned();
            // gemChangeFunc(gemData, nil, nil, slot): gemMatch is nil
            self.imbuedSupport_gemChangeFunc(build, gemData, false, Some(Name::from(slot)));
        }

        // L461
        for child in &node.children {
            let child = match child {
                XmlChild::Node(c) => c,
                XmlChild::Text(_) => lua_error!("attempt to index field 'attrib' (a nil value)"),
            };
            // L462
            let gemInstance: GemInstanceRef = Rc::new(GemInstance::default());
            // L463
            gemInstance
                .nameSpec
                .set(LStr::from(pob_lua::string::sanitiseText(
                    attr(child, "nameSpec").unwrap_or(""),
                )));
            // L464
            if let Some(gemId) = attr(child, "gemId") {
                // L465
                let mut gemData: Option<GemDataRef> = None;
                // L466
                let possibleVariants = build.data.gemsByGameId.0.get(&Name::from(gemId));
                // L467
                if let Some(possibleVariants) = possibleVariants {
                    // If it is a known gem, try to determine which variant is used
                    // L469
                    if let Some(variantId) = attr(child, "variantId") {
                        // New save format from 3.23 that stores the specific variation (transfiguration)
                        // L471
                        gemData = possibleVariants.get(&Name::from(variantId)).cloned();
                    } else if let Some(skillId) = attr(child, "skillId") {
                        // Old format relying on the uniqueness of the granted effects id
                        // L474
                        // LUA-ORDER: pairs over the variants (string keys); the first match wins
                        for (_, variant) in possibleVariants {
                            // L475
                            if variant.grantedEffectId.as_str() == skillId {
                                // L476
                                gemData = Some(variant.clone());
                                // L477
                                break;
                            }
                        }
                    }
                }
                // L482
                if let Some(gemData) = gemData {
                    // L483
                    gemInstance.gemId.set(Some(gemData.id));
                    // L484
                    gemInstance.skillId.set(Some(gemData.grantedEffectId));
                    // L485 (`gemData.nameSpec`: gem records have no such key, so this is nil;
                    // `sanitiseText(nil)` (L497) keeps nil and ProcessSocketGroup's
                    // `nameSpec or ""` (L1166) runs before anything reads it, so "" here)
                    let _ = &gemData;
                    gemInstance.nameSpec.set(LStr::from(""));
                }
            } else if let Some(skillId) = attr(child, "skillId") {
                // L488
                let grantedEffect = build.data.skills.get(&Name::from(skillId)).cloned();
                // L489
                if let Some(grantedEffect) = grantedEffect {
                    // L490 (`data.gemForSkill[grantedEffect]`: keyed by the granted effect table)
                    gemInstance
                        .gemId
                        .set(build.data.gemForSkill.get(&grantedEffect.id).copied());
                    // L491
                    gemInstance.skillId.set(Some(grantedEffect.id));
                    // L492
                    gemInstance
                        .nameSpec
                        .set(LStr::from(grantedEffect.name.as_str()));
                }
            }
            // L495 (nil when the attribute is missing: NaN, CCR-0019)
            gemInstance
                .level
                .set(tonum(attr(child, "level")).unwrap_or(f64::NAN));
            // L496
            gemInstance
                .quality
                .set(tonum(attr(child, "quality")).unwrap_or(f64::NAN));
            // L497
            let ns = pob_lua::string::sanitiseText(&gemInstance.nameSpec.get());
            gemInstance.nameSpec.set(LStr::from(ns));
            // L498
            gemInstance.enabled.set(match attr(child, "enabled") {
                None => true,
                Some(v) => v == "true",
            });
            // L499
            gemInstance
                .enableGlobal1
                .set(match attr(child, "enableGlobal1") {
                    None => true,
                    Some(v) => v == "true",
                });
            // L500
            gemInstance
                .enableGlobal2
                .set(attr(child, "enableGlobal2") == Some("true"));
            // L501
            gemInstance
                .count
                .set(Some(tonum(attr(child, "count")).unwrap_or(1.0)));
            // L502
            gemInstance.skillPart.set(tonum(attr(child, "skillPart")));
            // L503
            gemInstance
                .skillPartCalcs
                .set(tonum(attr(child, "skillPartCalcs")));
            // L504
            gemInstance
                .skillStageCount
                .set(tonum(attr(child, "skillStageCount")));
            // L505
            gemInstance
                .skillStageCountCalcs
                .set(tonum(attr(child, "skillStageCountCalcs")));
            // L506
            gemInstance
                .skillMineCount
                .set(tonum(attr(child, "skillMineCount")));
            // L507
            gemInstance
                .skillMineCountCalcs
                .set(tonum(attr(child, "skillMineCountCalcs")));
            // L508
            gemInstance
                .skillMinion
                .set(attr(child, "skillMinion").map(Name::from));
            // L509
            gemInstance
                .skillMinionCalcs
                .set(attr(child, "skillMinionCalcs").map(Name::from));
            // L510
            gemInstance
                .skillMinionItemSet
                .set(tonum(attr(child, "skillMinionItemSet")));
            // L511
            gemInstance
                .skillMinionItemSetCalcs
                .set(tonum(attr(child, "skillMinionItemSetCalcs")));
            // L512
            gemInstance
                .skillMinionSkill
                .set(tonum(attr(child, "skillMinionSkill")));
            // L513
            gemInstance
                .skillMinionSkillCalcs
                .set(tonum(attr(child, "skillMinionSkillCalcs")));
            // L514
            socketGroup.gemList.get().push(gemInstance);
        }
        // L516
        if attr(node, "skillPart").is_some() {
            if let Some(first) = socketGroup.gemList.get().get(1) {
                // L517
                first.skillPart.set(tonum(attr(node, "skillPart")));
            }
        }
        // L519
        self.ProcessSocketGroup(build, &socketGroup);
        // L520
        let skillSet = self
            .skillSet(skillSetId)
            .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
        skillSet_socketGroupList(&skillSet).push(socketGroup);
    }
}

/// @pob Classes/SkillsTab.lua:523-571 h=cc969f7f
impl SkillsTab {
    pub fn Load(&self, build: &BuildRef, xml: &crate::model::XmlNode, fileName: &str) {
        let ui = self.ui_state();
        // L524
        self.activeSkillSetId.set(0.0);
        // L525
        self.skillSets.with_mut(|m| m.clear());
        // L526
        self.skillSetOrderList.wipe();
        // Handle legacy configuration settings when loading `defaultGemLevel`
        // L528
        let mut selIndex = ui.num(n!("defaultLevelSelIndex"));
        if attr(xml, "matchGemLevelToCharacterLevel") == Some("true") {
            // L529
            selIndex = selByValue(&defaultGemLevelList, selIndex, Some("characterLevel"));
        } else if let Some(v) =
            attr(xml, "defaultGemLevel").filter(|v| pob_lua::tonumber(v).is_none())
        {
            // L531
            selIndex = selByValue(&defaultGemLevelList, selIndex, Some(v));
        } else {
            // L533
            selIndex = selByValue(&defaultGemLevelList, selIndex, Some("normalMaximum"));
        }
        ui.set(n!("defaultLevelSelIndex"), selIndex);
        // L535
        self.defaultGemLevel
            .set(Some(Name::from(defaultGemLevelList[selIndex as usize - 1])));
        // L536
        self.defaultGemQuality.set(Some(m_max(
            m_min(tonum(attr(xml, "defaultGemQuality")).unwrap_or(0.0), 23.0),
            0.0,
        )));
        // L537 UI: self.controls.defaultQuality:SetText(self.defaultGemQuality or "")
        // L538
        if let Some(v) = attr(xml, "sortGemsByDPS") {
            // L539
            ui.set(n!("sortGemsByDPS"), v == "true");
        }
        // L541 UI: self.controls.sortGemsByDPS.state = self.sortGemsByDPS
        // L542
        if let Some(v) = attr(xml, "showLegacyGems") {
            // L543
            ui.set(n!("showLegacyGems"), v == "true");
        }
        // L545 UI: self.controls.showLegacyGems.state = self.showLegacyGems
        // L546
        let s = selByValue(
            &showSupportGemTypeList,
            ui.num(n!("showSupportGemTypesSelIndex")),
            Some(attr(xml, "showSupportGemTypes").unwrap_or("ALL")),
        );
        ui.set(n!("showSupportGemTypesSelIndex"), s);
        // L547
        let f = selByValue(
            &sortGemTypeList,
            ui.num(n!("sortGemsByDPSFieldSelIndex")),
            Some(attr(xml, "sortGemsByDPSField").unwrap_or("CombinedDPS")),
        );
        ui.set(n!("sortGemsByDPSFieldSelIndex"), f);
        // L548
        ui.set(
            n!("showSupportGemTypes"),
            showSupportGemTypeList[s as usize - 1],
        );
        // L549
        ui.set(n!("sortGemsByDPSField"), sortGemTypeList[f as usize - 1]);
        // L550
        for node in &xml.children {
            // a text child has no `elem` (nil): neither branch applies
            let XmlChild::Node(node) = node else { continue };
            // L551
            if &*node.elem == "Skill" {
                // Old format, initialize skill sets if needed
                // L553
                if self.skillSetOrderList.get(1).is_none() {
                    // L554
                    self.skillSetOrderList.set(1, 1);
                    // L555
                    self.NewSkillSet(Some(1.0));
                }
                // L557
                self.LoadSkill(build, node, 1.0);
            }

            // L560
            if &*node.elem == "SkillSet" {
                // L561
                let skillSet = self.NewSkillSet(tonum(attr(node, "id")));
                // L562
                skillSet.set(n!("title"), attr(node, "title").map_or(Val::Nil, Val::from));
                // L563
                let id = skillSet.num(n!("id"));
                self.skillSetOrderList.push(
                    set_key(id)
                        .unwrap_or_else(|| lua_error!("CCR-0019: non-integer skill set id {id}")),
                );
                // L564
                for subNode in &node.children {
                    match subNode {
                        // L565
                        XmlChild::Node(subNode) => self.LoadSkill(build, subNode, id),
                        // `subNode.elem` of a string is nil: LoadSkill returns at once
                        XmlChild::Text(_) => {}
                    }
                }
            }
        }
        // L569
        self.SetActiveSkillSet(build, tonum(attr(xml, "activeSkillSet")).unwrap_or(1.0));
        // L570 UI: self:ResetUndo()
        let _ = fileName;
    }
}

/// @pob Classes/SkillsTab.lua:573-629 h=04da39af
impl SkillsTab {
    pub fn Save(&self, xml: &mut crate::model::XmlNode) {
        let ui = self.ui_state();
        // L574
        xml.attrib = indexmap::IndexMap::new();
        set_attr(
            xml,
            "activeSkillSet",
            Some(tostring_num(self.activeSkillSetId.get())),
        );
        set_attr(
            xml,
            "defaultGemLevel",
            self.defaultGemLevel.get().map(|n| n.to_string()),
        );
        set_attr(
            xml,
            "defaultGemQuality",
            Some(
                self.defaultGemQuality
                    .get()
                    .map_or_else(|| "nil".to_owned(), tostring_num),
            ),
        );
        set_attr(
            xml,
            "sortGemsByDPS",
            Some(tostring_bool(ui.truthy(n!("sortGemsByDPS")))),
        );
        set_attr(
            xml,
            "showSupportGemTypes",
            ui.str(n!("showSupportGemTypes")).map(|s| s.to_string()),
        );
        set_attr(
            xml,
            "sortGemsByDPSField",
            ui.str(n!("sortGemsByDPSField")).map(|s| s.to_string()),
        );
        set_attr(
            xml,
            "showLegacyGems",
            Some(tostring_bool(ui.truthy(n!("showLegacyGems")))),
        );
        // L583
        for_ipairs!((_i, skillSetId) in self.skillSetOrderList => {
            // L584
            let skillSet = self
                .skillSet(f64::from(skillSetId))
                .unwrap_or_else(|| lua_error!("attempt to index local 'skillSet' (a nil value)"));
            // L585
            let mut child = new_xml("SkillSet");
            set_attr(&mut child, "id", Some(tostring_num(f64::from(skillSetId))));
            set_attr(&mut child, "title", skillSet.str(n!("title")).map(|s| s.to_string()));
            // L586 (t_insert(xml, child) happens before the children are added; same tree)

            // L588
            for_ipairs!((_j, socketGroup) in skillSet_socketGroupList(&skillSet) => {
                // L589
                let mut node = new_xml("Skill");
                set_attr(&mut node, "enabled", Some(tostring_bool(socketGroup.enabled.get())));
                set_attr(
                    &mut node,
                    "includeInFullDPS",
                    Some(tostring_val(&socketGroup_includeInFullDPS(&socketGroup))),
                );
                set_attr(&mut node, "groupCount", socketGroup.groupCount.get().map(tostring_num));
                set_attr(&mut node, "label", socketGroup_label(&socketGroup).map(|s| s.to_string()));
                set_attr(&mut node, "slot", socketGroup.slot.get().map(|s| s.to_string()));
                set_attr(&mut node, "source", socketGroup.source.get().map(|s| s.to_string()));
                set_attr(
                    &mut node,
                    "mainActiveSkill",
                    Some(tostring_num(socketGroup.mainActiveSkill.get())),
                );
                set_attr(
                    &mut node,
                    "mainActiveSkillCalcs",
                    Some(tostring_num(socketGroup.mainActiveSkillCalcs.get())),
                );
                set_attr(
                    &mut node,
                    "imbuedSupport",
                    match socketGroup.extra.get(n!("imbuedSupport")) {
                        Val::Nil | Val::Bool(false) => None,
                        v => Some(tostring_val(&v)),
                    },
                );
                // L600
                for_ipairs!((_k, gemInstance) in socketGroup.gemList.get() => {
                    // L601
                    let mut g = new_xml("Gem");
                    let gemData = gemInstance.gemData.get();
                    set_attr(&mut g, "nameSpec", Some(gemInstance.nameSpec.get().to_string()));
                    set_attr(&mut g, "skillId", gemInstance.skillId.get().map(|s| s.to_string()));
                    set_attr(&mut g, "gemId", gemData.as_ref().map(|d| d.gameId.to_string()));
                    set_attr(&mut g, "variantId", gemData.as_ref().map(|d| d.variantId.to_string()));
                    set_attr(&mut g, "level", Some(tostring_opt_nan(gemInstance.level.get())));
                    set_attr(&mut g, "quality", Some(tostring_opt_nan(gemInstance.quality.get())));
                    set_attr(&mut g, "enabled", Some(tostring_bool(gemInstance.enabled.get())));
                    set_attr(&mut g, "enableGlobal1", Some(tostring_bool(gemInstance.enableGlobal1.get())));
                    set_attr(&mut g, "enableGlobal2", Some(tostring_bool(gemInstance.enableGlobal2.get())));
                    set_attr(
                        &mut g,
                        "count",
                        Some(gemInstance.count.get().map_or_else(|| "nil".to_owned(), tostring_num)),
                    );
                    set_attr(&mut g, "skillPart", gemInstance.skillPart.get().map(tostring_num));
                    set_attr(&mut g, "skillPartCalcs", gemInstance.skillPartCalcs.get().map(tostring_num));
                    set_attr(&mut g, "skillStageCount", gemInstance.skillStageCount.get().map(tostring_num));
                    set_attr(
                        &mut g,
                        "skillStageCountCalcs",
                        gemInstance.skillStageCountCalcs.get().map(tostring_num),
                    );
                    set_attr(&mut g, "skillMineCount", gemInstance.skillMineCount.get().map(tostring_num));
                    set_attr(
                        &mut g,
                        "skillMineCountCalcs",
                        gemInstance.skillMineCountCalcs.get().map(tostring_num),
                    );
                    set_attr(&mut g, "skillMinion", gemInstance.skillMinion.get().map(|s| s.to_string()));
                    set_attr(
                        &mut g,
                        "skillMinionCalcs",
                        gemInstance.skillMinionCalcs.get().map(|s| s.to_string()),
                    );
                    set_attr(
                        &mut g,
                        "skillMinionItemSet",
                        gemInstance.skillMinionItemSet.get().map(tostring_num),
                    );
                    set_attr(
                        &mut g,
                        "skillMinionItemSetCalcs",
                        gemInstance.skillMinionItemSetCalcs.get().map(tostring_num),
                    );
                    set_attr(&mut g, "skillMinionSkill", gemInstance.skillMinionSkill.get().map(tostring_num));
                    set_attr(
                        &mut g,
                        "skillMinionSkillCalcs",
                        gemInstance.skillMinionSkillCalcs.get().map(tostring_num),
                    );
                    node.children.push(XmlChild::Node(g));
                });
                // L626
                child.children.push(XmlChild::Node(node));
            });
            // L586
            xml.children.push(XmlChild::Node(child));
        });
    }
}

/// Lua `tostring(v)` of a scalar value.
pub(crate) fn tostring_val(v: &Val) -> String {
    match v {
        Val::Nil => "nil".to_owned(),
        Val::Bool(b) => tostring_bool(*b),
        Val::Num(n) => tostring_num(*n),
        Val::Str(s) => s.to_string(),
        Val::Table(t) => format!("table: {:p}", t.as_ptr()),
        Val::Data(_) | Val::Mod(_) => "table".to_owned(),
        Val::Obj(_) => "userdata".to_owned(),
    }
}

// ---- SkillsTab.lua:1103-1127 FindSkillGem (CCR-0019: ProcessSocketGroup needs it) ---------

/// @pob Classes/SkillsTab.lua:1103-1127 h=c67cf851
impl SkillsTab {
    /// → `(errMsg, gemData)`.
    pub fn FindSkillGem(
        &self,
        build: &BuildRef,
        nameSpec: &str,
    ) -> (Option<String>, Option<GemDataRef>) {
        use pob_lua::pattern::{gsub, gsub_fn};
        let caseClass = |c: &pob_lua::pattern::Captures<'_>| {
            let a = c[0].to_str();
            Some(format!(
                "[{}{}]",
                pob_lua::string::upper(&a),
                pob_lua::string::lower(&a)
            ))
        };
        // Search for gem name using increasingly broad search patterns
        // L1105
        let noSpaces = gsub(nameSpec, " ", "", None).0;
        let patternList: [String; 5] = [
            // Exact match (case-insensitive)
            format!("^ {}$", gsub_fn(nameSpec, "%a", caseClass, None).0),
            // Simple abbreviation ("CtF" -> "Cold to Fire")
            format!("^{}$", gsub(nameSpec, "%a", " %0%%l+", None).0),
            // Abbreviated words ("CldFr" -> "Cold to Fire")
            format!("^ {}%l+$", gsub(&noSpaces, "%l", "%%l*%0", None).0),
            // Global abbreviation ("CtoF" -> "Cold to Fire")
            format!("^{}", gsub(&noSpaces, "%a", ".*%0", None).0),
            // Case insensitive global abbreviation ("ctof" -> "Cold to Fire")
            format!(
                "^{}",
                gsub_fn(
                    &noSpaces,
                    "%a",
                    |c| {
                        let a = c[0].to_str();
                        Some(format!(
                            ".*[{}{}]",
                            pob_lua::string::upper(&a),
                            pob_lua::string::lower(&a)
                        ))
                    },
                    None
                )
                .0
            ),
        ];
        // L1112
        for pattern in &patternList {
            // L1113
            let mut foundGemData: Option<GemDataRef> = None;
            // L1114
            // LUA-ORDER: pairs(data.gems); only the "Ambiguous" message depends on it
            for (_gemId, gemData) in &build.data.gems {
                // L1115
                let hay = format!(" {}", gemData.name);
                if pob_lua::pattern::r#match(&hay, pattern, None).is_some() {
                    // L1116
                    if let Some(found) = &foundGemData {
                        // L1117
                        return (
                            Some(format!(
                                "Ambiguous gem name '{}': matches '{}', '{}'",
                                nameSpec, found.name, gemData.name
                            )),
                            None,
                        );
                    }
                    // L1119
                    foundGemData = Some(gemData.clone());
                }
            }
            // L1122
            if foundGemData.is_some() {
                // L1123
                return (None, foundGemData);
            }
        }
        // L1126
        (Some(format!("Unrecognised gem name '{nameSpec}'")), None)
    }
}

/// @pob Classes/SkillsTab.lua:1129-1157 h=9e5720e6
impl SkillsTab {
    pub fn ProcessGemLevel(&self, build: &BuildRef, gemData: &GemDataRef, imbued: bool) -> f64 {
        // L1130
        let grantedEffect = &gemData.grantedEffect;
        // L1131
        let naturalMaxLevel = gemData.naturalMaxLevel;
        let defaultGemLevel = self.defaultGemLevel.get();
        let is = |s: &str| defaultGemLevel.is_some_and(|d| d.as_str() == s);
        // L1132
        if imbued || is("levelOne") {
            // L1133
            1.0
        } else if is("awakenedMaximum") {
            // L1135
            naturalMaxLevel + 1.0
        } else if is("corruptedMaximum") {
            // L1137
            if grantedEffect.plusVersionOf.is_some() {
                // L1138
                naturalMaxLevel
            } else {
                // L1140
                naturalMaxLevel + 1.0
            }
        } else if is("normalMaximum") {
            // L1143
            naturalMaxLevel
        } else {
            // self.defaultGemLevel == "characterLevel"
            // L1145
            let mut maxGemLevel = naturalMaxLevel;
            // L1146
            if grantedEffect.levels.get(maxGemLevel).is_none() {
                // L1147
                maxGemLevel = grantedEffect.levels.len() as f64;
            }
            // L1149 (`self.build and self.build.characterLevel or 1`: the build always exists)
            let characterLevel = build.characterLevel.get();
            // L1150
            let mut gemLevel = maxGemLevel;
            while gemLevel >= 1.0 {
                // L1151
                let lvl = grantedEffect
                    .levels
                    .get(gemLevel)
                    .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
                if lvl.levelRequirement <= characterLevel {
                    // L1152
                    return gemLevel;
                }
                gemLevel -= 1.0;
            }
            // L1155
            1.0
        }
    }
}

// Processes the given socket group, filling in information that will be used for display or calculations
/// @pob Classes/SkillsTab.lua:1161-1234 h=fc890135
impl SkillsTab {
    pub fn ProcessSocketGroup(&self, build: &BuildRef, socketGroup: &SocketGroupRef) {
        // Loop through the skill gem list
        // L1163
        let data = build.data;
        // L1164
        for_ipairs!((_i, gemInstance) in socketGroup.gemList.get() => {
            // L1165
            gemInstance.extra.set(n!("color"), "^8");
            // L1166 (`nameSpec or ""`: the typed field is never nil)
            // L1167
            let prevDefaultLevel: Option<f64> = match gemInstance.gemData.get() {
                Some(g) => Some(g.naturalMaxLevel),
                None => gemInstance.extra.truthy(n!("new")).then_some(20.0),
            };
            // L1168
            gemInstance.gemData.set(None);
            gemInstance.grantedEffect.set(None);
            // L1169
            if let Some(gemId) = gemInstance.gemId.get() {
                // Specified by gem ID
                // Used for skills granted by skill gems
                // L1172
                gemInstance.extra.set(n!("errMsg"), Val::Nil);
                // L1173
                gemInstance.gemData.set(data.gems.get(&gemId).cloned());
                // L1174
                if let Some(gemData) = gemInstance.gemData.get() {
                    // L1175
                    gemInstance.nameSpec.set(LStr::from(gemData.name.as_str()));
                    // L1176
                    gemInstance.skillId.set(Some(gemData.grantedEffectId));
                }
            } else if let Some(skillId) = gemInstance.skillId.get() {
                // Specified by skill ID
                // Used for skills granted by items
                // L1181
                gemInstance.extra.set(n!("errMsg"), Val::Nil);
                // L1182
                // LUA-QUIRK: `data.gemForSkill` is keyed by granted-effect tables (Data.lua:1094), so
                // indexing it with the skill id string is always nil.
                let gemId: Option<Name> = { let _ = skillId; None };
                // L1183
                if let Some(gemId) = gemId {
                    // L1184
                    gemInstance.gemData.set(data.gems.get(&gemId).cloned());
                } else {
                    // L1186
                    gemInstance.grantedEffect.set(data.skills.get(&skillId).cloned());
                }
                // L1188
                if gemInstance.triggered.get() {
                    // L1189
                    let grantedEffect = gemInstance.grantedEffect.get().unwrap_or_else(|| {
                        lua_error!("attempt to index field 'grantedEffect' (a nil value)")
                    });
                    let level = gemInstance.level.get();
                    if grantedEffect.levels.get(level).is_some() {
                        // L1190 (a write into the global data: CCR-0019)
                        LEVEL_COST_CLEARED.with(|s| {
                            s.borrow_mut().insert((grantedEffect.id, level.to_bits()));
                        });
                    }
                }
            } else if pob_lua::pattern::r#match(&gemInstance.nameSpec.get(), "%S", None).is_some() {
                // Specified by gem/skill name, try to match it
                // Used to migrate pre-1.4.20 builds
                // L1196
                let (errMsg, gemData) = self.FindSkillGem(build, &gemInstance.nameSpec.get());
                gemInstance.extra.set(n!("errMsg"), errMsg.map_or(Val::Nil, Val::from));
                gemInstance.gemData.set(gemData);
                // L1197
                gemInstance.gemId.set(gemInstance.gemData.get().map(|g| g.id));
                // L1198
                gemInstance
                    .skillId
                    .set(gemInstance.gemData.get().map(|g| g.grantedEffectId));
                // L1199
                if let Some(gemData) = gemInstance.gemData.get() {
                    // L1200
                    gemInstance.nameSpec.set(LStr::from(gemData.name.as_str()));
                }
            } else {
                // L1203
                gemInstance.extra.set(n!("errMsg"), Val::Nil);
                gemInstance.gemData.set(None);
                gemInstance.skillId.set(None);
            }
            // L1205 (`grantedEffect.unsupported`: no granted effect in the dump has the key
            // (strict schema, pob_gamedata::GrantedEffect), so it is always nil)
            if gemInstance.gemData.get().is_some() && grantedEffect_unsupported() {
                // L1206
                gemInstance.extra.set(
                    n!("errMsg"),
                    format!("{} is not supported yet", gemInstance.nameSpec.get()),
                );
                // L1207
                gemInstance.gemData.set(None);
            }
            // L1209
            if gemInstance.gemData.get().is_some() || gemInstance.grantedEffect.get().is_some() {
                // L1210
                gemInstance.extra.set(n!("new"), Val::Nil);
                // L1211
                let grantedEffect = gemInstance
                    .grantedEffect
                    .get()
                    .or_else(|| gemInstance.gemData.get().map(|g| g.grantedEffect.clone()))
                    .expect("checked above");
                // L1212
                let color = match grantedEffect.color {
                    Some(1) => colorCode("STRENGTH"),
                    // L1214
                    Some(2) => colorCode("DEXTERITY"),
                    // L1216
                    Some(3) => colorCode("INTELLIGENCE"),
                    // L1219
                    _ => colorCode("NORMAL"),
                };
                gemInstance.extra.set(n!("color"), color);
                // L1221
                if let (Some(prev), Some(gemData)) = (prevDefaultLevel, gemInstance.gemData.get()) {
                    if gemData.naturalMaxLevel != prev {
                        // L1222
                        gemInstance.level.set(gemData.naturalMaxLevel);
                        // L1223
                        gemInstance.extra.set(n!("naturalMaxLevel"), gemInstance.level.get());
                    }
                }
                // L1225
                crate::modules::calc_tools::validateGemLevel(&*gemInstance);
                // L1226
                if let Some(gemData) = gemInstance.gemData.get() {
                    // L1227
                    let lvl = grantedEffect
                        .levels
                        .get(gemInstance.level.get())
                        .unwrap_or_else(|| lua_error!("attempt to index a nil value"));
                    gemInstance.reqLevel.set(Some(lvl.levelRequirement));
                    let reqLevel = lvl.levelRequirement;
                    // L1228
                    gemInstance.extra.set(
                        n!("reqStr"),
                        crate::modules::calc_tools::getGemStatRequirement(reqLevel, grantedEffect.support, gemData.reqStr),
                    );
                    // L1229
                    gemInstance.extra.set(
                        n!("reqDex"),
                        crate::modules::calc_tools::getGemStatRequirement(reqLevel, grantedEffect.support, gemData.reqDex),
                    );
                    // L1230
                    gemInstance.extra.set(
                        n!("reqInt"),
                        crate::modules::calc_tools::getGemStatRequirement(reqLevel, grantedEffect.support, gemData.reqInt),
                    );
                }
            }
        });
    }
}

/// `grantedEffect.unsupported` (SkillsTab.lua:1205): the key exists on no
/// granted effect at the pin (`pob_gamedata::GrantedEffect` is strict).
#[inline]
fn grantedEffect_unsupported() -> bool {
    false
}

// reprocess socket groups on rebuild
/// @pob Classes/SkillsTab.lua:1237-1273 h=2751684a
impl SkillsTab {
    pub fn UpdateSocketGroups(&self, build: &BuildRef) {
        // L1238
        let slotSocketedCounts = Table::new();
        // L1239
        for_ipairs!((_i, socketGroup) in self.socketGroupList.get() => {
            // Clear stale matches when a group is no longer assigned to an item.
            // L1241
            for_ipairs!((_j, gemInstance) in socketGroup.gemList.get() => {
                // L1242
                gemInstance.extra.set(n!("matchesSocket"), false);
            });
            // L1244
            if let Some(slotName) = socketGroup.slot.get() {
                // L1245
                let gemOffset = slotSocketedCounts.num_or(slotName, 0.0);
                // L1246
                for_ipairs!((i, gemInstance) in socketGroup.gemList.get() => {
                    // add quality for matching sockets by looking up linked item
                    // L1248
                    if gemInstance.grantedEffect.get().is_some() || gemInstance.gemData.get().is_some() {
                        // L1249
                        let grantedEffect = gemInstance
                            .grantedEffect
                            .get()
                            .or_else(|| gemInstance.gemData.get().map(|g| g.grantedEffect.clone()))
                            .expect("checked above");
                        // L1250
                        let slot = build.itemsTab.slots.with(|m| m.get(&slotName).cloned());
                        // since PoB processes split links on an item as separate
                        // groups, we can assume that we continue from where the last
                        // socket group with the slot ended at
                        // L1254
                        let colours = ["R", "G", "B"];
                        // L1255
                        let gemIdx = gemOffset + i as f64;
                        // L1256
                        if let Some(slot) = slot {
                            // L1257
                            let selItemId = slot.get(n!("selItemId"));
                            let item = match selItemId {
                                Val::Num(id) => set_key(id)
                                    .and_then(|k| build.itemsTab.items.with(|m| m.get(&k).cloned())),
                                _ => None,
                            };
                            // L1258 (`item.sockets` is always a table on a parsed item)
                            if let Some(item) = item {
                                // e.g. dialla's malefaction
                                // L1260 (`item.sockets.colourAlwaysMatches`: CCR-0019)
                                if item.extra.truthy(n!("colourAlwaysMatches")) {
                                    // L1261
                                    gemInstance.extra.set(n!("matchesSocket"), true);
                                } else {
                                    // L1263
                                    let gemColour: Option<&str> = grantedEffect
                                        .color
                                        .and_then(|c| colours.get(usize::from(c).wrapping_sub(1)).copied());
                                    // L1264
                                    let sockets = item.sockets.get();
                                    let socket = if gemIdx >= 1.0 && gemIdx.fract() == 0.0 {
                                        sockets.get(gemIdx as usize)
                                    } else {
                                        None
                                    };
                                    let v: Val = match socket {
                                        None => Val::Nil,
                                        Some(s) => {
                                            let c = s.get(n!("color"));
                                            Val::Bool(match gemColour {
                                                Some(g) => c.str().is_some_and(|c| &*c == g),
                                                None => c.is_nil(),
                                            })
                                        }
                                    };
                                    gemInstance.extra.set(n!("matchesSocket"), v);
                                }
                            }
                        }
                    }
                });
                // L1270
                slotSocketedCounts.set(slotName, gemOffset + socketGroup.gemList.get().len() as f64);
            }
        });
    }
}

// Set the skill to be displayed/edited
/// @pob Classes/SkillsTab.lua:1275-1278 h=8398304a
impl SkillsTab {
    /// The model half of `SetDisplayGroup` (the rest, 1279-1307, updates controls: ui doc U30).
    pub fn SetDisplayGroup(&self, build: &BuildRef, socketGroup: Option<SocketGroupRef>) {
        // L1276
        self.displayGroup.set(socketGroup.clone());
        // L1277
        if let Some(socketGroup) = socketGroup {
            // L1278
            self.ProcessSocketGroup(build, &socketGroup);
            // UI: SkillsTab.lua:1280-1306 (controls, UpdateGemSlots)
        }
    }
}

// Creates a new skill set
/// @pob Classes/SkillsTab.lua:1452-1462 h=3a18af65
impl SkillsTab {
    pub fn NewSkillSet(&self, skillSetId: Option<f64>) -> Table {
        // L1453
        let skillSet = Table::new();
        skillSet.set(n!("id"), skillSetId.map_or(Val::Nil, Val::Num));
        skillSet.set(
            n!("socketGroupList"),
            new_socketGroupList_val(&Shared::new(Vec::new())),
        );
        // L1454
        if skillSetId.is_none() {
            // L1455
            skillSet.set(n!("id"), 1.0);
            // L1456
            while self.skillSet(skillSet.num(n!("id"))).is_some() {
                // L1457
                let id = skillSet.num(n!("id"));
                skillSet.set(n!("id"), id + 1.0);
            }
        }
        // L1460
        let id = skillSet.num(n!("id"));
        let key =
            set_key(id).unwrap_or_else(|| lua_error!("CCR-0019: non-integer skill set id {id}"));
        self.skillSets.with_mut(|m| {
            m.insert(key, skillSet.clone());
        });
        // L1461
        skillSet
    }
}

/// @pob Classes/SkillsTab.lua:1464-1475 h=69f61fb4
impl SkillsTab {
    pub fn RebuildImbuedSupportBySlot(&self, build: &BuildRef) {
        // L1465
        self.imbuedSupportBySlot.wipe();
        // L1466
        for_ipairs!((_i, socketGroup) in self.socketGroupList.get() => {
            // L1467
            let imbuedSupport = socketGroup.extra.get(n!("imbuedSupport"));
            if let (Some(slot), true) = (socketGroup.slot.get(), imbuedSupport.truthy()) {
                // L1468 (uses the global `data`)
                let key = format!("{} support", pob_lua::string::lower(&tostring_val(&imbuedSupport)));
                let gemId = build.data.gemForBaseName.get(key.as_str()).copied();
                // L1469
                let gem = gemId.and_then(|id| build.data.gems.get(&id).cloned());
                // L1470 (`gem.grantedEffect` is always set on a gem record)
                if let Some(gem) = gem {
                    // L1471
                    self.imbuedSupportBySlot.set(
                        slot,
                        Val::Obj(Rc::new(gem.grantedEffect.clone()) as Rc<dyn std::any::Any>),
                    );
                }
            }
        });
    }
}

// Changes the active skill set
/// @pob Classes/SkillsTab.lua:1478-1502 h=7266cccb
impl SkillsTab {
    pub fn SetActiveSkillSet(&self, build: &BuildRef, skillSetId: f64) {
        // Initialize skill sets if needed
        // L1480
        if self.skillSetOrderList.get(1).is_none() {
            // L1481
            self.skillSetOrderList.set(1, 1);
            // L1482
            self.NewSkillSet(Some(1.0));
        }

        // L1485 (`if not skillSetId`: this signature always passes a number)
        let mut skillSetId = skillSetId;

        // L1489
        if self.skillSet(skillSetId).is_none() {
            // L1490
            skillSetId = self
                .skillSetOrderList
                .get(1)
                .map_or_else(|| lua_error!("attempt to index a nil value"), f64::from);
        }

        // L1493
        let skillSet = self
            .skillSet(skillSetId)
            .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
        self.socketGroupList
            .set(skillSet_socketGroupList(&skillSet));
        // L1494
        self.RebuildImbuedSupportBySlot(build);
        // L1495 UI: self.controls.groupList.list = self.socketGroupList
        // L1496
        self.activeSkillSetId.set(skillSetId);
        // L1497
        build.buildFlag.set(true);

        // set the loadout option to the dummy option since it is now dirty
        // L1500
        self.SetDisplayGroup(build, self.socketGroupList.get().get(1));
        // L1501 UI: self.build:SyncLoadouts() (loadout dropdown, Build.lua:771)
    }
}
