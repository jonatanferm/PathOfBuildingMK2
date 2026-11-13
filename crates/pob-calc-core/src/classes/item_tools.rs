//! @pob-file Modules/ItemTools.lua @16de4b82d
//! `itemLib` (Modules/ItemTools.lua 1-413).
//!
//! Owner: IT-C. `applyRange`/`isModLineScalable` take `range` as a Lua value (CCR-0032 §4).
//!
//! Globals `itemLib` reads that the frozen contract has no home for (CCR-0061):
//! * `main.defaultItemAffixQuality` is IT-A's thread-local
//!   ([`crate::classes::item_parse::main_defaultItemAffixQuality`], CCR-0025 §1);
//! * `main.notSupportedModTooltips` (Main.lua:119 `true`; the settings file and the options
//!   checkbox change it, Main.lua:651/1166) is the thread-local [`main_notSupportedModTooltips`];
//!   `main.notSupportedTooltipText` (Main.lua:120) is the constant [`main_notSupportedTooltipText`];
//! * `launch.devModeAlt` (Launch.lua:122, dev mode and ALT held) is the thread-local
//!   [`launch_devModeAlt`] (default false; the UI sets it).
//!
//! `modLib.parseMod` (pob-modparser) is called through [`parseMod`], which with the `probe`
//! feature consults [`replay`] (the IT-C oracle test replays PoB's parser results while the
//! ModParser units are unported; the same seam as `item_parse::replay`, CCR-0025 §3).
#![allow(non_snake_case, non_upper_case_globals, clippy::all, clippy::pedantic)]

use std::cell::Cell;

use pob_lua::pattern::{self, Captures};

use crate::prelude::*;

// ---- replay seam (feature `probe`) ----------------------------------------------------------

/// Oracle-test seam (feature `probe`): `crates/pob-oracle/tests/IT-C.rs` installs a
/// `modLib.parseMod` hook that returns PoB's recorded results (keyed by the arguments) while the
/// ModParser lists (MP3-MP7) are unported. Used by `applyRange` (ItemTools.lua:323) and by
/// `getRangedModList` (Item.lua:2527). Unset, the real parser runs.
#[cfg(feature = "probe")]
pub mod replay {
    use super::*;
    use std::cell::RefCell;

    pub type ParseModFn = Rc<dyn Fn(&str, bool) -> (Option<Vec<Mod>>, Option<String>)>;

    #[derive(Default, Clone)]
    pub struct Hooks {
        pub parseMod: Option<ParseModFn>,
    }

    thread_local! {
        static HOOKS: RefCell<Hooks> = RefCell::new(Hooks::default());
    }

    /// Install hooks for this thread.
    pub fn set(h: Hooks) {
        HOOKS.with(|x| *x.borrow_mut() = h);
    }

    /// Remove every hook.
    pub fn clear() {
        set(Hooks::default());
    }

    pub(crate) fn get() -> Hooks {
        HOOKS.with(|x| x.borrow().clone())
    }
}

/// `modLib.parseMod(line, isComb)` (pob-modparser), through the [`replay`] seam.
pub(crate) fn parseMod(line: &str, isComb: bool) -> (Option<Vec<Mod>>, Option<String>) {
    #[cfg(feature = "probe")]
    if let Some(f) = replay::get().parseMod {
        return f(line, isComb);
    }
    pob_modparser::parseMod(line, isComb)
}

// ---- globals (CCR-0061) ----------------------------------------------------------------------

/// `main.notSupportedTooltipText` (Main.lua:120; never reassigned).
pub const main_notSupportedTooltipText: &str = " ^8(Not supported in PoB yet)";

thread_local! {
    /// `main.notSupportedModTooltips` (Main.lua:119 `self.notSupportedModTooltips = true`).
    static MAIN_NOT_SUPPORTED_MOD_TOOLTIPS: Cell<bool> = const { Cell::new(true) };
    /// `launch.devModeAlt` (Launch.lua:122: `self.devMode and IsKeyDown("ALT")`).
    static LAUNCH_DEV_MODE_ALT: Cell<bool> = const { Cell::new(false) };
}

/// `main.notSupportedModTooltips` (CCR-0061).
pub fn main_notSupportedModTooltips() -> bool {
    MAIN_NOT_SUPPORTED_MOD_TOOLTIPS.with(Cell::get)
}

/// Set `main.notSupportedModTooltips` (the UI's options checkbox / settings file; CCR-0061).
pub fn set_main_notSupportedModTooltips(v: bool) {
    MAIN_NOT_SUPPORTED_MOD_TOOLTIPS.with(|c| c.set(v));
}

