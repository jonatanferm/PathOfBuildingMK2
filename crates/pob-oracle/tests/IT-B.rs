//! IT-B acceptance (calc doc §11.1 row IT-B): Item core (Item.lua 1-423, 1646-2191),
//! ItemSlotHelper.lua and Utils.lua against PoB under LuaJIT (interpreter, `jit.off`).
//!
//! ParseRaw is IT-A's, so the Rust items are not parsed by Rust: PoB parses every item
//! (`new("Item"):Item(raw, …)`), `corpus/IT-B/item_oracle.lua` dumps the parsed item's fields
//! (typed and `extra`, CCR-0015 §2 shapes) and PoB's results, and this test injects the dump
//! into a Rust `Item` and runs the Rust functions on it:
//!
//! * `BuildRaw` text (= `BuildRaw(ParseRaw(x))` with ParseRaw from PoB), for every unique
//!   (all types, `data.uniques`), every rare (`data.rares`, state before Craft), every item of
//!   the all57 build corpus (`.parity/builds`) and 60 synthetic variant-group / version items
//!   (Zana's Ingenuity, the one unique with groups, and a made-up versioned item, under random
//!   group/version selections; the made-up item also has 7-decimal `{range:}` and 5-decimal
//!   `{corruptedRange:}` lines for BuildRaw's `round(.., 6)` / `round(.., 2)`); again for the first 8 and the last variant of each non-group
//!   variant item (with `CheckModLineVariant` for every variant);
//! * `GetPrimarySlot`, `CheckModLineVariant` + `GetModLineVariantCount` for every mod line,
//!   `FindModifierSubstring` for 32 substrings at the item's primary slot,
//!   `GetVariantGroupOptions` (both modes) and `IsVariantGroupOptionEligible` per group/variant;
//! * `NormaliseQuality`, `NormaliseVariantSelections` (state after), `MutateMod` (text after);
//! * `GetModSpawnWeight` (plain over all `itemMods.Item` mods, with include/exclude tags over a
//!   fifth of them) and `GetNecropolisModSpawnWeight` over `necropolisMods`, for half of the
//!   rares and every 20th corpus item; `CheckIfModIsDelve` over `itemMods.Item`;
//! * the Item.lua locals (`normaliseModLine`, `specToNumber`, `getCatalystScalar`,
//!   `sortCraftedModLines`; the oracle re-runs Item.lua with a trailing `return` of them, as the
//!   headless LuaJIT has no `debug` library) on vectors;
//! * Utils `stringify` / `stringifyInline` / `saveTableToFile` on 300 random tables.
//!
//! `Craft` (rares, plus two synthetic crafted rares with list `{range:a,b}` affixes, which no
//! `data.rares` entry has) needs IT-C's `itemLib.applyRange`; while that is a stub its
//! comparison is counted as skipped. After `MutateMod`/`Craft` PoB re-parses (`BuildAndParseRaw`); while
//! IT-A's ParseRaw is a stub the Rust text is the `BuildRaw` before the re-parse, which is the
//! raw PoB re-parsed (`self.raw`).
//!
//! Self-skips without PoB / the data dump (set `POB_ORACLE_REQUIRE=1` to fail instead). The
//! inputs are gitignored, so from a worktree point `POB_SRC`, `POB_PARITY_DIR` and
//! `POB_DUMP_DIR` at the main checkout's `.PathOfBuilding/src`, `.parity` and `data/dump`.
//! `IT_B_LIMIT=<n>` limits the uniques (quick runs); `IT_B_DUMP=<file>` keeps the oracle JSON.

#![allow(clippy::pedantic, clippy::explicit_auto_deref, clippy::op_ref)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;

use pob_calc::classes::item::{self as item_mod, ItemAffixes};
use pob_calc::classes::item_tools;
use pob_calc::classes::Item;
use pob_calc::prelude::{tostring_num, LuaError, Name, Shared};
use pob_calc::{Key, LStr, Table, Val};
use pob_oracle::headless::{Engine, Ret};
use pob_oracle::jit::JitPob;
use pob_oracle::oracle::{find_parity_dir, find_pob_src, find_up};
use serde_json::Value as J;

fn skip(why: &str) {
    let required = std::env::var_os("POB_ORACLE_REQUIRE").is_some_and(|v| v == "1");
    assert!(!required, "required input missing: {why}");
    eprintln!("SKIPPED: {why}");
}

fn init_data() -> bool {
    if pob_gamedata::is_initialised() {
        return true;
    }
    let dir = std::env::var_os("POB_DUMP_DIR")
        .map(PathBuf::from)
        .or_else(|| find_up("data/dump/manifest.json").map(|p| p.parent().unwrap().to_path_buf()));
    let Some(dir) = dir else { return false };
    if dir.join("data.bin").is_file() {
        pob_gamedata::init_from_dir(&dir).unwrap_or_else(|e| panic!("data.bin: {e}"));
    } else {
        let tree = pob_gamedata::load_dump_dir(&dir).unwrap_or_else(|e| panic!("{e}"));
        pob_gamedata::init_with(
            pob_gamedata::Data::from_tree(&tree).unwrap_or_else(|e| panic!("{e}")),
        );
    }
    true
}

/// Silence the default panic message for Lua errors (they are expected and caught).
fn quiet_lua_errors() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if info.payload().downcast_ref::<LuaError>().is_none() {
            prev(info);
        }
    }));
}

fn catch<R>(f: impl FnOnce() -> R) -> Result<R, String> {
    // pob_lua::error::catch (Lua pcall): only LuaError payloads are caught
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => Ok(r),
        Err(payload) => match payload.downcast::<LuaError>() {
            Ok(e) => Err(e.0),
            Err(other) => std::panic::resume_unwind(other),
        },
    }
}

