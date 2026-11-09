//! The jewel func bodies (ModParser.lua 6151-6615 closures): `func(node, out, data)`; node None = the
//! "finish" call (CalcSetup.lua 259-305). Provenance per func is MP7's (the table statements are
//! marked in pob-modparser/src/jewel_func_list.rs, so the bodies here carry plain
//! `// ModParser.lua:<a>-<b>` references, not `@pob` markers that would cover the lines twice).
//!
//! Owner: MP7.
//!
//! `spec.key` (pob-modparser/src/jewel_func_list.rs documents the encoding):
//!
//! * a lowercased `jewelFuncList` key: the wrapper of 6554-6592 around the table value `v`
//!   ([`jewelFuncListSource`]); `args` = the pattern captures when the line matched a pattern key
//!   (6644-6646), empty for an exact key (6649-6651) (CCR-0014 §3);
//! * `"$getSimpleConv"` (ModParser.lua:19-38), `"$getPerStat"` (6370-6376), `"$getThreshold"`
//!   (6457-6478): the helpers' closures, upvalues in `args`;
//! * `"Modules/ModParser.lua:<a>-<b>"`: an inline closure of the jewel tables (6202-6230,
//!   6238-6242, 6250-6254, 6255-6259, 6388-6395, 6420-6435, 6436-6451), an inner closure a
//!   capture closure returned (6232-6236 … 6359-6364; upvalues in `args`), or one of the two
//!   `ExtraJewelFunc` closures of specialModList (2622-2627 `args = [dmgType]`, 2630-2634
//!   `args = [val, attr]`; CCR-0039 asks the specialModList units to emit exactly these);
//! * `"$default"`: CalcSetup.lua:877-884, the default radius func of a jewel without a funcList.
#![allow(non_snake_case, clippy::all, clippy::pedantic)]

use crate::prelude::*;
use pob_gamedata::mod_tools::createMod;
use pob_gamedata::{JewelFuncKind, JewelFuncSpec};
use pob_lua::bit::{band, bnot, bor};
use pob_lua::string::firstToUpper;
use pob_modparser::jewel_func_list::{callCaptureClosure, jewelFuncListSource};
use std::cell::RefCell;

/// `spec.func(node, out, data)`.
pub fn callJewelFunc(
    spec: &pob_gamedata::JewelFuncSpec,
    node: Option<&NodeRef>,
    out: &ModList,
    data: &Table,
) {
    let key = spec.key.as_str();
    match key {
        "$getSimpleConv" => getSimpleConv_func(&spec.args, node, out, data),
        "$getPerStat" => getPerStat_func(&spec.args, node, out, data),
        "$getThreshold" => getThreshold_func(&spec.args, node, out, data),
        "$default" => default_func(node, out, data),
        _ => {
            if let Some(range) = key.strip_prefix("Modules/ModParser.lua:") {
                closure_func(range, &spec.args, node, out, data)
            } else {
                jewelFuncList_func(spec, node, out, data)
            }
        }
    }
}

// ---- Rust-only plumbing -------------------------------------------------------------------

/// Lua arithmetic on a mod value / capture (numbers and numeric strings; else a LuaError).
fn arith(v: &Value, what: &str) -> f64 {
    match v {
        Value::Num(x) => *x,
        Value::Str(s) => match pob_lua::tonumber(s) {
            Some(x) => x,
            None => lua_error!("attempt to perform arithmetic on {} (a string value)", what),
        },
        other => lua_error!(
            "attempt to perform arithmetic on {} (a {} value)",
            what,
            other.lua_type()
        ),
    }
}

/// Lua `tonumber(v)` (nil for anything that is not a number or a numeric string).
fn tonumber_v(v: &Value) -> Option<f64> {
    match v {
        Value::Num(x) => Some(*x),
        Value::Str(s) => pob_lua::tonumber(s),
        _ => None,
    }
}

fn arg(args: &[Value], i: usize) -> Value {
    args.get(i).cloned().unwrap_or(Value::Nil)
}

fn arg_name(args: &[Value], i: usize, what: &str) -> Name {
    match args.get(i) {
        Some(Value::Str(s)) => Name::from(&**s),
        _ => lua_error!("jewel func: upvalue {} is not a string", what),
    }
}

fn arg_type(args: &[Value], i: usize) -> Option<ModType> {
    match args.get(i) {
        Some(Value::Str(s)) => ModType::parse(s),
        _ => None,
    }
}

/// `data.modSource`.
fn modSource(data: &Table) -> Option<LStr> {
    data.str(n!("modSource"))
}

/// `out:NewMod(name, type, value, source, flags, keywordFlags, …)` with a source that may be
/// nil (ModStore.lua:109-111: `self:AddMod(mod_createMod(...))`).
fn new_mod(
    out: &ModList,
    name: Name,
    ty: ModType,
    value: impl Into<Value>,
    source: &Option<LStr>,
    flags: u32,
    keywordFlags: u32,
    tags: &[Tag],
) {
    out.AddMod(Arc::new(createMod(
        name,
        ty,
        value,
        source.as_deref(),
        flags,
        keywordFlags,
        tags,
    )));
}

