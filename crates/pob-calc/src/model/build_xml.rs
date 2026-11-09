//! @pob-file Modules/Build.lua @16de4b82d
//! Build load/save (Modules/Build.lua 976-1099, 2222-2335): the calc half of `build:Load`/`Save`
//! and the database text entry points, plus the model half of `Build:Init` (64-110, 603-698)
//! that [`loadBuildFromXML`] runs.
//!
//! Owner: BM-A. UI statements (controls, popups, window title, loadouts) are left to the UI port
//! (ui doc U19/U20) and marked `// UI:`. CCR-0080 lists the contract gaps worked around here:
//! * `Build` (model/mod.rs, U0) has no field for `characterLevelAutoMode`, `timelessData`,
//!   `importLink`, `xmlSectionList`, `dbFileName`, `buildName` nor for the per-tab state the
//!   frozen tab structs lack: all of it lives in a per-build side table, [`build_extra`];
//! * the display-stat part of `Build:Save` (1033-1080) reads `Modules/BuildDisplayStats`
//!   (ui doc U21, `pob-app`), which the calc crate cannot see: the UI registers it with
//!   [`set_save_stats_hook`]; without a hook no `PlayerStat`/`FullDPSSkill`/`MinionStat`
//!   elements are written (PoB's `Load` never reads them back);
//! * the UI-only savers (`Notes`, `TreeView`, `Import`; Build.lua:629-639) are U20's: until it
//!   registers real ones the model keeps the section it loaded and writes it back unchanged.
#![allow(non_snake_case, clippy::all, clippy::pedantic)]

use std::cell::RefCell;

use crate::model::xml::{ComposeXML, ParseXML_root};
use crate::model::{XmlChild, XmlNode};
use crate::prelude::*;

// ---- the per-build side table (Rust-only plumbing, CCR-0080 §2) ---------------------------

thread_local! {
    static BUILD_EXTRA: RefCell<Vec<(Weak<Build>, Table)>> = const { RefCell::new(Vec::new()) };
    /// `Build:Save`'s display-stat writer (see the module doc).
    static SAVE_STATS_HOOK: RefCell<Option<Rc<dyn Fn(&Build, &mut XmlNode)>>> =
        const { RefCell::new(None) };
}

/// The build's side table for the Lua fields the frozen model structs have no slot for
/// (`characterLevelAutoMode`, `timelessData`, `importLink`, `xmlSectionList`, `dbFileName`,
/// `buildName`, `uiSections`; `itemsTab` / `treeTab` sub-tables), created on first use.
/// Entries of dropped builds are released.
pub fn build_extra(build: &BuildRef) -> Table {
    if let Some(t) = lookup_extra(build) {
        return t;
    }
    BUILD_EXTRA.with(|v| {
        let mut v = v.borrow_mut();
        v.retain(|(w, _)| w.strong_count() > 0);
        let t = Table::new();
        v.push((Rc::downgrade(build), t.clone()));
        t
    })
}

/// The side table of a build seen through `&Build` (no `Rc` at hand): the registered one, or
/// a fresh unregistered table when nothing was ever stored for the build.
pub fn build_extra_ref(build: &Build) -> Table {
    lookup_extra(build).unwrap_or_default()
}

fn lookup_extra(build: &Build) -> Option<Table> {
    let ptr = build as *const Build;
    BUILD_EXTRA.with(|v| {
        v.borrow()
            .iter()
            .find(|(w, _)| w.as_ptr() == ptr && w.strong_count() > 0)
            .map(|(_, t)| t.clone())
    })
}

/// The `Rc` of a build seen through `&Build`: the registered handle, else the active spec's
/// back pointer.
fn self_rc(build: &Build) -> BuildRef {
    let ptr = build as *const Build;
    let registered = BUILD_EXTRA.with(|v| {
        v.borrow()
            .iter()
            .find(|(w, _)| w.as_ptr() == ptr)
            .and_then(|(w, _)| w.upgrade())
    });
    registered
        .or_else(|| {
            build
                .spec
                .get()
                .build
                .upgrade()
                .filter(|b| Rc::as_ptr(b) == ptr)
        })
        .unwrap_or_else(|| lua_error!("CCR-0080: the build has no Rc handle (Build::new)"))
}

/// Registers `Build:Save`'s display-stat writer (Build.lua 1033-1080; U21's
/// `BuildDisplayStats`). `None` removes it.
pub fn set_save_stats_hook(hook: Option<Rc<dyn Fn(&Build, &mut XmlNode)>>) {
    SAVE_STATS_HOOK.with(|h| *h.borrow_mut() = hook);
}

