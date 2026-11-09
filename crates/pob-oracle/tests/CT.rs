//! Unit CT acceptance (calc doc §11.1, row CT): the calc tools against PoB
//! itself, booted once under LuaJIT with the JIT off (`JitPob::boot_oracle`,
//! A6), each check running the same inputs through the Lua and the Rust port
//! and comparing the results byte for byte:
//!
//! * `modcache`: `modLib.formatMod` / `formatSourceMod` / `formatTags`, the
//!   `parseFormattedSourceMod` round trip, `compareModParams` of neighbours and
//!   `calcLib.getConvertedModTags` on every mod of Data/ModCache.lua.
//! * `type_expr`: `calcLib.doesTypeExpressionMatch` on random RPN expressions.
//! * `support`: `calcLib.canGrantedEffectSupportActiveSkill` over all
//!   (support, active skill) pairs, in three actor/flag variants.
//! * `instance_stats`: `calcLib.buildSkillInstanceStats` for every gem's
//!   granted effects at levels {1, 20, 21} × quality {0, 20} × actorLevel {nil, 70}.
//! * `gem_level`: `calcLib.validateGemLevel` for gems and skill effects at odd levels
//!   (incl. `next(levels)` on the holey level tables).
//! * `stat_map`: `grantedEffect.statMap[stat]` (own entries and the
//!   `data.skillStatMap` fallback + processMod) for every skill × its stats, every
//!   global stat on three probe skills, then `grantedEffect.hasGlobalEffect` of every skill.
//! * `gem_fns`: `getGemStatRequirement`, `gemIsType`, `getGameIdFromGemName`, `isGemIdSame`.
//! * `breakdown`: the CalcBreakdown helpers on random inputs (`multiChain`, `area`,
//!   `effMult`, `dot`, `critDot`, and `simple` / `mod` / `slot` / `leech` over an
//!   empty mod store, whose Sum/More results are taken from the Rust store so the
//!   check holds before and after MS-A).
//! * `format`: `formatNumSep` / `formatCalcVal` under several separator settings and
//!   `formatCalcStr`'s `{output:…}` forms.
//! * `uuid`: `cacheSkillUUID` on synthetic socket groups.
//!
//! Two classes of difference are counted apart, not failed: a formatted zero whose sign differs
//! (the oracle host is a dual-number ARM64 LuaJIT, whose `-0` literals load as +0; CCR-0002), and
//! a `Skill:<id>` source that names a skill's AltX/AltY variant (PoB's own per-process choice,
//! calc doc §9.5, CCR-0010). `CT_ONLY=a,b` runs a subset of the checks.
//!
//! Inputs missing (data/dump, the PoB checkout) skip with a message; with
//! `POB_ORACLE_REQUIRE=1` (or `CI`) they fail instead.
#![allow(non_snake_case)]
#![allow(
    clippy::format_push_string,
    clippy::many_single_char_names,
    clippy::explicit_auto_deref,
    clippy::no_effect_underscore_binding,
    clippy::format_collect,
    clippy::manual_string_new,
    clippy::type_complexity
)]

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use pob_calc::prelude::*;
use pob_oracle::headless::{Engine, Ret};
use pob_oracle::jit::JitPob;
use pob_oracle::oracle::{find_pob_src, find_up};

fn skip(why: &str) {
    let required = std::env::var_os("POB_ORACLE_REQUIRE").is_some_and(|v| v == "1")
        || std::env::var_os("CI").is_some_and(|v| !v.is_empty() && v != "false" && v != "0");
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

/// PoB under LuaJIT; `eval` returns the chunk's single string result.
struct Lua(JitPob);

impl Lua {
    fn eval(&mut self, code: &str) -> String {
        let r = self.0.run(code).unwrap_or_else(|e| panic!("lua: {e}"));
        match r.first() {
            Some(Ret::Str(s)) => s.clone(),
            other => panic!("lua returned {other:?}"),
        }
    }
}

/// Lua `pcall`: a `lua_error!` becomes `Err(())`.
fn pcall<R>(f: impl FnOnce() -> R) -> Result<R, ()> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => Ok(r),
        Err(p) if p.downcast_ref::<LuaError>().is_some() => Err(()),
        Err(p) => std::panic::resume_unwind(p),
    }
}

/// splitmix64.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn chance(&mut self, p: f64) -> bool {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64 <= p
    }
    /// A number from a mix of edge values and random magnitudes.
    fn num(&mut self) -> f64 {
        const EDGE: &[f64] = &[
            0.0, 1.0, -1.0, 0.5, 2.25, 10.0, 37.5, -12.75, 100.0, 1234.5678, 0.001, 75.0, -30.0,
            0.125, 99.995, 1e6, -0.5, 3.0, 0.7, 14.0,
        ];
        match self.below(3) {
            0 => EDGE[self.below(EDGE.len() as u64) as usize],
            1 => (self.below(20001) as f64 - 10000.0) / 100.0,
            _ => (self.below(2_000_001) as f64 - 1_000_000.0) / 7.0,
        }
    }
}

/// A Lua number literal that reads back as exactly `x` (finite).
fn lnum(x: f64) -> String {
    format!("{x:?}")
}

fn f17(x: f64) -> String {
    lfmt!("%.17g", x)
}

/// One check's tally.
struct Check {
    name: &'static str,
    total: usize,
    bad: Vec<String>,
    /// Records that differ only in the sign of a formatted zero (CCR-0002 host artefact).
    zero_sign: usize,
    /// Records that differ only where PoB itself differs between processes (calc doc §9.5).
    nondet: usize,
}

/// `s` with `Skill:<id>AltX` / `Skill:<id>AltY` written as `Skill:<id>`. Data.lua:1049-1080
/// processes a statMap table that a skill shares with its Vaal AltX/AltY variant once per skill,
/// in `pairs(data.skills)` order (random per PoB process), so the surviving `mod.source` names
/// either variant (CCR-0010: ExplosiveTrap / ExplosiveTrapAltX).
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

/// `s` with every standalone `-0` number (not followed by a digit or `.`, not preceded by a
/// digit or `.`) written as `0`.
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

impl Check {
    fn new(name: &'static str) -> Self {
        Check {
            name,
            total: 0,
            bad: Vec::new(),
            zero_sign: 0,
            nondet: 0,
        }
    }
    fn cmp(&mut self, what: &str, lua: &str, rust: &str) {
        self.total += 1;
        if lua != rust {
            // CCR-0002: the oracle host is a dual-number LuaJIT (ARM64), whose `-0` literals in
            // Data/ModCache.lua load as +0; the port follows x64 PoB. A difference that only
            // affects formatted zeros is a host artefact, counted apart.
            if unsign_zero(lua) == unsign_zero(rust) {
                self.zero_sign += 1;
            } else if self.name == "stat_map" && strip_alt_source(lua) == strip_alt_source(rust) {
                self.nondet += 1;
            } else {
                self.bad
                    .push(format!("{what}\n    lua  {lua:?}\n    rust {rust:?}"));
            }
        }
    }
}

/// Compare `\x1e`-separated records line by line.
fn cmp_records(c: &mut Check, keys: &[String], lua: &str, rust: &[String]) {
    let l: Vec<&str> = if lua.is_empty() {
        Vec::new()
    } else {
        lua.split('\x1e').collect()
    };
    assert_eq!(l.len(), rust.len(), "{}: record count", c.name);
    for (i, (a, b)) in l.iter().zip(rust).enumerate() {
        c.cmp(keys.get(i).map_or("", String::as_str), a, b);
    }
}

const LUA_NORM: &str = r#"
local function norm(s) return (tostring(s):gsub("function: [%w#]+", "function: builtin")) end
"#;

// ---------------------------------------------------------------------------------------------
// modcache

