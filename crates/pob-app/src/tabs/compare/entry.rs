//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/CompareEntry.lua 1-552. Owner: U47.
//!
//! @pob-file Classes/CompareEntry.lua @16de4b82d
//!
//! A lightweight Build wrapper for comparison: it loads the XML, creates the tabs and runs the
//! calculations without the primary build's UI chrome.
//!
//! # The object
//!
//! `self.compareEntries[i]` is U46's [`CompareEntryRef`] (CCR-0136 §1): `label`, `model` (the
//! entry's Build model: every field CompareEntry.lua keeps on the calc doc's `Build`, and its
//! tabs' model halves) and `ext`, which holds this file's [`CompareEntry`]: the UI-side fields
//! (`buildName`, `xmlText`, `notesText`, `xmlSectionList`), the entry TreeTab's `viewer` (a
//! [`PassiveTreeView`]: the `TreeView` saver loads it, the compare tree footers read and write
//! its `searchStr`, U49's DrawTree draws it) and the calc facade the entry's
//! `calcsTab:BuildOutput()` runs on (one per entry, from `calc_factory`, as each Lua entry has
//! its own CalcsTab). `newClass("CompareEntry", "ControlHost")`: the entry never holds controls
//! and nothing draws it, so it has no arena node and no `Widget` impl.
//!
//! Fields that CompareEntry.lua sets and the model keeps: `viewMode`, `characterLevel`,
//! `targetVersion`, `bandit`, `pantheonMajorGod`, `pantheonMinorGod`, `mainSocketGroup`,
//! `spectreList`, `latestTree`, `data`, `buildFlag`, `outputRevision` (model fields);
//! `characterLevelAutoMode`, `timelessData`, `importLink` (calc:BM-A's `build_extra` side table,
//! as `Build:Init` keeps them). `displayStats`/`minionDisplayStats`/`extraSaveStats` are U21's
//! `build::display_stats` module tables.
//!
//! # The entry's tabs (L106-116)
//!
//! `new("ConfigTab"):ConfigTab(self)` etc. are the tab classes' constructors run with the entry
//! as their build. Their model halves are calc:BM-A/BM-B's (`ctor_model`, `Load`, `PostLoad`,
//! `SetActiveSpec`, `SetActiveItemSet`, `SetActiveSkillSet`, `SetActiveConfigSet`,
//! `UpdateControls`, `BuildModList`) and run on the entry's model. Their UI halves (U25, U33,
//! U30, U39, U41: controls, the item slot controls, PopulateSlots' UI part, SetCompareSpec) are
//! built against the open build's node (`cx.build()`) and cannot be instantiated for a second
//! build; CompareTab never draws or reads them except:
//! * `entry.treeTab.viewer` (above) and `entry.treeTab:GetSpecList()` ([`GetSpecList`]: the
//!   TreeTab method run on the entry's spec list);
//! * `entry.treeTab:OpenVersionConvertPopup(version, true)` ([`OpenVersionConvertPopup`]: the
//!   TreeTab popup and its `ConvertToVersion` run on the entry's tree tab);
//! * `entry.configTab.varControls[var]` (CompareTab.lua:1208, the source of the compare config
//!   controls' tooltips): the entry has a UI ConfigTab (FUP6, closing CCR-0137 §2), U41's
//!   constructor for the entry's model ([`new_config_tab`]); its closures run with the entry
//!   swapped in ([`with_entry_as_build`]), so the copied `tooltipFunc` shows the option's help and
//!   the entry build's stat comparison. Its `UpdateControls`/`SetActiveConfigSet` calls from
//!   CompareTab stay the model halves ([`entry_config_update_controls`]): the entry's controls
//!   are never drawn, and the tooltips read the model (`input`, `placeholder`).
//! * `compareEntry.itemsTab` (CompareTab.lua:3988, PassiveTreeView.lua:1492/1783): U38's
//!   `AddItemTooltip` with the entry's `ItemsTabSelf` (FUP6, `tabs/items/tooltip.rs` module
//!   notes).
//! * the window title the entry TreeTab's `SetActiveSpec` sets (TreeTab.lua:556,
//!   `self.build.spec:SetWindowTitleWithBuildClass()` with `self.build` the entry, so its
//!   `buildName`): [`set_title`].
//!
//! The PartyTab stub (L109-111: an empty `enemyModList` and an actor with empty buff lists and a
//! ModDB) is the model's PartyTab after its constructor's model half (`ctor_model`: the same
//! empty lists, `modDB.actor`); the entry never loads a `Party` section (no saver), as in Lua.
//!
//! CCR-0137 §1: calc:BM-A keeps the constructor halves CompareEntry needs private
//! (`build_xml::skillsTab_ctor_model`, `ImportCalcSettings`, `new_timelessData`); the bodies
//! below repeat them (marked `// CCR-0137`).
//!
//! # Lua semantics
//!
//! * `main.defaultCharLevel` is `MainState::default_char_level` (nil → `None`).
//! * `tonumber(attr)` is `pob_lua::tonumber`; `xml.attrib[k] or "None"` reads the attribute.
//! * `socketGroupList[i]` / `displaySkillList[i]` index with a Lua number (nil unless an
//!   integer in range): `init::at_num`.
//! * `mainGroup.mainActiveSkill or 1`: the model keeps a nil number as NaN.
//! * `tostring(x or "")` of the stage/mine counts: `tostring_num` of a number.
//! * `controls.X.selIndex = n`: the dropdown's index (a non-integer or nil index selects
//!   nothing, as `list[selIndex]` is nil).
//!
//! ORDER: `pairs(self.savers)` (L155) calls `PostLoad` on the one saver that has it (TreeTab)
//! (order-free, (c)); `pairs({ "bandit", ... })` (L196) is an array (in order);
//! `pairs(self.skillsTab.socketGroupList)` (L294) is an array (in order).
//!
//! DIVERGENCE: none beyond the gaps above. Props with side effects (ui doc §7.3): none.

#![allow(non_snake_case)]

use std::cell::{Cell, Ref, RefCell};
use std::rc::Rc;

use pob_lua::math::{m_floor, m_max, m_min};
use pob_lua::{lfmt, n, tostring_num, Name, Shared};
use pob_sg::Font;
use pob_ui_core::controls::{ButtonControl, DropDownControl, DropItem, EditControl, LabelControl};
use pob_ui_core::luastr::format_num_sep_main;
use pob_ui_core::popups::{self, PopupOpts};
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::Tooltip;

use pob_calc::active_skill::ActiveSkillRef;
use pob_calc::actor::ActorRef;
use pob_calc::classes::passive_spec::new_PassiveSpec;

use super::init::{at_num, CompareEntryHandle, CompareEntryRef};
use crate::app::CxApp;
use crate::build::display_stats::{self, DisplayStat};
use crate::build::init::{self as build_init, MinionListValue};
use crate::calc::{CalcFacade, MiscCalc};
use crate::model::build_xml;
use crate::model::xml::{XmlChild, XmlNode};
use crate::model::{self, gamedata::ModType, Output, Table, Val};
use crate::tree_view::PassiveTreeView;

// @pob Classes/CompareEntry.lua:1-13 h=f76eefb1
// (header comment, the `local` aliases and `newClass("CompareEntry", "ControlHost")`: the
// struct below)
// @pob Classes/CompareEntry.lua:552-552 h=652e9b08
// (`return CompareEntryClass`: this module)

/// The CompareEntry fields that are not the model's (module notes), kept in
/// `CompareEntryHandle::ext`.
pub struct CompareEntry {
    /// `self.buildName` (`label or "Comparison Build"`)
    pub buildName: String,
    /// `self.xmlText`
    pub xmlText: Option<String>,
    /// `self.notesText`
    pub notesText: String,
    /// `self.xmlSectionList` (the sections LoadFromXML stored, document order)
    pub xmlSectionList: Vec<XmlNode>,
    /// `self.treeTab.viewer` (the entry TreeTab's PassiveTreeView)
    pub viewer: RefCell<PassiveTreeView>,
    /// the calc the entry's `calcsTab:BuildOutput()` runs on
    pub calc: RefCell<Box<dyn CalcFacade>>,
    /// `self.configTab.calcFunc`, `self.configTab.calcBase` (ConfigTab.lua:159-161, 756-757):
    /// the entry ConfigTab's misc calculator, in `BuildState::calc_func`/`calc_base` while the
    /// entry is swapped in ([`with_entry_as_build`])
    pub config_calc: RefCell<(Option<Rc<dyn MiscCalc>>, Option<Output>)>,
    /// `self.configTab` (L112): the entry's ConfigTab node (U41's constructor for the entry's
    /// model, [`new_config_tab`]); `None` until LoadFromXML builds it
    pub config_tab: Cell<Option<CtrlId>>,
}

/// The entry's [`CompareEntry`] (the handle's `ext`).
#[track_caller]
pub fn ext(entry: &CompareEntryRef) -> Ref<'_, CompareEntry> {
    Ref::map(entry.ext.borrow(), |e| {
        e.as_ref()
            .and_then(|b| b.downcast_ref::<CompareEntry>())
            .unwrap_or_else(|| lua_error!("CompareEntry: the entry has no CompareEntry state"))
    })
}

/// `ConPrintf(fmt, ...)`
fn ConPrintf(cx: &mut Cx, s: &str) {
    cx.sys.platform.con_print(s);
}

