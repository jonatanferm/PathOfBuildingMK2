//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/CompareCalcsHelpers.lua 1-476.
//! Owner: U48.
//!
//! @pob-file Classes/CompareCalcsHelpers.lua @16de4b82d
//!
//! The stateless calcs-tooltip helpers of the Compare tab (`local calcsHelpers =
//! require("Classes.CompareCalcsHelpers")`, CompareTab.lua:14): modifier formatting, source
//! resolution, tabulation, the hover tooltip that splits a cell's modifiers into common /
//! primary-only / compare-only, and the SkillBuffs / SkillDebuffs breakdown panel. Callers are
//! U50's DrawCalcs (CompareTab.lua:4496-4502, 4639-4675, 4920-4930).
//!
//! The `build` arguments are the builds' models (`self.primaryBuild` is `cx.build().model`, a
//! CompareEntry is its `model`; both carry `itemsTab`, `spec`, `latestTree`, `data` and
//! `calcsTab`, as the Lua objects do). `sectionData`/`colData` are the calc tables of
//! `tabs::calcs::sections_data` (CalcSections.lua as data). A modifier row is the
//! `{ value = …, mod = … }` table that `Tabulate` rows become (as in U40's breakdown.rs); a
//! breakdown's `modList` entries already have that shape.
//!
//! Lua semantics:
//! * `FormatCalcModName` returns gsub's two values; every caller uses the first.
//! * `FormatModRow`'s `displayValue` is a string (`FormatCalcModValue`) or a number
//!   (`formatRound(row.value, 2)`): [`Val`]. `pVal == cVal` is Lua `==` (raw equality of
//!   numbers/strings) and `s_format("%-10s", v)` formats it with `tostring`.
//! * `value >= 0` in `FormatCalcModValue` is Lua's comparison (a non-number raises); `value ..
//!   "% increased"` concatenates a number with `tostring` (`%.14g`).
//! * `tostring(value)` of a table value (a LIST mod's) is an address in PoB (`table: 0x…`);
//!   the port prints `table` (DIVERGENCE(tostring-table), never reached by a `modType`-less
//!   CalcSections cell of a numeric mod).
//! * `tooltip:CheckForUpdate(colData, rowLabel)`: `colData` is compared by identity; the key is
//!   the table's address (`Table::as_ptr`; the CalcSections tables live as long as the tab).
//!
//! ORDER: no `pairs` loops (the rows are sequences; `cByKey`/`cMatched` are lookups only).
//! `table.sort` of the common / primary-only / compare-only rows (L261, L284, L300) is Lua 5.1's
//! `auxsort` (`pob_lua::sort`, not stable): rows tied under the comparator keep an order that
//! depends on `Tabulate`'s (U40 ORDER note), as in PoB. The comparator returns false for
//! non-number values (so it is a strict weak order only among numbers, as in the Lua).
//!
//! Props with side effects (ui doc §7.3): none.
#![allow(non_snake_case)]

use pob_lua::{n, tostring_num, Name};
use pob_sg::{Align, Font};
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::{Tooltip, TtKey};

use super::init::CompareEntryRef;
use crate::model::{self, Table, Val};
use crate::tabs::calcs::breakdown::formatRound;
use pob_calc::actor::ActorRef;
use pob_calc::cfg::{Cfg, ModCfg};
use pob_calc::classes::ModStoreRef;
use pob_gamedata::ModType;

// @pob Classes/CompareCalcsHelpers.lua:1-11 h=8c90008c
// (header comment, the local aliases and `local M = {}`: this module)

// ---- small helpers (Rust-only) ----------------------------------------------------------------

/// Lua `tostring(v)` of a modifier value.
fn tostring_val(v: &Val) -> String {
    match v {
        Val::Nil => "nil".to_owned(),
        Val::Bool(b) => b.to_string(),
        Val::Num(x) => tostring_num(*x),
        Val::Str(s) => s.to_string(),
        // DIVERGENCE(tostring-table): see the module notes
        Val::Table(_) | Val::Data(_) | Val::Mod(_) => "table".to_owned(),
        Val::Obj(_) => "userdata".to_owned(),
    }
}

/// `a .. b` where `a` is a string or a number (anything else raises).
fn concat_str(v: &Val, what: &str) -> String {
    match v {
        Val::Str(s) => s.to_string(),
        Val::Num(x) => tostring_num(*x),
        other => lua_error!("attempt to concatenate {what} (a {} value)", other.lua_type()),
    }
}

/// Lua `a >= b` with a number `b`.
fn ge0(v: &Val) -> bool {
    match v {
        Val::Num(x) => *x >= 0.0,
        other => lua_error!("attempt to compare number with {}", other.lua_type()),
    }
}

/// `s:match(p)` with one capture (`None`: no match).
fn match1(s: &str, p: &str) -> Option<String> {
    pob_ui_core::luastr::lmatch(s, p)
}