fn check_modcache(lua: &mut Lua) -> Check {
    use pob_gamedata::mod_tools::*;
    let mut c = Check::new("modcache");
    let code = format!(
        r#"{LUA_NORM}
local cache = LoadModule("Data/ModCache")
local keys = {{}}
for k in pairs(cache) do keys[#keys+1] = k end
table.sort(keys)
local out = {{}}
local prev
local function tags(t) local ok, s = pcall(modLib.formatTags, t) return ok and norm(s) or "ERR" end
for _, k in ipairs(keys) do
  local e = cache[k]
  if e[1] then
    for i, mod in ipairs(e[1]) do
      local ok1, fm = pcall(modLib.formatMod, mod)
      local ok2, fs = pcall(modLib.formatSourceMod, mod)
      local rt = "ERR"
      if ok2 then
        local ok, m2 = pcall(modLib.parseFormattedSourceMod, fs)
        if ok then
          if m2 then
            local ok3, s3 = pcall(modLib.formatSourceMod, m2)
            rt = ok3 and norm(s3) or "ERR"
          else rt = "nil" end
        end
      end
      local cmpv = "-"
      if prev then
        local ok, r = pcall(modLib.compareModParams, mod, prev)
        cmpv = ok and (r and "1" or "0") or "ERR"
      end
      local okc, conv = pcall(calcLib.getConvertedModTags, mod, 1.5, true)
      local okd, conv2 = pcall(calcLib.getConvertedModTags, mod, 3, false)
      out[#out+1] = table.concat({{ ok1 and norm(fm) or "ERR", ok2 and norm(fs) or "ERR", tags(mod), rt, cmpv,
        okc and tags(conv) or "ERR", okd and tags(conv2) or "ERR" }}, "\31")
      prev = mod
    end
  end
end
return table.concat(out, "\30")
"#
    );
    let l = lua.eval(&code);
    let d = pob_gamedata::data();
    let mut keys: Vec<&Arc<str>> = d.modCache.keys().collect();
    keys.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    let mut rust = Vec::new();
    let mut names = Vec::new();
    let mut prev: Option<pob_gamedata::ModRef> = None;
    let tags = |t: &[Tag]| pcall(|| formatTags(t)).unwrap_or_else(|()| "ERR".to_owned());
    for k in keys {
        let Some(mods) = &d.modCache[k].mods else {
            continue;
        };
        for (i, m) in mods.iter().enumerate() {
            let fm = pcall(|| formatMod(m)).unwrap_or_else(|()| "ERR".into());
            let fs = pcall(|| formatSourceMod(m));
            let rt = match &fs {
                Ok(fs) => match pcall(|| parseFormattedSourceMod(fs)) {
                    Ok(Some(m2)) => {
                        pcall(|| formatSourceMod(&m2)).unwrap_or_else(|()| "ERR".into())
                    }
                    Ok(None) => "nil".into(),
                    Err(()) => "ERR".into(),
                },
                Err(()) => "ERR".into(),
            };
            let cmpv = match &prev {
                None => "-".to_owned(),
                Some(p) => match pcall(|| compareModParams(m, p)) {
                    Ok(true) => "1".into(),
                    Ok(false) => "0".into(),
                    Err(()) => "ERR".into(),
                },
            };
            let conv = pcall(|| pob_calc::modules::calc_tools::getConvertedModTags(m, 1.5, true))
                .map_or_else(|()| "ERR".to_owned(), |t| tags(&t));
            let conv2 = pcall(|| pob_calc::modules::calc_tools::getConvertedModTags(m, 3.0, false))
                .map_or_else(|()| "ERR".to_owned(), |t| tags(&t));
            rust.push(
                [
                    fm,
                    fs.unwrap_or_else(|()| "ERR".into()),
                    tags(&m.tags),
                    rt,
                    cmpv,
                    conv,
                    conv2,
                ]
                .join("\x1f"),
            );
            names.push(format!("{k:?}[{}]", i + 1));
            prev = Some(m.clone());
        }
    }
    cmp_records(&mut c, &names, &l, &rust);
    c
}

// ---------------------------------------------------------------------------------------------
// type_expr

fn check_type_expr(lua: &mut Lua) -> Check {
    use pob_calc::modules::calc_tools::doesTypeExpressionMatch;
    let mut c = Check::new("type_expr");
    let mut rng = Rng(0xC7_0001);
    let n = 20_000;
    let mut cases: Vec<(Vec<u8>, SkillTypeSet, Option<SkillTypeSet>)> = Vec::new();
    for _ in 0..n {
        let len = rng.below(9) as usize;
        let mut expr = Vec::new();
        for _ in 0..len {
            let t = if rng.chance(0.35) {
                [SkillType::OR, SkillType::AND, SkillType::NOT][rng.below(3) as usize]
            } else if rng.chance(0.9) {
                1 + rng.below(10) as u8
            } else {
                1 + rng.below(SkillType::COUNT as u64 - 1) as u8
            };
            expr.push(t);
        }
        let set = |rng: &mut Rng| {
            let mut s = SkillTypeSet::default();
            for t in 1..=10u8 {
                if rng.chance(0.4) {
                    s.set(t, true);
                }
            }
            s
        };
        let st = set(&mut rng);
        let mt = if rng.chance(0.5) {
            Some(set(&mut rng))
        } else {
            None
        };
        cases.push((expr, st, mt));
    }
    let lset = |s: &SkillTypeSet| {
        let parts: Vec<String> = s.iter().map(|t| format!("[{t}]=true")).collect();
        format!("{{{}}}", parts.join(","))
    };
    let mut code = String::from("local cases = {\n");
    for (e, st, mt) in &cases {
        let es: Vec<String> = e.iter().map(u8::to_string).collect();
        code.push_str(&format!(
            "{{{{{}}},{},{}}},\n",
            es.join(","),
            lset(st),
            mt.as_ref().map_or("nil".to_owned(), lset)
        ));
    }
    code.push_str(
        r#"}
local out = {}
for i, cs in ipairs(cases) do
  local ok, r = pcall(calcLib.doesTypeExpressionMatch, cs[1], cs[2], cs[3])
  out[i] = ok and (r and "1" or "0") or "E"
end
return table.concat(out)
"#,
    );
    let l = lua.eval(&code);
    let mut r = String::new();
    for (e, st, mt) in &cases {
        let ex: Vec<SkillTypeExpr> = e
            .iter()
            .map(|t| SkillTypeExpr::from_skill_type(*t))
            .collect();
        r.push(
            match pcall(|| doesTypeExpressionMatch(&ex, st, mt.as_ref())) {
                Ok(true) => '1',
                Ok(false) => '0',
                Err(()) => 'E',
            },
        );
    }
    for (i, (a, b)) in l.chars().zip(r.chars()).enumerate() {
        c.cmp(
            &format!("case {i}: {:?}", cases[i].0),
            &a.to_string(),
            &b.to_string(),
        );
    }
    assert_eq!(l.len(), r.len());
    c
}

// ---------------------------------------------------------------------------------------------
// support

fn sorted_skills(support: bool) -> Vec<pob_gamedata::GrantedEffectRef> {
    let mut v: Vec<_> = pob_gamedata::data()
        .skills
        .values()
        .filter(|g| g.support == support)
        .cloned()
        .collect();
    v.sort_by(|a, b| a.id.as_str().as_bytes().cmp(b.id.as_str().as_bytes()));
    v
}

fn make_active(
    ge: &pob_gamedata::GrantedEffectRef,
    variant: u32,
) -> (ActorRef, ActorRef, ActiveSkillRef) {
    let player = Actor::new();
    let enemy = Actor::new();
    enemy.player.set(Rc::downgrade(&player));
    player.enemy.set(Rc::downgrade(&enemy));
    if variant == 2 {
        let w1 = Table::new();
        w1.set(n!("type"), "Claw");
        w1.set(n!("countsAsAll1H"), true);
        player.weaponData1.set(w1);
        let w2 = Table::new();
        w2.set(n!("type"), "Shield");
        player.weaponData2.set(w2);
    }
    let eff = SkillEffect::new(ge.clone(), 20.0, 0.0);
    eff.gemData
        .set(pob_gamedata::data().gem_for_skill(ge.id).cloned());
    let a = ActiveSkill::new(eff, Shared::new(Vec::new()), Rc::downgrade(&player));
    a.skillTypes.set(Shared::new(ge.skillTypes));
    a.minionSkillTypes.set(ge.minionSkillTypes.map(Shared::new));
    (player, enemy, a)
}

