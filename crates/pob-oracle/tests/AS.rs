//! Unit AS acceptance (calc doc §11.1, row AS): `createActiveSkill`, `buildActiveSkillModList`
//! and `createMinionSkills` against PoB itself.
//!
//! * `as_oracle`: PoB booted once under LuaJIT with the JIT off (`JitPob::boot_oracle`, A6). The
//!   same cases run through PoB's `calcs.createActiveSkill` → `calcs.buildActiveSkillModList` →
//!   (minion) `calcs.createMinionSkills` and through the Rust port, on a minimal env (player,
//!   enemy, build stand-in), and every result is serialised identically on both sides:
//!   skillTypes, skillFlags, skillData, effectList, the skill / weapon cfgs, weapon flags,
//!   disable reason, part / stage / mine / totem state, triggeredBy, the skill mod list **in
//!   order**, the buff list (with its mod lists), the minion (type, level, weapon data, uses,
//!   every minion skill recursively), the srcInstance writes and `env.auxSkillList` /
//!   `ActiveMineCount`. Cases:
//!   - `corpus`: every socket group of the 57 corpus builds (`.parity/builds`), each active gem
//!     with the group's supports at the XML levels/qualities, under 5 weapon set-ups;
//!   - `synthetic`: every non-support granted effect of `data.skills`, no supports, one weapon
//!     set-up each, every 3rd with extra player mods (ExtraSkillMod, SkillData, ExtraSkillStat,
//!     SupportedGemProperty, ExtraMinionSkill, a conditional DisableSkill), every 4th in CALCS mode
//!     as the main skill, with skill part / stage / mine counts on the gem instance.
//!   A mod list that matches only as a multiset is counted as `LUA-ORDER` (PoB adds stat-map mods
//!   in `pairs` order over a string-keyed table, random per process; calc doc §3.4/§9.5). A
//!   `Skill:<id>AltX/AltY` source is PoB's per-process variant choice (CCR-0010) and compared
//!   with the suffix dropped; a formatted `-0` vs `0` is the CCR-0002 host artefact.
//! * `as_l4_corpus`: L4 on the probe dumps `.parity/probe/<stem>/state/buildActiveSkillModList:in#n`:
//!   inject, run the Rust `buildActiveSkillModList`, diff the whole state against `:out#n`.
//! * `as_census`: the data facts the port relies on (`minionList = { }` only on Raise Spectre;
//!   every support has `addSkillTypes`).
//!
//! Inputs missing skip with a message; with `POB_ORACLE_REQUIRE=1` (or `CI`) they fail.
#![allow(non_snake_case)]
#![allow(clippy::too_many_lines, clippy::format_push_string)]

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use pob_calc::modules::calc_active_skill::{
    buildActiveSkillModList, createActiveSkill, createMinionSkills, triggeredBy, LifeTable,
};
use pob_calc::prelude::*;
use pob_gamedata::mod_tools::{createMod, formatMod};
use pob_gamedata::ValueTable;
use pob_oracle::headless::{Engine, Ret};
use pob_oracle::jit::JitPob;
use pob_oracle::oracle::{find_parity_dir, find_pob_src, find_up};

fn skip(why: &str) {
    let required = std::env::var_os("POB_ORACLE_REQUIRE").is_some_and(|v| v == "1")
        || std::env::var_os("CI").is_some_and(|v| !v.is_empty() && v != "false" && v != "0");
    assert!(!required, "required input missing: {why}");
    eprintln!("SKIPPED: {why}");
}

fn init_data() -> bool {
    pob_oracle::testkit::init_default_data()
}

// ---------------------------------------------------------------------------------------------
// cases

#[derive(Clone)]
struct Gem {
    id: String,
    level: f64,
    quality: f64,
}

#[derive(Clone)]
struct Case {
    name: String,
    active: Gem,
    supports: Vec<Gem>,
    weapon: u32,
    calcs: bool,
    rich: bool,
    part: Option<f64>,
    stage: Option<f64>,
    mine: Option<f64>,
}

const WEAPONS: u32 = 5;

/// Socket groups of the corpus builds: (build stem, [gems]).
fn corpus_groups(parity: &Path) -> Vec<(String, Vec<Gem>)> {
    let mut out = Vec::new();
    let mut files: Vec<PathBuf> = std::fs::read_dir(parity.join("builds"))
        .map(|rd| rd.filter_map(Result::ok).map(|e| e.path()).collect())
        .unwrap_or_default();
    files.retain(|p| p.extension().is_some_and(|e| e == "xml"));
    files.sort();
    let attr = |tag: &str, k: &str| -> Option<String> {
        let pat = format!(" {k}=\"");
        let i = tag.find(&pat)? + pat.len();
        let j = tag[i..].find('"')? + i;
        Some(tag[i..j].to_owned())
    };
    for f in files {
        let Ok(text) = std::fs::read_to_string(&f) else {
            continue;
        };
        let stem = f.file_stem().unwrap().to_string_lossy().into_owned();
        for grp in text.split("<Skill ").skip(1) {
            let body = grp.split("</Skill>").next().unwrap_or("");
            let mut gems = Vec::new();
            for g in body.split("<Gem ").skip(1) {
                let tag = g.split('>').next().unwrap_or("");
                let tag = format!(" {tag}");
                let Some(id) = attr(&tag, "skillId") else {
                    continue;
                };
                if !data().skills.contains_key(&Name::from(id.as_str())) {
                    continue;
                }
                let level = attr(&tag, "level")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(20.0);
                let quality = attr(&tag, "quality")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0.0);
                gems.push(Gem { id, level, quality });
            }
            if !gems.is_empty() {
                out.push((stem.clone(), gems));
            }
        }
    }
    out
}

fn build_cases(parity: Option<&Path>) -> Vec<Case> {
    let mut cases = Vec::new();
    if let Some(parity) = parity {
        for (gi, (stem, gems)) in corpus_groups(parity).into_iter().enumerate() {
            let is_sup = |g: &Gem| data().skills[&Name::from(g.id.as_str())].support;
            let supports: Vec<Gem> = gems.iter().filter(|g| is_sup(g)).cloned().collect();
            for a in gems.iter().filter(|g| !is_sup(g)) {
                for w in 0..WEAPONS {
                    cases.push(Case {
                        name: format!("corpus {stem} group {gi} {} weapon {w}", a.id),
                        active: a.clone(),
                        supports: supports.clone(),
                        weapon: w,
                        calcs: false,
                        rich: false,
                        part: None,
                        stage: None,
                        mine: None,
                    });
                }
            }
        }
    }
    let mut ids: Vec<Name> = data()
        .skills
        .iter()
        .filter(|(_, g)| !g.support)
        .map(|(k, _)| *k)
        .collect();
    ids.sort_by(|a, b| a.as_str().as_bytes().cmp(b.as_str().as_bytes()));
    for (i, id) in ids.into_iter().enumerate() {
        cases.push(Case {
            name: format!("synthetic {id}"),
            active: Gem {
                id: id.as_str().to_owned(),
                level: 20.0,
                quality: 20.0,
            },
            supports: Vec::new(),
            weapon: (i as u32) % WEAPONS,
            calcs: i % 4 == 0,
            rich: i % 3 == 0,
            part: (i % 2 == 0).then_some(2.0),
            stage: (i % 5 == 0).then_some(3.0),
            mine: (i % 3 == 1).then_some(2.0),
        });
    }
    cases
}