/// The window title the model queued for the entry's spec (`main:SetWindowTitleSubtext(
/// s_format("%s (%s)", self.build.buildName, classText))`, PassiveSpec.lua:2395, with
/// `self.build` the entry): the queue is flushed with the entry's `buildName`.
fn flush_titles_as(cx: &mut Cx, buildName: &str) {
    let saved = cx.build_mut().build_name.replace(buildName.to_owned());
    build_init::flush_window_titles(cx);
    cx.build_mut().build_name = saved;
}

/// Run `f` with the entry as `self.build` of the code it calls: the open build's `model`, calc
/// facade, `build_name` and ConfigTab misc calculator (`calc_func`/`calc_base`) are swapped
/// for the entry's (its [`CompareEntry`] holds the open build's meanwhile) and swapped back
/// afterwards, also when `f` raises (`catch_unwind` + `resume_unwind`; wasm32 aborts on panic).
/// Nested calls for the same entry run `f` directly. CCR-0143 §2 (U49's tree draw), FUP6 (the
/// entry's ItemsTab and ConfigTab methods).
pub fn with_entry_as_build<R>(cx: &mut Cx, entry: &CompareEntryRef, f: impl FnOnce(&mut Cx) -> R) -> R {
    if Rc::ptr_eq(&cx.build().model, &entry.model) {
        return f(cx);
    }
    let swap = |cx: &mut Cx| {
        let e = ext(entry);
        let b = cx.build_mut();
        std::mem::swap(&mut b.calc, &mut *e.calc.borrow_mut());
        let mut cc = e.config_calc.borrow_mut();
        std::mem::swap(&mut b.calc_func, &mut cc.0);
        std::mem::swap(&mut b.calc_base, &mut cc.1);
    };
    let name = Some(ext(entry).buildName.clone());
    let saved_model = std::mem::replace(&mut cx.build_mut().model, entry.model.clone());
    let saved_name = std::mem::replace(&mut cx.build_mut().build_name, name);
    swap(cx);
    #[cfg(not(target_arch = "wasm32"))]
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(cx)));
    #[cfg(target_arch = "wasm32")]
    let r: Result<R, ()> = Ok(f(cx));
    swap(cx);
    cx.build_mut().model = saved_model;
    cx.build_mut().build_name = saved_name;
    match r {
        Ok(r) => r,
        #[cfg(not(target_arch = "wasm32"))]
        Err(p) => std::panic::resume_unwind(p),
        #[cfg(target_arch = "wasm32")]
        Err(()) => unreachable!(),
    }
}

/// `self.configTab = new("ConfigTab"):ConfigTab(self)` (L112): U41's constructor for the
/// entry's model ([`ConfigTab::new_for`]), run with the entry swapped in, its closures (the
/// option tooltips CompareTab copies, CompareTab.lua:1170-1173) run the same way, and
/// `self.build:AddStatComparesToTooltip` CompareEntry's. The node is owned by the open build's
/// BuildMode node (freed with the build; the compare config controls keep the entry's tooltip
/// closures, which read these controls, as Lua's references keep them alive).
/// DIVERGENCE(init-order): built after the entry's sections are loaded, as the open build's
/// ConfigTab UI half (`tabs/config/ctor.rs` module notes).
fn new_config_tab(cx: &mut Cx, entry: &CompareEntryRef) -> CtrlId {
    use crate::tabs::config::ctor::ForBuild;
    use crate::tabs::config::ConfigTab;
    let weak = Rc::downgrade(entry);
    let run_entry = weak.clone();
    let stat_entry = weak;
    let for_build = ForBuild {
        run: Rc::new(move |cx: &mut Cx, f: &mut dyn FnMut(&mut Cx)| {
            match run_entry.upgrade() {
                Some(e) => with_entry_as_build(cx, &e, f),
                None => f(cx),
            }
        }),
        add_stat_compares: Rc::new(
            move |cx: &mut Cx, tooltip: &mut Tooltip, base: &Output, output: &Output, header: &str| {
                let e = stat_entry
                    .upgrade()
                    .unwrap_or_else(|| lua_error!("CompareEntry: the entry was dropped"));
                CompareEntry::AddStatComparesToTooltip(cx, &e, tooltip, base, output, header, None)
            },
        ),
    };
    let owner = cx.app().build.as_ref().map(|b| b.node.id());
    with_entry_as_build(cx, entry, |cx| cx.add_free(owner, ConfigTab::new_for(CtrlId::default(), for_build)).id())
}

/// `self.build.spec:SetWindowTitleWithBuildClass()` of the entry's TreeTab:SetActiveSpec
/// (TreeTab.lua:556).
fn set_title(cx: &mut Cx, m: &model::BuildRef, buildName: &str) {
    m.spec.get().SetWindowTitleWithBuildClass();
    flush_titles_as(cx, buildName);
}

/// `xml.attrib[k]`
fn attr<'a>(xml: &'a XmlNode, k: &str) -> Option<&'a str> {
    xml.attrib.get(k).map(|v| &**v)
}

/// `tonumber(s)` of an optional attribute.
fn tonum(s: Option<&str>) -> Option<f64> {
    s.and_then(pob_lua::tonumber)
}

/// `{ [k] = v }` (a nil `v` leaves the table empty).
fn id_table(k: Name, v: Option<f64>) -> Table {
    let t = Table::new();
    if let Some(v) = v {
        t.set(k, v);
    }
    t
}

/// The control map `RefreshSkillSelectControls(controls, ...)` receives (CompareTab.lua:2248,
/// 2264, 2281: `{ mainSocketGroup = self.controls.X, ... }`). `mainSkillMinionLibrary` (`{ shown
/// = false }`) is never read by CompareEntry's version.
#[derive(Clone, Copy)]
pub struct SkillControls {
    pub mainSocketGroup: CtrlId,
    pub mainSkill: CtrlId,
    pub mainSkillPart: CtrlId,
    pub mainSkillStageCount: CtrlId,
    pub mainSkillMineCount: CtrlId,
    pub mainSkillMinion: CtrlId,
    pub mainSkillMinionSkill: CtrlId,
}

fn set_shown(cx: &mut Cx, id: CtrlId, shown: bool) {
    cx.base_mut(id).shown = Prop::Val(shown);
}

fn set_enabled(cx: &mut Cx, id: CtrlId, enabled: bool) {
    cx.base_mut(id).enabled = Prop::Val(enabled);
}

/// `control.selIndex = n` for a Lua number.
fn sel(n: f64) -> usize {
    if n.is_finite() && n >= 0.0 && n.fract() == 0.0 {
        n as usize
    } else {
        0
    }
}

/// A nil `Cell<f64>` is NaN.
fn opt(n: f64) -> Option<f64> {
    (!n.is_nan()).then_some(n)
}

/// `x.k` of an explode source (an item, a passive node, or a table).
fn source_field(src: &Val, k: &str) -> Val {
    if let Some(item) = src.obj::<model::Item>() {
        return match k {
            "name" => Val::from(item.name.get()),
            "rarity" => Val::from(item.rarity.get()),
            _ => item.extra.get(Name::from(k)),
        };
    }
    if let Some(node) = src.obj::<model::Node>() {
        // the Node type has no `name`; PassiveTree.lua:534 sets `dn = name`
        return match k {
            "name" | "dn" => Val::from(node.dn.get()),
            _ => Val::Nil,
        };
    }
    src.get(Name::from(k))
}

/// `x or y` on values.
fn lor(a: Val, b: impl FnOnce() -> Val) -> Val {
    if a.truthy() {
        a
    } else {
        b()
    }
}

/// `tostring(v)` of a number or string (the stage/mine count fallbacks).
fn tostring_val(v: &Val) -> String {
    match v {
        Val::Nil => "nil".to_owned(),
        Val::Bool(b) => b.to_string(),
        Val::Num(n) => tostring_num(*n),
        Val::Str(s) => s.to_string(),
        other => lua_error!("tostring of a {} value", other.lua_type()),
    }
}