// ---- corpus ---------------------------------------------------------------------------------

fn xml_unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let Some(j) = tail.find(';') else {
            out.push_str(tail);
            return out;
        };
        let ent = &tail[1..j];
        match ent {
            "lt" => out.push('<'),
            "gt" => out.push('>'),
            "amp" => out.push('&'),
            "quot" => out.push('"'),
            "apos" => out.push('\''),
            e if e.starts_with("#x") => {
                if let Some(c) = u32::from_str_radix(&e[2..], 16)
                    .ok()
                    .and_then(char::from_u32)
                {
                    out.push(c);
                }
            }
            e if e.starts_with('#') => {
                if let Some(c) = e[1..].parse::<u32>().ok().and_then(char::from_u32) {
                    out.push(c);
                }
            }
            _ => out.push_str(&tail[..=j]),
        }
        rest = &tail[j + 1..];
    }
    out.push_str(rest);
    out
}

/// `(src, raw)` of every `<Item>` of every build in `.parity/builds` (child elements removed).
fn corpus_items() -> Vec<(String, String)> {
    let Some(parity) = find_parity_dir() else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = std::fs::read_dir(parity.join("builds"))
        .map(|d| d.filter_map(|e| e.ok().map(|e| e.path())).collect())
        .unwrap_or_default();
    files.retain(|p| p.extension().is_some_and(|e| e == "xml"));
    files.sort();
    let mut out = Vec::new();
    for f in files {
        let Ok(xml) = std::fs::read_to_string(&f) else {
            continue;
        };
        let stem = f.file_stem().unwrap().to_string_lossy().into_owned();
        let mut rest = xml.as_str();
        let mut n = 0;
        while let Some(i) = rest.find("<Item ") {
            let after = &rest[i..];
            let Some(open_end) = after.find('>') else {
                break;
            };
            let body_start = &after[open_end + 1..];
            let Some(close) = body_start.find("</Item>") else {
                break;
            };
            let body = &body_start[..close];
            // drop child elements (<ModRange …/>)
            let mut text = String::new();
            let mut b = body;
            while let Some(lt) = b.find('<') {
                text.push_str(&b[..lt]);
                match b[lt..].find('>') {
                    Some(gt) => b = &b[lt + gt + 1..],
                    None => {
                        b = "";
                    }
                }
            }
            text.push_str(b);
            n += 1;
            out.push((format!("{stem}#{n}"), xml_unescape(&text)));
            rest = &body_start[close..];
        }
    }
    out
}

/// A Lua long-string literal for `s`.
fn lua_long(s: &str) -> String {
    let mut eq = String::new();
    while s.contains(&format!("]{eq}]")) || s.contains(&format!("[{eq}[")) {
        eq.push('=');
    }
    // a leading newline would be dropped by Lua
    format!("[{eq}[\n{s}]{eq}]")
}

// ---- random tables for Utils.stringify ------------------------------------------------------

struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// A random value as (Lua constructor source, Rust value).
fn gen_value(r: &mut Lcg, depth: u32) -> (String, Val) {
    let choice = if depth >= 3 { r.below(4) } else { r.below(6) };
    match choice {
        0 => {
            const PIECES: &[&str] = &[
                "a", "b", "Z", " ", "\"", "\\", "\n", "\r", "\t", "\0", "é", "%", "x1", "{", "]",
            ];
            let n = r.below(6);
            let s: String = (0..n)
                .map(|_| PIECES[r.below(PIECES.len() as u64) as usize])
                .collect();
            let lua: String = s.bytes().map(|b| format!("\\{b}")).collect();
            (format!("\"{lua}\""), Val::from(s.as_str()))
        }
        1 => {
            let x = (r.below(2001) as f64) - 1000.0;
            (tostring_num(x), Val::Num(x))
        }
        2 => {
            let xs = [0.5, 1.0 / 3.0, -2.25, 1e15, 123456789.125, 0.1];
            let x = xs[r.below(xs.len() as u64) as usize];
            (format!("{x:e}"), Val::Num(x))
        }
        3 => {
            let b = r.below(2) == 1;
            (b.to_string(), Val::Bool(b))
        }
        _ => {
            let t = Table::new();
            let mut lua = String::from("{");
            let n = r.below(4);
            for i in 1..=n {
                let (l, v) = gen_value(r, depth + 1);
                lua.push_str(&format!("[{i}]={l},"));
                t.set(Key::I(i as i64), v);
            }
            let m = r.below(4);
            for j in 0..m {
                let (l, v) = gen_value(r, depth + 1);
                match r.below(4) {
                    0 => {
                        let k = n as i64 + 2 + j as i64; // a hole after the array part
                        lua.push_str(&format!("[{k}]={l},"));
                        t.set(Key::I(k), v);
                    }
                    1 if j == 0 => {
                        lua.push_str(&format!("[true]={l},"));
                        t.set(Key::B(true), v);
                    }
                    _ => {
                        let k = format!("k{}_{}", depth, r.below(50) * 10 + j);
                        if t.rawget(Name::from(k.as_str())).is_nil() {
                            lua.push_str(&format!("[\"{k}\"]={l},"));
                            t.set(Name::from(k.as_str()), v);
                        }
                    }
                }
            }
            lua.push('}');
            (lua, Val::Table(t))
        }
    }
}

// ---- dump → Rust ----------------------------------------------------------------------------

fn jnum(j: &J) -> Option<f64> {
    match j {
        J::Number(n) => n.as_f64(),
        J::String(s) if s == "$nan" => Some(f64::NAN),
        J::String(s) if s == "$inf" => Some(f64::INFINITY),
        J::String(s) if s == "$-inf" => Some(f64::NEG_INFINITY),
        _ => None,
    }
}

