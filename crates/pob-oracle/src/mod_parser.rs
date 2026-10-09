//! PoB's `Modules/ModParser.lua`, run unchanged in the embedded interpreter.
//!
//! The environment mirrors what PoB provides when the module loads:
//! `Data/Global.lua` (ModFlag / KeywordFlag / SkillType), the few
//! `Modules/Common.lua` + `Modules/ModTools.lua` helpers it calls, and the
//! slice of PoB's `data` table it reads (dumped by
//! `pob_diff --dump-modparser-env`). The one change to the Lua source is that
//! the hot `scan` helper is replaced by a native implementation with the same
//! contract.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use ahash::AHashMap;

use crate::lua::{Interp, LuaError, TableRef, Val};
use pob_calc::lua_pattern;
use pob_calc::modifier::{Mod, ModType, Sockets, Table, Tag, TagType, Value};

/// PoB helper functions ModParser expects as globals.
const PRELUDE: &str = r#"
function copyTable(tbl, noRecurse)
	local out = {}
	for k, v in pairs(tbl) do
		if not noRecurse and type(v) == "table" then
			out[k] = copyTable(v)
		else
			out[k] = v
		end
	end
	return out
end
function isValueInArray(tbl, val)
	for i, v in ipairs(tbl) do
		if val == v then
			return i
		end
	end
end
modLib = { }
function modLib.createMod(modName, modType, modVal, ...)
	local flags = 0
	local keywordFlags = 0
	local tagStart = 1
	local source
	if select('#', ...) >= 1 and type(select(1, ...)) == "string" then
		source = select(1, ...)
		tagStart = 2
	end
	if select('#', ...) >= 2 and type(select(2, ...)) == "number" then
		flags = select(2, ...)
		tagStart = 3
	end
	if select('#', ...) >= 3 and type(select(3, ...)) == "number" then
		keywordFlags = select(3, ...)
		tagStart = 4
	end
	return {
		name = modName,
		type = modType,
		value = modVal,
		flags = flags,
		keywordFlags = keywordFlags,
		source = source,
		select(tagStart, ...)
	}
end
"#;

/// Sources the parser is built from (all read from PoB's tree / `data/lua`).
pub struct ModParserSources<'a> {
    pub global_lua: &'a str,
    pub mod_parser_lua: &'a str,
    pub env_lua: &'a str,
}

/// A loaded ModParser.
pub struct ModParser {
    interp: RefCell<Interp>,
    parse_fn: Val,
    /// ModParser's own `parseModCache` table (`line → { modList, extra }`).
    lua_cache: TableRef,
    cache: RefCell<AHashMap<String, (Option<Vec<Mod>>, Option<String>)>>,
}

/// Result of parsing one mod line: `None` mods = unrecognised line; `extra`
/// is the unparsed remainder when only part of the line was understood.
#[derive(Debug, Clone, Default)]
pub struct ParsedLine {
    pub mods: Option<Vec<Mod>>,
    pub extra: Option<String>,
}

fn lua_err(e: LuaError) -> String {
    e.0
}

/// Replace ModParser's `local function scan(...) ... end` with a call into
/// the native `__native_scan`.
fn patch_scan(src: &str) -> Result<String, String> {
    let start = src
        .find("local function scan(line, patternList, plain)")
        .ok_or("ModParser.lua: scan() not found")?;
    let end_rel = src[start..]
        .find("\nend\n")
        .ok_or("ModParser.lua: end of scan() not found")?;
    let end = start + end_rel + "\nend\n".len();
    Ok(format!(
        "{}local function scan(line, patternList, plain) return __native_scan(line, patternList, plain) end\n{}",
        &src[..start],
        &src[end..]
    ))
}

