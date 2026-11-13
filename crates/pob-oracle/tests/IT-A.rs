//! IT-A acceptance (calc doc §11.1 row IT-A, §9.4 L2 "Item: ParseRaw fields"): `Item:ParseRaw`
//! (Classes/Item.lua 424-1641) against PoB under LuaJIT (interpreter, `jit.off`).
//!
//! `corpus/IT-A/parse_raw_oracle.lua` parses, in PoB:
//! * every unique (`data.uniques`, all types; `new("Item"):Item(raw, "UNIQUE", true)`, Main.lua:166),
//! * every variant of every unique with more than one (`variant = v`, `BuildRaw`, parsed into a
//!   fresh item as `BuildAndParseRaw` does),
//! * every rare (`data.rares`, `"RARE", true`),
//! * every item of the build corpus (`.parity/builds`), as ItemsTab:Load does (`Item("")`, the
//!   `id`/`variant*` attributes, then `ParseRaw(text)` on the same object),
//! * synthetic pasted items covering the advanced-copy, magic/normal, catalyst, cluster, jewel,
//!   variant-group/version, reminder-text and mod-magnitude paths,
//!
//! and records for each `ParseRaw` call: its arguments, every call it makes into code that other
//! units own (`itemLib.applyRange`, `modLib.parseMod`, `Item:CanHaveMod`) with the result, the
//! item's state at the entry of `BuildModList` (every field ParseRaw set) and what ParseRaw does
//! after BuildModList. The Rust ParseRaw runs the same calls with those callees replayed through
//! `pob_calc::classes::item_parse::replay` (applyRange/BuildModList/CanHaveMod are IT-C's and
//! unported; MP1-MP6's parser lists are unported), and the test checks:
//! * the sequence of applyRange/parseMod calls (arguments, in order) equals PoB's, and every
//!   CanHaveMod call has a PoB counterpart (as a multiset: `pairs(self.affixes)` order is random
//!   in PoB);
//! * every item field at BuildModList entry equals PoB's (typed fields, `Item.extra`, influences;
//!   mod lines with their mods; data references by identity);
//! * `jewelRadiusIndex` / `isUnique` after the call, and Lua errors (both raise or neither).
//!
//! Informational (not gated): how many of PoB's `parseMod` calls the native Rust parser already
//! reproduces (all, and those served by the ModCache seed).
//!
//! Fails without PoB / the data dump (`pob_testpaths::require`; `POB_ALLOW_SKIP=1` skips). The inputs
//! are gitignored, so from a worktree point `POB_SRC`, `POB_PARITY_DIR` and `POB_DUMP_DIR` at the
//! main checkout's `.PathOfBuilding/src`, `.parity` and `data/dump`. `IT_A_LIMIT=<n>` limits the
//! uniques; `IT_A_KEEP=<file>` keeps the oracle's JSONL;
//! PoB's output is cached in the shared probe cache (HYG3: keyed by the PoB source fingerprint and
//! oracle runtime, the oracle input and the script; `POB_QUERY_CACHE=refresh|off`), so a warm run
//! boots no PoB; a cold run holds the machine-wide oracle lock while PoB runs;
//! `IT_A_ONLY=<substring>` checks only the items whose source contains it (debugging; the
//! completeness asserts are skipped); `IT_A_VERBOSE=1` prints both call sequences of a mismatch.

#![allow(clippy::pedantic, non_snake_case)]

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::io::BufRead;
use std::path::PathBuf;
use std::rc::Rc;