fn check_support(lua: &mut Lua) -> Check {
    use pob_calc::modules::calc_tools::canGrantedEffectSupportActiveSkill;
    let mut c = Check::new("support");
    let code = r#"
local skills, supports = {}, {}
for id, ge in pairs(data.skills) do
  if ge.support then supports[#supports+1] = id else skills[#skills+1] = id end
end
table.sort(skills) table.sort(supports)
local out = {}
for variant = 1, 3 do
  for _, sid in ipairs(skills) do
    local ge = data.skills[sid]
    local actor = { weaponData1 = {}, weaponData2 = {} }
    if variant == 2 then actor.weaponData1 = { type = "Claw", countsAsAll1H = true } actor.weaponData2 = { type = "Shield" } end
    actor.enemy = { player = actor }
    local gemId = data.gemForSkill[ge]
    local as = { activeEffect = { grantedEffect = ge, gemData = gemId and data.gems[gemId] or nil }, actor = actor,
      skillTypes = copyTable(ge.skillTypes), minionSkillTypes = ge.minionSkillTypes and copyTable(ge.minionSkillTypes) or nil }
    local bits = {}
    for i, pid in ipairs(supports) do
      local ok, r = pcall(calcLib.canGrantedEffectSupportActiveSkill, data.skills[pid], as, nil, variant == 3)
      bits[i] = ok and (r and "1" or "0") or "E"
    end
    out[#out+1] = table.concat(bits)
  end
end
return table.concat(out, "\30")
"#;
    let l = lua.eval(code);
    let skills = sorted_skills(false);
    let supports = sorted_skills(true);
    let mut rust = Vec::new();
    let mut names = Vec::new();
    let mut pairs = 0usize;
    for variant in 1..=3u32 {
        for ge in &skills {
            let (_p, _e, a) = make_active(ge, variant);
            let mut bits = String::new();
            for sup in &supports {
                pairs += 1;
                bits.push(
                    match pcall(|| canGrantedEffectSupportActiveSkill(sup, &a, None, variant == 3))
                    {
                        Ok(true) => '1',
                        Ok(false) => '0',
                        Err(()) => 'E',
                    },
                );
            }
            rust.push(bits);
            names.push(format!("variant {variant} {}", ge.id));
        }
    }
    cmp_records(&mut c, &names, &l, &rust);
    eprintln!(
        "support: {} active × {} supports × 3 variants = {pairs} pairs",
        skills.len(),
        supports.len()
    );
    c.total = pairs;
    c
}

// ---------------------------------------------------------------------------------------------
// instance_stats + gem_level

fn sorted_gems() -> Vec<pob_gamedata::GemDataRef> {
    let mut v: Vec<_> = pob_gamedata::data().gems.values().cloned().collect();
    v.sort_by(|a, b| a.id.as_str().as_bytes().cmp(b.id.as_str().as_bytes()));
    v
}

fn check_instance_stats(lua: &mut Lua) -> Check {
    use pob_calc::modules::calc_tools::buildSkillInstanceStats;
    let mut c = Check::new("instance_stats");
    let code = r#"
local ids = {}
for id in pairs(data.gems) do ids[#ids+1] = id end
table.sort(ids)
local out = {}
for _, id in ipairs(ids) do
  local gem = data.gems[id]
  for _, ge in ipairs({ gem.grantedEffect, gem.secondaryGrantedEffect or false }) do
    if ge then
      for _, L in ipairs({1, 20, 21}) do
        for _, Q in ipairs({0, 20}) do
          for _, AL in ipairs({false, 70}) do
            local ok, stats = pcall(calcLib.buildSkillInstanceStats, { level = L, quality = Q, actorLevel = AL or nil }, ge)
            if ok then
              local kv = {}
              for k, v in pairs(stats) do kv[#kv+1] = k .. "=" .. string.format("%.17g", v) end
              table.sort(kv)
              out[#out+1] = table.concat(kv, ",")
            else
              out[#out+1] = "ERR"
            end
          end
        end
      end
    end
  end
end
return table.concat(out, "\30")
"#;
    let l = lua.eval(code);
    let mut rust = Vec::new();
    let mut names = Vec::new();
    for gem in sorted_gems() {
        for ge in [
            Some(gem.grantedEffect.clone()),
            gem.secondaryGrantedEffect.clone(),
        ]
        .into_iter()
        .flatten()
        {
            for L in [1.0, 20.0, 21.0] {
                for Q in [0.0, 20.0] {
                    for AL in [None, Some(70.0)] {
                        let inst = SkillEffect::new(ge.clone(), L, Q);
                        inst.actorLevel.set(AL);
                        let r = pcall(|| buildSkillInstanceStats(&inst, &ge));
                        rust.push(r.map_or_else(
                            |()| "ERR".to_owned(),
                            |t| {
                                let mut kv: Vec<String> = t
                                    .pairs()
                                    .map(|(k, v)| {
                                        format!(
                                            "{}={}",
                                            k.name().map_or_else(String::new, |n| n.to_string()),
                                            f17(v.num())
                                        )
                                    })
                                    .collect();
                                kv.sort();
                                kv.join(",")
                            },
                        ));
                        names.push(format!("{} {} L{L} Q{Q} AL{AL:?}", gem.id, ge.id));
                    }
                }
            }
        }
    }
    cmp_records(&mut c, &names, &l, &rust);
    c
}

fn check_gem_level(lua: &mut Lua) -> Check {
    use pob_calc::modules::calc_tools::{levels_next_key, validateGemLevel};
    let mut c = Check::new("gem_level");
    let levels = [0.0, 1.0, 2.5, 20.0, 21.0, 25.0, 31.0, 40.0];
    let lv = levels
        .iter()
        .map(|x| lnum(*x))
        .collect::<Vec<_>>()
        .join(",");
    let code = format!(
        r#"
local levels = {{{lv}}}
local ids = {{}}
for id in pairs(data.gems) do ids[#ids+1] = id end
table.sort(ids)
local sids = {{}}
for id in pairs(data.skills) do sids[#sids+1] = id end
table.sort(sids)
local out = {{}}
for _, id in ipairs(ids) do
  for _, L in ipairs(levels) do
    local g = {{ gemData = data.gems[id], level = L }}
    local ok = pcall(calcLib.validateGemLevel, g)
    out[#out+1] = ok and string.format("%.17g", g.level) or "ERR"
  end
end
for _, id in ipairs(sids) do
  local ge = data.skills[id]
  out[#out+1] = "next=" .. tostring(next(ge.levels))
  for _, L in ipairs(levels) do
    local g = {{ grantedEffect = ge, level = L }}
    local ok = pcall(calcLib.validateGemLevel, g)
    out[#out+1] = ok and string.format("%.17g", g.level) or "ERR"
  end
end
return table.concat(out, "\30")
"#
    );
    let l = lua.eval(&code);
    let mut rust = Vec::new();
    let mut names = Vec::new();
    for gem in sorted_gems() {
        for L in levels {
            let g = GemInstance::default();
            g.gemData.set(Some(gem.clone()));
            g.level.set(L);
            rust.push(match pcall(|| validateGemLevel(&g)) {
                Ok(()) => f17(g.level.get()),
                Err(()) => "ERR".into(),
            });
            names.push(format!("gem {} L{L}", gem.id));
        }
    }
    let mut skills: Vec<_> = pob_gamedata::data().skills.values().cloned().collect();
    skills.sort_by(|a, b| a.id.as_str().as_bytes().cmp(b.id.as_str().as_bytes()));
    for ge in skills {
        rust.push(format!(
            "next={}",
            levels_next_key(&ge.levels).map_or("nil".to_owned(), tostring_num)
        ));
        names.push(format!("next(levels) {}", ge.id));
        for L in levels {
            let e = SkillEffect::new(ge.clone(), L, 0.0);
            rust.push(match pcall(|| validateGemLevel(&*e)) {
                Ok(()) => f17(e.level.get()),
                Err(()) => "ERR".into(),
            });
            names.push(format!("skill {} L{L}", ge.id));
        }
    }
    cmp_records(&mut c, &names, &l, &rust);
    c
}

// ---------------------------------------------------------------------------------------------
// stat_map

fn stat_list(ge: &pob_gamedata::GrantedEffect) -> Vec<Name> {
    let mut v: Vec<Name> = ge.stats.clone();
    v.extend(ge.constantStats.iter().map(|s| s.0));
    v.extend(ge.qualityStats.iter().map(|s| s.0));
    v
}

fn ser_map(map: Option<&pob_gamedata::StatMapEntry>) -> String {
    use pob_gamedata::mod_tools::{formatSourceMod, formatValue};
    use pob_gamedata::StatMapItem;
    let Some(map) = map else { return "nil".into() };
    let m1 = |m: &pob_gamedata::Mod| {
        let inner = m.value.inner_mod().map_or("-".to_owned(), |i| {
            i.source.as_deref().unwrap_or("nil").to_owned()
        });
        format!(
            "{}|inner={inner}",
            pcall(|| formatSourceMod(m)).unwrap_or_else(|()| "ERR".into())
        )
    };
    let opt = |x: Option<f64>| x.map_or("nil".to_owned(), f17);
    let mut parts: Vec<String> = Vec::new();
    for it in &map.items {
        match it {
            StatMapItem::Mod(m) => parts.push(format!("M:{}", m1(m))),
            StatMapItem::Group {
                mods,
                value,
                mult,
                div,
                base,
            } => {
                let g: Vec<String> = mods.iter().map(|m| m1(m)).collect();
                parts.push(format!(
                    "G[{}]v={} m={} d={} b={}",
                    g.join(";"),
                    value.as_ref().map_or("nil".to_owned(), formatValue),
                    opt(*mult),
                    opt(*div),
                    opt(*base)
                ));
            }
        }
    }
    parts.push(format!(
        "value={} mult={} div={} base={} skillFlag={}",
        map.value.as_ref().map_or("nil".to_owned(), formatValue),
        opt(map.mult),
        opt(map.div),
        opt(map.base),
        map.skillFlag.map_or("nil".to_owned(), |s| s.to_string())
    ));
    parts.join("\x1f")
}

const PROBE_SKILLS: &[&str] = &["SupportRage", "AncestralCry", "Fireball"];

fn check_stat_map(lua: &mut Lua) -> Check {
    let mut c = Check::new("stat_map");
    pob_gamedata::skill_stat_map::reset_stat_map_state();
    let probes = PROBE_SKILLS
        .iter()
        .map(|s| format!("{s:?}"))
        .collect::<Vec<_>>()
        .join(",");
    let code = format!(
        r#"
local function f17(x) return x == nil and "nil" or string.format("%.17g", x) end
local function m1(m)
  local inner = "-"
  if type(m.value) == "table" and m.value.mod then inner = tostring(m.value.mod.source) end
  local ok, f = pcall(modLib.formatSourceMod, m)
  return (ok and f or "ERR") .. "|inner=" .. inner
end
local function ser(map)
  if not map then return "nil" end
  local parts = {{}}
  for _, m in ipairs(map) do
    if m.name then parts[#parts+1] = "M:" .. m1(m)
    else
      local g = {{}}
      for _, mm in ipairs(m) do g[#g+1] = m1(mm) end
      parts[#parts+1] = "G[" .. table.concat(g, ";") .. "]v=" .. (m.value ~= nil and modLib.formatValue(m.value) or "nil")
        .. " m=" .. f17(m.mult) .. " d=" .. f17(m.div) .. " b=" .. f17(m.base)
    end
  end
  parts[#parts+1] = "value=" .. (map.value ~= nil and modLib.formatValue(map.value) or "nil") .. " mult=" .. f17(map.mult)
    .. " div=" .. f17(map.div) .. " base=" .. f17(map.base) .. " skillFlag=" .. tostring(map.skillFlag)
  return table.concat(parts, "\31")
end
local sids = {{}}
for id in pairs(data.skills) do sids[#sids+1] = id end
table.sort(sids)
local out = {{}}
local before = {{}}
for _, id in ipairs(sids) do before[#before+1] = data.skills[id].hasGlobalEffect and "1" or "0" end
out[#out+1] = table.concat(before)
local function look(ge, stat)
  local ok, r = pcall(function() return ge.statMap[stat] end)
  out[#out+1] = ok and ser(r) or "ERR"
end
for _, id in ipairs(sids) do
  local ge = data.skills[id]
  for _, s in ipairs(ge.stats or {{}}) do look(ge, s) end
  for _, s in ipairs(ge.constantStats or {{}}) do look(ge, s[1]) end
  for _, s in ipairs(ge.qualityStats or {{}}) do look(ge, s[1]) end
end
local gkeys = {{}}
for k in pairs(data.skillStatMap) do gkeys[#gkeys+1] = k end
table.sort(gkeys)
for _, id in ipairs({{ {probes} }}) do
  local ge = data.skills[id]
  for _, k in ipairs(gkeys) do look(ge, k) end
end
local after = {{}}
for _, id in ipairs(sids) do after[#after+1] = data.skills[id].hasGlobalEffect and "1" or "0" end
out[#out+1] = table.concat(after)
return table.concat(out, "\30")
"#
    );
    let l = lua.eval(&code);
    let d = pob_gamedata::data();
    let mut skills: Vec<_> = d.skills.values().cloned().collect();
    skills.sort_by(|a, b| a.id.as_str().as_bytes().cmp(b.id.as_str().as_bytes()));
    let mut rust = Vec::new();
    let mut names = Vec::new();
    rust.push(
        skills
            .iter()
            .map(|g| if g.hasGlobalEffect { '1' } else { '0' })
            .collect::<String>(),
    );
    names.push("hasGlobalEffect before".to_owned());
    let look = |ge: &pob_gamedata::GrantedEffect,
                stat: Name,
                rust: &mut Vec<String>,
                names: &mut Vec<String>| {
        rust.push(
            pcall(|| pob_gamedata::statMapLookup(ge, stat))
                .map_or_else(|()| "ERR".to_owned(), |m| ser_map(m.as_deref())),
        );
        names.push(format!("{}.statMap[{stat}]", ge.id));
    };
    let mut lookups = 0usize;
    for ge in &skills {
        for s in stat_list(ge) {
            look(ge, s, &mut rust, &mut names);
            lookups += 1;
        }
    }
    let mut gkeys: Vec<Name> = d.skillStatMap.keys().copied().collect();
    gkeys.sort_by(|a, b| a.as_str().as_bytes().cmp(b.as_str().as_bytes()));
    for id in PROBE_SKILLS {
        let ge = d.skills[&Name::from(*id)].clone();
        for k in &gkeys {
            look(&ge, *k, &mut rust, &mut names);
            lookups += 1;
        }
    }
    rust.push(
        skills
            .iter()
            .map(|g| {
                if pob_gamedata::skill_stat_map::hasGlobalEffect(g) {
                    '1'
                } else {
                    '0'
                }
            })
            .collect::<String>(),
    );
    names.push("hasGlobalEffect after".to_owned());
    cmp_records(&mut c, &names, &l, &rust);
    let flipped = skills
        .iter()
        .filter(|g| !g.hasGlobalEffect && pob_gamedata::skill_stat_map::hasGlobalEffect(g))
        .count();
    eprintln!(
        "stat_map: {lookups} lookups; {flipped} skills gained hasGlobalEffect through the fallback"
    );
    c
}

// ---------------------------------------------------------------------------------------------
// gem_fns

fn check_gem_fns(lua: &mut Lua) -> Check {
    use pob_calc::modules::calc_tools::{
        gemIsType, getGameIdFromGemName, getGemStatRequirement, isGemIdSame,
    };
    let mut c = Check::new("gem_fns");
    // getGemStatRequirement
    let mut rng = Rng(0xC7_0002);
    let mut cases = Vec::new();
    for _ in 0..5000 {
        let level = rng.below(45) as f64 - if rng.chance(0.05) { 0.5 } else { 0.0 };
        let sup = rng.chance(0.5);
        let multi = match rng.below(4) {
            0 => 0.0,
            1 => 100.0,
            _ => rng.below(1000) as f64 / 7.0,
        };
        cases.push((level, sup, multi));
    }
    let mut code = String::from("local cases = {\n");
    for (a, b, m) in &cases {
        code.push_str(&format!("{{{},{},{}}},\n", lnum(*a), b, lnum(*m)));
    }
    code.push_str(
        r#"}
local out = {}
for i, cs in ipairs(cases) do out[i] = string.format("%.17g", calcLib.getGemStatRequirement(cs[1], cs[2], cs[3])) end
return table.concat(out, "\30")
"#,
    );
    let l = lua.eval(&code);
    let rust: Vec<String> = cases
        .iter()
        .map(|(a, b, m)| f17(getGemStatRequirement(*a, *b, *m)))
        .collect();
    let names: Vec<String> = cases
        .iter()
        .map(|x| format!("getGemStatRequirement{x:?}"))
        .collect();
    cmp_records(&mut c, &names, &l, &rust);

    // gemIsType / getGameIdFromGemName / isGemIdSame
    let gems = sorted_gems();
    let base_types = [
        "all",
        "elemental",
        "aoe",
        "trap or mine",
        "active skill",
        "grants_active_skill",
        "skill",
        "non-vaal",
        "non-exceptional",
        "strength",
        "dexterity",
        "intelligence",
        "melee",
        "attack",
        "spell",
        "fire",
        "cold",
        "lightning",
        "chaos",
        "physical",
        "minion",
        "vaal",
        "support",
        "area",
        "bow",
        "herald",
        "golem",
        "fireball",
        "vaal fireball",
        "cleave",
        "nonsense",
    ];
    let code = format!(
        r#"
local types = {{ {} }}
local ids = {{}}
for id in pairs(data.gems) do ids[#ids+1] = id end
table.sort(ids)
local out = {{}}
local function r(ok, v) return ok and (v and "1" or "0") or "E" end
for i, id in ipairs(ids) do
  local gem = data.gems[id]
  local other = data.gems[ids[(i % #ids) + 1]]
  local ts = {{}}
  for _, t in ipairs(types) do ts[#ts+1] = t end
  ts[#ts+1] = gem.name:lower()
  ts[#ts+1] = other.name:lower()
  local bits = {{}}
  for _, t in ipairs(ts) do
    bits[#bits+1] = r(pcall(calcLib.gemIsType, gem, t, false))
    bits[#bits+1] = r(pcall(calcLib.gemIsType, gem, t, true))
  end
  local names = {{ gem.name, gem.name:lower(), gem.baseTypeName or "x", "Nonsense Gem" }}
  for _, nm in ipairs(names) do
    for _, dv in ipairs({{ false, true }}) do
      local ok, g = pcall(calcLib.getGameIdFromGemName, nm, dv)
      bits[#bits+1] = ok and tostring(g) or "E"
    end
  end
  bits[#bits+1] = r(pcall(calcLib.isGemIdSame, gem.name, other.name, true))
  bits[#bits+1] = r(pcall(calcLib.isGemIdSame, gem.name, gem.name, false))
  out[#out+1] = table.concat(bits, " ")
end
return table.concat(out, "\30")
"#,
        base_types
            .iter()
            .map(|t| format!("{t:?}"))
            .collect::<Vec<_>>()
            .join(",")
    );
    let l = lua.eval(&code);
    let r = |x: Result<bool, ()>| match x {
        Ok(true) => "1".to_owned(),
        Ok(false) => "0".to_owned(),
        Err(()) => "E".to_owned(),
    };
    let mut rust = Vec::new();
    let mut names = Vec::new();
    for (i, gem) in gems.iter().enumerate() {
        let other = &gems[(i + 1) % gems.len()];
        let mut ts: Vec<String> = base_types.iter().map(|s| (*s).to_owned()).collect();
        ts.push(gem.name.as_str().to_lowercase());
        ts.push(other.name.as_str().to_lowercase());
        let mut bits: Vec<String> = Vec::new();
        for t in &ts {
            bits.push(r(pcall(|| gemIsType(gem, t, false))));
            bits.push(r(pcall(|| gemIsType(gem, t, true))));
        }
        let nms = [
            gem.name.as_str().to_owned(),
            gem.name.as_str().to_lowercase(),
            gem.baseTypeName.map_or("x".to_owned(), |b| b.to_string()),
            "Nonsense Gem".to_owned(),
        ];
        for nm in &nms {
            for dv in [false, true] {
                bits.push(match pcall(|| getGameIdFromGemName(Some(nm), dv)) {
                    Ok(Some(g)) => g.to_string(),
                    Ok(None) => "nil".to_owned(),
                    Err(()) => "E".to_owned(),
                });
            }
        }
        bits.push(r(pcall(|| isGemIdSame(&gem.name, &other.name, true))));
        bits.push(r(pcall(|| isGemIdSame(&gem.name, &gem.name, false))));
        rust.push(bits.join(" "));
        names.push(format!("gem {}", gem.id));
    }
    cmp_records(&mut c, &names, &l, &rust);
    c
}

// ---------------------------------------------------------------------------------------------
// breakdown

fn out_text(t: &Table) -> String {
    let mut parts: Vec<String> = Vec::new();
    for_ipairs!((_i, v) in t => {
        parts.push(match v {
            Val::Str(s) => s.to_string(),
            Val::Num(n) => f17(n),
            other => format!("{other:?}"),
        });
    });
    let r = t.get(n!("radius"));
    if !r.is_nil() {
        parts.push(format!("radius={}", f17(r.num())));
    }
    parts.join("\x1f")
}

const LUA_OUT_TEXT: &str = r#"
local function outText(t)
  if t == nil then return "nil" end
  local parts = {}
  for _, v in ipairs(t) do parts[#parts+1] = type(v) == "number" and string.format("%.17g", v) or tostring(v) end
  if t.radius ~= nil then parts[#parts+1] = "radius=" .. string.format("%.17g", t.radius) end
  return table.concat(parts, "\31")
end
"#;

fn check_breakdown(lua: &mut Lua) -> Check {
    let mut c = Check::new("breakdown");
    let mut rng = Rng(0xC7_0003);
    let player = Actor::new();
    let store = ModStore::ModDB(None);
    let S = store.Sum(ModType::INC, None, &[n!("Life")]);
    let M = store.More(None, &[n!("Life")]);
    let bd = newBreakdown(&store, &Table::new(), &player);
    let skill = ActiveSkill::new(
        SkillEffect::new(
            pob_gamedata::data().skills[&n!("Fireball")].clone(),
            1.0,
            0.0,
        ),
        Shared::new(Vec::new()),
        Rc::downgrade(&player),
    );
    player.mainSkill.set(Some(skill.clone()));

    let opt = |rng: &mut Rng, p: f64| if rng.chance(p) { Some(rng.num()) } else { None };
    let lopt = |x: Option<f64>| x.map_or("nil".to_owned(), lnum);
    let mut lua_cases: Vec<String> = Vec::new();
    let mut rust: Vec<String> = Vec::new();
    let mut names: Vec<String> = Vec::new();
    let mut push = |name: String, l: String, r: String| {
        names.push(name);
        lua_cases.push(l);
        rust.push(r);
    };
    let catch_text = |f: &dyn Fn() -> String| pcall(f).unwrap_or_else(|()| "ERR".to_owned());
    for i in 0..3000 {
        // area
        let (base, areaMod, total) = (
            rng.num(),
            rng.num().abs(),
            if rng.chance(0.3) { 0.0 } else { rng.num() },
        );
        let base = if rng.chance(0.2) { total } else { base };
        let bp: [Option<f64>; 4] = if rng.chance(0.7) {
            [
                Some(rng.below(300) as f64),
                Some(rng.below(300) as f64),
                Some(rng.below(300) as f64),
                opt(&mut rng, 0.9),
            ]
        } else {
            [None, None, None, None]
        };
        let label = if rng.chance(0.5) {
            Some("Radius:")
        } else {
            None
        };
        let radiusMod = match rng.below(3) {
            0 => None,
            1 => Some(1.0),
            _ => Some(rng.num()),
        };
        push(
            format!("area #{i}"),
            format!(
                "outText(bd.area({},{},{},{},{},{},{},{},{}))",
                lnum(base),
                lnum(areaMod),
                lnum(total),
                lopt(bp[0]),
                lopt(bp[1]),
                lopt(bp[2]),
                lopt(bp[3]),
                label.map_or("nil".to_owned(), |s| format!("{s:?}")),
                lopt(radiusMod)
            ),
            catch_text(&|| {
                out_text(&bd.area(
                    base, areaMod, total, bp[0], bp[1], bp[2], bp[3], label, radiusMod,
                ))
            }),
        );
        // effMult
        let dt = ["Physical", "Fire", "Chaos"][rng.below(3) as usize];
        let sourceRes = match rng.below(3) {
            0 => None,
            1 => Some(dt),
            _ => Some("Your Fire Resistance"),
        };
        let (resist, pen, taken, mult, takenMore) = (
            (rng.below(200) as f64) - 50.0,
            if rng.chance(0.4) { 0.0 } else { rng.num() },
            rng.num(),
            rng.num(),
            rng.num(),
        );
        let useRes = rng.chance(0.7);
        let invert = match rng.below(3) {
            0 => None,
            1 => Some(0.0),
            _ => Some(rng.below(100) as f64 / 100.0),
        };
        let eff = opt(&mut rng, 0.5);
        push(
            format!("effMult #{i}"),
            format!(
                "outText(bd.effMult({dt:?},{},{},{},{},{},{},{},{},{}))",
                lnum(resist),
                lnum(pen),
                lnum(taken),
                lnum(mult),
                lnum(takenMore),
                sourceRes.map_or("nil".to_owned(), |s| format!("{s:?}")),
                useRes,
                lopt(invert),
                lopt(eff)
            ),
            catch_text(&|| {
                out_text(&bd.effMult(
                    Name::from(dt),
                    resist,
                    pen,
                    taken,
                    mult,
                    takenMore,
                    sourceRes.map(Name::from),
                    useRes,
                    invert,
                    eff,
                ))
            }),
        );
        // dot
        let (bv, inc, more, total) = (rng.num(), rng.num(), rng.num(), rng.num());
        let (m, rate, aura, em) = (
            opt(&mut rng, 0.5),
            opt(&mut rng, 0.5),
            opt(&mut rng, 0.5),
            opt(&mut rng, 0.5),
        );
        let more = if rng.chance(0.2) { 1.0 } else { more };
        push(
            format!("dot #{i}"),
            format!(
                "(function() local o = {{}} bd.dot(o,{},{},{},{},{},{},{},{}) return outText(o) end)()",
                lnum(bv), lnum(inc), lnum(more), lopt(m), lopt(rate), lopt(aura), lopt(em), lnum(total)
            ),
            catch_text(&|| {
                let o = Table::new();
                bd.dot(&o, bv, inc, more, m, rate, aura, em, total);
                out_text(&o)
            }),
        );
        // critDot
        let cd: [f64; 4] = [
            rng.num(),
            if rng.chance(0.2) { 0.0 } else { rng.num() },
            if rng.chance(0.3) { 0.0 } else { rng.num() },
            if rng.chance(0.3) { 0.0 } else { rng.num() },
        ];
        push(
            format!("critDot #{i}"),
            format!(
                "outText(bd.critDot({},{},{},{}))",
                lnum(cd[0]),
                lnum(cd[1]),
                lnum(cd[2]),
                lnum(cd[3])
            ),
            catch_text(&|| out_text(&bd.critDot(cd[0], cd[1], cd[2], cd[3]))),
        );
        // multiChain: random chains
        let hasBase = rng.chance(0.6);
        let baseV = if rng.chance(0.2) { 0.0 } else { rng.num() };
        let n = rng.below(5);
        let mut entries: Vec<Option<f64>> = Vec::new();
        for _ in 0..n {
            entries.push(match rng.below(4) {
                0 => None,
                1 => Some(1.0),
                _ => Some(rng.num()),
            });
        }
        let lab = rng.chance(0.5);
        let tot = rng.chance(0.3);
        let noTotal = rng.chance(0.3);
        let lchain = format!(
            "{{ {}{}{}{}{} }}",
            if hasBase {
                format!("base = {{ \"%.1f ^8(b)\", {} }}, ", lnum(baseV))
            } else {
                String::new()
            },
            if lab { "label = \"L:\", " } else { "" },
            if tot { "total = \"= T\", " } else { "" },
            if noTotal { "noTotal = true, " } else { "" },
            entries
                .iter()
                .map(|e| format!("{{ \"%.2f ^8(x)\", {} }}, ", lopt(*e)))
                .collect::<String>()
        );
        push(
            format!("multiChain #{i}"),
            format!("(function() local o = {{}} local n = bd.multiChain(o, {lchain}) return outText(o) .. \"|\" .. n end)()"),
            catch_text(&|| {
                let chain = Table::new();
                if hasBase {
                    chain.set(n!("base"), tbl!["%.1f ^8(b)", baseV]);
                }
                if lab {
                    chain.set(n!("label"), "L:");
                }
                if tot {
                    chain.set(n!("total"), "= T");
                }
                if noTotal {
                    chain.set(n!("noTotal"), true);
                }
                for e in &entries {
                    chain.push(tbl!["%.2f ^8(x)", Val::from(*e)]);
                }
                let o = Table::new();
                let n = bd.multiChain(&o, &chain);
                format!("{}|{}", out_text(&o), tostring_num(n))
            }),
        );
        // simple / mod / slot / leech over the empty store
        let eb = opt(&mut rng, 0.6);
        let tot = rng.num();
        push(
            format!("simple #{i}"),
            format!(
                "outText(bd.simple({}, nil, {}, \"Life\"))",
                lopt(eb),
                lnum(tot)
            ),
            catch_text(&|| {
                bd.simple(eb, None, tot, &[n!("Life")])
                    .map_or("nil".to_owned(), |t| out_text(&t))
            }),
        );
        if i == 0 {
            push(
                "mod".to_owned(),
                "outText(bd.mod(fakeDB, nil, \"Life\"))".to_owned(),
                catch_text(&|| {
                    bd.r#mod(&store, None, &[n!("Life")])
                        .map_or("nil".to_owned(), |t| out_text(&t))
                }),
            );
        }
        let (sb, st) = (rng.num(), opt(&mut rng, 0.5));
        let sn = if rng.chance(0.5) {
            Some("Evasion to Armour")
        } else {
            None
        };
        push(
            format!("slot #{i}"),
            format!(
                "(function() bd.Armour = {{ slots = {{}} }} bd.slot(\"Global\", {}, nil, {}, {}, \"Armour\", \"Defences\") local s = bd.Armour.slots[1] \
                 return table.concat({{ string.format(\"%.17g\", s.base), tostring(s.inc), tostring(s.more), s.total, s.source, tostring(s.sourceName), tostring(s.item) }}, \"\\31\") end)()",
                sn.map_or("nil".to_owned(), |s| format!("{s:?}")), lnum(sb), lopt(st)
            ),
            catch_text(&|| {
                let slots = Table::new();
                let arm = Table::new();
                arm.set(n!("slots"), slots.clone());
                bd.t.set(n!("Armour"), arm);
                bd.slot("Global", sn, None, sb, st, &[n!("Armour"), n!("Defences")]);
                let s = slots.table(1_i64).expect("row");
                let tv = |v: Val| match v {
                    Val::Str(s) => s.to_string(),
                    Val::Bool(b) => b.to_string(),
                    Val::Nil => "nil".to_owned(),
                    other => format!("{other:?}"),
                };
                [f17(s.num(n!("base"))), tv(s.get(n!("inc"))), tv(s.get(n!("more"))), tv(s.get(n!("total"))),
                    tv(s.get(n!("source"))), tv(s.get(n!("sourceName"))), tv(s.get(n!("item")))]
                    .join("\x1f")
            }),
        );
        let showAverage = rng.chance(0.5);
        let lv: [f64; 8] = [
            if rng.chance(0.3) { 0.0 } else { rng.num() },
            if rng.chance(0.3) { 0.0 } else { rng.num() },
            if rng.chance(0.3) { 0.0 } else { rng.num() },
            rng.num(),
            rng.num(),
            rng.num(),
            if rng.chance(0.3) { 1.0 } else { rng.num() },
            rng.num(),
        ];
        push(
            format!("leech #{i}"),
            format!(
                "(function() actor.mainSkill.skillData.showAverage = {showAverage} return outText(bd.leech({},{},{},{},\"LifeLeechRate\",{},{},{},{})) end)()",
                lnum(lv[0]), lnum(lv[1]), lnum(lv[2]), lnum(lv[3]), lnum(lv[4]), lnum(lv[5]), lnum(lv[6]), lnum(lv[7])
            ),
            catch_text(&|| {
                skill.skillData.get().set(n!("showAverage"), showAverage);
                out_text(&bd.leech(lv[0], lv[1], lv[2], lv[3], n!("LifeLeechRate"), lv[4], lv[5], lv[6], lv[7]))
            }),
        );
    }
    let mut code = format!(
        "{LUA_OUT_TEXT}\nlocal S, M = {}, {}\nlocal fakeDB = {{ Sum = function() return S end, More = function() return M end }}\n\
         actor = {{ mainSkill = {{ skillData = {{}} }}, itemList = {{}} }}\n\
         bd = require(\"Modules.CalcBreakdown\")(fakeDB, {{}}, actor)\nlocal out = {{}}\n",
        lnum(S),
        lnum(M)
    );
    for (i, l) in lua_cases.iter().enumerate() {
        code.push_str(&format!(
            "do local ok, r = pcall(function() return {l} end) out[{}] = ok and r or \"ERR\" end\n",
            i + 1
        ));
    }
    code.push_str("return table.concat(out, \"\\30\")\n");
    let l = lua.eval(&code);
    cmp_records(&mut c, &names, &l, &rust);
    c
}

// ---------------------------------------------------------------------------------------------
// format

fn check_format(lua: &mut Lua) -> Check {
    use pob_calc::modules::calc_format::{
        formatCalcStr, formatCalcVal, formatNumSep, set_num_sep_settings, NumSepSettings,
    };
    let mut c = Check::new("format");
    let settings = [
        NumSepSettings::default(),
        NumSepSettings {
            showThousandsSeparators: false,
            ..NumSepSettings::default()
        },
        NumSepSettings {
            showThousandsSeparators: true,
            thousandsSeparator: ".".into(),
            decimalSeparator: ",".into(),
        },
        NumSepSettings {
            showThousandsSeparators: true,
            thousandsSeparator: "".into(),
            decimalSeparator: ".".into(),
        },
        NumSepSettings {
            showThousandsSeparators: true,
            thousandsSeparator: " ".into(),
            decimalSeparator: "'".into(),
        },
        // Multi-byte separators are left out: the oracle's `lua-utf8` is a byte-level shim of
        // `string` (pob-oracle jit.rs), while PoB's C module reverses and finds by code point,
        // which the port follows.
    ];
    let strs = [
        "1234567.891",
        "^x00FF00123456.78",
        "^7-1234",
        "abc 12345 def 6.5 and 1000000",
        "^1^2 99",
        "-0.5",
        "12",
        "123",
        "1234",
        "^xZZ1234",
        "100.0",
        "1e+15",
        "x1234567",
    ];
    let mut rng = Rng(0xC7_0004);
    let vals: Vec<(f64, Option<f64>)> = (0..400)
        .map(|_| {
            (
                rng.num() * if rng.chance(0.3) { 1000.0 } else { 1.0 },
                if rng.chance(0.2) {
                    None
                } else {
                    Some(rng.below(4) as f64)
                },
            )
        })
        .collect();
    let mut code = String::from("local out = {}\nlocal function rec(s) out[#out+1] = s end\n");
    let mut rust = Vec::new();
    let mut names = Vec::new();
    for (si, s) in settings.iter().enumerate() {
        code.push_str(&format!(
            "main.showThousandsSeparators = {} main.thousandsSeparator = {:?} main.decimalSeparator = {:?}\n",
            s.showThousandsSeparators, s.thousandsSeparator, s.decimalSeparator
        ));
        set_num_sep_settings(s.clone());
        for st in strs {
            code.push_str(&format!("rec((formatNumSep({st:?})))\n"));
            rust.push(formatNumSep(st));
            names.push(format!("settings {si} formatNumSep({st:?})"));
        }
        for (v, p) in &vals {
            code.push_str(&format!(
                "rec((formatCalcVal({}, {})))\n",
                lnum(*v),
                p.map_or("nil".to_owned(), lnum)
            ));
            rust.push(formatCalcVal(*v, *p));
            names.push(format!("settings {si} formatCalcVal({v}, {p:?})"));
        }
    }
    set_num_sep_settings(NumSepSettings::default());
    // formatCalcStr {output:…} forms
    code.push_str(
        r#"main.showThousandsSeparators = true main.thousandsSeparator = "," main.decimalSeparator = "."
local actor = { output = { Life = 1234.5, Mana = 12345678.25, Str = "text", MainHand = { Speed = 1.5, Name = "Hand" }, Zero = 0 } }
for _, s in ipairs({ "{output:Life}", "{2:output:Life}", "{1:output:MainHand.Speed}", "{output:MainHand.Name} x", "{output:Missing}|",
  "{0:output:Missing}", "{3:output:Mana} / {0:output:Zero}", "{output:Str}{output:Life}", "no tags", "{output:Missing.Foo}", "{2:output:Str}" }) do
  local ok, r = pcall(formatCalcStr, s, actor, {})
  rec(ok and r or "ERR")
end
rec(formatCalcStr("{output:Life}", nil, {}))
return table.concat(out, "\30")
"#,
    );
    let actor = Actor::new();
    let o = actor.output.get();
    o.set(n!("Life"), 1234.5);
    o.set(n!("Mana"), 12_345_678.25);
    o.set(n!("Str"), "text");
    let mh = Table::new();
    mh.set(n!("Speed"), 1.5);
    mh.set(n!("Name"), "Hand");
    o.set(n!("MainHand"), mh);
    o.set(n!("Zero"), 0.0);
    let col = Val::Table(Table::new());
    for s in [
        "{output:Life}",
        "{2:output:Life}",
        "{1:output:MainHand.Speed}",
        "{output:MainHand.Name} x",
        "{output:Missing}|",
        "{0:output:Missing}",
        "{3:output:Mana} / {0:output:Zero}",
        "{output:Str}{output:Life}",
        "no tags",
        "{output:Missing.Foo}",
        "{2:output:Str}",
    ] {
        rust.push(pcall(|| formatCalcStr(s, Some(&actor), &col)).unwrap_or_else(|()| "ERR".into()));
        names.push(format!("formatCalcStr({s:?})"));
    }
    rust.push(formatCalcStr("{output:Life}", None, &col));
    names.push("formatCalcStr(nil actor)".into());
    let l = lua.eval(&code);
    cmp_records(&mut c, &names, &l, &rust);
    c
}

// ---------------------------------------------------------------------------------------------
// uuid

fn check_uuid(lua: &mut Lua) -> Check {
    use pob_calc::modules::common::cacheSkillUUID;
    let mut c = Check::new("uuid");
    let d = pob_gamedata::data();
    let build = Build::new(d);
    let groups: Vec<SocketGroupRef> = (0..3).map(|_| Rc::new(SocketGroup::default())).collect();
    groups[1].slot.set(Some(n!("Weapon 1")));
    groups[2].slot.set(Some(n!("Body Armour")));
    let gems: Vec<GemInstanceRef> = (0..3).map(|_| Rc::new(GemInstance::default())).collect();
    groups[1]
        .gemList
        .set(Shared::new(vec![gems[0].clone(), gems[1].clone()]));
    build
        .skillsTab
        .socketGroupList
        .set(Shared::new(groups.clone()));
    let env = Env::new(
        &build,
        Mode::MAIN,
        Rc::new(Override::default()),
        Rc::new(PassiveSpec::default()),
    );
    // (skill id, group index or none, gem index or none)
    let cases: [(&str, Option<usize>, Option<usize>); 7] = [
        ("Fireball", Some(1), Some(1)),
        ("Fireball", Some(1), Some(0)),
        ("Fireball", Some(1), Some(2)),
        ("SummonRagingSpirit", Some(2), None),
        ("VaalFireball", None, Some(0)),
        ("DarkPact", Some(0), Some(0)),
        ("ArcticArmour", Some(2), Some(1)),
    ];
    let mut code = String::from(
        r#"
local groups = { {}, { slot = "Weapon 1" }, { slot = "Body Armour" } }
local gems = { {}, {}, {} }
groups[2].gemList = { gems[1], gems[2] }
local env = { build = { skillsTab = { socketGroupList = groups } } }
local out = {}
"#,
    );
    let mut rust = Vec::new();
    let mut names = Vec::new();
    for (id, g, gem) in cases {
        let ge = d.skills[&Name::from(id)].clone();
        code.push_str(&format!(
            "out[#out+1] = cacheSkillUUID({{ activeEffect = {{ grantedEffect = data.skills[{id:?}], srcInstance = {} }}, socketGroup = {} }}, env)\n",
            gem.map_or("nil".to_owned(), |i| format!("gems[{}]", i + 1)),
            g.map_or("nil".to_owned(), |i| format!("groups[{}]", i + 1)),
        ));
        let eff = SkillEffect::new(ge, 1.0, 0.0);
        eff.srcInstance.set(gem.map(|i| gems[i].clone()));
        let a = ActiveSkill::new(eff, Shared::new(Vec::new()), std::rc::Weak::new());
        a.socketGroup.set(g.map(|i| groups[i].clone()));
        rust.push(cacheSkillUUID(&a, &env).to_string());
        names.push(format!("{id} {g:?} {gem:?}"));
    }
    code.push_str("return table.concat(out, \"\\30\")\n");
    let l = lua.eval(&code);
    cmp_records(&mut c, &names, &l, &rust);
    c
}

// ---------------------------------------------------------------------------------------------

#[test]
fn ct_oracle() {
    if !init_data() {
        return skip("data/dump not found");
    }
    let Some(src) = find_pob_src() else {
        return skip("PoB checkout not found");
    };
    let _lock = pob_oracle::runner::OracleLock::acquire().unwrap_or_else(|e| panic!("{e}"));
    let t0 = std::time::Instant::now();
    let pob = JitPob::boot_oracle(&src, None).unwrap_or_else(|e| panic!("boot: {e}"));
    let mut lua = Lua(pob);
    eprintln!("PoB booted in {:?}", t0.elapsed());
    let only = std::env::var("CT_ONLY").ok();
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let mut checks: Vec<Check> = Vec::new();
    let all: [(&str, fn(&mut Lua) -> Check); 10] = [
        ("modcache", check_modcache),
        ("type_expr", check_type_expr),
        ("support", check_support),
        ("instance_stats", check_instance_stats),
        ("gem_level", check_gem_level),
        ("stat_map", check_stat_map),
        ("gem_fns", check_gem_fns),
        ("breakdown", check_breakdown),
        ("format", check_format),
        ("uuid", check_uuid),
    ];
    for (name, f) in all {
        if only
            .as_deref()
            .is_some_and(|o| !o.split(',').any(|x| x == name))
        {
            continue;
        }
        let t = std::time::Instant::now();
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&mut lua)));
        match r {
            Ok(c) => {
                eprintln!(
                    "{name}: {} compared, {} mismatches, {} sign-of-zero host artefacts, {} PoB-nondeterministic ({:?})",
                    c.total,
                    c.bad.len(),
                    c.zero_sign,
                    c.nondet,
                    t.elapsed()
                );
                for b in c.bad.iter().take(12) {
                    eprintln!("  {b}");
                }
                checks.push(c);
            }
            Err(p) => {
                let msg = p
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| p.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                    .or_else(|| {
                        p.downcast_ref::<LuaError>()
                            .map(|e| format!("LuaError: {}", e.0))
                    })
                    .unwrap_or_default();
                std::panic::set_hook(hook);
                panic!("check {name} panicked: {msg}");
            }
        }
    }
    std::panic::set_hook(hook);
    let mut failed = false;
    for c in &checks {
        eprintln!(
            "== {}: {} compared, {} differ, {} sign-of-zero host artefacts, {} PoB-nondeterministic",
            c.name,
            c.total,
            c.bad.len(),
            c.zero_sign,
            c.nondet
        );
        failed |= !c.bad.is_empty();
    }
    assert!(!failed, "CT oracle mismatches (see above)");
}

/// The census facts the port relies on (no LuaJIT needed).
#[test]
fn ct_data_census() {
    if !init_data() {
        return skip("data/dump not found");
    }
    let d = pob_gamedata::data();
    // statMapLookup: data.skillStatMap has no groups (the group branch of the fallback cannot
    // append processMod's tag to a typed group).
    let groups = d
        .skillStatMap
        .values()
        .filter(|e| {
            e.items
                .iter()
                .any(|i| matches!(i, pob_gamedata::StatMapItem::Group { .. }))
        })
        .count();
    assert_eq!(groups, 0, "data.skillStatMap groups");
    // validateGemLevel: every skill has a level, so `next(levels)` is never nil.
    assert!(d
        .skills
        .values()
        .all(|g| g.levels.0.iter().any(Option::is_some)));
    // gemData.naturalMaxLevel is always set (typed f64 in the schema).
    assert!(!d.gems.is_empty());
}

/// copyTableSafe keeps cycles and (with preserveMeta) the metatable.
#[test]
fn ct_copy_table_safe() {
    use pob_calc::modules::common::{copyTableSafe, isValueInTable};
    let a = Table::new();
    let b = Table::new();
    a.set(n!("b"), b.clone());
    b.set(n!("a"), a.clone());
    b.set(n!("x"), 5.0);
    a.set(n!("b2"), b.clone());
    let parent = Table::new();
    a.set_meta(Some(IndexMeta::Table(parent.clone())));
    let c = copyTableSafe(&a, false, true);
    let cb = c.table(n!("b")).unwrap();
    assert!(!cb.ptr_eq(&b));
    assert!(
        cb.table(n!("a")).unwrap().ptr_eq(&c),
        "cycle maps to the copy"
    );
    assert!(
        c.table(n!("b2")).unwrap().ptr_eq(&cb),
        "shared subtable copied once"
    );
    assert!(matches!(c.meta(), Some(IndexMeta::Table(p)) if p.ptr_eq(&parent)));
    let shallow = copyTableSafe(&a, true, false);
    assert!(shallow.table(n!("b")).unwrap().ptr_eq(&b));
    assert!(shallow.meta().is_none());
    assert_eq!(isValueInTable(&b, &Val::Num(5.0)), Some(Key::S(n!("x"))));
    assert_eq!(isValueInTable(&b, &Val::Num(6.0)), None);
}

/// cacheData / wipeGlobalCache / supportEnabled on a synthetic env (Common.lua 924-958).
#[test]
fn ct_global_cache() {
    use pob_calc::global_cache::cachedData;
    use pob_calc::modules::common::{cacheData, supportEnabled, wipeGlobalCache};
    if !init_data() {
        return skip("data/dump not found");
    }
    let d = pob_gamedata::data();
    let build = Build::new(d);
    let env = Env::new(
        &build,
        Mode::CALCS,
        Rc::new(Override::default()),
        Rc::new(PassiveSpec::default()),
    );
    let ge = d.skills[&n!("Fireball")].clone();
    let skill = ActiveSkill::new(
        SkillEffect::new(ge, 20.0, 0.0),
        Shared::new(Vec::new()),
        Rc::downgrade(&env.player),
    );
    env.player.mainSkill.set(Some(skill.clone()));
    let out = env.player.output.get();
    out.set(n!("Speed"), 1.25);
    out.set(n!("TotalDPS"), 1000.0);
    cacheData(n!("Fireball_NO_SLOT_1_1"), &env);
    let e = cachedData(Mode::CALCS, n!("Fireball_NO_SLOT_1_1")).expect("cached");
    assert_eq!(e.Name.as_str(), "Fireball");
    assert_eq!(e.Speed.opt_num(), Some(1.25));
    assert_eq!(e.TotalDPS.opt_num(), Some(1000.0));
    assert!(e.ManaCost.is_nil());
    assert!(Rc::ptr_eq(&e.ActiveSkill, &skill));
    assert!(Rc::ptr_eq(&e.Env, &env));
    assert!(cachedData(Mode::MAIN, n!("Fireball_NO_SLOT_1_1")).is_none());
    drop(e);
    wipeGlobalCache();
    assert!(cachedData(Mode::CALCS, n!("Fireball_NO_SLOT_1_1")).is_none());

    let group = Rc::new(SocketGroup::default());
    let g1 = Rc::new(GemInstance::default());
    g1.skillId.set(Some(n!("SupportA")));
    g1.enabled.set(false);
    let g2 = Rc::new(GemInstance::default());
    g2.skillId.set(Some(n!("SupportA")));
    g2.enabled.set(true);
    group.gemList.set(Shared::new(vec![g1, g2]));
    skill.socketGroup.set(Some(group));
    assert!(!supportEnabled(n!("SupportA"), &skill), "first match wins");
    assert!(supportEnabled(n!("SupportB"), &skill), "absent gem: true");
}