/// `launch.devModeAlt` (CCR-0061).
pub fn launch_devModeAlt() -> bool {
    LAUNCH_DEV_MODE_ALT.with(Cell::get)
}

/// Set `launch.devModeAlt` (the UI's dev-mode ALT state; CCR-0061).
pub fn set_launch_devModeAlt(v: bool) {
    LAUNCH_DEV_MODE_ALT.with(|c| c.set(v));
}

// ---- plumbing (Rust-only helpers; snake_case) ------------------------------------------------

/// `colorCodes[name]` (Data/Global.lua).
pub fn color_code(name: &str) -> &'static str {
    pob_gamedata::colorCodes
        .iter()
        .find(|(k, _)| *k == name)
        .map(|(_, v)| *v)
        .unwrap_or_else(|| lua_error!("attempt to concatenate field '{name}' (a nil value)"))
}

/// The bytes of a capture as text (captures of byte classes may split a UTF-8 character; the
/// pieces are rejoined before the result is read as a string).
fn cap_bytes<'a>(c: &Captures<'a>, i: usize) -> &'a [u8] {
    c.get(i).map_or(b"" as &[u8], |x| x.as_bytes())
}

/// `tonumber(capture)` of a capture that a numeric pattern matched.
fn cap_num(c: &Captures<'_>, i: usize) -> f64 {
    c.get(i)
        .and_then(|x| x.num())
        .unwrap_or_else(|| lua_error!("attempt to perform arithmetic on a nil value"))
}

/// Bytes back to a string (lossless when the pieces rejoin whole characters).
fn into_string(b: Vec<u8>) -> String {
    match String::from_utf8(b) {
        Ok(s) => s,
        Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
    }
}

/// `s:gsub(p, function(...) ... end, n)` building the replacement as bytes.
fn gsub_fn_bytes(
    s: &str,
    p: &str,
    mut f: impl FnMut(&Captures<'_>) -> Vec<u8>,
    n: Option<i64>,
) -> String {
    let mut g = |c: &Captures<'_>| Some(f(c));
    let (b, _) =
        pattern::gsub_bytes_with(s.as_bytes(), p.as_bytes(), pattern::Repl::Func(&mut g), n);
    into_string(b)
}

/// `s:gsub(p, repl, n)` with a replacement string (`%` escapes expand, as in Lua).
fn gsub_str(s: &str, p: &str, repl: &str, n: Option<i64>) -> String {
    pattern::gsub(s, p, repl, n).0
}

/// `s:match(p)` is truthy.
fn matches(s: &str, p: &str) -> bool {
    pattern::match_bytes(s.as_bytes(), p.as_bytes(), None).is_some()
}

/// A Lua arithmetic operand (numbers, numeric strings); anything else raises with the
/// operand's description, as LuaJIT does.
fn arith(v: &Val, what: &str) -> f64 {
    match v {
        Val::Num(n) => *n,
        Val::Str(s) => pob_lua::tonumber(s).unwrap_or_else(|| {
            lua_error!("attempt to perform arithmetic on {what} (a string value)")
        }),
        other => lua_error!(
            "attempt to perform arithmetic on {what} (a {} value)",
            other.lua_type()
        ),
    }
}

/// `t[i]` of a Lua list value (a calc table or a data list).
fn index_val(t: &Val, i: i64) -> Val {
    match t {
        Val::Table(t) => t.get(Key::I(i)),
        Val::Data(Value::Table(t)) => {
            if i >= 1 {
                Val::from_value(t.index(i as usize))
            } else {
                Val::Nil
            }
        }
        other => lua_error!("attempt to index a {} value", other.lua_type()),
    }
}

/// `type(v) == "table"`.
fn is_table(v: &Val) -> bool {
    matches!(v, Val::Table(_) | Val::Data(Value::Table(_)) | Val::Mod(_))
}

// ---- module level ----------------------------------------------------------------------------

// @pob Modules/ItemTools.lua:6-34 h=734fcb5c
// (module-level statements: t_insert,t_remove,m_min,m_max,m_floor,itemLib,itemLib.influenceInfo)

/// One entry of `itemLib.influenceInfo[...]`: `{ key, display, color }`. `color` names the
/// `colorCodes` entry ([`color_code`]).
pub struct InfluenceInfo {
    pub key: &'static str,
    pub display: &'static str,
    pub color: &'static str,
}

impl InfluenceInfo {
    /// `info.color` (`colorCodes[...]`).
    pub fn color(&self) -> &'static str {
        color_code(self.color)
    }
}