use pob_calc::classes::item::ItemAffixes;
use pob_calc::classes::item_parse::{self, replay};
use pob_calc::classes::Item;
use pob_calc::prelude::{LuaError, Mod, Name};
use pob_calc::{Key, LStr, Table, Val};
use pob_gamedata::schema::affixes::AffixTable;
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
        match &tail[1..j] {
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

/// Every `<Item>` of every build in `.parity/builds`: its text (child elements removed) and the
/// attributes ItemsTab:Load reads before parsing (ItemsTab.lua:1224-1243).
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

/// A Lua long-string literal for `s`.
fn lua_long(s: &str) -> String {
    let mut eq = String::new();
    while s.contains(&format!("]{eq}]")) || s.contains(&format!("[{eq}[")) {
        eq.push('=');
    }
    format!("[{eq}[\n{s}]{eq}]")
}

/// Synthetic pasted items: branches the unique/rare/corpus items reach rarely or never.
const SYNTH: &[&str] = &[
    // advanced copy (Ctrl+Alt+C): affix names, tiers, tags, implicit/crafted/custom lines
    "Item Class: Rings\nRarity: Rare\nGolem Loop\nRuby Ring\n--------\nRequirements:\nLevel: 56\n--------\nItem Level: 84\n--------\n{ Implicit Modifier \u{2014} Elemental, Fire, Resistance }\n+25(20-30)% to Fire Resistance (implicit)\n--------\n{ Prefix Modifier \"Hale\" (Tier: 9) \u{2014} Life }\n+12(10-19) to maximum Life\n{ Suffix Modifier \"of the Furnace\" (Tier: 3) \u{2014} Elemental, Fire, Resistance }\n+38(36-41)% to Fire Resistance\n{ Master Crafted Suffix Modifier \"of Craft\" (Rank: 1) \u{2014} Speed }\n+5(4-6)% increased Attack Speed (crafted)\n",
    // advanced copy: influence implicit, fractured prefix, unknown affix name, enhancement line,
    // influence/fractured/corrupted flags, sockets with groups, catalyst-free quality
    "Item Class: Helmets\nRarity: Rare\nDoom Visor\nHubris Circlet\n--------\nQuality: +20% (augmented)\nEnergy Shield: 250 (augmented)\n--------\nRequirements:\nLevel: 69\nInt: 154\n--------\nSockets: B-B-B G\n--------\nItem Level: 86\n--------\n{ Searing Exarch Implicit Modifier (Lesser) \u{2014} Damage }\n5% increased Damage (implicit)\n--------\n{ Fractured Prefix Modifier \"Seraphim's\" (Tier: 2) \u{2014} Defences, Energy Shield }\n+40(39-42) to maximum Energy Shield (fractured)\n{ Prefix Modifier \"Blazing\" (Tier: 1) }\n88(80-91)% increased Energy Shield\n{ Unique Modifier \u{2014} Enhancement }\nSomething odd\n--------\nSearing Exarch Item\nFractured Item\nCorrupted\n",
    // magic flask: name affixes around the base, in-game flask state lines
    "Rarity: Magic\nSeething Divine Life Flask of Staunching\n--------\nRecovers 2400 Life over 2.50 Seconds\nConsumes 15 of 45 Charges on use\nCurrently has 45 Charges\n--------\n66% of Recovery applied Instantly\nGrants Immunity to Bleeding for 4 seconds if used while Bleeding\n",
    // normal Energy Blade with an item class
    "Item Class: One Hand Swords\nRarity: Normal\nEnergy Blade\n",
    // magic Two-Toned Boots with the evasion hack
    "Rarity: Magic\nStalwart Two-Toned Boots of the Lizard\n--------\nArmour: 120\nEvasion Rating: 120\n--------\nItem Level: 80\n--------\n+8% to all Elemental Resistances (implicit)\n--------\n+20 to Armour\n",
    // rare jewel with radius and limit
    "Rarity: Rare\nGloom Eye\nCobalt Jewel\n--------\nLimited to: 2\nRadius: Medium\n--------\nItem Level: 84\n--------\n+12% to Global Critical Strike Multiplier\n",
    // cluster jewel: skill and node count
    "Rarity: Rare\nKraken Spark\nLarge Cluster Jewel\n--------\nCluster Jewel Skill: affliction_fire_damage\nCluster Jewel Node Count: 12\n--------\nItem Level: 84\n--------\nAdds 8 Passive Skills (enchant)\n2 Added Passive Skills are Jewel Sockets (enchant)\nAdded Small Passive Skills grant: 12% increased Fire Damage (enchant)\n--------\n1 Added Passive Skill is Burning Bright\n",
    // catalyst quality
    "Rarity: Rare\nStorm Band\nAmethyst Ring\n--------\nQuality (Life and Mana Modifiers): +20% (augmented)\n--------\nItem Level: 84\n--------\n+17% to Chaos Resistance (implicit)\n--------\n+52 to maximum Life\n+40 to maximum Mana\n+30% to Cold Resistance\n",
    // PoB format: crafted affixes (list range, None, unknown), mod groups, unscalable value,
    // reminder text, ranges, affix limit lines
    "Rarity: RARE\nTest Crafted\nSapphire Ring\nCrafted: true\nPrefix: {range:0.5}IncreasedLife7\nPrefix: {fractured}{range:0.2,0.8}IncreasedLife6\nSuffix: None\nSuffix: Unknownish\nLevelReq: 60\nCatalyst: Fertile\nCatalystQuality: 15\nImplicits: 1\n{crafted}+20% to Cold Resistance\n{modGroup:abc}Adds 10 to 20 Physical Damage to Attacks\n{modGroup:abc}+5 to Strength\n+10 to Dexterity - Unscalable Value\n(Some reminder text\nspanning lines)\n{range:0.3}{tags:resource,life}+(20-30) to maximum Life\n+(10-20)% increased Rarity of Items found\n+1 Prefix Modifier allowed\n-1 Suffix Modifier allowed\nImplicit Modifiers Cannot Be Changed\n",
    // relic
    "Rarity: Unique\nHeadhunter\nLeather Belt\n--------\nFoil Unique (Celestial Ruby)\n--------\n+40 to maximum Life (implicit)\n--------\n+55 to Strength\n+50 to Dexterity\n+60 to maximum Life\n20% increased Damage with Hits against Rare monsters\nWhen you Kill a Rare Monster, you gain its Modifiers for 60 seconds\n",
    // mod magnitudes on an advanced-copy unique
    "Rarity: Unique\nTest Magnitude\nGold Amulet\n--------\nItem Level: 80\n--------\n{tags:resource,life}{range:0.5}+(20-30) to maximum Life\n{tags:attack,speed}{range:1}(5-10)% increased Attack Speed\n{prefix}+10 to Intelligence\n50% increased Explicit Life Modifier magnitudes\nImplicit Modifier magnitudes are doubled\n25% increased effect of Prefixes\n",
    // unidentified
    "Rarity: Rare\nUnidentified Thing\nVaal Regalia\n--------\nUnidentified\n",
    // class requirement, influences, synthesised/split/mirrored
    "Rarity: Rare\nThing\nSynthesised Ruby Ring\n--------\nRequires Class Witch\n--------\nItem Level: 84\n--------\n+10% to Fire Resistance\n--------\nShaper Item\nElder Item\nSynthesised Item\nSplit\nMirrored\n",
    // versions, variant groups, versioned base lines, selected version/group
    "Rarity: UNIQUE\nTest Versioned\nPrismatic Ring\nVersion: Pre 3.0\nVersion: Current\nVariant: A\nVariant: B\nVariant: C\nVariant: D\nSelected Variant Group: 1=2\nSelected Variant Group: 2 = 3\nSelected Version: 1\nImplicits: 1\n{version:1}+(8-10)% to all Elemental Resistances\n{version:2}{variant:1}{group:1}10% increased maximum Life\n{version:1,2}{variant:2}{group:1}20% increased maximum Mana\n{variant:3}{group:2}Damage cannot be Reflected\n{version:2}{variant:1,3}{group:2}{tags:attack,speed}5% increased Attack Speed\n{group:2}Line with a group but no variant\n{variant:9}{group:1}Line naming an unknown variant\n{range:0.1234567}+(20-40) to maximum Life\n{corruptedRange:1.23456}+(10-20) to Strength\n",
    // old Watcher's Eye compatibility line, alt variants, league/source/note/upgrade specs
    "Rarity: UNIQUE\nTest Alt\nCrimson Jewel\nVariant: One\nVariant: Two\nVariant: Three\nHas Alt Variant: true\nHas Alt Variant Two: true\nSelected Variant: 2\nSelected Alt Variant: 3\nHas Variants: 1,2\nignored line\nLeague: Heist\nSource: Drops somewhere\nNote: hello\nUpgrade: Upgrades via something\nUnreleased: true\nLimited to: 1\nRadius: Variable\nImplicits: 0\n{variant:1}Only affects Passives in Small Ring\n{variant:2}Only affects Passives in Medium Ring\n{variant:3}+10 to Strength\n",
];

// ---- oracle JSON helpers ---------------------------------------------------------------------

/// The oracle writes every byte >= 128 as `\u00XX`: rebuild the byte string.
fn bytes_str(s: &str) -> String {
    if s.chars().all(|c| (c as u32) < 256) {
        let b: Vec<u8> = s.chars().map(|c| c as u8).collect();
        String::from_utf8_lossy(&b).into_owned()
    } else {
        s.to_owned()
    }
}

/// Apply [`bytes_str`] to every string of a JSON tree.
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
                let k = if k.bytes().any(|b| b >= 0x80) {
                    bytes_str(&k)
                } else {
                    k
                };
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

/// `%.17g` as the oracle formats replay keys.
fn g17(x: f64) -> String {
    lua_format("%.17g", &[FmtArg::Num(x)])
}

/// The oracle's `repr` of an applyRange argument.
fn repr(v: &Val) -> String {
    match v {
        Val::Nil => "nil".to_owned(),
        Val::Num(x) => g17(*x),
        Val::Table(t) => {
            let parts: Vec<String> = t.ipairs().iter().map(|(_, x)| repr(x)).collect();
            format!("{{{}}}", parts.join(","))
        }
        Val::Str(s) => s.to_string(),
        Val::Bool(b) => b.to_string(),
        other => format!("{other:?}"),
    }
}

fn ropt(x: Option<f64>) -> String {
    x.map_or_else(|| "nil".to_owned(), g17)
}

fn decode_mods(j: &J) -> Option<Vec<Mod>> {
    match j {
        J::Null => None,
        J::Array(a) => Some(
            a.iter()
                .map(|m| {
                    serde_json::from_value::<Mod>(m.clone())
                        .unwrap_or_else(|e| panic!("mod {m}: {e}"))
                })
                .collect(),
        ),
        other => panic!("bad mod list {other}"),
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
    /// A data reference (by identity) or the item's own prefixes/suffixes list.
    Ref(String),
}

fn ceq(a: &C, b: &C, path: &str, diffs: &mut Vec<String>) {
    let ok = match (a, b) {
        (C::Nil, C::Nil) => true,
        (C::B(x), C::B(y)) => x == y,
        (C::N(x), C::N(y)) => x == y || (x.is_nan() && y.is_nan()),
        (C::S(x), C::S(y)) => x == y,
        (C::Ref(x), C::Ref(y)) => x == y,
        (C::Mods(x), C::Mods(y)) => {
            if x.len() != y.len() {
                false
            } else {
                for (i, (m, n)) in x.iter().zip(y).enumerate() {
                    if m != n {
                        diffs.push(format!(
                            "{path}[{}]: mod rust={m:?}\n      lua={n:?}",
                            i + 1
                        ));
                    }
                }
                true
            }
        }
        (C::T(x), C::T(y)) => {
            for k in x.keys().chain(y.keys().filter(|k| !x.contains_key(k))) {
                let p = format!("{path}.{}", ck_show(k));
                let va = x.get(k).unwrap_or(&C::Nil);
                let vb = y.get(k).unwrap_or(&C::Nil);
                ceq(va, vb, &p, diffs);
            }
            true
        }
        _ => false,
    };
    if !ok {
        diffs.push(format!("{path}: rust={} lua={}", cshow(a), cshow(b)));
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
        C::Ref(r) => format!("ref {r}"),
    };
    if s.len() > 300 {
        format!(
            "{}…",
            &s[..s.char_indices().nth(300).map_or(s.len(), |(i, _)| i)]
        )
    } else {
        s
    }
}

fn ck_from_num(x: f64) -> CK {
    if x == x.trunc() && x.abs() < 9.0e15 {
        CK::I(x as i64)
    } else {
        CK::F(x.to_bits())
    }
}

/// Lua state JSON → C. `calls` resolves `{"$modsOf": i}`.
fn c_from_lua(j: &J, calls: &[J]) -> C {
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
                    m.insert(k, c_from_lua(&kv[1], calls));
                }
                C::T(m)
            } else if let Some(i) = o.get("$modsOf").and_then(J::as_u64) {
                let call = &calls[i as usize - 1];
                C::Mods(decode_mods(&call[3]).unwrap_or_default())
            } else if let Some(ms) = o.get("$mods") {
                C::Mods(decode_mods(ms).unwrap_or_default())
            } else if let Some(J::String(r)) = o.get("$ref") {
                C::Ref(resolve_lua_ref(r))
            } else {
                C::Ref(format!("opaque {j}"))
            }
        }
        J::Array(_) => panic!("unexpected array in state"),
    }
}