/// `self.dbFileName` (nil → `None`).
pub fn dbFileName(build: &Build) -> Option<LStr> {
    build_extra_ref(build).str(n!("dbFileName"))
}

/// `self.dbFileName = …`.
pub fn set_dbFileName(build: &BuildRef, name: Option<&str>) {
    build_extra(build).set(n!("dbFileName"), name.map_or(Val::Nil, Val::from));
}

/// `self.characterLevelAutoMode`.
pub fn characterLevelAutoMode(build: &Build) -> bool {
    build_extra_ref(build).truthy(n!("characterLevelAutoMode"))
}

/// `self.timelessData` (created by `Build:Init`).
pub fn timelessData(build: &Build) -> Table {
    build_extra_ref(build).get_or_new_table(n!("timelessData"))
}

/// `self.importLink`.
pub fn importLink(build: &Build) -> Option<LStr> {
    build_extra_ref(build).str(n!("importLink"))
}

/// `self.xmlSectionList`: the sections `LoadDB` stored, in document order.
pub fn xmlSectionList(build: &Build) -> Vec<XmlNode> {
    let list = build_extra_ref(build).get_or_new_table(n!("xmlSectionList"));
    list.ipairs()
        .into_iter()
        .filter_map(|(_, v)| v.obj::<XmlNode>().map(|n| (*n).clone()))
        .collect()
}

/// `self.latestTree = main.tree[latestTreeVersion]` and `data.setJewelRadiiGlobally(…)`
/// (Build.lua:605-606), done once.
pub(crate) fn ensure_latestTree(build: &Build) {
    if build.latestTree.get().is_none() {
        let v = data().gameVersions.latestTreeVersion;
        // L605
        build
            .latestTree
            .set(crate::classes::passive_spec::main_LoadTree(v));
        // L606
        crate::classes::passive_tree::setJewelRadiiGlobally(&v);
    }
}

// ---- small helpers -------------------------------------------------------------------------

/// `launch:ShowErrMsg(fmt, ...)` (UI): the calc prints it.
fn ShowErrMsg(s: &str) {
    eprintln!("{s}");
}

/// `ConPrintf(...)`: PoB's console.
fn ConPrintf(s: &str) {
    eprintln!("{s}");
}

fn attr<'a>(node: &'a XmlNode, k: &str) -> Option<&'a str> {
    node.attrib.get(k).map(|v| &**v)
}

fn tonum(s: Option<&str>) -> Option<f64> {
    s.and_then(pob_lua::tonumber)
}

fn new_xml(elem: &str) -> XmlNode {
    XmlNode {
        elem: LStr::from(elem),
        attrib: indexmap::IndexMap::new(),
        children: Vec::new(),
    }
}

fn set_attr(node: &mut XmlNode, k: &str, v: Option<String>) {
    if let Some(v) = v {
        node.attrib.insert(LStr::from(k), LStr::from(v));
    }
}

/// An attribute value as `composeNode` takes it: strings are written, nil/false skipped, any
/// other value is the "is not a string" error of xml.lua:124-125.
fn attr_val(k: &str, v: &Val) -> Option<String> {
    match v {
        Val::Nil | Val::Bool(false) => None,
        Val::Str(s) => Some(s.to_string()),
        _ => lua_error!("invalid xml tree (value for attribute '{k}' in <Build> is not a string)"),
    }
}

/// `tostring(v)` of a scalar.
fn tostring_val(v: &Val) -> String {
    match v {
        Val::Nil => "nil".to_owned(),
        Val::Bool(b) => b.to_string(),
        Val::Num(x) => tostring_num(*x),
        Val::Str(s) => s.to_string(),
        other => lua_error!("Build: tostring of a {} value", other.lua_type()),
    }
}

/// `{ id = v }` (`{ id = nil }` is an empty table).
fn id_table(k: Name, v: Option<f64>) -> Table {
    let t = Table::new();
    t.set(k, v.map_or(Val::Nil, Val::Num));
    t
}

/// `next(t) and tostring(t[k])`: nil for an empty (or missing) table.
fn next_and_tostring(t: Option<Table>, k: Name) -> Option<String> {
    let t = t?;
    if t.pairs().next().is_none() {
        return None;
    }
    // `tostring(nil)` when the table holds other keys only
    Some(tostring_val(&t.get(k)))
}

/// The default `timelessData` of `Build:Init` (Build.lua:81).
fn new_timelessData() -> Table {
    let t = Table::new();
    t.set(n!("jewelType"), Table::new());
    t.set(n!("conquerorType"), Table::new());
    t.set(n!("devotionVariant1"), 1.0);
    t.set(n!("devotionVariant2"), 1.0);
    t.set(n!("jewelSocket"), Table::new());
    t.set(n!("fallbackWeightMode"), Table::new());
    t.set(n!("searchList"), "");
    t.set(n!("searchListFallback"), "");
    t.set(n!("searchResults"), Table::new());
    t.set(n!("sharedResults"), Table::new());
    t
}