// L15-34: Info table for all types of item influence
/// `itemLib.influenceInfo.all`.
pub const influenceInfo_all: [InfluenceInfo; 8] = [
    InfluenceInfo {
        key: "shaper",
        display: "Shaper",
        color: "SHAPER",
    },
    InfluenceInfo {
        key: "elder",
        display: "Elder",
        color: "ELDER",
    },
    InfluenceInfo {
        key: "adjudicator",
        display: "Warlord",
        color: "ADJUDICATOR",
    },
    InfluenceInfo {
        key: "basilisk",
        display: "Hunter",
        color: "BASILISK",
    },
    InfluenceInfo {
        key: "crusader",
        display: "Crusader",
        color: "CRUSADER",
    },
    InfluenceInfo {
        key: "eyrie",
        display: "Redeemer",
        color: "EYRIE",
    },
    InfluenceInfo {
        key: "cleansing",
        display: "Searing Exarch",
        color: "CLEANSING",
    },
    InfluenceInfo {
        key: "tangle",
        display: "Eater of Worlds",
        color: "TANGLE",
    },
];

/// `itemLib.influenceInfo.default`.
pub const influenceInfo_default: [InfluenceInfo; 6] = [
    InfluenceInfo {
        key: "shaper",
        display: "Shaper",
        color: "SHAPER",
    },
    InfluenceInfo {
        key: "elder",
        display: "Elder",
        color: "ELDER",
    },
    InfluenceInfo {
        key: "adjudicator",
        display: "Warlord",
        color: "ADJUDICATOR",
    },
    InfluenceInfo {
        key: "basilisk",
        display: "Hunter",
        color: "BASILISK",
    },
    InfluenceInfo {
        key: "crusader",
        display: "Crusader",
        color: "CRUSADER",
    },
    InfluenceInfo {
        key: "eyrie",
        display: "Redeemer",
        color: "EYRIE",
    },
];

/// Apply value scalars to the first n numbers present
/// @pob Modules/ItemTools.lua:37-57 h=42d468ce
pub fn applyValueScalar(
    line: &str,
    valueScalar: Option<f64>,
    baseValueScalar: Option<f64>,
    numbers: Option<f64>,
    precision: Option<f64>,
) -> String {
    // L38: type(valueScalar) == "number" and valueScalar or 1
    let valueScalar = valueScalar.unwrap_or(1.0);
    // L39
    if valueScalar == 1.0 && (baseValueScalar.is_none() || baseValueScalar == Some(1.0)) {
        // L40
        return line.to_owned();
    }

    // L43
    let power = lpow(10.0, precision.unwrap_or(0.0));
    // L44
    let scaleValue = |num: &[u8]| -> String {
        // L45
        let mut value = pob_lua::format::tonumber_bytes(num).unwrap_or_else(|| {
            lua_error!("attempt to perform arithmetic on local 'value' (a nil value)")
        });
        // L46
        if let Some(b) = baseValueScalar.filter(|b| *b != 1.0) {
            // L47
            value = round(value * b * power, None) / power;
        }
        // L49
        tostring_num(
            m_floor(value * valueScalar * power + if precision.is_some() { 0.0 } else { 0.001 })
                / power,
        )
    };
    // gsub's `n` (luaL_optint truncates)
    let n = numbers.map(|x| x as i64);
    // L51
    if precision.is_some() {
        // L52
        return gsub_fn_bytes(
            line,
            "(%d+%.?%d*)",
            |c| scaleValue(cap_bytes(c, 0)).into_bytes(),
            n,
        );
    }
    // L54
    gsub_fn_bytes(
        line,
        "(%d+)([^%.])",
        |c| {
            // L55
            let mut out = scaleValue(cap_bytes(c, 0)).into_bytes();
            out.extend_from_slice(cap_bytes(c, 1));
            out
        },
        n,
    )
}

