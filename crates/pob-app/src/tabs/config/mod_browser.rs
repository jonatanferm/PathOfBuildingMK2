//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Modules/ConfigModBrowser.lua 1-311.
//! Owner: U42.
//!
//! @pob-file Modules/ConfigModBrowser.lua @16de4b82d
//!
//! The "Mod Browser" popup of a custom modifier block ("Add Mod", CustomModBlockControl.lua:52):
//! every supported mod line of the tree nodes and the item mods, searchable, added to the block's
//! text.
//!
//! Lua semantics:
//! * `modSet` is keyed by the line's template; an entry is `{ text, sources, supported,
//!   template, sortKey }` ([`ModEntry`]); `sources` is a string set. `displayList` holds
//!   `copyTable(mod)` copies, so a row value is a fresh `Rc` per `updateDisplayList` (Lua
//!   table identity for `selValue == value`).
//! * `supported` is always a boolean at the call sites (`(not not modList) and not extra`), so
//!   `modEntry.supported or supported` is the boolean `or`.
//! * `node.stats` is `node.sd` (PassiveTree.lua:535 assigns the same table, which ProcessStats
//!   then splits in place); a node of a pre-3.10 tree has no `stats` field, which is the Lua
//!   index error here as in PoB.
//! * `node.mods[i].list` after the combine loop is the dummy `{ list = {}, combined = true }`
//!   of the last combined line: a truthy `list`, so such a line counts as supported
//!   (LUA-QUIRK, kept).
//! * `mod.tradeHashes`: the data schema stores an absent `tradeHashes` as an empty map, so an
//!   empty map is read as nil (the crafted-mod branch, L94). No mod of the data has
//!   `tradeHashes = { }` (`data.masterMods` have none, every other table has one per mod).
//! * `data.beastCraft` is an untyped dump root ([`pob_gamedata::Data::other`]); its mods are read
//!   through the same accessor as the typed affix tables.
//! * `seenGroups[mod.group] = true` with a nil group is the Lua "table index is nil" error.
//! * `string.find(s, p, 1, true)` is a plain find; `searchStr:sub(1, len1)` byte slices.
//!
//! ORDER: the loops that fill `modSet` (`pairs(tree.nodes)` L158, `pairs(bData.itemMods)` L194
//! and `pairs(catMods)` L196, `pairs(bData.veiledMods)` L203, `pairs(bData.beastCraft)` L208,
//! `pairs(mod.tradeHashes)` L84) decide which line of a template becomes its `text` (the first
//! one, or the first supported one) and which mod of a `group` is recorded (`seenGroups`), so
//! the list shows e.g. "4%" or "5% additional Physical Damage Reduction while moving". PoB's
//! order there is per-process random (string keys; `tree.nodes` held the key `root`, ui doc §8
//! rule 6), and so is `pairs(value.sources)` (L246, the row tooltip's lines). Resolution (a):
//! the port walks each of these tables in a canonical order (keys ascending: node ids and trade
//! hashes numerically, names bytewise), and the oracle runs ConfigModBrowser with the same
//! loops patched to a sorted `pairs` (`tests/u42_config.rs` `LUA_STUBS`; CCR-0121 §6 proposes
//! moving it to U8's patches.rs). `pairs(modSet)` (L214) only builds `supportedList`, which is
//! sorted with a total order (template keys are unique) (c).
//!
//! Props with side effects (ui doc §7.3): none.
#![allow(non_snake_case)]

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use indexmap::{IndexMap, IndexSet};
use pob_gamedata::value::{DataTable, DataValue};
use pob_lua::n;
use pob_ui_core::controls::list::{ListControl, ListData, ListSource, ListValue};
use pob_ui_core::controls::{ButtonControl, EditArgs, EditControl, LabelControl, ScrollBarControl};
use pob_ui_core::luastr::gsub;
use pob_ui_core::popups::{self, PopupOpts};
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::Tooltip;

use super::ConfigTab;
use crate::app::CxApp;
use crate::model::{self, Table, Val};

// @pob Modules/ConfigModBrowser.lua:1-3 h=7a82b8e6
// (`local M = {}`, `local t_insert = table.insert`: the module's functions below)

