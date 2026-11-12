//! @pob-file Modules/ModTools.lua @16de4b82d
//! `modLib` (Modules/ModTools.lua 1-245; `mergeKeystones` 246-259 is
//! `pob_calc::modules::mod_tools_env`). Owner: CT. Signatures frozen by U0
//! (calc doc §4.3).
//!
//! The format functions walk a mod or tag the way Lua's `pairs` walks the
//! table: [`lua_pairs_of_tag`] / [`lua_pairs_of_value`] list the non-nil keys
//! of the Lua table a [`Tag`] / [`Value`] stands for. Every Lua call sorts the
//! keys before printing, so the order of `pairs` itself is never observable
//! here. A jewel closure's `func` has no Rust counterpart: it prints as
//! `function: 0x` and a hash of the closure's spec ([`function_text`]), where
//! Lua prints the closure's address (which differs per process).
#![allow(non_snake_case)]

use std::sync::Arc;

use crate::generated::global::{KeywordFlag, ModFlag};
use crate::modifier::{for_each_tag_field, JewelFuncSpec, Mod, ModType, Tag, TagType};
use crate::value::{AStr, Value, ValueTable};
use crate::Name;
use pob_lua::{lfmt, lua_error, tostring_num};

/// @pob Modules/ModTools.lua:1-32 h=5b927ece
/// Module header: stdlib locals and `modLib = { }` (no Rust counterpart).
const _MODULE_HEADER: () = ();

/// @pob Modules/ModTools.lua:33-67 h=106d7eaa
/// `modLib.createMod(modName, modType, modVal, ...)`: the varargs
/// `source, flags, keywordFlags, tags…` are already split by the caller
/// (L38-49 pick them by type: a string first vararg is the source, numbers in
/// the 2nd/3rd position are the flags; the rest are the tags, `select(tagStart, ...)`).
pub fn createMod(
    modName: Name,
    modType: ModType,
    modVal: impl Into<Value>,
    source: Option<&str>,
    flags: u32,
    keywordFlags: u32,
    tags: &[Tag],
) -> Mod {
    // L58
    Mod {
        name: modName,
        r#type: modType,
        value: modVal.into(),
        flags,
        keywordFlags,
        source: source.map(AStr::from),
        tags: tags.iter().cloned().collect(),
        extra: None,
    }
}

/// @pob Modules/ModTools.lua:68-70 h=f47504bd
/// `modLib.parseMod = modParserModule.parseMod` (pob-modparser `parseMod`).
const _PARSE_MOD_ALIAS: () = ();

/// Tag keys whose Lua value [`Tag`] can hold as written by `parseTags`: a tag
/// built from `(key, value)` pairs keeps a pair in its typed field when the
/// field accepts that Lua value, and in `extra` otherwise (any string a key
/// does not type, e.g. `neg=false` is the truthy string `"false"`).
fn tag_from_pairs(pairs: &[(String, Value)]) -> Tag {
    let to_json = |v: &Value| match v {
        Value::Bool(b) => serde_json::Value::Bool(*b),
        Value::Num(n) => serde_json::Number::from_f64(*n)
            .map_or(serde_json::Value::Null, serde_json::Value::Number),
        Value::Str(s) => serde_json::Value::String(s.to_string()),
        _ => serde_json::Value::Null,
    };
    let mut good = serde_json::Map::new();
    let mut rest: Vec<(Name, Value)> = Vec::new();
    for (k, v) in pairs {
        let mut one = serde_json::Map::new();
        if k != "type" {
            // a key alone must deserialize; `type` is checked with the whole set
            one.insert("type".into(), serde_json::Value::String("Condition".into()));
        }
        one.insert(k.clone(), to_json(v));
        let ok = !matches!(to_json(v), serde_json::Value::Null)
            && <Tag as serde::Deserialize>::deserialize(serde_json::Value::Object(one)).is_ok();
        // a non-boolean value of a boolean key is kept verbatim (it is truthy in Lua)
        if ok {
            good.insert(k.clone(), to_json(v));
        } else {
            rest.push((Name::from(k.as_str()), v.clone()));
        }
    }
    let mut tag = <Tag as serde::Deserialize>::deserialize(serde_json::Value::Object(good))
        .unwrap_or_else(|_| Tag::new(TagType::Other(Name::from(""))));
    if !rest.is_empty() {
        let mut ex = tag.extra.take().map_or_else(ValueTable::new, |b| *b);
        for (k, v) in rest {
            ex.set(k, v);
        }
        tag.extra = Some(Box::new(ex));
    }
    tag
}

