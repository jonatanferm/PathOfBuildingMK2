//! IT-C acceptance (calc doc §11.1 row IT-C, §9.4 L2 "Item: … modList, slotModList, weaponData,
//! armourData, flaskData, tinctureData, jewelData"): `calcLocal`, `Item:BuildModListForSlotNum`,
//! `getRangedModList`, `Item:BuildModList`, `Item:CanHaveMod` (Classes/Item.lua 2192-2736) and
//! `itemLib` (Modules/ItemTools.lua) against PoB under LuaJIT (interpreter, `jit.off`).
//!
//! `corpus/IT-C/item_mods_oracle.lua` parses, in PoB, every unique (all types, every variant),
//! every rare, every item of the build corpus (`.parity/builds`, as ItemsTab:Load does) and the
//! synthetic items below, and records:
//! * for each ParseRaw call, BuildModList's outputs: `modList`, `slotModList[1..3]`,
//!   `baseModList`, `buffModList`, `weaponData`, `armourData`, `flaskData`, `tinctureData`,
//!   `jewelData`, the item fields BuildModList writes (requirements, grantedSkills, sockets,
//!   socket counts, classRestriction, craftedQuality, quality, …), every mod line's `modList` /
//!   `showSlider` afterwards, `itemLib.formatModLine` of every mod line (both modes),
//!   `itemLib.isModLineScalable` of every ranged line, and the `Item:CanHaveMod` results;
//! * every `itemLib.applyRange` call PoB made (unique by arguments), plus a sweep over every
//!   ranged line of `data.uniques` with ranges 0/0.37/0.5/0.84/1, list ranges and value scalars;
//! * every `modLib.parseMod` call (unique by arguments).
//!
//! The Rust side runs the real ParseRaw (IT-A), applyRange/BuildModList/CanHaveMod (IT-C) with only
//! `modLib.parseMod` replayed from PoB's results (`item_parse::replay` and `item_tools::replay`):
//! the ModParser lists are other units' and not all ported, and IT-C is checked in isolation.
//! Checks (every one must pass):
//! * applyRange: the result (or that both raise) for every recorded call;
//! * per parse: Lua error status, every output above (mods compared field by field, data tables
//!   key by key), and each CanHaveMod result PoB computed (the Rust CanHaveMod runs in its place
//!   and is compared call by call).
//!
//! Fails without PoB / the data dump (`pob_testpaths::require`; `POB_ALLOW_SKIP=1` skips). From a
//! worktree point `POB_SRC`, `POB_PARITY_DIR` and `POB_DUMP_DIR` at the main checkout's
//! `.PathOfBuilding/src`, `.parity` and `data/dump`. `IT_C_LIMIT=<n>` limits the uniques;
//! `IT_C_KEEP=<file>` keeps the oracle's JSONL; `IT_C_ONLY=<substring>` checks only the items
//! whose source contains it (the completeness asserts are skipped).
//! PoB's output is cached in the shared probe cache (HYG3: keyed by the PoB source fingerprint and
//! oracle runtime, the oracle input and the script; `POB_QUERY_CACHE=refresh|off`), so a warm run
//! boots no PoB; a cold run holds the machine-wide oracle lock while PoB runs.

#![allow(clippy::pedantic, non_snake_case)]

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::io::BufRead;
use std::path::PathBuf;
use std::rc::Rc;

use pob_calc::classes::item_parse::{self, replay};
use pob_calc::classes::item_tools::{self as itemLib, replay as tools_replay};
use pob_calc::classes::{Item, ModList};
use pob_calc::prelude::{LuaError, Mod, Name, Value};
use pob_calc::{Key, Table, Val};
use pob_lua::{lua_format, FmtArg};
use pob_oracle::headless::{Engine, Ret};
use pob_oracle::jit::JitPob;
use pob_oracle::oracle::{find_parity_dir, find_pob_src, find_up};
use pob_oracle::runner::{OracleLock, QueryCache};
use serde_json::Value as J;

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
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => Ok(r),
        Err(payload) => match payload.downcast::<LuaError>() {
            Ok(e) => Err(e.0),
            Err(other) => std::panic::resume_unwind(other),
        },
    }
}