/// lua: ConfigModBrowser.lua:5-38 fuzzyScore(modText, searchStr, words)
/// @pob Modules/ConfigModBrowser.lua:5-38 h=6fd18f13
pub fn fuzzyScore(modText: &str, searchStr: &str, words: &[String]) -> Option<i32> {
    // L6
    let modLower = pob_lua::string::lower(modText);
    // L7
    if pob_lua::pattern::find(&modLower, searchStr, Some(1), true).is_some() {
        // L8
        return Some(1);
    }
    // L10
    if words.len() > 1 {
        // L11
        let mut allFound = true;
        // L12
        for w in words {
            // L13
            if pob_lua::pattern::find(&modLower, w, Some(1), true).is_none() {
                // L14
                allFound = false;
                // L15
                break;
            }
        }
        // L18
        if allFound {
            // L19
            return Some(2);
        }
    }
    // L22
    if words.len() == 1 && searchStr.len() >= 3 {
        // L23-26
        let textWords: Vec<&[u8]> = pob_lua::pattern::gmatch(&modLower, "%w+")
            .map(|c| c[0].as_bytes())
            .collect();
        let s = searchStr.as_bytes();
        // L27
        for i in 0..textWords.len() {
            // L28
            for len1 in 2..s.len() {
                // L29
                let part1 = &s[..len1];
                // L30
                let part2 = &s[len1..];
                // L31
                if textWords[i].starts_with(part1)
                    && textWords
                        .get(i + 1)
                        .is_some_and(|next| next.starts_with(part2))
                {
                    // L32
                    return Some(3);
                }
            }
        }
    }
    // L37
    None
}