/// @pob Modules/ModTools.lua:72-98 h=155ea718
pub fn parseTags(line: &str) -> Vec<Tag> {
    parseTags_opt(Some(line))
}

/// `modLib.parseTags(line)` with a possibly nil `line` (L73).
pub fn parseTags_opt(line: Option<&str>) -> Vec<Tag> {
    // L73
    let line = match line {
        Some(l) if l != "-" => l,
        // L74
        _ => return Vec::new(),
    };
    // L76
    let mut Tags = Vec::new();
    // L77
    for tagGroup in pob_lua::gmatch(line, "([^,]*),?") {
        let tagGroup = tagGroup[0].to_str();
        // L78
        if !tagGroup.is_empty() {
            // L79
            let mut tagSet: Vec<(String, Value)> = Vec::new();
            // L80
            for tag in pob_lua::gmatch(&tagGroup, "([^/]*)/?") {
                let tag = tag[0].to_str();
                // L81
                if !tag.is_empty() {
                    // L82
                    let m = pob_lua::lmatch(&tag, "^(%a+)=(.+)", None);
                    // L83
                    if let Some(m) = m {
                        let tagName = m[0].to_str().into_owned();
                        let mut tagValue: Option<Value> = Some(Value::from(&*m[1].to_str()));
                        // L84 list of all the tag parts that should be numbers
                        // L85
                        if tagName == "threshold" {
                            // L86
                            tagValue = pob_lua::tonumber(&m[1].to_str()).map(Value::Num);
                        }
                        // L88
                        let v = match tagValue {
                            Some(Value::Str(s)) if &*s == "true" => Some(Value::Bool(true)),
                            other => other,
                        };
                        tagSet.retain(|(k, _)| *k != tagName);
                        if let Some(v) = v {
                            tagSet.push((tagName, v));
                        }
                    } else {
                        // L90
                        eprintln!("Error tag invalid: {tag}");
                    }
                }
            }
            // L94
            Tags.push(tag_from_pairs(&tagSet));
        }
    }
    // L97
    Tags
}

/// @pob Modules/ModTools.lua:100-119 h=161a8bac
pub fn parseFormattedSourceMod(line: &str) -> Option<Mod> {
    // L101
    let mut modStrings: Vec<String> = Vec::new();
    // L102
    for line2 in pob_lua::gmatch(line, "([^|]*)|?") {
        // L103
        modStrings.push(line2[0].to_str().into_owned());
    }
    // L105
    if modStrings.len() >= 4 {
        // L106
        let value = if modStrings[0] == "true" {
            Value::Bool(true)
        } else {
            Value::Num(pob_lua::tonumber(&modStrings[0]).unwrap_or(0.0))
        };
        let mut m = Mod {
            value,
            source: Some(AStr::from(modStrings[1].as_str())),
            name: Name::from(modStrings[2].as_str()),
            // a type string `ModType` does not know is kept in `extra["type"]` (see `mod_type_text`)
            r#type: ModType::parse(&modStrings[3]).unwrap_or(ModType::Nil),
            // LUA-QUIRK: ModFlag["Attack,Hit"] is nil, so a formatted multi-flag list gives 0
            flags: modStrings
                .get(4)
                .and_then(|s| ModFlag::by_name(s))
                .unwrap_or(0),
            keywordFlags: modStrings
                .get(5)
                .and_then(|s| KeywordFlag::by_name(s))
                .unwrap_or(0),
            tags: smallvec::SmallVec::new(),
            extra: None,
        };
        if ModType::parse(&modStrings[3]).is_none() {
            let mut ex = ValueTable::new();
            ex.set(pob_lua::n!("type"), Value::from(modStrings[3].as_str()));
            m.extra = Some(Box::new(ex));
        }
        // L114
        for tag in parseTags_opt(modStrings.get(6).map(String::as_str)) {
            // L115
            m.tags.push(tag);
        }
        // L117
        return Some(m);
    }
    None
}