/// precision is express a multiplier/divide and displayPrecision is expressed as decimal precision on rounding.
/// ifRequired determines whether trailing zeros are displayed or not.
/// @pob Modules/ItemTools.lua:61-75 h=c3aa4f2e
pub fn formatValue(
    value: f64,
    baseValueScalar: Option<f64>,
    valueScalar: Option<f64>,
    precision: Option<f64>,
    displayPrecision: Option<f64>,
    ifRequired: bool,
) -> String {
    let Some(precision) = precision else {
        lua_error!("attempt to perform arithmetic on local 'precision' (a nil value)")
    };
    // L62: resolve range to internal value
    let mut value = roundSymmetric(value * precision, None);
    // L63: apply corrupted mult
    if let Some(b) = baseValueScalar.filter(|b| *b != 1.0) {
        value = alwaysPositiveRound(value * b, None);
    }
    // L64: apply modifier magnitude
    if let Some(v) = valueScalar.filter(|v| *v != 1.0) {
        value = floorSymmetric(value * v, None);
    }
    // L65-66: convert back to display space
    value = value / precision;
    // L67: presentation
    if let Some(dp) = displayPrecision {
        value = roundSymmetric(value, Some(dp as i32));
    }
    // L68: whitespace is needed
    if let Some(dp) = displayPrecision.filter(|_| !ifRequired) {
        // L69
        pob_lua::format::lua_format(
            &format!("%.{}f", tostring_num(dp)),
            &[pob_lua::format::FmtArg::Num(value)],
        )
    } else if displayPrecision.is_some() {
        // L71: tostring(value, displayPrecision) (tostring ignores its second argument)
        tostring_num(value)
    } else {
        // L73: max decimals ingame is 2
        let dec = m_min(2.0, m_floor(m_log(precision, Some(10.0)) + 0.001));
        tostring_num(roundSymmetric(value, Some(dec as i32)))
    }
}

// @pob Modules/ItemTools.lua:76-81 h=3d97ddd5
// (module-level statements: antonyms)
/// `antonyms[word]`.
fn antonyms(word: &[u8]) -> Option<&'static str> {
    match word {
        b"increased" => Some("reduced"),
        b"reduced" => Some("increased"),
        b"more" => Some("less"),
        b"less" => Some("more"),
        _ => None,
    }
}

/// @pob Modules/ItemTools.lua:83-86 h=e7105ba1
pub fn antonymFunc(num: &str, word: &str) -> String {
    into_string(antonymFunc_bytes(num.as_bytes(), word.as_bytes()))
}

/// [`antonymFunc`] on the bytes of two captures.
fn antonymFunc_bytes(num: &[u8], word: &[u8]) -> Vec<u8> {
    // L84
    let antonym = antonyms(word);
    // L85
    let mut out = Vec::with_capacity(num.len() + word.len() + 2);
    match antonym {
        Some(a) => {
            out.extend_from_slice(num);
            out.push(b' ');
            out.extend_from_slice(a.as_bytes());
        }
        None => {
            out.push(b'-');
            out.extend_from_slice(num);
            out.push(b' ');
            out.extend_from_slice(word);
        }
    }
    out
}

/// `gsub(p, antonymFunc)` (both captures).
fn gsub_antonym(s: &str, p: &str) -> String {
    gsub_fn_bytes(
        s,
        p,
        |c| antonymFunc_bytes(cap_bytes(c, 0), cap_bytes(c, 1)),
        None,
    )
}

/// @pob Modules/ItemTools.lua:88-90 h=40feee29
pub fn isZeroValueLine(line: &str) -> bool {
    // L89
    matches(line, "^%+?0%%? ")
        || (matches(line, " %+?0%%? ")
            && !matches(line, "0 to [1-9]")
            && !matches(line, "0%% to %d+%%"))
        || matches(line, " 0%-0 ")
        || matches(line, " 0 to 0 ")
}