// ---- Build:Init (model half) ---------------------------------------------------------------

/// The savers of Build.lua:629-639, in the constructor's order (PoB's `pairs` order over them
/// is random per process; §3.4).
const SAVERS: [&str; 9] = [
    "Config", "Notes", "Party", "Tree", "TreeView", "Items", "Skills", "Calcs", "Import",
];

/// Savers whose model is the UI's (see the module doc): their loaded section is kept.
const UI_SAVERS: [&str; 3] = ["Notes", "TreeView", "Import"];

/// @pob Modules/Build.lua:64-110 h=69d8174b
/// @pob Modules/Build.lua:603-698 h=7f4d9ba2
/// `Build:Init(dbFileName, buildName, buildXML, convertBuild, importLink)`, model half: the
/// initial state, `LoadDB`/`LoadDBFile`, the tab constructors' model halves, the section loaders
/// (trees deferred), `PostLoad`, `UpdateSocketGroups`. With `buildOutput` it ends with
/// `calcsTab:BuildOutput()` (Build.lua:694, PWR) as PoB does. → `true` when PoB would close the
/// build instead (load error, conversion popup).
pub fn Init_model(
    build: &BuildRef,
    dbFileName: Option<&str>,
    buildName: Option<&str>,
    buildXML: Option<&str>,
    convertBuild: bool,
    importLink: Option<&str>,
    buildOutput: bool,
) -> bool {
    let gv = &data().gameVersions;
    let extra = build_extra(build);
    // L65
    set_dbFileName(build, dbFileName);
    // L66
    extra.set(n!("buildName"), buildName.map_or(Val::Nil, Val::from));
    // L67
    extra.set(n!("importLink"), importLink.map_or(Val::Nil, Val::from));
    // L68-76 UI: dbFileSubPath, main:SetMode, saveAsSortMode

    // Load build file
    // L79
    extra.set(n!("xmlSectionList"), Table::new());
    // L80
    build.spectreList.set(Shared::new(Vec::new()));
    // L81
    extra.set(n!("timelessData"), new_timelessData());
    // L82
    build.viewMode.set(n!("TREE"));
    // L83 (main.defaultCharLevel is nil headless)
    build.characterLevel.set(m_min(m_max(1.0, 1.0), 100.0));
    // L84
    build.targetVersion.set(gv.liveTargetVersion);
    // L85-87
    build.bandit.set(n!("None"));
    build.pantheonMajorGod.set(n!("None"));
    build.pantheonMinorGod.set(n!("None"));
    // L88
    extra.set(n!("characterLevelAutoMode"), true);
    // L89
    if let Some(buildXML) = buildXML {
        // L90
        if build.LoadDB(buildXML, "Unnamed build") {
            // L91 UI: self:CloseBuild()
            // L92
            return true;
        }
        // L94
        extra.set(n!("modFlag"), true);
    } else {
        // L96
        if build.LoadDBFile() {
            // L97 UI: self:CloseBuild()
            // L98
            return true;
        }
        // L100
        extra.set(n!("modFlag"), false);
    }

    // L103
    if convertBuild {
        // L104
        build.targetVersion.set(gv.liveTargetVersion);
    }
    // L106
    if build.targetVersion.get() != gv.liveTargetVersion {
        // L107 self.targetVersion = nil (the frozen field cannot hold nil)
        // L108 UI: self:OpenConversionPopup()
        ShowErrMsg(&format!(
            "build targets version '{}' (PoB opens the conversion popup)",
            build.targetVersion.get()
        ));
        // L109
        return true;
    }
    // L110-602 UI: controls, display stats, tooltips

    // Initialise build components
    // L605-606
    ensure_latestTree(build);
    // L607 self.data = data (the frozen `build.data`)
    // L608-609 UI: importTab, notesTab
    // L610
    build.partyTab.ctor_model();
    // L611
    build.configTab.ctor_model();
    configTab_varControls_model(build);
    // L612
    build.itemsTab.ctor_model(build);
    // L613
    build.treeTab.ctor_model(build);
    // L614 (SkillsTab.lua:89, 95-96, 423-426)
    skillsTab_ctor_model(build);
    // L615
    build.calcsTab.ctor_model();
    // L616-626 UI: breakdown control, compare tab, overlay panes

    // Load sections from the build file
    // L629-642 savers / legacyLoaders: see SAVERS
    let uiSections = extra.get_or_new_table(n!("uiSections"));

    //special rebuild to properly initialise boss placeholders
    // L645
    build.configTab.BuildModList(build);
    // L646 UI: self:UpdateClassDropdowns()

    // Load legacy bandit and pantheon choices from build section
    // L649
    for (control, v) in [
        (n!("bandit"), build.bandit.get()),
        (n!("pantheonMajorGod"), build.pantheonMajorGod.get()),
        (n!("pantheonMinorGod"), build.pantheonMinorGod.get()),
    ] {
        // L650
        build.configTab.input.set(control, v);
    }

    // so we ran into problems with converted trees, trying to check passive tree routes and also consider thread jewels
    // but we can't check jewel info because items have not been loaded yet, and they come after passives in the xml.
    // the simplest solution seems to be making sure passive trees (which contain jewel sockets) are loaded last.
    // L656
    let mut deferredPassiveTrees: Vec<XmlNode> = Vec::new();
    let dbFileName = dbFileName_str(build);
    // L657
    for node in xmlSectionList(build) {
        // Check if there is a saver that can load this section
        // L659
        let elem = &*node.elem;
        let isTree = elem == "Tree" || elem == "Spec";
        if !(SAVERS.contains(&elem) || elem == "Spec") {
            continue;
        }
        // if the saver is treeTab, defer it until everything is loaded
        // L662
        if isTree {
            // L663
            deferredPassiveTrees.push(node);
        } else {
            // L665
            let failed = match elem {
                "Config" => {
                    build.configTab.Load(build, &node, &dbFileName);
                    false
                }
                "Party" => {
                    build.partyTab.Load(build, &node, &dbFileName);
                    false
                }
                "Items" => {
                    build.itemsTab.Load(build, &node, &dbFileName);
                    false
                }
                "Skills" => {
                    build.skillsTab.Load(build, &node, &dbFileName);
                    false
                }
                "Calcs" => build.calcsTab.Load_ret(build, &node, &dbFileName),
                // UI savers: keep the section (module doc)
                _ => {
                    debug_assert!(UI_SAVERS.contains(&elem));
                    uiSections.set(Name::from(elem), Val::Obj(Rc::new(node.clone())));
                    false
                }
            };
            if failed {
                // L666 UI: self:CloseBuild()
                // L667
                return true;
            }
        }
    }
    // L672
    for node in &deferredPassiveTrees {
        // Check if there is a saver that can load this section
        // L674
        if build.treeTab.Load_ret(build, node, &dbFileName) {
            // L675 UI: self:CloseBuild()
            // L676
            return true;
        }
    }
    // L679 (TreeTab is the only saver with a PostLoad)
    build.treeTab.PostLoad(build);

    // L685
    if build.configTab.input.pairs().next().is_none() {
        // Check for old calcs tab settings
        // L687
        ImportCalcSettings(build);
    }

    // reprocess socket groups as they might depend on items which don't necessarily load first.
    // L691
    build.skillsTab.UpdateSocketGroups(build);
    // Build calculation output tables
    // L693
    crate::modules::common::wipeGlobalCache();
    // L694
    build.outputRevision.set(1.0);
    // L695
    if buildOutput {
        crate::modules::power::BuildOutput(build);
    }
    // L696 UI: self:RefreshStatList()
    // L697
    build.buildFlag.set(false);
    // L699 UI: self.spec:SetWindowTitleWithBuildClass()
    false
}