/// @pob Modules/ModTools.lua:121-134 h=881ee494
pub fn compareModParams(a: &Mod, b: &Mod) -> bool {
    let (modA, modB) = (a, b);
    // L122
    if modA.name != modB.name
        || mod_type_text(modA) != mod_type_text(modB)
        || lua_flags(modA) != lua_flags(modB)
        || modA.keywordFlags != modB.keywordFlags
        || modA.tags.len() != modB.tags.len()
    {
        // L123
        return false;
    }
    // L125
    for (i, tag) in modA.tags.iter().enumerate() {
        // L126
        if tag_type_val(tag) != tag_type_val(&modB.tags[i]) {
            // L127
            return false;
        }
        // L129
        if formatTag(tag) != formatTag(&modB.tags[i]) {
            // L130
            return false;
        }
    }
    // L133
    true
}

/// `mod.type` as Lua sees it: the enum's text (`"nil"` for nil), or a type
/// string `ModType` does not know, kept in `extra["type"]` (only
/// `parseFormattedSourceMod` makes such mods).
pub fn mod_type_text(m: &Mod) -> String {
    match (m.r#type, m.extra_get(pob_lua::n!("type"))) {
        (ModType::Nil, Some(Value::Str(s))) => s.to_string(),
        (t, _) => t.as_str().to_owned(),
    }
}

/// `mod.flags` as Lua sees it: a number, or the tag table some data passes as
/// flags (kept in `extra["flags"]`, SkillStatMap.lua:509).
fn lua_flags(m: &Mod) -> Value {
    match m.extra_get(pob_lua::n!("flags")) {
        Some(raw) => raw.clone(),
        None => Value::Num(f64::from(m.flags)),
    }
}

/// `tag.type` (nil when the tag table has no `type`; a non-table tag raises).
fn tag_type_val(t: &Tag) -> Option<Name> {
    match t.r#type {
        TagType::LuaNonTable => lua_error!("attempt to index field '?' (a number value)"),
        TagType::Other(n) if n.is_empty() => None,
        ref ty => Some(Name::from(ty.as_str())),
    }
}

/// @pob Modules/ModTools.lua:136-149 h=7408756c
/// `src` is `ModFlag` / `KeywordFlag` as `(name, value)` entries
/// (`ModFlag::ENTRIES`); Lua iterates it with `pairs` and sorts the names.
pub fn formatFlags(flags: u32, src: &[(&str, u32)]) -> String {
    // L137
    let mut flagNames: Vec<&str> = Vec::new();
    // L138
    for &(name, val) in src {
        // L139
        if flags & val == val {
            // L140
            flagNames.push(name);
        }
    }
    // L143 (distinct names: any correct sort gives LuaJIT's order)
    flagNames.sort_unstable();
    // L144
    let mut ret: Option<String> = None;
    // L145
    for name in flagNames {
        // L146
        ret = Some(match ret {
            Some(r) => r + "," + name,
            None => name.to_owned(),
        });
    }
    // L148
    ret.unwrap_or_else(|| "-".to_owned())
}

/// A key of a Lua table walked by `pairs` in the format functions.
#[derive(Clone, Debug, PartialEq)]
pub enum LuaKey {
    Num(f64),
    Str(Name),
}

impl LuaKey {
    fn text(&self) -> String {
        match self {
            LuaKey::Num(n) => tostring_num(*n),
            LuaKey::Str(s) => s.as_str().to_owned(),
        }
    }
}

/// The prefix every jewel closure's text starts with ([`function_text`]); Lua's
/// `tostring(func)` of a closure is `function: 0x` and its address.
pub const FUNCTION_TEXT_PREFIX: &str = "function: 0x";