// ---- corpus (as tests/IT-A.rs) ------------------------------------------------------------------

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
        match &tail[1..j] {
            "lt" => out.push('<'),
            "gt" => out.push('>'),
            "amp" => out.push('&'),
            "quot" => out.push('"'),
            "apos" => out.push('\''),
            e if e.starts_with("#x") => {
                if let Some(c) = u32::from_str_radix(&e[2..], 16).ok().and_then(char::from_u32) {
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

struct CorpusItem {
    src: String,
    raw: String,
    attrs: BTreeMap<&'static str, f64>,
}

const SETUP_KEYS: [&str; 7] = [
    "id",
    "variant",
    "variantAlt",
    "variantAlt2",
    "variantAlt3",
    "variantAlt4",
    "variantAlt5",
];

/// Every `<Item>` of every build in `.parity/builds` (ItemsTab.lua:1224-1243).
fn corpus_items() -> Vec<CorpusItem> {
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
            let tag = &after[..open_end];
            let body_start = &after[open_end + 1..];
            let Some(close) = body_start.find("</Item>") else {
                break;
            };
            let body = &body_start[..close];
            let mut text = String::new();
            let mut b = body;
            while let Some(lt) = b.find('<') {
                text.push_str(&b[..lt]);
                match b[lt..].find('>') {
                    Some(gt) => b = &b[lt + gt + 1..],
                    None => b = "",
                }
            }
            text.push_str(b);
            let mut attrs = BTreeMap::new();
            for k in SETUP_KEYS {
                let pat = format!(" {k}=\"");
                if let Some(p) = tag.find(&pat) {
                    let v = &tag[p + pat.len()..];
                    let v = &v[..v.find('"').unwrap_or(v.len())];
                    if let Some(x) = pob_lua::tonumber(v) {
                        attrs.insert(k, x);
                    }
                }
            }
            n += 1;
            out.push(CorpusItem {
                src: format!("{stem}#{n}"),
                raw: xml_unescape(&text),
                attrs,
            });
            rest = &body_start[close..];
        }
    }
    out
}

fn lua_long(s: &str) -> String {
    let mut eq = String::new();
    while s.contains(&format!("]{eq}]")) || s.contains(&format!("[{eq}[")) {
        eq.push('=');
    }
    format!("[{eq}[\n{s}]{eq}]")
}

/// Synthetic pasted items: local weapon/armour/flask/tincture/jewel paths, sockets, requirements.
const SYNTH: &[&str] = &[
    // weapon: local physical/elemental/speed/crit/accuracy/leech/on-hit/bleed, sockets with links
    "Rarity: Rare\nGale Bite\nVaal Axe\n--------\nQuality: +20%\nSockets: R-R-G B-W\n--------\nItem Level: 84\n--------\n+30 to Accuracy Rating\n120% increased Physical Damage\nAdds 10 to 20 Fire Damage\nAdds 1 to 60 Lightning Damage\n15% increased Attack Speed\n+1.5% to Critical Strike Chance\n30% increased Critical Strike Chance\n0.5% of Physical Attack Damage Leeched as Life\n+5 Life gained for each Enemy hit by Attacks\n10% chance to cause Bleeding on Hit\n+10% to Global Critical Strike Multiplier\n",
    // weapon with ranges, a one-handed base, alternate quality-like lines
    "Rarity: Rare\nTest Blade\nJewelled Foil\n--------\nQuality: +12%\n--------\nItem Level: 80\n--------\n{range:0.5}(60-80)% increased Physical Damage\n{range:0.3}Adds (5-8) to (12-15) Cold Damage\n{range:1}(5-10)% increased Attack Speed\n+20% chance to Impale Enemies on Hit with Attacks\n",
    // armour with a recorded Armour value (percentiles), local defences, movement penalty, block
    "Rarity: Rare\nDoom Shell\nAstral Plate\n--------\nQuality: +20%\nArmour: 1200\n--------\nItem Level: 86\n--------\n+12% to all Elemental Resistances (implicit)\n--------\n+100 to Armour\n80% increased Armour\n+90 to maximum Life\n",
    "Rarity: Rare\nHybrid Ward\nRunic Crown\n--------\nQuality: +10%\nArmour: 300\nWard: 120\n--------\nItem Level: 86\n--------\n+30 to Ward\n40% increased Armour and Ward\n10% increased Defences\n",
    "Rarity: Rare\nShield Thing\nPinnacle Tower Shield\n--------\nQuality: +20%\n--------\nItem Level: 86\n--------\n+5% Chance to Block\n50% increased Armour\nSockets: R-R-R\n",
    "Rarity: Rare\nEvasive Things\nSlink Boots\n--------\nQuality: +20%\nEvasion Rating: 400\nEnergy Shield: 60\n--------\nItem Level: 86\n--------\n+40 to Evasion Rating\n+20 to maximum Energy Shield\n60% increased Evasion and Energy Shield\n30% increased Movement Speed\n",
    // flasks: life, mana, hybrid, utility
    "Rarity: Magic\nSeething Divine Life Flask of Staunching\n--------\nQuality: +20%\n--------\n66% of Recovery applied Instantly\nGrants Immunity to Bleeding for 4 seconds if used while Bleeding\n",
    "Rarity: Magic\nCeaseless Eternal Mana Flask of Heat\n--------\nQuality: +15%\n--------\n20% increased Charge Recovery\n50% increased Recovery rate\n",
    "Rarity: Magic\nExperimenter's Silver Flask of the Rainbow\n--------\nQuality: +20%\n--------\n30% increased Duration\n+10 to Maximum Charges\n",
    "Rarity: Normal\nDivine Life Flask\n",
    // tincture
    "Rarity: Magic\nPrismatic Tincture\n--------\nQuality: +10%\n--------\n20% reduced Mana Burn rate\n15% increased effect\n",
    // jewels: plain, radius, cluster (skill, node count, notable)
    "Rarity: Rare\nGloom Eye\nCobalt Jewel\n--------\nLimited to: 2\nRadius: Medium\n--------\nItem Level: 84\n--------\n+12% to Global Critical Strike Multiplier\n",
    "Rarity: Rare\nKraken Spark\nLarge Cluster Jewel\n--------\nCluster Jewel Skill: affliction_fire_damage\nCluster Jewel Node Count: 12\n--------\nItem Level: 84\n--------\nAdds 8 Passive Skills (enchant)\n2 Added Passive Skills are Jewel Sockets (enchant)\nAdded Small Passive Skills grant: 12% increased Fire Damage (enchant)\n--------\n1 Added Passive Skill is Burning Bright\n",
    "Rarity: Rare\nCurse Small\nSmall Cluster Jewel\n--------\nItem Level: 84\n--------\nAdds 3 Passive Skills (enchant)\nAdded Small Passive Skills grant: 2% increased Effect of your Curses (enchant)\n--------\n+5 to Strength\n",
    // ring (three slot lists) with a catalyst
    "Rarity: Rare\nStorm Band\nAmethyst Ring\n--------\nQuality (Life and Mana Modifiers): +20% (augmented)\n--------\nItem Level: 84\n--------\n+17% to Chaos Resistance (implicit)\n--------\n+52 to maximum Life\n+40 to maximum Mana\nAdds 5 to 10 Physical Damage to Attacks\n",
    // requirements, class restriction, variants, reminder text, crafted/custom lines
    "Rarity: Rare\nThing\nSynthesised Ruby Ring\n--------\nRequires Class Witch\n--------\nItem Level: 84\n--------\n+10% to Fire Resistance\n--------\nShaper Item\nElder Item\nSynthesised Item\n",
    "Rarity: RARE\nTest Crafted\nSapphire Ring\nCrafted: true\nPrefix: {range:0.5}IncreasedLife7\nPrefix: {fractured}{range:0.2,0.8}IncreasedLife6\nSuffix: None\nLevelReq: 60\nCatalyst: Fertile\nCatalystQuality: 15\nImplicits: 1\n{crafted}+20% to Cold Resistance\n{range:0.3}{tags:resource,life}+(20-30) to maximum Life\n+(10-20)% increased Rarity of Items found\n",
    "Rarity: UNIQUE\nTest Magnitude\nGold Amulet\n--------\nItem Level: 80\n--------\n{tags:resource,life}{range:0.5}+(20-30) to maximum Life\n{tags:attack,speed}{range:1}(5-10)% increased Attack Speed\n{range:0.25}(0.4-0.8)% of Physical Attack Damage Leeched as Life\n{range:0.7}+(0-1) to Level of all Minion Skill Gems\n50% increased Explicit Life Modifier magnitudes\nImplicit Modifier magnitudes are doubled\n",
    "Rarity: UNIQUE\nTest Corrupted\nLeather Belt\n--------\n{corruptedRange:1.22}{range:0.5}+(40-60) to maximum Life\n{corruptedRange:0.8}{range:0.1}(10-20)% increased Damage\n{range:0.6}(1.0-2.0)% of Life Regenerated per second\nCorrupted\n",
    // sockets: abyssal sockets on a belt, a forced socket count, no sockets
    "Rarity: Rare\nAbyss Belt\nStygian Vise\n--------\nItem Level: 84\n--------\nHas 1 Abyssal Socket (implicit)\n--------\n+40 to maximum Life\n",
    "Rarity: Rare\nThree Socket Helm\nHubris Circlet\n--------\nSockets: B-B G-R\n--------\nItem Level: 86\n--------\nHas 2 Sockets\n+60 to maximum Energy Shield\n",
    "Rarity: Rare\nNo Sockets\nHubris Circlet\n--------\nSockets: B-B\n--------\nItem Level: 86\n--------\nHas no Sockets\nAll Sockets are White\n",
    // granted skill, attribute requirements
    "Rarity: Rare\nSkill Ring\nGold Ring\n--------\nItem Level: 84\n--------\nGrants Level 20 Summon Bestial Rhoa Skill\n+20 to Strength\n",
    "Rarity: Rare\nHeavy Thing\nGlorious Plate\n--------\nItem Level: 84\n--------\n20% reduced Attribute Requirements\n+50 to Strength\n",
    "Rarity: Rare\nWeightless\nGlorious Plate\n--------\nItem Level: 84\n--------\nItems and Gems have no Attribute Requirements\n",
];

// ---- oracle JSON helpers ---------------------------------------------------------------------

fn bytes_str(s: &str) -> String {
    if s.chars().all(|c| (c as u32) < 256) {
        let b: Vec<u8> = s.chars().map(|c| c as u8).collect();
        String::from_utf8_lossy(&b).into_owned()
    } else {
        s.to_owned()
    }
}

fn fix_json(j: &mut J) {
    match j {
        J::String(s) => {
            if s.bytes().any(|b| b >= 0x80) {
                *s = bytes_str(s);
            }
        }
        J::Array(a) => a.iter_mut().for_each(fix_json),
        J::Object(o) => {
            let entries: Vec<(String, J)> = std::mem::take(o).into_iter().collect();
            for (k, mut v) in entries {
                fix_json(&mut v);
                let k = if k.bytes().any(|b| b >= 0x80) { bytes_str(&k) } else { k };
                o.insert(k, v);
            }
        }
        _ => {}
    }
}

fn jnum(j: &J) -> Option<f64> {
    match j {
        J::Number(n) => n.as_f64(),
        J::String(s) if s == "$nan" => Some(f64::NAN),
        J::String(s) if s == "$inf" => Some(f64::INFINITY),
        J::String(s) if s == "$-inf" => Some(f64::NEG_INFINITY),
        _ => None,
    }
}

fn g17(x: f64) -> String {
    lua_format("%.17g", &[FmtArg::Num(x)])
}

fn decode_mods(j: &J) -> Option<Vec<Mod>> {
    match j {
        J::Null => None,
        J::Array(a) => Some(
            a.iter()
                .map(|m| {
                    // `sourceSlot` (Item.lua:2247) is not a key the strict dump decoder knows; the
                    // port keeps it in `Mod.extra` (pob_gamedata::modifier docs)
                    let mut m = m.clone();
                    let slot = m.as_object_mut().and_then(|o| o.remove("sourceSlot"));
                    let mut r = serde_json::from_value::<Mod>(m.clone())
                        .unwrap_or_else(|e| panic!("mod {m}: {e}"));
                    if let Some(J::String(s)) = slot {
                        r.extra
                            .get_or_insert_with(Default::default)
                            .set(Name::from("sourceSlot"), Value::Str(s.as_str().into()));
                    }
                    r
                })
                .collect(),
        ),
        other => panic!("bad mod list {other}"),
    }
}

/// A mod with its `extra` keys sorted (the oracle writes keys sorted; the port appends).
fn norm(m: &Mod) -> Mod {
    let mut m = m.clone();
    if let Some(ex) = m.extra.as_mut() {
        ex.fields.sort_by(|a, b| a.0.as_str().cmp(b.0.as_str()));
    }
    m
}

fn mods_eq(rust: &[Mod], lua: &[Mod], path: &str, diffs: &mut Vec<String>) {
    if rust.len() != lua.len() {
        diffs.push(format!("{path}: {} mods in rust, {} in lua", rust.len(), lua.len()));
        let n = rust.len().max(lua.len());
        for i in 0..n {
            let (a, b) = (rust.get(i).map(norm), lua.get(i).map(norm));
            if a != b {
                diffs.push(format!("{path}[{}]: first difference\n      rust={a:?}\n      lua ={b:?}", i + 1));
                break;
            }
        }
        return;
    }
    for (i, (a, b)) in rust.iter().zip(lua).enumerate() {
        let (a, b) = (norm(a), norm(b));
        if a != b {
            diffs.push(format!("{path}[{}]:\n      rust={a:?}\n      lua ={b:?}", i + 1));
        }
    }
}

// ---- comparable values -----------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum CK {
    B(bool),
    I(i64),
    F(u64),
    S(String),
}