/// `out:MergeNewMod(name, type, value, mod.source, mod.flags, mod.keywordFlags, unpack(tags))`.
fn merge_new_mod(
    out: &ModList,
    name: Name,
    ty: ModType,
    value: Value,
    m: &Mod,
    flags: u32,
    tags: &[Tag],
) {
    out.MergeNewMod(createMod(
        name,
        ty,
        value,
        m.source.as_deref(),
        flags,
        m.keywordFlags,
        tags,
    ));
}

/// `{ mod = m }`.
fn payload(m: Mod) -> Value {
    let mut t = ValueTable::new();
    t.set(n!("mod"), Value::Mod(Arc::new(m)));
    Value::Table(Arc::new(t))
}

fn node_type(node: &NodeRef) -> Name {
    node.r#type.get()
}

/// `node.conqueredBy` is truthy.
fn conquered(node: &NodeRef) -> bool {
    node.conqueredBy.get().is_some()
}

// ---- the jewelFuncList wrappers (ModParser.lua:6552-6593) -----------------------------------

fn jewelFuncList_func(spec: &JewelFuncSpec, node: Option<&NodeRef>, out: &ModList, data: &Table) {
    let Some(&v) = jewelFuncListSource.get(spec.key.as_str()) else {
        lua_error!(
            "attempt to call field 'func' (a nil value): no jewelFuncList entry {:?}",
            spec.key.as_str()
        );
    };
    let captured = !spec.args.is_empty();
    match spec.kind {
        // ModParser.lua:6555-6570: { func = function(cap1, cap2, cap3, cap4, cap5) … end, type = "Other" }
        JewelFuncKind::Other => {
            if !captured {
                // Exact key (6649-6651): CalcSetup calls the outer function as func(node, out, data).
                // -- Need to not modify any nodes already modified by timeless jewels
                // -- Some functions return a function instead of simply adding mods, so if
                // -- we don't see a node right away, run the outer function first
                // L6559: if cap1 and type(cap1) == "table" and cap1.conqueredBy then
                if let Some(n) = node {
                    if conquered(n) {
                        // L6560
                        return;
                    }
                }
                // L6562: local innerFuncOrNil = v(cap1, cap2, cap3, cap4, cap5)
                call_value(v, node, out, data);
                // L6564: return function(node, out, other) … end — dropped by the caller
            } else {
                // Pattern key (6644-6646): L6559-6562 ran at parse time with the captures
                // (strings, so no conqueredBy); the func is the closure of 6564-6569.
                // L6562: local innerFuncOrNil = v(cap1, cap2, cap3, cap4, cap5)
                // -- In all (current) cases, there is only one nested layer, so no need for recursion
                let innerFuncOrNil = callCaptureClosure(v, &spec.args);
                // L6565: if node and type(node) == "table" and node.conqueredBy then
                if let Some(n) = node {
                    if conquered(n) {
                        // L6566
                        return;
                    }
                }
                // L6568: return innerFuncOrNil(node, out, other)
                match innerFuncOrNil {
                    Some(inner) => callJewelFunc(&inner, node, out, data),
                    None => lua_error!("attempt to call upvalue 'innerFuncOrNil' (a nil value)"),
                }
            }
        }
        // L6573 / L6576: { func = v, type = "Self" / "SelfUnalloc" }
        JewelFuncKind::Self_ | JewelFuncKind::SelfUnalloc => {
            if captured {
                // LUA-QUIRK: no Self/SelfUnalloc key has a capture; Lua would run v at parse time
                // with the captures as node/out/data.
                lua_error!(
                    "jewel func {:?}: a Self entry matched as a pattern",
                    spec.key.as_str()
                );
            }
            call_value(v, node, out, data);
        }
        JewelFuncKind::Threshold => {
            // L6580: if type(v) == "table" then
            if v.key.as_str() == "$list" {
                // L6582: func = function(node, out, data)
                // L6583
                let funcData = match data.table(n!("funcData")) {
                    Some(t) => t,
                    None => Table::new(),
                };
                data.set(n!("funcData"), funcData.clone());
                // L6584: for index, func in ipairs(v) do
                for (i, f) in v.args.iter().enumerate() {
                    let Value::JewelFunc(func) = f else { break };
                    let index = (i + 1) as i64;
                    // L6585
                    let fd = match funcData.table(index) {
                        Some(t) => t,
                        None => Table::new(),
                    };
                    funcData.set(index, fd.clone());
                    // L6586
                    fd.set(n!("modSource"), data.get(n!("modSource")));
                    // L6587
                    callJewelFunc(func, node, out, &fd);
                }
            } else if captured {
                // L6646: patternVal.func(cap1, …) = v(cap1, …), the getThreshold closure
                match callCaptureClosure(v, &spec.args) {
                    Some(inner) => callJewelFunc(&inner, node, out, data),
                    None => lua_error!(
                        "jewel func {:?}: a threshold entry without captures matched as a pattern",
                        spec.key.as_str()
                    ),
                }
            } else {
                // L6591: { func = v, type = "Threshold" }
                call_value(v, node, out, data);
            }
        }
    }
}