/// `self.dbFileName` as the loaders receive it (`nil` → `""`; only error messages read it).
fn dbFileName_str(build: &Build) -> String {
    dbFileName(build).map_or_else(String::new, |s| s.to_string())
}

/// The SkillsTab constructor's model half (SkillsTab.lua:89, 95-96, 423-426; CCR-0019 §10).
fn skillsTab_ctor_model(build: &BuildRef) {
    let st = &build.skillsTab;
    // L89
    st.socketGroupList.set(Shared::new(Vec::new()));
    // L95
    st.defaultGemLevel.set(Some(n!("normalMaximum")));
    // L96 self.defaultGemQuality = main.defaultGemQuality (nil headless)
    st.defaultGemQuality.set(None);
    // L423
    st.skillSets.with_mut(|m| m.clear());
    // L424
    st.skillSetOrderList.wipe();
    st.skillSetOrderList.push(1);
    // L425
    st.NewSkillSet(Some(1.0));
    // L426
    st.SetActiveSkillSet(build, 1.0);
}

/// @pob Classes/ConfigTab.lua:672-681 h=c9190b17
/// The ConfigTab constructor's control half (CCR-0080 §1; the proposal of CCR-0037/CCR-0038):
/// one control-state table per option with a `var` in `self.varControls`, with the fields the
/// model reads (`_className`, `enabled`, `buf`, `state`, `placeholder`, `list`, `selIndex`).
/// The control classes' constructors contribute `enabled = true` (Control.lua:60),
/// `buf = ""` (EditControl.lua:48 `SetText(init or "")`, init is `""`), `list = varData.list`
/// and `selIndex = 1` (DropDownControl.lua:45-46). The input/placeholder defaults of L673/L676/L679
/// are `NewConfigSet`'s (ConfigTab.lua:1289-1296, already applied by `ctor_model`), and
/// `defaultState` (L683-692) is `config_tab::defaultState`.
fn configTab_varControls_model(build: &BuildRef) {
    let varControls = &build.configTab.varControls;
    // L261 for _, varData in ipairs(varList) do (section headers carry no var)
    for varData in &build.data.configOptions {
        if varData.section.is_some() {
            continue;
        }
        let control = Table::new();
        // ConfigTab.lua:280-347: the control class the option's type selects
        let className = match varData.r#type.as_ref().map(|t| t.as_str()) {
            Some("check") => "CheckBoxControl",
            Some("count" | "integer" | "countAllowZero" | "float") => "EditControl",
            Some("list") => "DropDownControl",
            // `varData.resizable` is never set in ConfigOptions (the schema has no such field)
            Some("text") => "EditControl",
            _ => "Control",
        };
        control.set(n!("_className"), className);
        control.set(n!("enabled"), true);
        if className == "EditControl" {
            control.set(n!("buf"), "");
        } else if className == "DropDownControl" {
            let list = Table::new();
            for item in varData.list.iter().flatten() {
                let e = Table::new();
                e.set(n!("label"), &*item.label);
                e.set(n!("val"), Val::from_value(&item.val));
                list.push(e);
            }
            control.set(n!("list"), list);
            control.set(n!("selIndex"), 1.0);
        }
        // L672
        let Some(var) = varData.var else { continue };
        // L674
        control.set(
            n!("state"),
            varData
                .defaultState
                .as_ref()
                .map_or(Val::Nil, Val::from_value),
        );
        // L677
        control.set(
            n!("placeholder"),
            varData
                .defaultPlaceholderState
                .as_ref()
                .map_or(Val::Nil, Val::from_value),
        );
        // L678
        if let Some(defaultIndex) = varData.defaultIndex {
            // L680
            control.set(n!("selIndex"), defaultIndex);
        }
        // L675
        varControls.set(var, control);
    }
}