/// A data reference as an identity string `kind@ptr`.
fn ptr_id<T: ?Sized>(kind: &str, p: *const T) -> String {
    format!("{kind}@{:p}", p.cast::<u8>())
}

fn resolve_lua_ref(r: &str) -> String {
    let d = pob_gamedata::data();
    let (kind, key) = r.split_once(':').unwrap_or((r, ""));
    match kind {
        "base" => match d.itemBases.get(key) {
            Some(b) => base_id(b),
            None => format!("base?{key}"),
        },
        "itemMods" => match d.itemMods.get(&Name::from(key)) {
            Some(t) => affixes_id(t),
            None => format!("affixes?{key}"),
        },
        "rareLike" => match d.rareLikeUniques.0.get(key) {
            // `affixes` refs name the rare-like table's affixes; `rareLikeUnique` refs the record
            Some(t) => format!("rareLike@{:p}", std::sync::Arc::as_ptr(t)),
            None => format!("rareLike?{key}"),
        },
        "ench" => match d.enchantments.get(key) {
            Some(e) => format!("ench@{:p}", std::sync::Arc::as_ptr(e)),
            None => format!("ench?{key}"),
        },
        "foulborn" => match d.foulbornMap.0.get(key) {
            Some(e) => format!("foulborn@{:p}", std::sync::Arc::as_ptr(e)),
            None => format!("foulborn?{key}"),
        },
        "cluster" => match d.clusterJewels.jewels.get(&Name::from(key)) {
            Some(c) => format!("cluster:{}", c.size),
            None => format!("cluster?{key}"),
        },
        "prefixes" | "suffixes" => kind.to_owned(),
        _ => format!("?{r}"),
    }
}