thread_local! {
    /// L39 `modTemplateCache`
    static MOD_TEMPLATE_CACHE: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

/// lua: ConfigModBrowser.lua:41-49 getModTemplate(modText)
/// @pob Modules/ConfigModBrowser.lua:39-49 h=30c499af
pub fn getModTemplate(modText: &str) -> String {
    // L42
    if let Some(t) = MOD_TEMPLATE_CACHE.with(|c| c.borrow().get(modText).cloned()) {
        return t;
    }
    // L43-46
    let t = pob_lua::string::lower(&gsub(
        &gsub(modText, "([%+-]?)%((%-?%d+%.?%d*)%-(%-?%d+%.?%d*)%)", "%1#"),
        "%d+%.?%d*",
        "#",
    ));
    MOD_TEMPLATE_CACHE.with(|c| c.borrow_mut().insert(modText.to_owned(), t.clone()));
    // L48
    t
}

/// A `modSet` entry (and its `copyTable` copies in `displayList`).
#[derive(Clone, Debug, Default)]
pub struct ModEntry {
    pub text: String,
    /// `sources` (a string set; `nil` for the "no match" row)
    pub sources: Option<IndexSet<String>>,
    pub supported: Option<bool>,
    pub template: Option<String>,
    pub sortKey: Option<String>,
    /// `rank` (set on the display copies of a search)
    pub rank: Option<i32>,
}

type ModSet = IndexMap<String, ModEntry>;

/// Either adds a new entry or adds the source to an existing entry.
/// lua: ConfigModBrowser.lua:55-70 addModLine(set, line, source, supported)
/// @pob Modules/ConfigModBrowser.lua:50-70 h=433414f9
fn addModLine(set: &mut ModSet, line: &str, source: &str, supported: Option<bool>) {
    // L56
    let template = getModTemplate(line);
    // L57-63
    let modEntry = set.entry(template).or_insert_with(|| {
        // L59-60
        ModEntry {
            text: line.to_owned(),
            sources: Some(IndexSet::new()),
            ..ModEntry::default()
        }
    });
    // L59 / L62
    modEntry
        .sources
        .get_or_insert_with(IndexSet::new)
        .insert(source.to_owned());
    // L64
    if let Some(supported) = supported {
        // L65
        if supported && !modEntry.supported.unwrap_or(false) {
            // L66
            modEntry.text = line.to_owned();
        }
        // L68
        modEntry.supported = Some(modEntry.supported.unwrap_or(false) || supported);
    }
}

/// An item mod as `addItemMod` reads it: `group`, `tradeHashes`, the stat lines.
struct ItemModView {
    group: Option<String>,
    /// `mod.tradeHashes` (`None`: nil): each `statDescription` (a list of lines)
    trade_hashes: Option<Vec<Vec<String>>>,
    /// `ipairs(mod)`
    lines: Vec<String>,
}

impl ItemModView {
    fn of_item_mod(m: &pob_gamedata::schema::affixes::ItemMod) -> ItemModView {
        ItemModView {
            group: m.group.map(|g| g.to_string()),
            // module notes: an empty map is an absent `tradeHashes`
            trade_hashes: (!m.tradeHashes.is_empty()).then(|| {
                m.tradeHashes
                    .values()
                    .map(|v| v.iter().map(ToString::to_string).collect())
                    .collect()
            }),
            lines: m.lines.iter().map(ToString::to_string).collect(),
        }
    }

    fn of_data(d: &pob_gamedata::Data, t: &DataTable) -> ItemModView {
        let strs = |v: &DataValue| -> Vec<String> {
            match d.resolve(v) {
                DataValue::Table(t) => t
                    .seq
                    .iter()
                    .filter_map(|x| d.resolve(x).as_str().map(ToOwned::to_owned))
                    .collect(),
                _ => Vec::new(),
            }
        };
        let th = d.resolve(t.field("tradeHashes"));
        ItemModView {
            group: d.resolve(t.field("group")).as_str().map(ToOwned::to_owned),
            trade_hashes: th.table().map(|th| {
                th.seq
                    .iter()
                    .chain(th.num.iter().map(|(_, v)| v))
                    .chain(th.fields.iter().map(|(_, v)| v))
                    .map(strs)
                    .collect()
            }),
            lines: t
                .seq
                .iter()
                .filter_map(|x| d.resolve(x).as_str().map(ToOwned::to_owned))
                .collect(),
        }
    }
}

/// `pairs(t)` over an affix table in the canonical order (keys bytewise; module notes, ORDER).
fn sorted_mods(
    t: &IndexMap<model::Name, pob_gamedata::schema::affixes::ItemModRef>,
) -> Vec<&pob_gamedata::schema::affixes::ItemModRef> {
    let mut v: Vec<_> = t.iter().collect();
    v.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    v.into_iter().map(|(_, m)| m).collect()
}

/// `modLib.parseMod(line)` => `(not not modList) and not extra`
fn parse_supported(line: &str) -> bool {
    let (modList, extra) = pob_modparser::parseMod(line, false);
    modList.is_some() && extra.is_none()
}

/// `itemLib.applyRange(line, 0.5)`
fn apply_range_half(line: &str) -> String {
    pob_calc::classes::item_tools::applyRange(line, &Val::Num(0.5), None, None)
}

/// lua: ConfigModBrowser.lua:75-101 addItemMod(seenGroups, set, mod, source)
/// @pob Modules/ConfigModBrowser.lua:71-101 h=eb6893f5
fn addItemMod(
    seenGroups: &mut IndexSet<String>,
    set: &mut ModSet,
    r#mod: &ItemModView,
    source: &str,
) {
    // L76
    let group = r#mod.group.as_deref();
    if !group.is_some_and(|g| seenGroups.contains(g)) {
        // L77
        let g = group.unwrap_or_else(|| lua_error!("table index is nil"));
        seenGroups.insert(g.to_owned());
        // the actual trade hashes aren't relevant, but this
        // field is separated by stat description, which
        // means that getting rid of split lines won't
        // result in accidentally combining two separate
        // stats
        // L83
        if let Some(tradeHashes) = &r#mod.trade_hashes {
            // L84 ORDER: module notes
            for statDescription in tradeHashes {
                // L85
                let line = statDescription.join(" ");
                // note that exactly 0.5 range is necessary as that is what the
                // mod cache uses
                // L88
                let rangedLine = apply_range_half(&line);
                // L89-90
                let supported = parse_supported(&rangedLine);
                addModLine(set, &rangedLine, source, Some(supported));
            }
        } else {
            // crafted mods have a different format
            // L94
            for line in &r#mod.lines {
                // L95
                let rangedLine = apply_range_half(line);
                // L96-97
                let supported = parse_supported(&rangedLine);
                addModLine(set, &rangedLine, source, Some(supported));
            }
        }
    }
}