impl CompareEntry {
    /// lua: CompareEntry.lua:15-61 `CompareEntryClass:CompareEntry(xmlText, label)`
    /// `None` when the entry has no `calcsTab` (`xmlText` nil, or LoadFromXML returned before
    /// creating the tabs): CompareTab's ImportBuild drops such an entry (CCR-0136 §1).
    /// @pob Classes/CompareEntry.lua:15-61 h=4ea95c1e
    pub fn new(cx: &mut Cx, xmlText: Option<&str>, label: Option<&str>) -> Option<CompareEntryRef> {
        let gv = &model::data().gameVersions;
        // titles the open build queued before this (theirs)
        build_init::flush_window_titles(cx);
        // L16 self:ControlHost() (no controls, module notes)
        let m = model::Build::new(model::data());
        let extra = build_xml::build_extra(&m);
        // L18
        let label = label.unwrap_or("Comparison Build").to_owned();
        // L19
        let buildName = label.clone();
        // L20 self.xmlText = xmlText (the struct below)

        // Default build properties
        // L23
        m.viewMode.set(n!("TREE"));
        // L24
        let dcl = cx.app().main.default_char_level;
        m.characterLevel.set(m_min(m_max(dcl.unwrap_or(1.0), 1.0), 100.0));
        // L25
        m.targetVersion.set(gv.liveTargetVersion);
        // L26-28
        m.bandit.set(n!("None"));
        m.pantheonMajorGod.set(n!("None"));
        m.pantheonMinorGod.set(n!("None"));
        // L29
        extra.set(n!("characterLevelAutoMode"), dcl == Some(1.0) || dcl.is_none());
        // L30
        m.mainSocketGroup.set(1.0);
        // L31 self.notesText = "" (the struct below)

        // L33
        m.spectreList.set(Shared::new(Vec::new()));
        // L34-40 (CCR-0137 §1: build_xml::new_timelessData)
        let timelessData = Table::new();
        timelessData.set(n!("jewelType"), Table::new());
        timelessData.set(n!("conquerorType"), Table::new());
        timelessData.set(n!("devotionVariant1"), 1.0);
        timelessData.set(n!("devotionVariant2"), 1.0);
        timelessData.set(n!("jewelSocket"), Table::new());
        timelessData.set(n!("fallbackWeightMode"), Table::new());
        timelessData.set(n!("searchList"), "");
        timelessData.set(n!("searchListFallback"), "");
        timelessData.set(n!("searchResults"), Table::new());
        timelessData.set(n!("sharedResults"), Table::new());
        extra.set(n!("timelessData"), timelessData);

        // Shared data (read-only references)
        // L43
        m.latestTree
            .set(pob_calc::classes::passive_spec::main_LoadTree(gv.latestTreeVersion));
        // L44 self.data = data (the model's `data`)

        // Flags
        // L47
        m.buildFlag.set(false);
        // L48
        m.outputRevision.set(1.0);

        // Display stats (same as primary build uses)
        // L51-54: `display_stats::{displayStats, minionDisplayStats, extraSaveStats}`

        let calc = (cx.app().calc_factory)();
        let viewer = PassiveTreeView::new(cx);
        let entry: CompareEntryRef = Rc::new(CompareEntryHandle {
            label,
            model: m,
            ext: RefCell::new(Some(Box::new(CompareEntry {
                buildName,
                xmlText: xmlText.map(str::to_owned),
                notesText: String::new(),
                xmlSectionList: Vec::new(),
                viewer: RefCell::new(viewer),
                calc: RefCell::new(calc),
                config_calc: RefCell::new((None, None)),
                config_tab: Cell::new(None),
            }))),
        });

        // Load from XML
        // L57
        if let Some(xmlText) = xmlText {
            // L58
            if Self::LoadFromXML(cx, &entry, xmlText) {
                return None;
            }
            // L60
            return Some(entry);
        }
        // L60 (no XML: the entry has no calcsTab)
        None
    }

    /// lua: CompareEntry.lua:63-186 `CompareEntryClass:LoadFromXML(xmlText)`
    /// → `true` when it returned before creating the tabs.
    /// @pob Classes/CompareEntry.lua:63-186 h=dd5c2d63
    pub fn LoadFromXML(cx: &mut Cx, entry: &CompareEntryRef, xmlText: &str) -> bool {
        let gv = &model::data().gameVersions;
        let m = entry.model.clone();
        let extra = build_xml::build_extra(&m);
        let buildName = ext(entry).buildName.clone();
        // Parse the XML
        // L65
        let (dbXML, errMsg) = model::xml::ParseXML(xmlText);
        // L66
        if let Some(errMsg) = errMsg {
            // L67
            ConPrintf(cx, &format!("CompareEntry: Error parsing XML: {errMsg}"));
            // L68
            return true;
        }
        // L70
        let Some(root) = dbXML.first().filter(|r| &*r.elem == "PathOfBuilding") else {
            // L71
            ConPrintf(cx, "CompareEntry: 'PathOfBuilding' root element missing");
            // L72
            return true;
        };
        let nodes: Vec<&XmlNode> = root
            .children
            .iter()
            .filter_map(|c| match c {
                XmlChild::Node(n) => Some(n),
                XmlChild::Text(_) => None,
            })
            .collect();

        // Load Build section first
        // L76
        for node in &nodes {
            // L77
            if &*node.elem == "Build" {
                // L78
                Self::LoadBuildSection(&m, node);
                // L79
                break;
            }
        }

        // Check for import link
        // L84
        for node in &nodes {
            // L85
            if &*node.elem == "Import" {
                // L86
                if let Some(link) = attr(node, "importLink") {
                    // L87
                    extra.set(n!("importLink"), link);
                }
                // L89
                break;
            }
        }

        // Store XML sections for tab loading
        // L94-99
        let xmlSectionList: Vec<XmlNode> = nodes.iter().map(|n| (*n).clone()).collect();
        entry.ext.borrow_mut().as_mut().and_then(|b| b.downcast_mut::<CompareEntry>()).expect("ext").xmlSectionList =
            xmlSectionList.clone();

        // Version check
        // L102
        if m.targetVersion.get() != gv.liveTargetVersion {
            // L103
            m.targetVersion.set(gv.liveTargetVersion);
        }

        // Create tabs
        // PartyTab is replaced with a stub providing an empty enemyModList and actor
        // (CalcPerform.lua:1088 accesses build.partyTab.actor for party member buffs)
        // L109-111 (module notes)
        m.partyTab.ctor_model();
        // L112
        m.configTab.ctor_model();
        m.configTab.ctor_controls_model(m.data);
        // L113
        m.itemsTab.ctor_model(&m);
        // L114 (TreeTab.lua:26-38: the viewer is the struct's; L37 SetActiveSpec(1) sets the
        // window title, L556)
        m.treeTab.ctor_model(&m);
        set_title(cx, &m, &buildName);
        // L115 (SkillsTab.lua:89, 95-96, 423-426; CCR-0137 §1: build_xml::skillsTab_ctor_model)
        {
            let st = &m.skillsTab;
            // SkillsTab.lua:89
            st.socketGroupList.set(Shared::new(Vec::new()));
            // SkillsTab.lua:95
            st.defaultGemLevel.set(Some(n!("normalMaximum")));
            // SkillsTab.lua:96
            st.defaultGemQuality.set(cx.app().main.default_gem_quality);
            // SkillsTab.lua:423
            st.skillSets.with_mut(|s| s.clear());
            // SkillsTab.lua:424
            st.skillSetOrderList.wipe();
            st.skillSetOrderList.push(1);
            // SkillsTab.lua:425
            st.NewSkillSet(Some(1.0));
            // SkillsTab.lua:426
            st.SetActiveSkillSet(&m, 1.0);
        }
        // L116
        m.calcsTab.ctor_model();

        // Set up savers table
        // L119-129: the section loaders below (Config, Tree, TreeView, Items, Skills, Calcs;
        // the legacy loader Spec)

        // Special rebuild to properly initialise boss placeholders
        // L132
        m.configTab.BuildModList(&m);

        // Load legacy bandit and pantheon choices from build section
        // L135
        for (control, v) in [
            (n!("bandit"), m.bandit.get()),
            (n!("pantheonMajorGod"), m.pantheonMajorGod.get()),
            (n!("pantheonMinorGod"), m.pantheonMinorGod.get()),
        ] {
            // L136
            m.configTab.input.set(control, v);
        }

        // Load XML sections into tabs
        // Defer passive trees until after items are loaded (jewel socket issue)
        // L141
        let mut deferredPassiveTrees: Vec<&XmlNode> = Vec::new();
        // L142
        for node in &xmlSectionList {
            // L143
            let elem = &*node.elem;
            // L144-150
            match elem {
                // L145-146 (saver == self.treeTab)
                "Tree" | "Spec" => deferredPassiveTrees.push(node),
                // L148
                "Config" => m.configTab.Load(&m, node, "CompareEntry"),
                "TreeView" => {
                    let e = ext(entry);
                    e.viewer.borrow_mut().load(node, Some("CompareEntry"));
                }
                "Items" => m.itemsTab.Load(&m, node, "CompareEntry"),
                "Skills" => m.skillsTab.Load(&m, node, "CompareEntry"),
                "Calcs" => m.calcsTab.Load(&m, node, "CompareEntry"),
                _ => {}
            }
        }
        // L152
        for node in deferredPassiveTrees {
            // L153 (TreeTab.lua:500-527: L526 SetActiveSpec sets the window title, L556; the
            // legacy `Spec` branch returns before it, and so does an unknown tree version)
            let failed = m.treeTab.Load_ret(&m, node, "CompareEntry");
            flush_titles_as(cx, &buildName);
            if &*node.elem != "Spec" && !failed {
                set_title(cx, &m, &buildName);
            }
        }
        // L155-159 (TreeTab is the only saver with a PostLoad; TreeTab.lua:533's
        // `itemsTab:PopulateSlots()` UI part has no entry controls)
        m.treeTab.PostLoad(&m);

        // Extract notes from the build XML
        // L162
        for node in &xmlSectionList {
            // L163
            if &*node.elem == "Notes" {
                // L164
                for child in &node.children {
                    // L165
                    if let XmlChild::Text(s) = child {
                        // L166
                        let notes = s.to_string();
                        entry.ext.borrow_mut().as_mut().and_then(|b| b.downcast_mut::<CompareEntry>()).expect("ext").notesText =
                            notes;
                        // L167
                        break;
                    }
                }
                // L170
                break;
            }
        }

        // L174
        if m.configTab.input.pairs().next().is_none() {
            // L175-176 (ConfigTab.lua:1205-1240; CCR-0137 §1: build_xml::ImportCalcSettings)
            ImportCalcSettings(&m);
        }

        // L180
        Self::SyncCalcsSkillSelection(entry);
        // reprocess socket groups as they might depend on items which don't necessarily load first.
        // L182
        m.skillsTab.UpdateSocketGroups(&m);
        // L183
        pob_calc::modules::common::wipeGlobalCache();
        // L184
        Self::BuildOutput(entry);
        // L112 (the UI half; DIVERGENCE(init-order), `new_config_tab`; outside BUILD mode, where
        // no CompareTab exists, there is no build to swap with)
        if cx.app().build.is_some() {
            let config_tab = new_config_tab(cx, entry);
            ext(entry).config_tab.set(Some(config_tab));
        }
        // L185
        m.buildFlag.set(false);
        flush_titles_as(cx, &buildName);
        false
    }

