//! MS-B: rebuild PoB's mod stores, actors, items and cfgs in Rust from the JSON that
//! `corpus/MS-B/l3_full.lua` returns. Shared by `tests/MS-B.rs` (L3 full matrix) and
//! `benches/MS-B.rs` (gate G1a) through `#[path]`.
#![allow(dead_code)]

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use pob_calc::prelude::*;
use serde_json::Value as J;

pub const SCRIPT: &str = include_str!("l3_full.lua");

pub fn init_data() -> bool {
    if pob_gamedata::is_initialised() {
        return true;
    }
    let dir = std::env::var_os("POB_DUMP_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            pob_oracle::oracle::find_up("data/dump/manifest.json")
                .map(|p| p.parent().unwrap().to_path_buf())
        });
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

/// A LuaError panic as `Err` (other panics propagate).
pub fn catch<R>(f: impl FnOnce() -> R) -> Result<R, LuaError> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => Ok(r),
        Err(p) => match p.downcast::<LuaError>() {
            Ok(e) => Err(*e),
            Err(other) => std::panic::resume_unwind(other),
        },
    }
}

/// A mod entry `{"mod": {…}, "marks": {…}}`.
pub fn decode_mod(e: &J) -> Result<Mod, String> {
    let mut m: Mod = serde_json::from_value(e["mod"].clone()).map_err(|x| x.to_string())?;
    if let Some(marks) = e["marks"].as_object() {
        if !marks.is_empty() {
            let mut ex = m.extra.take().map(|b| *b).unwrap_or_default();
            for (k, v) in marks {
                let v: Value = serde_json::from_value(v.clone()).map_err(|x| x.to_string())?;
                ex.set(Name::from(k.as_str()), v);
            }
            m.extra = Some(Box::new(ex));
        }
    }
    Ok(m)
}

pub fn decode_value(v: &J) -> Value {
    serde_json::from_value(v.clone()).unwrap_or_else(|e| panic!("value {v}: {e}"))
}

pub fn num(v: &J) -> Option<f64> {
    match v {
        J::Number(n) => n.as_f64(),
        J::Object(o) => match o.get("$float").and_then(J::as_str) {
            Some("nan") => Some(f64::NAN),
            Some("inf") => Some(f64::INFINITY),
            Some("-inf") => Some(f64::NEG_INFINITY),
            Some("-0") => Some(-0.0),
            _ => None,
        },
        _ => None,
    }
}

pub fn json_val(v: &J) -> Val {
    match v {
        J::Bool(b) => Val::Bool(*b),
        J::String(s) => Val::Str(s.as_str().into()),
        other => num(other).map_or(Val::Nil, Val::Num),
    }
}

fn opt_name(v: &J) -> Option<Name> {
    v.as_str().map(Name::from)
}

fn scalars(t: &Table, v: &J) {
    if let Some(o) = v.as_object() {
        for (k, x) in o {
            t.set(Name::from(k.as_str()), json_val(x));
        }
    }
}

/// Tag keys PoB writes at runtime that the frozen `Tag` census lacks (CCR-0014 §3).
pub const KNOWN_TAG_GAPS: &[&str] = &["warcryPowerBonus"];

pub struct Build {
    pub mods: Vec<Option<ModRef>>,
    pub mod_id: HashMap<*const Mod, usize>,
    pub stores: Vec<ModStoreRef>,
    pub labels: Vec<String>,
    pub cfgs: Vec<Option<Cfg>>,
    pub cfg_names: Vec<String>,
    pub actors: Vec<ActorRef>,
    pub items: Vec<ItemRef>,
    pub undecodable: usize,
    pub known_undecodable: usize,
    /// stores whose actor has no `activeSkillList` in Lua (the enemy; CCR-0014 §2)
    pub no_skill_list: HashSet<usize>,
    /// stores whose actor has no `itemList` in Lua (CCR-0015 §3)
    pub no_item_list: HashSet<usize>,
    /// stores whose actor has a minionData without monsterTags (CCR-0015 §3)
    pub no_monster_tags: HashSet<usize>,
    /// stores whose actor has no `output` table in Lua (CCR-0015 §3)
    pub no_output: HashSet<usize>,
    /// stores with a MonsterTag-tagged mod anywhere in the chain (narrows the CCR-0015 §3
    /// `monsterTags` known difference to queries that can evaluate a MonsterTag)
    pub monster_tag_chain: Vec<bool>,
    /// names with an undecodable mod anywhere in the store's chain
    pub undecodable_names: Vec<HashSet<Name>>,
    /// cfgs whose skillGrantedEffect is not `data.skills[id]` (a copy)
    pub ge_copies: usize,
    /// tag types of the dumped mods (coverage report)
    pub tag_types: std::collections::BTreeMap<String, usize>,
}

