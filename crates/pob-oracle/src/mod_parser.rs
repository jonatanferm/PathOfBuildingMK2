//! PoB's `Modules/ModParser.lua`, run unchanged in the embedded interpreter.
//!
//! The environment mirrors what PoB provides when the module loads:
//! `Data/Global.lua` (ModFlag / KeywordFlag / SkillType), the few
//! `Modules/Common.lua` + `Modules/ModTools.lua` helpers it calls, and the
//! slice of PoB's `data` table it reads (dumped by
//! `pob_diff --dump-modparser-env`). The one change to the Lua source is that
//! the hot `scan` helper is replaced by a native implementation with the same
//! contract.
//!
//! INT-4 (AMENDMENTS A13) deleted `pob_diff` with pob-extract, so an existing
//! `data/lua/modparser_env.lua` can no longer be regenerated; nothing in the
//! workspace uses this module any more.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use ahash::AHashMap;

use pob_gamedata::{Mod, Tag, Value};
use pob_lua::pattern as lua_pattern;
use serde_json::{Map, Value as J};

use crate::lua::{Interp, LuaError, TableRef, Val};

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
            let found = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                lua_pattern::find_bytes(&lower, pattern.as_bytes(), Some(1), plain)
            }))
            .map_err(|p| match p.downcast::<pob_lua::LuaError>() {
                Ok(e) => LuaError::msg(e.0),
                Err(other) => std::panic::resume_unwind(other),
            })?;
            let Some((f_start, f_end, f_caps)) = found else {
                continue;
            };
            let better = match &best {
                None => true,
                Some((bs, be, bl, _, _)) => {
                    f_start < *bs
                        || (f_start == *bs
                            && (f_end > *be || (f_end == *be && pattern.len() > *bl)))
                }
            };
            if better {
                let caps = f_caps
                    .iter()
                    .map(|c| match c {
                        lua_pattern::Capture::Str(s) => Val::bytes(s),
                        lua_pattern::Capture::Pos(p) => Val::Num(*p as f64),
                    })
                    .collect();
                best = Some((f_start, f_end, pattern.len(), v.clone(), caps));
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
//
// ORC-X: the interpreter's tables become `pob_gamedata::Mod` through the same
// decoders the probe injection uses (`crate::mod_json::{decode_mod, decode_tag,
// j_to_value}`), via the pob-state JSON shape. Keys the typed `Tag` has no
// field for, or cannot hold exactly, go to `tag.extra` verbatim.

/// A Lua value as pob-state JSON: tables with string keys, the `1..n` part under
/// `$seq` (an array when there is nothing else), other number keys under `$num`;
/// a nested mod (a table with string `name` and `type`) in the plain-table shape
/// (`$seq` tags), which `j_to_value` reads back as a `Value::Mod`.
fn lua_to_json(v: &Val) -> J {
    match v {
        Val::Nil => J::Null,
        Val::Bool(b) => J::Bool(*b),
        Val::Num(n) => pob_calc::probe::jnum(*n),
        Val::Str(s) => J::String(s.to_str().into_owned()),
        Val::Func(_) => J::String("<function>".into()),
        Val::Table(t) => {
            let arr = t.array();
            let n = arr.len();
            let mut o = Map::new();
            let mut num = Map::new();
            for (k, val) in t.pairs() {
                match k {
                    Val::Num(x) if x.fract() == 0.0 && x >= 1.0 && (x as usize) <= n => {}
                    Val::Num(x) => {
                        num.insert(pob_calc::probe::key_text(x), lua_to_json(&val));
                    }
                    Val::Str(s) => {
                        let key = s.to_str().into_owned();
                        let key = if key.starts_with('$') {
                            format!("${key}")
                        } else {
                            key
                        };
                        o.insert(key, lua_to_json(&val));
                    }
                    _ => {}
                }
            }
            if o.is_empty() && num.is_empty() && n > 0 {
                return J::Array(arr.iter().map(lua_to_json).collect());
            }
            if n > 0 {
                o.insert(
                    "$seq".into(),
                    J::Array(arr.iter().map(lua_to_json).collect()),
                );
            }
            if !num.is_empty() {
                o.insert("$num".into(), J::Object(num));
            }
            J::Object(o)
        }
    }
}

/// A Lua mod table in the pob-state mod shape (`tags` as a list).
fn lua_mod_json(v: &Val) -> Result<J, String> {
    let t = v.as_table().ok_or("mod is not a table")?;
    let mut o = Map::new();
    let tags: Vec<J> = t
        .array()
        .iter()
        .filter(|x| x.truthy())
        .map(lua_to_json)
        .collect();
    for (k, val) in t.pairs() {
        if let Val::Str(s) = k {
            o.insert(s.to_str().into_owned(), lua_to_json(&val));
        }
    }
    o.insert("tags".into(), J::Array(tags));
    Ok(J::Object(o))
}

/// Convert any Lua value to a mod [`Value`].
pub fn lua_to_value(v: &Val) -> Result<Value, String> {
    Ok(crate::mod_json::j_to_value(&lua_to_json(v)))
}

/// Convert a Lua tag table to a [`Tag`].
pub fn lua_to_tag(v: &Val) -> Result<Tag, String> {
    v.as_table().ok_or("tag is not a table")?;
    crate::mod_json::decode_tag(&lua_to_json(v))
}

/// Convert a Lua mod table to a [`Mod`].
pub fn lua_to_mod(v: &Val) -> Result<Mod, String> {
    crate::mod_json::decode_mod(&lua_mod_json(v)?).map(|(m, _)| m)
}

/// Shared handle for callers that need the parser from several places.
pub type SharedModParser = Rc<ModParser>;