/// Lua `v(node, out, data)` for a table value `v`: a node func runs; a capture closure only
/// builds and returns its inner closure (the caller drops it), evaluating what its body
/// evaluates eagerly (`num / 100` with `num` = `node` raises, as in Lua; LUA-QUIRK: no real line
/// equals a pattern key, so this path is never taken).
fn call_value(v: &JewelFuncSpec, node: Option<&NodeRef>, out: &ModList, data: &Table) {
    let node_v = match node {
        Some(_) => Value::Table(Arc::new(ValueTable::new())),
        None => Value::Nil,
    };
    let as_caps = [
        node_v,
        Value::Table(Arc::new(ValueTable::new())),
        Value::Table(Arc::new(ValueTable::new())),
    ];
    if callCaptureClosure(v, &as_caps).is_some() {
        return;
    }
    callJewelFunc(v, node, out, data);
}

// ---- helper closures -------------------------------------------------------------------------

/// ModParser.lua:19-38 (`getSimpleConv`'s closure), upvalues
/// `[srcList, dst, type, remove, factor, srcType]`.
fn getSimpleConv_func(args: &[Value], node: Option<&NodeRef>, out: &ModList, data: &Table) {
    let srcList: Vec<Name> = match args.first() {
        Some(Value::Table(t)) => t
            .array
            .iter()
            .map(|v| match v {
                Value::Str(s) => Name::from(&**s),
                _ => lua_error!("getSimpleConv: srcList entry is not a string"),
            })
            .collect(),
        _ => lua_error!("getSimpleConv: srcList is not a table"),
    };
    let dst = arg_name(args, 1, "dst");
    let Some(ty) = arg_type(args, 2) else {
        lua_error!("getSimpleConv: type is not a mod type")
    };
    let remove = arg(args, 3).truthy();
    let factor = tonumber_v(&arg(args, 4));
    let srcType = arg_type(args, 5);
    getSimpleConv_body(&srcList, dst, ty, remove, factor, srcType, node, out, data);
}

