//! @pob-file Classes/Item.lua @16de4b82d
//! @pob-file Modules/Common.lua @16de4b82d
//! Item:ParseRaw (Classes/Item.lua 424-1641), plus Common.lua `escapeGGGString` (1110-1116,
//! outside the calc scope; ParseRaw is its only calc-side caller).
//!
//! Owner: IT-A.
//!
//! **Field shapes** follow CCR-0032 §2 (IT-B): untyped `self.<x>` fields live in `Item.extra`
//! under their Lua name with their Lua shape (strings `Val::Str`, numbers `Val::Num`, booleans
//! `Val::Bool`, tables `Table`); the influence flags are in `Item.influences`; mod lines are
//! `Table`s with their Lua keys. IT-A's own choices (CCR-0025 §2):
//! * a mod line's `modList` is a list `Table` of `Val::Mod` (the fresh mods `parseMod`
//!   returned; `{ }` when it returned nil);
//! * `affixes` is `Val::Obj(Rc<item::ItemAffixes>)` (CCR-0032 §2); `rareLikeUnique`,
//!   `enchantments` and `mutatedLines` (references into `data`) are `Val::Obj` of the data `Arc`
//!   ([`RareLikeUniqueRef`], [`EnchantmentSlotRef`], `Arc<FoulbornLines>`), read through
//!   [`Item::rareLikeUnique_ref`] / [`Item::enchantments_ref`] / [`Item::mutatedLines_ref`];
//! * `clusterJewel` (`data.clusterJewels.jewels[baseName]`, typed `Field<Option<Table>>` by U0)
//!   is a `Table` copy of the data record with its Lua keys ([`clusterJewel_table`]);
//! * `pendingAffixList` entries are `{ modId = <string>, table = <self.prefixes|self.suffixes> }`
//!   (the `table` member aliases the item's list, as in Lua).
//!
//! `main.defaultItemAffixQuality` (Modules/Main.lua:113, a UI setting) has no home in the
//! frozen contract: it is the thread-local [`main_defaultItemAffixQuality`] here (CCR-0025 §1).
//!
//! Callees that other units own: `itemLib.applyRange` and `Item:BuildModList` / `Item:CanHaveMod`
//! (IT-C), `modLib.parseMod` (MP0-MP6). They are called through thin wrappers; with the `probe`
//! feature the wrappers consult [`replay`], the oracle test's seam for replaying PoB's results
//! while those units are unported (calc doc §9.2 "Replay"; CCR-0025 §3). Without hooks (and in
//! every non-`probe` build) the wrappers call the owning unit's function.
#![allow(non_snake_case, non_upper_case_globals, clippy::all, clippy::pedantic)]

use std::cell::Cell;

use pob_gamedata::schema::affixes::{AffixTable, ItemMod};
use pob_gamedata::schema::item_data::{EnchantmentSlotRef, FoulbornLines, RareLikeUniqueRef};
use pob_gamedata::schema::misc::ClusterJewelSize;
use pob_lua::pattern::{self, Captures};

use crate::classes::item::{
    catalystDescriptorList, catalystList, getCatalystScalar, influenceInfo, influenceItemMap,
    is_lineFlag, normaliseModLine, sortCraftedModLines, specToNumber, uniqueModStatOrder,
    ItemAffixes,
};
use crate::classes::item_tools as itemLib;
use crate::prelude::*;

// ---- `main` (Modules/Main.lua) --------------------------------------------------------------

thread_local! {
    /// `main.defaultItemAffixQuality` (Main.lua:113 `self.defaultItemAffixQuality = 0.5`; the
    /// options dialog and settings file change it, Main.lua:636/1137). CCR-0025 §1.
    static MAIN_DEFAULT_ITEM_AFFIX_QUALITY: Cell<Option<f64>> = const { Cell::new(Some(0.5)) };
}

/// `main.defaultItemAffixQuality` (CCR-0025 §1).
pub fn main_defaultItemAffixQuality() -> Option<f64> {
    MAIN_DEFAULT_ITEM_AFFIX_QUALITY.with(Cell::get)
}

/// Set `main.defaultItemAffixQuality` (the UI's option; CCR-0025 §1).
pub fn set_main_defaultItemAffixQuality(v: Option<f64>) {
    MAIN_DEFAULT_ITEM_AFFIX_QUALITY.with(|c| c.set(v));
}

// ---- replay seam (feature `probe`) ----------------------------------------------------------

/// Oracle-test seam (feature `probe`, CCR-0025 §3): while `itemLib.applyRange`,
/// `Item:BuildModList`, `Item:CanHaveMod` (IT-C) and the ModParser lists (MP1-MP6) are unported,
/// `crates/pob-oracle/tests/IT-A.rs` installs hooks that return PoB's recorded results for
/// ParseRaw's calls (keyed by their arguments). Unset hooks call the real function.
#[cfg(feature = "probe")]
pub mod replay {
    use super::*;
    use std::cell::RefCell;

    pub type ApplyRangeFn = Rc<dyn Fn(&str, &Val, Option<f64>, Option<f64>) -> String>;
    pub type ParseModFn = Rc<dyn Fn(&str, bool) -> (Option<Vec<Mod>>, Option<String>)>;
    pub type BuildModListFn = Rc<dyn Fn(&Item)>;
    pub type CanHaveModFn = Rc<dyn Fn(&Item, Name, &ItemMod) -> bool>;

    #[derive(Default, Clone)]
    pub struct Hooks {
        pub applyRange: Option<ApplyRangeFn>,
        pub parseMod: Option<ParseModFn>,
        pub BuildModList: Option<BuildModListFn>,
        pub CanHaveMod: Option<CanHaveModFn>,
    }

    thread_local! {
        static HOOKS: RefCell<Hooks> = RefCell::new(Hooks::default());
    }

    /// Install hooks for this thread.
    pub fn set(h: Hooks) {
        HOOKS.with(|x| *x.borrow_mut() = h);
    }

    /// Remove every hook.
    pub fn clear() {
        set(Hooks::default());
    }

    pub(super) fn get() -> Hooks {
        HOOKS.with(|x| x.borrow().clone())
    }
}

/// `itemLib.applyRange(line, range, valueScalar, baseValueScalar)` (IT-C).
fn applyRange(
    line: &str,
    range: &Val,
    valueScalar: Option<f64>,
    baseValueScalar: Option<f64>,
) -> String {
    #[cfg(feature = "probe")]
    if let Some(f) = replay::get().applyRange {
        return f(line, range, valueScalar, baseValueScalar);
    }
    itemLib::applyRange(line, range, valueScalar, baseValueScalar)
}

/// `modLib.parseMod(line, isComb)` (pob-modparser).
fn parseMod(line: &str, isComb: bool) -> (Option<Vec<Mod>>, Option<String>) {
    #[cfg(feature = "probe")]
    if let Some(f) = replay::get().parseMod {
        return f(line, isComb);
    }
    pob_modparser::parseMod(line, isComb)
}

impl Item {
    /// `self:BuildModList()` (IT-C).
    fn parse_BuildModList(&self) {
        #[cfg(feature = "probe")]
        if let Some(f) = replay::get().BuildModList {
            return f(self);
        }
        self.BuildModList();
    }

    /// `self:CanHaveMod(modData)` (IT-C); `modId` is the affix key the caller iterates.
    fn parse_CanHaveMod(&self, modId: Name, modData: &ItemMod) -> bool {
        let _ = modId;
        #[cfg(feature = "probe")]
        if let Some(f) = replay::get().CanHaveMod {
            return f(self, modId, modData);
        }
        self.CanHaveMod(modData, None)
    }
}

// ---- plumbing (Rust-only helpers; snake_case) ------------------------------------------------

/// `ConPrintf(fmt, ...)`: PoB's console.
fn ConPrintf(s: String) {
    eprintln!("{s}");
}

/// `s:match(p)` as owned captures (a pattern without captures yields the whole match).
fn smatch(s: &str, p: &str) -> Option<Vec<String>> {
    pattern::r#match(s, p, None).map(|c| caps_owned(&c))
}

fn caps_owned(c: &Captures<'_>) -> Vec<String> {
    c.iter().map(|x| x.to_str().into_owned()).collect()
}

/// `s:match(p)` → its first capture.
fn smatch1(s: &str, p: &str) -> Option<String> {
    smatch(s, p).map(|mut v| v.swap_remove(0))
}

/// `s:find(p)` (a pattern) is truthy.
fn sfind(s: &str, p: &str) -> bool {
    pattern::find(s, p, None, false).is_some()
}

/// `s:find(p, 1, true)` → `(start, end)`, 1-based inclusive.
fn sfind_plain(s: &str, p: &str) -> Option<(usize, usize)> {
    pattern::find(s, p, Some(1), true).map(|(a, b, _)| (a, b))
}

/// `(s:gsub(p, r))` (first result).
fn gsub1(s: &str, p: &str, r: &str) -> String {
    pattern::gsub(s, p, r, None).0
}

/// `s:gmatch(p)` collected (the subject is a snapshot, as in Lua).
fn gmatch_all(s: &str, p: &str) -> Vec<Vec<String>> {
    pattern::gmatch(s, p).map(|c| caps_owned(&c)).collect()
}

/// `tonumber(s)`.
fn tonumber(s: &str) -> Option<f64> {
    pob_lua::tonumber(s)
}

/// A string operand of Lua arithmetic (`value - min`): coerced with `tonumber`, else an error.
fn arith(s: &str) -> f64 {
    match tonumber(s) {
        Some(x) => x,
        None => lua_error!("attempt to perform arithmetic on a string value"),
    }
}

/// `specToNumber(s)` where `s` may be nil (`nil:match` raises inside specToNumber).
fn specToNumber_opt(s: Option<&str>) -> Option<f64> {
    match s {
        Some(s) => specToNumber(s),
        None => lua_error!("attempt to index local 's' (a nil value)"),
    }
}

/// `s:lower()`.
fn lower(s: &str) -> String {
    pob_lua::string::lower(s)
}

/// `s:sub(i, j)`.
fn ssub(s: &str, i: i64, j: Option<i64>) -> String {
    pob_lua::string::sub(s, i, j).into_owned()
}

/// A Lua string value.
fn vstr(s: &str) -> Val {
    Val::Str(LStr::from(s))
}

/// The Lua value of an optional number.
fn onum(x: Option<f64>) -> Val {
    x.map_or(Val::Nil, Val::Num)
}

/// The Lua value of an optional string.
fn ostr(x: Option<&str>) -> Val {
    x.map_or(Val::Nil, vstr)
}

/// `t[x]` for a number key that may be non-integral (FUP1: a float key is a key; a NaN
/// read is nil).
fn tget_num(t: &Table, x: f64) -> Val {
    Key::try_num(x).map_or(Val::Nil, |k| t.get(k))
}

/// `ipairs(v)` over a table value (`ipairs(nil)` raises).
fn ipairs_tbl(v: &Val) -> Vec<Val> {
    match v {
        Val::Table(t) => t.ipairs().into_iter().map(|(_, x)| x).collect(),
        other => lua_error!(
            "bad argument #1 to 'ipairs' (table expected, got {})",
            other.lua_type()
        ),
    }
}

/// `#v` on a table value.
fn tlen(v: &Val) -> usize {
    match v {
        Val::Table(t) => t.len(),
        other => lua_error!("attempt to get length of a {} value", other.lua_type()),
    }
}

/// A `Table` as a value that must be a table (index on nil raises).
fn as_table(v: &Val, what: &str) -> Table {
    match v {
        Val::Table(t) => t.clone(),
        other => lua_error!(
            "attempt to index field '{what}' (a {} value)",
            other.lua_type()
        ),
    }
}

/// A list of mods as a mod line's `modList` (list of `Val::Mod`).
fn modlist_table(mods: Vec<Mod>) -> Table {
    let t = Table::new();
    for m in mods {
        t.push(Val::Mod(Arc::new(m)));
    }
    t
}

/// A list `Table` of strings.
fn str_list<S: AsRef<str>>(xs: impl IntoIterator<Item = S>) -> Table {
    let t = Table::new();
    for x in xs {
        t.push(vstr(x.as_ref()));
    }
    t
}

/// A list `Table` of numbers.
fn num_list(xs: &[f64]) -> Table {
    let t = Table::new();
    for x in xs {
        t.push(*x);
    }
    t
}

/// `data.clusterJewels.jewels[name]` as a Lua-shaped `Table` (CCR-0025 §2): `size`,
/// `sizeIndex`, `minNodes`, `maxNodes`, `smallIndicies`, `notableIndicies`, `socketIndicies`,
/// `totalIndicies`, `skills[id] = { name, icon, masteryIcon, tag, stats, enchant }`.
pub fn clusterJewel_table(j: &ClusterJewelSize) -> Table {
    let t = Table::new();
    t.set(n!("size"), j.size);
    t.set(n!("sizeIndex"), j.sizeIndex);
    t.set(n!("minNodes"), j.minNodes);
    t.set(n!("maxNodes"), j.maxNodes);
    t.set(n!("smallIndicies"), num_list(&j.smallIndicies));
    t.set(n!("notableIndicies"), num_list(&j.notableIndicies));
    t.set(n!("socketIndicies"), num_list(&j.socketIndicies));
    t.set(n!("totalIndicies"), j.totalIndicies);
    let skills = Table::new();
    for (id, s) in &j.skills {
        let st = Table::new();
        st.set(n!("name"), vstr(&s.name));
        st.set(n!("icon"), vstr(&s.icon));
        st.set(n!("masteryIcon"), ostr(s.masteryIcon.as_deref()));
        st.set(n!("tag"), s.tag);
        st.set(n!("stats"), str_list(s.stats.iter().map(|x| &**x)));
        st.set(n!("enchant"), str_list(s.enchant.iter().map(|x| &**x)));
        skills.set(*id, st);
    }
    t.set(n!("skills"), skills);
    t
}

impl Item {
    /// `self.<k>` for an untyped field (`Item.extra`).
    fn xget(&self, k: Name) -> Val {
        self.extra.get(k)
    }

    /// `self.<k> = v` for an untyped field.
    fn xset(&self, k: Name, v: impl Into<Val>) {
        self.extra.set(k, v);
    }