    /// `self.calcsTab:BuildOutput()` on the entry's calc.
    fn BuildOutput(entry: &CompareEntryRef) {
        let e = ext(entry);
        let _ = e.calc.borrow_mut().build_output(&entry.model);
    }

    /// lua: CompareEntry.lua:189-219 `CompareEntryClass:LoadBuildSection(xml)`
    /// Load build section attributes
    /// @pob Classes/CompareEntry.lua:189-219 h=8c23ab23
    pub fn LoadBuildSection(m: &model::BuildRef, xml: &XmlNode) {
        let gv = &model::data().gameVersions;
        let extra = build_xml::build_extra(m);
        // L190
        m.targetVersion
            .set(attr(xml, "targetVersion").map_or(gv.legacyTargetVersion, Name::from));
        // L191
        if let Some(v) = attr(xml, "viewMode") {
            // L192
            m.viewMode.set(Name::from(v));
        }
        // L194
        m.characterLevel.set(tonum(attr(xml, "level")).unwrap_or(1.0));
        // L195
        extra.set(
            n!("characterLevelAutoMode"),
            attr(xml, "characterLevelAutoMode") == Some("true"),
        );
        // L196 (an array: `pairs` visits it in order)
        for diff in ["bandit", "pantheonMajorGod", "pantheonMinorGod"] {
            // L197
            let v = Name::from(attr(xml, diff).unwrap_or("None"));
            match diff {
                "bandit" => m.bandit.set(v),
                "pantheonMajorGod" => m.pantheonMajorGod.set(v),
                _ => m.pantheonMinorGod.set(v),
            }
        }
        // L199
        m.mainSocketGroup.set(
            tonum(attr(xml, "mainSkillIndex"))
                .or_else(|| tonum(attr(xml, "mainSocketGroup")))
                .unwrap_or(1.0),
        );
        // L200
        m.spectreList.get().wipe();
        // L201
        for child in &xml.children {
            let XmlChild::Node(child) = child else {
                continue;
            };
            // L202
            if &*child.elem == "Spectre" {
                // L203
                if let Some(id) = attr(child, "id") {
                    if m.data.minions.contains_key(&Name::from(id)) {
                        // L204
                        m.spectreList.get().push(Name::from(id));
                    }
                }
            // L206
            } else if &*child.elem == "TimelessData" {
                let td = build_xml::timelessData(m);
                // L207
                td.set(n!("jewelType"), id_table(n!("id"), tonum(attr(child, "jewelTypeId"))));
                // L208
                td.set(
                    n!("conquerorType"),
                    id_table(n!("id"), tonum(attr(child, "conquerorTypeId"))),
                );
                // L209
                td.set(n!("devotionVariant1"), tonum(attr(child, "devotionVariant1")).unwrap_or(1.0));
                // L210
                td.set(n!("devotionVariant2"), tonum(attr(child, "devotionVariant2")).unwrap_or(1.0));
                // L211
                td.set(n!("jewelSocket"), id_table(n!("id"), tonum(attr(child, "jewelSocketId"))));
                // L212
                td.set(
                    n!("fallbackWeightMode"),
                    id_table(n!("idx"), tonum(attr(child, "fallbackWeightModeIdx"))),
                );
                // L213
                td.set(n!("socketFilter"), attr(child, "socketFilter") == Some("true"));
                // L214
                td.set(
                    n!("socketFilterDistance"),
                    tonum(attr(child, "socketFilterDistance")).unwrap_or(0.0),
                );
                // L215
                td.set(n!("searchList"), attr(child, "searchList").map_or(Val::Nil, Val::from));
                // L216
                td.set(
                    n!("searchListFallback"),
                    attr(child, "searchListFallback").map_or(Val::Nil, Val::from),
                );
            }
        }
    }

    /// lua: CompareEntry.lua:221-223 `CompareEntryClass:GetOutput()`
    /// @pob Classes/CompareEntry.lua:221-223 h=e53f6a93
    pub fn GetOutput(entry: &CompareEntryRef) -> Option<Output> {
        // L222
        entry.model.calcsTab.mainOutput.get()
    }

    /// lua: CompareEntry.lua:225-227 `CompareEntryClass:GetSpec()`
    /// `self.spec` (the entry Build's active spec, as TreeTab:SetActiveSpec sets it).
    /// @pob Classes/CompareEntry.lua:225-227 h=83420788
    pub fn GetSpec(entry: &CompareEntryRef) -> model::PassiveSpecRef {
        // L226
        entry.model.spec.get()
    }

    /// lua: CompareEntry.lua:229-248 `CompareEntryClass:SyncCalcsSkillSelection()`
    /// @pob Classes/CompareEntry.lua:229-248 h=24d6d097
    pub fn SyncCalcsSkillSelection(entry: &CompareEntryRef) {
        let m = &entry.model;
        // L230
        m.calcsTab
            .input
            .set(n!("skill_number"), m.mainSocketGroup.get());

        // L232
        let mainGroup = m
            .skillsTab
            .socketGroupList
            .get()
            .with(|l| at_num(l, m.mainSocketGroup.get()));
        // L233
        let Some(mainGroup) = mainGroup else { return };

        // L235
        mainGroup.mainActiveSkillCalcs.set(mainGroup.mainActiveSkill.get());

        // L237
        let displaySkillList = mainGroup.displaySkillList.get();
        // L238
        let activeSkill = displaySkillList
            .and_then(|l| l.with(|l| at_num(l, opt(mainGroup.mainActiveSkill.get()).unwrap_or(1.0))));
        // L239
        if let Some(activeSkill) = activeSkill {
            if let Some(src) = activeSkill.activeEffect.get().srcInstance.get() {
                // L240 local src = activeSkill.activeEffect.srcInstance
                // L241
                src.skillPartCalcs.set(src.skillPart.get());
                // L242
                src.skillStageCountCalcs.set(src.skillStageCount.get());
                // L243
                src.skillMineCountCalcs.set(src.skillMineCount.get());
                // L244
                src.skillMinionCalcs.set(src.skillMinion.get());
                // L245
                src.skillMinionItemSetCalcs.set(src.skillMinionItemSet.get());
                // L246
                src.skillMinionSkillCalcs.set(src.skillMinionSkill.get());
            }
        }
    }

    /// lua: CompareEntry.lua:250-256 `CompareEntryClass:Rebuild()`
    /// @pob Classes/CompareEntry.lua:250-256 h=8f212546
    pub fn Rebuild(entry: &CompareEntryRef) {
        let m = &entry.model;
        // L251
        pob_calc::modules::common::wipeGlobalCache();
        // L252
        m.outputRevision.set(m.outputRevision.get() + 1.0);
        // L253
        m.skillsTab.UpdateSocketGroups(m);
        // L254
        Self::BuildOutput(entry);
        // L255
        m.buildFlag.set(false);
    }

    /// lua: CompareEntry.lua:258-263 `CompareEntryClass:SetActiveSpec(index)`
    /// `self.treeTab:SetActiveSpec(index)`: the model half (calc:BM-A) and the window title
    /// (TreeTab.lua:556; module notes).
    /// @pob Classes/CompareEntry.lua:258-263 h=061e4215
    pub fn SetActiveSpec(cx: &mut Cx, entry: &CompareEntryRef, index: f64) {
        let m = entry.model.clone();
        // L259 (the entry always has a treeTab with SetActiveSpec)
        // L260
        m.treeTab.SetActiveSpec(&m, index);
        let buildName = ext(entry).buildName.clone();
        set_title(cx, &m, &buildName);
        // L261
        Self::Rebuild(entry);
    }

    /// lua: CompareEntry.lua:265-270 `CompareEntryClass:SetActiveItemSet(id)`
    /// @pob Classes/CompareEntry.lua:265-270 h=b5b3cc76
    pub fn SetActiveItemSet(entry: &CompareEntryRef, id: f64) {
        let m = &entry.model;
        // L266-267
        m.itemsTab.SetActiveItemSet(m, id);
        // L268
        Self::Rebuild(entry);
    }

    /// lua: CompareEntry.lua:272-277 `CompareEntryClass:SetActiveSkillSet(id)`
    /// @pob Classes/CompareEntry.lua:272-277 h=ce002f77
    pub fn SetActiveSkillSet(entry: &CompareEntryRef, id: f64) {
        let m = &entry.model;
        // L273-274
        m.skillsTab.SetActiveSkillSet(m, id);
        // L275
        Self::Rebuild(entry);
    }

    /// lua: CompareEntry.lua:280-282 `CompareEntryClass:RefreshStatList()`
    /// Stub methods that the build interface may call
    /// @pob Classes/CompareEntry.lua:280-282 h=fe4f8c9a
    pub fn RefreshStatList(_entry: &CompareEntryRef) {
        // No sidebar to refresh in comparison entry
    }