fn base_id(b: &pob_gamedata::ItemBaseRef) -> String {
    format!("base@{:p}", std::sync::Arc::as_ptr(b))
}

/// Two affix tables hold the same mods (same ids, each mod the same `Arc` or equal content).
fn same_affixes(a: &AffixTable, b: &AffixTable) -> bool {
    a.0.len() == b.0.len()
        && a.0.iter().all(|(k, m)| {
            b.0.get(k).is_some_and(|n| {
                std::sync::Arc::ptr_eq(m, n)
                    || (m.lines == n.lines
                        && m.affix == n.affix
                        && m.r#type == n.r#type
                        && m.statOrder == n.statOrder)
            })
        })
}

/// The identity of an affix table. A rare-like unique whose Lua `affixes` IS a `data.itemMods`
/// table (`data.itemMods.Explicit` for Dread Captain's Cutlass, Data.lua:1414) holds a copy in the
/// typed data, so a `data.itemMods` table is named by its content.
fn affixes_id(a: &'static AffixTable) -> String {
    let d = pob_gamedata::data();
    for (name, t) in &d.itemMods {
        if std::ptr::eq(t, a) || same_affixes(t, a) {
            return format!("affixes:{name}");
        }
    }
    for (_, t) in &d.rareLikeUniques.0 {
        if std::ptr::eq(&t.affixes, a) {
            return format!("rareLike@{:p}", std::sync::Arc::as_ptr(t));
        }
    }
    ptr_id("affixes", a as *const AffixTable)
}

/// Rust value → C. `key` is the field name the value is stored under (for `modList`, `table`).
fn c_from_val(v: &Val, key: Option<&str>, item: &Item) -> C {
    match v {
        Val::Nil => C::Nil,
        Val::Bool(b) => C::B(*b),
        Val::Num(n) => C::N(*n),
        Val::Str(s) => C::S(s.to_string()),
        Val::Table(t) => {
            if key == Some("modList") {
                let mods = t
                    .ipairs()
                    .into_iter()
                    .map(|(_, m)| match m {
                        Val::Mod(m) => (*m).clone(),
                        other => panic!("modList entry {other:?}"),
                    })
                    .collect();
                return C::Mods(mods);
            }
            if key == Some("table") {
                for k in ["prefixes", "suffixes"] {
                    if let Val::Table(x) = item.extra.get(Name::from(k)) {
                        if x.ptr_eq(t) {
                            return C::Ref(k.to_owned());
                        }
                    }
                }
            }
            let mut m = BTreeMap::new();
            for (k, x) in t.pairs() {
                let (ck, ks) = match k {
                    Key::S(s) => (CK::S(s.as_str().to_owned()), Some(s.as_str().to_owned())),
                    Key::I(i) => (CK::I(i), None),
                    Key::B(b) => (CK::B(b), None),
                    Key::F(f) => (CK::F(f.get().to_bits()), None),
                };
                m.insert(ck, c_from_val(&x, ks.as_deref(), item));
            }
            C::T(m)
        }
        Val::Obj(_) => match key {
            Some("affixes") => match v.obj::<ItemAffixes>() {
                Some(a) => C::Ref(affixes_id(a.0)),
                None => C::Ref("affixes?".into()),
            },
            Some("rareLikeUnique") => match item.rareLikeUnique_ref() {
                Some(r) => C::Ref(format!("rareLike@{:p}", std::sync::Arc::as_ptr(&r))),
                None => C::Ref("rareLike?".into()),
            },
            Some("enchantments") => match item.enchantments_ref() {
                Some(e) => C::Ref(format!("ench@{:p}", std::sync::Arc::as_ptr(&e))),
                None => C::Ref("ench?".into()),
            },
            Some("mutatedLines") => match item.mutatedLines_ref() {
                Some(e) => C::Ref(format!("foulborn@{:p}", std::sync::Arc::as_ptr(&e))),
                None => C::Ref("foulborn?".into()),
            },
            other => C::Ref(format!("obj {other:?}")),
        },
        other => C::Ref(format!("{other:?}")),
    }
}