/// Native `scan`: earliest match, then longest match, then longest pattern.
fn native_scan(args: Vec<Val>) -> Result<Vec<Val>, LuaError> {
    let line = match args.first() {
        Some(Val::Str(s)) => s.clone(),
        _ => return Err(LuaError::msg("scan: line expected")),
    };
    let list = match args.get(1) {
        Some(Val::Table(t)) => t.clone(),
        _ => return Err(LuaError::msg("scan: patternList expected")),
    };
    let plain = args.get(2).is_some_and(Val::truthy);
    let lower = line.as_bytes().to_ascii_lowercase();
    // (start, end, patlen, val, caps)
    let mut best: Option<(usize, usize, usize, Val, Vec<Val>)> = None;
    {
        let table = list.0.borrow();
        for (k, (_, v)) in &table.hash {
            let crate::lua::value::Key::Str(pattern) = k else {
                continue;
            };
            if v.is_nil() {
                continue;
            }
            let found = lua_pattern::find(&lower, pattern.as_bytes(), 1, plain)
                .map_err(|e| LuaError::msg(e.0))?;
            let Some(f) = found else { continue };
            let better = match &best {
                None => true,
                Some((bs, be, bl, _, _)) => {
                    f.start < *bs
                        || (f.start == *bs
                            && (f.end > *be || (f.end == *be && pattern.len() > *bl)))
                }
            };
            if better {
                let caps = f
                    .captures
                    .iter()
                    .map(|c| match c {
                        lua_pattern::Capture::Str(s) => Val::bytes(s),
                        lua_pattern::Capture::Pos(p) => Val::Num(*p as f64),
                    })
                    .collect();
                best = Some((f.start, f.end, pattern.len(), v.clone(), caps));
            }
        }
    }
    match best {
        Some((start, end, _, val, caps)) => {
            let bytes = line.as_bytes();
            let mut rest = bytes[..start - 1].to_vec();
            rest.extend_from_slice(&bytes[end.min(bytes.len())..]);
            // `{ cap1, …, cap5 }` — trailing nils dropped, like Lua's constructor.
            let mut caps: Vec<Val> = caps;
            caps.truncate(5);
            Ok(vec![
                val,
                Val::bytes(&rest),
                Val::Table(TableRef::from_array(caps)),
            ])
        }
        None => Ok(vec![Val::Nil, Val::Str(line)]),
    }
}

impl ModParser {
    /// Build from PoB sources.
    pub fn new(src: &ModParserSources<'_>) -> Result<Self, String> {
        let mut it = Interp::new();
        it.set_global("__native_scan", Val::native(|_, a| native_scan(a)));
        it.exec_chunk(PRELUDE, "prelude", Vec::new())
            .map_err(lua_err)?;
        it.exec_chunk(src.global_lua, "Global.lua", Vec::new())
            .map_err(lua_err)?;
        let data = it
            .exec_chunk(src.env_lua, "modparser_env.lua", Vec::new())
            .map_err(lua_err)?
            .into_iter()
            .next()
            .unwrap_or_default();
        it.set_global("data", data);
        let patched = patch_scan(src.mod_parser_lua)?;
        let module = it
            .exec_chunk(&patched, "ModParser.lua", Vec::new())
            .map_err(lua_err)?
            .into_iter()
            .next()
            .unwrap_or_default();
        let parse_fn = module
            .as_table()
            .map(|t| t.get_str("parseMod"))
            .filter(|f| !f.is_nil())
            .ok_or("ModParser.lua did not return parseMod")?;
        let lua_cache = module
            .as_table()
            .and_then(|t| t.get_str("parseModCache").as_table().cloned())
            .ok_or("ModParser.lua did not return parseModCache")?;
        Ok(Self {
            interp: RefCell::new(it),
            parse_fn,
            lua_cache,
            cache: RefCell::new(AHashMap::default()),
        })
    }

    /// Load from a PoB checkout (`src/`) and the dumped env file.
    pub fn from_paths(pob_src: &Path, env_lua: &Path) -> Result<Self, String> {
        let read =
            |p: &Path| std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()));
        let global_lua = read(&pob_src.join("Data/Global.lua"))?;
        let mod_parser_lua = read(&pob_src.join("Modules/ModParser.lua"))?;
        let env_lua = read(env_lua)?;
        Self::new(&ModParserSources {
            global_lua: &global_lua,
            mod_parser_lua: &mod_parser_lua,
            env_lua: &env_lua,
        })
    }

    /// `modLib.parseMod(line)` → typed mods + unparsed remainder.
    pub fn parse(&self, line: &str) -> Result<ParsedLine, String> {
        if let Some((mods, extra)) = self.cache.borrow().get(line) {
            return Ok(ParsedLine {
                mods: mods.clone(),
                extra: extra.clone(),
            });
        }
        self.interp
            .borrow_mut()
            .call(&self.parse_fn, vec![Val::str(line)])
            .map_err(|e| format!("parseMod({line:?}): {e}"))?;
        // Read the raw `{ modList, extra }` PoB cached (the wrapper's
        // `unpack(copyTable(...))` can drop `extra` when modList is nil).
        let entry = self.lua_cache.get_str(line);
        let entry = entry.as_table().ok_or("parseMod did not cache the line")?;
        let mods = match entry.get_int(1) {
            Val::Table(t) => Some(
                t.array()
                    .iter()
                    .map(lua_to_mod)
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            _ => None,
        };
        let extra = entry.get_int(2).as_str().map(str::to_owned);
        self.cache
            .borrow_mut()
            .insert(line.to_owned(), (mods.clone(), extra.clone()));
        Ok(ParsedLine { mods, extra })
    }

    /// Raw Lua access (e.g. for tests that compare tables).
    pub fn with_interp<T>(&self, f: impl FnOnce(&mut Interp) -> T) -> T {
        f(&mut self.interp.borrow_mut())
    }
}