/// Apply range value (0 to 1) to a modifier that has a range: "(x-x)" or "(x-x) to (x-x)"
///
/// `range` is a Lua value (CCR-0032 §4): a number, or a list of numbers used per `(x-y)` range
/// (`ranges[rangeIndex] or 0.5`, ItemTools.lua:99/104), as stored by ParseRaw for
/// `{range:a,b}` (Item.lua:803-809); callers pass `modLine.range`, which may be nil.
/// @pob Modules/ItemTools.lua:96-355 h=46779a84
pub fn applyRange(
    line: &str,
    range: &Val, // CCR-0032 §4
    valueScalar: Option<f64>,
    baseValueScalar: Option<f64>,
) -> String {
    let d = data();
    // stripLines down to # in place of any number and store numbers inside values also remove all + signs are kept if value is positive
    // L98
    let mut values: Vec<String> = Vec::new();
    // L99
    let mut rangeIndex: i64 = 0;
    // L100
    let ranges = is_table(range);
    // L101
    let strippedLine = gsub_fn_bytes(
        line,
        "([%+-]?)%((%-?%d+%.?%d*)%-(%-?%d+%.?%d*)%)",
        |c| {
            let sign = cap_bytes(c, 0);
            // L102
            rangeIndex += 1;
            // L103
            let valueRange = if ranges {
                lor(index_val(range, rangeIndex), || Val::Num(0.5))
            } else {
                range.clone()
            };
            // L104: min + valueRange * (tonumber(max) - min)
            let min = cap_num(c, 1);
            let max = cap_num(c, 2);
            let mut value = min + arith(&valueRange, "local 'valueRange'") * (max - min);
            // L105
            if sign == b"-" {
                value = value * -1.0;
            }
            // L106
            if sign == b"+" && value > 0.0 {
                format!("+{}", tostring_num(value)).into_bytes()
            } else {
                tostring_num(value).into_bytes()
            }
        },
        None,
    );
    // L108
    let strippedLine = gsub_antonym(&strippedLine, "%-(%d+%.?%d*%%) (%a+)");
    // L109
    let strippedLine = gsub_fn_bytes(
        &strippedLine,
        "(%-?%d+%.?%d*)",
        |c| {
            // L110
            values.push(into_string(cap_bytes(c, 0).to_vec()));
            // L111
            b"#".to_vec()
        },
        None,
    );

    //- Takes a completely strippedLine where all values and ranges are replaced with a # + signs are kept for consistency upon re-substitution.
    //- This will then substitute back in the values until a line in scalabilityData is found this start with substituting everything and until none.
    //- This means if there is a more generic mod that might be scalable on both parameters but their is a narrower one that isn't it won't be scaled.
    //-@param line string the modLine stripped of all values and ranges replaced by #
    //-@param values any values present in the modLine
    //-@return any scalableLine line with only scalableValues replaced with #
    //-@return any scalableValues values which can be scaled and added into scalableLine in place of a #
    // L114-188
    let findScalableLine = |line: &str, values: &[String]| -> Option<(String, Vec<String>)> {
        // L122
        let replaceNthInstance = |input: &str, replacement: &str, n: usize| -> String {
            // L123
            let mut count = 0usize;
            // L124
            gsub_fn_bytes(
                input,
                "#",
                |c| {
                    // L125
                    count += 1;
                    // L126
                    if count == n {
                        // L127
                        replacement.as_bytes().to_vec()
                    } else {
                        // L129
                        cap_bytes(c, 0).to_vec()
                    }
                },
                None,
            )
        };

        // check combinations recursively largest to smallest
        // L135
        fn checkSubstitutionCombinations(
            line: &str,
            values: &[String],
            replaceNthInstance: &dyn Fn(&str, &str, usize) -> String,
            i: usize,
            numSubstitutions: usize,
            indices: &mut Vec<usize>,
        ) -> Option<(String, Vec<String>)> {
            let d = data();
            // L136
            if indices.len() == numSubstitutions {
                // L137
                let mut modifiedLine = line.to_owned();
                // L138
                let mut substituted = 0usize;
                // L139
                for &i in indices.iter() {
                    // L140
                    modifiedLine =
                        replaceNthInstance(&modifiedLine, &values[i - 1], i - substituted);
                    // L141
                    substituted += 1;
                }

                // Check if the modified line matches any scalability data
                // L145
                let key = gsub_str(&modifiedLine, "+#", "#", None);
                // L146
                if d.modScalability.contains_key(key.as_str()) {
                    // Return modified line and remaining values (those not substituted)
                    // L148
                    let mut remainingValues = Vec::new();
                    // L149
                    let mut used = vec![false; values.len() + 1];
                    // L150
                    for &index in indices.iter() {
                        // L151
                        used[index] = true;
                    }
                    // L153
                    for (i, value) in values.iter().enumerate() {
                        // L154
                        if !used[i + 1] {
                            // L155
                            remainingValues.push(value.clone());
                        }
                    }
                    // L158
                    return Some((modifiedLine, remainingValues));
                }
                // L160
                return None;
            }
            // L162
            for j in i..=values.len() {
                // L163
                indices.push(j);
                // L164
                let r = checkSubstitutionCombinations(
                    line,
                    values,
                    replaceNthInstance,
                    j + 1,
                    numSubstitutions,
                    indices,
                );
                // L165
                if r.is_some() {
                    // L166
                    return r;
                }
                // L168
                indices.pop();
            }
            None
        }

        // L172
        for i in (1..=values.len()).rev() {
            // L173
            let r = checkSubstitutionCombinations(
                line,
                values,
                &replaceNthInstance,
                1,
                i,
                &mut Vec::new(),
            );
            // L174
            if r.is_some() {
                // L175
                return r;
            }
        }

        // Check scalability with 0 substitutions
        // L180
        let key = gsub_str(line, "+#", "#", None);
        // L181
        if d.modScalability.contains_key(key.as_str()) {
            // L182
            return Some((line.to_owned(), values.to_vec()));
        }

        // L185
        None
    };

    // L190
    let found = findScalableLine(&strippedLine, &values);

    // L192: found scalability data
    if let Some((mut scalableLine, mut scalableValues)) = found {
        let key = gsub_str(&scalableLine, "+#", "#", None);
        let Some(entries) = d.modScalability.get(key.as_str()) else {
            lua_error!("bad argument #1 to 'ipairs' (table expected, got nil)")
        };
        // L193
        for (i, scalability) in entries.iter().enumerate() {
            // L194-196
            let mut precision: Option<f64> = None;
            let mut displayPrecision: Option<f64> = None;
            let mut ifRequired = false;
            // L197
            if let Some(formats) = &scalability.formats {
                // L198
                for format in formats {
                    // L199-287
                    match format.as_str() {
                        "divide_by_two_0dp" => {
                            precision = Some(2.0);
                            displayPrecision = Some(0.0);
                            ifRequired = true;
                        }
                        "divide_by_three" => precision = Some(3.0),
                        "divide_by_four" => precision = Some(4.0),
                        "divide_by_five" => precision = Some(5.0),
                        "divide_by_six" => precision = Some(6.0),
                        "divide_by_ten_0dp" => {
                            precision = Some(10.0);
                            displayPrecision = Some(0.0);
                        }
                        "divide_by_ten_1dp" => {
                            precision = Some(10.0);
                            displayPrecision = Some(1.0);
                        }
                        "divide_by_ten_1dp_if_required" => {
                            precision = Some(10.0);
                            displayPrecision = Some(1.0);
                            ifRequired = true;
                        }
                        "divide_by_twelve" => precision = Some(12.0),
                        "divide_by_fifteen_0dp" => {
                            precision = Some(15.0);
                            displayPrecision = Some(0.0);
                        }
                        "divide_by_twenty" => precision = Some(20.0),
                        // might be incorrect?
                        "divide_by_twenty_then_double_0dp" => {
                            precision = Some(10.0);
                            displayPrecision = Some(0.0);
                        }
                        "divide_by_one_hundred" | "divide_by_one_hundred_and_negate" => {
                            precision = Some(100.0)
                        }
                        "divide_by_one_hundred_0dp" => {
                            precision = Some(100.0);
                            displayPrecision = Some(0.0);
                        }
                        "divide_by_one_hundred_1dp" => {
                            precision = Some(100.0);
                            displayPrecision = Some(1.0);
                        }
                        "divide_by_one_hundred_2dp" => {
                            precision = Some(100.0);
                            displayPrecision = Some(2.0);
                        }
                        "divide_by_one_hundred_2dp_if_required" => {
                            precision = Some(100.0);
                            displayPrecision = Some(2.0);
                            ifRequired = true;
                        }
                        "divide_by_one_thousand" => precision = Some(1000.0),
                        "divide_by_ten_thousand_1dp" => {
                            precision = Some(10000.0);
                            displayPrecision = Some(1.0);
                        }
                        "per_minute_to_per_second" => precision = Some(60.0),
                        "per_minute_to_per_second_0dp" => {
                            precision = Some(60.0);
                            displayPrecision = Some(0.0);
                        }
                        "per_minute_to_per_second_1dp" => {
                            precision = Some(60.0);
                            displayPrecision = Some(1.0);
                        }
                        "per_minute_to_per_second_2dp" => {
                            precision = Some(60.0);
                            displayPrecision = Some(2.0);
                        }
                        "per_minute_to_per_second_2dp_if_required" => {
                            precision = Some(60.0);
                            displayPrecision = Some(2.0);
                            ifRequired = true;
                        }
                        "milliseconds_to_seconds" => precision = Some(1000.0),
                        "milliseconds_to_seconds_halved" => precision = Some(1000.0),
                        "milliseconds_to_seconds_0dp" => {
                            precision = Some(1000.0);
                            displayPrecision = Some(0.0);
                        }
                        "milliseconds_to_seconds_1dp" => {
                            precision = Some(1000.0);
                            displayPrecision = Some(1.0);
                        }
                        "milliseconds_to_seconds_2dp" => {
                            precision = Some(1000.0);
                            displayPrecision = Some(2.0);
                        }
                        "locations_to_metres" => {
                            precision = Some(10.0);
                            displayPrecision = Some(1.0);
                        }
                        "milliseconds_to_seconds_2dp_if_required" => {
                            precision = Some(1000.0);
                            displayPrecision = Some(2.0);
                            ifRequired = true;
                        }
                        "deciseconds_to_seconds" => precision = Some(10.0),
                        _ => {}
                    }
                }
            }
            // L290: `scalableValues[i]` (a string) in arithmetic; nil raises
            let Some(v) = scalableValues.get(i) else {
                lua_error!("attempt to perform arithmetic on local 'value' (a nil value)")
            };
            let v = arith(&Val::Str(LStr::from(v.as_str())), "local 'value'");
            if scalability.isScalable
                && (baseValueScalar.is_some_and(|b| b != 1.0)
                    || valueScalar.is_some_and(|v| v != 1.0))
            {
                // L291
                scalableValues[i] = formatValue(
                    v,
                    baseValueScalar,
                    valueScalar,
                    Some(precision.unwrap_or(1.0)),
                    displayPrecision,
                    ifRequired,
                );
            } else {
                // L294
                scalableValues[i] = formatValue(
                    v,
                    Some(1.0),
                    Some(1.0),
                    Some(precision.unwrap_or(1.0)),
                    displayPrecision,
                    ifRequired,
                );
            }
        }
        // L298
        for replacement in &scalableValues {
            // L299
            scalableLine = gsub_str(&scalableLine, "#", replacement, Some(1));
        }
        // L301
        scalableLine
    } else {
        // fallback to old method for determining scalability
        // ConPrintf("Couldn't find scalability data falling back to old implementation: %s", strippedLine)
        // L304
        let mut precisionSame = true;
        // Create a line with ranges removed to check if the mod is a high precision mod.
        // L306
        let testLine = if !line.contains('-') {
            line.to_owned()
        } else {
            let t = gsub_fn_bytes(
                line,
                "(%+?)%((%-?%d+%.?%d*)%-(%-?%d+%.?%d*)%)",
                |c| {
                    let plus = cap_bytes(c, 0);
                    // L309
                    let min = cap_num(c, 1);
                    // L310
                    let maxPrecision = min + arith(range, "local 'range'") * (cap_num(c, 2) - min);
                    // L311
                    let minPrecision = m_floor(maxPrecision + 0.5);
                    // L312
                    if minPrecision != maxPrecision {
                        // L313
                        precisionSame = false;
                    }
                    // L315
                    let mut out = if minPrecision < 0.0 {
                        Vec::new()
                    } else {
                        plus.to_vec()
                    };
                    out.extend_from_slice(tostring_num(minPrecision).as_bytes());
                    out
                },
                None,
            );
            // L317
            gsub_antonym(&t, "%-(%d+%%) (%a+)")
        };

        // L319
        if precisionSame
            && (valueScalar.is_none() || valueScalar == Some(1.0))
            && (baseValueScalar.is_none() || baseValueScalar == Some(1.0))
        {
            // L320
            return testLine;
        }

        // L323
        let mut precision: Option<f64> = None;
        // L324
        let (modList, extra) = parseMod(&testLine, false);
        // L325
        if let (Some(modList), None) = (&modList, &extra) {
            // L326 (`pairs` over a list: the array part, in index order)
            for r#mod in modList {
                // L327
                let mut subMod: &Mod = r#mod;
                // L328
                if let Value::Table(t) = &r#mod.value {
                    if let Value::Mod(m) = t.get(n!("mod")) {
                        // L329
                        subMod = m;
                    }
                }
                // L331
                if let Value::Num(_) = subMod.value {
                    if let Some(p) = d
                        .highPrecisionMods
                        .get(&subMod.name)
                        .and_then(|m| m.get(&subMod.r#type))
                    {
                        // L332
                        precision = Some(f64::from(*p));
                    }
                }
            }
        }
        // L336
        if precision.is_none() && matches(line, "(%d+%.%d*)") {
            // L337
            precision = Some(f64::from(d.defaultHighPrecision));
        }
        // L339
        let mut numbers = 0.0;
        // L340
        let line = gsub_fn_bytes(
            line,
            "(%+?)%((%-?%d+%.?%d*)%-(%-?%d+%.?%d*)%)",
            |c| {
                let plus = cap_bytes(c, 0);
                // L342
                numbers = numbers + 1.0;
                // L343
                let power = lpow(10.0, precision.unwrap_or(0.0));
                // L344
                let min = cap_num(c, 1);
                let numVal = m_floor(
                    (min + arith(range, "local 'range'") * (cap_num(c, 2) - min)) * power + 0.5,
                ) / power;
                // L346
                let mut out = if numVal < 0.0 {
                    Vec::new()
                } else {
                    plus.to_vec()
                };
                out.extend_from_slice(tostring_num(numVal).as_bytes());
                out
            },
            None,
        );
        // L348
        let line = gsub_antonym(&line, "%-(%d+%%) (%a+)");

        // If a mod contains x or x% and is not already a ranged value, then assume only the first number will be scalable.
        // L350
        if numbers == 0.0 && matches(&line, "(%d+%.?%d*)%%? ") {
            // L351
            numbers = 1.0;
        }

        // L354
        applyValueScalar(
            &line,
            valueScalar,
            baseValueScalar,
            Some(numbers),
            precision,
        )
    }
}