fn lua_str(s: &str) -> String {
    format!("{s:?}")
}

fn lua_opt(v: Option<f64>) -> String {
    v.map_or("nil".to_owned(), |x| format!("{x:?}"))
}

fn cases_to_lua(cases: &[Case]) -> String {
    let mut s = String::from("{\n");
    for c in cases {
        let sups: Vec<String> = c
            .supports
            .iter()
            .map(|g| format!("{{{},{:?},{:?}}}", lua_str(&g.id), g.level, g.quality))
            .collect();
        s.push_str(&format!(
            "{{a={{{},{:?},{:?}}},s={{{}}},w={},calcs={},rich={},part={},stage={},mine={}}},\n",
            lua_str(&c.active.id),
            c.active.level,
            c.active.quality,
            sups.join(","),
            c.weapon,
            c.calcs,
            c.rich,
            lua_opt(c.part),
            lua_opt(c.stage),
            lua_opt(c.mine),
        ));
    }
    s.push('}');
    s
}

/// The two spectres of the stand-in build's `spectreList` (sorted `data.spectres` keys).
fn spectre_list() -> Vec<Name> {
    let mut v: Vec<Name> = data().spectres.keys().copied().collect();
    v.sort_by(|a, b| a.as_str().as_bytes().cmp(b.as_str().as_bytes()));
    v.truncate(2);
    v
}

// ---------------------------------------------------------------------------------------------
// the Lua side

const LUA_RUN: &str = r#"
require("Modules.Calcs")
local calcs = require("Modules.CalcBase")
local function num(v) return string.format("%.17g", v) end
local function sval(v)
  local t = type(v)
  if t == "number" then return num(v) elseif t == "boolean" or t == "nil" then return tostring(v) elseif t == "string" then return v else return "table" end