/// lua: ConfigModBrowser.lua:102-110 sortAlphabeticallyIgnoringValues(a, b)
/// @pob Modules/ConfigModBrowser.lua:102-110 h=ed1749b4
fn sortAlphabeticallyIgnoringValues(a: &ModEntry, b: &ModEntry) -> bool {
    let key = |e: &ModEntry| -> String {
        e.sortKey
            .clone()
            .unwrap_or_else(|| lua_error!("attempt to compare nil with string"))
    };
    // L103
    if a.sortKey != b.sortKey {
        // L104
        return key(a).as_bytes() < key(b).as_bytes();
    }
    // L106
    if a.template != b.template {
        // L107
        let t = |e: &ModEntry| {
            e.template
                .clone()
                .unwrap_or_else(|| lua_error!("attempt to compare nil with string"))
        };
        return t(a).as_bytes() < t(b).as_bytes();
    }
    // L109
    a.text.as_bytes() < b.text.as_bytes()
}

/// L112
pub const NO_MATCH_TEXT: &str = "No matching modifiers found";

/// `displayList`: the rows the popup's list shows (a shared upvalue; the list aliases it).
type DisplayList = Rc<RefCell<Vec<Rc<ModEntry>>>>;

fn row_value(e: &Rc<ModEntry>) -> ListValue {
    ListValue::of(e.clone())
}

/// The list control's `self.list = displayList` (not mutable: never inserted into or removed
/// from by the list).
struct DisplaySource(DisplayList);

impl ListSource for DisplaySource {
    fn len(&self, _cx: &Cx) -> usize {
        self.0.borrow().len()
    }
    fn get(&self, _cx: &Cx, i: usize) -> Option<ListValue> {
        let l = self.0.borrow();
        i.checked_sub(1).and_then(|i| l.get(i)).map(row_value)
    }
    fn insert(&self, _cx: &mut Cx, _i: usize, _v: ListValue) {
        lua_error!("ConfigModBrowser: the mod list is not mutable")
    }
    fn remove(&self, _cx: &mut Cx, _i: usize) -> ListValue {
        lua_error!("ConfigModBrowser: the mod list is not mutable")
    }
}

/// The entry of a row value.
fn entry_of(v: &ListValue) -> Rc<ModEntry> {
    v.rc::<ModEntry>()
        .unwrap_or_else(|| lua_error!("ConfigModBrowser: a list value that is not a mod entry"))
}