/// `range` is passed to `applyRange` unchanged (CCR-0032 §4).
/// @pob Modules/ItemTools.lua:357-359 h=49fbc3b8
pub fn isModLineScalable(line: &str, range: &Val, valueScalar: Option<f64>) -> bool {
    // L358
    applyRange(line, range, valueScalar, Some(1.0))
        != applyRange(line, range, valueScalar, Some(2.0))
}

/// @pob Modules/ItemTools.lua:361-382 h=57921347
pub fn formatModLine(modLine: &Table, dbMode: bool) -> Option<String> {
    let modLine_line = || match modLine.get(n!("line")) {
        Val::Str(s) => s,
        other => lua_error!(
            "bad argument #1 to 'match' (string expected, got {})",
            other.lua_type()
        ),
    };
    // L362
    let shouldApplyRange =
        !dbMode && (modLine.truthy(n!("range")) || modLine.truthy(n!("corruptedRange")));
    // L363
    let line: String = if shouldApplyRange {
        let range = lor(modLine.get(n!("range")), || {
            crate::classes::item_parse::main_defaultItemAffixQuality().map_or(Val::Nil, Val::Num)
        });
        applyRange(
            &modLine_line(),
            &range,
            modLine.opt_num(n!("valueScalar")),
            modLine.opt_num(n!("corruptedRange")),
        )
    } else {
        modLine_line().to_string()
    };
    // L365: Hack to hide 0-value modifiers
    if matches(&line, "^%+?0%%? ")
        || (matches(&line, " %+?0%%? ") && !matches(&line, "0 to [1-9]"))
        || matches(&line, " 0%-0 ")
        || matches(&line, " 0 to 0 ")
    {
        // L366
        return None;
    }
    // L368
    if modLine.truthy(n!("disabled")) {
        // L369
        return Some(format!("{}{line}", color_code("DISABLED")));
    }
    // L371
    let colorCode: &str;
    let mut line = line;
    // L372
    let extra = modLine.get(n!("extra"));
    if extra.truthy() {
        // L373
        colorCode = color_code("UNSUPPORTED");
        // L374 (CCR-0061)
        if main_notSupportedModTooltips() {
            line = format!("{line}{main_notSupportedTooltipText}");
        }
        // L375
        if launch_devModeAlt() {
            // L376
            let e = match &extra {
                Val::Str(s) => s.to_string(),
                Val::Num(n) => tostring_num(*n),
                other => lua_error!(
                    "attempt to concatenate field 'extra' (a {} value)",
                    other.lua_type()
                ),
            };
            line = format!("{line}   ^1'{e}'");
        }
    } else {
        // L379
        colorCode = if modLine.truthy(n!("fractured")) {
            color_code("FRACTURED")
        } else if modLine.truthy(n!("crafted")) {
            color_code("CRAFTED")
        } else if modLine.truthy(n!("mutated")) {
            color_code("MUTATED")
        } else if modLine.truthy(n!("scourge")) {
            color_code("SCOURGE")
        } else if modLine.truthy(n!("custom")) {
            color_code("CUSTOM")
        } else if modLine.truthy(n!("crucible")) {
            color_code("CRUCIBLE")
        } else if modLine.truthy(n!("vestigial")) {
            color_code("VESTIGIAL")
        } else {
            color_code("MAGIC")
        };
    }
    // L381
    Some(format!("{colorCode}{line}"))
}

// @pob-skip Modules/ItemTools.lua:384-413 reason="itemLib.wiki: opens poewiki.net in the browser (OpenURL, F1 key binding); UI only, the UI port owns it"