#[derive(Debug, Clone)]
enum C {
    Nil,
    B(bool),
    N(f64),
    S(String),
    T(BTreeMap<CK, C>),
    Mods(Vec<Mod>),
    Other(String),
}

fn ck_from_num(x: f64) -> CK {
    if x == x.trunc() && x.abs() < 9.0e15 {
        CK::I(x as i64)
    } else {
        CK::F(x.to_bits())
    }
}

fn ck_show(k: &CK) -> String {
    match k {
        CK::B(b) => format!("[{b}]"),
        CK::I(i) => format!("[{i}]"),
        CK::F(f) => format!("[{}]", f64::from_bits(*f)),
        CK::S(s) => s.clone(),
    }
}

fn cshow(c: &C) -> String {
    let s = match c {
        C::Nil => "nil".to_owned(),
        C::B(b) => b.to_string(),
        C::N(n) => g17(*n),
        C::S(s) => format!("{s:?}"),
        C::T(t) => format!("table({} keys)", t.len()),
        C::Mods(m) => format!("{} mods", m.len()),
        C::Other(r) => r.clone(),
    };
    if s.len() > 300 {
        format!("{}…", &s[..s.char_indices().nth(300).map_or(s.len(), |(i, _)| i)])
    } else {
        s
    }
}

fn ceq(a: &C, b: &C, path: &str, diffs: &mut Vec<String>) {
    let ok = match (a, b) {
        (C::Nil, C::Nil) => true,
        (C::B(x), C::B(y)) => x == y,
        // ±0 is a host artefact (CCR-0002); == already treats them as equal
        (C::N(x), C::N(y)) => x == y || (x.is_nan() && y.is_nan()),
        (C::S(x), C::S(y)) => x == y,
        (C::Other(x), C::Other(y)) => x == y,
        (C::Mods(x), C::Mods(y)) => {
            mods_eq(x, y, path, diffs);
            true
        }
        (C::T(x), C::T(y)) => {
            for k in x.keys().chain(y.keys().filter(|k| !x.contains_key(k))) {
                let p = format!("{path}.{}", ck_show(k));
                ceq(x.get(k).unwrap_or(&C::Nil), y.get(k).unwrap_or(&C::Nil), &p, diffs);
            }
            true
        }
        _ => false,
    };
    if !ok {
        diffs.push(format!("{path}: rust={} lua={}", cshow(a), cshow(b)));
    }
}