/// The text a jewel closure prints as (`tostring(func)` in `formatValue`, ModTools.lua:198).
///
/// Lua prints the closure's address, so two closures print differently and one closure prints
/// the same wherever it is formatted (PoB's parse cache, ModParser.lua:6994-7018, gives every
/// parse of a line the same closure). `node.modKey` (PassiveTree.lua:848) is built from this
/// text and keys the power report's cache (CalcsTab.lua:613), so it must tell closures apart
/// (CCR-0141; FUP2, FOLLOWUPS "PWR" item 2; the port used to print one `function: builtin` for all).
/// The port's closure is its spec: the jewelFuncList key (or defining closure) and the
/// captured arguments (`kind` follows the key). The text keeps Lua's shape, `function: 0x` and
/// 16 hex digits, with a hash of the spec (FNV-1a 64) standing in for the address: equal for
/// equal specs, which run the same code on the same captures, and the same in every process
/// (an address is not). Readers that compare with PoB normalise the hex part as for PoB's
/// address (`function: 0x%x+`).
pub fn function_text(j: &JewelFuncSpec) -> String {
    let spec = format!("{}\0{:?}\0{:?}", j.key.as_str(), j.kind, &j.args[..]);
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in spec.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{FUNCTION_TEXT_PREFIX}{h:016x}")
}

/// Every non-nil key of the Lua table `tag` stands for, with its value
/// (`type` first, then the typed fields, then `extra`, where a later entry
/// replaces an earlier one of the same key: a numeric key that holds a Lua
/// string keeps the string in `extra`). A non-table tag (flags passed as a
/// tag) raises, as `pairs(number)` does.
pub fn lua_pairs_of_tag(t: &Tag) -> Vec<(LuaKey, Value)> {
    if t.r#type == TagType::LuaNonTable {
        let v = t
            .extra
            .as_ref()
            .map_or(Value::Nil, |e| e.get(pob_lua::n!("$value")).clone());
        lua_error!(
            "bad argument #1 to 'pairs' (table expected, got {})",
            v.lua_type()
        );
    }
    let no_type = matches!(t.r#type, TagType::Other(n) if n.is_empty());
    let mut out: Vec<(LuaKey, Value)> = Vec::new();
    for_each_tag_field(t, |k, v| {
        if k == "type" && no_type {
            return;
        }
        if let Some(e) = out
            .iter_mut()
            .find(|(ok, _)| *ok == LuaKey::Str(Name::from(k)))
        {
            e.1 = v;
        } else {
            out.push((LuaKey::Str(Name::from(k)), v));
        }
    });
    out
}

/// Every non-nil key of the Lua table a table-like [`Value`] stands for
/// (`None` for a value that is not a table).
pub fn lua_pairs_of_value(v: &Value) -> Option<Vec<(LuaKey, Value)>> {
    match v {
        Value::Table(t) => {
            let mut out: Vec<(LuaKey, Value)> = Vec::new();
            for (i, x) in t.array.iter().enumerate() {
                if !x.is_nil() {
                    out.push((LuaKey::Num((i + 1) as f64), x.clone()));
                }
            }
            for (k, x) in &t.fields {
                if !x.is_nil() {
                    // "#<n>": a numeric key outside the sequence (dump encoding)
                    let key = k
                        .as_str()
                        .strip_prefix('#')
                        .and_then(pob_lua::tonumber)
                        .map_or(LuaKey::Str(*k), LuaKey::Num);
                    out.push((key, x.clone()));
                }
            }
            Some(out)
        }
        Value::Mod(m) => Some(lua_pairs_of_mod(m)),
        Value::JewelFunc(j) => {
            let mut out: Vec<(LuaKey, Value)> = Vec::new();
            if let Some(t) = j.r#type {
                out.push((LuaKey::Str(pob_lua::n!("type")), Value::from(t)));
            }
            if let Some(r) = j.radius {
                out.push((LuaKey::Str(pob_lua::n!("radius")), Value::from(r)));
            }
            out.push((
                LuaKey::Str(pob_lua::n!("func")),
                Value::from(function_text(j).as_str()),
            ));
            Some(out)
        }
        _ => None,
    }
}