    /// lua: CompareEntry.lua:284-287 `CompareEntryClass:SetMainSocketGroup(index)`
    /// @pob Classes/CompareEntry.lua:284-287 h=bb2f6335
    pub fn SetMainSocketGroup(entry: &CompareEntryRef, index: f64) {
        // L285
        entry.model.mainSocketGroup.set(index);
        // L286
        entry.model.buildFlag.set(true);
    }

    /// lua: CompareEntry.lua:289-377 `CompareEntryClass:RefreshSkillSelectControls(controls,
    /// mainGroup, suffix)`
    /// @pob Classes/CompareEntry.lua:289-377 h=7152de23
    #[allow(clippy::too_many_lines)]
    pub fn RefreshSkillSelectControls(
        cx: &mut Cx,
        entry: &CompareEntryRef,
        controls: &SkillControls,
        mainGroup: Val,
        suffix: &str,
    ) {
        let calcs = match suffix {
            "" => false,
            "Calcs" => true,
            other => lua_error!("RefreshSkillSelectControls: unknown suffix {other:?}"),
        };
        let m = entry.model.clone();
        // Populate skill select controls
        // L291 (the map always holds mainSocketGroup)
        // L292
        cx.w_mut::<DropDownControl>(controls.mainSocketGroup).sel_index = mainGroup.opt_num().map_or(0, sel);
        // L293
        cx.w_mut::<DropDownControl>(controls.mainSocketGroup).list.clear();
        // L294 (ORDER: `pairs` over the group list's array part visits 1..n, (c))
        let groups: Vec<model::SocketGroupRef> = m.skillsTab.socketGroupList.get().snapshot();
        for (i, socketGroup) in groups.iter().enumerate() {
            // L295
            let label = socketGroup
                .displayLabel
                .get()
                .map(|s| s.to_string())
                .unwrap_or_default();
            cx.w_mut::<DropDownControl>(controls.mainSocketGroup)
                .list
                .push(DropItem::new(label, Rc::new((i + 1) as f64)));
        }
        // L297
        DropDownControl::check_dropped_width(cx, controls.mainSocketGroup, true);

        // Helper: hide all skill detail controls
        // L300-307
        let hideAllSkillControls = |cx: &mut Cx| {
            set_shown(cx, controls.mainSkill, false);
            set_shown(cx, controls.mainSkillPart, false);
            set_shown(cx, controls.mainSkillMineCount, false);
            set_shown(cx, controls.mainSkillStageCount, false);
            set_shown(cx, controls.mainSkillMinion, false);
            set_shown(cx, controls.mainSkillMinionSkill, false);
        };

        // L309
        if cx.w::<DropDownControl>(controls.mainSocketGroup).list.is_empty() {
            // L310
            cx.w_mut::<DropDownControl>(controls.mainSocketGroup)
                .list
                .push(DropItem::new("<No skills added yet>", Rc::new(1.0)));
            // L311
            hideAllSkillControls(cx);
            // L312
            return;
        }

        // L315
        let mut mainSocketGroup = mainGroup.opt_num().and_then(|g| at_num(&groups, g));
        // L316
        if mainSocketGroup.is_none() {
            // L317
            mainSocketGroup = groups.first().cloned();
            // L318 mainGroup = 1 (not read again)
        }
        let mainSocketGroup = mainSocketGroup.expect("a non-empty group list");
        // L320
        let displaySkillList = if calcs {
            mainSocketGroup.displaySkillListCalcs.get()
        } else {
            mainSocketGroup.displaySkillList.get()
        };
        // L321
        let Some(displaySkillList) = displaySkillList else {
            // L322
            hideAllSkillControls(cx);
            // L323
            return;
        };
        let displaySkillList: Vec<ActiveSkillRef> = displaySkillList.snapshot();

        // Populate main skill dropdown
        // L327
        let mainActiveSkill = opt(if calcs {
            mainSocketGroup.mainActiveSkillCalcs.get()
        } else {
            mainSocketGroup.mainActiveSkill.get()
        })
        .unwrap_or(1.0);
        // L328
        cx.w_mut::<DropDownControl>(controls.mainSkill).list.clear();
        // L329
        for (i, activeSkill) in displaySkillList.iter().enumerate() {
            let activeEffect = activeSkill.activeEffect.get();
            // L330
            let srcInstance = activeEffect
                .srcInstance
                .get()
                .unwrap_or_else(|| lua_error!("attempt to index field 'srcInstance' (a nil value)"));
            let explodeSource = srcInstance.extra.get(Name::from("explodeSource"));
            // L331
            let explodeSourceName = if explodeSource.truthy() {
                lor(source_field(&explodeSource, "name"), || source_field(&explodeSource, "dn"))
            } else {
                explodeSource.clone()
            };
            // L332
            let colourCoded = if explodeSourceName.truthy() {
                let rarity = lor(source_field(&explodeSource, "rarity"), || Val::from("NORMAL"));
                let code = match &rarity {
                    Val::Str(r) => cx.main.cc.get(r).cloned(),
                    _ => None,
                }
                .unwrap_or_else(|| lua_error!("attempt to concatenate a nil value"));
                Some(format!("From {code}{}", tostring_val(&explodeSourceName)))
            } else {
                None
            };
            // L333
            let label = colourCoded.unwrap_or_else(|| activeEffect.grantedEffect.get().name.to_string());
            cx.w_mut::<DropDownControl>(controls.mainSkill)
                .list
                .push(DropItem::new(label, Rc::new((i + 1) as f64)));
        }
        // L335
        set_enabled(cx, controls.mainSkill, displaySkillList.len() > 1);
        // L336
        cx.w_mut::<DropDownControl>(controls.mainSkill).sel_index = sel(mainActiveSkill);
        // L337
        set_shown(cx, controls.mainSkill, true);
        // L338
        hideAllSkillControls(cx);
        // L339 restore after hideAll
        set_shown(cx, controls.mainSkill, true);

        // L341
        let activeSkill =
            at_num(&displaySkillList, mainActiveSkill).or_else(|| displaySkillList.first().cloned());
        // L342
        let Some(activeSkill) = activeSkill else { return };
        // L343
        let activeEffect = activeSkill.activeEffect.get();
        // L344 (`activeEffect` is always set)
        let grantedEffect = activeEffect.grantedEffect.get();
        let srcInstance = || {
            activeEffect
                .srcInstance
                .get()
                .unwrap_or_else(|| lua_error!("attempt to index field 'srcInstance' (a nil value)"))
        };
        let skillFlags = activeSkill.skillFlags.get();
        let skillData = activeSkill.skillData.get();
        let parts = &grantedEffect.parts;

        // Skill parts
        // L347
        if parts.len() > 1 {
            // L348
            set_shown(cx, controls.mainSkillPart, true);
            // L349
            cx.w_mut::<DropDownControl>(controls.mainSkillPart).list.clear();
            // L350
            for (i, part) in parts.iter().enumerate() {
                // L351
                cx.w_mut::<DropDownControl>(controls.mainSkillPart)
                    .list
                    .push(DropItem::new(part.name.to_string(), Rc::new((i + 1) as f64)));
            }
            // L353
            let src = srcInstance();
            let part_sel = if calcs { src.skillPartCalcs.get() } else { src.skillPart.get() }.unwrap_or(1.0);
            cx.w_mut::<DropDownControl>(controls.mainSkillPart).sel_index = sel(part_sel);
            // L354
            let selectedPart = at_num(parts, part_sel);
            // L355
            if selectedPart
                .as_ref()
                .is_some_and(|p| p.flags.iter().any(|(n, v)| n.as_str() == "stages" && *v))
            {
                // L356
                set_shown(cx, controls.mainSkillStageCount, true);
                // L357 (`selectedPart.stagesMin` is never set in the data: SkillPart has no
                // such field)
                let count = if calcs { src.skillStageCountCalcs.get() } else { src.skillStageCount.get() };
                let v = count.map_or(Val::Num(1.0), Val::Num);
                cx.w_mut::<EditControl>(controls.mainSkillStageCount).buf = tostring_val(&v);
            }
        }

        // Mine count
        // L362
        if skillFlags.truthy(n!("mine")) {
            // L363
            set_shown(cx, controls.mainSkillMineCount, true);
            // L364
            let src = srcInstance();
            let count = if calcs { src.skillMineCountCalcs.get() } else { src.skillMineCount.get() };
            let v = count.map_or(Val::from(""), Val::Num);
            cx.w_mut::<EditControl>(controls.mainSkillMineCount).buf = tostring_val(&v);
        }

        // Stage count (for multi-stage skills without parts)
        // L368
        if skillFlags.truthy(n!("multiStage")) && parts.len() <= 1 {
            // L369
            set_shown(cx, controls.mainSkillStageCount, true);
            // L370
            let src = srcInstance();
            let count = if calcs { src.skillStageCountCalcs.get() } else { src.skillStageCount.get() };
            let v = lor(count.map_or(Val::Nil, Val::Num), || {
                lor(skillData.get(n!("stagesMin")), || Val::Num(1.0))
            });
            cx.w_mut::<EditControl>(controls.mainSkillStageCount).buf = tostring_val(&v);
        }

        // Minion controls
        // L374 (`grantedEffect.minionList` nil vs `{ }`: calc:AS's minionList_is_set,
        // CCR-0103 §3)
        let ge_minion_list_set =
            pob_calc::modules::calc_active_skill::minionList_is_set(&grantedEffect);
        let activeMinionList: Vec<Name> = activeSkill.minionList.get().snapshot();
        if !skillFlags.truthy(n!("disable")) && (ge_minion_list_set || !activeMinionList.is_empty()) {
            // L375
            Self::RefreshMinionControls(cx, entry, controls, &activeSkill, suffix);
        }
    }