/// A string field used as a name (`nil` and non-strings: `None`).
fn opt_name(v: &Val) -> Option<Name> {
    v.str().map(|s| Name::from(&s))
}

/// `actor.mainSkill[name]` for a cfg name (`"skill" .. "Cfg"`, …): the typed cfg fields, then
/// the computed ones in `extra` (as U40's `main_skill_cfg`).
fn main_skill_cfg(skill: &pob_calc::active_skill::ActiveSkill, name: &str) -> Option<Cfg> {
    match name {
        "skillCfg" => Some(skill.skillCfg.get()),
        "weapon1Cfg" | "weapon2Cfg" | "bleedCfg" | "poisonCfg" | "igniteCfg" | "OHbleedCfg"
        | "OHpoisonCfg" | "OHigniteCfg" | "dotCfg" | "decayCfg" => skill.cfg_field(Name::from(name)).get(),
        _ => skill.extra.get(Name::from(name)).obj::<ModCfg>(),
    }
}

/// `table.sort(rows, function(a, b) if type(a.value) == "number" and type(b.value) == "number"
/// then return a.value > b.value end return false end)` over `get`'s rows.
fn sort_by_value_desc<T>(rows: &mut [T], get: impl Fn(&T) -> Val) {
    pob_lua::sort::sort(rows, |a, b| match (get(a), get(b)) {
        (Val::Num(x), Val::Num(y)) => x > y,
        _ => false,
    });
}