/// The keys of a mod table: its fields, then its tags at `1..n`.
fn lua_pairs_of_mod(m: &Mod) -> Vec<(LuaKey, Value)> {
    let mut out: Vec<(LuaKey, Value)> =
        vec![(LuaKey::Str(pob_lua::n!("name")), Value::from(m.name))];
    if m.r#type != ModType::Nil || m.extra_get(pob_lua::n!("type")).is_some() {
        out.push((
            LuaKey::Str(pob_lua::n!("type")),
            Value::from(mod_type_text(m).as_str()),
        ));
    }
    if !m.value.is_nil() {
        out.push((LuaKey::Str(pob_lua::n!("value")), m.value.clone()));
    }
    out.push((LuaKey::Str(pob_lua::n!("flags")), lua_flags(m)));
    out.push((
        LuaKey::Str(pob_lua::n!("keywordFlags")),
        Value::Num(f64::from(m.keywordFlags)),
    ));
    if let Some(s) = &m.source {
        out.push((LuaKey::Str(pob_lua::n!("source")), Value::Str(s.clone())));
    }
    if let Some(ex) = &m.extra {
        for (k, v) in &ex.fields {
            if k.as_str() != "flags" && k.as_str() != "type" && !v.is_nil() {
                out.push((LuaKey::Str(*k), v.clone()));
            }
        }
    }
    for (i, t) in m.tags.iter().enumerate() {
        out.push((
            LuaKey::Num((i + 1) as f64),
            Value::Table(Arc::new(crate::modifier::tag_to_value_table(t))),
        ));
    }
    out
}

/// `tostring(v)` of a non-table value.
fn tostring_value(v: &Value) -> String {
    match v {
        Value::Nil => "nil".to_owned(),
        Value::Bool(b) => b.to_string(),
        Value::Num(n) => tostring_num(*n),
        Value::Str(s) => s.to_string(),
        // tables print their address in Lua; only non-tables reach here
        Value::Mod(_) | Value::Table(_) | Value::JewelFunc(_) => "table".to_owned(),
    }
}

/// `t_sort(paramNames)` over `pairs` keys: numbers compare numerically,
/// strings bytewise; a mix raises (LuaJIT's `<` on number and string).
fn sort_keys(keys: &mut [LuaKey]) {
    pob_lua::sort(keys, |a, b| match (a, b) {
        (LuaKey::Num(x), LuaKey::Num(y)) => x < y,
        (LuaKey::Str(x), LuaKey::Str(y)) => x.as_str().as_bytes() < y.as_str().as_bytes(),
        (LuaKey::Num(_), LuaKey::Str(_)) => {
            lua_error!("attempt to compare number with string")
        }
        (LuaKey::Str(_), LuaKey::Num(_)) => {
            lua_error!("attempt to compare string with number")
        }
    });
}

/// The `paramNames` of `formatTag` / `formatValue` (L152-164 / L200-212):
/// the keys other than `type`, sorted, with `type` first when present.
fn param_names(pairs: &[(LuaKey, Value)]) -> Vec<LuaKey> {
    let type_key = LuaKey::Str(pob_lua::n!("type"));
    // L152 / L200
    let mut paramNames: Vec<LuaKey> = Vec::new();
    // L153 / L201
    let mut haveType = false;
    // L154 / L202
    for (name, _) in pairs {
        // L155 / L203
        if *name == type_key {
            // L156 / L204
            haveType = true;
        } else {
            // L158 / L206
            paramNames.push(name.clone());
        }
    }
    // L161 / L209
    sort_keys(&mut paramNames);
    // L162 / L210
    if haveType {
        // L163 / L211
        paramNames.insert(0, type_key);
    }
    paramNames
}