    /// lua: CompareEntry.lua:379-413 `CompareEntryClass:RefreshMinionControls(controls,
    /// activeSkill, activeEffect, suffix)` (`activeEffect` is `activeSkill.activeEffect`)
    /// @pob Classes/CompareEntry.lua:379-413 h=c754401d
    pub fn RefreshMinionControls(
        cx: &mut Cx,
        entry: &CompareEntryRef,
        controls: &SkillControls,
        activeSkill: &ActiveSkillRef,
        suffix: &str,
    ) {
        let calcs = suffix == "Calcs";
        let m = entry.model.clone();
        let activeEffect = activeSkill.activeEffect.get();
        let grantedEffect = activeEffect.grantedEffect.get();
        let srcInstance = || {
            activeEffect
                .srcInstance
                .get()
                .unwrap_or_else(|| lua_error!("attempt to index field 'srcInstance' (a nil value)"))
        };
        let minion = controls.mainSkillMinion;
        // L380
        cx.w_mut::<DropDownControl>(minion).list.clear();
        // L381
        if grantedEffect.minionHasItemSet {
            // L382
            for itemSetId in m.itemsTab.itemSetOrderList.snapshot() {
                // L383
                let itemSet = m
                    .itemsTab
                    .itemSets
                    .with(|s| s.get(&itemSetId).cloned())
                    .unwrap_or_else(|| lua_error!("attempt to index local 'itemSet' (a nil value)"));
                // L384-387
                let label = itemSet
                    .str(n!("title"))
                    .map_or_else(|| "Default Item Set".to_owned(), |s| s.to_string());
                cx.w_mut::<DropDownControl>(minion).list.push(DropItem::new(
                    label,
                    Rc::new(MinionListValue {
                        item_set_id: Some(f64::from(itemSetId)),
                        minion_id: None,
                    }),
                ));
            }
            // L389
            let src = srcInstance();
            let value = if calcs { src.skillMinionItemSetCalcs.get() } else { src.skillMinionItemSet.get() }
                .unwrap_or(1.0);
            DropDownControl::sel_by_value(cx, minion, &|item| {
                item.get::<MinionListValue>()
                    .is_some_and(|v| v.item_set_id == Some(value))
            });
        } else {
            // L391
            for minionId in activeSkill.minionList.get().snapshot() {
                // L392-395
                let label = m
                    .data
                    .minions
                    .get(&minionId)
                    .map_or_else(|| minionId.to_string(), |d| d.name.to_string());
                cx.w_mut::<DropDownControl>(minion).list.push(DropItem::new(
                    label,
                    Rc::new(MinionListValue {
                        item_set_id: None,
                        minion_id: Some(minionId),
                    }),
                ));
            }
            // L397
            let src = srcInstance();
            let first = cx
                .w::<DropDownControl>(minion)
                .list
                .first()
                .and_then(|i| i.get::<MinionListValue>().and_then(|v| v.minion_id));
            let value = if calcs { src.skillMinionCalcs.get() } else { src.skillMinion.get() }.or(first);
            DropDownControl::sel_by_value(cx, minion, &|item| {
                // `listVal.minionId == value` (a nil value matches an entry without minionId)
                item.get::<MinionListValue>()
                    .is_some_and(|v| v.minion_id == value)
            });
        }
        // L399
        let n = cx.w::<DropDownControl>(minion).list.len();
        set_enabled(cx, minion, n > 1);
        // L400
        set_shown(cx, minion, true);

        let minion_skill = controls.mainSkillMinionSkill;
        // L402
        cx.w_mut::<DropDownControl>(minion_skill).list.clear();
        // L403
        if let Some(minionActor) = activeSkill.minion.get() {
            // L404
            for minionSkill in minionActor.activeSkillList.get().snapshot() {
                // L405
                let name = minionSkill.activeEffect.get().grantedEffect.get().name.to_string();
                cx.w_mut::<DropDownControl>(minion_skill).list.push(DropItem::str(name));
            }
            // L407
            let src = srcInstance();
            let s = if calcs { src.skillMinionSkillCalcs.get() } else { src.skillMinionSkill.get() }
                .unwrap_or(1.0);
            cx.w_mut::<DropDownControl>(minion_skill).sel_index = sel(s);
            // L408
            set_shown(cx, minion_skill, true);
            // L409
            let n = cx.w::<DropDownControl>(minion_skill).list.len();
            set_enabled(cx, minion_skill, n > 1);
        } else {
            // L411 (into the minion list, as the Lua does)
            cx.w_mut::<DropDownControl>(minion)
                .list
                .push(DropItem::str("<No spectres in build>"));
        }
    }

    /// lua: CompareEntry.lua:415-417 `CompareEntryClass:UpdateClassDropdowns()`
    /// @pob Classes/CompareEntry.lua:415-417 h=60510d7c
    pub fn UpdateClassDropdowns(_entry: &CompareEntryRef) {
        // No class dropdowns in comparison entry
    }

    /// lua: CompareEntry.lua:419-421 `CompareEntryClass:SyncLoadouts()`
    /// @pob Classes/CompareEntry.lua:419-421 h=94b83f66
    pub fn SyncLoadouts(_entry: &CompareEntryRef) {
        // No loadout syncing in comparison entry
    }

    /// lua: CompareEntry.lua:423-425 `CompareEntryClass:OpenSpectreLibrary()`
    /// @pob Classes/CompareEntry.lua:423-425 h=45d588a5
    pub fn OpenSpectreLibrary(_entry: &CompareEntryRef) {
        // No spectre library in comparison entry
    }

    /// lua: CompareEntry.lua:427-442 `CompareEntryClass:AddStatComparesToTooltip(tooltip,
    /// baseOutput, compareOutput, header, nodeCount)`
    /// @pob Classes/CompareEntry.lua:427-442 h=c8de7b94
    pub fn AddStatComparesToTooltip(
        cx: &Cx,
        entry: &CompareEntryRef,
        tooltip: &mut Tooltip,
        baseOutput: &Output,
        compareOutput: &Output,
        header: &str,
        nodeCount: Option<f64>,
    ) -> f64 {
        let m = &entry.model;
        let mut header = header.to_owned();
        // Reuse the stat comparison logic
        // L429
        let mut count = 0.0;
        // L430
        let Some(mainEnv) = m.calcsTab.mainEnv.get() else { return count };
        let Some(mainSkill) = mainEnv.player.mainSkill.get() else { return count };
        // L431
        let baseMinion = baseOutput.get(n!("Minion"));
        let compareMinion = compareOutput.get(n!("Minion"));
        if mainSkill.minion.get().is_some() && baseMinion.truthy() && compareMinion.truthy() {
            let as_table = |v: &Val| {
                v.table().unwrap_or_else(|| {
                    lua_error!("attempt to index local 'baseOutput' (a {} value)", v.lua_type())
                })
            };
            // L432
            count += Self::CompareStatList(
                cx,
                tooltip,
                &display_stats::minionDisplayStats(),
                mainEnv.minion.get().as_ref(),
                &as_table(&baseMinion),
                &as_table(&compareMinion),
                &format!("{header}\n^7Minion:"),
                nodeCount,
            );
            // L433
            if count > 0.0 {
                // L434
                header = "^7Player:".to_owned();
            } else {
                // L436
                header += "\n^7Player:";
            }
        }
        // L439
        count += Self::CompareStatList(
            cx,
            tooltip,
            &display_stats::displayStats(),
            Some(&mainEnv.player),
            baseOutput,
            compareOutput,
            &header,
            nodeCount,
        );
        // L441
        count
    }