/// Oracle JSON (`$t` tables, `$mods`) → C.
fn c_from_lua(j: &J) -> C {
    match j {
        J::Null => C::Nil,
        J::Bool(b) => C::B(*b),
        J::Number(_) => C::N(jnum(j).unwrap()),
        J::String(s) => match jnum(j) {
            Some(x) => C::N(x),
            None => C::S(s.clone()),
        },
        J::Object(o) => {
            if let Some(J::Array(kvs)) = o.get("$t") {
                let mut m = BTreeMap::new();
                for kv in kvs {
                    let k = match &kv[0] {
                        J::String(s) => CK::S(s.clone()),
                        J::Number(_) => ck_from_num(jnum(&kv[0]).unwrap()),
                        J::Object(b) => CK::B(b.get("$b").and_then(J::as_bool).unwrap_or(false)),
                        other => panic!("bad key {other}"),
                    };
                    m.insert(k, c_from_lua(&kv[1]));
                }
                C::T(m)
            } else if let Some(ms) = o.get("$mods") {
                C::Mods(decode_mods(ms).unwrap_or_default())
            } else {
                C::Other(format!("{j}"))
            }
        }
        J::Array(a) => {
            // a JSON list (rangeLineList): a 1-based table
            let mut m = BTreeMap::new();
            for (i, x) in a.iter().enumerate() {
                m.insert(CK::I(i as i64 + 1), c_from_lua(x));
            }
            C::T(m)
        }
    }
}