fn shared_list(l: &pob_lua::Shared<Vec<Table>>, item: &Item) -> C {
    let mut m = BTreeMap::new();
    for (i, t) in l.snapshot().into_iter().enumerate() {
        m.insert(CK::I(i as i64 + 1), c_from_val(&Val::Table(t), None, item));
    }
    C::T(m)
}

/// The fields of a Rust item as PoB's `pairs(item)` sees them (typed fields, influences, extra).
fn rust_state(item: &Item) -> BTreeMap<String, C> {
    let mut s: BTreeMap<String, C> = BTreeMap::new();
    let mut put = |k: &str, c: C| {
        if !matches!(c, C::Nil) {
            s.insert(k.to_owned(), c);
        }
    };
    let num = |x: Option<f64>| x.map_or(C::Nil, C::N);
    let st = |x: Option<LStr>| x.map_or(C::Nil, |s| C::S(s.to_string()));
    let flag = |b: bool| if b { C::B(true) } else { C::Nil };
    put("id", num(item.id.get()));
    put("raw", C::S(item.raw.get().to_string()));
    put("name", C::S(item.name.get().to_string()));
    put("title", st(item.title.get()));
    put(
        "baseName",
        item.baseName.get().map_or(C::Nil, |n| C::S(n.to_string())),
    );
    put(
        "base",
        item.base.get().map_or(C::Nil, |b| C::Ref(base_id(&b))),
    );
    let ty = item.r#type.get();
    put(
        "type",
        if ty.is_empty() {
            C::Nil
        } else {
            C::S(ty.to_string())
        },
    );
    put("rarity", C::S(item.rarity.get().to_string()));
    put("uniqueID", st(item.uniqueID.get()));
    put("corrupted", flag(item.corrupted.get()));
    put("mirrored", flag(item.mirrored.get()));
    put("fractured", flag(item.fractured.get()));
    put("split", flag(item.split.get()));
    put("synthesised", flag(item.synthesised.get()));
    put("quality", num(item.quality.get()));
    put("itemLevel", num(item.itemLevel.get()));
    put("sockets", shared_list(&item.sockets.get(), item));
    put("abyssalSocketCount", C::N(item.abyssalSocketCount.get()));
    put("variant", num(item.variant.get()));
    put("variantAlt", num(item.variantAlt.get()));
    put("variantAlt2", num(item.variantAlt2.get()));
    put("variantAlt3", num(item.variantAlt3.get()));
    put("variantAlt4", num(item.variantAlt4.get()));
    put("variantAlt5", num(item.variantAlt5.get()));
    if let Some(vl) = item.variantList.get() {
        let mut m = BTreeMap::new();
        for (i, v) in vl.snapshot().into_iter().enumerate() {
            m.insert(CK::I(i as i64 + 1), C::S(v.to_string()));
        }
        put("variantList", C::T(m));
    }
    put(
        "explicitModLines",
        shared_list(&item.explicitModLines.get(), item),
    );
    put(
        "implicitModLines",
        shared_list(&item.implicitModLines.get(), item),
    );
    put(
        "enchantModLines",
        shared_list(&item.enchantModLines.get(), item),
    );
    put(
        "scourgeModLines",
        shared_list(&item.scourgeModLines.get(), item),
    );
    put(
        "crucibleModLines",
        shared_list(&item.crucibleModLines.get(), item),
    );
    put("buffModLines", shared_list(&item.buffModLines.get(), item));
    put(
        "classRequirementModLines",
        shared_list(&item.classRequirementModLines.get(), item),
    );
    put(
        "armourData",
        item.armourData
            .get()
            .map_or(C::Nil, |t| c_from_val(&Val::Table(t), None, item)),
    );
    put("jewelRadiusIndex", num(item.jewelRadiusIndex.get()));
    put("jewelRadiusLabel", st(item.jewelRadiusLabel.get()));
    put(
        "clusterJewel",
        item.clusterJewel.get().map_or(C::Nil, |t| {
            C::Ref(format!(
                "cluster:{}",
                t.get(Name::from("size")).str().unwrap_or_default()
            ))
        }),
    );
    put(
        "clusterJewelSkill",
        item.clusterJewelSkill
            .get()
            .map_or(C::Nil, |n| C::S(n.to_string())),
    );
    put(
        "clusterJewelNodeCount",
        num(item.clusterJewelNodeCount.get()),
    );
    put("catalyst", num(item.catalyst.get()));
    put("catalystQuality", num(item.catalystQuality.get()));
    put("talismanTier", num(item.talismanTier.get()));
    put(
        "requirements",
        c_from_val(&Val::Table(item.requirements.clone()), None, item),
    );
    put(
        "classRestriction",
        item.classRestriction
            .get()
            .map_or(C::Nil, |n| C::S(n.to_string())),
    );
    put("limit", num(item.limit.get()));
    for (k, v) in item.influences.pairs() {
        if let Key::S(k) = k {
            put(k.as_str(), flag(v.truthy()));
        }
    }
    for (k, v) in item.extra.pairs() {
        if let Key::S(k) = k {
            put(k.as_str(), c_from_val(&v, Some(k.as_str()), item));
        }
    }
    s
}

/// Typed boolean fields: Lua leaves them nil, the Rust `Cell<bool>` is false.
const TYPED_BOOLS: [&str; 5] = ["corrupted", "mirrored", "fractured", "split", "synthesised"];