pub fn load(doc: &J) -> Build {
    let mut undecodable = 0;
    let mut known_undecodable = 0;
    let mut mods: Vec<Option<ModRef>> = Vec::new();
    let mut mod_id = HashMap::new();
    let mut tag_types = std::collections::BTreeMap::new();
    for (i, e) in doc["mods"].as_array().unwrap().iter().enumerate() {
        match decode_mod(e) {
            Ok(m) => {
                for t in &m.tags {
                    *tag_types.entry(t.r#type.as_str().to_owned()).or_default() += 1;
                }
                let m = Arc::new(m);
                mod_id.insert(Arc::as_ptr(&m), i + 1);
                mods.push(Some(m));
            }
            Err(err) => {
                if KNOWN_TAG_GAPS
                    .iter()
                    .any(|k| err.contains(&format!("\"{k}\"")))
                {
                    known_undecodable += 1;
                } else {
                    undecodable += 1;
                    if undecodable <= 3 {
                        eprintln!("  undecodable mod {}: {err}", i + 1);
                    }
                }
                mods.push(None);
            }
        }
    }
    let sj = doc["stores"].as_array().unwrap();
    let stores: Vec<ModStoreRef> = sj
        .iter()
        .map(|s| match s["kind"].as_str().unwrap() {
            "ModDB" => ModStore::ModDB(None),
            _ => ModStore::ModList(None),
        })
        .collect();
    let labels = sj
        .iter()
        .map(|s| s["label"].as_str().unwrap_or("?").to_owned())
        .collect();
    let mut own_bad: Vec<HashSet<Name>> = vec![HashSet::new(); sj.len()];
    let mut own_monster: Vec<bool> = vec![false; sj.len()];
    for (i, s) in sj.iter().enumerate() {
        let store = &stores[i];
        if let Some(p) = s["parent"].as_u64() {
            store.parent.set(Some(stores[p as usize - 1].clone()));
        }
        scalars(&store.conditions, &s["conditions"]);
        scalars(&store.multipliers, &s["multipliers"]);
        let mut add = |name: Name, id: &J| match mods[id.as_u64().unwrap() as usize - 1].clone() {
            Some(m) => {
                assert_eq!(m.name, name, "mod stored under another name");
                if m.tags.iter().any(|t| t.r#type.as_str() == "MonsterTag") {
                    own_monster[i] = true;
                }
                store.AddMod(m);
            }
            None => {
                own_bad[i].insert(name);
            }
        };
        if let Some(b) = s["buckets"].as_array() {
            for bucket in b {
                let name = Name::from(bucket[0].as_str().unwrap());
                for id in bucket[1].as_array().unwrap() {
                    add(name, id);
                }
            }
        } else {
            for id in s["list"].as_array().unwrap() {
                let name = mods[id.as_u64().unwrap() as usize - 1]
                    .as_ref()
                    .map_or_else(|| Name::from("?"), |m| m.name);
                add(name, id);
            }
        }
    }
    let mut undecodable_names = Vec::new();
    let mut monster_tag_chain = Vec::new();
    for (i, s) in sj.iter().enumerate() {
        let mut set = own_bad[i].clone();
        let mut monster = own_monster[i];
        let mut p = s["parent"].as_u64();
        while let Some(pi) = p {
            set.extend(own_bad[pi as usize - 1].iter().copied());
            monster |= own_monster[pi as usize - 1];
            p = sj[pi as usize - 1]["parent"].as_u64();
        }
        undecodable_names.push(set);
        monster_tag_chain.push(monster);
    }

    // items
    let items: Vec<ItemRef> = doc["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|it| {
            let item = Item::default();
            item.name.set(it["name"].as_str().unwrap_or("").into());
            item.r#type
                .set(Name::from(it["type"].as_str().unwrap_or("")));
            item.rarity
                .set(Name::from(it["rarity"].as_str().unwrap_or("")));
            // Lua `item.corrupted` is true or nil (Item.lua:551)
            item.corrupted.set(it["corrupted"].as_bool() == Some(true));
            for k in ["shaper", "elder"] {
                if let Some(b) = it[k].as_bool() {
                    item.influences.set(Name::from(k), b);
                }
            }
            // The enabled mod lines (l3_full.lua filters `disabled` / CheckModLineVariant), read
            // by Item:FindModifierSubstring for ItemCondition `searchCond` tags.
            let lists = [
                ("explicitModLines", &item.explicitModLines),
                ("enchantModLines", &item.enchantModLines),
                ("scourgeModLines", &item.scourgeModLines),
                ("implicitModLines", &item.implicitModLines),
                ("crucibleModLines", &item.crucibleModLines),
            ];
            for (key, field) in lists {
                let dst = field.get();
                for l in it[key].as_array().into_iter().flatten() {
                    let t = Table::new();
                    t.set(n!("line"), json_val(l));
                    dst.push(t);
                }
            }
            Rc::new(item)
        })
        .collect();

    // actors
    let aj = doc["actors"].as_array().unwrap();
    let actors: Vec<ActorRef> = aj.iter().map(|_| Actor::new()).collect();
    let mut no_skill_list = HashSet::new();
    let mut no_item_list = HashSet::new();
    let mut no_monster_tags = HashSet::new();
    let mut no_output = HashSet::new();
    let skill_ge = pob_gamedata::data().skills.values().next().unwrap().clone();
    let at = |v: &J| v.as_u64().map(|i| actors[i as usize - 1].clone());
    for (i, a) in aj.iter().enumerate() {
        let actor = &actors[i];
        if let Some(db) = a["modDB"].as_u64() {
            actor.modDB.set(stores[db as usize - 1].clone());
        }
        if let Some(x) = at(&a["player"]) {
            actor.player.set(Rc::downgrade(&x));
        }
        if let Some(x) = at(&a["enemy"]) {
            actor.enemy.set(Rc::downgrade(&x));
        }
        if let Some(x) = at(&a["parent"]) {
            actor.parent.set(Rc::downgrade(&x));
        }
        if let Some(x) = at(&a["partyMembers"]) {
            actor.partyMembers.set(Rc::downgrade(&x));
        }
        // `minion` is a strong handle in the frozen Actor; links back are Weak, so no cycle.
        actor.minion.set(at(&a["minion"]));
        scalars(&actor.weaponData1.get(), &a["weaponData1"]);
        scalars(&actor.weaponData2.get(), &a["weaponData2"]);
        if let Some(il) = a["itemList"].as_object() {
            actor.itemList.with_mut(|l| {
                for (slot, id) in il {
                    l.insert(
                        Name::from(slot.as_str()),
                        items[id.as_u64().unwrap() as usize - 1].clone(),
                    );
                }
            });
        }
        if let Some(k) = a["minionData"].as_str() {
            let md = pob_gamedata::data()
                .minions
                .get(&Name::from(k))
                .unwrap_or_else(|| panic!("minionData {k} not in data.minions"))
                .clone();
            actor.minionData.set(Some(md));
        }
        scalars(&actor.output.get(), &a["output"]);
        if let Some(e) = num(&a["ManaEfficiency"]) {
            actor.extra.set(n!("ManaEfficiency"), e);
        }
        let list = actor.activeSkillList.get();
        for sk in a["skills"].as_array().unwrap() {
            let s = ActiveSkill::new(
                SkillEffect::new(skill_ge.clone(), 1.0, 0.0),
                Shared::new(Vec::new()),
                Rc::downgrade(actor),
            );
            s.skillTypes.get().with_mut(|t| {
                t.set(
                    SkillType::HasReservation,
                    sk["hasReservation"].as_bool().unwrap(),
                );
            });
            s.skillFlags
                .get()
                .set(n!("disable"), sk["disable"].as_bool().unwrap());
            let bl = s.buffList.get();
            for bn in sk["buffNames"].as_array().unwrap() {
                let b = Table::new();
                if let Some(n) = bn.as_str() {
                    b.set(n!("name"), n);
                }
                bl.push(b);
            }
            for k in ["ManaReservedBase", "LifeReservedBase"] {
                if let Some(x) = num(&sk[k]) {
                    s.skillData.get().set(Name::from(k), x);
                }
            }
            list.push(s);
        }
        for (si, s) in sj.iter().enumerate() {
            if s["actor"].as_u64() == Some(i as u64 + 1) {
                stores[si].actor.set(Rc::downgrade(actor));
                if a["hasSkillList"].as_bool() == Some(false) {
                    no_skill_list.insert(si);
                }
                if a["itemList"].is_null() {
                    no_item_list.insert(si);
                }
                if a["minionData"].is_string() && a["minionHasTags"].as_bool() == Some(false) {
                    no_monster_tags.insert(si);
                }
                if a["hasOutput"].as_bool() == Some(false) {
                    no_output.insert(si);
                }
            }
        }
    }

    // cfgs
    let mut ge_copies = 0;
    let mut cfg_names = Vec::new();
    let cfgs = doc["cfgs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            cfg_names.push(c["name"].as_str().unwrap_or("?").to_owned());
            if c["nil"].as_bool() == Some(true) {
                return None;
            }
            let cfg = ModCfg::new();
            if let Some(f) = num(&c["flags"]) {
                cfg.flags.set(f as u32);
            }
            if let Some(f) = num(&c["keywordFlags"]) {
                cfg.keywordFlags.set(f as u32);
            }
            cfg.source.set(opt_name(&c["source"]));
            cfg.actor.set(opt_name(&c["actor"]));
            cfg.skillName.set(opt_name(&c["skillName"]));
            cfg.summonSkillName.set(opt_name(&c["summonSkillName"]));
            if c["skillStats"].is_object() {
                let t = Table::new();
                scalars(&t, &c["skillStats"]);
                cfg.skillStats.set(Some(t));
            }
            if c["skillCond"].is_object() {
                let t = Table::new();
                scalars(&t, &c["skillCond"]);
                cfg.skillCond.set(Some(t));
            }
            if let Some(st) = c["skillTypes"].as_array() {
                let mut set = SkillTypeSet::default();
                for t in st {
                    set.set(t.as_u64().unwrap() as SkillType::T, true);
                }
                cfg.skillTypes.set(Some(Shared::new(set)));
            }
            if let Some(id) = c["skillGrantedEffect"].as_str() {
                if c["skillGrantedEffectIsData"].as_bool() != Some(true) {
                    ge_copies += 1;
                }
                cfg.skillGrantedEffect
                    .set(pob_gamedata::data().skills.get(&Name::from(id)).cloned());
            }
            if let Some(id) = c["skillGem"].as_str() {
                cfg.skillGem.set(Some(
                    pob_gamedata::data()
                        .gems
                        .get(&Name::from(id))
                        .unwrap_or_else(|| panic!("gem {id} not in data.gems"))
                        .clone(),
                ));
            }
            cfg.skillPart.set(num(&c["skillPart"]));
            cfg.skillDist.set(num(&c["skillDist"]));
            cfg.slotName.set(opt_name(&c["slotName"]));
            cfg.socketColor.set(opt_name(&c["socketColor"]));
            cfg.socketNum.set(num(&c["socketNum"]));
            cfg.strengthGems.set(num(&c["strengthGems"]));
            cfg.dexterityGems.set(num(&c["dexterityGems"]));
            cfg.intelligenceGems.set(num(&c["intelligenceGems"]));
            if let Some(i) = c["item"].as_u64() {
                cfg.item.set(Some(items[i as usize - 1].clone()));
            }
            if let Some(bf) = c["baseFlags"].as_array() {
                let set: ahash::AHashSet<Name> =
                    bf.iter().map(|x| Name::from(x.as_str().unwrap())).collect();
                cfg.baseFlags.set(Some(Arc::new(set)));
            }
            Some(cfg)
        })
        .collect();
    Build {
        mods,
        mod_id,
        stores,
        labels,
        cfgs,
        cfg_names,
        actors,
        items,
        undecodable,
        known_undecodable,
        no_skill_list,
        no_item_list,
        no_monster_tags,
        no_output,
        monster_tag_chain,
        undecodable_names,
        ge_copies,
        tag_types,
    }
}