fn jkey(j: &J) -> Key {
    match j {
        J::String(s) => Key::S(Name::from(s.as_str())),
        J::Number(_) => Key::from(jnum(j).unwrap()),
        J::Object(o) => Key::B(o.get("$b").and_then(J::as_bool).unwrap_or(false)),
        other => panic!("bad key {other}"),
    }
}

fn jval(j: &J) -> Val {
    match j {
        J::Null => Val::Nil,
        J::Bool(b) => Val::Bool(*b),
        J::Number(_) => Val::Num(jnum(j).unwrap()),
        J::String(s) => match jnum(j) {
            Some(x) => Val::Num(x),
            None => Val::from(s.as_str()),
        },
        J::Object(o) => match o.get("$t") {
            Some(J::Array(kvs)) => {
                let t = Table::new();
                for kv in kvs {
                    t.set(jkey(&kv[0]), jval(&kv[1]));
                }
                Val::Table(t)
            }
            // $obj / $truthy / $fn / $deep: a truthy placeholder
            _ => Val::Bool(true),
        },
        J::Array(_) => panic!("unexpected array"),
    }
}

fn jstr(j: &J) -> LStr {
    match jval(j) {
        Val::Str(s) => s,
        Val::Num(n) => LStr::from(tostring_num(n).as_str()),
        other => panic!("expected a string, got {other:?}"),
    }
}

fn list_of_tables(j: &J) -> Shared<Vec<Table>> {
    let v = match jval(j) {
        Val::Table(t) => t
            .ipairs()
            .into_iter()
            .map(|(_, v)| v.table().expect("table"))
            .collect(),
        _ => Vec::new(),
    };
    Shared::new(v)
}

/// A Rust `Item` with the dumped fields of a PoB item.
fn inject(state: &serde_json::Map<String, J>) -> Item {
    let d = pob_gamedata::data();
    let item = Item::default();
    let influence_keys = [
        "shaper",
        "elder",
        "adjudicator",
        "basilisk",
        "crusader",
        "eyrie",
        "cleansing",
        "tangle",
    ];
    for (k, j) in state {
        let v = || jval(j);
        let opt = || jnum(j);
        match k.as_str() {
            "id" => item.id.set(opt()),
            "raw" => item.raw.set(jstr(j)),
            "name" => item.name.set(jstr(j)),
            "title" => item.title.set(Some(jstr(j))),
            "baseName" => item.baseName.set(Some(Name::from(&*jstr(j)))),
            "base" => {
                let key = j.as_str().unwrap_or("");
                item.base.set(d.itemBases.get(key).cloned());
            }
            "type" => item.r#type.set(Name::from(&*jstr(j))),
            "rarity" => item.rarity.set(Name::from(&*jstr(j))),
            "uniqueID" => item.uniqueID.set(Some(jstr(j))),
            "corrupted" => item.corrupted.set(v().truthy()),
            "mirrored" => item.mirrored.set(v().truthy()),
            "fractured" => item.fractured.set(v().truthy()),
            "split" => item.split.set(v().truthy()),
            "synthesised" => item.synthesised.set(v().truthy()),
            k if influence_keys.contains(&k) => item.influences.set(Name::from(k), v()),
            "quality" => item.quality.set(opt()),
            "itemLevel" => item.itemLevel.set(opt()),
            "variant" => item.variant.set(opt()),
            "variantAlt" => item.variantAlt.set(opt()),
            "variantAlt2" => item.variantAlt2.set(opt()),
            "variantAlt3" => item.variantAlt3.set(opt()),
            "variantAlt4" => item.variantAlt4.set(opt()),
            "variantAlt5" => item.variantAlt5.set(opt()),
            "jewelRadiusIndex" => item.jewelRadiusIndex.set(opt()),
            "clusterJewelNodeCount" => item.clusterJewelNodeCount.set(opt()),
            "catalyst" => item.catalyst.set(opt()),
            "catalystQuality" => item.catalystQuality.set(opt()),
            "talismanTier" => item.talismanTier.set(opt()),
            "limit" => item.limit.set(opt()),
            "abyssalSocketCount" => item.abyssalSocketCount.set(opt().unwrap_or(0.0)),
            "sockets" => item.sockets.set(list_of_tables(j)),
            "variantList" => {
                let l: Vec<LStr> = match v() {
                    Val::Table(t) => t
                        .ipairs()
                        .into_iter()
                        .map(|(_, v)| v.str().unwrap())
                        .collect(),
                    _ => Vec::new(),
                };
                item.variantList.set(Some(Shared::new(l)));
            }
            "explicitModLines" => item.explicitModLines.set(list_of_tables(j)),
            "implicitModLines" => item.implicitModLines.set(list_of_tables(j)),
            "enchantModLines" => item.enchantModLines.set(list_of_tables(j)),
            "scourgeModLines" => item.scourgeModLines.set(list_of_tables(j)),
            "crucibleModLines" => item.crucibleModLines.set(list_of_tables(j)),
            "buffModLines" => item.buffModLines.set(list_of_tables(j)),
            "classRequirementModLines" => item.classRequirementModLines.set(list_of_tables(j)),
            "armourData" => item.armourData.set(v().table()),
            "weaponData" => item.weaponData.set(Some(Table::new())),
            "flaskData" => item.flaskData.set(Some(Table::new())),
            "tinctureData" => item.tinctureData.set(Some(Table::new())),
            "jewelData" => item.jewelData.set(Some(Table::new())),
            "jewelRadiusData" => item.jewelRadiusData.set(Some(Table::new())),
            "clusterJewel" => item.clusterJewel.set(Some(Table::new())),
            "jewelRadiusLabel" => item.jewelRadiusLabel.set(Some(jstr(j))),
            "clusterJewelSkill" => item.clusterJewelSkill.set(Some(Name::from(&*jstr(j)))),
            "classRestriction" => item.classRestriction.set(Some(Name::from(&*jstr(j)))),
            "modSource" => item.modSource.set(Some(jstr(j))),
            "requirements" => {
                if let Val::Table(t) = v() {
                    for (k, x) in t.pairs() {
                        item.requirements.set(k, x);
                    }
                }
            }
            "modList" | "slotModList" | "baseModList" | "buffModList" | "grantedSkills" => {}
            "affixes" => {
                let key = j.as_str().unwrap_or("");
                let table = if let Some(cat) = key.strip_prefix("itemMods:") {
                    d.itemMods.get(&Name::from(cat))
                } else if let Some(title) = key.strip_prefix("rareLike:") {
                    d.rareLikeUniques.0.get(title).map(|r| &r.affixes)
                } else {
                    None
                };
                let table = table.unwrap_or_else(|| panic!("unknown affixes {key}"));
                item.extra
                    .set(n("affixes"), Val::Obj(Rc::new(ItemAffixes(table))));
            }
            other => item.extra.set(n(other), v()),
        }
    }
    item
}