fn lua_state(j: &J, calls: &[J]) -> BTreeMap<String, C> {
    let mut s = BTreeMap::new();
    for (k, v) in j.as_object().expect("state") {
        let c = c_from_lua(v, calls);
        if TYPED_BOOLS.contains(&k.as_str()) && matches!(c, C::B(false)) {
            continue;
        }
        if !matches!(c, C::Nil) {
            s.insert(k.clone(), c);
        }
    }
    s
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
            // at most 3 reports per check and item (variants of one unique share a prefix)
            let msg = format!("[{cat}] {}", what());
            let item = msg.split([':', '@']).take(2).collect::<Vec<_>>().join(":");
            let n = self.fails.iter().filter(|f| f.starts_with(&item)).count();
            if self.fails.len() < 60 && n < 3 {
                self.fails.push(msg);
            }
        }
    }
    fn failed(&self) -> u64 {
        self.rows.values().map(|r| r.1).sum()
    }
}

#[derive(Default)]
struct NativeStats {
    all: (u64, u64),
    cached: (u64, u64),
}

// ---- one parse ---------------------------------------------------------------------------------

type ParseResult = (Option<Vec<Mod>>, Option<String>);

/// Replays one recorded ParseRaw call on `item` and compares.
fn check_parse(t: &mut Tally, native: &mut NativeStats, src: &str, item: &Item, p: &J) {
    let d = pob_gamedata::data();
    let calls: Vec<J> = p["calls"].as_array().cloned().unwrap_or_default();

    // replay tables
    let mut apply: HashMap<String, String> = HashMap::new();
    let mut parse: HashMap<(String, bool), ParseResult> = HashMap::new();
    let mut canHave: HashMap<String, bool> = HashMap::new();
    let mut lua_seq: Vec<String> = Vec::new();
    let mut lua_can: Vec<String> = Vec::new();
    for c in &calls {
        match c[0].as_str().unwrap() {
            "A" => {
                let key = c[1].as_str().unwrap().to_owned();
                lua_seq.push(format!("A {key}"));
                apply.insert(key, c[2].as_str().unwrap().to_owned());
            }
            "P" => {
                let line = c[1].as_str().unwrap().to_owned();
                let isComb = c[2].as_bool().unwrap();
                lua_seq.push(format!("P {line}\u{1}{isComb}"));
                let mods = decode_mods(&c[3]);
                let extra = c[4].as_str().map(str::to_owned);
                // informational: the native parser on the same line
                let (nm, ne) = pob_modparser::parseMod(&line, isComb);
                let same = nm == mods && ne == extra;
                native.all.0 += u64::from(same);
                native.all.1 += 1;
                if d.modCache.contains_key(line.as_str()) {
                    native.cached.0 += u64::from(same);
                    native.cached.1 += 1;
                }
                parse.insert((line, isComb), (mods, extra));
            }
            "C" => {
                let id = c[1].as_str().unwrap().to_owned();
                lua_can.push(id.clone());
                canHave.insert(id, c[2].as_bool().unwrap());
            }
            other => panic!("call kind {other}"),
        }
    }
    let rust_seq: Rc<RefCell<Vec<String>>> = Rc::default();
    let rust_can: Rc<RefCell<Vec<String>>> = Rc::default();
    let misses: Rc<RefCell<Vec<String>>> = Rc::default();
    let snapshot: Rc<RefCell<Option<BTreeMap<String, C>>>> = Rc::default();

    let apply = Rc::new(apply);
    let parse = Rc::new(parse);
    let canHave = Rc::new(canHave);
    let bml = p["bml"].clone();
    {
        let (seq, miss) = (rust_seq.clone(), misses.clone());
        let apply = apply.clone();
        let (seq2, miss2) = (rust_seq.clone(), misses.clone());
        let parse = parse.clone();
        let (can, miss3) = (rust_can.clone(), misses.clone());
        let canHave = canHave.clone();
        let snap = snapshot.clone();
        replay::set(replay::Hooks {
            applyRange: Some(Rc::new(move |line, range, vs, bvs| {
                let key = format!(
                    "{line}\u{1}{}\u{1}{}\u{1}{}",
                    repr(range),
                    ropt(vs),
                    ropt(bvs)
                );
                seq.borrow_mut().push(format!("A {key}"));
                match apply.get(&key) {
                    Some(r) => r.clone(),
                    None => {
                        miss.borrow_mut().push(format!("applyRange {key:?}"));
                        line.to_owned()
                    }
                }
            })),
            parseMod: Some(Rc::new(move |line, isComb| {
                seq2.borrow_mut().push(format!("P {line}\u{1}{isComb}"));
                match parse.get(&(line.to_owned(), isComb)) {
                    Some(r) => r.clone(),
                    None => {
                        miss2
                            .borrow_mut()
                            .push(format!("parseMod {line:?} {isComb}"));
                        (None, Some(line.to_owned()))
                    }
                }
            })),
            CanHaveMod: Some(Rc::new(move |_item, modId, _m| {
                can.borrow_mut().push(modId.as_str().to_owned());
                match canHave.get(modId.as_str()) {
                    Some(r) => *r,
                    None => {
                        miss3.borrow_mut().push(format!("CanHaveMod {modId}"));
                        false
                    }
                }
            })),
            BuildModList: Some(Rc::new(move |item: &Item| {
                *snap.borrow_mut() = Some(rust_state(item));
                if let Some(err) = bml["err"].as_str() {
                    pob_lua::lua_error!("{err}");
                }
                if bml["hasJewelData"].as_bool() == Some(true) {
                    let jd = Table::new();
                    if let Some(r) = jnum(&bml["radiusIndex"]) {
                        jd.set(Name::from("radiusIndex"), r);
                    }
                    item.jewelData.set(Some(jd));
                }
            })),
        });
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
    replay::clear();

    let lua_err = p["err"].as_str();
    t.check("Lua error status", r.is_err() == lua_err.is_some(), || {
        format!("{src}: rust={r:?} lua={lua_err:?}")
    });
    let rust_seq = rust_seq.borrow();
    let same_seq = *rust_seq == lua_seq;
    if !same_seq && std::env::var_os("IT_A_VERBOSE").is_some() {
        eprintln!("== {src}\n-- raw:\n{}", p["raw"].as_str().unwrap_or(""));
        for (i, (a, b)) in rust_seq
            .iter()
            .map(Some)
            .chain(std::iter::repeat(None))
            .zip(lua_seq.iter().map(Some).chain(std::iter::repeat(None)))
            .take(rust_seq.len().max(lua_seq.len()))
            .enumerate()
        {
            let mark = if a == b { " " } else { "!" };
            eprintln!("{mark}{:4} rust={a:?}\n      lua ={b:?}", i + 1);
        }
    }
    t.check("applyRange/parseMod call sequence", same_seq, || {
        let i = rust_seq
            .iter()
            .zip(&lua_seq)
            .position(|(a, b)| a != b)
            .unwrap_or(rust_seq.len().min(lua_seq.len()));
        format!(
            "{src}: first difference at call {} of rust {} / lua {}:\n      rust={:?}\n      lua ={:?}",
            i + 1,
            rust_seq.len(),
            lua_seq.len(),
            rust_seq.get(i),
            lua_seq.get(i)
        )
    });
    let mut rc = rust_can.borrow().clone();
    let mut lc = lua_can.clone();
    rc.sort();
    lc.sort();
    t.check("CanHaveMod calls (multiset)", rc == lc, || {
        format!("{src}: rust={rc:?} lua={lc:?}")
    });
    t.check("replay misses", misses.borrow().is_empty(), || {
        format!("{src}: {:?}", misses.borrow())
    });

    // state at BuildModList entry
    match (&*snapshot.borrow(), p["state"].is_null()) {
        (None, true) => t.check("state at BuildModList", true, String::new),
        (Some(rs), false) => {
            let ls = lua_state(&p["state"], &calls);
            let mut diffs = Vec::new();
            for k in rs.keys().chain(ls.keys().filter(|k| !rs.contains_key(*k))) {
                ceq(
                    rs.get(k).unwrap_or(&C::Nil),
                    ls.get(k).unwrap_or(&C::Nil),
                    k,
                    &mut diffs,
                );
            }
            t.check("state at BuildModList", diffs.is_empty(), || {
                format!(
                    "{src}: {} differences:\n    {}",
                    diffs.len(),
                    diffs
                        .iter()
                        .take(6)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join("\n    ")
                )
            });
        }
        (a, _) => t.check("state at BuildModList", false, || {
            format!(
                "{src}: rust reached BuildModList: {}, lua: {}",
                a.is_some(),
                !p["state"].is_null()
            )
        }),
    }

    // after BuildModList
    if r.is_ok() && lua_err.is_none() {
        let after = &p["after"];
        let jri = jnum(&after["jewelRadiusIndex"]);
        t.check(
            "jewelRadiusIndex after",
            item.jewelRadiusIndex.get() == jri,
            || format!("{src}: rust={:?} lua={jri:?}", item.jewelRadiusIndex.get()),
        );
        let iu = after["isUnique"].as_bool();
        let ru = item.extra.get(Name::from("isUnique"));
        t.check(
            "isUnique after",
            ru.truthy() == (iu == Some(true)) && !ru.is_nil(),
            || format!("{src}: rust={ru:?} lua={iu:?}"),
        );
    }
}

#[test]
fn parse_raw_vs_pob() {
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
        .join(format!("ita_oracle_{}.jsonl", std::process::id()));
    // the oracle's input (everything but the output path, which is not part of the cache key)
    let mut prologue = String::from("__ita_corpus = {\n");
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
    prologue.push_str("}\n__ita_synth = {\n");
    for s in SYNTH {
        prologue.push_str(&lua_long(s));
        prologue.push_str(",\n");
    }
    prologue.push_str("}\n");
    if let Ok(l) = std::env::var("IT_A_LIMIT") {
        prologue.push_str(&format!("__ita_limit = {l}\n"));
    }
    let script = include_str!("../corpus/IT-A/parse_raw_oracle.lua");

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
                "__ita_out = {}\n{prologue}\n{script}",
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
    // the oracle's data.jewelRadius / main.defaultItemAffixQuality are the ones the port uses
    let labels: Vec<String> = pob_gamedata::jewelRadius()
        .jewelRadius
        .iter()
        .map(|r| r.label.to_string())
        .collect();
    let lua_labels: Vec<String> = header["jewelRadius"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap().to_owned())
        .collect();
    t.check("data.jewelRadius labels", labels == lua_labels, || {
        format!("rust={labels:?} lua={lua_labels:?}")
    });
    let daq = jnum(&header["defaultItemAffixQuality"]);
    t.check(
        "main.defaultItemAffixQuality",
        daq == item_parse::main_defaultItemAffixQuality(),
        || {
            format!(
                "rust={:?} lua={daq:?}",
                item_parse::main_defaultItemAffixQuality()
            )
        },
    );

    // items
    let t1 = std::time::Instant::now();
    let records: Box<dyn BufRead> = match (&cache, from_cache) {
        (Some(c), true) => c
            .open("records")
            .unwrap_or_else(|e| panic!("cached oracle output: {e}")),
        _ => Box::new(std::io::BufReader::new(
            std::fs::File::open(&out).expect("oracle output"),
        )),
    };
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    let mut native = NativeStats::default();
    let mut nparses = 0usize;
    for line in records.lines() {
        let line = line.expect("read");
        let mut rec: J = serde_json::from_str(&line).expect("record JSON");
        fix_json(&mut rec);
        let src = rec["src"].as_str().unwrap_or("?").to_owned();
        if let Ok(only) = std::env::var("IT_A_ONLY") {
            if !src.contains(only.as_str()) {
                continue;
            }
        }
        *kinds
            .entry(rec["kind"].as_str().unwrap_or("?").to_owned())
            .or_default() += 1;
        let item = Item::default();
        for p in rec["parses"].as_array().unwrap() {
            check_parse(&mut t, &mut native, &src, &item, p);
            nparses += 1;
        }
    }
    match (std::env::var_os("IT_A_KEEP"), &cache, from_cache) {
        (Some(p), Some(c), true) => {
            let mut r = c.open("records").expect("IT_A_KEEP");
            std::io::copy(&mut r, &mut std::fs::File::create(p).expect("IT_A_KEEP"))
                .expect("IT_A_KEEP");
        }
        (Some(p), _, _) => {
            std::fs::rename(&out, p).expect("IT_A_KEEP");
        }
        (None, _, _) => {
            let _ = std::fs::remove_file(&out);
        }
    }

    eprintln!(
        "IT-A: items by kind {kinds:?}; {nparses} ParseRaw calls; Rust side {:?}",
        t1.elapsed()
    );
    eprintln!("{:<48} {:>8} {:>6}", "check", "pass", "fail");
    for (k, (p, f)) in &t.rows {
        eprintln!("{k:<48} {p:>8} {f:>6}");
    }
    eprintln!(
        "informational: native pob_modparser::parseMod equals PoB on {}/{} recorded calls ({}/{} on ModCache-seeded lines)",
        native.all.0, native.all.1, native.cached.0, native.cached.1
    );
    for f in &t.fails {
        eprintln!("FAIL {f}");
    }
    assert_eq!(
        t.failed(),
        0,
        "{} IT-A mismatches (first ones above)",
        t.failed()
    );
    if std::env::var_os("IT_A_ONLY").is_some() {
        return;
    }
    assert!(
        kinds.get("unique").copied().unwrap_or(0) > 0,
        "no uniques checked"
    );
    assert_eq!(
        kinds.get("synthetic").copied().unwrap_or(0),
        SYNTH.len(),
        "every synthetic item must reach the oracle"
    );
    if std::env::var_os("IT_A_LIMIT").is_none() {
        // coverage (HYG2/V2): every unique, variant, rare, corpus and synthetic item reached
        // the oracle and came back
        let got: Vec<(&str, usize)> = kinds.iter().map(|(k, v)| (k.as_str(), *v)).collect();
        assert_eq!(got, IT_A_KINDS, "oracle records by kind");
        assert_eq!(
            corpus_n, IT_A_CORPUS_ITEMS,
            "corpus <Item>s in .parity/builds"
        );
        assert!(
            nparses >= IT_A_MIN_PARSES,
            "{nparses} ParseRaw calls, expected at least {IT_A_MIN_PARSES}"
        );
    }
}

/// `<Item>`s in the 57 corpus builds; measured by HYG2 on rust-port 746b103.
const IT_A_CORPUS_ITEMS: usize = 1527;
/// Oracle records by kind (sorted); measured by HYG2 on rust-port 746b103.
const IT_A_KINDS: &[(&str, usize)] = &[
    ("corpus", 1527),
    ("rare", 205),
    ("synthetic", 15),
    ("unique", 1333),
    ("variant", 5425),
];
/// ParseRaw calls compared; measured by HYG2 on rust-port 746b103.
const IT_A_MIN_PARSES: usize = 10_032;

/// Rust-only checks of paths the oracle corpus cannot reach or that need no oracle.
#[test]
fn parse_raw_units() {
    quiet_lua_errors();
    assert_eq!(
        item_parse::escapeGGGString("<b>{Critical} [Critical|Critical Hit] [Fire]"),
        "Critical Critical Hit Fire"
    );
    // a text that ends at the separator: PoB leaves `name` nil (CCR-0025 §4); the port keeps ""
    if init_data() {
        let item = Item::default();
        let r = catch(|| item.ParseRaw("Rarity: Rare\n--------", None, false));
        assert!(r.is_ok(), "{r:?}");
        assert_eq!(&*item.name.get(), "");
        assert_eq!(item.rarity.get().as_str(), "RARE");
        assert!(item.base.get().is_none());
        // "Rarity: X" as the only line after the item class line
        let item = Item::default();
        let r = catch(|| item.ParseRaw("Item Class: Rings", None, false));
        assert!(r.is_err(), "indexing the missing rarity line raises: {r:?}");
        // a re-parse keeps `type`/`affixes` (Item.lua:424-531 resets neither): a crafted RARE
        // without a base reads `self.base.subType` only for a Jewel (Item.lua:1486)
        let d = pob_gamedata::data();
        let item = Item::default();
        item.r#type.set(Name::from("Sword"));
        item.extra.set(
            Name::from("affixes"),
            Val::Obj(Rc::new(ItemAffixes(&d.itemMods[&Name::from("Item")]))),
        );
        replay::set(replay::Hooks {
            applyRange: Some(Rc::new(|line, _, _, _| line.to_owned())),
            parseMod: Some(Rc::new(|line, _| (None, Some(line.to_owned())))),
            BuildModList: Some(Rc::new(|_| {})),
            CanHaveMod: Some(Rc::new(|_, _, _| false)),
        });
        let r = catch(|| {
            item.ParseRaw(
                "Rarity: RARE\nSome Name\nNot A Base\nCrafted: true",
                None,
                false,
            )
        });
        replay::clear();
        assert!(r.is_ok(), "{r:?}");
        assert!(item.base.get().is_none());
        assert_eq!(item.extra.get(Name::from("affixLimit")).num(), 6.0);
        let prefixes = item
            .extra
            .get(Name::from("prefixes"))
            .table()
            .expect("prefixes");
        assert_eq!(prefixes.len(), 3);
    }
}