/// @pob Classes/ConfigTab.lua:1205-1240 h=66e7f9af
/// `configTab:ImportCalcSettings()` (Build.lua:687 calls it; no calc unit owns it, CCR-0080 §6).
fn ImportCalcSettings(build: &BuildRef) {
    let configTab = &build.configTab;
    // L1206
    let input = configTab
        .activeConfigSet()
        .table(n!("input"))
        .unwrap_or_else(|| lua_error!("attempt to index field 'input' (a nil value)"));
    // L1207
    let calcsInput = &build.calcsTab.input;
    // L1208
    let import = |old: &str, new: &str| {
        // L1209
        input.set(Name::from(new), calcsInput.get(Name::from(old)));
        // L1210
        calcsInput.set(Name::from(old), Val::Nil);
    };
    // L1212-1237
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
    // L1238
    configTab.BuildModList(build);
    // L1239
    configTab.UpdateControls(build);
}

// ---- Build:Load / Build:Save ----------------------------------------------------------------

/// @pob Modules/Build.lua:976-1015 h=f404e6dd
impl Build {
    pub fn Load(self: &Rc<Self>, xml: &crate::model::XmlNode, fileName: &str) {
        let _ = fileName;
        let gv = &data().gameVersions;
        let extra = build_extra(self);
        // L977
        self.targetVersion
            .set(attr(xml, "targetVersion").map_or(gv.legacyTargetVersion, Name::from));
        // L978
        if let Some(v) = attr(xml, "viewMode") {
            // L979
            self.viewMode.set(Name::from(v));
        }
        // L981
        self.characterLevel
            .set(tonum(attr(xml, "level")).unwrap_or(1.0));
        // L982
        extra.set(
            n!("characterLevelAutoMode"),
            attr(xml, "characterLevelAutoMode") == Some("true"),
        );
        // L983 (an array: `pairs` visits it in order)
        for diff in ["bandit", "pantheonMajorGod", "pantheonMinorGod"] {
            // L984
            let v = Name::from(attr(xml, diff).unwrap_or("None"));
            match diff {
                "bandit" => self.bandit.set(v),
                "pantheonMajorGod" => self.pantheonMajorGod.set(v),
                _ => self.pantheonMinorGod.set(v),
            }
        }
        // L986
        self.mainSocketGroup.set(
            tonum(attr(xml, "mainSkillIndex"))
                .or_else(|| tonum(attr(xml, "mainSocketGroup")))
                .unwrap_or(1.0),
        );
        // L987
        self.spectreList.get().wipe();
        // L988
        for child in &xml.children {
            let XmlChild::Node(child) = child else {
                continue;
            };
            // L989
            if &*child.elem == "Spectre" {
                // L990
                if let Some(id) = attr(child, "id") {
                    if self.data.minions.contains_key(&Name::from(id)) {
                        // L991
                        self.spectreList.get().push(Name::from(id));
                    }
                }
            } else if &*child.elem == "TimelessData" {
                let td = timelessData(self);
                // L994
                td.set(
                    n!("jewelType"),
                    id_table(n!("id"), tonum(attr(child, "jewelTypeId"))),
                );
                // L997
                td.set(
                    n!("conquerorType"),
                    id_table(n!("id"), tonum(attr(child, "conquerorTypeId"))),
                );
                // L1000
                td.set(
                    n!("devotionVariant1"),
                    tonum(attr(child, "devotionVariant1")).unwrap_or(1.0),
                );
                // L1001
                td.set(
                    n!("devotionVariant2"),
                    tonum(attr(child, "devotionVariant2")).unwrap_or(1.0),
                );
                // L1002
                td.set(
                    n!("jewelSocket"),
                    id_table(n!("id"), tonum(attr(child, "jewelSocketId"))),
                );
                // L1005
                td.set(
                    n!("fallbackWeightMode"),
                    id_table(n!("idx"), tonum(attr(child, "fallbackWeightModeIdx"))),
                );
                // L1008
                td.set(
                    n!("socketFilter"),
                    attr(child, "socketFilter") == Some("true"),
                );
                // L1009
                td.set(
                    n!("socketAllocate"),
                    attr(child, "socketAllocate") == Some("true"),
                );
                // L1010
                td.set(
                    n!("socketFilterDistance"),
                    tonum(attr(child, "socketFilterDistance")).unwrap_or(0.0),
                );
                // L1011
                td.set(
                    n!("searchList"),
                    attr(child, "searchList").map_or(Val::Nil, Val::from),
                );
                // L1012
                td.set(
                    n!("searchListFallback"),
                    attr(child, "searchListFallback").map_or(Val::Nil, Val::from),
                );
            }
        }
    }
}