fn n(s: &str) -> Name {
    Name::from(s)
}

// ---- comparison bookkeeping -----------------------------------------------------------------

#[derive(Default)]
struct Tally {
    rows: BTreeMap<&'static str, (u64, u64, u64)>, // pass, fail, skipped
    fails: Vec<String>,
}

impl Tally {
    /// `check` for a text result; matching Lua errors are also counted under `<cat> (both error)`.
    fn check_str(
        &mut self,
        cat: &'static str,
        both_err: &'static str,
        rust: &Result<String, String>,
        lua: &J,
        what: impl FnOnce() -> String,
    ) {
        let ok = same_str(rust, lua);
        if ok && rust.is_err() {
            self.rows.entry(both_err).or_default().0 += 1;
        }
        self.check(cat, ok, what);
    }
    fn check(&mut self, cat: &'static str, ok: bool, what: impl FnOnce() -> String) {
        let e = self.rows.entry(cat).or_default();
        if ok {
            e.0 += 1;
        } else {
            e.1 += 1;
            if self.fails.len() < 40 {
                self.fails.push(format!("[{cat}] {}", what()));
            }
        }
    }
    fn skip(&mut self, cat: &'static str) {
        self.rows.entry(cat).or_default().2 += 1;
    }
    fn failed(&self) -> u64 {
        self.rows.values().map(|r| r.1).sum()
    }
}

/// PoB's result (a JSON value or `{"$err": msg}`) as `Err` / `Ok(value)`.
fn lua_res(j: &J) -> Result<&J, String> {
    match j.get("$err") {
        Some(e) => Err(e.as_str().unwrap_or("").to_owned()),
        None => Ok(j),
    }
}

fn same_str(rust: &Result<String, String>, lua: &J) -> bool {
    match (rust, lua_res(lua)) {
        (Ok(a), Ok(b)) => b.as_str() == Some(a.as_str()),
        (Err(_), Err(_)) => true,
        _ => false,
    }
}

fn show(rust: &Result<String, String>, lua: &J) -> String {
    format!("rust={rust:?}\n   lua={lua}")
}

/// A Rust weight against PoB's compact entry (`"E"` = a Lua error).
fn wcmp(rust: &Result<f64, String>, lua: Option<&str>) -> bool {
    match (rust, lua) {
        (Err(_), Some(s)) => s == "E",
        (Ok(w), Some(s)) => s.parse::<f64>().ok() == Some(*w),
        _ => false,
    }
}

/// Mod ids in bytewise order (Lua `table.sort` on strings).
fn sorted_ids<'a>(ids: impl Iterator<Item = &'a Name>) -> Vec<&'a Name> {
    let mut v: Vec<&Name> = ids.collect();
    v.sort_by(|a, b| a.as_str().as_bytes().cmp(b.as_str().as_bytes()));
    v
}

fn jbool(j: &J) -> Option<bool> {
    j.as_bool()
}

fn opt_eq(a: Option<f64>, b: Option<f64>) -> bool {
    match (a, b) {
        (Some(x), Some(y)) => x == y || (x.is_nan() && y.is_nan()),
        (None, None) => true,
        _ => false,
    }
}

const LISTS: [&str; 6] = [
    "enchantModLines",
    "scourgeModLines",
    "classRequirementModLines",
    "implicitModLines",
    "explicitModLines",
    "crucibleModLines",
];

fn mod_lines(item: &Item, list: &str) -> Vec<Table> {
    match list {
        "enchantModLines" => item.enchantModLines.get().snapshot(),
        "scourgeModLines" => item.scourgeModLines.get().snapshot(),
        "classRequirementModLines" => item.classRequirementModLines.get().snapshot(),
        "implicitModLines" => item.implicitModLines.get().snapshot(),
        "explicitModLines" => item.explicitModLines.get().snapshot(),
        _ => item.crucibleModLines.get().snapshot(),
    }
}

/// Lua value equality for the state compare (tables structurally).
fn deep_eq(a: &Val, b: &Val) -> bool {
    match (a, b) {
        (Val::Table(x), Val::Table(y)) => {
            let xs: Vec<(Key, Val)> = x.pairs().collect();
            let ys: Vec<(Key, Val)> = y.pairs().collect();
            xs.len() == ys.len() && xs.iter().all(|(k, v)| deep_eq(v, &y.rawget(*k)))
        }
        _ => a.raw_eq(b),
    }
}