// ---- Lua table → typed Mod ------------------------------------------------------

fn lstr(v: &Val) -> Option<String> {
    v.as_str().map(str::to_owned)
}

fn lnum(v: &Val) -> Option<f64> {
    v.as_num()
}

fn lbool(v: &Val) -> bool {
    v.truthy()
}

fn str_list(v: &Val) -> Option<Vec<String>> {
    v.as_table()
        .map(|t| t.array().iter().filter_map(lstr).collect())
}

fn num_list(v: &Val) -> Option<Vec<f64>> {
    v.as_table()
        .map(|t| t.array().iter().filter_map(lnum).collect())
}

/// Convert any Lua value to a mod [`Value`].
pub fn lua_to_value(v: &Val) -> Result<Value, String> {
    Ok(match v {
        Val::Nil => Value::Nil,
        Val::Bool(b) => Value::Bool(*b),
        Val::Num(n) => Value::Num(*n),
        Val::Str(s) => Value::Str(s.to_string()),
        Val::Func(_) => Value::Str("<function>".into()),
        Val::Table(t) => {
            // A mod table (has name + type) nested as a value.
            let is_mod =
                t.get_str("name").as_str().is_some() && t.get_str("type").as_str().is_some();
            if is_mod {
                return Ok(Value::Mod(Box::new(lua_to_mod(v)?)));
            }
            let mut table = Table::default();
            for (k, val) in t.pairs() {
                match k {
                    Val::Num(n) if n.fract() == 0.0 && n >= 1.0 && (n as usize) <= t.len() => {}
                    Val::Str(s) => table.fields.push((s.to_string(), lua_to_value(&val)?)),
                    other => table.fields.push((other.to_string(), lua_to_value(&val)?)),
                }
            }
            table.array = t
                .array()
                .iter()
                .map(lua_to_value)
                .collect::<Result<_, _>>()?;
            Value::Table(Box::new(table))
        }
    })
}