end
local function kv(t)
  if not t then return "nil" end
  local ks = {}
  for k, v in pairs(t) do ks[#ks+1] = (type(k) == "number" and num(k) or tostring(k)) .. "=" .. sval(v) end
  table.sort(ks)
  return table.concat(ks, ",")
end
local function cfgs(c)
  if not c then return "nil" end
  return table.concat({ sval(c.flags), sval(c.keywordFlags), sval(c.skillName), sval(c.summonSkillName), sval(c.skillPart),
    sval(c.skillDist), sval(c.slotName), sval(c.skillCond and c.skillCond.MainHandAttack), sval(c.skillCond and c.skillCond.OffHandAttack),
    sval(c.skillGrantedEffect and c.skillGrantedEffect.id), sval(c.skillTypes and c.skillTypes == nil) }, ";")
end
local function mods(ml)
  local out = {}
  for i = 1, #ml do local m = ml[i]; out[#out+1] = modLib.formatMod(m) .. "|" .. tostring(m.source) end
  return table.concat(out, "\31")
end
local ser
ser = function(s, depth)
  local p = {}
  local st = {} for k in pairs(s.skillTypes) do st[#st+1] = k end table.sort(st)
  p[#p+1] = "types=" .. table.concat(st, ",")
  local ef = {} for _, e in ipairs(s.effectList) do ef[#ef+1] = e.grantedEffect.id end
  p[#p+1] = "effects=" .. table.concat(ef, ",")
  p[#p+1] = "flags=" .. kv(s.skillFlags)
  p[#p+1] = "data=" .. kv(s.skillData)
  p[#p+1] = "cfg=" .. cfgs(s.skillCfg)
  p[#p+1] = "w1cfg=" .. cfgs(s.weapon1Cfg)
  p[#p+1] = "w2cfg=" .. cfgs(s.weapon2Cfg)
  p[#p+1] = "wflags=" .. sval(s.weapon1Flags) .. "," .. sval(s.weapon2Flags)
  p[#p+1] = "dis=" .. sval(s.disableReason)
  p[#p+1] = "part=" .. sval(s.skillPart) .. "," .. sval(s.skillPartName)
  p[#p+1] = "counts=" .. sval(s.skillTotemId) .. "," .. sval(s.activeMineCount or nil) .. "," .. sval(s.activeStageCount)
  p[#p+1] = "trig=" .. sval(s.triggeredBy and s.triggeredBy.grantedEffect.id)
  p[#p+1] = "extra=" .. (s.extraSkillModList and #s.extraSkillModList or 0)
  p[#p+1] = "minionList=" .. table.concat(s.minionList or {}, ",")
  local ae = s.activeEffect
  p[#p+1] = "ae=" .. sval(ae.level) .. "," .. sval(ae.quality) .. "," .. sval(ae.supportQuality) .. "," .. sval(ae.actorLevel) .. "," .. sval(ae.grantedEffectLevel and (ae.grantedEffectLevel.levelRequirement or 0))
  p[#p+1] = "mods=" .. mods(s.skillModList)
  local bl = {}
  for _, b in ipairs(s.buffList) do
    local c = {} for k, v in pairs(b) do if k ~= "modList" then c[k] = v end end
    bl[#bl+1] = kv(c) .. "[" .. mods(b.modList) .. "]"
  end
  p[#p+1] = "buffs=" .. table.concat(bl, "\29")
  if s.minion and depth == 0 then
    local m = s.minion
    local uses = {} for k in pairs(m.uses or {}) do uses[#uses+1] = k end table.sort(uses)
    p[#p+1] = "minion=" .. sval(m.type) .. "," .. sval(m.level) .. "," .. sval(m.hostile) .. ",wd1{" .. kv(m.weaponData1) .. "},wd2{" .. kv(m.weaponData2) .. "},uses{" .. table.concat(uses, ",") .. "}," .. #(m.activeSkillList or {}) .. "," .. sval(m.mainSkill and m.mainSkill.activeEffect.grantedEffect.id)
    for i, ms in ipairs(m.activeSkillList or {}) do
      p[#p+1] = "mskill" .. i .. "{" .. ser(ms, 1) .. "}"
    end
  end
  return table.concat(p, "\n")
end
local spectres = {} for k in pairs(data.spectres) do spectres[#spectres+1] = k end table.sort(spectres)
local spectreList = { spectres[1], spectres[2] }
local function weapons(actor, w)
  actor.itemList = {}
  if w == 0 then actor.weaponData1 = { type = "None" } actor.weaponData2 = {}
  elseif w == 1 then actor.weaponData1 = { type = "Bow", AttackRate = 1.4 } actor.weaponData2 = {} actor.itemList["Weapon 1"] = { type = "Bow" }
  elseif w == 2 then actor.weaponData1 = { type = "One Handed Sword", AttackRate = 1.5 } actor.weaponData2 = {} actor.itemList["Weapon 1"] = { type = "One Handed Sword" } actor.itemList["Weapon 2"] = { type = "Shield" }
  elseif w == 3 then actor.weaponData1 = { type = "Claw", AttackRate = 1.6 } actor.weaponData2 = { type = "Claw", AttackRate = 1.6 } actor.itemList["Weapon 1"] = { type = "Claw" } actor.itemList["Weapon 2"] = { type = "Claw" }
  else actor.weaponData1 = { type = "Staff", AttackRate = 1.2 } actor.weaponData2 = {} actor.itemList["Weapon 1"] = { type = "Staff" } end
end
local function richMods(modDB)
  modDB:AddMod(modLib.createMod("ExtraSkillMod", "LIST", { mod = modLib.createMod("Damage", "INC", 10, "Test") }, "Test"))
  modDB:AddMod(modLib.createMod("SkillData", "LIST", { key = "buffNotPlayer", value = true }, "Test"))
  modDB:AddMod(modLib.createMod("ExtraSkillStat", "LIST", { key = "base_skill_area_of_effect_+%", value = 5 }, "Test"))
  modDB:AddMod(modLib.createMod("SupportedGemProperty", "LIST", { keyword = "grants_active_skill", key = "quality", value = 5 }, "Test"))
  modDB:AddMod(modLib.createMod("ExtraMinionSkill", "LIST", { skillId = "MinionInstability" }, "Test"))
  modDB:AddMod(modLib.createMod("DisableSkill", "FLAG", true, "Test", { type = "SkillType", skillType = SkillType.Cold }))
end
local out = {}
for ci, c in ipairs(CASES) do
  local ok, r = pcall(function()
    local env = { mode = c.calcs and "CALCS" or "MAIN", mode_buffs = true, mode_combat = true, mode_effective = true, data = data,
      configInput = {}, configPlaceholder = { meleeDistance = 15, projectileDistance = 30 },
      build = { characterLevel = 90, spectreList = spectreList, itemsTab = { itemSets = { [1] = { ["Weapon 1"] = { selItemId = 0 }, ["Weapon 2"] = { selItemId = 0 } } }, itemSetOrderList = { 1 }, items = {} } },
      auxSkillList = {}, enemyLevel = 84 }
    env.player = { level = 90 }
    env.player.modDB = new("ModDB"):ModDB() env.player.modDB.actor = env.player
    env.enemy = { level = 84 }
    env.enemy.modDB = new("ModDB"):ModDB() env.enemy.modDB.actor = env.enemy
    env.player.enemy = env.enemy env.enemy.enemy = env.player
    env.modDB = env.player.modDB
    weapons(env.player, c.w)
    if c.rich then richMods(env.player.modDB) end
    local ge = data.skills[c.a[1]]
    local gemId = data.gemForSkill[ge]
    local gemData = gemId and data.gems[gemId] or nil
    local src = { level = c.a[2], quality = c.a[3], gemData = gemData, skillPart = c.part, skillPartCalcs = c.part,
      skillStageCount = c.stage, skillStageCountCalcs = c.stage, skillMineCount = c.mine, skillMineCountCalcs = c.mine }
    local ae = { grantedEffect = ge, level = c.a[2], quality = c.a[3], srcInstance = src, gemData = gemData }
    local supports = {}
    for _, s in ipairs(c.s) do
      local sge = data.skills[s[1]]
      local sid = data.gemForSkill[sge]
      supports[#supports+1] = { grantedEffect = sge, level = s[2], quality = s[3], gemData = sid and data.gems[sid] or nil, isSupporting = {} }
    end
    local skill = calcs.createActiveSkill(ae, supports, env.player, nil, nil)
    if c.calcs then env.player.mainSkill = skill end
    calcs.buildActiveSkillModList(env, skill)
    if skill.minion then
      skill.minion.modDB = new("ModDB"):ModDB()
      skill.minion.modDB.actor = skill.minion
      calcs.createMinionSkills(env, skill)
    end
    return ser(skill, 0) .. "\nsrc=" .. kv(src) .. "\nenv=" .. #env.auxSkillList .. "," .. sval(env.enemy.modDB.multipliers.ActiveMineCount)
  end)
  out[ci] = ok and r or ("ERR " .. tostring(r))
end
return table.concat(out, "\30")
"#;

// ---------------------------------------------------------------------------------------------
// the Rust side

fn num(v: f64) -> String {
    lfmt!("%.17g", v)
}

fn sval(v: &Val) -> String {
    match v {
        Val::Num(n) => num(*n),
        Val::Bool(b) => b.to_string(),
        Val::Nil => "nil".to_owned(),
        Val::Str(s) => s.to_string(),
        _ => "table".to_owned(),
    }
}

fn so(v: Option<f64>) -> String {
    v.map_or("nil".to_owned(), num)
}

fn ss(v: Option<&str>) -> String {
    v.map_or("nil".to_owned(), str::to_owned)
}

fn key_str(k: &Key) -> String {
    match k {
        Key::S(n) => n.as_str().to_owned(),
        Key::I(i) => num(*i as f64),
        Key::B(b) => b.to_string(),
    }
}

fn kv(t: &Table, skip_key: Option<&str>) -> String {
    let mut ks: Vec<String> = t
        .pairs()
        .filter(|(k, _)| skip_key.map_or(true, |s| key_str(k) != s))
        .map(|(k, v)| format!("{}={}", key_str(&k), sval(&v)))
        .collect();
    ks.sort();
    ks.join(",")
}

fn cfgs(c: Option<Cfg>) -> String {
    let Some(c) = c else {
        return "nil".to_owned();
    };
    let cond = |k: Name| c.skillCond.get().map_or(Val::Nil, |t| t.get(k));
    [
        num(f64::from(c.flags.get())),
        num(f64::from(c.keywordFlags.get())),
        ss(c.skillName.get().as_ref().map(|n| n.as_str())),
        ss(c.summonSkillName.get().as_ref().map(|n| n.as_str())),
        so(c.skillPart.get()),
        so(c.skillDist.get()),
        ss(c.slotName.get().as_ref().map(|n| n.as_str())),
        sval(&cond(n!("MainHandAttack"))),
        sval(&cond(n!("OffHandAttack"))),
        ss(c.skillGrantedEffect.get().as_ref().map(|g| g.id.as_str())),
        "false".to_owned(),
    ]
    .join(";")
}

fn mods_of(ms: &[ModRef]) -> String {
    ms.iter()
        .map(|m| format!("{}|{}", formatMod(m), m.source.as_deref().unwrap_or("nil")))
        .collect::<Vec<_>>()
        .join("\x1f")
}

fn ser(s: &ActiveSkillRef, depth: u32) -> String {
    let mut p: Vec<String> = Vec::new();
    let st: Vec<String> = s
        .skillTypes
        .get()
        .with(|t| t.iter().map(|x| x.to_string()).collect());
    let mut st_n: Vec<u32> = st.iter().map(|x| x.parse().unwrap()).collect();
    st_n.sort_unstable();
    p.push(format!(
        "types={}",
        st_n.iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",")
    ));
    let ef: Vec<String> = s.effectList.get().with(|v| {
        v.iter()
            .map(|e| e.grantedEffect.get().id.as_str().to_owned())
            .collect()
    });
    p.push(format!("effects={}", ef.join(",")));
    p.push(format!("flags={}", kv(&s.skillFlags.get(), None)));
    p.push(format!("data={}", kv(&s.skillData.get(), None)));
    p.push(format!("cfg={}", cfgs(Some(s.skillCfg.get()))));
    p.push(format!("w1cfg={}", cfgs(s.weapon1Cfg.get())));
    p.push(format!("w2cfg={}", cfgs(s.weapon2Cfg.get())));
    p.push(format!(
        "wflags={},{}",
        so(s.weapon1Flags.get().map(f64::from)),
        so(s.weapon2Flags.get().map(f64::from))
    ));
    p.push(format!("dis={}", ss(s.disableReason.get().as_deref())));
    p.push(format!(
        "part={},{}",
        so(s.skillPart.get()),
        ss(s.skillPartName.get().as_deref())
    ));
    p.push(format!(
        "counts={},{},{}",
        so(s.skillTotemId.get()),
        so(s.activeMineCount.get()),
        so(s.activeStageCount.get())
    ));
    p.push(format!(
        "trig={}",
        ss(triggeredBy(s)
            .map(|e| e.grantedEffect.get().id)
            .as_ref()
            .map(|n| n.as_str()))
    ));
    p.push(format!("extra={}", s.extraSkillModList.get().len()));
    p.push(format!(
        "minionList={}",
        s.minionList.get().with(|v| v
            .iter()
            .map(|n| n.as_str().to_owned())
            .collect::<Vec<_>>()
            .join(","))
    ));
    let ae = s.activeEffect.get();
    p.push(format!(
        "ae={},{},{},{},{}",
        num(ae.level.get()),
        num(ae.quality.get()),
        so(ae.supportQuality.get()),
        so(ae.actorLevel.get()),
        so(ae.grantedEffectLevel.get().map(|l| l.levelRequirement))
    ));
    p.push(format!("mods={}", mods_of(&s.skillModList.get().ipairs())));
    let bl: Vec<String> = s
        .buffList
        .get()
        .with(Clone::clone)
        .iter()
        .map(|b| {
            let ml = b.table(n!("modList")).unwrap();
            let ms: Vec<ModRef> = ml
                .ipairs()
                .into_iter()
                .map(|(_, v)| match v {
                    Val::Mod(m) => m,
                    _ => panic!("buff mod"),
                })
                .collect();
            format!("{}[{}]", kv(b, Some("modList")), mods_of(&ms))
        })
        .collect();
    p.push(format!("buffs={}", bl.join("\x1d")));
    if let (Some(m), 0) = (s.minion.get(), depth) {
        let mut uses: Vec<String> = m
            .extra
            .table(n!("uses"))
            .map(|t| t.pairs().map(|(k, _)| key_str(&k)).collect())
            .unwrap_or_default();
        uses.sort();
        let list = m.activeSkillList.get().with(Clone::clone);
        p.push(format!(
            "minion={},{},{},wd1{{{}}},wd2{{{}}},uses{{{}}},{},{}",
            sval(&m.extra.get(n!("type"))),
            num(m.level.get()),
            m.hostile.get(),
            kv(&m.weaponData1.get(), None),
            kv(&m.weaponData2.get(), None),
            uses.join(","),
            list.len(),
            ss(m.mainSkill
                .get()
                .map(|ms| ms.activeEffect.get().grantedEffect.get().id)
                .as_ref()
                .map(|n| n.as_str()))
        ));
        for (i, ms) in list.iter().enumerate() {
            p.push(format!("mskill{}{{{}}}", i + 1, ser(ms, 1)));
        }
    }
    p.join("\n")
}

fn src_kv(src: &GemInstance) -> String {
    let mut ks: Vec<String> = Vec::new();
    let mut pushn = |k: &str, v: Option<f64>| {
        if let Some(v) = v {
            ks.push(format!("{k}={}", num(v)));
        }
    };
    pushn("level", Some(src.level.get()));
    pushn("quality", Some(src.quality.get()));
    pushn("skillPart", src.skillPart.get());
    pushn("skillPartCalcs", src.skillPartCalcs.get());
    pushn("skillStageCount", src.skillStageCount.get());
    pushn("skillStageCountCalcs", src.skillStageCountCalcs.get());
    pushn("skillMineCount", src.skillMineCount.get());
    pushn("skillMineCountCalcs", src.skillMineCountCalcs.get());
    pushn("skillMinionItemSet", src.skillMinionItemSet.get());
    pushn("skillMinionItemSetCalcs", src.skillMinionItemSetCalcs.get());
    pushn("skillMinionSkill", src.skillMinionSkill.get());
    pushn("skillMinionSkillCalcs", src.skillMinionSkillCalcs.get());
    if let Some(m) = src.skillMinion.get() {
        ks.push(format!("skillMinion={m}"));
    }
    if let Some(m) = src.skillMinionCalcs.get() {
        ks.push(format!("skillMinionCalcs={m}"));
    }
    if src.gemData.get().is_some() {
        ks.push("gemData=table".to_owned());
    }
    ks.sort();
    ks.join(",")
}

fn item(ty: &str) -> ItemRef {
    let it = Item::default();
    it.r#type.set(Name::from(ty));
    Rc::new(it)
}

fn wd(ty: &str, rate: Option<f64>) -> Table {
    let t = Table::new();
    t.set(n!("type"), ty);
    if let Some(r) = rate {
        t.set(n!("AttackRate"), r);
    }
    t
}

fn weapons(actor: &ActorRef, w: u32) {
    actor.itemList.with_mut(|m| m.clear());
    let put = |slot: &str, ty: &str| {
        actor
            .itemList
            .with_mut(|m| m.insert(Name::from(slot), item(ty)));
    };
    match w {
        0 => {
            actor.weaponData1.set(wd("None", None));
            actor.weaponData2.set(Table::new());
        }
        1 => {
            actor.weaponData1.set(wd("Bow", Some(1.4)));
            actor.weaponData2.set(Table::new());
            put("Weapon 1", "Bow");
        }
        2 => {
            actor.weaponData1.set(wd("One Handed Sword", Some(1.5)));
            actor.weaponData2.set(Table::new());
            put("Weapon 1", "One Handed Sword");
            put("Weapon 2", "Shield");
        }
        3 => {
            actor.weaponData1.set(wd("Claw", Some(1.6)));
            actor.weaponData2.set(wd("Claw", Some(1.6)));
            put("Weapon 1", "Claw");
            put("Weapon 2", "Claw");
        }
        _ => {
            actor.weaponData1.set(wd("Staff", Some(1.2)));
            actor.weaponData2.set(Table::new());
            put("Weapon 1", "Staff");
        }
    }
}

fn vt(fields: Vec<(Name, Value)>) -> Value {
    Value::Table(Arc::new(ValueTable {
        array: Vec::new(),
        fields,
    }))
}

fn rich_mods(modDB: &ModDB) {
    let list =
        |name: Name, v: Value| Arc::new(createMod(name, ModType::LIST, v, Some("Test"), 0, 0, &[]));
    let dmg = Arc::new(createMod(
        n!("Damage"),
        ModType::INC,
        10.0,
        Some("Test"),
        0,
        0,
        &[],
    ));
    modDB.AddMod(list(
        n!("ExtraSkillMod"),
        vt(vec![(n!("mod"), Value::Mod(dmg))]),
    ));
    modDB.AddMod(list(
        n!("SkillData"),
        vt(vec![
            (n!("key"), Value::Str(AStr::from("buffNotPlayer"))),
            (n!("value"), Value::Bool(true)),
        ]),
    ));
    modDB.AddMod(list(
        n!("ExtraSkillStat"),
        vt(vec![
            (
                n!("key"),
                Value::Str(AStr::from("base_skill_area_of_effect_+%")),
            ),
            (n!("value"), Value::Num(5.0)),
        ]),
    ));
    modDB.AddMod(list(
        n!("SupportedGemProperty"),
        vt(vec![
            (n!("keyword"), Value::Str(AStr::from("grants_active_skill"))),
            (n!("key"), Value::Str(AStr::from("quality"))),
            (n!("value"), Value::Num(5.0)),
        ]),
    ));
    modDB.AddMod(list(
        n!("ExtraMinionSkill"),
        vt(vec![(
            n!("skillId"),
            Value::Str(AStr::from("MinionInstability")),
        )]),
    ));
    let mut tag = Tag::new(TagType::SkillType);
    tag.skillType = Some(SkillType::Cold);
    modDB.AddMod(Arc::new(createMod(
        n!("DisableSkill"),
        ModType::FLAG,
        true,
        Some("Test"),
        0,
        0,
        &[tag],
    )));
}

fn run_rust(c: &Case, build: &BuildRef, spectres: &[Name]) -> String {
    let mode = if c.calcs { Mode::CALCS } else { Mode::MAIN };
    let env = Env::new(
        build,
        mode,
        Rc::new(Override::default()),
        Rc::new(PassiveSpec::default()),
    );
    env.mode_buffs.set(true);
    env.mode_combat.set(true);
    env.mode_effective.set(true);
    env.configPlaceholder.set(n!("meleeDistance"), 15.0);
    env.configPlaceholder.set(n!("projectileDistance"), 30.0);
    build.characterLevel.set(90.0);
    build.spectreList.set(Shared::new(spectres.to_vec()));
    env.enemyLevel.set(84.0);
    env.player.level.set(90.0);
    let pdb = ModStore::ModDB(None);
    pdb.actor.set(Rc::downgrade(&env.player));
    env.player.modDB.set(pdb.clone());
    env.enemy.level.set(84.0);
    let edb = ModStore::ModDB(None);
    edb.actor.set(Rc::downgrade(&env.enemy));
    env.enemy.modDB.set(edb);
    env.player.enemy.set(Rc::downgrade(&env.enemy));
    env.enemy.enemy.set(Rc::downgrade(&env.player));
    env.modDB.set(pdb.clone());
    weapons(&env.player, c.weapon);
    if c.rich {
        rich_mods(&pdb);
    }
    let ge = data().skills[&Name::from(c.active.id.as_str())].clone();
    let gemData = data().gem_for_skill(ge.id).cloned();
    let src = GemInstance::default();
    src.level.set(c.active.level);
    src.quality.set(c.active.quality);
    src.gemData.set(gemData.clone());
    src.skillPart.set(c.part);
    src.skillPartCalcs.set(c.part);
    src.skillStageCount.set(c.stage);
    src.skillStageCountCalcs.set(c.stage);
    src.skillMineCount.set(c.mine);
    src.skillMineCountCalcs.set(c.mine);
    let src = Rc::new(src);
    let ae = SkillEffect::new(ge, c.active.level, c.active.quality);
    ae.srcInstance.set(Some(src.clone()));
    ae.gemData.set(gemData);
    let supports: Vec<SkillEffectRef> = c
        .supports
        .iter()
        .map(|s| {
            let sge = data().skills[&Name::from(s.id.as_str())].clone();
            let e = SkillEffect::new(sge.clone(), s.level, s.quality);
            e.gemData.set(data().gem_for_skill(sge.id).cloned());
            e.isSupporting.set(Some(Shared::new(PtrSet::new())));
            e
        })
        .collect();
    let supports = Shared::new(supports);
    let skill = createActiveSkill(ae, &supports, Rc::downgrade(&env.player), None, None);
    if c.calcs {
        env.player.mainSkill.set(Some(skill.clone()));
    }
    buildActiveSkillModList(&env, &skill);
    if let Some(minion) = skill.minion.get() {
        let mdb = ModStore::ModDB(None);
        mdb.actor.set(Rc::downgrade(&minion));
        minion.modDB.set(mdb);
        createMinionSkills(&env, &skill);
        // the minion's lifeTable is one of the data tables (CCR-0040 representation check)
        assert!(minion.extra.obj::<LifeTable>(n!("lifeTable")).is_some());
    }
    let out = format!(
        "{}\nsrc={}\nenv={},{}",
        ser(&skill, 0),
        src_kv(&src),
        env.auxSkillList.get().len(),
        sval(&env.enemy.modDB.get().multipliers.get(n!("ActiveMineCount")))
    );
    env.player.mainSkill.set(None);
    out
}

/// Lua `pcall` around the Rust side: a `lua_error!` becomes `ERR`; any other panic is a port bug.
fn pcall_rust(c: &Case, build: &BuildRef, spectres: &[Name]) -> String {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_rust(c, build, spectres)
    })) {
        Ok(s) => s,
        Err(p) if p.downcast_ref::<LuaError>().is_some() => "ERR".to_owned(),
        Err(p) => {
            let msg = p
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| p.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                .unwrap_or_default();
            format!("PANIC {msg}")
        }
    }
}

// ---------------------------------------------------------------------------------------------
// comparison

/// `s` with `Skill:<id>AltX` / `Skill:<id>AltY` written as `Skill:<id>` (CCR-0010).
fn strip_alt_source(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find("Skill:") {
        out.push_str(&rest[..i + 6]);
        rest = &rest[i + 6..];
        let end = rest
            .find(|c: char| !c.is_ascii_alphanumeric())
            .unwrap_or(rest.len());
        let id = &rest[..end];
        out.push_str(
            id.strip_suffix("AltX")
                .or_else(|| id.strip_suffix("AltY"))
                .unwrap_or(id),
        );
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// Every standalone `-0` written as `0` (CCR-0002).
fn unsign_zero(s: &str) -> String {
    let b = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'-'
            && b.get(i + 1) == Some(&b'0')
            && !b
                .get(i + 2)
                .is_some_and(|c| c.is_ascii_digit() || *c == b'.')
            && !(i > 0 && (b[i - 1].is_ascii_digit() || b[i - 1] == b'.'))
        {
            i += 1;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The record with every mod list sorted (mods, buff mod lists): equality up to `pairs` order.
fn order_free(s: &str) -> String {
    s.lines()
        .map(|l| {
            if let Some(rest) = l.strip_prefix("mods=") {
                let mut v: Vec<&str> = rest.split('\x1f').collect();
                v.sort_unstable();
                format!("mods={}", v.join("\x1f"))
            } else if let Some(rest) = l.strip_prefix("buffs=") {
                let (rest, close) = match rest.strip_suffix('}') {
                    Some(r) => (r, "}"),
                    None => (rest, ""),
                };
                let mut bs: Vec<String> = rest
                    .split('\x1d')
                    .map(|b| match b.split_once('[') {
                        Some((h, ms)) => {
                            let ms = ms.strip_suffix(']').unwrap_or(ms);
                            let mut v: Vec<&str> = ms.split('\x1f').collect();
                            v.sort_unstable();
                            format!("{h}[{}]", v.join("\x1f"))
                        }
                        None => b.to_owned(),
                    })
                    .collect();
                bs.sort();
                format!("buffs={}{close}", bs.join("\x1d"))
            } else if l.starts_with("mskill") {
                // a minion skill's record is one line: sort its mods part too
                let mut out = String::new();
                for part in l.split('\n') {
                    out.push_str(part);
                }
                out
            } else {
                l.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The mods of every `mods=` line whose source is not a `Skill:` source (the `NewMod`s with
/// literal sources: Max Level, Max Quality, Socket Quality, Base, Damage Fixup, the test's
/// own mods), in order. Their order does not depend on `pairs`, so a `LUA-ORDER` verdict also
/// requires it to match (a swapped `NewMod` statement would otherwise pass as an order-only
/// difference).
fn fixed_order(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    for l in s.lines() {
        if let Some(rest) = l.strip_prefix("mods=") {
            for m in rest.split('\x1f') {
                if !m.contains("|Skill:") {
                    out.push(m.to_owned());
                }
            }
            out.push("--".to_owned());
        }
    }
    out
}

#[derive(Default)]
struct Tally {
    total: usize,
    exact: usize,
    order: usize,
    nondet: usize,
    zero: usize,
    err_both: usize,
    bad: Vec<String>,
}

fn first_diff(a: &str, b: &str) -> String {
    let al: Vec<&str> = a.split('\n').collect();
    let bl: Vec<&str> = b.split('\n').collect();
    for i in 0..al.len().max(bl.len()) {
        let x = al.get(i).copied().unwrap_or("<none>");
        let y = bl.get(i).copied().unwrap_or("<none>");
        if x != y {
            let clip = |s: &str| {
                s.chars()
                    .take(600)
                    .collect::<String>()
                    .replace('\x1f', " ¦ ")
            };
            return format!("    lua  {}\n    rust {}", clip(x), clip(y));
        }
    }
    String::new()
}

#[test]
fn as_oracle() {
    if !init_data() {
        return skip("data/dump not found");
    }
    let Some(src) = find_pob_src() else {
        return skip("PoB checkout not found");
    };
    let parity = find_parity_dir();
    let cases = build_cases(parity.as_deref());
    let corpus_n = cases
        .iter()
        .filter(|c| c.name.starts_with("corpus"))
        .count();
    let _lock = pob_oracle::runner::OracleLock::acquire().unwrap_or_else(|e| panic!("{e}"));
    let t0 = std::time::Instant::now();
    let mut pob = JitPob::boot_oracle(&src, None).unwrap_or_else(|e| panic!("boot: {e}"));
    eprintln!("PoB booted in {:?}", t0.elapsed());
    let code = format!("CASES = {}\n{LUA_RUN}", cases_to_lua(&cases));
    let r = pob.run(&code).unwrap_or_else(|e| panic!("lua: {e}"));
    let lua = match r.first() {
        Some(Ret::Str(s)) => s.clone(),
        other => panic!("lua returned {other:?}"),
    };
    eprintln!("lua side: {} cases in {:?}", cases.len(), t0.elapsed());
    let lua_recs: Vec<&str> = lua.split('\x1e').collect();
    assert_eq!(lua_recs.len(), cases.len(), "record count");

    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let build = Build::new(data());
    {
        // item set 1 with empty weapon slots (the minion item-set path of CalcActiveSkill.lua:735-784)
        let set = Table::new();
        for slot in ["Weapon 1", "Weapon 2"] {
            let t = Table::new();
            t.set(n!("selItemId"), 0.0);
            set.set(Name::from(slot), t);
        }
        build.itemsTab.itemSets.with_mut(|m| m.insert(1, set));
        build.itemsTab.itemSetOrderList.with_mut(|v| v.push(1));
    }
    let spectres = spectre_list();
    let t1 = std::time::Instant::now();
    let mut t = Tally::default();
    for (c, l) in cases.iter().zip(lua_recs) {
        let r = pcall_rust(c, &build, &spectres);
        t.total += 1;
        if l.starts_with("ERR") && r == "ERR" {
            t.err_both += 1;
        } else if l == r {
            if l == "ERR" {
                t.err_both += 1;
            } else {
                t.exact += 1;
            }
        } else if order_free(l) == order_free(&r) && fixed_order(l) == fixed_order(&r) {
            t.order += 1;
        } else if strip_alt_source(&order_free(l)) == strip_alt_source(&order_free(&r))
            && fixed_order(l) == fixed_order(&r)
        {
            t.nondet += 1;
        } else if unsign_zero(&strip_alt_source(&order_free(l)))
            == unsign_zero(&strip_alt_source(&order_free(&r)))
        {
            t.zero += 1;
        } else {
            t.bad.push(format!(
                "{}\n{}",
                c.name,
                first_diff(
                    &strip_alt_source(&order_free(l)),
                    &strip_alt_source(&order_free(&r))
                )
            ));
        }
    }
    std::panic::set_hook(hook);
    eprintln!(
        "AS oracle: {} cases ({corpus_n} corpus, {} synthetic): {} exact, {} LUA-ORDER (mod order only), {} AltX/AltY source (CCR-0010), {} ±0 (CCR-0002), {} raise on both sides, {} MISMATCH; rust side {:?}",
        t.total,
        t.total - corpus_n,
        t.exact,
        t.order,
        t.nondet,
        t.zero,
        t.err_both,
        t.bad.len(),
        t1.elapsed()
    );
    for b in t.bad.iter().take(25) {
        eprintln!("MISMATCH {b}");
    }
    assert!(t.bad.is_empty(), "{} mismatching cases", t.bad.len());
}

// ---------------------------------------------------------------------------------------------
// L4 on the corpus probe dumps

/// Harness bookkeeping after running a function on an injected graph: the injection marked the
/// skill's nil fields absent (ORC-X registry), and the exporter keeps honouring the mark after the
/// Rust code assigns the field. Unmark what `buildActiveSkillModList` assigned; on a minion actor
/// the port created, mark the frozen `Actor`'s default-valued fields that PoB's `minion = { }`
/// does not have (yet) absent.
fn fix_absent_marks(skill: &ActiveSkillRef) {
    use pob_calc::probe::{mark_absent, rc_ptr, unmark_absent};
    let p = rc_ptr(skill);
    let early_return = {
        let f = skill.skillFlags.get();
        f.pairs().count() == 1 && f.truthy(n!("disable"))
    };
    for k in ["skillCfg", "skillModList", "baseSkillModList"] {
        unmark_absent(p, k);
    }
    if !early_return {
        for k in ["extraSkillModList", "minionList"] {
            unmark_absent(p, k);
        }
    }
    let opt = [
        ("weapon1Cfg", skill.weapon1Cfg.get().is_some()),
        ("weapon2Cfg", skill.weapon2Cfg.get().is_some()),
        ("weapon1Flags", skill.weapon1Flags.get().is_some()),
        ("weapon2Flags", skill.weapon2Flags.get().is_some()),
        ("skillTotemId", skill.skillTotemId.get().is_some()),
        ("skillPart", skill.skillPart.get().is_some()),
        ("skillPartName", skill.skillPartName.get().is_some()),
        ("disableReason", skill.disableReason.get().is_some()),
        ("activeMineCount", skill.activeMineCount.get().is_some()),
        ("activeStageCount", skill.activeStageCount.get().is_some()),
        ("minion", skill.minion.get().is_some()),
    ];
    for (k, set) in opt {
        if set {
            unmark_absent(p, k);
        }
    }
    let ae = skill.activeEffect.get();
    let pe = rc_ptr(&ae);
    for (k, set) in [
        ("grantedEffectLevel", ae.grantedEffectLevel.get().is_some()),
        ("actorLevel", ae.actorLevel.get().is_some()),
        ("supportQuality", ae.supportQuality.get().is_some()),
    ] {
        if set {
            unmark_absent(pe, k);
        }
    }
    if let Some(m) = skill.minion.get() {
        let pm = rc_ptr(&m);
        for k in ["modDB", "output", "activeSkillList", "strDmgBonus"] {
            mark_absent(pm, k);
        }
    }
}

/// Split the state-diff mismatches into unexplained ones and documented differences:
/// * `minion.lifeTable`: PoB writes the data table's `$data` marker; the port stores a
///   [`LifeTable`] handle (CCR-0040) — accepted when it points at the very table the marker names;
/// * mods whose only difference is a `Skill:<id>AltX/AltY` source (PoB's per-process gem variant
///   choice, CCR-0010), matched pairwise between ModMissing and ModExtra;
/// * `minion.itemSet` / `srcInstance.skillMinionItemSet`: the build model's
///   `itemsTab.itemSets` is not part of the dump (`env.build` is an `$object` marker) and the
///   build loader (BM-A) is not ported yet, so the item set lookup sees an empty build.
fn explain(
    mm: Vec<pob_oracle::diff::Mismatch>,
    skill: &ActiveSkillRef,
) -> (Vec<pob_oracle::diff::Mismatch>, Vec<&'static str>) {
    use pob_oracle::diff::MismatchKind as K;
    let mut why = Vec::new();
    let mut rest = Vec::new();
    let strip = |j: &Option<serde_json::Value>| -> Option<String> {
        let mut j = j.clone()?;
        if let Some(o) = j.as_object_mut() {
            o.remove("$at");
            if let Some(serde_json::Value::String(s)) = o.get_mut("source") {
                *s = strip_alt_source(s);
            }
        }
        Some(j.to_string())
    };
    let mut missing: Vec<(String, pob_oracle::diff::Mismatch)> = Vec::new();
    let mut extra: Vec<(String, pob_oracle::diff::Mismatch)> = Vec::new();
    let itemSetGap = mm.iter().any(|m| m.path.ends_with("minion.itemSet"));
    for m in mm {
        if m.path.ends_with("minion.uses") {
            // CCR-0030 #2: PoB's `$data` marker of the granted effect's `minionUses`; the port
            // keeps the same slot names as a table in `minion.extra["uses"]`
            let ge = skill.activeEffect.get().grantedEffect.get();
            let mut want: Vec<String> = ge
                .minionUses
                .as_ref()
                .map(|u| u.iter().map(|n| n.as_str().to_owned()).collect())
                .unwrap_or_default();
            want.sort();
            let mut have: Vec<String> = m
                .ours
                .as_ref()
                .and_then(serde_json::Value::as_object)
                .map(|o| o.keys().cloned().collect())
                .unwrap_or_default();
            have.sort();
            let marker_ok = m
                .pob
                .as_ref()
                .and_then(|j| j.get("$data"))
                .and_then(serde_json::Value::as_str)
                == Some(&format!("data.skills.{}.minionUses", ge.id));
            if marker_ok && want == have {
                why.push("minion.uses holds the granted effect's minionUses slots (CCR-0030 #2 representation)");
                continue;
            }
            rest.push(m);
        } else if itemSetGap
            && (m.path.ends_with("minion.weaponData1") || m.path.ends_with("minion.weaponData2"))
        {
            why.push("minion weapon data from the item set: item sets are not in the dump (BM-A)");
        } else if m.path.ends_with("minion.lifeTable") {
            let named = m
                .pob
                .as_ref()
                .and_then(|j| j.get("$data"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned);
            let d = data();
            let target: Option<&[f64]> = match named.as_deref() {
                Some("data.monsterLifeTable") => Some(&d.monsterLifeTable),
                Some("data.monsterLifeTable2") => Some(&d.monsterLifeTable2),
                Some("data.monsterLifeTable3") => Some(&d.monsterLifeTable3),
                Some("data.monsterAllyLifeTable") => Some(&d.monsterAllyLifeTable),
                _ => None,
            };
            let ours = skill
                .minion
                .get()
                .and_then(|mi| mi.extra.obj::<LifeTable>(n!("lifeTable")));
            if let (Some(t), Some(o)) = (target, ours) {
                if std::ptr::eq(t.as_ptr(), o.0.as_ptr()) {
                    why.push("minion.lifeTable is the same data table (CCR-0040 representation)");
                    continue;
                }
            }
            rest.push(m);
        } else if m.path.ends_with("minion.itemSet") || m.path.ends_with("skillMinionItemSet") {
            why.push("item sets are not in the dump and BM-A's build loader is not ported (empty itemsTab)");
        } else if matches!(m.kind, K::ModMissing) {
            missing.push((strip(&m.pob).unwrap_or_default(), m));
        } else if matches!(m.kind, K::ModExtra) {
            extra.push((strip(&m.ours).unwrap_or_default(), m));
        } else {
            rest.push(m);
        }
    }
    for (k, m) in missing {
        if let Some(i) = extra.iter().position(|(e, _)| *e == k) {
            extra.remove(i);
            why.push("mod source names the AltX/AltY gem variant PoB picked (CCR-0010)");
        } else {
            rest.push(m);
        }
    }
    rest.extend(extra.into_iter().map(|(_, m)| m));
    (rest, why)
}

#[test]
fn as_l4_corpus() {
    use pob_oracle::schema::StateDump;
    use pob_oracle::testkit;
    if !init_data() {
        return skip("data/dump not found");
    }
    let Some(parity) = find_parity_dir() else {
        return skip(".parity not found");
    };
    let probe = parity.join("probe");
    let mut stems: Vec<String> = std::fs::read_dir(&probe)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    stems.sort();
    let mut pairs = 0usize;
    let mut pass = 0usize;
    let mut fails: Vec<String> = Vec::new();
    let mut paths: std::collections::BTreeMap<String, usize> = Default::default();
    let mut explained: std::collections::BTreeMap<&'static str, usize> = Default::default();
    for stem in &stems {
        let dir = probe.join(stem).join("state");
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut ins: Vec<String> = rd
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("buildActiveSkillModList:in#"))
            .collect();
        ins.sort();
        for fin in ins {
            let fout = fin.replace(":in#", ":out#");
            let (Ok(din), Ok(dout)) = (
                StateDump::load(&dir.join(&fin)),
                StateDump::load(&dir.join(&fout)),
            ) else {
                continue;
            };
            pairs += 1;
            let build = testkit::empty_build();
            // the dump's `env.build` is an `$object` marker; the one build-model value the
            // function reads that changes results is the character level (minion level), taken
            // from the build XML
            if let Some(level) =
                std::fs::read_to_string(parity.join("builds").join(format!("{stem}.xml")))
                    .ok()
                    .and_then(|t| {
                        let tag = t.split("<Build ").nth(1)?.split('>').next()?.to_owned();
                        let i = tag.find(" level=\"")? + 8;
                        tag[i..].split('"').next()?.parse::<f64>().ok()
                    })
            {
                build.characterLevel.set(level);
            }
            let inj = match pob_oracle::inject::inject(&din, &build) {
                Ok(i) => i,
                Err(e) => {
                    fails.push(format!("{stem} {fin}: inject {e}"));
                    continue;
                }
            };
            let locals: Vec<(String, Val)> = inj.locals.into_iter().collect();
            let get = |k: &str| {
                locals
                    .iter()
                    .find(|(n, _)| n == k)
                    .map_or(Val::Nil, |(_, v)| v.clone())
            };
            let (Some(env), Some(skill)) = (
                get("env").obj::<Env>(),
                get("activeSkill").obj::<ActiveSkill>(),
            ) else {
                fails.push(format!("{stem} {fin}: locals env/activeSkill not injected"));
                continue;
            };
            let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                buildActiveSkillModList(&env, &skill);
            }));
            if let Err(p) = run {
                let msg = p
                    .downcast_ref::<LuaError>()
                    .map(|e| e.0.clone())
                    .or_else(|| p.downcast_ref::<String>().cloned())
                    .unwrap_or_default();
                fails.push(format!("{stem} {fin}: run {msg}"));
                continue;
            }
            fix_absent_marks(&skill);
            let mut locals = locals;
            // the `:out` dump carries the return values (`{ n = 0 }`: the function returns nothing)
            let ret = Table::new();
            ret.set(n!("n"), 0.0);
            locals.push(("ret".to_owned(), Val::Table(ret)));
            let mm = testkit::diff_against(&dout, Some(&env), &locals, stem);
            let (mm, why) = explain(mm, &skill);
            for w in why {
                *explained.entry(w).or_default() += 1;
            }
            if mm.is_empty() {
                pass += 1;
            } else {
                for m in &mm {
                    let g: String = m
                        .path
                        .split('.')
                        .map(|s| {
                            if s.chars().all(|c| c.is_ascii_digit()) {
                                "#"
                            } else {
                                s
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(".");
                    *paths.entry(g).or_default() += 1;
                }
                fails.push(format!(
                    "{stem} {fin}: {} mismatches, e.g. {}",
                    mm.len(),
                    mm.iter()
                        .take(4)
                        .map(|m| format!(
                            "{} {:?}: pob={:?} ours={:?}",
                            m.path, m.kind, m.pob, m.ours
                        ))
                        .collect::<Vec<_>>()
                        .join(" | ")
                ));
            }
        }
    }
    if pairs == 0 {
        return skip("no buildActiveSkillModList probe dumps (scripts/port/oracle.sh corpus)");
    }
    eprintln!("AS L4: {pass}/{pairs} buildActiveSkillModList in→out dumps match");
    for (w, n) in &explained {
        eprintln!("  explained ({n} mismatches): {w}");
    }
    for (p, n) in paths.iter().rev().take(30) {
        eprintln!("  path {p}: {n}");
    }
    for f in fails.iter().take(60) {
        eprintln!("FAIL {f}");
    }
    assert_eq!(pass, pairs, "{} of {pairs} dumps differ", pairs - pass);
}

// ---------------------------------------------------------------------------------------------
// data census

#[test]
fn as_census() {
    if !init_data() {
        return skip("data/dump not found");
    }
    // every support has an `addSkillTypes` table (CalcActiveSkill.lua:530 reads it as a truth value)
    let Some(dump) = find_up("data/dump/skills.json") else {
        return skip("data/dump/skills.json not found");
    };
    let j: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dump).unwrap()).unwrap();
    let obj = j.as_object().unwrap();
    let mut empty_minion_lists: Vec<String> = Vec::new();
    for (id, ge) in obj {
        let Some(ge) = ge.as_object() else { continue };
        if ge.get("support").and_then(serde_json::Value::as_bool) == Some(true) {
            assert!(
                ge.contains_key("addSkillTypes"),
                "{id}: support without addSkillTypes"
            );
        }
        if let Some(ml) = ge.get("minionList") {
            let empty = ml.as_object().is_some_and(serde_json::Map::is_empty)
                || ml.as_array().is_some_and(Vec::is_empty);
            if empty {
                empty_minion_lists.push(id.clone());
            }
        }
        // minionList_is_set agrees with the dump for every skill
        let typed = &data().skills[&Name::from(id.as_str())];
        assert_eq!(
            pob_calc::modules::calc_active_skill::minionList_is_set(typed),
            ge.contains_key("minionList"),
            "{id}: minionList presence"
        );
    }
    empty_minion_lists.sort();
    assert_eq!(empty_minion_lists, vec!["RaiseSpectre", "RaiseSpectreAltX"]);
}