/// The text a mutation produced: PoB re-parses (`BuildAndParseRaw`) and keeps the text in
/// `self.raw`. With IT-A's ParseRaw ported, Rust does the same; while it is a stub, `raw`
/// is untouched and the text is the current `BuildRaw`.
fn mutated_text(item: &Item, raw_before: &LStr) -> Result<String, String> {
    let raw = item.raw.get();
    if !Rc::ptr_eq(&raw, raw_before) && &*raw != &**raw_before {
        Ok(raw.to_string())
    } else {
        catch(|| item.BuildRaw())
    }
}

fn check_item(t: &mut Tally, rec: &J, find_subs: &[String], apply_range_ported: bool) {
    let d = pob_gamedata::data();
    let src = rec["src"].as_str().unwrap_or("?").to_owned();
    let state = rec["state"].as_object().expect("state");
    let item = inject(state);

    // BuildRaw
    let r = catch(|| item.BuildRaw());
    t.check_str(
        "BuildRaw",
        "  (BuildRaw: both raised a Lua error)",
        &r,
        &rec["buildRaw"],
        || format!("{src}: {}", show(&r, &rec["buildRaw"])),
    );

    // GetPrimarySlot
    let r = catch(|| item.GetPrimarySlot().as_str().to_owned());
    t.check_str(
        "GetPrimarySlot",
        "  (GetPrimarySlot: both raised a Lua error)",
        &r,
        &rec["primarySlot"],
        || format!("{src}: {}", show(&r, &rec["primarySlot"])),
    );

    // CheckModLineVariant / GetModLineVariantCount
    for (li, list) in LISTS.iter().enumerate() {
        let lines = mod_lines(&item, list);
        let rows = rec["cmv"][li].as_array().cloned().unwrap_or_default();
        t.check("modLine count", rows.len() == lines.len(), || {
            format!("{src} {list}: {} vs {}", lines.len(), rows.len())
        });
        for (ml, row) in lines.iter().zip(&rows) {
            let a = catch(|| item.CheckModLineVariant(ml));
            let b = catch(|| item.GetModLineVariantCount(ml));
            let ok_a = match (&a, jbool(&row[0])) {
                (Ok(x), Some(y)) => *x == y,
                (Err(_), None) => true,
                _ => false,
            };
            let ok_b = match (&b, row[1].as_f64()) {
                (Ok(x), Some(y)) => *x == y,
                (Err(_), None) => true,
                _ => false,
            };
            t.check("CheckModLineVariant", ok_a, || {
                format!("{src} {list}: rust={a:?} lua={}", row[0])
            });
            t.check("GetModLineVariantCount", ok_b, || {
                format!("{src} {list}: rust={b:?} lua={}", row[1])
            });
        }
    }

    // FindModifierSubstring
    let slot = rec["findSlot"].as_str().unwrap_or("").to_owned();
    for (s, lua) in find_subs
        .iter()
        .zip(rec["find"].as_array().cloned().unwrap_or_default())
    {
        let r = catch(|| item.FindModifierSubstring(s, &slot));
        let ok = match (&r, jbool(&lua)) {
            (Ok(x), Some(y)) => *x == y,
            (Err(_), None) => true,
            _ => false,
        };
        t.check("FindModifierSubstring", ok, || {
            format!("{src} {s:?}@{slot:?}: rust={r:?} lua={lua}")
        });
    }

    // variant groups
    for g in rec["groups"].as_array().cloned().unwrap_or_default() {
        let gid = g[0].as_f64().unwrap();
        for (mode, col) in [(false, 1), (true, 2)] {
            let r = catch(|| item.GetVariantGroupOptions(gid, mode));
            let lua: Result<Vec<f64>, String> = match lua_res(&g[col]) {
                Ok(j) => Ok(match jval(j) {
                    Val::Table(t) => t.ipairs().into_iter().map(|(_, v)| v.num()).collect(),
                    _ => Vec::new(),
                }),
                Err(e) => Err(e),
            };
            let ok = match (&r, &lua) {
                (Ok(a), Ok(b)) => a == b,
                (Err(_), Err(_)) => true,
                _ => false,
            };
            t.check("GetVariantGroupOptions", ok, || {
                format!("{src} group {gid} {mode}: rust={r:?} lua={lua:?}")
            });
        }
        for (vi, e) in g[3]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .enumerate()
        {
            let r = catch(|| item.IsVariantGroupOptionEligible(gid, (vi + 1) as f64));
            t.check(
                "IsVariantGroupOptionEligible",
                r.as_ref().ok() == e.as_bool().as_ref(),
                || format!("{src} {gid}/{}: rust={r:?} lua={e}", vi + 1),
            );
        }
    }

    // spawn weights (compact strings in bytewise mod-id order, "E" = a Lua error)
    if let Some(spawn) = rec.get("spawn") {
        let incl = Table::new();
        incl.set(n("two_hand_weapon"), true);
        let excl = Table::new();
        excl.set(n("one_hand_weapon"), true);
        let mods = &d.itemMods[&Name::from("Item")].0;
        let ids = sorted_ids(mods.keys());
        let lua1: Vec<&str> = spawn.as_str().unwrap_or("").split(',').collect();
        let lua2: Vec<&str> = rec["spawnIncl"].as_str().unwrap_or("").split(',').collect();
        let (mut bad1, mut bad2) = (String::new(), String::new());
        let mut k2 = 0;
        t.check(
            "GetModSpawnWeight mod count",
            lua1.len() == ids.len(),
            || format!("{src}: lua {} rust {}", lua1.len(), ids.len()),
        );
        for (k, id) in ids.iter().enumerate() {
            let m = &mods[*id];
            let w1 = catch(|| item.GetModSpawnWeight(m, None, None, None));
            if !wcmp(&w1, lua1.get(k).copied()) && bad1.is_empty() {
                bad1 = format!("{id}: rust={w1:?} lua={:?}", lua1.get(k));
            }
            let b = id.as_str().as_bytes();
            if (b.len() + usize::from(b[0])) % 5 == 0 {
                let w2 = catch(|| item.GetModSpawnWeight(m, Some(&incl), Some(&excl), None));
                if !wcmp(&w2, lua2.get(k2).copied()) && bad2.is_empty() {
                    bad2 = format!("{id}: rust={w2:?} lua={:?}", lua2.get(k2));
                }
                k2 += 1;
            }
        }
        t.check(
            "GetModSpawnWeight (item x itemMods.Item)",
            bad1.is_empty(),
            || format!("{src} ({} mods): {bad1}", ids.len()),
        );
        t.check(
            "GetModSpawnWeight incl/excl (item x sample)",
            bad2.is_empty() && k2 == lua2.len(),
            || format!("{src} ({k2} mods, lua {}): {bad2}", lua2.len()),
        );
        let necro = &d.necropolisMods.0;
        let nids = sorted_ids(necro.keys());
        let lua3: Vec<&str> = rec["necro"].as_str().unwrap_or("").split(',').collect();
        let mut bad3 = String::new();
        for (k, id) in nids.iter().enumerate() {
            let w = catch(|| item.GetNecropolisModSpawnWeight(&necro[*id]));
            if !wcmp(&w, lua3.get(k).copied()) && bad3.is_empty() {
                bad3 = format!("{id}: rust={w:?} lua={:?}", lua3.get(k));
            }
        }
        t.check(
            "GetNecropolisModSpawnWeight (item x all)",
            bad3.is_empty() && nids.len() == lua3.len(),
            || format!("{src}: {bad3}"),
        );
    }

    // the mutation sequence (uniques and corpus)
    if let Some(q) = rec.get("quality") {
        let r = catch(|| item.NormaliseQuality());
        let ok = r.is_ok() && q.as_str() != Some("$err") && opt_eq(item.quality.get(), jnum(q));
        t.check("NormaliseQuality", ok, || {
            format!("{src}: rust={:?} lua={q}", item.quality.get())
        });

        let nvs = &rec["nvs"];
        let r = catch(|| item.NormaliseVariantSelections());
        let ok = match (r, nvs.as_str()) {
            (Err(_), Some("$err")) => true,
            (Ok(()), None) => {
                let sv = item.extra.get(n("selectedVersion"));
                let sel = item.extra.get(n("variantGroupSelections"));
                deep_eq(&sv, &jval(&nvs["selectedVersion"]))
                    && deep_eq(&sel, &jval(&nvs["variantGroupSelections"]))
            }
            _ => false,
        };
        t.check("NormaliseVariantSelections", ok, || {
            format!(
                "{src}: rust sv={:?} sel={:?} lua={nvs}",
                item.extra.get(n("selectedVersion")),
                item.extra.get(n("variantGroupSelections"))
            )
        });

        let variants = rec["variants"].as_array().cloned().unwrap_or_default();
        if !variants.is_empty() {
            let saved = item.variant.get();
            for (vi, row) in variants.iter().enumerate() {
                item.variant.set(Some((vi + 1) as f64));
                // the oracle keeps the text of the first 8 variants and the last
                if !row[0].is_null() {
                    let r = catch(|| item.BuildRaw());
                    t.check_str(
                        "BuildRaw per variant",
                        "  (BuildRaw per variant: both raised a Lua error)",
                        &r,
                        &row[0],
                        || format!("{src} variant {}: {}", vi + 1, show(&r, &row[0])),
                    );
                }
                let lines = item.explicitModLines.get().snapshot();
                let lua = row[1].as_array().cloned().unwrap_or_default();
                let rust: Vec<Option<bool>> = lines
                    .iter()
                    .map(|ml| catch(|| item.CheckModLineVariant(ml)).ok())
                    .collect();
                let ok = rust.len() == lua.len()
                    && rust.iter().zip(&lua).all(|(a, b)| *a == b.as_bool());
                t.check("CheckModLineVariant per variant", ok, || {
                    format!("{src} variant {}: rust={rust:?} lua={lua:?}", vi + 1)
                });
            }
            item.variant.set(saved);
        }

        if let Some(m) = rec.get("mutate") {
            let args = &m["args"];
            let raw_before = item.raw.get();
            let r = catch(|| {
                item.MutateMod(
                    Name::from(args[0].as_str().unwrap()),
                    Name::from(args[1].as_str().unwrap()),
                    args[2].as_bool().unwrap(),
                )
            });
            let text = r.and_then(|()| mutated_text(&item, &raw_before));
            t.check_str(
                "MutateMod",
                "  (MutateMod: both raised a Lua error)",
                &text,
                &m["raw"],
                || format!("{src} {args}: {}", show(&text, &m["raw"])),
            );
        }
    }

    // Craft (rares)
    if let Some(c) = rec.get("craft") {
        if apply_range_ported {
            let raw_before = item.raw.get();
            let r = catch(|| item.Craft()).and_then(|()| mutated_text(&item, &raw_before));
            t.check("Craft", same_str(&r, c), || {
                format!("{src}: {}", show(&r, c))
            });
        } else {
            t.skip("Craft");
        }
    }
}