fn c_from_value(v: &Value) -> C {
    match v {
        Value::Nil => C::Nil,
        Value::Bool(b) => C::B(*b),
        Value::Num(n) => C::N(*n),
        Value::Str(s) => C::S(s.to_string()),
        Value::Mod(m) => C::Mods(vec![(**m).clone()]),
        Value::JewelFunc(f) => C::S(format!("$jf:{}", f.r#type.map_or("nil".to_owned(), |t| t.to_string()))),
        Value::Table(t) => {
            let mut m = BTreeMap::new();
            for (i, x) in t.array.iter().enumerate() {
                if !x.is_nil() {
                    m.insert(CK::I(i as i64 + 1), c_from_value(x));
                }
            }
            for (k, x) in &t.fields {
                if !x.is_nil() {
                    m.insert(CK::S(k.to_string()), c_from_value(x));
                }
            }
            C::T(m)
        }
    }
}

/// Rust value → C.
fn c_from_val(v: &Val) -> C {
    match v {
        Val::Nil => C::Nil,
        Val::Bool(b) => C::B(*b),
        Val::Num(n) => C::N(*n),
        Val::Str(s) => C::S(s.to_string()),
        Val::Mod(m) => C::Mods(vec![(**m).clone()]),
        Val::Data(d) => c_from_value(d),
        Val::Table(t) => {
            let mut m = BTreeMap::new();
            for (k, x) in t.pairs() {
                let ck = match k {
                    Key::S(s) => CK::S(s.as_str().to_owned()),
                    Key::I(i) => CK::I(i),
                    Key::B(b) => CK::B(b),
                };
                m.insert(ck, c_from_val(&x));
            }
            C::T(m)
        }
        Val::Obj(_) => C::Other("userdata".to_owned()),
    }
}

fn opt_table(t: Option<Table>) -> C {
    t.map_or(C::Nil, |t| c_from_val(&Val::Table(t)))
}

fn shared_list(l: &pob_lua::Shared<Vec<Table>>) -> C {
    let mut m = BTreeMap::new();
    for (i, t) in l.snapshot().into_iter().enumerate() {
        m.insert(CK::I(i as i64 + 1), c_from_val(&Val::Table(t)));
    }
    C::T(m)
}

fn list_mods(l: &Option<ModList>) -> Option<Vec<Mod>> {
    l.as_ref().map(|l| l.ipairs().into_iter().map(|m| (*m).clone()).collect())
}

/// The jewelData table with `funcList` as `$jf:<type>` strings (as the oracle writes it).
fn jewel_c(t: Option<Table>) -> C {
    let Some(t) = t else { return C::Nil };
    let mut c = c_from_val(&Val::Table(t));
    if let C::T(m) = &mut c {
        if let Some(C::T(fl)) = m.get_mut(&CK::S("funcList".into())) {
            for v in fl.values_mut() {
                if let C::T(f) = v {
                    let ty = f.get(&CK::S("type".into())).map_or("false".to_owned(), |x| match x {
                        C::S(s) => s.clone(),
                        _ => "false".into(),
                    });
                    *v = C::S(format!("$jf:{ty}"));
                }
            }
        }
    }
    c
}

// ---- bookkeeping -----------------------------------------------------------------------------

#[derive(Default)]
struct Tally {
    rows: BTreeMap<&'static str, (u64, u64)>,
    fails: Vec<String>,
}

impl Tally {
    fn check(&mut self, cat: &'static str, ok: bool, what: impl FnOnce() -> String) {
        let e = self.rows.entry(cat).or_default();
        if ok {
            e.0 += 1;
        } else {
            e.1 += 1;
            let n = self.fails.iter().filter(|f| f.starts_with(&format!("[{cat}]"))).count();
            if self.fails.len() < 80 && n < 6 {
                self.fails.push(format!("[{cat}] {}", what()));
            }
        }
    }
    fn failed(&self) -> u64 {
        self.rows.values().map(|r| r.1).sum()
    }
}

type ParseResult = (Option<Vec<Mod>>, Option<String>);

fn mods_check(t: &mut Tally, cat: &'static str, src: &str, rust: Option<Vec<Mod>>, lua: &J) {
    let lua = decode_mods(lua);
    let mut diffs = Vec::new();
    match (&rust, &lua) {
        (None, None) => {}
        (Some(a), Some(b)) => mods_eq(a, b, cat, &mut diffs),
        _ => diffs.push(format!("{cat}: rust is {}, lua is {}", rust.is_some(), lua.is_some())),
    }
    t.check(cat, diffs.is_empty(), || {
        format!("{src}: {}", diffs.iter().take(3).cloned().collect::<Vec<_>>().join("\n    "))
    });
}

fn c_check(t: &mut Tally, cat: &'static str, src: &str, rust: &C, lua: &C) {
    let mut diffs = Vec::new();
    ceq(rust, lua, cat, &mut diffs);
    t.check(cat, diffs.is_empty(), || {
        format!("{src}: {}", diffs.iter().take(4).cloned().collect::<Vec<_>>().join("\n    "))
    });
}

/// The fields BuildModList writes, as the oracle writes them (`json(item[k])`).
fn rust_fields(item: &Item) -> BTreeMap<&'static str, C> {
    let num = |x: Option<f64>| x.map_or(C::Nil, C::N);
    let mut m = BTreeMap::new();
    m.insert("requirements", c_from_val(&Val::Table(item.requirements.clone())));
    m.insert("grantedSkills", shared_list(&item.grantedSkills.get()));
    // the sockets list carries the string key `colourAlwaysMatches` in Lua (CCR-0019 §7: the
    // port keeps it in `Item.extra`)
    let mut sockets = shared_list(&item.sockets.get());
    if let (C::T(s), cam @ Val::Bool(true)) = (&mut sockets, item.extra.get(Name::from("colourAlwaysMatches"))) {
        s.insert(CK::S("colourAlwaysMatches".into()), c_from_val(&cam));
    }
    m.insert("sockets", sockets);
    m.insert("abyssalSocketCount", C::N(item.abyssalSocketCount.get()));
    for k in ["selectableSocketCount", "socketedJewelEffectModifier", "hasModTags", "craftedQuality", "corruptImplicitCount"] {
        m.insert(k, c_from_val(&item.extra.get(Name::from(k))));
    }
    m.insert(
        "classRestriction",
        item.classRestriction.get().map_or(C::Nil, |n| C::S(n.to_string())),
    );
    m.insert("quality", num(item.quality.get()));
    m.insert(
        "clusterJewelSkill",
        item.clusterJewelSkill.get().map_or(C::Nil, |n| C::S(n.to_string())),
    );
    m.insert("clusterJewelNodeCount", num(item.clusterJewelNodeCount.get()));
    m.insert(
        "modSource",
        item.modSource.get().map_or(C::Nil, |s| C::S(s.to_string())),
    );
    m.insert("colourAlwaysMatches", c_from_val(&item.extra.get(Name::from("colourAlwaysMatches"))));
    m
}

const LINE_LISTS: [&str; 7] = [
    "enchantModLines",
    "scourgeModLines",
    "classRequirementModLines",
    "implicitModLines",
    "explicitModLines",
    "crucibleModLines",
    "buffModLines",
];

fn line_list(item: &Item, name: &str) -> Vec<Table> {
    match name {
        "enchantModLines" => item.enchantModLines.get().snapshot(),
        "scourgeModLines" => item.scourgeModLines.get().snapshot(),
        "classRequirementModLines" => item.classRequirementModLines.get().snapshot(),
        "implicitModLines" => item.implicitModLines.get().snapshot(),
        "explicitModLines" => item.explicitModLines.get().snapshot(),
        "crucibleModLines" => item.crucibleModLines.get().snapshot(),
        "buffModLines" => item.buffModLines.get().snapshot(),
        _ => unreachable!(),
    }
}

/// `fmt`/`fmtDb` of the oracle: a string, null (formatModLine returned nil) or `"$err"`.
fn fmt_c(r: Result<Option<String>, String>) -> C {
    match r {
        Ok(Some(s)) => C::S(s),
        Ok(None) => C::Nil,
        Err(_) => C::S("$err".into()),
    }
}

struct CanState {
    lua: HashMap<String, bool>,
    mismatches: Vec<String>,
    calls: u64,
}

fn check_parse(t: &mut Tally, src: &str, item: &Item, p: &J, can: &Rc<RefCell<CanState>>) {
    // CanHaveMod: PoB's results for this parse
    {
        let mut c = can.borrow_mut();
        c.lua.clear();
        c.mismatches.clear();
        c.calls = 0;
        for e in p["can"].as_array().into_iter().flatten() {
            c.lua.insert(e[0].as_str().unwrap().to_owned(), e[1].as_bool().unwrap());
        }
    }
    // set-up (ItemsTab.lua:1224-1243)
    if let Some(setup) = p["setup"].as_object() {
        let g = |k: &str| setup.get(k).and_then(jnum);
        item.id.set(g("id"));
        item.variant.set(g("variant"));
        let alts = [
            (&item.variantAlt, "variantAlt", "hasAltVariant"),
            (&item.variantAlt2, "variantAlt2", "hasAltVariant2"),
            (&item.variantAlt3, "variantAlt3", "hasAltVariant3"),
            (&item.variantAlt4, "variantAlt4", "hasAltVariant4"),
            (&item.variantAlt5, "variantAlt5", "hasAltVariant5"),
        ];
        for (cell, k, has) in alts {
            if let Some(x) = g(k) {
                item.extra.set(Name::from(has), true);
                cell.set(Some(x));
            }
        }
    }
    let raw = p["raw"].as_str().unwrap();
    let rarity = p["rarity"].as_str();
    let hq = p["highQuality"].as_bool().unwrap_or(false);
    let r = catch(|| item.ParseRaw(raw, rarity, hq));
    let lua_err = p["err"].as_str();
    t.check("Lua error status", r.is_err() == lua_err.is_some(), || {
        format!("{src}: rust={r:?} lua={lua_err:?}")
    });
    {
        let c = can.borrow();
        t.check("CanHaveMod results", c.mismatches.is_empty(), || {
            format!("{src}: {} of {} calls differ: {:?}", c.mismatches.len(), c.calls, &c.mismatches[..c.mismatches.len().min(5)])
        });
    }
    if r.is_err() || lua_err.is_some() {
        return;
    }
    let out = &p["out"];
    if out.get("$dumpErr").is_some() {
        t.check("oracle dump", false, || format!("{src}: {out}"));
        return;
    }
    // mod lists
    mods_check(t, "modList", src, list_mods(&item.modList.get()), &out["modList"]);
    match out["slotModList"].as_array() {
        Some(s) => {
            mods_check(t, "slotModList[1]", src, list_mods(&item.slotModList[0].get()), &s[0]);
            mods_check(t, "slotModList[2]", src, list_mods(&item.slotModList[1].get()), &s[1]);
            mods_check(t, "slotModList[3]", src, list_mods(&item.slotModList3()), &s[2]);
        }
        None => {
            let any = item.slotModList[0].get().is_some()
                || item.slotModList[1].get().is_some()
                || item.slotModList3().is_some();
            t.check("slotModList[1]", !any, || format!("{src}: rust has slot lists, lua none"));
        }
    }
    mods_check(t, "baseModList", src, list_mods(&item.baseModList.get()), &out["baseModList"]);
    mods_check(t, "buffModList", src, list_mods(&item.buffModList.get()), &out["buffModList"]);
    // data tables
    c_check(t, "weaponData", src, &opt_table(item.weaponData.get()), &c_from_lua(&out["weaponData"]));
    c_check(t, "armourData", src, &opt_table(item.armourData.get()), &c_from_lua(&out["armourData"]));
    c_check(t, "flaskData", src, &opt_table(item.flaskData.get()), &c_from_lua(&out["flaskData"]));
    c_check(t, "tinctureData", src, &opt_table(item.tinctureData.get()), &c_from_lua(&out["tinctureData"]));
    c_check(t, "jewelData", src, &jewel_c(item.jewelData.get()), &c_from_lua(&out["jewelData"]));
    // fields
    let rf = rust_fields(item);
    let lf = out["fields"].as_object().expect("fields");
    let mut diffs = Vec::new();
    // without a base BuildModList returns at once (Item.lua:2535): the typed defaults of a fresh
    // Rust item (empty grantedSkills, abyssalSocketCount 0) are Lua's nil; nothing to compare
    let has_base = item.base.get().is_some();
    for (k, rv) in rf.iter().filter(|_| has_base) {
        let lv = c_from_lua(lf.get(*k).unwrap_or(&J::Null));
        ceq(rv, &lv, k, &mut diffs);
    }
    t.check("item fields", diffs.is_empty(), || {
        format!("{src}: {}", diffs.iter().take(4).cloned().collect::<Vec<_>>().join("\n    "))
    });
    // rangeLineList
    let rl = match item.extra.get(Name::from("rangeLineList")) {
        Val::Table(l) => {
            let mut m = BTreeMap::new();
            for (i, x) in l.ipairs() {
                let line = x.table().map_or(Val::Nil, |t| t.get(Name::from("line")));
                m.insert(CK::I(i as i64), c_from_val(&line));
            }
            C::T(m)
        }
        _ => C::Nil,
    };
    c_check(t, "rangeLineList", src, &rl, &c_from_lua(&out["rangeLineList"]));
    // mod lines
    for name in LINE_LISTS {
        let lua_lines = out["lines"][name].as_array().cloned().unwrap_or_default();
        let rust_lines = line_list(item, name);
        t.check("mod line count", lua_lines.len() == rust_lines.len(), || {
            format!("{src}: {name}: rust {} lua {}", rust_lines.len(), lua_lines.len())
        });
        for (i, (rl, ll)) in rust_lines.iter().zip(&lua_lines).enumerate() {
            let at = format!("{src} {name}[{}] {:?}", i + 1, rl.get(Name::from("line")));
            t.check("mod line showSlider", rl.truthy(Name::from("showSlider")) == (ll["showSlider"] == J::Bool(true)), || at.clone());
            let rm: Option<Vec<Mod>> = rl.table(Name::from("modList")).map(|m| {
                m.ipairs()
                    .into_iter()
                    .filter_map(|(_, v)| match v {
                        Val::Mod(m) => Some((*m).clone()),
                        _ => None,
                    })
                    .collect()
            });
            mods_check(t, "mod line modList", &at, rm, &ll["modList"]);
            let f = fmt_c(catch(|| itemLib::formatModLine(rl, false)));
            c_check(t, "formatModLine", &at, &f, &c_from_lua(&ll["fmt"]));
            let f = fmt_c(catch(|| itemLib::formatModLine(rl, true)));
            c_check(t, "formatModLine (dbMode)", &at, &f, &c_from_lua(&ll["fmtDb"]));
            if !ll["scal"].is_null() {
                let line = rl.get(Name::from("line"));
                let s = catch(|| {
                    itemLib::isModLineScalable(
                        &line.str().unwrap(),
                        &rl.get(Name::from("range")),
                        rl.opt_num(Name::from("valueScalar")),
                    )
                });
                let rc = match s {
                    Ok(b) => J::Bool(b),
                    Err(_) => J::String("err".into()),
                };
                t.check("isModLineScalable", rc == ll["scal"], || format!("{at}: rust={rc} lua={}", ll["scal"]));
            }
        }
    }
}

fn range_val(j: &J) -> Val {
    match j {
        J::Null => Val::Nil,
        J::Array(a) => {
            let t = Table::new();
            for x in a {
                t.push(jnum(x).unwrap());
            }
            Val::Table(t)
        }
        J::String(s) => Val::from(s.as_str()),
        _ => Val::Num(jnum(j).unwrap()),
    }
}

#[test]
fn item_mods_vs_pob() {
    let Some(src) = pob_testpaths::require(find_pob_src(), "no PoB checkout (.PathOfBuilding/src)")
    else {
        return; // POB_ALLOW_SKIP=1
    };
    if pob_testpaths::require(init_data().then_some(()), "data/dump not found").is_none() {
        return; // POB_ALLOW_SKIP=1
    }
    quiet_lua_errors();
    let d = pob_gamedata::data();

    // data.jewelRadius as PoB's Data.lua/Main.lua leave it (setJewelRadiiGlobally(latest) = 3_16)
    let radii = d.jewelRadii[&Name::from("3_16")].clone();
    let maxJewelRadius = radii.iter().map(|r| r.outer).fold(0.0, f64::max);
    pob_gamedata::data::set_jewel_radius_sel(pob_gamedata::DataSel {
        jewelRadius: radii,
        maxJewelRadius,
    });

    // inputs (HYG2: a missing `.parity` used to drop the corpus half silently)
    if pob_testpaths::require(find_parity_dir(), ".parity (build corpus)").is_none() {
        return; // POB_ALLOW_SKIP=1
    }
    let corpus = corpus_items();
    let corpus_n = corpus.len();
    eprintln!("corpus: {corpus_n} <Item>s in .parity/builds");
    // not std::env::temp_dir(): /tmp is a RAM-backed tmpfs on the port machines
    let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("itc_oracle_{}.jsonl", std::process::id()));
    // the oracle's input (everything but the output path, which is not part of the cache key)
    let mut prologue = String::from("__itc_corpus = {\n");
    for c in &corpus {
        prologue.push_str(&format!(
            "{{ src = {}, raw = {}",
            lua_long(&c.src),
            lua_long(&c.raw)
        ));
        for (k, v) in &c.attrs {
            prologue.push_str(&format!(", {k} = {}", g17(*v)));
        }
        prologue.push_str(" },\n");
    }
    prologue.push_str("}\n__itc_synth = {\n");
    for s in SYNTH {
        prologue.push_str(&lua_long(s));
        prologue.push_str(",\n");
    }
    prologue.push_str("}\n");
    if let Ok(l) = std::env::var("IT_C_LIMIT") {
        prologue.push_str(&format!("__itc_limit = {l}\n"));
    }
    let script = include_str!("../corpus/IT-C/item_mods_oracle.lua");

    // HYG3 (test audit S3): PoB's output (the header and the JSONL records) is a function of the
    // PoB source, the input above and the script, so it is cached in the shared probe cache
    // (`.parity/probe-cache/<source-key>/<input-key>/query-<script-key>/`; `POB_QUERY_CACHE`);
    // a miss boots PoB under the machine-wide oracle lock.
    let t0 = std::time::Instant::now();
    let cache = QueryCache::new(&src, prologue.as_bytes(), &[script])
        .unwrap_or_else(|e| panic!("query cache: {e}"));
    let cached = cache.as_ref().and_then(|c| c.get("result"));
    let from_cache = cached.is_some();
    let header = cached.unwrap_or_else(|| {
        let _lock = OracleLock::acquire().expect("oracle lock");
        let mut pob = JitPob::boot_oracle(&src, None).unwrap_or_else(|e| panic!("boot: {e}"));
        let ret = pob
            .run(&format!(
                "__itc_out = {}\n{prologue}\n{script}",
                lua_long(&out.to_string_lossy())
            ))
            .unwrap_or_else(|e| panic!("oracle script: {e}"));
        let Some(Ret::Str(header)) = ret.first() else {
            panic!("oracle returned {ret:?}")
        };
        let header = header.clone();
        drop(pob);
        if let Some(c) = &cache {
            // the header and the records published as one entry (never paired across runs)
            if let Err(e) = c.put_entry(&header, &[("records", &out)]) {
                eprintln!("query cache: not stored: {e}");
            }
        }
        header
    });
    let header: J = serde_json::from_str(header.as_str()).expect("header JSON");
    eprintln!(
        "oracle: {header} in {:?}{}",
        t0.elapsed(),
        if from_cache { " (cached, HYG3)" } else { "" }
    );

    let mut t = Tally::default();
    let daq = jnum(&header["defaultItemAffixQuality"]);
    t.check(
        "main.defaultItemAffixQuality",
        daq == item_parse::main_defaultItemAffixQuality(),
        || format!("rust={:?} lua={daq:?}", item_parse::main_defaultItemAffixQuality()),
    );
    t.check(
        "main.notSupportedModTooltips",
        header["notSupportedModTooltips"].as_bool() == Some(itemLib::main_notSupportedModTooltips())
            && header["notSupportedTooltipText"].as_str() == Some(itemLib::main_notSupportedTooltipText),
        || format!("lua={} {}", header["notSupportedModTooltips"], header["notSupportedTooltipText"]),
    );
    t.check(
        "launch.devModeAlt",
        header["devModeAlt"].as_bool() == Some(itemLib::launch_devModeAlt()),
        || format!("lua={}", header["devModeAlt"]),
    );

    // hooks: parseMod replayed; CanHaveMod runs the port and is compared with PoB. The oracle's
    // JSONL is streamed (memory budget: never hold every record): a parseMod record precedes every
    // applyRange / item record whose PoB call made it, so the map is complete when it is used.
    let parse: Rc<RefCell<HashMap<(String, bool), ParseResult>>> = Rc::default();
    let misses: Rc<RefCell<Vec<String>>> = Rc::default();
    let pm: tools_replay::ParseModFn = {
        let parse = parse.clone();
        let misses = misses.clone();
        Rc::new(move |line: &str, isComb: bool| {
            let hit = parse.borrow().get(&(line.to_owned(), isComb)).cloned();
            match hit {
                Some(r) => r,
                None => {
                    misses.borrow_mut().push(format!("{line:?} {isComb}"));
                    pob_modparser::parseMod(line, isComb)
                }
            }
        })
    };
    let can = Rc::new(RefCell::new(CanState {
        lua: HashMap::new(),
        mismatches: Vec::new(),
        calls: 0,
    }));
    {
        let can = can.clone();
        let pm2 = pm.clone();
        replay::set(replay::Hooks {
            applyRange: None,
            parseMod: Some(Rc::new(move |l, c| pm2(l, c))),
            BuildModList: None,
            CanHaveMod: Some(Rc::new(move |item: &Item, modId: Name, m| {
                let r = item.CanHaveMod(m, None);
                let mut c = can.borrow_mut();
                c.calls += 1;
                match c.lua.get(modId.as_str()).copied() {
                    Some(l) if l == r => {}
                    other => {
                        let s = format!("{modId}: rust={r} lua={other:?}");
                        c.mismatches.push(s);
                    }
                }
                r
            })),
        });
    }
    tools_replay::set(tools_replay::Hooks { parseMod: Some(pm.clone()) });

    let t1 = std::time::Instant::now();
    let only = std::env::var("IT_C_ONLY").ok();
    let mut napplies = 0usize;
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    let mut nparses = 0usize;
    let records: Box<dyn BufRead> = match (&cache, from_cache) {
        (Some(c), true) => c
            .open("records")
            .unwrap_or_else(|e| panic!("cached oracle output: {e}")),
        _ => Box::new(std::io::BufReader::new(
            std::fs::File::open(&out).expect("oracle output"),
        )),
    };
    for line in records.lines() {
        let line = line.expect("read");
        let mut rec: J = serde_json::from_str(&line).expect("record JSON");
        drop(line);
        fix_json(&mut rec);
        match rec["t"].as_str() {
            Some("P") => {
                let key = (rec["line"].as_str().unwrap().to_owned(), rec["comb"].as_bool().unwrap());
                let mods = decode_mods(&rec["mods"]);
                let extra = rec["extra"].as_str().map(str::to_owned);
                parse.borrow_mut().entry(key).or_insert((mods, extra));
            }
            Some("A") => {
                // applyRange: the result (or that both raise)
                let a = &rec;
                napplies += 1;
                let line = a["line"].as_str().unwrap();
                let range = range_val(&a["range"]);
                let vs = jnum(&a["vs"]);
                let bvs = jnum(&a["bvs"]);
                let r = catch(|| itemLib::applyRange(line, &range, vs, bvs));
                let ok = match (&r, a["err"].is_null()) {
                    (Ok(s), true) => a["r"].as_str() == Some(s.as_str()),
                    (Err(_), false) => true,
                    _ => false,
                };
                t.check("applyRange", ok, || {
                    format!(
                        "{line:?} range={} vs={vs:?} bvs={bvs:?}: rust={r:?} lua={} err={}",
                        a["range"], a["r"], a["err"]
                    )
                });
            }
            Some("I") => {
                let src = rec["src"].as_str().unwrap_or("?").to_owned();
                if only.as_deref().is_some_and(|o| !src.contains(o)) {
                    continue;
                }
                *kinds.entry(rec["kind"].as_str().unwrap_or("?").to_owned()).or_default() += 1;
                let item = Item::default();
                for p in rec["parses"].as_array().unwrap() {
                    check_parse(&mut t, &src, &item, p, &can);
                    nparses += 1;
                }
            }
            other => panic!("record kind {other:?}"),
        }
    }
    match (std::env::var_os("IT_C_KEEP"), &cache, from_cache) {
        (Some(p), Some(c), true) => {
            let mut r = c.open("records").expect("IT_C_KEEP");
            std::io::copy(&mut r, &mut std::fs::File::create(p).expect("IT_C_KEEP"))
                .expect("IT_C_KEEP");
        }
        (Some(p), _, _) => {
            std::fs::rename(&out, p).expect("IT_C_KEEP");
        }
        (None, _, _) => {
            let _ = std::fs::remove_file(&out);
        }
    }
    replay::clear();
    tools_replay::clear();
    let nmiss = misses.borrow().len();
    t.check("parseMod replay misses", nmiss == 0, || {
        format!("{nmiss} calls PoB did not make, e.g. {:?}", &misses.borrow()[..nmiss.min(5)])
    });

    eprintln!(
        "IT-C: {napplies} applyRange calls; items by kind {kinds:?}; {nparses} ParseRaw calls; Rust side {:?}",
        t1.elapsed()
    );
    eprintln!("{:<32} {:>8} {:>6}", "check", "pass", "fail");
    for (k, (p, f)) in &t.rows {
        eprintln!("{k:<32} {p:>8} {f:>6}");
    }
    for f in &t.fails {
        eprintln!("FAIL {f}");
    }
    assert_eq!(t.failed(), 0, "{} IT-C mismatches (first ones above)", t.failed());
    if std::env::var_os("IT_C_ONLY").is_some() {
        return;
    }
    assert!(kinds.get("unique").copied().unwrap_or(0) > 0, "no uniques checked");
    assert_eq!(
        kinds.get("synthetic").copied().unwrap_or(0),
        SYNTH.len(),
        "every synthetic item must reach the oracle"
    );
    if std::env::var_os("IT_C_LIMIT").is_none() {
        // coverage (HYG2/V2): every unique, variant, rare, corpus and synthetic item reached
        // the oracle and came back, with all its ParseRaw and applyRange calls
        let got: Vec<(&str, usize)> = kinds.iter().map(|(k, v)| (k.as_str(), *v)).collect();
        assert_eq!(got, IT_C_KINDS, "oracle records by kind");
        assert_eq!(
            corpus_n, IT_C_CORPUS_ITEMS,
            "corpus <Item>s in .parity/builds"
        );
        assert!(
            nparses >= IT_C_MIN_CALLS.0 && napplies >= IT_C_MIN_CALLS.1,
            "{nparses} ParseRaw / {napplies} applyRange calls, expected at least {IT_C_MIN_CALLS:?}"
        );
    }
}