    /// lua: CompareEntry.lua:445-511 `CompareEntryClass:CompareStatList(tooltip, statList,
    /// actor, baseOutput, compareOutput, header, nodeCount)`
    /// Stat comparison
    /// @pob Classes/CompareEntry.lua:445-511 h=e570204a
    #[allow(clippy::too_many_arguments)]
    pub fn CompareStatList(
        cx: &Cx,
        tooltip: &mut Tooltip,
        statList: &[DisplayStat],
        actor: Option<&ActorRef>,
        baseOutput: &Output,
        compareOutput: &Output,
        header: &str,
        _nodeCount: Option<f64>,
    ) -> f64 {
        // L446 local s_format = string.format
        // L447
        let mut count = 0.0;
        // L448
        let Some(mainSkill) = actor.and_then(|a| a.mainSkill.get()) else {
            // L449
            return 0.0;
        };
        let skillFlags = mainSkill.skillFlags.get();
        // L451
        for statData in statList {
            // L452
            let Some(stat) = statData.stat else { continue };
            if statData.childStat.is_some() || stat == "SkillDPS" {
                continue;
            }
            // L453
            let mut flagMatch = true;
            // L454-465 (`statData.flag` is a string in the display stat tables)
            if let Some(flag) = statData.flag {
                // L456
                flagMatch = skillFlags.truthy(Name::from(flag));
            }
            // L466-479
            if let Some(notFlag) = statData.notFlag {
                // L468
                if skillFlags.truthy(Name::from(notFlag)) {
                    // L469
                    flagMatch = false;
                }
            }
            // L480
            if flagMatch {
                // L481
                let statVal1 = compareOutput.get(Name::from(stat)).num_or(0.0);
                // L482
                let statVal2 = baseOutput.get(Name::from(stat)).num_or(0.0);
                // L483
                let mut diff = statVal1 - statVal2;
                // L484
                if stat == "FullDPS" && !compareOutput.truthy(Name::from(stat)) {
                    // L485
                    diff = 0.0;
                }
                // L487
                let co = Val::Table(compareOutput.clone());
                let bo = Val::Table(baseOutput.clone());
                if (diff > 0.001 || diff < -0.001)
                    && statData.condFunc.map_or(true, |f| {
                        f(&Val::Num(statVal1), &co).truthy() || f(&Val::Num(statVal2), &bo).truthy()
                    })
                {
                    // L488
                    if count == 0.0 {
                        // L489
                        tooltip.add_line(cx, 14.0, header);
                    }
                    // L491
                    let color = if (statData.lowerIsBetter && diff < 0.0)
                        || (!statData.lowerIsBetter && diff > 0.0)
                    {
                        cx.main.cc.POSITIVE.clone()
                    } else {
                        cx.main.cc.NEGATIVE.clone()
                    };
                    // L492
                    let val = diff * if statData.pc || statData.r#mod { 100.0 } else { 1.0 };
                    // L493
                    let fmt = statData
                        .fmt
                        .unwrap_or_else(|| lua_error!("attempt to concatenate field 'fmt' (a nil value)"));
                    let mut valStr = lfmt!(&format!("%+{fmt}"), val);
                    // L494
                    let caps = pob_lua::lmatch(&valStr, "^([%+%-]?%d+%.%d+)(%D*)$", None)
                        .map(|c| (c[0].to_str().into_owned(), c[1].to_str().into_owned()));
                    // L495
                    if let Some((number, suffix)) = caps {
                        // L496
                        let nn = pob_lua::gsub(&number, "0+$", "", None).0;
                        let nn = pob_lua::gsub(&nn, "%.$", "", None).0;
                        valStr = nn + &suffix;
                    }
                    // L498
                    let valStr = format_num_sep_main(&cx.main, &valStr);
                    // L499
                    let label = statData.label.unwrap_or_else(|| {
                        lua_error!("bad argument #4 to 'format' (string expected, got nil)")
                    });
                    let mut line = lfmt!("%s%s %s", color.as_str(), valStr.as_str(), label);
                    // L500
                    if statData.compPercent && statVal1 != 0.0 && statVal2 != 0.0 {
                        // L501
                        let pc = statVal1 / statVal2 * 100.0 - 100.0;
                        // L502
                        line += &lfmt!(" (%+.1f%%)", pc);
                    }
                    // L504
                    tooltip.add_line(cx, 14.0, &line);
                    // L505
                    count += 1.0;
                }
            }
        }
        // L510
        count
    }

    /// lua: CompareEntry.lua:514-550 `CompareEntryClass:AddRequirementsToTooltip(tooltip,
    /// level, str, dex, int, strBase, dexBase, intBase)` (`req` is the block's shared list,
    /// wiped at the end: a local here).
    /// Add requirements to tooltip
    /// @pob Classes/CompareEntry.lua:514-550 h=f1d01e14
    #[allow(clippy::too_many_arguments)]
    pub fn AddRequirementsToTooltip(
        cx: &Cx,
        entry: &CompareEntryRef,
        tooltip: &mut Tooltip,
        level: Option<f64>,
        str: Option<f64>,
        dex: Option<f64>,
        int: Option<f64>,
        strBase: Option<f64>,
        dexBase: Option<f64>,
        intBase: Option<f64>,
    ) {
        use crate::main_mode::folders::stat_color;
        let m = &entry.model;
        // L515
        let mut req: Vec<String> = Vec::new();
        let mainOutput = || {
            m.calcsTab
                .mainOutput
                .get()
                .unwrap_or_else(|| lua_error!("attempt to index field 'mainOutput' (a nil value)"))
        };
        let out = |k: &str| mainOutput().get(Name::from(k));
        // `a > b` with a nil `b` raises
        let gt = |a: f64, b: &Val| -> bool {
            match b {
                Val::Num(b) => a > *b,
                other => lua_error!("attempt to compare {} with number", other.lua_type()),
            }
        };
        // L517
        if let Some(level) = level.filter(|l| *l > 0.0) {
            // L518
            req.push(lfmt!(
                "^x7F7F7FLevel %s%d",
                stat_color(cx, level, None, Some(m.characterLevel.get())).as_str(),
                level
            ));
        }
        // L520
        let mainEnv = m
            .calcsTab
            .mainEnv
            .get()
            .unwrap_or_else(|| lua_error!("attempt to index field 'mainEnv' (a nil value)"));
        let modDB = mainEnv.modDB.get();
        if modDB.Flag(None, &[n!("OmniscienceRequirements")]) {
            // L521
            let omniSatisfy = modDB.Sum(ModType::INC, None, &[n!("OmniAttributeRequirements")]);
            // L522
            let mut highestAttribute = 0.0;
            // L523
            for stat in [str, dex, int] {
                // L524
                if stat.unwrap_or(0.0) > highestAttribute {
                    // L525
                    highestAttribute = stat.unwrap_or(f64::NAN);
                }
            }
            // L528
            let omni = m_floor(highestAttribute * (100.0 / omniSatisfy));
            // L529
            if omni > 0.0 || gt(omni, &out("Omni")) {
                // L530
                req.push(lfmt!(
                    "%s%d ^x7F7F7FOmni",
                    stat_color(cx, omni, Some(0.0), out("Omni").opt_num()).as_str(),
                    omni
                ));
            }
        } else {
            // L533-541
            for (stat, base, key) in [(str, strBase, "Str"), (dex, dexBase, "Dex"), (int, intBase, "Int")] {
                if let Some(stat) = stat {
                    if stat > 14.0 || gt(stat, &out(key)) {
                        req.push(lfmt!(
                            &format!("%s%d ^x7F7F7F{key}"),
                            stat_color(cx, stat, base, out(key).opt_num()).as_str(),
                            stat
                        ));
                    }
                }
            }
        }
        // L543
        if !req.is_empty() {
            // L544
            let fontSizeBig = if cx.main.show_flavour_text { 18.0 } else { 16.0 };
            // L545
            tooltip.add_line_ex(
                cx,
                fontSizeBig,
                &format!("^x7F7F7FRequires {}", req.join("^x7F7F7F, ")),
                Some(Font::FontinSc),
                None,
                None,
            );
            // L546
            tooltip.add_separator(cx, Some(10.0));
        }
        // L548 wipeTable(req)
    }
}

// ---- the entry's tabs (module notes) ----------------------------------------------------------

/// `configTab:ImportCalcSettings()` of the entry's ConfigTab (ConfigTab.lua:1205-1240;
/// CCR-0137 §1: calc:BM-A's `build_xml::ImportCalcSettings` is private). The UI half of its
/// `UpdateControls` has no entry controls.
fn ImportCalcSettings(build: &model::BuildRef) {
    let configTab = &build.configTab;
    // ConfigTab.lua:1206
    let input = configTab
        .activeConfigSet()
        .table(n!("input"))
        .unwrap_or_else(|| lua_error!("attempt to index field 'input' (a nil value)"));
    // ConfigTab.lua:1207
    let calcsInput = &build.calcsTab.input;
    // ConfigTab.lua:1208-1211
    let import = |old: &str, new: &str| {
        input.set(Name::from(new), calcsInput.get(Name::from(old)));
        calcsInput.set(Name::from(old), Val::Nil);
    };
    // ConfigTab.lua:1212-1237
    for (old, new) in [
        ("Cond_LowLife", "conditionLowLife"),
        ("Cond_FullLife", "conditionFullLife"),
        ("Cond_LowMana", "conditionLowMana"),
        ("Cond_FullMana", "conditionFullMana"),
        ("buff_power", "usePowerCharges"),
        ("buff_frenzy", "useFrenzyCharges"),
        ("buff_endurance", "useEnduranceCharges"),
        ("CondBuff_Onslaught", "buffOnslaught"),
        ("CondBuff_Phasing", "buffPhasing"),
        ("CondBuff_Fortify", "buffFortify"),
        ("CondBuff_UsingFlask", "conditionUsingFlask"),
        ("buff_pendulum", "usePendulum"),
        ("CondEff_EnemyCursed", "conditionEnemyCursed"),
        ("CondEff_EnemyBleeding", "conditionEnemyBleeding"),
        ("CondEff_EnemyPoisoned", "conditionEnemyPoisoned"),
        ("CondEff_EnemyBurning", "conditionEnemyBurning"),
        ("CondEff_EnemyIgnited", "conditionEnemyIgnited"),
        ("CondEff_EnemyChilled", "conditionEnemyChilled"),
        ("CondEff_EnemyFrozen", "conditionEnemyFrozen"),
        ("CondEff_EnemyShocked", "conditionEnemyShocked"),
        ("effective_physicalRed", "enemyPhysicalReduction"),
        ("effective_fireResist", "enemyFireResist"),
        ("effective_coldResist", "enemyColdResist"),
        ("effective_lightningResist", "enemyLightningResist"),
        ("effective_chaosResist", "enemyChaosResist"),
        ("effective_enemyIsBoss", "enemyIsBoss"),
    ] {
        import(old, new);
    }
    // ConfigTab.lua:1238
    configTab.BuildModList(build);
    // ConfigTab.lua:1239 (model half)
    configTab.UpdateControls(build);
}