#[test]
fn item_core_vs_pob() {
    let Some(src) = find_pob_src() else {
        return skip("no PoB checkout (.PathOfBuilding/src)");
    };
    if !init_data() {
        return skip("data/dump not found");
    }
    quiet_lua_errors();
    let d = pob_gamedata::data();

    // inputs
    let corpus = corpus_items();
    let mut r = Lcg(0x1b_2024);
    let mut str_vals: Vec<Val> = Vec::new();
    let mut prologue = String::from("__itb_corpus = {\n");
    for (s, raw) in &corpus {
        prologue.push_str(&format!(
            "{{ src = {}, raw = {} }},\n",
            lua_long(s),
            lua_long(raw)
        ));
    }
    prologue.push_str("}\n__itb_stringify = {\n");
    for _ in 0..300 {
        // tables only (stringify's useful input), forced by depth 0 retries
        let (lua, v) = loop {
            let (l, v) = gen_value(&mut r, 0);
            if matches!(v, Val::Table(_)) {
                break (l, v);
            }
        };
        prologue.push_str(&lua);
        prologue.push_str(",\n");
        str_vals.push(v);
    }
    prologue.push_str("}\n");
    let tmp = std::env::temp_dir().join(format!("itb_save_{}.lua", std::process::id()));
    prologue.push_str(&format!(
        "__itb_tmp = {}\n",
        lua_long(&tmp.to_string_lossy())
    ));
    if let Ok(l) = std::env::var("IT_B_LIMIT") {
        prologue.push_str(&format!("__itb_limit = {l}\n"));
    }
    let script = include_str!("../corpus/IT-B/item_oracle.lua");

    let t0 = std::time::Instant::now();
    let mut pob = JitPob::boot_oracle(&src, None).unwrap_or_else(|e| panic!("boot: {e}"));
    let out = pob
        .run(&format!("{prologue}\n{script}"))
        .unwrap_or_else(|e| panic!("oracle script: {e}"));
    let Some(Ret::Str(json)) = out.first() else {
        panic!("oracle returned {out:?}")
    };
    eprintln!(
        "oracle: {} corpus items, {:.1} MB JSON in {:?}",
        corpus.len(),
        json.len() as f64 / 1e6,
        t0.elapsed()
    );
    if let Some(p) = std::env::var_os("IT_B_DUMP") {
        // debugging aid: keep the oracle document
        std::fs::write(p, json).expect("IT_B_DUMP");
    }
    let doc: J = serde_json::from_str(json).expect("oracle JSON");
    drop(pob);

    let mut t = Tally::default();
    let find_subs: Vec<String> = match jval(&doc["findSubstrings"]) {
        Val::Table(tb) => tb
            .ipairs()
            .into_iter()
            .map(|(_, v)| v.str().unwrap().to_string())
            .collect(),
        _ => panic!("findSubstrings"),
    };
    let apply_range_ported =
        !item_tools::applyRange("+10 to maximum Life", &Val::Num(0.5), None, None).is_empty();

    // items
    let items = doc["items"].as_array().expect("items");
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    for rec in items {
        *kinds
            .entry(rec["kind"].as_str().unwrap_or("?").to_owned())
            .or_default() += 1;
        check_item(&mut t, rec, &find_subs, apply_range_ported);
    }

    // locals
    for row in doc["normalise"].as_array().unwrap() {
        let (i, o) = (row[0].as_str().unwrap(), row[1].as_str().unwrap());
        let r = item_mod::normaliseModLine(i);
        t.check("normaliseModLine", r == o, || {
            format!("{i:?}: rust={r:?} lua={o:?}")
        });
    }
    for row in doc["spec"].as_array().unwrap() {
        let s = row[0].as_str().unwrap();
        let r = item_mod::specToNumber(s);
        t.check("specToNumber", opt_eq(r, jnum(&row[1])), || {
            format!("{s:?}: rust={r:?} lua={}", row[1])
        });
    }
    let tag_sets: Vec<Vec<String>> = match jval(&doc["tagSets"]) {
        Val::Table(tb) => tb
            .ipairs()
            .into_iter()
            .map(|(_, v)| match v {
                Val::Table(x) => x
                    .ipairs()
                    .into_iter()
                    .map(|(_, s)| s.str().unwrap().to_string())
                    .collect(),
                _ => Vec::new(),
            })
            .collect(),
        _ => panic!("tagSets"),
    };
    // tagSets[1] is `{ }`, which the encoder writes as an empty `$t`
    for row in doc["catalyst"].as_array().unwrap() {
        let ti = row[0].as_f64().unwrap() as usize;
        let m = Table::new();
        let tags = Table::new();
        for s in tag_sets.get(ti - 1).cloned().unwrap_or_default() {
            tags.push(s.as_str());
        }
        m.set(n("modTags"), tags);
        for (k, col) in [("prefix", 1), ("suffix", 2), ("unscalable", 3)] {
            if row[col].as_bool() == Some(true) {
                m.set(n(k), true);
            }
        }
        let c = row[4].as_f64(); // the string id "3" is not a number: None
        let q = row[5].as_f64();
        let r = item_mod::getCatalystScalar(c, &m, q);
        let l = row[6].as_f64().unwrap();
        t.check("getCatalystScalar", r == l, || format!("{row}: rust={r}"));
    }
    for row in doc["sort"].as_array().unwrap() {
        let spec = row[0].as_array().unwrap();
        let list: Vec<Table> = spec
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let m = Table::new();
                m.set(n("id"), (i + 1) as f64);
                for (k, col) in [("crafted", 0), ("custom", 1), ("fractured", 2)] {
                    if s[col].as_bool() == Some(true) {
                        m.set(n(k), true);
                    }
                }
                if let Some(o) = s[3].as_f64() {
                    m.set(n("order"), o);
                }
                m
            })
            .collect();
        let sh = Shared::new(list);
        let r = catch(|| item_mod::sortCraftedModLines(&sh));
        let ids: Vec<f64> = sh.snapshot().iter().map(|m| m.num(n("id"))).collect();
        let ok = match (&r, row[1].as_array()) {
            (Ok(()), Some(l)) => {
                ids.iter().zip(l).all(|(a, b)| Some(*a) == b.as_f64()) && ids.len() == l.len()
            }
            (Err(_), None) => true,
            _ => false,
        };
        t.check("sortCraftedModLines", ok, || {
            format!("{spec:?}: rust={r:?} {ids:?} lua={}", row[1])
        });
    }

    // Utils
    for (v, row) in str_vals.iter().zip(doc["stringify"].as_array().unwrap()) {
        let a = catch(|| item_mod::stringify(v, false, None));
        let b = catch(|| item_mod::stringify(v, true, Some(3.0)));
        let c = catch(|| item_mod::stringifyInline(v));
        t.check("stringify", same_str(&a, &row[0]), || show(&a, &row[0]));
        t.check("stringify(allowNewlines, 3)", same_str(&b, &row[1]), || {
            show(&b, &row[1])
        });
        t.check("stringifyInline", same_str(&c, &row[2]), || {
            show(&c, &row[2])
        });
    }
    if let Some(lua) = doc["save"].as_str().filter(|s| *s != "$skip") {
        let path = std::env::temp_dir().join(format!("itb_save_rust_{}.lua", std::process::id()));
        let ok = item_mod::saveTableToFile(
            &path.to_string_lossy(),
            &str_vals[0],
            Some("Line one\nline two"),
            false,
        );
        let rust = std::fs::read_to_string(&path).unwrap_or_default();
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&tmp);
        t.check("saveTableToFile", ok && rust == lua, || {
            format!("rust={rust:?}\n   lua={lua:?}")
        });
    }
    if let Some(lua) = doc["saveErr"].as_str().filter(|s| *s != "$skip") {
        // Lua's message carries the `Modules/Utils.lua:93: ` position prefix
        let missing = format!("{}.missing/x.lua", tmp.to_string_lossy());
        let r =
            catch(|| item_mod::saveTableToFile(&missing, &Val::Table(Table::new()), None, false));
        let ok = matches!(&r, Err(e) if !e.is_empty() && lua.ends_with(e.as_str()));
        t.check("saveTableToFile (open error)", ok, || {
            format!("rust={r:?}\n   lua={lua:?}")
        });
    }
    let delve = d.itemMods[&Name::from("Item")]
        .0
        .values()
        .filter(|m| Item::default().CheckIfModIsDelve(m))
        .count();
    t.check(
        "CheckIfModIsDelve (count over itemMods.Item)",
        Some(delve as u64) == doc["delve"].as_u64(),
        || format!("rust={delve} lua={}", doc["delve"]),
    );

    eprintln!("IT-B: items by kind {kinds:?}");
    eprintln!(
        "{:<48} {:>8} {:>6} {:>8}",
        "check", "pass", "fail", "skipped"
    );
    for (k, (p, f, s)) in &t.rows {
        eprintln!("{k:<48} {p:>8} {f:>6} {s:>8}");
    }
    for f in &t.fails {
        eprintln!("FAIL {f}");
    }
    assert_eq!(
        t.failed(),
        0,
        "{} IT-B mismatches (first ones above)",
        t.failed()
    );
    assert!(
        kinds.get("unique").copied().unwrap_or(0) > 0,
        "no uniques checked"
    );
    // The synthetic crafted rares (item_oracle.lua `craftSynth`) must reach Craft in PoB with a
    // list `range` (Item.lua:803-809), so that the Craft comparison covers applyRange's table
    // path (ItemTools.lua:99/104) once IT-C ports it.
    let synth: Vec<&J> = items.iter().filter(|r| r["kind"] == "craftsynth").collect();
    let list_range = |rec: &J| match jval(&rec["state"]["prefixes"]) {
        Val::Table(p) => p.ipairs().into_iter().any(|(_, a)| match a {
            Val::Table(a) => matches!(a.get(n("range")), Val::Table(_)),
            _ => false,
        }),
        _ => false,
    };
    assert!(
        synth.len() == 2
            && synth
                .iter()
                .all(|r| r["craft"].is_string() && list_range(r)),
        "synthetic crafted rares: PoB did not parse/craft them with list ranges: {synth:?}"
    );
}