/// `<Item>`s in the 57 corpus builds; measured by HYG2 on rust-port 746b103.
const IT_C_CORPUS_ITEMS: usize = 1527;
/// Oracle records by kind (sorted); measured by HYG2 on rust-port 746b103.
const IT_C_KINDS: &[(&str, usize)] = &[
    ("corpus", 1527),
    ("rare", 205),
    ("synthetic", 25),
    ("unique", 1333),
    ("variant", 5425),
];
/// `(ParseRaw calls, applyRange calls)` compared; measured by HYG2 on rust-port 746b103.
const IT_C_MIN_CALLS: (usize, usize) = (10_042, 71_549);

/// Rust-only checks of itemLib paths that need no oracle.
#[test]
fn item_tools_units() {
    quiet_lua_errors();
    assert_eq!(itemLib::antonymFunc("10%", "increased"), "10% reduced");
    assert_eq!(itemLib::antonymFunc("10%", "faster"), "-10% faster");
    assert!(itemLib::isZeroValueLine("+0% to Fire Resistance"));
    assert!(itemLib::isZeroValueLine("Adds 0-0 Fire Damage"));
    assert!(!itemLib::isZeroValueLine("Adds 0 to 10 Fire Damage"));
    assert_eq!(itemLib::applyValueScalar("+10 to Strength", None, None, Some(1.0), None), "+10 to Strength");
    assert_eq!(itemLib::applyValueScalar("+10 to Strength", Some(1.5), None, Some(1.0), None), "+15 to Strength");
    assert_eq!(itemLib::formatValue(0.5, None, None, Some(10.0), Some(1.0), false), "0.5");
    // a nil precision raises (Lua `value * precision`)
    assert!(catch(|| itemLib::formatValue(1.0, None, None, None, None, false)).is_err());
    if init_data() {
        // fallback path with a list range raises: `range * …` on a table (ItemTools.lua:310)
        let t = Table::new();
        t.push(0.5);
        let r = catch(|| itemLib::applyRange("+(10-20) to Unscalable Nonsense Stat - x", &Val::Table(t), None, None));
        let _ = r; // the result depends on modScalability; both outcomes are checked by the oracle test
        assert_eq!(
            itemLib::applyRange("+(10-20) to maximum Life", &Val::Num(0.5), None, None),
            "+15 to maximum Life"
        );
    }
}