/// Convert a Lua tag table to a [`Tag`].
pub fn lua_to_tag(v: &Val) -> Result<Tag, String> {
    let t = v.as_table().ok_or("tag is not a table")?;
    let ty = t
        .get_str("type")
        .as_str()
        .map(TagType::parse)
        .ok_or("tag without type")?;
    let mut tag = Tag::new(ty);
    for (k, val) in t.pairs() {
        let Val::Str(key) = &k else { continue };
        match key.as_str() {
            "type" => {}
            "var" => tag.var = lstr(&val),
            "varList" => tag.var_list = str_list(&val),
            "neg" => tag.neg = lbool(&val),
            "actor" => tag.actor = lstr(&val),
            "stat" => tag.stat = lstr(&val),
            "statList" => tag.stat_list = str_list(&val),
            "div" => tag.div = lnum(&val),
            "divVar" => tag.div_var = lstr(&val),
            "base" => tag.base = lnum(&val),
            "noFloor" => tag.no_floor = lbool(&val),
            "invert" => tag.invert = lbool(&val),
            "floor" => tag.floor = lbool(&val),
            "limit" => tag.limit = lnum(&val),
            "limitVar" => tag.limit_var = lstr(&val),
            "limitStat" => tag.limit_stat = lstr(&val),
            "limitTotal" => tag.limit_total = lbool(&val),
            "limitNegTotal" => tag.limit_neg_total = lbool(&val),
            "limitActor" => tag.limit_actor = lstr(&val),
            "percent" => tag.percent = lnum(&val),
            "percentVar" => tag.percent_var = lstr(&val),
            "threshold" => tag.threshold = lnum(&val),
            "thresholdVar" => tag.threshold_var = lstr(&val),
            "thresholdStat" => tag.threshold_stat = lstr(&val),
            "thresholdActor" => tag.threshold_actor = lstr(&val),
            "thresholdPercent" => tag.threshold_percent = lnum(&val),
            "thresholdPercentVar" => tag.threshold_percent_var = lstr(&val),
            "upper" => tag.upper = lbool(&val),
            "equals" => tag.equals = lbool(&val),
            "ramp" => {
                tag.ramp = val.as_table().map(|r| {
                    let arr = r.array();
                    if arr.iter().all(|x| x.as_num().is_some()) {
                        vec![arr.iter().filter_map(Val::as_num).collect()]
                    } else {
                        arr.iter().filter_map(num_list).collect()
                    }
                });
            }
            "itemSlot" => tag.item_slot = lstr(&val),
            "allSlots" => tag.all_slots = lbool(&val),
            "bothSlots" => tag.both_slots = lbool(&val),
            "excludeSelf" => tag.exclude_self = lbool(&val),
            "searchCond" => tag.search_cond = lstr(&val),
            "rarityCond" => tag.rarity_cond = lstr(&val),
            "corruptedCond" => tag.corrupted_cond = Some(lbool(&val)),
            "shaperCond" => tag.shaper_cond = Some(lbool(&val)),
            "elderCond" => tag.elder_cond = Some(lbool(&val)),
            "nameCond" => tag.name_cond = lstr(&val),
            "slotName" => tag.slot_name = lstr(&val),
            "slotNameList" => tag.slot_name_list = str_list(&val),
            "keyword" => tag.keyword = lstr(&val),
            "socketColor" => tag.socket_color = lstr(&val),
            "sockets" => {
                tag.sockets = match &val {
                    Val::Str(s) if s.as_str() == "all" => Some(Sockets::All),
                    Val::Num(n) => Some(Sockets::Count(*n)),
                    Val::Table(_) => num_list(&val).map(Sockets::List),
                    _ => None,
                }
            }
            "skillName" => tag.skill_name = lstr(&val),
            "skillNameList" => tag.skill_name_list = str_list(&val),
            "includeTransfigured" => tag.include_transfigured = lbool(&val),
            "summonSkill" => tag.summon_skill = lbool(&val),
            "skillId" => tag.skill_id = lstr(&val),
            "skillPart" => tag.skill_part = lnum(&val),
            "skillPartList" => tag.skill_part_list = num_list(&val),
            "skillType" => tag.skill_type = lnum(&val).map(|n| n as u32),
            "skillTypeList" => {
                tag.skill_type_list =
                    num_list(&val).map(|v| v.into_iter().map(|n| n as u32).collect())
            }
            "baseFlag" => tag.base_flag = lstr(&val),
            "modFlags" => tag.mod_flags = lnum(&val).unwrap_or(0.0) as u32,
            "keywordFlags" => tag.keyword_flags = lnum(&val).unwrap_or(0.0) as u32,
            "monsterTag" => tag.monster_tag = lstr(&val),
            "monsterTagList" => tag.monster_tag_list = str_list(&val),
            "globalLimit" => tag.global_limit = lnum(&val),
            "globalLimitKey" => tag.global_limit_key = lstr(&val),
            "num" => tag.num = lnum(&val),
            "effectType" => tag.effect_type = lstr(&val),
            "effectName" => tag.effect_name = lstr(&val),
            "effectCond" => tag.effect_cond = lstr(&val),
            "effectEnemyCond" => tag.effect_enemy_cond = lstr(&val),
            "effectStackVar" => tag.effect_stack_var = lstr(&val),
            "effectStackLimit" => tag.effect_stack_limit = lnum(&val),
            "modCond" => tag.mod_cond = lstr(&val),
            "unscalable" => tag.unscalable = lbool(&val),
            "allowTotemBuff" => tag.allow_totem_buff = lbool(&val),
            "applyNotPlayer" => tag.apply_not_player = lbool(&val),
            "applyMinions" => tag.apply_minions = lbool(&val),
            other => tag.extra.push((other.to_owned(), lua_to_value(&val)?)),
        }
    }
    Ok(tag)
}

/// Convert a Lua mod table to a [`Mod`].
pub fn lua_to_mod(v: &Val) -> Result<Mod, String> {
    let t = v.as_table().ok_or("mod is not a table")?;
    let name = lstr(&t.get_str("name")).ok_or("mod without name")?;
    let ty_s = lstr(&t.get_str("type")).ok_or("mod without type")?;
    let ty = ModType::parse(&ty_s).ok_or_else(|| format!("unknown mod type {ty_s}"))?;
    let mut m = Mod::new(name, ty, lua_to_value(&t.get_str("value"))?);
    m.flags = lnum(&t.get_str("flags")).unwrap_or(0.0) as u32;
    m.keyword_flags = lnum(&t.get_str("keywordFlags")).unwrap_or(0.0) as u32;
    m.source = lstr(&t.get_str("source"));
    m.source_slot = lstr(&t.get_str("sourceSlot"));
    for tag in t.array() {
        if tag.truthy() {
            m.tags.push(lua_to_tag(&tag)?);
        }
    }
    Ok(m)
}

/// Shared handle for callers that need the parser from several places.
pub type SharedModParser = Rc<ModParser>;