/// `{ value = row.value, mod = row.mod }` of a Tabulate row.
fn row_table(r: &pob_calc::classes::TabRow) -> Table {
    let t = Table::new();
    t.set(n!("value"), Val::from_value(&r.value));
    t.set(n!("mod"), Val::Mod(r.r#mod.clone()));
    t
}

/// A breakdown value with a nested breakdown instance as its data table (CCR-0144 §2; as
/// `build::stats`' `bd_val`).
fn bd_val(v: Val) -> Val {
    match v.obj::<pob_calc::breakdown::Breakdown>() {
        Some(b) => Val::Table(b.t.clone()),
        None => v,
    }
}

// ---- the helpers ------------------------------------------------------------------------------

/// Format a modifier value with its type for display
/// lua: CompareCalcsHelpers.lua:13-35 `M.FormatCalcModValue(value, modType)`
/// @pob Classes/CompareCalcsHelpers.lua:13-35 h=8e9fdd42
pub fn FormatCalcModValue(value: &Val, modType: &Val) -> String {
    let mt = modType.str();
    let mt = mt.as_deref();
    // L14
    if mt == Some("BASE") {
        // L15
        pob_lua::lua_format("%+g base", &[pob_lua::FmtArg::from(value.num())])
    // L16
    } else if mt == Some("INC") {
        // L17
        if ge0(value) {
            // L18
            format!("{}% increased", concat_str(value, "local 'value'"))
        } else {
            // L20
            format!("{}% reduced", tostring_num(-value.num()))
        }
    // L22
    } else if mt == Some("MORE") {
        // L23
        if ge0(value) {
            // L24
            format!("{}% more", concat_str(value, "local 'value'"))
        } else {
            // L26
            format!("{}% less", tostring_num(-value.num()))
        }
    // L28
    } else if mt == Some("OVERRIDE") {
        // L29
        format!("Override: {}", tostring_val(value))
    // L30
    } else if mt == Some("FLAG") {
        // L31
        if value.truthy() { "True" } else { "False" }.to_owned()
    } else {
        // L33
        tostring_val(value)
    }
}

/// Format CamelCase mod name to spaced words
/// lua: CompareCalcsHelpers.lua:37-40 `M.FormatCalcModName(modName)` (the first of gsub's
/// two values)
/// @pob Classes/CompareCalcsHelpers.lua:37-40 h=47ed2d35
pub fn FormatCalcModName(modName: &str) -> String {
    // L39
    let (s, _) = pob_lua::pattern::gsub(modName, "([%l%d]:?)(%u)", "%1 %2", None);
    let (s, _) = pob_lua::pattern::gsub(&s, "(%l)(%d)", "%1 %2", None);
    s
}

/// Resolve a modifier's source to a human-readable name
/// lua: CompareCalcsHelpers.lua:42-76 `M.ResolveSourceName(mod, build)`
/// @pob Classes/CompareCalcsHelpers.lua:42-76 h=19bb8268
pub fn ResolveSourceName(cx: &Cx, r#mod: &Val, build: &model::BuildRef) -> String {
    // L46
    let Some(source) = r#mod.get(n!("source")).str().filter(|_| r#mod.get(n!("source")).truthy()) else {
        return String::new();
    };
    // L47
    let sourceType = match1(&source, "[^:]+").unwrap_or_default();
    // L48
    if sourceType == "Item" {
        // L49
        let itemId = match1(&source, "Item:(%d+):.+");
        // L50 (`tonumber(nil)` is nil; `items[nil]` reads nil)
        let item = itemId
            .as_deref()
            .and_then(pob_lua::tonumber)
            .filter(|id| id.fract() == 0.0 && *id >= 0.0 && *id <= f64::from(u32::MAX))
            .and_then(|id| build.itemsTab.items.with(|m| m.get(&(id as u32)).cloned()));
        // L51
        if let Some(item) = item {
            // L52
            let rarity = item.rarity.get();
            let Some(color) = cx.main.cc.get(rarity.as_str()) else {
                lua_error!("attempt to concatenate a nil value")
            };
            return format!("{color}{}", item.name.get());
        }
    // L54
    } else if sourceType == "Tree" {
        // L55
        let nodeId = match1(&source, "Tree:(%d+)");
        // L56
        if let Some(nodeId) = nodeId {
            // L57
            let nodeIdNum = pob_lua::tonumber(&nodeId).unwrap_or(f64::NAN);
            let key = nodeIdNum as i64;
            // L58-60
            let spec = build.spec.get();
            let node = spec
                .nodes
                .with(|m| m.get(key))
                .or_else(|| spec.tree.get().nodes.get(key))
                .or_else(|| build.latestTree.get().and_then(|t| t.nodes.get(key)));
            // L61
            if let Some(node) = node {
                // L62 (`node.dn` is always a string)
                return node.dn.get().to_string();
            }
        }
    // L65
    } else if sourceType == "Skill" {
        // L66
        let skillId = match1(&source, "Skill:(.+)");
        // L67
        if let Some(ge) = skillId.and_then(|id| build.data.skills.get(&Name::from(id.as_str())).cloned()) {
            // L68
            return ge.name.to_string();
        }
    // L70
    } else if sourceType == "Pantheon" {
        // L71
        return match1(&source, "Pantheon:(.+)").unwrap_or_default();
    // L72
    } else if sourceType == "Spectre" {
        // L73
        return match1(&source, "Spectre:(.+)").unwrap_or_default();
    }
    // L75
    String::new()
}

/// Get the modDB and config for a sectionData entry and actor
/// lua: CompareCalcsHelpers.lua:78-96 `M.GetModStoreAndCfg(sectionData, actor)`
/// @pob Classes/CompareCalcsHelpers.lua:78-96 h=69c60ec2
pub fn GetModStoreAndCfg(sectionData: &Table, actor: &ActorRef) -> (ModStoreRef, Cfg) {
    // L80
    let mut cfg: Cfg = ModCfg::new();
    let sCfg = sectionData.get(n!("cfg"));
    // L81
    if sCfg.truthy() {
        if let Some(skill) = actor.mainSkill.get() {
            let name = format!("{}Cfg", concat_str(&sCfg, "field 'cfg'"));
            if let Some(c) = main_skill_cfg(&skill, &name) {
                // L82
                cfg = c.copyTable(true);
            }
        }
    }
    // L84
    cfg.source.set(opt_name(&sectionData.get(n!("modSource"))));
    // L85
    cfg.actor.set(opt_name(&sectionData.get(n!("actor"))));

    // L87-94
    let enemy = actor.enemy().filter(|_| sectionData.truthy(n!("enemy")));
    let modStore: ModStoreRef = if let Some(enemy) = enemy {
        // L89
        enemy.modDB.get()
    } else if let (true, Some(skill)) = (sCfg.truthy(), actor.mainSkill.get()) {
        // L91
        skill.skillModList.get()
    } else {
        // L93
        actor.modDB.get()
    };
    // L95
    (modStore, cfg)
}

/// Tabulate modifiers for a sectionData entry and actor (the rows as `{ value, mod }` tables)
/// lua: CompareCalcsHelpers.lua:98-110 `M.TabulateMods(sectionData, actor)`
/// @pob Classes/CompareCalcsHelpers.lua:98-110 h=b41fea8a
pub fn TabulateMods(sectionData: &Table, actor: &ActorRef) -> Vec<Table> {
    // L100 (`modStore` is always set)
    let (modStore, cfg) = GetModStoreAndCfg(sectionData, actor);

    let sModType = sectionData.get(n!("modType"));
    let modType = match &sModType {
        Val::Nil | Val::Bool(false) => None,
        v => Some(ModType::parse(&concat_str(v, "field 'modType'")).unwrap_or(ModType::Nil)),
    };
    let modName = sectionData.get(n!("modName"));
    // L104
    let names: Vec<Name> = if let Val::Table(t) = &modName {
        // L105 (`unpack(sectionData.modName)`)
        t.ipairs()
            .iter()
            .map(|(_, v)| Name::from(concat_str(v, "a mod name").as_str()))
            .collect()
    } else {
        // L107
        vec![Name::from(concat_str(&modName, "field 'modName'").as_str())]
    };
    let rowList = modStore.Tabulate(modType, Some(&*cfg), &names);
    // L109
    rowList.iter().map(row_table).collect()
}

/// Build a unique key for a modifier row to match between builds
/// lua: CompareCalcsHelpers.lua:112-121 `M.ModRowKey(row)`
/// @pob Classes/CompareCalcsHelpers.lua:112-121 h=e3b430dc
pub fn ModRowKey(row: &Table) -> String {
    let m = row.get(n!("mod"));
    let or_empty = |v: Val| if v.truthy() { concat_str(&v, "a mod field") } else { String::new() };
    // L114
    let src = or_empty(m.get(n!("source")));
    // L115
    let name = or_empty(m.get(n!("name")));
    // L116
    let mtype = or_empty(m.get(n!("type")));
    // Normalize Item sources by stripping the build-specific numeric ID
    // "Item:5:Body Armour" -> "Item:Body Armour" so same items match across builds
    // L119
    let (normalizedSrc, _) = pob_lua::pattern::gsub(&src, "^(Item):%d+:", "%1:", None);
    // L120
    format!("{normalizedSrc}|{name}|{mtype}")
}

/// Format a single modifier row as a tooltip line: `displayValue, sourceType, sourceName,
/// modName`
/// lua: CompareCalcsHelpers.lua:123-140 `M.FormatModRow(row, sectionData, build)`
/// @pob Classes/CompareCalcsHelpers.lua:123-140 h=8f6acb4f
pub fn FormatModRow(cx: &Cx, row: &Table, sectionData: &Table, build: &model::BuildRef) -> (Val, String, String, String) {
    let m = row.get(n!("mod"));
    let value = row.get(n!("value"));
    // L125
    // L126
    let displayValue = if !sectionData.truthy(n!("modType")) {
        // L127
        Val::from(FormatCalcModValue(&value, &m.get(n!("type"))))
    } else {
        // L129
        Val::Num(formatRound(value.num(), Some(2.0)))
    };

    // L132
    let source = m.get(n!("source"));
    let sourceType = match source.str().filter(|_| source.truthy()) {
        Some(s) => match1(&s, "[^:]+").unwrap_or_else(|| "?".to_owned()),
        None => "?".to_owned(),
    };
    // L133
    let sourceName = ResolveSourceName(cx, &m, build);
    // L134
    let mut modName = String::new();
    // L135
    if matches!(sectionData.get(n!("modName")), Val::Table(_)) {
        // L136
        modName = format!("  {}", FormatCalcModName(&concat_str(&m.get(n!("name")), "field 'name'")));
    }

    // L139
    (displayValue, sourceType, sourceName, modName)
}

/// Get breakdown text lines for a build's actor
/// lua: CompareCalcsHelpers.lua:142-167 `M.GetBreakdownLines(sectionData, build)`
/// @pob Classes/CompareCalcsHelpers.lua:142-167 h=59df3bee
pub fn GetBreakdownLines(sectionData: &Table, build: &model::BuildRef) -> Option<Vec<String>> {
    // L146
    let key = sectionData.get(n!("breakdown"));
    if !key.truthy() {
        return None;
    }
    // L147
    let calcsActor = build.calcsTab.calcsEnv.get().map(|e| e.player.clone());
    // L148
    let actorBreakdown = calcsActor.and_then(|a| a.breakdown.get())?;

    // L150
    let breakdown: Val;
    // L151
    let key = concat_str(&key, "field 'breakdown'");
    let m = pob_lua::lmatch(&key, "^(%a+)%.(%a+)$", None);
    // L152
    if let Some(m) = m {
        let (ns, name) = (m[0].to_str(), m[1].to_str());
        // L153
        // CCR-0144 §2: `breakdown.MainHand`/`.OffHand` are nested breakdown instances
        // (`Val::Obj(Breakdown)`), plain tables in Lua
        let t = bd_val(actorBreakdown.get(Name::from(&ns)));
        breakdown = if t.truthy() { t.get(Name::from(&name)) } else { t };
    } else {
        // L155
        breakdown = actorBreakdown.get(Name::from(key.as_str()));
    }

    // L158
    let breakdown = match &breakdown {
        Val::Table(t) if !t.is_empty() => t.clone(),
        Val::Nil | Val::Bool(false) | Val::Table(_) => return None,
        other => lua_error!("attempt to get length of local 'breakdown' (a {} value)", other.lua_type()),
    };

    // L160
    let mut lines = Vec::new();
    // L161
    for (_, line) in breakdown.ipairs() {
        // L162
        if let Val::Str(s) = line {
            // L163
            lines.push(s.to_string());
        }
    }
    // L166
    (!lines.is_empty()).then_some(lines)
}

/// One row pair of the common modifiers.
type RowPair = (Table, Table);

/// Draw the calcs hover tooltip showing breakdown for both builds with common/unique grouping
/// (`tooltip`, `primaryBuild`, `primaryLabel` passed as args instead of self)
/// lua: CompareCalcsHelpers.lua:169-324 `M.DrawCalcsTooltip(tooltip, primaryBuild, primaryLabel,
/// colData, rowLabel, rowX, rowY, rowW, rowH, vp, compareEntry)`
/// @pob Classes/CompareCalcsHelpers.lua:169-324 h=242e5c09
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub fn DrawCalcsTooltip(
    cx: &mut Cx,
    tooltip: &mut Tooltip,
    primaryBuild: &model::BuildRef,
    primaryLabel: &str,
    colData: &Table,
    rowLabel: Option<&str>,
    rowX: f64,
    rowY: f64,
    rowW: f64,
    rowH: f64,
    vp: &ViewPort,
    compareEntry: &CompareEntryRef,
) {
    let cmp = &compareEntry.model;
    // L174 (`colData` by identity: module notes)
    let keys = [
        TtKey::Num(colData.as_ptr() as usize as f64),
        rowLabel.map_or(TtKey::Nil, |s| TtKey::Str(s.into())),
    ];
    if tooltip.check_for_update(cx, &keys) {
        // Get calcsEnv actors (these have breakdown data populated)
        // L176-179
        let primaryCalcsActor = primaryBuild.calcsTab.calcsEnv.get().map(|e| e.player.clone());
        let compareCalcsActor = cmp.calcsTab.calcsEnv.get().map(|e| e.player.clone());

        // L181
        let primaryActor =
            primaryCalcsActor.or_else(|| primaryBuild.calcsTab.mainEnv.get().map(|e| e.player.clone()));
        // L182
        let compareActor = compareCalcsActor.or_else(|| cmp.calcsTab.mainEnv.get().map(|e| e.player.clone()));

        // L184
        if primaryActor.is_none() && compareActor.is_none() {
            // L185
            return;
        }

        // L188 (`compareEntry.label` is always set)
        let compareLabel = compareEntry.label.clone();
        let (positive, warning) = (cx.main.cc.POSITIVE.clone(), cx.main.cc.WARNING.clone());

        // Tooltip header
        // L191
        tooltip.add_line(cx, 16.0, &format!("^7{}", rowLabel.unwrap_or("")));
        // L192
        tooltip.add_separator(cx, Some(10.0));

        // Process each sectionData entry in colData
        // L195
        for (_, sectionData) in colData.ipairs() {
            let Some(sectionData) = sectionData.table() else {
                lua_error!("attempt to index local 'sectionData' (a {} value)", sectionData.lua_type())
            };
            // Show breakdown formulas per build (these are always build-specific)
            // L197
            if sectionData.truthy(n!("breakdown")) {
                // L198
                let primaryLines = GetBreakdownLines(&sectionData, primaryBuild);
                // L199
                let compareLines = GetBreakdownLines(&sectionData, cmp);

                // L201
                if let Some(lines) = &primaryLines {
                    // L202
                    tooltip.add_line(cx, 14.0, &format!("{positive}{primaryLabel}:"));
                    // L203
                    for line in lines {
                        // L204
                        tooltip.add_line(cx, 14.0, &format!("^7  {line}"));
                    }
                }
                // L207
                if let Some(lines) = &compareLines {
                    // L208
                    tooltip.add_line(cx, 14.0, &format!("{warning}{compareLabel}:"));
                    // L209
                    for line in lines {
                        // L210
                        tooltip.add_line(cx, 14.0, &format!("^7  {line}"));
                    }
                }
                // L213
                if primaryLines.is_some() || compareLines.is_some() {
                    // L214
                    tooltip.add_separator(cx, Some(10.0));
                }
            }

            // Show modifier sources split into common / primary-only / compare-only
            // L219
            if sectionData.truthy(n!("modName")) {
                // L220
                let pRows = primaryActor.as_ref().map(|a| TabulateMods(&sectionData, a)).unwrap_or_default();
                // L221
                let cRows = compareActor.as_ref().map(|a| TabulateMods(&sectionData, a)).unwrap_or_default();

                // L223
                if !pRows.is_empty() || !cRows.is_empty() {
                    // Build lookup of compare rows by key
                    // L225
                    let mut cByKey: std::collections::HashMap<String, Table> = std::collections::HashMap::new();
                    // L226
                    for row in &cRows {
                        // L227
                        let key = ModRowKey(row);
                        // L228
                        cByKey.insert(key, row.clone());
                    }

                    // Classify into common, primary-only, compare-only
                    // L232
                    let mut common: Vec<RowPair> = Vec::new();
                    // L233
                    let mut pOnly: Vec<Table> = Vec::new();
                    // L234
                    let mut cMatched: std::collections::HashSet<String> = std::collections::HashSet::new();

                    // L236
                    for pRow in &pRows {
                        // L237
                        let key = ModRowKey(pRow);
                        // L238
                        if let Some(c) = cByKey.get(&key) {
                            // L239
                            common.push((pRow.clone(), c.clone()));
                            // L240
                            cMatched.insert(key);
                        } else {
                            // L242
                            pOnly.push(pRow.clone());
                        }
                    }

                    // L246
                    let mut cOnly: Vec<Table> = Vec::new();
                    // L247
                    for cRow in &cRows {
                        // L248
                        let key = ModRowKey(cRow);
                        // L249
                        if !cMatched.contains(&key) {
                            // L250
                            cOnly.push(cRow.clone());
                        }
                    }

                    // Sub-section header (e.g., "Sources", "Increased Life Regeneration Rate")
                    // L255
                    let label = sectionData.get(n!("label"));
                    let sectionLabel = if label.truthy() {
                        concat_str(&label, "local 'sectionLabel'")
                    } else {
                        "Player modifiers".to_owned()
                    };
                    // L256
                    tooltip.add_line(cx, 14.0, &format!("^7{sectionLabel}:"));

                    // Common modifiers
                    // L259
                    if !common.is_empty() {
                        // Sort by primary value descending
                        // L261-266
                        sort_by_value_desc(&mut common, |p| p.0.get(n!("value")));
                        // L267
                        tooltip.add_line(cx, 12.0, "^x808080  Common:");
                        // L268
                        for pair in &common {
                            // L269
                            let (pVal, sourceType, sourceName, modName) =
                                FormatModRow(cx, &pair.0, &sectionData, primaryBuild);
                            // L270
                            let (cVal, ..) = FormatModRow(cx, &pair.1, &sectionData, cmp);
                            // L271
                            // L272
                            let valStr = if pVal.raw_eq(&cVal) {
                                // L273
                                pob_lua::lua_format("^7%-10s", &[pob_lua::FmtArg::from(&tostring_val(&pVal))])
                            } else {
                                // L275
                                format!(
                                    "{positive}{}^7/{warning}{}",
                                    pob_lua::lua_format("%-5s", &[pob_lua::FmtArg::from(&tostring_val(&pVal))]),
                                    pob_lua::lua_format("%-5s", &[pob_lua::FmtArg::from(&tostring_val(&cVal))])
                                )
                            };
                            // L277
                            let line = pob_lua::lua_format(
                                "    %s ^7%-6s ^7%s%s",
                                &[
                                    pob_lua::FmtArg::from(&valStr),
                                    pob_lua::FmtArg::from(&sourceType),
                                    pob_lua::FmtArg::from(&sourceName),
                                    pob_lua::FmtArg::from(&modName),
                                ],
                            );
                            // L278
                            tooltip.add_line(cx, 12.0, &line);
                        }
                    }

                    // Primary-only modifiers
                    // L283
                    if !pOnly.is_empty() {
                        // L284-289
                        sort_by_value_desc(&mut pOnly, |r| r.get(n!("value")));
                        // L290
                        tooltip.add_line(cx, 12.0, &format!("{positive}  {primaryLabel} only:"));
                        // L291
                        for row in &pOnly {
                            // L292
                            let (displayValue, sourceType, sourceName, modName) =
                                FormatModRow(cx, row, &sectionData, primaryBuild);
                            // L293
                            let line = only_line(&displayValue, sourceType, sourceName, modName);
                            // L294
                            tooltip.add_line(cx, 12.0, &line);
                        }
                    }

                    // Compare-only modifiers
                    // L299
                    if !cOnly.is_empty() {
                        // L300-305
                        sort_by_value_desc(&mut cOnly, |r| r.get(n!("value")));
                        // L306
                        tooltip.add_line(cx, 12.0, &format!("{warning}  {compareLabel} only:"));
                        // L307
                        for row in &cOnly {
                            // L308
                            let (displayValue, sourceType, sourceName, modName) =
                                FormatModRow(cx, row, &sectionData, cmp);
                            // L309
                            let line = only_line(&displayValue, sourceType, sourceName, modName);
                            // L310
                            tooltip.add_line(cx, 12.0, &line);
                        }
                    }

                    // Separator between sub-sections
                    // L315
                    tooltip.add_separator(cx, Some(6.0));
                }
            }
        }
    }

    // L321
    cx.sg.set_draw_layer(None, Some(100));
    // L322
    tooltip.draw(cx, rowX, rowY, Some(rowW), Some(rowH), vp);
    // L323
    cx.sg.set_draw_layer(None, Some(0));
}

/// `s_format("    ^7%-10s ^7%-6s ^7%s%s", displayValue, sourceType, sourceName, modName)`
/// (L293, L309)
fn only_line(displayValue: &Val, sourceType: String, sourceName: String, modName: String) -> String {
    pob_lua::lua_format(
        "    ^7%-10s ^7%-6s ^7%s%s",
        &[
            pob_lua::FmtArg::from(&tostring_val(displayValue)),
            pob_lua::FmtArg::from(&sourceType),
            pob_lua::FmtArg::from(&sourceName),
            pob_lua::FmtArg::from(&modName),
        ],
    )
}

/// Resolve a modifier's source name for breakdown panel display: `sourceType, sourceName`
/// lua: CompareCalcsHelpers.lua:326-360 `local function resolveModSource(mod, build)`
/// @pob Classes/CompareCalcsHelpers.lua:326-360 h=7beaff96
fn resolveModSource(cx: &Cx, r#mod: &Val, build: &model::BuildRef) -> (String, String) {
    let source = r#mod.get(n!("source"));
    let source = source.str().filter(|_| source.truthy());
    // L330
    let sourceType = match &source {
        Some(s) => match1(s, "[^:]+").unwrap_or_else(|| "?".to_owned()),
        None => "?".to_owned(),
    };
    // L331
    let mut sourceName = String::new();
    // L332-358 (the same branches as ResolveSourceName, which a nil source cannot reach: its
    // `sourceType` is "?")
    if source.is_some() && matches!(sourceType.as_str(), "Item" | "Tree" | "Skill" | "Pantheon" | "Spectre") {
        sourceName = ResolveSourceName(cx, r#mod, build);
    }
    // L359
    (sourceType, sourceName)
}

/// Draw a breakdown panel for a single build's SkillBuffs or SkillDebuffs,
/// lua: CompareCalcsHelpers.lua:362-474 `M.DrawSkillBreakdownPanel(build, breakdownKey, label,
/// cellX, cellY, cellW, cellH, vp)`
/// @pob Classes/CompareCalcsHelpers.lua:362-474 h=1988a774
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub fn DrawSkillBreakdownPanel(
    cx: &mut Cx,
    build: &model::BuildRef,
    breakdownKey: &str,
    _label: Option<&str>,
    cellX: f64,
    cellY: f64,
    cellW: f64,
    _cellH: f64,
    vp: &ViewPort,
) {
    // L365-367
    let player = build.calcsTab.calcsEnv.get().map(|e| e.player.clone());
    let Some(playerBreakdown) = player.and_then(|p| p.breakdown.get()) else {
        return;
    };

    // L369
    let breakdown = playerBreakdown.get(Name::from(breakdownKey));
    // L370
    if !breakdown.truthy() {
        return;
    }
    let modList = breakdown.get(n!("modList"));
    let modList = match &modList {
        Val::Table(t) if !t.is_empty() => t.clone(),
        Val::Nil | Val::Bool(false) | Val::Table(_) => return,
        other => lua_error!("attempt to get length of field 'modList' (a {} value)", other.lua_type()),
    };

    // L372 (local modList = breakdown.modList)

    // Sort by mod name then value
    // L375
    let mut rowList: Vec<Val> = Vec::new();
    // L376
    for (_, entry) in modList.ipairs() {
        // L377
        rowList.push(entry);
    }
    // L379-383
    pob_lua::sort::sort(&mut rowList, |a, b| {
        let (an, bn) = (a.get(n!("mod")).get(n!("name")), b.get(n!("mod")).get(n!("name")));
        let (an, bn) = (concat_str(&an, "field 'name'"), concat_str(&bn, "field 'name'"));
        an.as_bytes() > bn.as_bytes()
            || (an == bn
                && match (a.get(n!("value")), b.get(n!("value"))) {
                    (Val::Num(x), Val::Num(y)) => x > y,
                    _ => false,
                })
    });

    // Process rows: compute display strings and measure column widths
    // L386-391
    struct ColDef {
        label: &'static str,
        key: usize,
        width: f64,
        x: f64,
    }
    let mut colDefs = [
        ColDef { label: "Value", key: 0, width: 0.0, x: 0.0 },
        ColDef { label: "Stat", key: 1, width: 0.0, x: 0.0 },
        ColDef { label: "Source", key: 2, width: 0.0, x: 0.0 },
        ColDef { label: "Source Name", key: 3, width: 0.0, x: 0.0 },
    ];

    // L393 (`row.displayValue`, `row.name`, `row.source`, `row.sourceName`: always strings)
    let mut rows: Vec<[String; 4]> = Vec::new();
    // L394
    for entry in &rowList {
        // L395
        let m = entry.get(n!("mod"));
        // L396-401
        let displayValue = FormatCalcModValue(&entry.get(n!("value")), &m.get(n!("type")));
        let name = m.get(n!("name"));
        let name = FormatCalcModName(&if name.truthy() { concat_str(&name, "field 'name'") } else { String::new() });
        // L399
        let (sourceType, sourceName) = resolveModSource(cx, &m, build);
        // L402
        rows.push([displayValue, name, sourceType, sourceName]);
    }

    // Measure column widths
    // L406
    for col in &mut colDefs {
        // L407
        col.width = cx.sg.draw_string_width(16.0, Font::Var, col.label) + 6.0;
        // L408
        for row in &rows {
            // L409-410
            col.width = col.width.max(cx.sg.draw_string_width(12.0, Font::Var, &row[col.key]) + 6.0);
        }
    }

    // Calculate panel size
    // L416
    let panelPadding = 4.0;
    // L417
    let headerRowH = 20.0;
    // L418
    let dataRowH = 14.0;
    // L419
    let mut panelW = panelPadding;
    // L420
    for col in &colDefs {
        // L421
        panelW += col.width;
    }
    // L423
    let panelH = headerRowH + rows.len() as f64 * dataRowH + 4.0;

    // Position panel next to the hovered cell (right side, or left if no room)
    // L426
    let mut panelX = cellX + cellW + 5.0;
    // L427
    if panelX + panelW > vp.x + vp.width {
        // L428
        panelX = vp.x.max(cellX - 5.0 - panelW);
    }
    // L430
    let panelY = cellY.min(vp.y + vp.height - panelH);

    // Draw background
    // L433
    cx.sg.set_draw_layer(None, Some(10));
    // L434
    cx.sg.set_draw_color_a(0.0, 0.0, 0.0, 0.9);
    // L435
    cx.sg.draw_image(None, panelX + 2.0, panelY + 2.0, panelW - 4.0, panelH - 4.0);

    // Draw border
    // L438
    cx.sg.set_draw_layer(None, Some(11));
    // L439
    cx.sg.set_draw_color(0.33, 0.66, 0.33);
    // L440-443
    cx.sg.draw_image(None, panelX, panelY, panelW, 2.0);
    cx.sg.draw_image(None, panelX, panelY + panelH - 2.0, panelW, 2.0);
    cx.sg.draw_image(None, panelX, panelY, 2.0, panelH);
    cx.sg.draw_image(None, panelX + panelW - 2.0, panelY, 2.0, panelH);
    // L444
    cx.sg.set_draw_layer(None, Some(10));

    // Draw column headers and separators
    // L447
    let mut colX = panelX + panelPadding;
    // L448
    for (i, col) in colDefs.iter_mut().enumerate() {
        // L449
        col.x = colX;
        // L450
        if i > 0 {
            // L451
            cx.sg.set_draw_color(0.5, 0.5, 0.5);
            // L452
            cx.sg.draw_image(None, colX - 2.0, panelY + 2.0, 1.0, panelH - 4.0);
        }
        // L454
        cx.sg.set_draw_color(1.0, 1.0, 1.0);
        // L455
        cx.sg.draw_string(colX, panelY + 2.0, Align::Left, 16.0, Font::Var, col.label);
        // L456
        colX += col.width;
    }

    // Draw rows
    // L460
    let mut rowY = panelY + headerRowH;
    // L461
    for row in &rows {
        // Row separator
        // L463
        cx.sg.set_draw_color(0.5, 0.5, 0.5);
        // L464
        cx.sg.draw_image(None, panelX + 2.0, rowY - 1.0, panelW - 4.0, 1.0);
        // L465
        for col in &colDefs {
            // L466
            if !row[col.key].is_empty() {
                // L467
                cx.sg.draw_string(col.x, rowY + 1.0, Align::Left, 12.0, Font::Var, &format!("^7{}", row[col.key]));
            }
        }
        // L470
        rowY += dataRowH;
    }

    // L473
    cx.sg.set_draw_layer(None, Some(0));
}

// @pob Classes/CompareCalcsHelpers.lua:476-476 h=85c34a63
// (`return M`: this module)

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_mod_value() {
        assert_eq!(FormatCalcModValue(&Val::Num(12.0), &Val::from("BASE")), "+12 base");
        assert_eq!(FormatCalcModValue(&Val::Num(-3.5), &Val::from("BASE")), "-3.5 base");
        assert_eq!(FormatCalcModValue(&Val::Num(20.0), &Val::from("INC")), "20% increased");
        assert_eq!(FormatCalcModValue(&Val::Num(-20.0), &Val::from("INC")), "20% reduced");
        assert_eq!(FormatCalcModValue(&Val::Num(0.0), &Val::from("MORE")), "0% more");
        assert_eq!(FormatCalcModValue(&Val::Num(-10.0), &Val::from("MORE")), "10% less");
        assert_eq!(FormatCalcModValue(&Val::Num(5.0), &Val::from("OVERRIDE")), "Override: 5");
        assert_eq!(FormatCalcModValue(&Val::Bool(true), &Val::from("FLAG")), "True");
        assert_eq!(FormatCalcModValue(&Val::Nil, &Val::from("FLAG")), "False");
        assert_eq!(FormatCalcModValue(&Val::Num(1.25), &Val::Nil), "1.25");
    }

    #[test]
    fn format_mod_name() {
        assert_eq!(FormatCalcModName("LifeRegenPercent"), "Life Regen Percent");
        assert_eq!(FormatCalcModName("Damage2Life"), "Damage 2 Life");
        assert_eq!(FormatCalcModName("ElementalDamage:Fire"), "Elemental Damage: Fire");
    }

    #[test]
    fn mod_row_key() {
        let m = Table::new();
        m.set(n!("source"), "Item:5:Body Armour");
        m.set(n!("name"), "Life");
        m.set(n!("type"), "BASE");
        let row = Table::new();
        row.set(n!("mod"), m);
        assert_eq!(ModRowKey(&row), "Item:Body Armour|Life|BASE");
        let m2 = Table::new();
        let row2 = Table::new();
        row2.set(n!("mod"), m2);
        assert_eq!(ModRowKey(&row2), "||");
    }
}