/// @pob Modules/Build.lua:1017-1098 h=8bce71b8
impl Build {
    pub fn Save(&self, xml: &mut crate::model::XmlNode) {
        let extra = build_extra_ref(self);
        let spec = self.spec.get();
        let input = &self.configTab.input;
        // L1018
        xml.attrib = indexmap::IndexMap::new();
        set_attr(
            xml,
            "targetVersion",
            Some(self.targetVersion.get().to_string()),
        );
        set_attr(xml, "viewMode", Some(self.viewMode.get().to_string()));
        set_attr(xml, "level", Some(tostring_num(self.characterLevel.get())));
        set_attr(
            xml,
            "className",
            spec.curClassName.get().map(|s| s.to_string()),
        );
        set_attr(
            xml,
            "ascendClassName",
            spec.curAscendClassName.get().map(|s| s.to_string()),
        );
        set_attr(xml, "bandit", attr_val("bandit", &input.get(n!("bandit"))));
        set_attr(
            xml,
            "pantheonMajorGod",
            attr_val("pantheonMajorGod", &input.get(n!("pantheonMajorGod"))),
        );
        set_attr(
            xml,
            "pantheonMinorGod",
            attr_val("pantheonMinorGod", &input.get(n!("pantheonMinorGod"))),
        );
        set_attr(
            xml,
            "mainSocketGroup",
            Some(tostring_num(self.mainSocketGroup.get())),
        );
        set_attr(
            xml,
            "characterLevelAutoMode",
            Some(tostring_val(&extra.get(n!("characterLevelAutoMode")))),
        );
        // L1030
        for id in self.spectreList.get().snapshot() {
            // L1031
            let mut c = new_xml("Spectre");
            set_attr(&mut c, "id", Some(id.to_string()));
            xml.children.push(XmlChild::Node(c));
        }
        // L1033-1080: PlayerStat / FullDPSSkill / MinionStat from the display stat lists
        // (module doc: the UI's hook writes them)
        if let Some(hook) = SAVE_STATS_HOOK.with(|h| h.borrow().clone()) {
            hook(self, xml);
        }
        // L1081
        let td = timelessData(self);
        let mut timelessDataNode = new_xml("TimelessData");
        // L1084
        set_attr(
            &mut timelessDataNode,
            "jewelTypeId",
            next_and_tostring(td.table(n!("jewelType")), n!("id")),
        );
        // L1085
        set_attr(
            &mut timelessDataNode,
            "conquerorTypeId",
            next_and_tostring(td.table(n!("conquerorType")), n!("id")),
        );
        // L1086
        set_attr(
            &mut timelessDataNode,
            "devotionVariant1",
            Some(tostring_val(&td.get(n!("devotionVariant1")))),
        );
        // L1087
        set_attr(
            &mut timelessDataNode,
            "devotionVariant2",
            Some(tostring_val(&td.get(n!("devotionVariant2")))),
        );
        // L1088
        set_attr(
            &mut timelessDataNode,
            "jewelSocketId",
            next_and_tostring(td.table(n!("jewelSocket")), n!("id")),
        );
        // L1089
        set_attr(
            &mut timelessDataNode,
            "fallbackWeightModeIdx",
            next_and_tostring(td.table(n!("fallbackWeightMode")), n!("idx")),
        );
        // L1090
        set_attr(
            &mut timelessDataNode,
            "socketFilter",
            td.truthy(n!("socketFilter")).then(|| "true".to_owned()),
        );
        // L1091
        set_attr(
            &mut timelessDataNode,
            "socketAllocate",
            td.truthy(n!("socketAllocate")).then(|| "true".to_owned()),
        );
        // L1092
        let v = td.get(n!("socketFilterDistance"));
        set_attr(
            &mut timelessDataNode,
            "socketFilterDistance",
            v.truthy().then(|| tostring_val(&v)),
        );
        // L1093
        let v = td.get(n!("searchList"));
        set_attr(
            &mut timelessDataNode,
            "searchList",
            v.truthy().then(|| tostring_val(&v)),
        );
        // L1094
        let v = td.get(n!("searchListFallback"));
        set_attr(
            &mut timelessDataNode,
            "searchListFallback",
            v.truthy().then(|| tostring_val(&v)),
        );
        // L1097
        xml.children.push(XmlChild::Node(timelessDataNode));
    }
}