/// The body of ModParser.lua:19-38.
#[allow(clippy::too_many_arguments)]
fn getSimpleConv_body(
    srcList: &[Name],
    dst: Name,
    r#type: ModType,
    remove: bool,
    factor: Option<f64>,
    srcType: Option<ModType>,
    node: Option<&NodeRef>,
    out: &ModList,
    _data: &Table,
) {
    // L20: local attributes = {["Dex"] = true, ["Int"] = true, ["Str"] = true}
    let attributes = |s: Name| s == n!("Dex") || s == n!("Int") || s == n!("Str");
    // L21
    if let Some(node) = node {
        // L22: for _, src in pairs(srcList) do (an array: 1..n)
        for &src in srcList {
            // L23
            for m in node.modList.get().ipairs() {
                // -- do not convert stats from tattoos
                // L25
                if m.name == src
                    && ((srcType.is_some() && Some(m.r#type) == srcType) || m.r#type == r#type)
                    && !(node.isTattoo.get() && attributes(src))
                {
                    // L26
                    if remove {
                        // L27
                        let v = -arith(&m.value, "field 'value'");
                        merge_new_mod(out, src, r#type, Value::Num(v), &m, m.flags, &m.tags);
                    }
                    // L29
                    if let Some(factor) = factor {
                        // L30
                        let v = m_floor(arith(&m.value, "field 'value'") * factor);
                        merge_new_mod(out, dst, r#type, Value::Num(v), &m, m.flags, &m.tags);
                    } else {
                        // L32
                        merge_new_mod(out, dst, r#type, m.value.clone(), &m, m.flags, &m.tags);
                    }
                }
            }
        }
    }
}

/// ModParser.lua:6370-6376 (`getPerStat`'s closure), upvalues `[dst, modType, flags, stat, factor]`.
fn getPerStat_func(args: &[Value], node: Option<&NodeRef>, out: &ModList, data: &Table) {
    let dst = arg_name(args, 0, "dst");
    let Some(modType) = arg_type(args, 1) else {
        lua_error!("getPerStat: modType is not a mod type")
    };
    let flags = tonumber_v(&arg(args, 2)).unwrap_or(0.0) as u32;
    let stat = arg_name(args, 3, "stat");
    let factor = arith(&arg(args, 4), "upvalue 'factor'");
    // L6371
    if node.is_some() {
        // L6372
        let v = data.num_or(stat, 0.0) + out.Sum(ModType::BASE, None, &[stat]);
        data.set(stat, v);
    // L6373: elseif data[stat] ~= 0 then
    } else if !matches!(data.get(stat), Val::Num(x) if x == 0.0) {
        // L6374
        let v = m_floor(data.num_or(stat, 0.0) * factor);
        new_mod(out, dst, modType, v, &modSource(data), flags, 0, &[]);
    }
}

thread_local! {
    /// LUA-ALIAS: getThreshold's `value` upvalue is the table `baseMod.value` points at, and
    /// 6473-6474 write `value.mod.source = data.modSource` after `copyTable(baseMod)`, so each
    /// finish call's copy carries the source the previous call wrote (nil the first time). Keyed
    /// by the closure's baseMod (one per getThreshold call, as in Lua); per thread, as per Lua
    /// state.
    static THRESHOLD_VALUE_SOURCE: RefCell<ahash::AHashMap<usize, Option<AStr>>> =
        RefCell::new(ahash::AHashMap::new());
}

/// ModParser.lua:6457-6478 (`getThreshold`'s closure), upvalues `[attrib, baseMod]`.
fn getThreshold_func(args: &[Value], node: Option<&NodeRef>, out: &ModList, data: &Table) {
    let attrib = arg(args, 0);
    let Some(Value::Mod(baseMod)) = args.get(1) else {
        lua_error!("getThreshold: baseMod is not a mod")
    };
    let value = &baseMod.value;
    // L6458
    if node.is_some() {
        // L6459
        if let Value::Table(t) = &attrib {
            // L6460
            for att in t.array.iter() {
                let Value::Str(att) = att else {
                    lua_error!("getThreshold: attrib entry is not a string")
                };
                let att = Name::from(&**att);
                // L6461
                let nodeVal = out.Sum(ModType::BASE, None, &[att]);
                // L6462
                data.set(att, data.num_or(att, 0.0) + nodeVal);
                // L6463
                data.set(n!("total"), data.num_or(n!("total"), 0.0) + nodeVal);
            }
        } else {
            let attrib = match &attrib {
                Value::Str(s) => Name::from(&**s),
                _ => lua_error!("getThreshold: attrib is not a string"),
            };
            // L6466
            let nodeVal = out.Sum(ModType::BASE, None, &[attrib]);
            // L6467
            data.set(attrib, data.num_or(attrib, 0.0) + nodeVal);
            // L6468
            data.set(n!("total"), data.num_or(n!("total"), 0.0) + nodeVal);
        }
    // L6470
    } else if data.num_or(n!("total"), 0.0) >= 40.0 {
        let src = modSource(data);
        let id = Arc::as_ptr(baseMod) as usize;
        // L6471: local mod = copyTable(baseMod)
        let mut m: Mod = (**baseMod).clone();
        let inner_mod = match value {
            Value::Table(t) => t.get(n!("mod")).as_mod().cloned(),
            _ => None,
        };
        if let (Value::Table(t), Some(im)) = (value, &inner_mod) {
            // the copy sees the source the previous call wrote into the shared `value.mod`
            let prev = THRESHOLD_VALUE_SOURCE.with(|s| s.borrow().get(&id).cloned());
            let mut im2: Mod = (**im).clone();
            if let Some(prev) = prev {
                im2.source = prev;
            }
            let mut t2: ValueTable = (**t).clone();
            t2.set(n!("mod"), Value::Mod(Arc::new(im2)));
            m.value = Value::Table(Arc::new(t2));
        }
        // L6472
        m.source = src.as_deref().map(AStr::from);
        // L6473: if type(value) == "table" and value.mod then
        if inner_mod.is_some() {
            // L6474
            let s = src.as_deref().map(AStr::from);
            THRESHOLD_VALUE_SOURCE.with(|st| {
                st.borrow_mut().insert(id, s);
            });
        }
        // L6476
        out.AddMod(Arc::new(m));
    }
}

/// CalcSetup.lua:877-884: the default radius func (a jewel without `jewelData.funcList`).
fn default_func(node: Option<&NodeRef>, out: &ModList, data: &Table) {
    // -- Default function just tallies all stats in radius
    // L879
    if node.is_some() {
        // L880: for _, stat in pairs({"Str","Dex","Int"}) do (an array: 1..3)
        for stat in [n!("Str"), n!("Dex"), n!("Int")] {
            // L881
            let v = data.num_or(stat, 0.0) + out.Sum(ModType::BASE, None, &[stat]);
            data.set(stat, v);
        }
    }
}

// ---- inline closures ------------------------------------------------------------------------

/// `node and node.type ~= "Keystone" and node.type ~= "Socket" and node.type ~= "ClassStart"`.
fn grants_node(node: Option<&NodeRef>) -> bool {
    match node {
        Some(n) => {
            let t = node_type(n);
            t != n!("Keystone") && t != n!("Socket") && t != n!("ClassStart")
        }
        None => false,
    }
}

fn closure_func(range: &str, args: &[Value], node: Option<&NodeRef>, out: &ModList, data: &Table) {
    match range {
        // ModParser.lua:6202-6230 ("Melee and Melee Weapon Type modifiers in Radius are Transformed to Bow Modifiers")
        "6202-6230" => {
            // L6203
            if let Some(node) = node {
                // L6204
                let mask1 = bor(
                    bor(
                        bor(
                            bor(
                                bor(bor(ModFlag::Axe, ModFlag::Claw), ModFlag::Dagger),
                                ModFlag::Mace,
                            ),
                            ModFlag::Staff,
                        ),
                        ModFlag::Sword,
                    ),
                    ModFlag::Melee,
                );
                // L6205
                let mask2 = bor(ModFlag::Weapon1H, ModFlag::WeaponMelee);
                // L6206
                let mask3 = bor(ModFlag::Weapon2H, ModFlag::WeaponMelee);
                // L6207
                for m in node.modList.get().ipairs() {
                    // L6208
                    if band(m.flags, mask1) != 0
                        || band(m.flags, mask2) == mask2
                        || band(m.flags, mask3) == mask3
                    {
                        // L6209
                        let neg = -arith(&m.value, "field 'value'");
                        merge_new_mod(out, m.name, m.r#type, Value::Num(neg), &m, m.flags, &m.tags);
                        // L6210
                        let flags = bor(
                            band(m.flags, bnot(bor(bor(mask1, mask2), mask3))),
                            ModFlag::Bow,
                        );
                        merge_new_mod(out, m.name, m.r#type, m.value.clone(), &m, flags, &m.tags);
                    // L6211: elseif mod[1] then
                    } else if !m.tags.is_empty() {
                        // L6212
                        let using = |v: Option<Name>| {
                            matches!(v, Some(v) if v == n!("UsingAxe") || v == n!("UsingClaw")
                                || v == n!("UsingDagger") || v == n!("UsingMace")
                                || v == n!("UsingStaff") || v == n!("UsingSword")
                                || v == n!("UsingMeleeWeapon"))
                        };
                        // L6213
                        for tag in m.tags.iter() {
                            // L6214
                            if tag.r#type == TagType::Condition && using(tag.var) {
                                // L6215: local newTagList = copyTable(mod)
                                let mut newTagList = m.tags.clone();
                                // L6216
                                for tag in newTagList.iter_mut() {
                                    // L6217
                                    if tag.r#type == TagType::Condition && using(tag.var) {
                                        // L6218
                                        tag.var = Some(n!("UsingBow"));
                                        // L6219
                                        break;
                                    }
                                }
                                // L6222
                                let neg = -arith(&m.value, "field 'value'");
                                merge_new_mod(
                                    out,
                                    m.name,
                                    m.r#type,
                                    Value::Num(neg),
                                    &m,
                                    m.flags,
                                    &m.tags,
                                );
                                // L6223
                                merge_new_mod(
                                    out,
                                    m.name,
                                    m.r#type,
                                    m.value.clone(),
                                    &m,
                                    m.flags,
                                    &newTagList,
                                );
                                // L6224
                                break;
                            }
                        }
                    }
                }
            }
        }
        // ModParser.lua:6232-6236 (inner closure of 6231, upvalue num)
        "6232-6236" => {
            // L6233
            if let Some(n) = node {
                let t = node_type(n);
                if t != n!("Keystone") && t != n!("ClassStart") {
                    // L6234
                    new_mod(
                        out,
                        n!("PassiveSkillEffect"),
                        ModType::INC,
                        tonumber_v(&arg(args, 0)),
                        &modSource(data),
                        0,
                        0,
                        &[],
                    );
                }
            }
        }
        // ModParser.lua:6238-6242 ("Notable Passive Skills in Radius grant nothing")
        "6238-6242" => {
            // L6239
            if let Some(n) = node {
                if node_type(n) == n!("Notable") {
                    // L6240
                    new_mod(
                        out,
                        n!("PassiveSkillHasNoEffect"),
                        ModType::FLAG,
                        true,
                        &modSource(data),
                        0,
                        0,
                        &[],
                    );
                }
            }
        }
        // ModParser.lua:6244-6248 (inner closure of 6243, upvalue num)
        "6244-6248" => {
            // L6245
            if let Some(n) = node {
                if n.isTattoo.get() {
                    // L6246
                    new_mod(
                        out,
                        n!("PassiveSkillEffect"),
                        ModType::INC,
                        tonumber_v(&arg(args, 0)),
                        &modSource(data),
                        0,
                        0,
                        &[],
                    );
                }
            }
        }
        // ModParser.lua:6250-6254 ("Allocated Small Passive Skills in Radius grant nothing")
        "6250-6254" => {
            // L6251
            if let Some(n) = node {
                if node_type(n) == n!("Normal") {
                    // L6252
                    new_mod(
                        out,
                        n!("AllocatedPassiveSkillHasNoEffect"),
                        ModType::FLAG,
                        true,
                        &modSource(data),
                        0,
                        0,
                        &[],
                    );
                }
            }
        }
        // ModParser.lua:6255-6259 ("Allocated Notable Passive Skills in Radius grant nothing")
        "6255-6259" => {
            // L6256
            if let Some(n) = node {
                if node_type(n) == n!("Notable") {
                    // L6257
                    new_mod(
                        out,
                        n!("AllocatedPassiveSkillHasNoEffect"),
                        ModType::FLAG,
                        true,
                        &modSource(data),
                        0,
                        0,
                        &[],
                    );
                }
            }
        }
        // ModParser.lua:6261-6266 (inner closure of 6260, upvalues min, max)
        "6261-6266" => {
            // L6262
            if let Some(n) = node {
                let t = node_type(n);
                if t != n!("Keystone") && t != n!("ClassStart") {
                    let src = modSource(data);
                    let kw = bor(KeywordFlag::Trap, KeywordFlag::Mine);
                    // L6263
                    new_mod(
                        out,
                        n!("PhysicalMin"),
                        ModType::BASE,
                        tonumber_v(&arg(args, 0)),
                        &src,
                        0,
                        kw,
                        &[],
                    );
                    // L6264
                    new_mod(
                        out,
                        n!("PhysicalMax"),
                        ModType::BASE,
                        tonumber_v(&arg(args, 1)),
                        &src,
                        0,
                        kw,
                        &[],
                    );
                }
            }
        }
        // ModParser.lua:6269-6273 (inner closure of 6268, upvalue num)
        "6269-6273" => {
            // L6270
            if let Some(n) = node {
                let t = node_type(n);
                if t != n!("Keystone") && t != n!("ClassStart") {
                    // L6271
                    let flags = bor(bor(ModFlag::Unarmed, ModFlag::Attack), ModFlag::Melee);
                    new_mod(
                        out,
                        n!("Speed"),
                        ModType::INC,
                        tonumber_v(&arg(args, 0)),
                        &modSource(data),
                        flags,
                        0,
                        &[],
                    );
                }
            }
        }
        // ModParser.lua:6276-6280 (inner closure of 6275, upvalue num)
        "6276-6280" => {
            // L6277
            if grants_node(node) {
                // L6278
                new_mod(
                    out,
                    n!("CritChance"),
                    ModType::INC,
                    tonumber_v(&arg(args, 0)),
                    &modSource(data),
                    0,
                    0,
                    &[],
                );
            }
        }
        // ModParser.lua:6283-6287
        "6283-6287" => {
            // L6284
            if grants_node(node) {
                // L6285
                new_mod(
                    out,
                    n!("Life"),
                    ModType::BASE,
                    tonumber_v(&arg(args, 0)),
                    &modSource(data),
                    0,
                    0,
                    &[],
                );
            }
        }
        // ModParser.lua:6290-6294
        "6290-6294" => {
            // L6291
            if grants_node(node) {
                // L6292
                new_mod(
                    out,
                    n!("Mana"),
                    ModType::BASE,
                    tonumber_v(&arg(args, 0)),
                    &modSource(data),
                    0,
                    0,
                    &[],
                );
            }
        }
        // ModParser.lua:6297-6301
        "6297-6301" => {
            // L6298
            if grants_node(node) {
                // L6299
                new_mod(
                    out,
                    n!("EnergyShield"),
                    ModType::INC,
                    tonumber_v(&arg(args, 0)),
                    &modSource(data),
                    0,
                    0,
                    &[],
                );
            }
        }
        // ModParser.lua:6304-6308
        "6304-6308" => {
            // L6305
            if grants_node(node) {
                // L6306
                new_mod(
                    out,
                    n!("Armour"),
                    ModType::INC,
                    tonumber_v(&arg(args, 0)),
                    &modSource(data),
                    0,
                    0,
                    &[],
                );
            }
        }
        // ModParser.lua:6311-6315
        "6311-6315" => {
            // L6312
            if grants_node(node) {
                // L6313
                new_mod(
                    out,
                    n!("Evasion"),
                    ModType::INC,
                    tonumber_v(&arg(args, 0)),
                    &modSource(data),
                    0,
                    0,
                    &[],
                );
            }
        }
        // ModParser.lua:6318-6325
        "6318-6325" => {
            // L6319
            if grants_node(node) {
                let v = tonumber_v(&arg(args, 0));
                let src = modSource(data);
                // L6320
                new_mod(out, n!("Str"), ModType::BASE, v, &src, 0, 0, &[]);
                // L6321
                new_mod(out, n!("Dex"), ModType::BASE, v, &src, 0, 0, &[]);
                // L6322
                new_mod(out, n!("Int"), ModType::BASE, v, &src, 0, 0, &[]);
                // L6323
                new_mod(out, n!("All"), ModType::BASE, v, &src, 0, 0, &[]);
            }
        }
        // ModParser.lua:6328-6332
        "6328-6332" => {
            // L6329
            if grants_node(node) {
                // L6330
                new_mod(
                    out,
                    n!("ChaosResist"),
                    ModType::BASE,
                    tonumber_v(&arg(args, 0)),
                    &modSource(data),
                    0,
                    0,
                    &[],
                );
            }
        }
        // ModParser.lua:6335-6339 (upvalues num, type)
        "6335-6339" => {
            // L6336
            if grants_node(node) {
                // L6337
                let Value::Str(ty) = arg(args, 1) else {
                    lua_error!("bad argument #1 to 'firstToUpper' (string expected)")
                };
                let name = cat!(firstToUpper(&ty), "Damage");
                new_mod(
                    out,
                    name,
                    ModType::INC,
                    tonumber_v(&arg(args, 0)),
                    &modSource(data),
                    0,
                    0,
                    &[],
                );
            }
        }
        // ModParser.lua:6342-6348 (upvalues num1, num2)
        "6342-6348" => {
            // L6343
            if let Some(n) = node {
                if node_type(n) == n!("Notable") {
                    let src = modSource(data);
                    // L6344
                    new_mod(
                        out,
                        n!("PassiveSkillHasOtherEffect"),
                        ModType::FLAG,
                        true,
                        &src,
                        0,
                        0,
                        &[],
                    );
                    // L6345
                    let inner1 = createMod(
                        n!("ManaCost"),
                        ModType::INC,
                        tonumber_v(&arg(args, 0)),
                        src.as_deref(),
                        0,
                        0,
                        &[],
                    );
                    new_mod(
                        out,
                        n!("NodeModifier"),
                        ModType::LIST,
                        payload(inner1),
                        &src,
                        0,
                        0,
                        &[],
                    );
                    // L6346
                    let inner2 = createMod(
                        n!("Damage"),
                        ModType::INC,
                        tonumber_v(&arg(args, 1)),
                        src.as_deref(),
                        ModFlag::Spell,
                        0,
                        &[],
                    );
                    new_mod(
                        out,
                        n!("NodeModifier"),
                        ModType::LIST,
                        payload(inner2),
                        &src,
                        0,
                        0,
                        &[],
                    );
                }
            }
        }
        // ModParser.lua:6351-6356
        "6351-6356" => {
            // L6352
            if let Some(n) = node {
                if node_type(n) == n!("Notable") {
                    let src = modSource(data);
                    // L6353
                    new_mod(
                        out,
                        n!("PassiveSkillHasOtherEffect"),
                        ModType::FLAG,
                        true,
                        &src,
                        0,
                        0,
                        &[],
                    );
                    // L6354
                    let dt = createMod(
                        n!("DamageTaken"),
                        ModType::INC,
                        tonumber_v(&arg(args, 0)),
                        src.as_deref(),
                        0,
                        0,
                        &[],
                    );
                    let mm = createMod(
                        n!("MinionModifier"),
                        ModType::LIST,
                        payload(dt),
                        None,
                        0,
                        0,
                        &[],
                    );
                    new_mod(
                        out,
                        n!("NodeModifier"),
                        ModType::LIST,
                        payload(mm),
                        &src,
                        0,
                        0,
                        &[],
                    );
                }
            }
        }
        // ModParser.lua:6359-6364
        "6359-6364" => {
            // L6360
            if let Some(n) = node {
                if node_type(n) == n!("Notable") {
                    let src = modSource(data);
                    // L6361
                    new_mod(
                        out,
                        n!("PassiveSkillHasOtherEffect"),
                        ModType::FLAG,
                        true,
                        &src,
                        0,
                        0,
                        &[],
                    );
                    // L6362
                    let Some(x) = tonumber_v(&arg(args, 0)) else {
                        lua_error!("attempt to perform arithmetic on a nil value")
                    };
                    let ms = createMod(
                        n!("MovementSpeed"),
                        ModType::INC,
                        -x,
                        src.as_deref(),
                        0,
                        0,
                        &[],
                    );
                    let mm = createMod(
                        n!("MinionModifier"),
                        ModType::LIST,
                        payload(ms),
                        None,
                        0,
                        0,
                        &[],
                    );
                    new_mod(
                        out,
                        n!("NodeModifier"),
                        ModType::LIST,
                        payload(mm),
                        &src,
                        0,
                        0,
                        &[],
                    );
                }
            }
        }
        // ModParser.lua:6388-6395 ("Dexterity and Intelligence from passives in Radius count towards
        // Strength Melee Damage bonus")
        "6388-6395" => {
            // L6389
            if let Some(node) = node {
                // L6390
                let dex = data.num_or(n!("Dex"), 0.0)
                    + node.modList.get().Sum(ModType::BASE, None, &[n!("Dex")]);
                data.set(n!("Dex"), dex);
                // L6391
                let int = data.num_or(n!("Int"), 0.0)
                    + node.modList.get().Sum(ModType::BASE, None, &[n!("Int")]);
                data.set(n!("Int"), int);
            // L6392
            } else if data.truthy(n!("Dex")) || data.truthy(n!("Int")) {
                // L6393
                let v = data.num_or(n!("Dex"), 0.0) + data.num_or(n!("Int"), 0.0);
                new_mod(
                    out,
                    n!("DexIntToMeleeBonus"),
                    ModType::BASE,
                    v,
                    &modSource(data),
                    0,
                    0,
                    &[],
                );
            }
        }
        // ModParser.lua:6420-6435 ("Grants all bonuses of Unallocated Small Passive Skills in Radius")
        "6420-6435" => unalloc_grants(n!("Normal"), node, out, data),
        // ModParser.lua:6436-6451 ("Grants all bonuses of Unallocated Notable Passive Skills in Radius")
        "6436-6451" => unalloc_grants(n!("Notable"), node, out, data),
        // ModParser.lua:2622-2627 (specialModList "non-unique jewels cause increases and reductions to
        // other damage types in a (%a+) radius …"; upvalue dmgType)
        "2622-2627" => {
            // -- Ignore Timeless Jewels
            // L2624
            if let Some(n) = node {
                if !conquered(n) {
                    let Value::Str(dmgType) = arg(args, 0) else {
                        lua_error!("attempt to index upvalue 'dmgType' (a nil value)")
                    };
                    // L2625
                    let dst = cat!(firstToUpper(&dmgType), "Damage");
                    getSimpleConv_body(
                        &[
                            n!("PhysicalDamage"),
                            n!("FireDamage"),
                            n!("ColdDamage"),
                            n!("LightningDamage"),
                            n!("ChaosDamage"),
                            n!("ElementalDamage"),
                        ],
                        dst,
                        ModType::INC,
                        true,
                        None,
                        None,
                        node,
                        out,
                        data,
                    );
                }
            }
        }
        // ModParser.lua:2630-2634 (specialModList "non-unique jewels cause small and notable passive
        // skills in a (%a+) radius to also grant %+(%d+) to (%a+)"; upvalues val, attr)
        "2630-2634" => {
            // L2631
            if let Some(n) = node {
                let t = node_type(n);
                if !conquered(n) && (t == n!("Notable") || t == n!("Normal")) {
                    let Value::Str(attr) = arg(args, 1) else {
                        lua_error!("bad argument #1 to 'firstToUpper' (string expected)")
                    };
                    // L2632
                    let up = firstToUpper(&attr);
                    let Some(c) = pob_lua::lmatch(&up, "^%a%l%l", None) else {
                        // LUA-QUIRK: Lua adds a mod with a nil name (attr shorter than 3 letters);
                        // a Name cannot be nil. No item line has such an attribute.
                        lua_error!(
                            "jewel func 2630-2634: attribute {:?} has no 3-letter prefix",
                            &*up
                        )
                    };
                    let name = Name::from(&*c[0].to_str());
                    new_mod(
                        out,
                        name,
                        ModType::BASE,
                        tonumber_v(&arg(args, 0)),
                        &modSource(data),
                        0,
                        0,
                        &[],
                    );
                }
            }
        }
        _ => lua_error!(
            "attempt to call a nil value: no jewel func closure Modules/ModParser.lua:{}",
            range
        ),
    }
}

/// ModParser.lua:6420-6435 / 6436-6451 (the two bodies differ only in the node type).
fn unalloc_grants(nodeType: Name, node: Option<&NodeRef>, out: &ModList, data: &Table) {
    // L6421
    if let Some(node) = node {
        // L6422
        if node_type(node) == nodeType {
            // L6423: data.modList = data.modList or new("ModList"):ModList()
            let modList: ModList = match data.obj::<ModStore>(n!("modList")) {
                Some(l) => l,
                None => ModStore::ModList(None),
            };
            data.set(n!("modList"), Val::Obj(modList.clone()));
            // -- Filter out "Condition:ConnectedTo" mods as these nodes are not technically allocated by this jewel func
            // L6426
            for m in out.ipairs() {
                // L6427
                if !m.name.as_str().starts_with("Condition:ConnectedTo") {
                    // L6428
                    modList.AddMod(m);
                }
            }
        }
    // L6432
    } else if let Some(modList) = data.obj::<ModStore>(n!("modList")) {
        // L6433
        out.AddList(&modList.ipairs());
    }
}