/// `formatTag(tag)` over the pairs of any Lua table (L151-186).
fn format_tag_pairs(pairs: &[(LuaKey, Value)]) -> String {
    let paramNames = param_names(pairs);
    // L165
    let mut str = String::new();
    // L166
    for (i, paramName) in paramNames.iter().enumerate() {
        // L167
        if i + 1 > 1 {
            // L168
            str.push('/');
        }
        // L170
        let val = pairs
            .iter()
            .find(|(k, _)| k == paramName)
            .map_or(Value::Nil, |(_, v)| v.clone());
        // L171
        let text = if let Some(tbl) = lua_pairs_of_value(&val) {
            let first = tbl
                .iter()
                .find(|(k, _)| *k == LuaKey::Num(1.0))
                .map(|(_, v)| v.clone())
                .unwrap_or(Value::Nil);
            // L172
            let inner = if first.truthy() {
                // L173
                if lua_pairs_of_value(&first).is_some() {
                    // L174
                    format_tags_value(&val)
                } else {
                    // L176
                    table_concat(&val, ",")
                }
            } else {
                // L179
                format_tag_pairs(&tbl)
            };
            // L181
            format!("{{{inner}}}")
        } else {
            tostring_value(&val)
        };
        // L183
        str.push_str(&lfmt!("%s=%s", paramName.text().as_str(), text.as_str()));
    }
    // L185
    str
}

/// `formatTags(val)` of a table value: `formatTag` of each `ipairs` element.
fn format_tags_value(v: &Value) -> String {
    let elems = ipairs_of_value(v);
    let mut ret: Option<String> = None;
    for e in elems {
        let Some(p) = lua_pairs_of_value(&e) else {
            lua_error!(
                "bad argument #1 to 'pairs' (table expected, got {})",
                e.lua_type()
            )
        };
        let t = format_tag_pairs(&p);
        ret = Some(match ret {
            Some(r) => r + "," + &t,
            None => t,
        });
    }
    ret.unwrap_or_else(|| "-".to_owned())
}

/// `ipairs(v)`: the `1..n` elements of a table-like value.
fn ipairs_of_value(v: &Value) -> Vec<Value> {
    match v {
        Value::Table(t) => t
            .array
            .iter()
            .take_while(|x| !x.is_nil())
            .cloned()
            .collect(),
        Value::Mod(m) => m
            .tags
            .iter()
            .map(|t| Value::Table(Arc::new(crate::modifier::tag_to_value_table(t))))
            .collect(),
        _ => Vec::new(),
    }
}

/// `table.concat(val, sep)`: strings and numbers (`%.14g`) only.
fn table_concat(v: &Value, sep: &str) -> String {
    let mut out = String::new();
    for (i, e) in ipairs_of_value(v).iter().enumerate() {
        if i > 0 {
            out.push_str(sep);
        }
        match e {
            Value::Str(s) => out.push_str(s),
            Value::Num(n) => out.push_str(&tostring_num(*n)),
            _ => lua_error!("invalid value (at index {}) in table for 'concat'", i + 1),
        }
    }
    out
}

/// @pob Modules/ModTools.lua:151-186 h=f30d60db
pub fn formatTag(t: &Tag) -> String {
    format_tag_pairs(&lua_pairs_of_tag(t))
}

/// @pob Modules/ModTools.lua:188-194 h=954b43a1
pub fn formatTags(t: &[Tag]) -> String {
    let tagList = t;
    // L189
    let mut ret: Option<String> = None;
    // L190
    for tag in tagList {
        // L191
        let f = formatTag(tag);
        ret = Some(match ret {
            Some(r) => r + "," + &f,
            None => f,
        });
    }
    // L193
    ret.unwrap_or_else(|| "-".to_owned())
}

/// @pob Modules/ModTools.lua:196-225 h=27b487f0
pub fn formatValue(v: &Value) -> String {
    let value = v;
    // L197
    let Some(pairs) = lua_pairs_of_value(value) else {
        // L198
        return tostring_value(value);
    };
    let paramNames = param_names(&pairs);
    // L213
    let mut ret = String::new();
    // L214
    for (i, paramName) in paramNames.iter().enumerate() {
        // L215
        if i + 1 > 1 {
            // L216
            ret.push('/');
        }
        let pv = pairs
            .iter()
            .find(|(k, _)| k == paramName)
            .map_or(Value::Nil, |(_, v)| v.clone());
        // L218
        if *paramName == LuaKey::Str(pob_lua::n!("mod")) {
            // L219
            let inner = match &pv {
                Value::Mod(m) => formatMod(m),
                other => lua_error!(
                    "formatMod: value.mod is a {} value, not a mod",
                    other.lua_type()
                ),
            };
            ret.push_str(&lfmt!("%s=[%s]", paramName.text().as_str(), inner.as_str()));
        } else {
            // L221
            ret.push_str(&lfmt!(
                "%s=%s",
                paramName.text().as_str(),
                formatValue(&pv).as_str()
            ));
        }
    }
    // L224
    format!("{{{ret}}}")
}