// ---- database text -------------------------------------------------------------------------

/// @pob Modules/Build.lua:2222-2260 h=dc6cc23e
impl Build {
    /// → true on error (Lua returns `true` after showing the error).
    pub fn LoadDB(self: &Rc<Self>, xmlText: &str, fileName: &str) -> bool {
        let extra = build_extra(self);
        // Parse the XML
        // L2224
        let dbXML = ParseXML_root(xmlText);
        // L2225
        let dbXML = match dbXML {
            Err(errMsg)
                if pob_lua::pattern::find(&errMsg, ".*file returns nil", None, false).is_some() =>
            {
                // L2226 UI: main:OpenCloudErrorPopup(fileName)
                ShowErrMsg(&format!("cloud error: {fileName}"));
                // L2227
                return true;
            }
            Err(errMsg) => {
                // L2229
                ShowErrMsg(&format!("^1{errMsg}"));
                // L2230
                return true;
            }
            Ok(x) => x,
        };
        let root = match dbXML.children.first() {
            Some(XmlChild::Node(n)) if &*n.elem == "PathOfBuilding" => n.clone(),
            Some(_) => {
                // L2232
                ShowErrMsg(&format!(
                    "^1Error parsing '{fileName}': 'PathOfBuilding' root element missing"
                ));
                // L2233
                return true;
            }
            // `dbXML[1].elem` on nil
            None => lua_error!("attempt to index field '?' (a nil value)"),
        };

        // Load Build section first
        // L2237
        for node in &root.children {
            // L2238
            if let XmlChild::Node(node) = node {
                if &*node.elem == "Build" {
                    // L2239
                    let dbFileName = dbFileName_str(self);
                    self.Load(node, &dbFileName);
                    // L2240
                    break;
                }
            }
        }

        // Check if xml has an import link
        // L2245
        for node in &root.children {
            // L2246
            if let XmlChild::Node(node) = node {
                if &*node.elem == "Import" {
                    // L2247
                    if let Some(link) = attr(node, "importLink") {
                        if !extra.truthy(n!("importLink")) {
                            // L2248
                            extra.set(n!("importLink"), link);
                        }
                    }
                    // L2250
                    break;
                }
            }
        }

        // Store other sections for later processing
        let list = extra.get_or_new_table(n!("xmlSectionList"));
        // L2255
        for node in root.children {
            // L2256
            if let XmlChild::Node(node) = node {
                // L2257
                list.push(Val::Obj(Rc::new(node)));
            }
        }
        // L2260
        false
    }
}

