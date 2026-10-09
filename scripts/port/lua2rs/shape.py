"""Output shape of lua2rs (configurable).

U0 has not frozen the Rust types yet, so every Rust spelling the emitter produces comes
from this table. The defaults follow the code listings of calc doc §4.3 (Mod, Tag, tag!,
createMod), §5.1/§5.3 (Val, ModList::NewMod), §5.8 (Build), §6 (Caps, ModSpec, TagVal,
SpecialVal, PatternEntry, PatternList), §8.1 (FnRow, ConfigApply) and the translation
table of §3.2. Override any key with `--shape FILE.json` (a JSON object merged
recursively over these defaults).

Templates are Python `str.format` strings; literal braces are doubled.
"""

import copy
import json

DEFAULT_SHAPE = {
    # Lines after the `//! @pob-file` header of every generated file.
    "prelude": {
        "modparser": [
            "#![allow(non_snake_case, non_upper_case_globals, unused_variables, unused_mut, unused_parens, unused_imports, clippy::all)]",
            "use std::sync::{Arc, LazyLock};",
            "use pob_lua::{cat, lfmt, lua_error, n, nm, Name};",
            "use pob_lua::bit::{band, bor, bnot};",
            "use pob_lua::format::{tonumber, tostring_num};",
            "use pob_lua::math::{lmod, lpow, m_ceil, m_floor, m_huge, m_max, m_min, m_abs, round};",
            "use pob_lua::pattern::{gsub, gsub_fn};",
            "use pob_lua::string::{firstToUpper, len, lower, sub, upper};",
            "use pob_gamedata::{data, tag, AStr, KeywordFlag, Mod, ModFlag, ModType, ModType::*, SkillType, Sockets, Tag, Value, ValueTable};",
            "use crate::types::{Caps, ModSpec, PatternEntry, PatternList, SpecialVal, TagVal};",
            "use crate::helpers::*;",
        ],
        "config": [
            "#![allow(non_snake_case, non_upper_case_globals, unused_variables, unused_mut, unused_parens, unused_imports, clippy::all)]",
            "use std::sync::Arc;",
            "use pob_lua::{cat, lfmt, lua_error, n, nm, Name};",
            "use pob_lua::bit::{band, bor, bnot};",
            "use pob_lua::format::{tonumber, tostring_num};",
            "use pob_lua::math::{lmod, lpow, m_ceil, m_floor, m_huge, m_max, m_min, m_abs, round};",
            "use pob_lua::string::{firstToUpper, len, lower, sub, upper};",
            "use pob_gamedata::{data, tag, AStr, KeywordFlag, Mod, ModFlag, ModType, ModType::*, SkillType, Sockets, Tag, Value, ValueTable};",
            "use pob_gamedata::mod_tools::createMod;",
            "use crate::lua_table::Val;",
            "use crate::classes::ModList;",
            "use crate::model::BuildRef;",
            "use crate::data_fns::FnRow;",
            "use crate::modules::config_options::ConfigApply;",
            "use crate::unported;",
        ],
    },
    # ---- names, types, constructors
    "name_lit": "n!({s})",                 # Lua string literal used as a Name
    "name_cat": "cat!({parts})",           # Lua `a .. b` used as a Name
    "name_from_str": "Name::from({e})",    # any other string expression (`{e}` is a &str) as a Name
    "modtype": "{t}",                      # ModType variant; doc §3.2 writes bare `BASE` (prelude imports ModType::*)
    "mod_fn": "r#mod",                     # ModParser.lua:2011 `local mod = modLib.createMod` (helpers.rs [MP0])
    "flag_fn": "flag",                     # ModParser.lua:2012-2014 (helpers.rs [MP0])
    "createMod_fn": "createMod",           # modLib.createMod (pob_gamedata::mod_tools, §4.3)
    "mod_call": "{fn}({name}, {ty}, {value}, {source}, {flags}, {kw}, &[{tags}])",
    "flag_call": "{fn}({name}, {source}, {flags}, {kw}, &[{tags}])",
    "newmod_call": "{obj}.NewMod({name}, {ty}, {value}, {source}, {flags}, {kw}, &[{tags}])",
    "addmod_call": "{obj}.AddMod(Arc::new({mod}))",  # NewMod with a nil source (ModStore.lua:109-111)
    "source_none": "None",
    "source_some": "Some({s})",
    "tag": "tag!({{ type = {t}{fields} }})",
    "tag_type": "{t}",                     # `type = Condition` inside tag! (doc §4.3)
    "value_nil": "Value::Nil",
    "value_from": "Value::from({e})",
    "vtab_call": "vtab(vec![{array}], vec![{fields}])",
    "vtab_field": "(n!({k}), {v})",
    "mods_vec": "vec![{items}]",
    "flag_const": "{group}::{k}",          # ModFlag.Attack -> ModFlag::Attack
    "skilltype_const": "SkillType::{k}",
    "data_fn": "data()",
    # ---- ModParser pattern lists (doc §6)
    "pattern_entry": "PatternEntry {{ pat: {pat}, val: {val}, lua_line: {line} }}",
    "list_types": {
        "formList": "&'static str",
        "modNameList": "ModSpec",
        "modFlagList": "ModSpec",
        "preFlagList": "TagVal",
        "modTagList": "TagVal",
        "specialModList": "SpecialVal",
    },
    "list_decl": "pub static {name}: LazyLock<PatternList<{ty}>> = LazyLock::new(|| {{\n    let mut v: Vec<PatternEntry<{ty}>> = Vec::with_capacity({n});\n{calls}\n    PatternList(v)\n}});",
    "block_fn": "fn {name}(v: &mut Vec<PatternEntry<{ty}>>) {{",
    "block_size": 50,                       # entries per @pob block (doc §8: "≈50 entries")
    "range_push_fn": "pub fn {name}(v: &mut Vec<PatternEntry<{ty}>>) {{",
    "spec_struct": "ModSpec {{ {fields}..Default::default() }}",
    "spec_names_fn": "spec_names",          # file-local helper: string modNameList entry -> ModSpec
    "spec_name_table_fn": "spec_name_table",  # file-local helper: names-only table modNameList entry
    "spec_name_table_field": "modNameIsTable",  # ModSpec flag: modName came from a (shared) table (CCR-0006)
    "special_mods": "SpecialVal::Mods({mods})",
    "special_fn": "SpecialVal::Fn({closure})",
    "tag_spec": "TagVal::Spec({spec})",
    "tag_fn": "TagVal::Fn({closure})",
    "closure": "|caps: &Caps| {body}",
    "caps_num": "caps.num",
    "caps_cap": "caps.cap[{i}]",
    "unported": "unported!({site})",
    "unported_macro_note": "// LUA2RS-UNTRANSLATED ({reason}): hand-port the Lua below",
    # ---- ConfigOptions apply (doc §8.1)
    "config_fn": "fn {name}({val}: &Val, {modList}: &ModList, {enemyModList}: &ModList, {build}: &BuildRef) {{",
    "config_row": "FnRow {{ id: {id}, hash: {hash}, f: {name} }}",
    "config_rows_decl": "pub static ROWS: &[FnRow<ConfigApply>] = &[",
    "config_input_get": "{build}.configTab.input.get(n!({k}))",
    # ---- helper fns (ModParser.lua 13-71, 2011-2129: helpers.rs [MP0]); see docs/port/ccr for the proposal
    # param kinds: str (&str), optstr (Option<&str>), tonumber (Option<f64>; the Lua body calls tonumber()),
    #   optnum (Option<f64>), bool (truthiness), optbool (Option<bool>), tags (rest args -> &[Tag]),
    #   opts:<Struct> (table literal -> struct literal, see helper_structs)
    #   luaarg (LuaArg::{Nil,Num,Str}: the Lua body uses the value both as a number and as a string)
    # return kinds: mods (Vec<Mod>), optmods (Option<Vec<Mod>>), str (String), optname (Option<Name>)
    "helpers": {
        "firstToUpper": {"rust": "firstToUpper", "params": ["str"], "ret": "str"},
        "triggerExtraSkill": {"rust": "triggerExtraSkill", "params": ["str", "tonumber", "opts:TriggerExtraSkillOptions"], "ret": "mods"},
        "grantedExtraSkill": {"rust": "grantedExtraSkill", "params": ["str", "tonumber", "optbool"], "ret": "optmods"},
        "extraSupport": {"rust": "extraSupport", "params": ["str", "tonumber", "optstr", "optbool"], "ret": "optmods"},
        "explodeFunc": {"rust": "explodeFunc", "params": ["optnum", "luaarg", "str", "tags"], "ret": "optmods"},
        "dealNoNonDamageType": {"rust": "dealNoNonDamageType", "params": ["str", "bool"], "ret": "optmods"},
    },
    "helper_structs": {
        # ModParser.lua:2055-2072 reads options.noSupports/sourceSkill/triggerChance/ignoreHexproof/onCrit
        "TriggerExtraSkillOptions": {
            "noSupports": "optbool", "sourceSkill": "optstring", "triggerChance": "tonumber",
            "ignoreHexproof": "bool", "onCrit": "bool",
        },
    },
    # table lookups ModParser does with `t[k]`: rendered as fns returning Option
    "lookups": {
        "gemIdLookup": {"rust": "gemIdLookup", "ret": "optname"},   # ModParser.lua:2033-2046
        "dmgTypes": {"rust": "dmgTypes", "ret": "optname"},         # ModParser.lua:2121-2127
    },
    # data.<table>["Key"].<field>: keyed data tables (doc §4.4 schema; field kinds)
    "data_tables": {
        "nonDamagingAilment": {"rust": "{data}.nonDamagingAilment[&{k}].{f}", "fields": {"default": "optnum"}},  # Data.lua:424-431 (Freeze has default = nil)
    },
    # Tag number fields that Lua may fill with a capture string and that EvalMod only uses in
    # arithmetic (so coercing the string like tonumber is exact): ModStore.lua EvalMod.
    "tag_num_from_string": ["div", "limit", "percent", "globalLimit", "base"],
    # data.<group>.<key> records generated by P1 (verbatim keys, f64 fields)
    "data_records": ["misc", "characterConstants", "monsterConstants", "gameConstants"],
    # ---- Tag fields (doc §4.3) and their kinds
    "tag_types": [
        "Condition", "ActorCondition", "Multiplier", "MultiplierThreshold", "PerStat", "PercentStat",
        "StatThreshold", "DistanceRamp", "MeleeProximity", "Limit", "ItemCondition", "SocketedIn", "SkillName",
        "SkillId", "SkillPart", "SkillType", "BaseFlag", "SlotName", "ModFlagOr", "KeywordFlagAnd", "MonsterTag",
        "GlobalEffect", "InSlot", "SlotNumber", "DisablesItem", "IgnoreCond",
        "Global",                                                            # CCR-0006
    ],
    "tag_fields": {
        "var": "name", "varList": "namelist", "neg": "bool", "actor": "name", "stat": "name", "statList": "namelist",
        "div": "num", "divVar": "name", "base": "num", "noFloor": "bool", "invert": "bool", "floor": "bool",
        "limit": "num", "limitVar": "name", "limitStat": "name", "limitTotal": "bool", "limitNegTotal": "bool",
        "limitActor": "name", "percent": "num", "percentVar": "name", "threshold": "num", "thresholdVar": "name",
        "thresholdStat": "name", "thresholdActor": "name", "thresholdPercent": "num", "thresholdPercentVar": "name",
        "upper": "bool", "equals": "bool", "ramp": "ramp", "itemSlot": "name", "allSlots": "bool", "bothSlots": "bool",
        "excludeSelf": "bool", "searchCond": "astr", "rarityCond": "name", "corruptedCond": "optbool",
        "shaperCond": "optbool", "elderCond": "optbool", "nameCond": "astr", "slotName": "name",
        "slotNameList": "namelist", "keyword": "name", "socketColor": "name", "sockets": "sockets",
        "skillName": "name", "skillNameList": "namelist", "includeTransfigured": "bool", "summonSkill": "bool",
        "skillId": "name", "skillPart": "num", "skillPartList": "numlist", "skillType": "skilltype",
        "skillTypeList": "skilltypelist", "baseFlag": "name", "modFlags": "u32", "keywordFlags": "u32",
        "monsterTag": "name", "monsterTagList": "namelist", "globalLimit": "num", "globalLimitKey": "name",
        "num": "num", "effectType": "name", "effectName": "name", "effectCond": "name", "effectEnemyCond": "name",
        "effectStackVar": "name", "effectStackLimit": "num", "modCond": "name", "unscalable": "bool",
        "allowTotemBuff": "bool", "applyNotPlayer": "bool", "applyMinions": "bool",
        "excludeItemType": "name",                                           # CCR-0006
    },
    # ---- ModSpec fields (doc §6, plus CCR-0006 additions) and their kinds
    "spec_fields": {
        "flags": "u32", "keywordFlags": "u32", "tag": "opttag", "tagList": "tags",
        "addToMinion": "bool", "addToMinionTag": "opttag", "addToSkill": "opttag", "addToAura": "bool",
        "applyToEnemy": "bool", "newAura": "bool", "newAuraOnlyAllies": "bool", "affectedByAura": "bool",
        "modSuffix": "optname", "modName": "optnames", "modType": "optmodtype",
        # CCR-0006
        "playerTag": "opttag", "playerTagList": "tags", "actorEnemy": "bool", "onlyAddToBanners": "bool",
    },
}

MOD_TYPES = ("BASE", "INC", "MORE", "FLAG", "OVERRIDE", "LIST", "MAX", "MIN", "Flag", "CHANCE", "DUMMY")


def _merge(base, over):
    for k, v in over.items():
        if isinstance(v, dict) and isinstance(base.get(k), dict):
            _merge(base[k], v)
        else:
            base[k] = v
    return base


def load_shape(path=None, overrides=None):
    shape = copy.deepcopy(DEFAULT_SHAPE)
    if path:
        with open(path, encoding="utf-8") as fh:
            _merge(shape, json.load(fh))
    if overrides:
        _merge(shape, copy.deepcopy(overrides))
    return shape