    /// `self.rareLikeUnique` (CCR-0025 §2).
    pub fn rareLikeUnique_ref(&self) -> Option<RareLikeUniqueRef> {
        self.extra
            .obj::<RareLikeUniqueRef>(n!("rareLikeUnique"))
            .map(|r| (*r).clone())
    }

    /// `self.enchantments` (CCR-0025 §2).
    pub fn enchantments_ref(&self) -> Option<EnchantmentSlotRef> {
        self.extra
            .obj::<EnchantmentSlotRef>(n!("enchantments"))
            .map(|r| (*r).clone())
    }

    /// `self.mutatedLines` (CCR-0025 §2).
    pub fn mutatedLines_ref(&self) -> Option<Arc<FoulbornLines>> {
        self.extra
            .obj::<Arc<FoulbornLines>>(n!("mutatedLines"))
            .map(|r| (*r).clone())
    }

    /// `self.affixes` (`Val::Obj(ItemAffixes)`, CCR-0032 §2).
    fn parse_affixes(&self) -> Option<&'static AffixTable> {
        self.extra.obj::<ItemAffixes>(n!("affixes")).map(|a| a.0)
    }

    /// `self[prefix .. "ModLines"]`.
    fn modLines_named(&self, prefix: &str) -> Option<Shared<Vec<Table>>> {
        Some(match prefix {
            "enchant" => self.enchantModLines.get(),
            "scourge" => self.scourgeModLines.get(),
            "implicit" => self.implicitModLines.get(),
            "explicit" => self.explicitModLines.get(),
            "crucible" => self.crucibleModLines.get(),
            "buff" => self.buffModLines.get(),
            "classRequirement" => self.classRequirementModLines.get(),
            _ => return None,
        })
    }

    /// `self.title` (Lua string or nil).
    fn title_str(&self) -> Option<LStr> {
        self.title.get()
    }
}

// ---- Common.lua -------------------------------------------------------------------------------

/// @pob Modules/Common.lua:1110-1116 h=1960e57f
// Removes GGG string tags used for keyword popups. E.g. "[Critical|Critical Hit]" -> "Critical Hit"
pub fn escapeGGGString(text: &str) -> String {
    // L1111
    let line = gsub1(text, "<[^>]+>{([^}]+)}", "%1");
    let line = gsub1(&line, "%[([^|%]]+)%]", "%1");
    let line = gsub1(&line, "%[[^|]+|([^|]+)%]", "%1");
    // L1115
    line
}

// ---- ParseRaw ---------------------------------------------------------------------------------

// @pob-skip Classes/Item.lua:409-423 reason="---@class ModLine annotation comments before ParseRaw (no code)"

/// The `GAME`-mode stage machine of ParseRaw (`gameModeStage`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Stage {
    FINDIMPLICIT,
    IMPLICIT,
    FINDEXPLICIT,
    EXPLICIT,
    DONE,
}