/// @pob Modules/Build.lua:2262-2275 h=0ffb09a9
impl Build {
    pub fn LoadDBFile(self: &Rc<Self>) -> bool {
        // L2263
        let Some(name) = dbFileName(self) else {
            // L2264
            return false;
        };
        // L2266
        ConPrintf(&format!("Loading '{name}'..."));
        // L2267
        let Ok(bytes) = std::fs::read(&*name) else {
            // L2269
            set_dbFileName(self, None);
            // L2270
            return true;
        };
        // L2272 (io.read "*a"; invalid UTF-8 becomes U+FFFD)
        let xmlText = String::from_utf8_lossy(&bytes).into_owned();
        // L2274
        self.LoadDB(&xmlText, &name)
    }
}

/// @pob Modules/Build.lua:2277-2301 h=e164c85c
impl Build {
    pub fn SaveDB(&self, fileName: &str) -> Option<String> {
        let _ = fileName;
        // L2278
        let mut dbXML = new_xml("PathOfBuilding");

        // Save Build section first
        // L2281
        {
            // L2282
            let mut node = new_xml("Build");
            // L2283
            self.Save(&mut node);
            // L2284
            dbXML.children.push(XmlChild::Node(node));
        }

        // Call on all savers to save their data in their respective sections
        let uiSections = build_extra_ref(self).get_or_new_table(n!("uiSections"));
        let rc = self_rc(self);
        // L2288 (constructor order; PoB's `pairs` order is random per process)
        // LUA-ORDER
        for elem in SAVERS {
            // L2289
            let mut node = new_xml(elem);
            // L2290
            match elem {
                "Config" => self.configTab.Save(&mut node),
                "Party" => self.partyTab.Save(&mut node),
                "Tree" => self.treeTab.Save(&rc, &mut node),
                "Items" => self.itemsTab.Save(&rc, &mut node),
                "Skills" => self.skillsTab.Save(&mut node),
                "Calcs" => self.calcsTab.Save(&mut node),
                // UI savers: the loaded section, unchanged (module doc)
                _ => {
                    if let Some(saved) = uiSections.obj::<XmlNode>(Name::from(elem)) {
                        node.attrib = saved.attrib.clone();
                        node.children = saved.children.clone();
                    }
                }
            }
            // L2291
            dbXML.children.push(XmlChild::Node(node));
        }

        // Compose the XML
        // L2295 (composeNode cannot fail on a typed tree)
        let xmlText = ComposeXML(&dbXML);
        // L2299
        Some(xmlText)
    }
}

/// @pob Modules/Build.lua:2304-2333 h=1efe5bf2
impl Build {
    pub fn SaveDBFile(&self) -> bool {
        // L2305
        let Some(name) = dbFileName(self) else {
            // L2306 UI: self:OpenSaveAsPopup()
            // L2307
            return false;
        };
        // L2309
        let Some(xmlText) = self.SaveDB(&name) else {
            // L2311
            return true;
        };
        // L2313
        if std::fs::write(&*name, xmlText.as_bytes()).is_err() {
            // L2315 UI: main:OpenMessagePopup("Error", "Couldn't save the build file: …")
            ShowErrMsg(&format!(
                "Couldn't save the build file:\n{name}\nMake sure the save folder exists and is writable."
            ));
            // L2316
            return true;
        }
        // L2320-2332 UI: actionOnSave, ResetModFlags, CloseBuild / Exit / ApplyUpdate
        false
    }
}

// @pob Modules/Build.lua:2335-2335 h=a3bceb2e
// (module-level statements: Return)

/// Rust entry point used by tests and the UI: a build from PoB build XML text
/// (`Build:Init` + `LoadDB`, including the first `calcsTab:BuildOutput()`).
pub fn loadBuildFromXML(xmlText: &str, data: &'static Data) -> BuildRef {
    let build = Build::new(data);
    let _ = Init_model(
        &build,
        None,
        Some("Unnamed build"),
        Some(xmlText),
        false,
        None,
        true,
    );
    build
}

/// [`loadBuildFromXML`] without the first `BuildOutput` (the loaded model only); `Err` when PoB
/// would close the build instead of opening it.
pub fn loadBuildModelFromXML(xmlText: &str, data: &'static Data) -> Result<BuildRef, String> {
    let build = Build::new(data);
    if Init_model(
        &build,
        None,
        Some("Unnamed build"),
        Some(xmlText),
        false,
        None,
        false,
    ) {
        return Err("PoB closes the build (load error or conversion popup; see stderr)".to_owned());
    }
    Ok(build)
}