/// `entry.treeTab:GetSpecList()` (TreeTab.lua:492-498 on the entry's tree tab; U25's
/// `TreeTab::GetSpecList` reads the open build's).
pub fn GetSpecList(entry: &CompareEntryRef) -> Vec<String> {
    let gv = &model::data().gameVersions;
    // TreeTab.lua:493
    let mut newSpecList = Vec::new();
    // TreeTab.lua:494
    for spec in entry.model.treeTab.specList.snapshot() {
        let tv = spec.treeVersion.get();
        // TreeTab.lua:495
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
        let title = spec
            .title
            .get()
            .map_or_else(|| "Default".to_owned(), |s| s.to_string());
        newSpecList.push(format!("{prefix}{title}"));
    }
    // TreeTab.lua:497
    newSpecList
}

/// `entry.treeTab.viewer.searchStr = buf` (CompareTab.lua:915).
pub fn set_viewer_search_str(entry: &CompareEntryRef, buf: &str) {
    ext(entry).viewer.borrow_mut().search_str = buf.to_owned();
}

/// `entry.treeTab:OpenVersionConvertPopup(version, ignoreTreeSubType)` (TreeTab.lua:655-672 on
/// the entry's tree tab). Its Cancel button re-selects the entry TreeTab's own `versionSelect`
/// (TreeTab.lua:668), a control the entry does not have (module notes): nothing to do.
pub fn OpenVersionConvertPopup(cx: &mut Cx, entry: &CompareEntryRef, version: Name, ignoreTreeSubType: bool) {
    let shown = model::data()
        .gameVersions
        .treeVersions
        .get(&version)
        .map(|t| t.display.to_string())
        .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
    // TreeTab.lua:671 (the title is evaluated after the constructors, which draw nothing)
    let title = format!("Convert to Version {shown}");
    let e1 = entry.clone();
    let e2 = entry.clone();
    popups::open_popup(
        cx,
        570.0,
        140.0,
        title,
        move |cx, p| {
            let p = p.id();
            // TreeTab.lua:656-658
            cx.add(
                p,
                "warningLabel",
                LabelControl::new(
                    Anchor::NONE,
                    rect(0, 20, 0, 16),
                    "^7Warning: some or all of the passives may be de-allocated due to changes in the tree.\n\nConvert will replace your current tree.\nCopy + Convert will backup your current tree.\n",
                ),
            );
            // TreeTab.lua:659-662
            let e = e1.clone();
            cx.add(
                p,
                "convert",
                ButtonControl::new(Anchor::NONE, rect(-125, 105, 100, 20), "Convert", move |cx| {
                    ConvertToVersion(cx, &e, version, true, false, ignoreTreeSubType);
                    popups::close_popup(cx);
                }),
            );
            // TreeTab.lua:663-666
            let e = e2.clone();
            cx.add(
                p,
                "convertCopy",
                ButtonControl::new(Anchor::NONE, rect(0, 105, 125, 20), "Copy + Convert", move |cx| {
                    ConvertToVersion(cx, &e, version, false, false, ignoreTreeSubType);
                    popups::close_popup(cx);
                }),
            );
            // TreeTab.lua:667-670
            cx.add(
                p,
                "cancel",
                ButtonControl::new(Anchor::NONE, rect(125, 105, 100, 20), "Cancel", |cx| {
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

/// `entry.treeTab:ConvertToVersion(version, remove, success, ignoreTreeSubType)`
/// (TreeTab.lua:594-618 on the entry's tree tab; `self.modFlag` and the success message are
/// the entry TreeTab's UI: `success` is false from the popup).
fn ConvertToVersion(
    cx: &mut Cx,
    entry: &CompareEntryRef,
    version: Name,
    remove: bool,
    _success: bool,
    ignoreTreeSubType: bool,
) {
    let m = entry.model.clone();
    let buildName = ext(entry).buildName.clone();
    let mut version = version;
    let buildSpec = m.spec.get();
    // TreeTab.lua:595
    let treeSubTypeCapture = pob_ui_core::luastr::lmatch(&buildSpec.treeVersion.get(), "(_%l+_?%l*)");
    // TreeTab.lua:596
    if let Some(cap) = treeSubTypeCapture.as_deref() {
        if !ignoreTreeSubType && pob_lua::pattern::find(&version, cap, None, false).is_none() {
            // TreeTab.lua:597
            let cand = format!("{version}{cap}");
            if model::data()
                .gameVersions
                .treeVersionList
                .iter()
                .any(|v| **v == *cand)
            {
                // TreeTab.lua:598
                version = Name::from(cand.as_str());
            }
        }
    }
    // TreeTab.lua:601
    let newSpec = new_PassiveSpec(&m, version, false);
    // TreeTab.lua:602
    newSpec.title.set(buildSpec.title.get());
    // TreeTab.lua:603
    let jewels = buildSpec.jewels.with(Clone::clone);
    newSpec.jewels.with_mut(|j| *j = jewels);
    // TreeTab.lua:604
    let state = buildSpec.CreateUndoState();
    newSpec.RestoreUndoState(&state, Some(version));
    flush_titles_as(cx, &buildName);
    // TreeTab.lua:605
    newSpec.BuildClusterJewelGraphs();
    // TreeTab.lua:606
    let active = m.treeTab.activeSpec.get();
    m.treeTab.specList.with_mut(|l| {
        let i = active as usize;
        if i > l.len() {
            lua_error!("bad argument #2 to 'insert' (position out of bounds)");
        }
        l.insert(i, newSpec);
    });
    // TreeTab.lua:607
    let target = if remove {
        // TreeTab.lua:608
        m.treeTab.specList.with_mut(|l| {
            let i = active as usize;
            if i >= 1 && i <= l.len() {
                l.remove(i - 1);
            }
        });
        // activeSpec + 1 is shifted down one on remove, otherwise we would set the spec below it if it exists
        // TreeTab.lua:610
        active
    } else {
        // TreeTab.lua:612
        active + 1.0
    };
    // TreeTab.lua:610/612 self:SetActiveSpec(..) (model half + the title, TreeTab.lua:556)
    m.treeTab.SetActiveSpec(&m, target);
    set_title(cx, &m, &buildName);
    // TreeTab.lua:614 self.modFlag = true (the entry TreeTab's UI flag)
    // TreeTab.lua:615-617 (success is false)
}

/// The CompareHooks entries this file answers (CCR-0136 §1): installed by
/// `init::default_compare_hooks`.
pub fn new_compare_entry(cx: &mut Cx, xmlText: Option<&str>, label: Option<&str>) -> Option<CompareEntryRef> {
    CompareEntry::new(cx, xmlText, label)
}

/// `entry:SetActiveSpec(index)`
pub fn entry_set_active_spec(cx: &mut Cx, entry: &CompareEntryRef, index: f64) {
    CompareEntry::SetActiveSpec(cx, entry, index);
}

/// `entry:SetActiveItemSet(id)`
pub fn entry_set_active_item_set(_cx: &mut Cx, entry: &CompareEntryRef, id: f64) {
    CompareEntry::SetActiveItemSet(entry, id);
}

/// `entry:SetActiveSkillSet(id)`
pub fn entry_set_active_skill_set(_cx: &mut Cx, entry: &CompareEntryRef, id: f64) {
    CompareEntry::SetActiveSkillSet(entry, id);
}

/// `entry:SetMainSocketGroup(index)`
pub fn entry_set_main_socket_group(_cx: &mut Cx, entry: &CompareEntryRef, index: f64) {
    CompareEntry::SetMainSocketGroup(entry, index);
}

/// `entry.configTab:SetActiveConfigSet(setId)`: the model half (calc:CO-C; the UI half's
/// controls are the entry ConfigTab's, which it does not have).
pub fn entry_config_set_active_config_set(_cx: &mut Cx, entry: &CompareEntryRef, setId: f64) {
    let m = &entry.model;
    m.configTab.SetActiveConfigSet(m, Some(setId), false);
}

/// `entry.configTab:UpdateControls()`: the model half.
pub fn entry_config_update_controls(_cx: &mut Cx, entry: &CompareEntryRef) {
    let m = &entry.model;
    m.configTab.UpdateControls(m);
}

/// `entry.configTab.varControls and entry.configTab.varControls[var]` (CompareTab.lua:1208):
/// the entry ConfigTab's option control (`None` for an entry without a ConfigTab node: one
/// LoadFromXML did not finish, or a test's stand-in entry whose `ext` is not a [`CompareEntry`]).
pub fn entry_config_var_control(
    cx: &Cx,
    entry: &CompareEntryRef,
    var: Name,
) -> Option<(CtrlId, crate::tabs::config::ctor::VarClass)> {
    let tab = entry
        .ext
        .borrow()
        .as_ref()
        .and_then(|b| b.downcast_ref::<CompareEntry>())
        .and_then(|e| e.config_tab.get())?;
    cx.w::<crate::tabs::config::ConfigTab>(tab)
        .ctor
        .var_controls
        .get(&var)
        .map(|v| (v.id, v.class))
}

/// `entry.treeTab.viewer.searchStr = buf`
pub fn entry_set_viewer_search_str(_cx: &mut Cx, entry: &CompareEntryRef, buf: &str) {
    set_viewer_search_str(entry, buf);
}

/// `entry.treeTab:OpenVersionConvertPopup(version, true)`
pub fn entry_open_version_convert_popup(cx: &mut Cx, entry: &CompareEntryRef, version: Name) {
    OpenVersionConvertPopup(cx, entry, version, true);
}