/// @pob Modules/ModTools.lua:227-229 h=aba0488a
pub fn formatModParams(m: &Mod) -> String {
    let flags = match lua_flags(m) {
        Value::Num(_) => m.flags,
        other => lua_error!(
            "bad argument #1 to 'band' (number expected, got {})",
            other.lua_type()
        ),
    };
    // L228
    lfmt!(
        "%s|%s|%s|%s|%s",
        m.name,
        mod_type_text(m).as_str(),
        formatFlags(flags, ModFlag::ENTRIES).as_str(),
        formatFlags(m.keywordFlags, KeywordFlag::ENTRIES).as_str(),
        formatTags(&m.tags).as_str()
    )
}

/// @pob Modules/ModTools.lua:231-233 h=22f8d9af
pub fn formatMod(m: &Mod) -> String {
    // L232
    formatValue(&m.value) + " = " + &formatModParams(m)
}

/// @pob Modules/ModTools.lua:235-237 h=0322ee29
pub fn formatSourceMod(m: &Mod) -> String {
    // L236
    lfmt!(
        "%s|%s|%s",
        formatValue(&m.value).as_str(),
        m.source.as_deref(),
        formatModParams(m).as_str()
    )
}

/// @pob Modules/ModTools.lua:239-245 h=4822ac80
/// Returns the re-sourced copy (ModTools.lua:239-246 mutates; §3.6: callers
/// pass a fresh copy or a mod under construction, except `mergeKeystones`,
/// CCR-0091).
pub fn setSource(m: &Mod, source: &str) -> Mod {
    let mut r#mod = m.clone();
    // L240
    r#mod.source = Some(AStr::from(source));
    // L241
    if let Some(inner) = r#mod.value.inner_mod() {
        // L242
        let mut inner = (**inner).clone();
        inner.source = Some(AStr::from(source));
        if let Value::Table(t) = &r#mod.value {
            r#mod.value = Value::Table(Arc::new(
                t.with(pob_lua::n!("mod"), Value::Mod(Arc::new(inner))),
            ));
        }
    }
    // L244
    r#mod
}

#[cfg(test)]
mod fup2_function_text_tests {
    use super::*;
    use crate::modifier::JewelFuncKind;

    fn jewel_func(key: &str, args: &[Value]) -> Value {
        Value::JewelFunc(Arc::new(JewelFuncSpec {
            key: Name::from(key),
            kind: JewelFuncKind::Other,
            args: args.iter().cloned().collect(),
            r#type: Some(Name::from("Other")),
            radius: Some(Name::from("Large")),
        }))
    }

    /// FOLLOWUPS "PWR" item 2: two different closures print differently (Lua: two addresses),
    /// one closure the same each time, in Lua's `function: 0x%x+` shape.
    #[test]
    fn distinct_closures_format_distinctly() {
        let fire = jewel_func("modparser:2630-2634", &[Value::from("fire")]);
        let cold = jewel_func("modparser:2630-2634", &[Value::from("cold")]);
        let other = jewel_func("$default", &[Value::from("fire")]);
        let (f, c, o) = (formatValue(&fire), formatValue(&cold), formatValue(&other));
        assert_ne!(
            f, c,
            "the same function with other captures is another closure"
        );
        assert_ne!(f, o);
        assert_eq!(f, formatValue(&fire.clone()));
        assert_eq!(
            f,
            formatValue(&jewel_func("modparser:2630-2634", &[Value::from("fire")]))
        );
        // `{type=Other/func=function: 0x…/radius=Large}`
        let head = "{type=Other/func=function: 0x";
        let tail = "/radius=Large}";
        assert!(f.starts_with(head) && f.ends_with(tail), "{f}");
        let hex = &f[head.len()..f.len() - tail.len()];
        assert_eq!(hex.len(), 16, "{f}");
        assert!(hex.bytes().all(|b| b.is_ascii_hexdigit()), "{f}");
    }
}