/// Rust-only checks of the plumbing the oracle cannot reach.
#[test]
fn item_core_units() {
    quiet_lua_errors();
    // influenceItemMap / lineFlags / catalyst tables
    assert_eq!(
        item_mod::influenceItemMap("Searing Exarch Item"),
        Some("cleansing")
    );
    assert_eq!(item_mod::influenceItemMap("Shaper Items"), None);
    assert!(item_mod::is_lineFlag("unveiled") && !item_mod::is_lineFlag("variant"));
    assert_eq!(item_mod::catalystList_get(12.0), Some("Unstable"));
    assert_eq!(item_mod::catalystList_get(0.0), None);
    // ResetInfluence sets every key to false
    let item = Item::default();
    item.influences.set(n("shaper"), true);
    item.ResetInfluence();
    assert!(!item.influences.truthy(n("shaper")));
    assert_eq!(item.influences.pairs().count(), 8);
    // BuildRaw with a title but no baseName: t_insert(rawLines, nil) is a no-op
    item.rarity.set(n("RARE"));
    item.title.set(Some(LStr::from("Foo")));
    assert_eq!(item.BuildRaw(), "Rarity: RARE\nFoo\nImplicits: 0");
    // no title and no baseName: concatenating nil raises
    item.title.set(None);
    assert!(catch(|| item.BuildRaw()).is_err());
    // Utils indentFor
    assert_eq!(item_mod::indentFor(&Val::Num(0.0)), "");
    assert_eq!(item_mod::indentFor(&Val::Num(3.0)), "\t\t\t");
}