/// @pob Classes/Item.lua:424-1641 h=aaf77de7
impl Item {
    // Parse raw item data and extract item name, base type, quality, and modifiers
    pub fn ParseRaw(&self, raw: &str, rarity: Option<&str>, highQuality: bool) {
        let d = data();
        // L425
        self.raw.set(LStr::from(raw));
        // L426
        self.name.set(LStr::from("?"));
        // L427
        self.xset(n!("namePrefix"), "");
        // L428
        self.xset(n!("nameSuffix"), "");
        // L429
        self.base.set(None);
        // L430
        self.rarity.set(Name::from(rarity.unwrap_or("UNIQUE")));
        // L431
        self.quality.set(None);
        // L432
        let rawLinesT = Table::new();
        self.xset(n!("rawLines"), rawLinesT.clone());
        let mut rawLines: Vec<LStr> = Vec::new();
        // Find non-blank lines and trim whitespace
        // L434
        for c in gmatch_all(raw, "%s*([^\n]*%S)") {
            // L435
            let line = escapeGGGString(&c[0]);
            // L436
            rawLinesT.push(vstr(&line));
            rawLines.push(LStr::from(line.as_str()));
        }
        // `self.rawLines[i]` (1-based; nil past the end)
        let rawLine = |i: usize| -> Option<LStr> {
            if i >= 1 {
                rawLines.get(i - 1).cloned()
            } else {
                None
            }
        };
        // L438
        let mut mode = if rarity.is_some() { "GAME" } else { "WIKI" };
        // L439
        let mut l: usize = 1;
        // L440
        let mut itemClass: Option<String> = None;
        // L441
        if rawLine(l).is_some() {
            // L442
            if smatch(&rawLine(l).unwrap(), "^Item Class:").is_some() {
                // L443
                itemClass = Some(gsub1(&rawLine(l).unwrap(), "^Item Class: %s+", "%1"));
                // L444
                l += 1; // Item class is already determined by the base type
            }
            // L446
            let rarity = match rawLine(l) {
                Some(s) => smatch1(&s, "^Rarity: (%a+)"),
                None => lua_error!("attempt to index field '?' (a nil value)"),
            };
            // L447
            if let Some(rarity) = rarity {
                // L448
                mode = "GAME";
                // L449
                let up = pob_lua::string::upper(&rarity);
                if pob_gamedata::colorCodes.iter().any(|(k, _)| *k == up) {
                    // L450
                    self.rarity.set(Name::from(up.as_str()));
                }
                // L452
                if self.rarity.get() == "UNIQUE" || self.rarity.get() == "RELIC" {
                    // Hack for relics
                    // L454
                    for line in rawLines.iter() {
                        // L455
                        if sfind(line, "Foil Unique") {
                            // L456
                            self.rarity.set(n!("RELIC"));
                            // L457
                            let captured = smatch1(line, "%((.-)%)");
                            // L458
                            self.xset(
                                n!("foilType"),
                                vstr(captured.as_deref().unwrap_or("Rainbow")),
                            );
                            // L459
                            break;
                        }
                    }
                }
                // L463
                l += 1;
            }
        }
        // L466
        if rawLine(l).is_some() {
            // L467
            if rawLine(l).as_deref() == Some("--------") {
                // L468
                l += 1;
            }
            // L470
            // LUA-QUIRK: a raw text that ends at the separator leaves `self.name` nil in Lua; the
            // typed `name` (U0: Field<LStr>) cannot hold nil and keeps "" (CCR-0025 §4).
            self.name.set(rawLine(l).unwrap_or_else(|| LStr::from("")));
            // Determine if "Unidentified" item
            // L472
            let mut unidentified = false;
            // L473
            for line in rawLines.iter() {
                // L474
                if &**line == "Unidentified" {
                    // L475
                    unidentified = true;
                    // L476
                    break;
                }
            }

            // Found the name for a rare or unique, but let's parse it if it's a magic or normal or Unidentified item to get the base
            // L481
            let r = self.rarity.get();
            if !(r == "NORMAL" || r == "MAGIC" || unidentified) {
                // L482
                l += 1;
            }
        }
        // L485
        self.xset(n!("checkSection"), false);
        // L486
        self.sockets.set(Shared::new(Vec::new()));
        // L487
        self.classRequirementModLines.set(Shared::new(Vec::new()));
        // L488
        self.buffModLines.set(Shared::new(Vec::new()));
        // L490
        self.enchantModLines.set(Shared::new(Vec::new()));
        // L492
        self.scourgeModLines.set(Shared::new(Vec::new()));
        // L494
        self.implicitModLines.set(Shared::new(Vec::new()));
        // L496
        self.explicitModLines.set(Shared::new(Vec::new()));
        // L498
        self.crucibleModLines.set(Shared::new(Vec::new()));
        // old items or trade-sourced items have increases to modifiers baked in to the item text, which
        // means that we can't add e.g. quality or mod magnitude effect to them during parsing. we will
        // assume an item to be an advanced copy format if either has mod roll information, a modifier
        // line with a range, or advanced copy lines
        // L503
        self.xset(n!("advancedCopy"), false);
        // L504
        let modMagnitudeMods = Table::new();
        self.xset(n!("modMagnitudeMods"), modMagnitudeMods.clone());
        // L505
        let mut implicitLines: f64 = 0.0;
        // L506
        self.variantList.set(None);
        // L507
        self.xset(n!("versionList"), Val::Nil);
        // L508
        self.xset(n!("allowDuplicateVariants"), false);
        // L509
        self.xset(n!("variantGroups"), Table::new());
        // L510
        let vgs = self.xget(n!("variantGroupSelections"));
        self.xset(
            n!("variantGroupSelections"),
            if vgs.truthy() {
                vgs
            } else {
                Val::Table(Table::new())
            },
        );
        // L511
        self.xset(n!("usesVariantGroups"), false);
        // L512
        let prefixes = Table::new();
        self.xset(n!("prefixes"), prefixes.clone());
        // L513
        let suffixes = Table::new();
        self.xset(n!("suffixes"), suffixes.clone());
        // L514
        // LUA-ALIAS: Lua assigns a fresh table; the typed `requirements` table is reused (wiped).
        // Nothing keeps a reference to an item's requirements table across a parse.
        self.requirements.wipe();
        // L515
        self.requirements.set(n!("str"), 0.0);
        // L516
        self.requirements.set(n!("dex"), 0.0);
        // L517
        self.requirements.set(n!("int"), 0.0);
        // L518
        let baseLines = Table::new();
        self.xset(n!("baseLines"), baseLines.clone());
        // L519
        self.xset(n!("foulborn"), false);
        // L520
        self.xset(n!("vestigial"), false);
        // L521
        self.xset(n!("mutatedLines"), Val::Nil);
        // L523
        self.xset(n!("rareLikeUnique"), Val::Nil);
        // L524
        let mut importedLevelReq: Option<f64> = None;
        // L525
        let mut flaskBuffLines: Option<Vec<LStr>> = None;
        // L526
        // Tincture bases carry no `buff` list (ItemBase.tincture = { manaBurn, cooldown },
        // deny_unknown_fields), so `tinctureBuffLines` stays nil (L530/L1060 never fire).
        let tinctureBuffLines: Option<Vec<LStr>> = None;
        // L527
        let mut deferJewelRadiusIndexAssignment = false;
        // L528
        let mut gameModeStage = Stage::FINDIMPLICIT;
        // L529
        let mut foundExplicit = false;
        let mut foundImplicit = false;
        // L530
        let mut linePrefix = String::new();
        // L531
        let mut linePostfix = String::new();

        // L533
        while let Some(rawl) = rawLine(l) {
            'cont: {
                // L534
                let mut line: String = rawl.to_string();
                // L535
                if line == "Veiled Prefix" || line == "Veiled Suffix" {
                    // L536
                    self.xset(n!("veiled"), true);
                }
                // L538
                if flaskBuffLines
                    .as_ref()
                    .is_some_and(|f| f.iter().any(|x| **x == *line))
                {
                    // L539
                    if let Some(f) = flaskBuffLines.as_mut() {
                        f.retain(|x| **x != *line);
                    }
                } else if tinctureBuffLines
                    .as_ref()
                    .is_some_and(|f| f.iter().any(|x| **x == *line))
                {
                    // L541 (unreachable: see L526)
                } else if line == "--------" {
                    // L543
                    linePrefix = String::new();
                    // L544
                    linePostfix = String::new();
                    // L545
                    self.xset(n!("checkSection"), true);
                } else if line == "Split" {
                    // L547
                    self.split.set(true);
                } else if line == "Mirrored" {
                    // L549
                    self.mirrored.set(true);
                } else if line == "Corrupted" {
                    // L551
                    self.corrupted.set(true);
                } else if line == "Fractured Item" {
                    // L553
                    self.fractured.set(true);
                } else if line == "Synthesised Item" {
                    // L555
                    self.synthesised.set(true);
                } else if smatch(&line, "Foil Unique").is_some() {
                    // L557
                    let captured = smatch1(&line, "%((.-)%)");
                    // L558
                    self.xset(
                        n!("foilType"),
                        vstr(captured.as_deref().unwrap_or("Rainbow")),
                    );
                } else if let Some(key) = influenceItemMap(&line) {
                    // L560
                    self.influences.set(Name::from(key), true);
                } else if line == "Requirements:" {
                    // nothing to do
                } else if smatch(&line, "^%(%a+").is_some()
                    || smatch(&line, "^%(%d+%%? of ").is_some()
                {
                    // Reminder text, nothing to parse
                    // L565
                    while rawLine(l).is_some_and(|s| smatch(&s, "%)$").is_none()) {
                        // L566
                        l += 1;
                    }
                } else if self.base.get().is_some_and(|b| b.flask.is_some())
                    && (smatch(&line, "^Lasts .+ Seconds$").is_some()
                        || smatch(&line, "^Consumes %d+ of %d+ Charges on use$").is_some()
                        || smatch(&line, "^Currently has %d+ Charges$").is_some())
                {
                    // In-game flask state and base properties aren't modifier lines.
                } else if smatch(&line, "^{ ").is_some() {
                    // We're parsing advanced copy/paste format
                    // L576
                    self.xset(n!("advancedCopy"), true);
                    // L577
                    linePrefix = String::new();
                    // L578
                    linePostfix = String::new();
                    // L579
                    self.xset(n!("crafted"), true);
                    // L580
                    let mut fullModName: Option<String> = None;
                    let mut modTags: Option<String> = None;
                    if let Some(c) = smatch(&line, "^{ (.-) %- (.-)  %- (%d*).*}$") {
                        fullModName = Some(c[0].clone());
                        modTags = Some(c[1].clone());
                        let _increasedAmt = c[2].clone();
                    }
                    // L581
                    if fullModName.is_none() {
                        // L582
                        if let Some(c) = smatch(&line, "^{ (.-) %- (.-) }$") {
                            fullModName = Some(c[0].clone());
                            modTags = Some(c[1].clone());
                        }
                    }
                    // L584
                    if fullModName.is_none() {
                        // L585
                        fullModName = smatch1(&line, "^{ (.-) }$");
                    }
                    let Some(fullModName) = fullModName else {
                        lua_error!("attempt to index local 'fullModName' (a nil value)")
                    };
                    // L587
                    let modName = smatch1(&fullModName, "^.*Modifier \"(.*)\"");
                    // L588
                    let affixes = self.parse_affixes();
                    if let (Some(modName), Some(affixes)) =
                        (modName.as_deref().filter(|m| !m.is_empty()), affixes)
                    {
                        // L589
                        let pendingAffixList = Table::new();
                        self.xset(n!("pendingAffixList"), pendingAffixList.clone());
                        // L590
                        let backupAffixList = Table::new();
                        // L591 LUA-ORDER: `pairs(self.affixes)` (string keys, random in PoB); the
                        // port visits the dump order. Only the order of equal-name affixes in the
                        // pending lists depends on it.
                        for (modId, modData) in &affixes.0 {
                            // L592
                            if modData.affix.as_deref() == Some(modName) {
                                // L593
                                let ignoreModType =
                                    self.rareLikeUnique_ref().is_some_and(|r| r.ignoreModType);
                                let entry = |table: &Table| {
                                    let e = Table::new();
                                    e.set(n!("modId"), *modId);
                                    e.set(n!("table"), table.clone());
                                    e
                                };
                                let ty = modData.r#type.map(|t| t.as_str());
                                // L594
                                if self.parse_CanHaveMod(*modId, modData) {
                                    // L595
                                    if ty == Some("Prefix") || ignoreModType {
                                        // L596
                                        pendingAffixList.push(entry(&prefixes));
                                    } else if ty == Some("Suffix") {
                                        // L598
                                        pendingAffixList.push(entry(&suffixes));
                                    }
                                } else {
                                    // Conqueror mods can't natively spawn on items, so we'll use those if we don't find a match otherwise
                                    // L602
                                    if ty == Some("Prefix") || ignoreModType {
                                        // L603
                                        backupAffixList.push(entry(&prefixes));
                                    } else if ty == Some("Suffix") {
                                        // L605
                                        backupAffixList.push(entry(&suffixes));
                                    }
                                }
                            }
                        }
                        // L611
                        if pendingAffixList.len() == 0 && backupAffixList.len() > 0 {
                            // L612
                            self.xset(n!("pendingAffixList"), backupAffixList.clone());
                        }
                        // L614
                        let pal = as_table(&self.xget(n!("pendingAffixList")), "pendingAffixList");
                        if pal.len() == 0 && backupAffixList.len() == 0 {
                            // Could be a veiled, temple, or other custom mod, so just keep it around
                            // L616
                            linePrefix = "{custom}".to_owned();
                        }
                    } else if smatch(&fullModName, "(.*)Enhancement.*").is_some() {
                        // L619
                        linePostfix = " (enchant)".to_owned();
                    }
                    // L621
                    let possibleLineFlags = smatch1(
                        &gsub1(&fullModName, "Foulborn", "Mutated"),
                        "(.*)Modifier.*",
                    );
                    // L622
                    if let Some(possibleLineFlags) = possibleLineFlags {
                        // L623
                        for flag in gmatch_all(&possibleLineFlags, "%a+") {
                            // L624
                            let flagLower = lower(&flag[0]);
                            // L625
                            if is_lineFlag(&flagLower) {
                                // L626
                                linePrefix = format!("{linePrefix}{{{flagLower}}}");
                            }
                        }
                    }
                    // L630
                    if smatch(&fullModName, "^Allocated Crucible Passive Skill").is_some() {
                        // L631
                        linePrefix = format!("{linePrefix}{{crucible}}");
                    }
                    // L633
                    if let Some(modTags) = modTags.as_deref().filter(|m| !m.is_empty()) {
                        // L634
                        linePrefix =
                            format!("{linePrefix}{{tags:{}}}", gsub1(&lower(modTags), "%s+", ""));
                    }
                } else {
                    // L637
                    line = format!("{linePrefix}{line}{linePostfix}");
                    // L638
                    if self.xget(n!("checkSection")).truthy() {
                        // L639
                        if gameModeStage == Stage::IMPLICIT {
                            // L640
                            if foundImplicit {
                                // There were definitely implicits, so any following modifiers must be explicits
                                // L642
                                gameModeStage = Stage::EXPLICIT;
                                // L643
                                foundExplicit = true;
                            } else {
                                // L645
                                gameModeStage = Stage::FINDEXPLICIT;
                            }
                        } else if gameModeStage == Stage::EXPLICIT {
                            // L648
                            gameModeStage = Stage::DONE;
                        } else if gameModeStage == Stage::FINDIMPLICIT
                            && self.itemLevel.get().is_some()
                            && smatch(&line, " %(implicit%)").is_none()
                            && smatch(&line, " %(enchant%)").is_none()
                            && !sfind(&line, "Talisman Tier")
                        {
                            // L651
                            gameModeStage = Stage::EXPLICIT;
                            // L652
                            foundExplicit = true;
                        }
                        // L654
                        self.xset(n!("checkSection"), false);
                    }
                    // L656
                    let mut specName: Option<String> = None;
                    let mut specVal: Option<String> = None;
                    if let Some(c) = smatch(&line, "^([%a %(%)]+:?): (.+)$") {
                        specName = Some(c[0].clone());
                        specVal = Some(c[1].clone());
                    }
                    // L657
                    if specName.is_some() {
                        // L658
                        if specName.as_deref() == Some("Class:") {
                            // L659
                            specName = Some("Requires Class".to_owned());
                        }
                    } else {
                        // L662
                        if let Some(c) = smatch(&line, "^(Requires %a+) (.+)$") {
                            specName = Some(c[0].clone());
                            specVal = Some(c[1].clone());
                        }
                    }
                    // L664
                    if let (Some(sn), Some(sv)) = (specName.clone(), specVal.clone()) {
                        let mut specName = sn;
                        let mut specVal = sv;
                        let sn_owned = specName.clone();
                        let sn = sn_owned.as_str();
                        // L665
                        if sn == "Unique ID" {
                            // L666
                            self.uniqueID.set(Some(LStr::from(specVal.as_str())));
                        } else if sn == "Item Level" {
                            // L668
                            self.itemLevel.set(specToNumber(&specVal));
                        } else if sn == "Memory Strands" {
                            // L670
                            self.xset(n!("memoryStrands"), onum(specToNumber(&specVal)));
                        } else if sn == "Requires Class" {
                            // L672
                            self.classRestriction
                                .set(Some(Name::from(specVal.as_str())));
                        } else if smatch(sn, "Quality %([%a%s]+ Modifiers%)").is_some() {
                            // L674
                            self.catalystQuality
                                .set(specToNumber_opt(smatch1(&specVal, "(%d+)%%").as_deref()));
                            // L675
                            for i in 1..=catalystDescriptorList.len() {
                                // L676
                                if smatch1(sn, "Quality %(([%a%s]+) Modifiers%)").as_deref()
                                    == Some(catalystDescriptorList[i - 1])
                                {
                                    // L677
                                    self.catalyst.set(Some(i as f64));
                                }
                            }
                        } else if sn == "Quality" {
                            // L681
                            self.quality.set(specToNumber(&specVal));
                        } else if sn == "Sockets" {
                            // L683
                            let mut group: f64 = 0.0;
                            let sockets = self.sockets.get();
                            // L684 (`.` matches one byte)
                            for &c in specVal.as_bytes() {
                                let cs = (c as char).to_string();
                                // L685
                                if matches!(c, b'R' | b'G' | b'B' | b'W' | b'A') {
                                    // L686
                                    let s = Table::new();
                                    s.set(n!("color"), vstr(&cs));
                                    s.set(n!("group"), group);
                                    sockets.push(s);
                                } else if c == b' ' {
                                    // L688
                                    group += 1.0;
                                }
                            }
                        } else if sn == "Radius" && self.r#type.get() == "Jewel" {
                            // L692
                            let label = smatch(&specVal, "^[%a ]+").map(|c| c[0].clone());
                            self.jewelRadiusLabel.set(label.as_deref().map(LStr::from));
                            // L693
                            if label.as_deref() == Some("Variable") {
                                // Jewel radius is variable and must be read from it's mods instead after they are parsed
                                // L695
                                deferJewelRadiusIndexAssignment = true;
                            } else {
                                // L697 (`data.jewelRadius` is a list: pairs visits 1..n)
                                let sel = pob_gamedata::jewelRadius();
                                for (index, data) in sel.jewelRadius.iter().enumerate() {
                                    // L698
                                    if label.as_deref() == Some(data.label.as_str()) {
                                        // L699
                                        self.jewelRadiusIndex.set(Some((index + 1) as f64));
                                        // L700
                                        break;
                                    }
                                }
                            }
                        } else if sn == "Limited to" && self.r#type.get() == "Jewel" {
                            // L705
                            self.limit.set(specToNumber(&specVal));
                        } else if sn == "Version" {
                            // L707
                            if !self.xget(n!("versionList")).truthy() {
                                // L708
                                self.xset(n!("versionList"), Table::new());
                            }
                            // L710
                            as_table(&self.xget(n!("versionList")), "versionList")
                                .push(vstr(&specVal));
                            // L711
                            self.xset(n!("usesVariantGroups"), true);
                        } else if sn == "Variant" {
                            // L713
                            if self.variantList.get().is_none() {
                                // L714
                                self.variantList.set(Some(Shared::new(Vec::new())));
                            }
                            let variantList = self.variantList.get().expect("set above");
                            // This has to be kept for backwards compatibility
                            // L717
                            let c = smatch(&specVal, "{([%w_]+)}(.+)");
                            // L718
                            if let Some(c) = c {
                                // L719
                                variantList.push(LStr::from(c[1].as_str()));
                            } else {
                                // L721
                                variantList.push(LStr::from(specVal.as_str()));
                            }
                        } else if sn == "Talisman Tier" {
                            // L724
                            self.talismanTier.set(specToNumber(&specVal));
                        } else if sn == "Armour"
                            || sn == "Evasion Rating"
                            || sn == "Evasion"
                            || sn == "Energy Shield"
                            || sn == "Ward"
                        {
                            // L726
                            if sn == "Evasion Rating" {
                                // L727
                                specName = "Evasion".to_owned();
                                // L728
                                if self.baseName.get().as_deref()
                                    == Some("Two-Toned Boots (Armour/Energy Shield)")
                                {
                                    // Another hack for Two-Toned Boots
                                    // L730
                                    self.baseName
                                        .set(Some(n!("Two-Toned Boots (Armour/Evasion)")));
                                    // L731
                                    self.base.set(
                                        d.itemBases
                                            .get("Two-Toned Boots (Armour/Evasion)")
                                            .cloned(),
                                    );
                                }
                            } else if sn == "Energy Shield" {
                                // L734
                                specName = "EnergyShield".to_owned();
                                // L735
                                if self.baseName.get().as_deref()
                                    == Some("Two-Toned Boots (Armour/Evasion)")
                                {
                                    // Yet another hack for Two-Toned Boots
                                    // L737
                                    self.baseName
                                        .set(Some(n!("Two-Toned Boots (Evasion/Energy Shield)")));
                                    // L738
                                    self.base.set(
                                        d.itemBases
                                            .get("Two-Toned Boots (Evasion/Energy Shield)")
                                            .cloned(),
                                    );
                                }
                            }
                            // L741
                            if self.armourData.get().is_none() {
                                self.armourData.set(Some(Table::new()));
                            }
                            // L742
                            self.armourData
                                .get()
                                .expect("set above")
                                .set(Name::from(specName.as_str()), onum(specToNumber(&specVal)));
                        } else if smatch(sn, "BasePercentile").is_some() {
                            // L744
                            if self.armourData.get().is_none() {
                                self.armourData.set(Some(Table::new()));
                            }
                            // L745
                            self.armourData.get().expect("set above").set(
                                Name::from(specName.as_str()),
                                specToNumber(&specVal).unwrap_or(0.0),
                            );
                        } else if sn == "Requires Level" {
                            // L747
                            self.requirements
                                .set(n!("level"), onum(specToNumber(&specVal)));
                        } else if sn == "Level" {
                            // Requirements from imported items can't always be trusted
                            // L750
                            importedLevelReq = specToNumber(&specVal);
                        } else if sn == "LevelReq" {
                            // L752
                            self.requirements
                                .set(n!("level"), onum(specToNumber(&specVal)));
                        } else if sn == "Has Alt Variant" {
                            // L754
                            self.xset(n!("hasAltVariant"), true);
                        } else if sn == "Has Alt Variant Two" {
                            // L756
                            self.xset(n!("hasAltVariant2"), true);
                        } else if sn == "Has Alt Variant Three" {
                            // L758
                            self.xset(n!("hasAltVariant3"), true);
                        } else if sn == "Has Alt Variant Four" {
                            // L760
                            self.xset(n!("hasAltVariant4"), true);
                        } else if sn == "Has Alt Variant Five" {
                            // L762
                            self.xset(n!("hasAltVariant5"), true);
                        } else if sn == "Selected Version" {
                            // L764
                            self.xset(n!("selectedVersion"), onum(specToNumber(&specVal)));
                            // L765
                            self.xset(n!("usesVariantGroups"), true);
                        } else if sn == "Selected Variant Group" {
                            // L767
                            let c = smatch(&specVal, "^(%d+)%s*=%s*(%d+)$");
                            // L768
                            if let Some(c) = c {
                                // L769
                                let sel = as_table(
                                    &self.xget(n!("variantGroupSelections")),
                                    "variantGroupSelections",
                                );
                                let groupId = tonumber(&c[0]).expect("digits");
                                sel.set(Key::from(groupId), onum(tonumber(&c[1])));
                                // L770
                                self.xset(n!("usesVariantGroups"), true);
                            }
                        } else if sn == "Selected Variant" {
                            // L773
                            self.variant.set(specToNumber(&specVal));
                        } else if sn == "Selected Alt Variant" {
                            // L775
                            self.variantAlt.set(specToNumber(&specVal));
                        } else if sn == "Selected Alt Variant Two" {
                            // L777
                            self.variantAlt2.set(specToNumber(&specVal));
                        } else if sn == "Selected Alt Variant Three" {
                            // L779
                            self.variantAlt3.set(specToNumber(&specVal));
                        } else if sn == "Selected Alt Variant Four" {
                            // L781
                            self.variantAlt4.set(specToNumber(&specVal));
                        } else if sn == "Selected Alt Variant Five" {
                            // L783
                            self.variantAlt5.set(specToNumber(&specVal));
                        } else if sn == "Allow Duplicate Variants" {
                            // L785
                            self.xset(n!("allowDuplicateVariants"), specVal == "true");
                        } else if sn == "Has Variants" || sn == "Selected Variants" {
                            // Need to skip this line for backwards compatibility
                            // with builds that used an old Watcher's Eye implementation
                            // L789
                            l += 1;
                        } else if sn == "League" {
                            // L791
                            self.xset(n!("league"), vstr(&specVal));
                        } else if sn == "Crafted" {
                            // L793
                            self.xset(n!("crafted"), true);
                        } else if sn == "Scourge" {
                            // L795
                            self.xset(n!("scourge"), true);
                        } else if sn == "Crucible" {
                            // L797
                            self.xset(n!("crucible"), true);
                        } else if sn == "Implicit" {
                            // L799
                            self.xset(n!("implicit"), true);
                        } else if sn == "Prefix" || sn == "Suffix" {
                            // L801
                            let affixes = if sn == "Prefix" { &prefixes } else { &suffixes };
                            // L802
                            let fractured = smatch(&specVal, "^{fractured}").is_some();
                            // L803
                            specVal = gsub1(&specVal, "^{fractured}", "");
                            // L804
                            let c = smatch(&specVal, "{range:([^}]+)}(.+)");
                            let rangeStr = c.as_ref().map(|c| c[0].clone());
                            let affix = c.as_ref().map(|c| c[1].clone());
                            // L805
                            let mut range: Val = if rangeStr
                                .as_deref()
                                .is_some_and(|r| sfind_plain(r, ",").is_some())
                            {
                                // L806
                                let ranges = Table::new();
                                // L807
                                for value in gmatch_all(rangeStr.as_deref().unwrap(), "[^,]+") {
                                    // L808 (`t_insert(ranges, nil)` is a no-op)
                                    if let Some(x) = tonumber(&value[0]) {
                                        ranges.push(x);
                                    }
                                }
                                // L810
                                Val::Table(ranges)
                            } else {
                                // L812 (`tonumber(nil)` is nil)
                                onum(rangeStr.as_deref().and_then(tonumber))
                            };
                            // L814
                            if !range.truthy()
                                && affix.as_deref().unwrap_or(specVal.as_str()) != "None"
                            {
                                // L815
                                range = onum(main_defaultItemAffixQuality());
                            }
                            // L817
                            let e = Table::new();
                            e.set(
                                n!("modId"),
                                vstr(affix.as_deref().unwrap_or(specVal.as_str())),
                            );
                            e.set(n!("range"), range);
                            e.set(
                                n!("fractured"),
                                if fractured { Val::Bool(true) } else { Val::Nil },
                            );
                            affixes.push(e);
                        } else if sn == "Implicits" {
                            // L823
                            implicitLines = specToNumber(&specVal).unwrap_or(0.0);
                            // L824
                            gameModeStage = Stage::EXPLICIT;
                        } else if sn == "Unreleased" {
                            // L826
                            self.xset(n!("unreleased"), specVal == "true");
                        } else if sn == "Upgrade" {
                            // L828
                            if !self.xget(n!("upgradePaths")).truthy() {
                                self.xset(n!("upgradePaths"), Table::new());
                            }
                            // L829
                            as_table(&self.xget(n!("upgradePaths")), "upgradePaths")
                                .push(vstr(&specVal));
                        } else if sn == "Source" {
                            // L831
                            self.xset(n!("source"), vstr(&specVal));
                        } else if sn == "Cluster Jewel Skill" {
                            // L833
                            if let Some(cj) = self.clusterJewel.get() {
                                if as_table(&cj.get(n!("skills")), "skills")
                                    .truthy(Name::from(specVal.as_str()))
                                {
                                    // L834
                                    self.clusterJewelSkill
                                        .set(Some(Name::from(specVal.as_str())));
                                }
                            }
                        } else if sn == "Cluster Jewel Node Count" {
                            // L837
                            if let Some(cj) = self.clusterJewel.get() {
                                // L838
                                let num = specToNumber(&specVal)
                                    .unwrap_or_else(|| cj.num(n!("maxNodes")));
                                // L839
                                self.clusterJewelNodeCount.set(Some(m_min(
                                    m_max(num, cj.num(n!("minNodes"))),
                                    cj.num(n!("maxNodes")),
                                )));
                            }
                        } else if sn == "Catalyst" {
                            // L842
                            for i in 1..=catalystList.len() {
                                // L843
                                if specVal == catalystList[i - 1] {
                                    // L844
                                    self.catalyst.set(Some(i as f64));
                                }
                            }
                        } else if sn == "CatalystQuality" {
                            // L848
                            self.catalystQuality.set(specToNumber(&specVal));
                        } else if sn == "Intangibility" {
                            // L850
                            self.xset(n!("intangibility"), onum(specToNumber(&specVal)));
                        } else if sn == "Note" {
                            // L852
                            self.xset(n!("note"), vstr(&specVal));
                        } else if sn == "Str"
                            || sn == "Strength"
                            || sn == "Dex"
                            || sn == "Dexterity"
                            || sn == "Int"
                            || sn == "Intelligence"
                        {
                            // L855
                            self.requirements.set(
                                Name::from(lower(&ssub(sn, 1, Some(3))).as_str()),
                                onum(specToNumber(&specVal)),
                            );
                        } else if sn == "Critical Strike Range"
                            || sn == "Attacks per Second"
                            || sn == "Weapon Range"
                            || sn == "Critical Strike Chance"
                            || sn == "Physical Damage"
                            || sn == "Elemental Damage"
                            || sn == "Chaos Damage"
                            || sn == "Chance to Block"
                            || sn == "Block chance"
                            || sn == "Armour"
                            || sn == "Energy Shield"
                            || sn == "Evasion"
                        {
                            // L860
                            self.xset(n!("hidden_specs"), true);
                        // Anything else is an explicit with a colon in it (Fortress Covenant, Pure Talent, etc) unless it's part of the custom name
                        } else if !(smatch(&self.name.get(), sn).is_some()
                            && smatch(&self.name.get(), &specVal).is_some())
                        {
                            // L863
                            foundExplicit = true;
                            // L864
                            gameModeStage = Stage::EXPLICIT;
                        }
                        let _ = (&mut specName, &mut specVal);
                    }
                    // L867
                    if line == "Prefixes:" {
                        // L868
                        foundExplicit = true;
                        // L869
                        gameModeStage = Stage::EXPLICIT;
                    }
                    // L871
                    if specName.is_none() || foundExplicit || foundImplicit {
                        // L873
                        let modLine = Table::new();
                        modLine.set(n!("modTags"), Table::new());

                        // L875
                        line = pattern::gsub_fn(
                            &line,
                            "{(%a*):?([^}]*)}",
                            |c| {
                                let k = c[0].to_str();
                                let val = c[1].to_str();
                                // L876
                                if k == "variant" {
                                    // L877
                                    let vl = Table::new();
                                    modLine.set(n!("variantList"), vl.clone());
                                    // L878
                                    for varId in gmatch_all(&val, "%d+") {
                                        // L879
                                        vl.set(
                                            Key::from(tonumber(&varId[0]).expect("digits")),
                                            true,
                                        );
                                    }
                                } else if k == "version" {
                                    // L882
                                    let vl = Table::new();
                                    modLine.set(n!("versionList"), vl.clone());
                                    // L883
                                    for versionId in gmatch_all(&val, "%d+") {
                                        // L884
                                        vl.set(
                                            Key::from(tonumber(&versionId[0]).expect("digits")),
                                            true,
                                        );
                                    }
                                    // L886
                                    self.xset(n!("usesVariantGroups"), true);
                                } else if k == "group" {
                                    // L888
                                    let gl = Table::new();
                                    modLine.set(n!("variantGroupList"), gl.clone());
                                    // L889
                                    for groupId in gmatch_all(&val, "%d+") {
                                        // L890
                                        let groupId = tonumber(&groupId[0]);
                                        // L891
                                        if let Some(groupId) = groupId.filter(|g| *g > 0.0) {
                                            // L892
                                            gl.set(Key::from(groupId), true);
                                        }
                                    }
                                    // L895
                                    self.xset(n!("usesVariantGroups"), true);
                                } else if k == "tags" {
                                    // L897
                                    let modTags = modLine.table(n!("modTags")).expect("modTags");
                                    for tag in gmatch_all(&val, "[%a_]+") {
                                        // L898
                                        modTags.push(vstr(&tag[0]));
                                    }
                                } else if k == "modGroup" {
                                    // L901
                                    modLine.set(n!("modGroup"), vstr(&val));
                                } else if k == "range" {
                                    // L903
                                    self.xset(n!("advancedCopy"), true);
                                    // L904
                                    modLine.set(n!("range"), onum(tonumber(&val)));
                                } else if k == "corruptedRange" {
                                    // L906
                                    modLine.set(n!("corruptedRange"), onum(tonumber(&val)));
                                } else if is_lineFlag(&k) {
                                    // L908
                                    modLine.set(Name::from(&*k), true);
                                }

                                // L911
                                Some(String::new())
                            },
                            None,
                        )
                        .0;
                        // L913
                        if let Some(variantGroupList) = modLine.table(n!("variantGroupList")) {
                            // L914
                            match modLine.table(n!("variantList")) {
                                None => {
                                    // L915
                                    ConPrintf(lfmt!(
                                        "Grouped item line has no variant: %s",
                                        line.as_str()
                                    ));
                                }
                                Some(mlVariantList) => {
                                    let variantGroups =
                                        as_table(&self.xget(n!("variantGroups")), "variantGroups");
                                    // L917 (int keys; only table writes and console output
                                    // depend on the order)
                                    for (groupId, _) in variantGroupList.pairs() {
                                        // L918
                                        let group = match variantGroups.get(groupId) {
                                            Val::Table(t) => t,
                                            _ => Table::new(),
                                        };
                                        // L919
                                        variantGroups.set(groupId, group.clone());
                                        // L920
                                        for (variantId, _) in mlVariantList.pairs() {
                                            let vid = variantId.to_val().num();
                                            // L921
                                            let known = self.variantList.get().is_some_and(|vl| {
                                                vid >= 1.0
                                                    && vid == vid.trunc()
                                                    && vl.get(vid as usize).is_some()
                                            });
                                            if known {
                                                // L922
                                                let versions = match group.get(variantId) {
                                                    Val::Table(t) => t,
                                                    _ => Table::new(),
                                                };
                                                // L923
                                                group.set(variantId, versions.clone());
                                                // L924
                                                if let Some(mlVersionList) =
                                                    modLine.table(n!("versionList"))
                                                {
                                                    // L925
                                                    for (versionId, _) in mlVersionList.pairs() {
                                                        // L926
                                                        let ver = versionId.to_val().num();
                                                        let vlist = self.xget(n!("versionList"));
                                                        if vlist.truthy()
                                                            && tget_num(
                                                                &as_table(&vlist, "versionList"),
                                                                ver,
                                                            )
                                                            .truthy()
                                                        {
                                                            // L927
                                                            versions.set(versionId, true);
                                                        }
                                                    }
                                                } else {
                                                    // L931
                                                    versions.set(Key::I(0), true);
                                                }
                                            } else {
                                                // L934
                                                ConPrintf(lfmt!(
                                                    "Grouped item line references unknown variant %d: %s",
                                                    vid,
                                                    line.as_str()
                                                ));
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        // L941
                        line = pattern::gsub_fn(
                            &line,
                            " %((%l+)%)",
                            |c| {
                                let k = c[0].to_str();
                                // L942
                                if is_lineFlag(&k) {
                                    // L943
                                    modLine.set(Name::from(&*k), true);
                                }
                                // L945
                                Some(String::new())
                            },
                            None,
                        )
                        .0;

                        // L948
                        if modLine.truthy(n!("enchant")) {
                            // L949
                            modLine.set(n!("crafted"), true);
                            // L950
                            modLine.set(n!("implicit"), true);
                        }
                        // L952
                        if modLine.truthy(n!("vestigial")) {
                            // L953
                            self.xset(n!("vestigial"), true);
                        }

                        // L956
                        let mut baseName: Option<String> = None;
                        // items containing (#-#) lines are marked since the values won't have catalyst or mod magnitudes applied
                        // L958
                        if sfind(&line, "%((%-?%d+%.?%d*)%-(%-?%d+%.?%d*)%)") {
                            // L959
                            self.xset(n!("advancedCopy"), true);
                        }
                        // L961
                        let r = self.rarity.get();
                        if self.base.get().is_none() && (r == "NORMAL" || r == "MAGIC") {
                            // Exact match (affix-less magic and normal items)
                            // L963
                            if smatch(&self.name.get(), "Energy Blade").is_some()
                                && itemClass.is_some()
                            {
                                // Special handling for energy blade base.
                                // L964
                                self.name.set(LStr::from(
                                    if smatch(itemClass.as_deref().unwrap(), "One Hand").is_some() {
                                        "Energy Blade One Handed"
                                    } else {
                                        "Energy Blade Two Handed"
                                    },
                                ));
                            }
                            // L966
                            let name = self.name.get();
                            if d.itemBases.contains_key(&*name) {
                                // L967
                                baseName = Some(name.to_string());
                            } else {
                                // L969
                                let mut best_match: Option<String> = None;
                                let mut best_length: f64 = -1.0;
                                let mut best_e: usize = 0;
                                let mut best_s: usize = 0;
                                // Partial match (magic items with affixes)
                                // L971 LUA-ORDER: `pairs(data.itemBases)` (random in PoB); the
                                // port visits the dump order. Only equally long matches tie.
                                for (itemBaseName, _baseData) in &d.itemBases {
                                    // L972
                                    let se = sfind_plain(&name, itemBaseName);
                                    // L973
                                    if let Some((s, e)) = se {
                                        if (e as f64 - s as f64) > best_length {
                                            // L974
                                            best_match = Some(itemBaseName.to_string());
                                            // L975
                                            best_length = e as f64 - s as f64;
                                            // L976
                                            best_e = e;
                                            // L977
                                            best_s = s;
                                        }
                                    }
                                }
                                // L980
                                if let Some(m) = best_match {
                                    // L981
                                    self.xset(
                                        n!("namePrefix"),
                                        vstr(&ssub(&name, 1, Some(best_s as i64 - 1))),
                                    );
                                    // L982
                                    self.xset(
                                        n!("nameSuffix"),
                                        vstr(&ssub(&name, best_e as i64 + 1, None)),
                                    );
                                    // L983
                                    baseName = Some(m);
                                }
                            }
                            // L986
                            if baseName.is_none() {
                                // L987
                                let name = self.name.get();
                                if let Some((s, e)) = sfind_plain(&name, "Two-Toned Boots") {
                                    // Hack for Two-Toned Boots
                                    // L990
                                    baseName = Some("Two-Toned Boots".to_owned());
                                    // L991
                                    self.xset(
                                        n!("namePrefix"),
                                        vstr(&ssub(&name, 1, Some(s as i64 - 1))),
                                    );
                                    // L992
                                    self.xset(
                                        n!("nameSuffix"),
                                        vstr(&ssub(&name, e as i64 + 1, None)),
                                    );
                                }
                            }
                            // L995
                            self.name
                                .set(LStr::from(gsub1(&self.name.get(), " %(.+%)", "").as_str()));
                        }
                        // L997
                        if baseName.is_none() {
                            // L998
                            let b = gsub1(&line, "^Superior ", "");
                            let b = gsub1(&b, "^Synthesised ", "");
                            baseName = Some(gsub1(&b, "^Vestigial ", ""));
                        }
                        let mut baseName = baseName.expect("set above");
                        // L1000
                        if baseName == "Two-Toned Boots" {
                            // L1001
                            baseName = "Two-Toned Boots (Armour/Energy Shield)".to_owned();
                        }
                        // L1003
                        let base = d.itemBases.get(baseName.as_str()).cloned();
                        // L1004
                        if let Some(base) = base {
                            // Items with variants can have multiple bases
                            // L1006 (LUA-ORDER: BuildRaw iterates `pairs(self.baseLines)`;
                            // insertion order here, CCR-0032 §2)
                            let bl = Table::new();
                            bl.set(n!("line"), vstr(&baseName));
                            bl.set(n!("variantList"), modLine.get(n!("variantList")));
                            bl.set(n!("versionList"), modLine.get(n!("versionList")));
                            bl.set(n!("variantGroupList"), modLine.get(n!("variantGroupList")));
                            baseLines.set(Name::from(baseName.as_str()), bl);
                            // L1012
                            if self.xget(n!("usesVariantGroups")).truthy()
                                && self.xget(n!("versionList")).truthy()
                                && !self.xget(n!("selectedVersion")).truthy()
                            {
                                // L1013
                                let n = tlen(&self.xget(n!("versionList"))) as f64;
                                self.xset(n!("selectedVersion"), n);
                            }
                            // Set the actual base if variant matches or doesn't have variants
                            // L1016
                            let usesVariantGroups = self.xget(n!("usesVariantGroups")).truthy();
                            let baseMatches = (usesVariantGroups
                                && self.CheckModLineVariant(&modLine))
                                || (!usesVariantGroups
                                    && (self.variant.get().is_none()
                                        || !modLine.truthy(n!("variantList"))
                                        || tget_num(
                                            &modLine.table(n!("variantList")).expect("truthy"),
                                            self.variant.get().unwrap(),
                                        )
                                        .truthy()));
                            // L1018
                            if baseMatches {
                                // L1019
                                self.baseName.set(Some(Name::from(baseName.as_str())));
                                // L1020
                                let r = self.rarity.get();
                                if !(r == "NORMAL" || r == "MAGIC") {
                                    // L1021
                                    self.title.set(Some(self.name.get()));
                                }
                                // L1023
                                if self.rarity.get() == "UNIQUE"
                                    && self.title_str().is_some_and(|t| {
                                        // the foulborn transformation on this item
                                        // increases the radius, which isn't a mod line
                                        // in PoB, which makes it incompatible with this
                                        // approach
                                        !sfind(&lower(&t), "might of the meek")
                                    })
                                {
                                    // L1028
                                    let t = gsub1(&self.title_str().unwrap(), "^[Ff]oulborn ", "");
                                    self.xset(
                                        n!("mutatedLines"),
                                        match d.foulbornMap.0.get(t.as_str()) {
                                            Some(m) => Val::Obj(Rc::new(m.clone())),
                                            None => Val::Nil,
                                        },
                                    );
                                }
                                // L1030
                                self.r#type.set(base.r#type);
                                // L1031
                                self.base.set(Some(base.clone()));
                                // L1032
                                let affixes: Option<&'static AffixTable> = base
                                    .subType
                                    .and_then(|st| {
                                        d.itemMods.get(&Name::from(
                                            format!("{}{}", base.r#type, st).as_str(),
                                        ))
                                    })
                                    .or_else(|| d.itemMods.get(&base.r#type))
                                    .or_else(|| d.itemMods.get(&n!("Item")));
                                self.xset(
                                    n!("affixes"),
                                    match affixes {
                                        Some(a) => Val::Obj(Rc::new(ItemAffixes(a))),
                                        None => Val::Nil,
                                    },
                                );
                                // L1035
                                if let Some(title) = self.title_str() {
                                    // L1036
                                    let rlu = d.rareLikeUniques.0.get(lower(&title).as_str());
                                    self.xset(
                                        n!("rareLikeUnique"),
                                        match rlu {
                                            Some(r) => Val::Obj(Rc::new(r.clone())),
                                            None => Val::Nil,
                                        },
                                    );
                                    // L1037
                                    if let Some(rlu) = rlu {
                                        // L1038
                                        let a: &'static AffixTable = &rlu.affixes;
                                        self.xset(n!("affixes"), Val::Obj(Rc::new(ItemAffixes(a))));
                                    }
                                }
                                // L1041
                                let enchantments = if base.flask.is_some() {
                                    // L1042: `self.base.utility_flask` is never set (ItemBase
                                    // has no such key; deny_unknown_fields), so L1043 is dead
                                    // L1045
                                    d.enchantments.get("Flask")
                                } else {
                                    // L1048
                                    d.enchantments.get(base.r#type.as_str())
                                };
                                self.xset(
                                    n!("enchantments"),
                                    match enchantments {
                                        Some(e) => Val::Obj(Rc::new(e.clone())),
                                        None => Val::Nil,
                                    },
                                );
                                // L1050
                                self.xset(n!("corruptible"), base.r#type != "Flask");
                                // L1051
                                self.xset(n!("canBeInfluenced"), base.influenceTags.is_some());
                                // L1052
                                self.clusterJewel.set(
                                    self.baseName
                                        .get()
                                        .and_then(|bn| d.clusterJewels.jewels.get(&bn))
                                        .map(clusterJewel_table),
                                );
                                // L1053
                                self.requirements
                                    .set(n!("str"), base.req.str.unwrap_or(0.0));
                                // L1054
                                self.requirements
                                    .set(n!("dex"), base.req.dex.unwrap_or(0.0));
                                // L1055
                                self.requirements
                                    .set(n!("int"), base.req.int.unwrap_or(0.0));
                                // L1056
                                let rs = self.requirements.num(n!("str"));
                                let rd = self.requirements.num(n!("dex"));
                                let ri = self.requirements.num(n!("int"));
                                let maxReq = m_max_n(&[rs, rd, ri]);
                                // L1057
                                self.xset(
                                    n!("defaultSocketColor"),
                                    if maxReq == rd {
                                        "G"
                                    } else if maxReq == ri {
                                        "B"
                                    } else {
                                        "R"
                                    },
                                );
                                // L1058 (`self.base.flask.buff`: a list, `{}` when absent. No
                                // base in Data/Bases has an explicit empty `buff = { }`, so an
                                // empty list is Lua's nil: the guard keeps `flaskBuffLines` unset
                                // and a later flask base line with a buff still gets its lines)
                                if let Some(flask) = base.flask.as_ref() {
                                    if !flask.buff.is_empty() && flaskBuffLines.is_none() {
                                        // L1059
                                        let mut fbl: Vec<LStr> = Vec::new();
                                        // L1060
                                        for line in &flask.buff {
                                            // L1061
                                            fbl.push(LStr::from(&**line));
                                            // L1062
                                            let (modList, extra) = parseMod(line, false);
                                            // L1063
                                            let ml = Table::new();
                                            ml.set(n!("line"), vstr(line));
                                            ml.set(n!("extra"), ostr(extra.as_deref()));
                                            ml.set(
                                                n!("modList"),
                                                modlist_table(modList.unwrap_or_default()),
                                            );
                                            self.buffModLines.get().push(ml);
                                        }
                                        flaskBuffLines = Some(fbl);
                                    }
                                }
                                // L1066-1073: `self.base.tincture.buff` does not exist (L526)
                            }
                            // Base lines don't need mod parsing, skip it
                            // L1076
                            break 'cont;
                        }
                        // L1078
                        if modLine.truthy(n!("implicit")) {
                            // L1079
                            foundImplicit = true;
                            // L1080
                            gameModeStage = Stage::IMPLICIT;
                        }
                        // L1082
                        let mut catalystScalar: f64 = 1.0;
                        // L1083
                        if smatch(&line, " %- Unscalable Value$").is_some() {
                            // L1084
                            line = gsub1(&line, " %- Unscalable Value$", "");
                            // L1085
                            modLine.set(n!("unscalable"), true);
                        } else {
                            // L1087
                            catalystScalar = getCatalystScalar(
                                self.catalyst.get(),
                                &modLine,
                                self.catalystQuality.get(),
                            );
                        }
                        // Advanced copy uses current(base) for fixed-value modifiers,
                        // in addition to the current(min-max) form handled below.
                        // L1091
                        line = gsub1(&line, "(%-?%d+%.?%d*)%((%-?%d+%.?%d*)%)", "%1");
                        // L1092
                        let pal = self.xget(n!("pendingAffixList"));
                        if pal.truthy() && tlen(&pal) > 0 {
                            let pal = as_table(&pal, "pendingAffixList");
                            let affixes = self.parse_affixes();
                            let affix_of = |modId: &Val| -> Option<&'static Arc<ItemMod>> {
                                let a = match affixes {
                                    Some(a) => a,
                                    None => {
                                        lua_error!("attempt to index field 'affixes' (a nil value)")
                                    }
                                };
                                match modId {
                                    Val::Str(s) => a.0.get(&Name::from(&**s)),
                                    _ => None,
                                }
                            };
                            // L1093
                            if pal.len() > 1 {
                                // Probably a conqueror or essence mod since the mod name is the same for all of them
                                // Try to match the line against one of the mods there
                                // L1096
                                let rangeLine = gsub1(&line, "%-?%d+%.?%d*%(", "(");
                                // L1097
                                let valueStrippedLine = gsub1(&rangeLine, "%-?%d+%.?%d*", "#");
                                // L1098
                                let mut exactAffix: Option<Table> = None;
                                // L1099
                                let mut fallbackAffix: Option<Table> = None;
                                // L1100
                                for (_, pendingAffix) in pal.ipairs() {
                                    let pendingAffix = pendingAffix.table().expect("pending affix");
                                    // L1101
                                    let Some(modData) = affix_of(&pendingAffix.get(n!("modId")))
                                    else {
                                        lua_error!(
                                            "bad argument #1 to 'ipairs' (table expected, got nil)"
                                        )
                                    };
                                    // L1102
                                    for modDataLine in &modData.lines {
                                        // L1103
                                        if *line == **modDataLine || *rangeLine == **modDataLine {
                                            // L1104
                                            exactAffix = Some(pendingAffix.clone());
                                            // L1105
                                            break;
                                        }
                                        // L1107
                                        if fallbackAffix.is_none()
                                            && valueStrippedLine
                                                == gsub1(modDataLine, "%-?%d+%.?%d*", "#")
                                        {
                                            // L1108
                                            fallbackAffix = Some(pendingAffix.clone());
                                        }
                                    }
                                    // L1111
                                    if exactAffix.is_some() {
                                        // L1112
                                        break;
                                    }
                                }
                                // L1115
                                let chosen = exactAffix
                                    .or(fallbackAffix)
                                    .or_else(|| pal.get(Key::I(1)).table());
                                let np = Table::new();
                                if let Some(c) = chosen {
                                    np.push(c);
                                }
                                self.xset(n!("pendingAffixList"), np);
                            }
                            // Use rolling Delta/Range in case one range is 1-3 and another is 1-100 so we get the finest precision possible
                            // L1118
                            let mut bestPrecisionDelta: f64 = -1.0;
                            // L1119
                            let mut bestPrecisionRange: f64 = -1.0;
                            // L1120
                            let rollRanges = Table::new();
                            let pal =
                                as_table(&self.xget(n!("pendingAffixList")), "pendingAffixList");
                            let first = as_table(&pal.get(Key::I(1)), "?");
                            // L1121
                            let affixMod = affix_of(&first.get(n!("modId")));
                            // L1122
                            modLine.set(
                                n!("order"),
                                match affixMod {
                                    Some(m) => onum(m.statOrder.first().copied()),
                                    None => Val::Nil,
                                },
                            );
                            // L1123
                            for c in gmatch_all(
                                &line.clone(),
                                "(%-?%d+%.?%d*)%((%-?%d+%.?%d*%-%-?%d+%.?%d*)%)",
                            ) {
                                let (value, range) = (c[0].clone(), c[1].clone());
                                // L1124
                                let mm = smatch(&range, "(%-?%d+%.?%d*)%-(%-?%d+%.?%d*)")
                                    .expect("matched above");
                                let (mut min, mut max) = (mm[0].clone(), mm[1].clone());
                                // L1125
                                if tonumber(&min).expect("number") > tonumber(&max).expect("number")
                                {
                                    // L1126
                                    std::mem::swap(&mut min, &mut max);
                                }
                                // L1128
                                let delta = tonumber(&max).expect("number") - arith(&min);
                                // L1129
                                rollRanges.push(if delta > 0.0 {
                                    round((arith(&value) - arith(&min)) / delta, Some(6))
                                } else {
                                    0.5
                                });
                                // L1130
                                line = gsub1(
                                    &line,
                                    &format!("{value}%({}%)", gsub1(&range, "%-", "%%-")),
                                    &value,
                                );
                                // L1131
                                if delta > bestPrecisionDelta {
                                    // L1132
                                    bestPrecisionRange =
                                        round((arith(&value) - arith(&min)) / delta, Some(3));
                                    // L1133
                                    bestPrecisionDelta = delta;
                                }
                            }
                            // L1136
                            let target = as_table(&first.get(n!("table")), "table");
                            let e = Table::new();
                            e.set(n!("modId"), first.get(n!("modId")));
                            // Legacy modifiers can roll outside the current data range. Keep the
                            // extrapolated range so crafting a different affix doesn't normalise it.
                            e.set(
                                n!("range"),
                                if rollRanges.len() > 1 {
                                    Val::Table(rollRanges)
                                } else if bestPrecisionDelta > 0.0 {
                                    Val::Num(bestPrecisionRange)
                                } else {
                                    Val::Num(0.5)
                                },
                            );
                            e.set(n!("fractured"), modLine.get(n!("fractured")));
                            target.push(e);
                            // L1143
                            self.xset(n!("pendingAffixList"), Table::new());
                        } else {
                            // L1145
                            let mut bestPrecisionDelta: f64 = -1.0;
                            // L1146
                            let mut bestPrecisionRange: f64 = -1.0;
                            // L1147
                            let mut firstRollRange: Option<f64> = None;
                            // L1148
                            let mut hasIndependentRolls = false;

                            // Advanced copy only provides the endpoints for enum ranges; keep the selected value.
                            // L1151
                            line = pattern::gsub_fn(
                                &line,
                                "(%s*)(%b())",
                                |c| {
                                    let space = c[0].to_str();
                                    let range = c[1].to_str();
                                    // L1152
                                    if sfind_plain(&range, "-").is_some() && !sfind(&range, "%d") {
                                        // L1153
                                        return Some(String::new());
                                    }
                                    // L1155
                                    Some(format!("{space}{range}"))
                                },
                                None,
                            )
                            .0;
                            // L1157
                            let advancedCopyLine = line.clone();

                            // L1159
                            for c in gmatch_all(
                                &line.clone(),
                                "(%-?%d+%.?%d*)%((%-?%d+%.?%d*%-%-?%d+%.?%d*)%)",
                            ) {
                                let (value, range) = (c[0].clone(), c[1].clone());
                                // L1160
                                let mm = smatch(&range, "(%-?%d+%.?%d*)%-(%-?%d+%.?%d*)")
                                    .expect("matched above");
                                let (mut min, mut max) = (mm[0].clone(), mm[1].clone());
                                // L1161
                                if tonumber(&min).expect("number") > tonumber(&max).expect("number")
                                {
                                    // L1162
                                    std::mem::swap(&mut min, &mut max);
                                }
                                // L1164
                                let delta = tonumber(&max).expect("number") - arith(&min);
                                // L1165
                                let rollRange = if delta > 0.0 {
                                    round((arith(&value) - arith(&min)) / delta, Some(6))
                                } else {
                                    0.5
                                };
                                // L1166
                                if firstRollRange.is_some_and(|f| f != rollRange) {
                                    // L1167
                                    hasIndependentRolls = true;
                                }
                                // L1169
                                firstRollRange = Some(firstRollRange.unwrap_or(rollRange));
                                // L1170
                                if delta > bestPrecisionDelta {
                                    // L1171
                                    bestPrecisionRange = rollRange;
                                    // L1172
                                    bestPrecisionDelta = delta;
                                }
                                let pat = format!("{value}%({}%)", gsub1(&range, "%-", "%%-"));
                                // L1174
                                if bestPrecisionRange > 1.0 || bestPrecisionRange < 0.0 {
                                    // L1175
                                    line = gsub1(&line, &pat, &value);
                                } else {
                                    // L1177
                                    let sign = if arith(&value) < 0.0 { "+" } else { "" };
                                    line = gsub1(&line, &pat, &format!("{sign}({min}-{max})"));
                                }
                            }
                            // L1180
                            if hasIndependentRolls {
                                // L1181
                                line = gsub1(
                                    &advancedCopyLine,
                                    "(%-?%d+%.?%d*)%(%-?%d+%.?%d*%-%-?%d+%.?%d*%)",
                                    "%1",
                                );
                            } else if bestPrecisionRange <= 1.0 && bestPrecisionRange >= 0.0 {
                                // L1183
                                modLine.set(n!("range"), bestPrecisionRange);
                            }
                        }
                        // L1186
                        let mut rangedLine = applyRange(
                            &line,
                            &Val::Num(1.0),
                            Some(catalystScalar),
                            modLine.opt_num(n!("corruptedRange")),
                        );
                        // L1187
                        let (mut modList, mut extra) = parseMod(&rangedLine, false);
                        // L1188
                        let nextModGroup =
                            rawLine(l + 1).and_then(|s| smatch1(&s, "{modGroup:([^}]+)}"));
                        // Only combine generated lines that came from the same modifier.
                        // L1191
                        let mlModGroup = modLine.str(n!("modGroup"));
                        if (modList.is_none() || extra.is_some())
                            && rawLine(l + 1).is_some()
                            && mlModGroup.as_deref() == nextModGroup.as_deref()
                        {
                            // Try to combine it with the next line
                            // L1194
                            let nextLine = gsub1(
                                &gsub1(&rawLine(l + 1).unwrap(), "%b{}", ""),
                                " ?%(%l+%)",
                                "",
                            );
                            // L1195
                            let combLine = format!("{line} {nextLine}");
                            // L1196
                            rangedLine = applyRange(
                                &combLine,
                                &Val::Num(1.0),
                                Some(catalystScalar),
                                modLine.opt_num(n!("corruptedRange")),
                            );
                            // L1197
                            (modList, extra) = parseMod(&rangedLine, true);
                            // L1198
                            if modList.is_some() && extra.is_none() {
                                // L1199
                                line = format!("{line}\n{nextLine}");
                                // L1200
                                l += 1;
                            } else {
                                // L1202 LUA-QUIRK: re-parses the combined line (rangedLine was
                                // overwritten at L1196), without isComb
                                (modList, extra) = parseMod(&rangedLine, false);
                            }
                        }
                        // L1205
                        let lineLower = if modLine.truthy(n!("disabled")) {
                            String::new()
                        } else {
                            lower(&line)
                        };
                        // \d+% increased/reduced explicit/implicit/ *tags* modifier magnitudes
                        // L1207
                        let modMagnitudePattern: [&str; 3] = [
                            "(%d+)%% ([ir][ne][cd][ru][ec][ae][sd]e?d?) ?([%a%s]*) modifier magnitudes",
                            // \d+% increased/reduced effect of suffixes/prefixes
                            "(%d+)%% ([ir][ne][cd][ru][ec][ae][sd]e?d?) effect of ([sp][ur][fe]fix)es",
                            // eyes of the greatwolf
                            "([%a%s]*) modifier magnitudes are doubled",
                        ];
                        let ll = lineLower.as_str();
                        // L1212
                        if ll == "implicit modifiers cannot be changed" {
                            // L1213
                            self.xset(n!("implicitsCannotBeChanged"), true);
                        } else if smatch(ll, " prefix modifiers? allowed").is_some() {
                            // L1215
                            let v = prefixes.num0(n!("limit"))
                                + smatch1(ll, "%+(%d+) prefix modifiers? allowed")
                                    .and_then(|s| tonumber(&s))
                                    .unwrap_or(0.0)
                                - smatch1(ll, "%-(%d+) prefix modifiers? allowed")
                                    .and_then(|s| tonumber(&s))
                                    .unwrap_or(0.0);
                            prefixes.set(n!("limit"), v);
                        } else if smatch(ll, " suffix modifiers? allowed").is_some() {
                            // L1217
                            let v = suffixes.num0(n!("limit"))
                                + smatch1(ll, "%+(%d+) suffix modifiers? allowed")
                                    .and_then(|s| tonumber(&s))
                                    .unwrap_or(0.0)
                                - smatch1(ll, "%-(%d+) suffix modifiers? allowed")
                                    .and_then(|s| tonumber(&s))
                                    .unwrap_or(0.0);
                            suffixes.set(n!("limit"), v);
                        } else if sfind(ll, "can be anointed") {
                            // blight uniques and Cord Belt
                            // L1219
                            self.xset(n!("canBeAnointed"), true);
                        } else if ll == "can have a second enchantment modifier" {
                            // L1221
                            self.xset(n!("canHaveTwoEnchants"), true);
                        } else if ll == "can have 1 additional enchantment modifiers" {
                            // L1223
                            self.xset(n!("canHaveTwoEnchants"), true);
                        } else if ll == "can have 2 additional enchantment modifiers" {
                            // L1225
                            self.xset(n!("canHaveTwoEnchants"), true);
                            // L1226
                            self.xset(n!("canHaveThreeEnchants"), true);
                        } else if ll == "can have 3 additional enchantment modifiers" {
                            // L1228
                            self.xset(n!("canHaveTwoEnchants"), true);
                            // L1229
                            self.xset(n!("canHaveThreeEnchants"), true);
                            // L1230
                            self.xset(n!("canHaveFourEnchants"), true);
                        } else if ll == "has elder, shaper and all conqueror influences" {
                            // L1232
                            self.xset(n!("HasElderShaperAndAllConquerorInfluences"), true);
                            // L1233: itemLib.influenceInfo.default = the first six of `.all`
                            // (ItemTools.lua:16-30)
                            for curInfluenceInfo in influenceInfo.iter().take(6) {
                                // L1234
                                self.influences.set(Name::from(curInfluenceInfo.key), true);
                            }
                        } else if smatch(ll, "if the eater of worlds is dominant").is_some() {
                            // L1237
                            self.xset(n!("canHaveEldritchInfluence"), true);
                        } else if ll
                            == "has a crucible passive skill tree with only support passive skills"
                        {
                            // L1239
                            self.xset(n!("canHaveOnlySupportSkillsCrucibleTree"), true);
                        } else if ll == "has a crucible passive skill tree" {
                            // L1241
                            self.xset(n!("canHaveShieldCrucibleTree"), true);
                        } else if ll == "has a two handed sword crucible passive skill tree" {
                            // L1243
                            self.xset(n!("canHaveTwoHandedSwordCrucibleTree"), true);
                        } else if ll == "cannot roll caster modifiers" {
                            // L1245
                            self.xset(n!("restrictTag"), true);
                            // L1246
                            self.xset(n!("noCaster"), true);
                        } else if ll == "cannot roll attack modifiers" {
                            // L1248
                            self.xset(n!("restrictTag"), true);
                            // L1249
                            self.xset(n!("noAttack"), true);
                        } else if ll == "cannot roll modifiers of non-cold damage types" {
                            // L1251
                            self.xset(n!("restrictDamageType"), true);
                            // L1252
                            self.xset(n!("onlyColdDamage"), true);
                        } else if ll == "cannot roll modifiers of non-fire damage types" {
                            // L1254
                            self.xset(n!("restrictDamageType"), true);
                            // L1255
                            self.xset(n!("onlyFireDamage"), true);
                        } else if ll == "cannot roll modifiers of non-lightning damage types" {
                            // L1257
                            self.xset(n!("restrictDamageType"), true);
                            // L1258
                            self.xset(n!("onlyLightningDamage"), true);
                        } else if ll == "cannot roll modifiers of non-chaos damage types" {
                            // L1260
                            self.xset(n!("restrictDamageType"), true);
                            // L1261
                            self.xset(n!("onlyChaosDamage"), true);
                        } else if ll == "cannot roll modifiers of non-physical damage types" {
                            // L1263
                            self.xset(n!("restrictDamageType"), true);
                            // L1264
                            self.xset(n!("onlyPhysicalDamage"), true);
                        }

                        // some tags might not match up exactly to tag strings. this has a list of exceptions
                        // L1268
                        let modMagnitudeTagMap = |w: &str| -> Option<&'static str> {
                            match w {
                                "defence" => Some("defences"),
                                // 3.29 eyes of the greatwolf
                                "enchantment" => Some("enchant"),
                                _ => None,
                            }
                        };
                        // L1273
                        for pattern in modMagnitudePattern {
                            // L1274
                            if !modLine.truthy(n!("disabled"))
                                && sfind(&lower(&rangedLine), pattern)
                            {
                                // L1275
                                let rangedLine = applyRange(
                                    &line,
                                    &lor(
                                        lor(modLine.get(n!("range")), || {
                                            onum(main_defaultItemAffixQuality())
                                        }),
                                        || Val::Num(1.0),
                                    ),
                                    Some(catalystScalar),
                                    modLine.opt_num(n!("corruptedRange")),
                                );
                                // L1276
                                let c = smatch(&lower(&rangedLine), pattern);
                                let mut amount: Option<Val> = c.as_ref().map(|c| vstr(&c[0]));
                                let mut increaseOrDecrease: Option<String> =
                                    c.as_ref().and_then(|c| c.get(1).cloned());
                                let mut modTagsString: Option<String> =
                                    c.as_ref().and_then(|c| c.get(2).cloned());
                                // L1277
                                let mut multiplier: Option<f64> = None;
                                // "are doubled" format -> swap variables
                                // L1279
                                if amount.is_some()
                                    && !(increaseOrDecrease.is_some() || modTagsString.is_some())
                                {
                                    // L1280
                                    modTagsString =
                                        amount.as_ref().and_then(Val::str).map(|s| s.to_string());
                                    // L1281
                                    amount = Some(Val::Num(100.0));
                                    // L1282
                                    increaseOrDecrease = Some("increased".to_owned());
                                    // L1283
                                    multiplier = Some(2.0);
                                }
                                // L1285
                                if let (Some(amount), Some(modTagsString)) =
                                    (amount.as_ref(), modTagsString.as_ref())
                                {
                                    let iod = increaseOrDecrease.as_deref();
                                    if iod == Some("increased") || iod == Some("reduced") {
                                        let amountNum = match amount {
                                            Val::Num(x) => *x,
                                            Val::Str(s) => tonumber(s).expect("digits"),
                                            _ => unreachable!(),
                                        };
                                        // L1286
                                        let modTags = Table::new();
                                        // L1287
                                        let mut modType: Option<String> = None;
                                        // L1288
                                        let quality = if iod == Some("increased") {
                                            amountNum
                                        } else {
                                            -amountNum
                                        };
                                        // L1289
                                        if modTagsString == "explicit physical and chaos damage" {
                                            // L1290
                                            let m = Table::new();
                                            m.set(n!("tags"), str_list(["damage"]));
                                            m.set(n!("anyTags"), str_list(["physical", "chaos"]));
                                            m.set(n!("quality"), quality);
                                            m.set(n!("modType"), "explicit");
                                            modMagnitudeMods.push(m);
                                        } else {
                                            // explicit elemental damage -> tags = {elemental, damage}, modType = explicit
                                            // L1293
                                            for word in
                                                gmatch_all(&format!("{modTagsString} "), "%S+")
                                            {
                                                // L1294
                                                let word = lower(&word[0]);
                                                // L1295
                                                let word = modMagnitudeTagMap(&word)
                                                    .map_or(word, str::to_owned);
                                                // L1296
                                                if word == "implicit"
                                                    || word == "explicit"
                                                    || word == "enchant"
                                                {
                                                    // L1297
                                                    modType = Some(word);
                                                } else {
                                                    // L1299
                                                    modTags.push(vstr(&word));
                                                }
                                            }
                                            // L1302
                                            let m = Table::new();
                                            m.set(n!("tags"), modTags);
                                            m.set(n!("quality"), quality);
                                            m.set(n!("multiplier"), onum(multiplier));
                                            m.set(n!("modType"), ostr(modType.as_deref()));
                                            modMagnitudeMods.push(m);
                                        }
                                        // L1304
                                        break;
                                    }
                                }
                            }
                        }
                        // L1308
                        let modLines: Shared<Vec<Table>>;
                        let nEnchant = self.enchantModLines.get().len() as f64;
                        let nScourge = self.scourgeModLines.get().len() as f64;
                        let nImplicit = self.implicitModLines.get().len() as f64;
                        // L1309
                        if modLine.truthy(n!("enchant"))
                            || (modLine.truthy(n!("crafted"))
                                && nEnchant + nImplicit < implicitLines)
                        {
                            // L1310
                            modLines = self.enchantModLines.get();
                        } else if modLine.truthy(n!("scourge")) {
                            // L1312
                            modLines = self.scourgeModLines.get();
                        } else if sfind(&line, "Requires Class") {
                            // L1314
                            modLines = self.classRequirementModLines.get();
                        } else if modLine.truthy(n!("implicit"))
                            || (!modLine.truthy(n!("crafted"))
                                && nEnchant + nScourge + nImplicit < implicitLines)
                        {
                            // L1316
                            modLines = self.implicitModLines.get();
                        } else if modLine.truthy(n!("crucible")) {
                            // L1318
                            modLines = self.crucibleModLines.get();
                        } else {
                            // L1320
                            modLines = self.explicitModLines.get();
                        }
                        // L1322
                        modLine.set(n!("line"), vstr(&line));
                        // L1323
                        if let Some(modList) = modList {
                            // L1324
                            modLine.set(n!("modList"), modlist_table(modList));
                            // L1325
                            modLine.set(n!("extra"), ostr(extra.as_deref()));
                            // L1326
                            modLine.set(n!("valueScalar"), catalystScalar);
                            // L1327
                            let r = lor(modLine.get(n!("range")), || {
                                onum(main_defaultItemAffixQuality())
                            });
                            modLine.set(n!("range"), r);
                            // L1328
                            modLines.push(modLine.clone());
                            // L1329
                            if mode == "GAME" {
                                // L1330
                                if gameModeStage == Stage::FINDIMPLICIT {
                                    // L1331
                                    gameModeStage = Stage::IMPLICIT;
                                } else if gameModeStage == Stage::FINDEXPLICIT {
                                    // L1333
                                    foundExplicit = true;
                                    // L1334
                                    gameModeStage = Stage::EXPLICIT;
                                } else if gameModeStage == Stage::EXPLICIT {
                                    // L1336
                                    foundExplicit = true;
                                }
                            } else {
                                // L1339
                                foundExplicit = true;
                            }
                        } else if mode == "GAME" {
                            // L1342
                            let base = self.base.get();
                            if gameModeStage == Stage::IMPLICIT
                                || gameModeStage == Stage::EXPLICIT
                                || (gameModeStage == Stage::FINDIMPLICIT
                                    && !d.itemBases.contains_key(line.as_str())
                                    && !(*self.name.get() == *line)
                                    && !sfind(&line, "Two%-Toned")
                                    && !base.as_ref().is_some_and(|b| {
                                        line == b.r#type.as_str()
                                            || b.subType.is_some_and(|st| {
                                                line == format!("{} {}", st, b.r#type)
                                            })
                                    }))
                            {
                                // L1343
                                modLine.set(n!("modList"), Table::new());
                                // L1344
                                modLine.set(n!("extra"), vstr(&line));
                                // L1345
                                modLine.set(n!("range"), onum(main_defaultItemAffixQuality()));
                                // L1346
                                modLines.push(modLine.clone());
                            } else if gameModeStage == Stage::FINDEXPLICIT {
                                // L1348
                                gameModeStage = Stage::DONE;
                            }
                        } else if foundExplicit
                            || (!foundExplicit && gameModeStage == Stage::EXPLICIT)
                        {
                            // L1351
                            modLine.set(n!("modList"), Table::new());
                            // L1352
                            modLine.set(n!("extra"), vstr(&line));
                            // L1353
                            modLines.push(modLine.clone());
                        }
                    }
                }
            }
            // ::continue::
            // L1358
            l += 1;
        }
        // L1360
        let rarity = self.rarity.get();
        let isUniqueRarity = rarity == "UNIQUE" || rarity == "RELIC";
        if self.xget(n!("advancedCopy")).truthy() && isUniqueRarity {
            // L1361
            let usso = uniqueModStatOrder.with(|u| u.borrow().clone());
            let usso = match usso {
                Some(t) => t,
                None => {
                    // L1362
                    let t = Table::new();
                    let exact = Table::new();
                    let normalised = Table::new();
                    t.set(n!("exact"), exact.clone());
                    t.set(n!("normalised"), normalised.clone());
                    uniqueModStatOrder.with(|u| *u.borrow_mut() = Some(t.clone()));
                    let Some(itemExclusive) = d.itemMods.get(&n!("ItemExclusive")) else {
                        lua_error!("bad argument #1 to 'pairs' (table expected, got nil)")
                    };
                    // L1363 (the result is a min over every line, so the order is not observable)
                    for (_, r#mod) in &itemExclusive.0 {
                        // L1364
                        for (index, line) in r#mod.lines.iter().enumerate() {
                            // L1365
                            let exactLine = gsub1(&lower(line), "\n", " ");
                            // L1366
                            let statLine = normaliseModLine(line);
                            let Some(so) = r#mod.statOrder.get(index).copied() else {
                                lua_error!("bad argument #2 to 'min' (number expected, got nil)")
                            };
                            // L1367
                            let ek = Name::from(exactLine.as_str());
                            exact.set(ek, m_min(exact.get(ek).num_or(f64::INFINITY), so));
                            // L1368
                            let sk = Name::from(statLine.as_str());
                            normalised.set(sk, m_min(normalised.get(sk).num_or(f64::INFINITY), so));
                        }
                    }
                    t
                }
            };
            let exact = as_table(&usso.get(n!("exact")), "exact");
            let normalised = as_table(&usso.get(n!("normalised")), "normalised");
            // L1372
            for modLine in self.explicitModLines.get().snapshot() {
                let mlLine = modLine.str(n!("line")).expect("line");
                // L1373
                let exactLine = gsub1(&lower(&mlLine), "\n", " ");
                // L1374
                let o = lor(exact.get(Name::from(exactLine.as_str())), || {
                    normalised.get(Name::from(normaliseModLine(&mlLine).as_str()))
                });
                modLine.set(n!("order"), o);
            }
        }
        // L1378
        let base = self.base.get();
        if let Some(base) = base.as_ref().filter(|_| self.sockets.get().len() > 0) {
            // In-game requirement totals include requirements from socketed gems.
            // Derive the item's attribute requirements from its base and local mods instead.
            // L1381
            self.requirements
                .set(n!("str"), base.req.str.unwrap_or(0.0));
            // L1382
            self.requirements
                .set(n!("dex"), base.req.dex.unwrap_or(0.0));
            // L1383
            self.requirements
                .set(n!("int"), base.req.int.unwrap_or(0.0));
        }
        // L1385
        if let Some(base) = base.as_ref() {
            if !self.requirements.truthy(n!("level")) {
                // L1386
                if importedLevelReq.is_some() && self.sockets.get().len() == 0 {
                    // Requirements on imported items can only be trusted for items with no sockets
                    // L1388
                    self.requirements.set(n!("level"), onum(importedLevelReq));
                } else {
                    // L1390
                    self.requirements.set(n!("level"), onum(base.req.level));
                }
            }
        }
        // hack for cane of kulemak
        // L1394
        if self
            .title_str()
            .is_some_and(|t| lower(&t) == "cane of kulemak")
            && isUniqueRarity
            && self.xget(n!("advancedCopy")).truthy()
        {
            // L1395
            for r#mod in self.explicitModLines.get().snapshot() {
                // L1396
                if smatch(&r#mod.str(n!("line")).expect("line"), "magnitude").is_none() {
                    // L1397
                    r#mod.set(n!("unveiled"), true);
                }
            }
        }
        // L1401
        if self.xget(n!("advancedCopy")).truthy() || self.xget(n!("crafted")).truthy() {
            // apply mod magnitude boost to matching mods
            // L1403
            if modMagnitudeMods.len() > 0 {
                // L1404
                for (_, modMagnitudeMod) in modMagnitudeMods.ipairs() {
                    let modMagnitudeMod = modMagnitudeMod.table().expect("table");
                    // L1405
                    let modLists: Vec<Option<Shared<Vec<Table>>>>;
                    // L1406
                    if let Some(modType) = modMagnitudeMod.str(n!("modType")) {
                        // L1407
                        modLists = vec![self.modLines_named(&modType)];
                    } else {
                        // L1409
                        modLists = vec![
                            Some(self.implicitModLines.get()),
                            Some(self.explicitModLines.get()),
                            Some(self.enchantModLines.get()),
                        ];
                    }
                    // L1411 (`ipairs({ nil })` visits nothing)
                    for mods in modLists.into_iter().map_while(|m| m) {
                        // L1412
                        for r#mod in mods.snapshot() {
                            'modMagnitudeContinue: {
                                // avoid scaling variant lines which are not active
                                // L1414
                                if r#mod.truthy(n!("variantList"))
                                    && self.GetModLineVariantCount(&r#mod) == 0.0
                                {
                                    // L1415
                                    break 'modMagnitudeContinue;
                                }
                                // Modifiers that grant skills are not affected by modifier magnitude.
                                // L1418
                                let mut grantsSkill = false;
                                // L1419
                                for parsedMod in ipairs_tbl(&r#mod.get(n!("modList"))) {
                                    // L1420
                                    if parsedMod.get(n!("name")).str().as_deref()
                                        == Some("ExtraSkill")
                                    {
                                        // L1421
                                        grantsSkill = true;
                                        // L1422
                                        break;
                                    }
                                }
                                // L1425
                                if r#mod.truthy(n!("extra")) && !grantsSkill {
                                    // L1426
                                    let line = lower(&r#mod.str(n!("line")).expect("line"));
                                    // L1427
                                    grantsSkill = smatch(&line, "^grants level %d+ ").is_some()
                                        || smatch(&line, "^grants %D+$").is_some();
                                }
                                // Create a fast lookup table for all provided tags
                                // L1430
                                let mut tagLookup: Vec<String> = Vec::new();
                                // L1431
                                for curTag in ipairs_tbl(&r#mod.get(n!("modTags"))) {
                                    // L1432
                                    if let Val::Str(s) = curTag {
                                        tagLookup.push(s.to_string());
                                    }
                                }
                                // these aren't actual mod tags but do appear in mod magnitude mods
                                // L1435
                                for lineFlag in ["unveiled", "prefix", "suffix"] {
                                    // L1436
                                    if r#mod.truthy(Name::from(lineFlag)) {
                                        // L1437
                                        tagLookup.push(lineFlag.to_owned());
                                    }
                                }
                                let has = |t: &Val| match t {
                                    Val::Str(s) => tagLookup.iter().any(|x| **x == **s),
                                    _ => false,
                                };
                                // L1440
                                let mut r#match = true;
                                // L1441
                                for magnitudeTag in ipairs_tbl(&modMagnitudeMod.get(n!("tags"))) {
                                    // L1442
                                    if !has(&magnitudeTag) {
                                        // L1443
                                        r#match = false;
                                    }
                                }
                                // L1446
                                if let Some(anyTags) = modMagnitudeMod.table(n!("anyTags")) {
                                    if !(has(&anyTags.get(Key::I(1)))
                                        || has(&anyTags.get(Key::I(2))))
                                    {
                                        // L1447
                                        r#match = false;
                                    }
                                }
                                // L1449
                                if r#match && !r#mod.truthy(n!("unscalable")) && !grantsSkill {
                                    // L1450
                                    if let Some(multiplier) =
                                        modMagnitudeMod.opt_num(n!("multiplier"))
                                    {
                                        // L1451
                                        let v = r#mod.opt_num(n!("valueScalar")).unwrap_or(1.0)
                                            * multiplier;
                                        r#mod.set(n!("valueScalar"), v);
                                    } else {
                                        // L1453
                                        let v = r#mod.opt_num(n!("valueScalar")).unwrap_or(1.0)
                                            + (modMagnitudeMod.num(n!("quality")) / 100.0);
                                        r#mod.set(n!("valueScalar"), v);
                                    }
                                }
                                // L1456
                                if let Some(vs) =
                                    r#mod.opt_num(n!("valueScalar")).filter(|v| *v != 1.0)
                                {
                                    // L1457
                                    let rangedLine = applyRange(
                                        &r#mod.str(n!("line")).expect("line"),
                                        &lor(r#mod.get(n!("range")), || Val::Num(1.0)),
                                        Some(vs),
                                        Some(1.0),
                                    );
                                    // L1458
                                    let (modList, extra) = parseMod(&rangedLine, false);
                                    // L1459
                                    if let Some(modList) = modList {
                                        // L1460
                                        r#mod.set(n!("displayValueScalar"), 1.0);
                                        // L1461
                                        r#mod.set(n!("modList"), modlist_table(modList));
                                        // L1462
                                        r#mod.set(n!("extra"), ostr(extra.as_deref()));
                                    }
                                }
                            }
                            // ::modMagnitudeContinue::
                        }
                    }
                }
            }
        }
        // L1470
        if self.xget(n!("advancedCopy")).truthy() && self.explicitModLines.get().len() > 1 {
            // L1471
            sortCraftedModLines(&self.explicitModLines.get());
        }
        // L1473
        self.xset(n!("affixLimit"), 0.0);
        // L1474
        if self.xget(n!("crafted")).truthy() {
            let r = self.rarity.get();
            // L1475
            if self.xget(n!("affixes")).is_nil() {
                // L1476
                self.xset(n!("crafted"), false);
            } else if r == "MAGIC" {
                // L1478
                if prefixes.truthy(n!("limit")) || suffixes.truthy(n!("limit")) {
                    // L1479
                    prefixes.set(
                        n!("limit"),
                        m_max(
                            m_min(prefixes.opt_num(n!("limit")).unwrap_or(0.0) + 1.0, 2.0),
                            0.0,
                        ),
                    );
                    // L1480
                    suffixes.set(
                        n!("limit"),
                        m_max(
                            m_min(suffixes.opt_num(n!("limit")).unwrap_or(0.0) + 1.0, 2.0),
                            0.0,
                        ),
                    );
                    // L1481
                    self.xset(
                        n!("affixLimit"),
                        prefixes.num(n!("limit")) + suffixes.num(n!("limit")),
                    );
                } else {
                    // L1483
                    self.xset(n!("affixLimit"), 2.0);
                }
            } else if r == "RARE" {
                // L1486
                let ty = self.r#type.get();
                // `self.base.subType` is read only for a Jewel (`self.base` may be nil otherwise:
                // a re-parse keeps the previous `type`/`affixes`)
                let four = (ty == "Jewel"
                    && !({
                        let Some(base) = self.base.get() else {
                            lua_error!("attempt to index field 'base' (a nil value)")
                        };
                        base.subType.is_some_and(|s| s == "Abyss")
                    } && self.corrupted.get()))
                    || ty == "Graft";
                let affixLimit = if four { 4.0 } else { 6.0 };
                self.xset(n!("affixLimit"), affixLimit);
                // L1487
                if prefixes.truthy(n!("limit")) || suffixes.truthy(n!("limit")) {
                    // L1488
                    prefixes.set(
                        n!("limit"),
                        m_max(
                            m_min(
                                prefixes.opt_num(n!("limit")).unwrap_or(0.0) + affixLimit / 2.0,
                                affixLimit,
                            ),
                            0.0,
                        ),
                    );
                    // L1489
                    suffixes.set(
                        n!("limit"),
                        m_max(
                            m_min(
                                suffixes.opt_num(n!("limit")).unwrap_or(0.0) + affixLimit / 2.0,
                                affixLimit,
                            ),
                            0.0,
                        ),
                    );
                    // L1490
                    self.xset(
                        n!("affixLimit"),
                        prefixes.num(n!("limit")) + suffixes.num(n!("limit")),
                    );
                }
            } else if let Some(rlu) = self.rareLikeUnique_ref() {
                // L1493
                prefixes.set(n!("limit"), rlu.prefixLimit);
                // L1494
                suffixes.set(n!("limit"), rlu.suffixLimit);
                // L1495
                self.xset(
                    n!("affixLimit"),
                    prefixes.num(n!("limit")) + suffixes.num(n!("limit")),
                );
            } else {
                // L1497
                self.xset(n!("crafted"), false);
            }
            // L1499
            if self.xget(n!("crafted")).truthy() {
                let affixes = self.parse_affixes().expect("crafted implies affixes");
                // L1500
                for list in [&prefixes, &suffixes] {
                    // L1501
                    let upper = match list.opt_num(n!("limit")) {
                        Some(x) => x,
                        None => self.xget(n!("affixLimit")).num() / 2.0,
                    };
                    let mut i: f64 = 1.0;
                    while i <= upper {
                        let ik = Key::from(i);
                        // L1502
                        match list.get(ik) {
                            v if !v.truthy() => {
                                // L1503
                                let e = Table::new();
                                e.set(n!("modId"), "None");
                                list.set(ik, e);
                            }
                            v => {
                                let entry = as_table(&v, "?");
                                let modId = entry.get(n!("modId"));
                                let known = |m: &Val| match m {
                                    Val::Str(s) => affixes.0.contains_key(&Name::from(&**s)),
                                    _ => false,
                                };
                                // L1504
                                if !matches!(&modId, Val::Str(s) if &**s == "None")
                                    && !known(&modId)
                                {
                                    // L1505 LUA-ORDER: `pairs(self.affixes)`, first affix whose
                                    // display name matches (dump order here)
                                    for (mid, r#mod) in &affixes.0 {
                                        // L1506
                                        if let (Val::Str(s), Some(a)) =
                                            (&modId, r#mod.affix.as_deref())
                                        {
                                            if **s == *a {
                                                // L1507
                                                entry.set(n!("modId"), *mid);
                                                // L1508
                                                break;
                                            }
                                        }
                                    }
                                    // L1511
                                    if !known(&entry.get(n!("modId"))) {
                                        // L1512
                                        entry.set(n!("modId"), "None");
                                    }
                                }
                            }
                        }
                        i += 1.0;
                    }
                }
            }
        }
        // L1519
        if let Some(base) = self.base.get() {
            if let Some(socketLimit) = base.socketLimit {
                // L1520
                let sockets = self.sockets.get();
                if sockets.len() == 0 {
                    // L1521
                    let mut i = 1.0;
                    while i <= socketLimit {
                        // L1522
                        let s = Table::new();
                        s.set(n!("color"), self.xget(n!("defaultSocketColor")));
                        s.set(n!("group"), 0.0);
                        sockets.push(s);
                        i += 1.0;
                    }
                }
            }
        }
        // L1529
        self.abyssalSocketCount.set(0.0);
        // L1530
        if self.xget(n!("usesVariantGroups")).truthy() {
            // L1531
            self.NormaliseVariantSelections();
        } else if let Some(variantList) = self.variantList.get() {
            let n = variantList.len() as f64;
            // L1533
            self.variant
                .set(Some(m_min(n, self.variant.get().unwrap_or(n))));
            let alts = [
                (n!("hasAltVariant"), &self.variantAlt),
                (n!("hasAltVariant2"), &self.variantAlt2),
                (n!("hasAltVariant3"), &self.variantAlt3),
                (n!("hasAltVariant4"), &self.variantAlt4),
                (n!("hasAltVariant5"), &self.variantAlt5),
            ];
            // L1534-1548
            for (has, variantAlt) in alts {
                if self.xget(has).truthy() {
                    variantAlt.set(Some(m_min(n, variantAlt.get().unwrap_or(n))));
                }
            }
        }
        // L1550
        if let Some(mutatedLines) = self.mutatedLines_ref() {
            // Match both sides so the same checkbox can apply or revert the transformation.
            // L1552
            let checkMod = |modId: &str, newModId: &str, mutated: bool| {
                // L1553
                let category = |c: Name| match d.itemMods.get(&c) {
                    Some(t) => t,
                    None => lua_error!("attempt to index field '{}' (a nil value)", c.as_str()),
                };
                let mid = Name::from(modId);
                let originalMod = (if mutated {
                    category(n!("Foulborn")).0.get(&mid)
                } else {
                    None
                })
                .or_else(|| category(n!("ItemExclusive")).0.get(&mid));
                // L1554
                let Some(originalMod) = originalMod else {
                    // L1555
                    ConPrintf(lfmt!(
                        "mod not found while testing mutated mods %s, %s",
                        modId,
                        if mutated {
                            pob_lua::FmtArg::Bool(true)
                        } else {
                            pob_lua::FmtArg::Nil
                        }
                    ));
                    // L1556
                    return;
                };
                // L1558
                let findMatchingLines = |lines: &[String]| -> Option<Vec<Table>> {
                    // L1559
                    let mut matchingLines: Vec<Table> = Vec::new();
                    // L1560
                    let mut matchedLines: Vec<Table> = Vec::new();
                    // L1561
                    for line in lines {
                        // L1562
                        let statLine = normaliseModLine(line);
                        // L1563
                        for modLine in self.explicitModLines.get().snapshot() {
                            // L1564
                            if !matchedLines.iter().any(|m| m.ptr_eq(&modLine))
                                && normaliseModLine(&modLine.str(n!("line")).expect("line"))
                                    == statLine
                                && self.CheckModLineVariant(&modLine)
                            {
                                // L1567
                                matchedLines.push(modLine.clone());
                                // L1568
                                matchingLines.push(modLine);
                                // L1569
                                break;
                            }
                        }
                    }
                    // L1573
                    (lines.len() == matchingLines.len()).then_some(matchingLines)
                };
                // Some modifier descriptions are represented by separate lines in PoB.
                // L1576
                let lines: Vec<String> = originalMod.lines.iter().map(|x| x.to_string()).collect();
                let matchingLines =
                    findMatchingLines(&[lines.join(" ")]).or_else(|| findMatchingLines(&lines));
                // L1578
                if let Some(matchingLines) = matchingLines {
                    // L1579
                    for modLine in matchingLines {
                        // L1580
                        modLine.set(n!("modId"), vstr(modId));
                        // L1581
                        modLine.set(n!("newModId"), vstr(newModId));
                        // L1582
                        if mutated {
                            // L1583
                            modLine.set(n!("mutated"), true);
                        }
                    }
                }
            };
            // L1552 LUA-ORDER: `pairs(self.mutatedLines)` (dkjson object, random in PoB); the
            // port visits the dump order. Two pairs that mark the same mod line would race.
            for (origModId, foulModId) in &mutatedLines.0 {
                // check if there are foulborn source mods
                // L1590
                checkMod(origModId, foulModId.as_str(), false);
                // check if there are foulborn mods which can be reverted, and mark
                // them as mutated as in case they are missing the flag
                // L1593
                checkMod(foulModId.as_str(), origModId, true);
            }
        }
        // L1596
        for v in self.explicitModLines.get().snapshot() {
            // L1597
            if v.truthy(n!("mutated")) {
                // L1598
                self.xset(n!("foulborn"), true);
            }
        }
        // ensure that foulborn items have the name prefix
        // L1602
        let hasFoulbornPrefix = self
            .title_str()
            .is_some_and(|t| sfind(&lower(&t), "^foulborn "));
        // L1603
        if self.xget(n!("foulborn")).truthy() && !hasFoulbornPrefix {
            // L1604
            let Some(title) = self.title_str() else {
                lua_error!("attempt to concatenate field 'title' (a nil value)")
            };
            self.title
                .set(Some(LStr::from(format!("Foulborn {title}").as_str())));
        } else if !self.xget(n!("foulborn")).truthy() && hasFoulbornPrefix {
            // L1606
            let t = gsub1(&self.title_str().unwrap(), "[Ff]oulborn ", "");
            self.title.set(Some(LStr::from(t.as_str())));
        }
        // L1608
        if let (Some(baseName), Some(title)) = (self.baseName.get(), self.title_str()) {
            // L1609
            let n = format!("{title}, {}", gsub1(baseName.as_str(), " %(.+%)", ""));
            self.name.set(LStr::from(n.as_str()));
        }
        // L1611
        if self.quality.get().is_none() {
            // L1612
            self.NormaliseQuality();
            // L1613
            if highQuality {
                // Behavior of NormaliseQuality should be looked at because calling it twice has different results.
                // Leaving it alone for now. Just moving it here from Main.lua so BuildAndParseRaw doesn't need to be called.
                // L1616
                self.NormaliseQuality();
            }
        }
        // L1619
        if let Some(base) = self.base.get() {
            if let Some(enchant) = base.enchant.as_ref() {
                if self.enchantModLines.get().len() == 0 {
                    // L1620
                    let mut enchantIndex: usize = 1;
                    // L1621
                    for line in gmatch_all(enchant, "[^\n]+") {
                        let line = &line[0];
                        // L1622
                        let (modList, extra) = parseMod(line, false);
                        // L1623
                        let ml = Table::new();
                        ml.set(n!("line"), vstr(line));
                        ml.set(n!("crafted"), true);
                        ml.set(n!("implicit"), true);
                        ml.set(n!("enchant"), true);
                        ml.set(n!("extra"), ostr(extra.as_deref()));
                        ml.set(n!("modList"), modlist_table(modList.unwrap_or_default()));
                        ml.set(
                            n!("modTags"),
                            match base.enchantModTypes.get(enchantIndex - 1) {
                                Some(tags) => str_list(tags.iter().map(|t| t.as_str())),
                                None => Table::new(),
                            },
                        );
                        self.enchantModLines.get().push(ml);
                        // L1632
                        enchantIndex += 1;
                    }
                }
            }
        }
        // L1635
        self.parse_BuildModList();
        // L1636
        if deferJewelRadiusIndexAssignment {
            // L1637
            let Some(jewelData) = self.jewelData.get() else {
                lua_error!("attempt to index field 'jewelData' (a nil value)")
            };
            self.jewelRadiusIndex
                .set(jewelData.opt_num(n!("radiusIndex")));
        }
        // L1639
        let r = self.rarity.get();
        self.xset(n!("isUnique"), r == "UNIQUE" || r == "RELIC");
    }
}