/// lua: ConfigModBrowser.lua:113-150 updateDisplayList(controls, displayList, modList); `p` is
/// the popup (`controls`).
/// @pob Modules/ConfigModBrowser.lua:112-150 h=e82695e8
fn updateDisplayList(cx: &mut Cx, p: CtrlId, displayList: &DisplayList, modList: &[Rc<ModEntry>]) {
    // L114
    displayList.borrow_mut().clear();
    // L115 (each `gsub` keeps its first result)
    let search = cx.child_as::<EditControl>(p, "search");
    let searchStr = gsub(
        &gsub(&pob_lua::string::lower(&cx.get(search).buf), "^[%s]+", ""),
        "[%s]+$",
        "",
    );

    // L117
    if searchStr.is_empty() {
        // L118
        let mut l = displayList.borrow_mut();
        for r#mod in modList {
            // L119
            l.push(Rc::new((**r#mod).clone()));
        }
    } else {
        // L122-125
        let words: Vec<String> = pob_lua::pattern::gmatch(&searchStr, "%S+")
            .map(|c| c[0].as_str().to_owned())
            .collect();
        // L126
        let mut l: Vec<Rc<ModEntry>> = Vec::new();
        for r#mod in modList {
            // L127
            let rank = fuzzyScore(&r#mod.text, &searchStr, &words);
            // L128
            if let Some(rank) = rank {
                // L129
                let mut entry = (**r#mod).clone();
                // L130
                entry.rank = Some(rank);
                // L131
                l.push(Rc::new(entry));
            }
        }
        // L134-139
        pob_lua::sort(&mut l, |a, b| {
            // L135
            if a.rank != b.rank {
                // L136
                return a.rank < b.rank;
            }
            // L138
            sortAlphabeticallyIgnoringValues(a, b)
        });
        *displayList.borrow_mut() = l;
    }

    // L142
    if displayList.borrow().is_empty() {
        // L143
        displayList.borrow_mut().push(Rc::new(ModEntry {
            text: NO_MATCH_TEXT.to_owned(),
            ..ModEntry::default()
        }));
    }
    // L145
    if let Some(list) = cx.child(p, "listControl") {
        let first = displayList.borrow().first().map(row_value);
        // L146-147
        {
            let w = cx.w_mut::<ListControl>(list);
            w.sel_index = Some(1);
            w.sel_value = first;
        }
        // L148
        let sb = cx.child_as::<ScrollBarControl>(list, "scrollBarV");
        cx.w_mut::<ScrollBarControl>(sb).offset = 0.0;
    }
}

/// The help text of the "?" button (L273-277).
const HELP_TEXT: &str = "This menu currently contains supported mod lines from tree nodes and item modifiers only.

This menu is not a representation of what PoB can parse, and this is only a limited list of mods. Additionally, some mods might be local to items and might not function in custom modifiers.

A mod being supported does not necessarily mean that it will be included in calculations, and only means that the mod parser accepts it.";

/// L154-226: every supported mod line of the active spec's tree and of the item mod tables,
/// sorted (`supportedList`).
fn supported_list(m: &model::BuildRef) -> Vec<Rc<ModEntry>> {
    // L154
    let bData = m.data;
    // L155
    let active = m.treeTab.activeSpec.get();
    let spec = m
        .treeTab
        .specList
        .with(|l| {
            (active >= 1.0 && active.fract() == 0.0)
                .then(|| l.get(active as usize - 1).cloned())
                .flatten()
        })
        .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
    let tree = spec.tree.get();
    // L156
    let mut modSet: ModSet = IndexMap::new();

    // L158 ORDER: module notes
    let mut nodes = tree.nodes.pairs();
    nodes.sort_by_key(|(id, _)| *id);
    for (_, node) in nodes {
        let ty = node.r#type.get();
        let masteryEffects = node.masteryEffects.get();
        // L159
        if let Some(masteryEffects) = masteryEffects.filter(|_| ty.as_str() == "Mastery") {
            // L160
            for (_, masteryEffect) in masteryEffects.ipairs() {
                let stats = masteryEffect.get(n!("stats")).table().unwrap_or_else(|| {
                    lua_error!("bad argument #1 to 'ipairs' (table expected, got nil)")
                });
                // L161
                for (_, statLine) in stats.ipairs() {
                    let statLine = statLine.str().map(|s| s.to_string()).unwrap_or_else(|| {
                        lua_error!("bad argument #1 to 'parseMod' (string expected)")
                    });
                    // L162-163
                    let supported = parse_supported(&statLine);
                    addModLine(
                        &mut modSet,
                        &statLine,
                        &format!("{} Node", ty.as_str()),
                        Some(supported),
                    );
                }
            }
        } else {
            // `node.stats` (module notes)
            if !node.extra.truthy(n!("stats")) {
                lua_error!("attempt to index field 'stats' (a nil value)");
            }
            let stats = node.sd.get().with(Clone::clone);
            let mods = node.mods.get().with(Clone::clone);
            let mods_at =
                |i: usize| -> Option<Table> { i.checked_sub(1).and_then(|i| mods.get(i)).cloned() };
            // L167
            let mut i: usize = 1;
            // L168
            while let Some(first) = stats.get(i - 1) {
                // L169
                let mut combinedLine = first.to_string();
                // L170
                while let Some(nextMod) = mods_at(i + 1) {
                    // L171-172
                    if nextMod.truthy(n!("combined")) {
                        // L173
                        let next = stats.get(i).map(|s| s.to_string()).unwrap_or_else(|| {
                            lua_error!("attempt to concatenate field '?' (a nil value)")
                        });
                        combinedLine = format!("{combinedLine} {next}");
                        // L174
                        i += 1;
                    } else {
                        // L176
                        break;
                    }
                }
                // L179
                let m_i = mods_at(i)
                    .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
                let supported = m_i.truthy(n!("list")) && !m_i.truthy(n!("extra"));
                // L180-181
                let ascendancyPrefix = if node.extra.truthy(n!("isBloodline")) {
                    "Bloodline "
                } else if node.ascendancyName.get().is_some() {
                    "Ascendancy "
                } else {
                    ""
                };
                // L182
                addModLine(
                    &mut modSet,
                    &combinedLine,
                    &format!("{ascendancyPrefix}{} Node", ty.as_str()),
                    Some(supported),
                );
                // L183
                i += 1;
            }
        }
    }
    // L187
    let mut recordedGroups: IndexSet<String> = IndexSet::new();
    // L188 (`data.masterMods` always exists)
    for r#mod in &bData.masterMods.0 {
        // L190
        addItemMod(
            &mut recordedGroups,
            &mut modSet,
            &ItemModView::of_item_mod(r#mod),
            "Crafted mod",
        );
    }
    // L193-194 ORDER: module notes
    let mut cats: Vec<_> = bData.itemMods.iter().collect();
    cats.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    for (catName, catMods) in cats {
        // L195
        if catName.as_str() != "Item" && catName.as_str() != "JewelCluster" {
            // L196
            for r#mod in sorted_mods(&catMods.0) {
                // L197
                addItemMod(
                    &mut recordedGroups,
                    &mut modSet,
                    &ItemModView::of_item_mod(r#mod),
                    "Item mod",
                );
            }
        }
    }
    // L202-203
    for r#mod in sorted_mods(&bData.veiledMods.0) {
        // L204
        addItemMod(
            &mut recordedGroups,
            &mut modSet,
            &ItemModView::of_item_mod(r#mod),
            "Veiled mod",
        );
    }
    // L207
    if let Some(DataValue::Table(beastCraft)) =
        bData.other.get("data.beastCraft").map(|v| bData.resolve(v))
    {
        // L208
        let mut fields: Vec<_> = beastCraft.fields.iter().collect();
        fields.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
        for (_, r#mod) in fields {
            if let DataValue::Table(t) = bData.resolve(r#mod) {
                // L209
                addItemMod(
                    &mut recordedGroups,
                    &mut modSet,
                    &ItemModView::of_data(bData, t),
                    "Item mod",
                );
            }
        }
    }

    // L213
    let mut supportedList: Vec<ModEntry> = Vec::new();
    // L214 ORDER: (c), module notes
    for (template, mut r#mod) in modSet {
        // L216
        // a key which is based on the stripped line, but one that also tries to
        // ignore leading special characters
        // L218-221 (strip left and right spaces)
        let sortKey = gsub(&gsub(&template, "#", " "), "[^%a]+", " ");
        let sortKey = pob_ui_core::luastr::lmatch(&sortKey, "^%s*(.-)%s*$").unwrap_or_default();
        r#mod.template = Some(template);
        r#mod.sortKey = Some(sortKey);
        // L222
        if r#mod.supported == Some(true) {
            // L223
            supportedList.push(r#mod);
        }
    }

    // L227
    pob_lua::sort(&mut supportedList, sortAlphabeticallyIgnoringValues);
    supportedList.into_iter().map(Rc::new).collect()
}

/// `configModBrowser.OpenAddModPopup(configTab, blockData)` (U41's `ConfigHooks` entry).
pub fn open_add_mod_popup_hook(cx: &mut Cx, config_tab: CtrlId, block_data: &Table) {
    OpenAddModPopup(cx, config_tab, block_data);
}

/// lua: ConfigModBrowser.lua:153-309 `M.OpenAddModPopup(configTab, blockData)`
/// @pob Modules/ConfigModBrowser.lua:151-309 h=88e4c2bf
#[allow(clippy::too_many_lines)]
pub fn OpenAddModPopup(cx: &mut Cx, configTab: CtrlId, blockData: &Table) {
    let m = cx.build().model.clone();
    // L154-227
    let supportedList = Rc::new(supported_list(&m));
    // L228
    let displayList: DisplayList = Rc::new(RefCell::new(Vec::new()));
    // L229 local controls = {}
    let blockData = blockData.clone();
    // L306 popupHeight = controls.search.y + controls.search.height + helpSize -
    // controls.whatDoesItDo.y - controls.close.y + controls.close.height + 8 (the raw rect
    // values of the controls below)
    let helpSize = 24.0;
    let popupHeight = 482.0 + 18.0 + helpSize - (-10.0) - (-4.0) + 20.0 + 8.0;

    let dl = displayList.clone();
    let sl = supportedList.clone();
    // L308
    popups::open_popup(
        cx,
        720.0,
        popupHeight,
        "Mod Browser",
        move |cx, p| {
            let p = p.id();
            let displayList = dl;
            let supportedList = sl;
            // L232
            let listControl = cx.add(
                p,
                "listControl",
                ListControl::new(
                    Anchor::nil(TopLeft, TopLeft),
                    rect(10, 20, 700, 454),
                    16.0,
                    true,
                    false,
                    ListData::Source(Rc::new(DisplaySource(displayList.clone()))),
                    false,
                ),
            );
            {
                let w = cx.w_mut::<ListControl>(listControl);
                // L233
                w.force_tooltip = true;
                // L234
                w.font = pob_sg::Font::Var;
                // L235-241
                w.ov.get_row_value = Some(Rc::new(|cx, _me, _column, _index, value| {
                    let value = entry_of(value);
                    // L236
                    if value.text == NO_MATCH_TEXT {
                        // L237
                        value.text.clone()
                    } else {
                        format!("{}{}", cx.main.cc.MAGIC, value.text)
                    }
                }));
                // L243-250
                w.ov.add_value_tooltip = Some(Rc::new(
                    |cx: &mut Cx, _me, tooltip: &mut Tooltip, _index, value: &ListValue| {
                        // L244
                        tooltip.clear(true);
                        let value = entry_of(value);
                        // L245
                        if let Some(sources) = &value.sources {
                            // L246 ORDER: module notes
                            let mut sources: Vec<&String> = sources.iter().collect();
                            sources.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
                            for source in sources {
                                // L247
                                tooltip.add_line(cx, 14.0, &format!("^7{source}"));
                            }
                        }
                    },
                ));
                // L251-256
                w.ov.on_sel_click = Some(Rc::new(move |cx, me, index, _value, doubleClick| {
                    // L252
                    ListControl::select_index(cx, me, index);
                    // L253
                    let save = cx.child(p, "save").unwrap_or_else(|| {
                        lua_error!("attempt to index field 'save' (a nil value)")
                    });
                    if doubleClick && cx.is_enabled(save) {
                        // L254
                        let f = cx.w::<ButtonControl>(save).on_click.clone();
                        f(cx);
                    }
                    None
                }));
            }

            // L258
            cx.add(
                p,
                "searchLabel",
                LabelControl::new(
                    Anchor::nil(TopRight, TopLeft),
                    rect(65, 482, 0, 16),
                    "^7Search:",
                ),
            );
            // L259-261
            let dl2 = displayList.clone();
            let sl2 = supportedList.clone();
            let search = cx.add(
                p,
                "search",
                EditControl::new(
                    Anchor::nil(TopLeft, TopLeft),
                    rect(70, 482, 640, 18),
                    EditArgs {
                        init: Some(String::new()),
                        filter: Some("%c".into()),
                        limit: Some(100),
                        change_func: Some(Rc::new(move |cx: &mut Cx, _buf: &str| {
                            // L260
                            updateDisplayList(cx, p, &dl2, &sl2);
                        })),
                        clearable: true,
                        ..EditArgs::default()
                    },
                ),
            );
            // L262-264
            let clear = cx.child(search, "buttonClear").unwrap_or_else(|| {
                lua_error!("attempt to index field 'buttonClear' (a nil value)")
            });
            cx.base_mut(clear).shown = Prop::f(move |cx, _| {
                // L263
                !cx.w::<EditControl>(search.id()).buf.is_empty()
            });

            // L266
            updateDisplayList(cx, p, &displayList, &supportedList);
            // L267 local helpSize = 24
            // L268
            let whatDoesItDo = cx.add(
                p,
                "whatDoesItDo",
                ButtonControl::new(
                    Anchor::nil(Bottom, Bottom),
                    rect(0, -10, 24, 24),
                    "?",
                    |_| {},
                ),
            );
            // L269
            cx.get_mut(whatDoesItDo).force_tooltip = true;
            // L271-278
            let tip = pob_ui_core::wrap::wrap_string(cx, HELP_TEXT, 16.0, 270.0).join("\n");
            cx.get_mut(whatDoesItDo).tt.tooltip_text = Some(Prop::Val(Some(tip)));

            // L280-294
            let dl3 = displayList.clone();
            let block = blockData.clone();
            let save = cx.add(
                p,
                "save",
                ButtonControl::new(
                    Anchor::new(BottomRight, whatDoesItDo, Top),
                    rect(-2, -4, 80, 20),
                    "Add",
                    move |cx| {
                        // L281
                        let selIndex = cx.w::<ListControl>(listControl.id()).sel_index.unwrap_or(1);
                        // L282
                        let selected = selIndex
                            .checked_sub(1)
                            .and_then(|i| dl3.borrow().get(i).cloned());
                        // L283
                        if let Some(selected) = selected.filter(|s| s.text != NO_MATCH_TEXT) {
                            let text = block.get(n!("text"));
                            // L284
                            if let Some(t) = text.str() {
                                if !t.is_empty()
                                    && pob_lua::pattern::r#match(&t, "\n$", None).is_none()
                                {
                                    // L285
                                    block.set(n!("text"), format!("{t}\n").as_str());
                                }
                            }
                            // L287
                            let cur = block.get(n!("text")).str().map(|s| s.to_string());
                            block.set(
                                n!("text"),
                                format!("{}{}", cur.unwrap_or_default(), selected.text).as_str(),
                            );
                            // L288
                            ConfigTab::update_custom_mods_controls(cx, configTab);
                            // L289
                            ConfigTab::add_undo_state(cx, configTab);
                            // L290
                            let m = cx.build().model.clone();
                            m.configTab.BuildModList(&m);
                            // the bridge (CCR-0110 §3)
                            ConfigTab::pull_var_controls(cx, configTab);
                            // L291
                            m.buildFlag.set(true);
                        }
                        // L293
                        popups::close_popup(cx);
                    },
                ),
            );
            // L295-299
            let dl4 = displayList.clone();
            cx.base_mut(save).enabled = Prop::f(move |cx, _| {
                // L296
                let selIndex = cx.w::<ListControl>(listControl.id()).sel_index.unwrap_or(1);
                // L297
                let l = dl4.borrow();
                let selected = selIndex.checked_sub(1).and_then(|i| l.get(i));
                // L298
                selected.is_some_and(|s| s.text != NO_MATCH_TEXT)
            });

            // L302-304
            cx.add(
                p,
                "close",
                ButtonControl::new(
                    Anchor::new(BottomLeft, whatDoesItDo, Top),
                    rect(2, -4, 80, 20),
                    "Cancel",
                    |cx| {
                        // L303
                        popups::close_popup(cx);
                    },
                ),
            );
        },
        // L308
        PopupOpts {
            enter: Some("save".into()),
            default: Some("search".into()),
            escape: Some("close".into()),
            ..PopupOpts::default()
        },
    );
}

// @pob Modules/ConfigModBrowser.lua:311-311 h=85c34a63
// (`return M`)

#[cfg(test)]
mod tests {
    use super::*;

    fn w(s: &str) -> Vec<String> {
        pob_lua::pattern::gmatch(s, "%S+")
            .map(|c| c[0].as_str().to_owned())
            .collect()
    }

    #[test]
    fn fuzzy_ranks() {
        // a plain substring
        assert_eq!(
            fuzzyScore("+10 to Maximum Life", "maximum life", &w("maximum life")),
            Some(1)
        );
        // every word, not adjacent
        assert_eq!(
            fuzzyScore("+10 to Maximum Life", "life maximum", &w("life maximum")),
            Some(2)
        );
        // one word split over two text words ("maxlife" = "max" + "life")
        assert_eq!(
            fuzzyScore("+10 to Maximum Life", "maxlife", &w("maxlife")),
            Some(3)
        );
        assert_eq!(fuzzyScore("+10 to Maximum Life", "zzz", &w("zzz")), None);
        // a pattern character is plain
        assert_eq!(fuzzyScore("1% increased", "1%", &w("1%")), Some(1));
    }

    #[test]
    fn templates() {
        assert_eq!(
            getModTemplate("+(10-20) to maximum Life"),
            "+# to maximum life"
        );
        assert_eq!(
            getModTemplate("12% increased Damage"),
            "#% increased damage"
        );
        assert_eq!(getModTemplate("Adds 1.5 to 3 Fire"), "adds # to # fire");
    }

    #[test]
    fn add_mod_line_prefers_supported_text() {
        let mut set = ModSet::new();
        addModLine(&mut set, "+10 to Strength", "a", Some(false));
        addModLine(&mut set, "+12 to Strength", "b", Some(true));
        addModLine(&mut set, "+14 to Strength", "c", Some(true));
        let e = &set["+# to strength"];
        assert_eq!(e.text, "+12 to Strength");
        assert_eq!(e.supported, Some(true));
        assert_eq!(e.sources.as_ref().unwrap().len(), 3);
    }
}
