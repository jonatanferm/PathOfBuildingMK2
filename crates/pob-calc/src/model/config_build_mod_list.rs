//! @pob-file Classes/ConfigTab.lua @16de4b82d
//! ConfigTab:BuildModList (Classes/ConfigTab.lua 1130-1204).
//!
//! Owner: CO-C. The signature is U0's (frozen).
//!
//! * `varList` (`require("Modules.ConfigOptions")`, ConfigTab.lua:12) is `data.configOptions`;
//!   each option's `apply` runs through the registry (`configApply`, CO-A/CO-B bodies).
//! * The applies read and write the model's control-state tables
//!   (`self.varControls`, CCR-0037/CCR-0038): BuildModList itself only passes `build` along.
//! * `modLib.parseMod` is `pob_modparser::parseMod` (`isComb` nil → false).
#![allow(non_snake_case, clippy::all, clippy::pedantic)]

use crate::modules::config_options::configApply;
use crate::prelude::*;

use pob_gamedata::mod_tools::setSource;
use pob_lua::pattern::{gmatch, r#match};
use pob_lua::string::StripEscapes;

/// `input[varData.var]` (`t[nil]` reads nil).
fn get_var(t: &Table, var: Option<Name>) -> Val {
    match var {
        Some(var) => t.get(var),
        None => Val::Nil,
    }
}

/// Lua `x ~= 0` (no coercion: only the number 0 is equal).
fn ne_zero(x: &Val) -> bool {
    !x.raw_eq(&Val::Num(0.0))
}

/// Lua `#s` on a value that is a string here (`#` of a table is its border; other kinds raise).
fn len_of(x: &Val) -> f64 {
    match x {
        Val::Str(s) => s.len() as f64,
        Val::Table(t) => t.len() as f64,
        other => lua_error!("attempt to get length of a {} value", other.lua_type()),
    }
}

/// The receiver of `text:gmatch(...)`: only strings have string methods.
fn str_of(x: &Val) -> LStr {
    match x {
        Val::Str(s) => s.clone(),
        other => lua_error!(
            "attempt to call method 'gmatch' (a nil value) on a {} value",
            other.lua_type()
        ),
    }
}

/// Lua `"Custom:" .. x` operand (numbers use `tostring`).
fn cat_operand(x: &Val) -> String {
    match x {
        Val::Str(s) => s.to_string(),
        Val::Num(n) => tostring_num(*n),
        other => lua_error!("attempt to concatenate a {} value", other.lua_type()),
    }
}

/// `StripEscapes(line):match("^%s*(.-)%s*$")` (the pattern always matches).
fn strip_line(line: &str) -> String {
    let stripped = StripEscapes(line);
    let r = match r#match(&stripped, "^%s*(.-)%s*$", None) {
        Some(caps) => caps[0].to_str().into_owned(),
        None => lua_error!("pattern '^%s*(.-)%s*$' did not match"),
    };
    r
}

/// The lines of `text:gmatch("([^\n]*)\n?")` (a final empty match included, as in Lua).
fn lines_of(text: &str) -> Vec<String> {
    gmatch(text, "([^\n]*)\n?")
        .map(|caps| caps[0].to_str().into_owned())
        .collect()
}

/// @pob Classes/ConfigTab.lua:1130-1203 h=f253337c
impl ConfigTab {
    pub fn BuildModList(&self, build: &BuildRef) {
        // L1131
        let modList = ModStore::ModList(None);
        // L1132
        self.modList.set(modList.clone());
        // L1133
        let enemyModList = ModStore::ModList(None);
        // L1134
        self.enemyModList.set(enemyModList.clone());
        // L1135
        let input = match self.activeConfigSet().get(n!("input")) {
            Val::Table(t) => t,
            other => lua_error!(
                "attempt to index local 'input' (a {} value)",
                other.lua_type()
            ),
        };
        // L1136
        let placeholder = match self.activeConfigSet().get(n!("placeholder")) {
            Val::Table(t) => t,
            other => lua_error!(
                "attempt to index local 'placeholder' (a {} value)",
                other.lua_type()
            ),
        };
        // L1137
        self.UpdateLevel(build); // enemy level handled here because it's needed to correctly set boss stats
                                 // L1138
        for varData in &build.data.configOptions {
            // L1139
            if let Some(apply) = &varData.apply {
                let apply = configApply(apply);
                let ty = varData.r#type.as_ref().map(|t| t.as_str());
                // L1140
                if ty == Some("check") {
                    // L1141
                    if get_var(&input, varData.var).truthy() {
                        // L1142
                        apply(&Val::Bool(true), &modList, &enemyModList, build);
                    }
                } else if matches!(ty, Some("count" | "integer" | "countAllowZero" | "float")) {
                    // L1145
                    let iv = get_var(&input, varData.var);
                    let pv = get_var(&placeholder, varData.var);
                    if iv.truthy() && (ne_zero(&iv) || ty == Some("countAllowZero")) {
                        // L1146
                        apply(&iv, &modList, &enemyModList, build);
                    } else if pv.truthy() && (ne_zero(&pv) || ty == Some("countAllowZero")) {
                        // L1147-1148
                        apply(&pv, &modList, &enemyModList, build);
                    }
                } else if ty == Some("list") {
                    // L1151
                    let iv = get_var(&input, varData.var);
                    if iv.truthy() {
                        // L1152
                        apply(&iv, &modList, &enemyModList, build);
                    }
                } else if ty == Some("text") {
                    // L1155
                    let iv = get_var(&input, varData.var);
                    if iv.truthy() {
                        // L1156
                        apply(&iv, &modList, &enemyModList, build);
                    }
                }
            }
        }

        // Apply Custom Modifier groups
        // L1163
        let customModsList = self.activeConfigSet().get(n!("customModsList"));
        // L1164
        let mut hasBlockText = false;
        // L1165
        if customModsList.truthy() {
            // L1166
            if customModsList.lua_type() != "table" {
                lua_error!(
                    "bad argument #1 to 'ipairs' (table expected, got {})",
                    customModsList.lua_type()
                );
            }
            let mut i: i64 = 1;
            loop {
                let block = customModsList.get(i);
                if block.is_nil() {
                    break;
                }
                // L1167
                let text = block.get(n!("text"));
                if !block.get(n!("enabled")).raw_eq(&Val::Bool(false))
                    && text.truthy()
                    && len_of(&text) > 0.0
                {
                    // L1168
                    hasBlockText = true;
                    // L1169
                    for line in lines_of(&str_of(&text)) {
                        // L1170
                        let strippedLine = strip_line(&line);
                        // L1171
                        let (mods, extra) = pob_modparser::parseMod(&strippedLine, false);
                        // L1172
                        if let (Some(mods), None) = (mods, extra) {
                            // L1173
                            let source = format!(
                                "Custom:{}",
                                cat_operand(&lor(block.get(n!("title")), || Val::from("Default")))
                            );
                            // L1174-1176 (`mods` has no holes)
                            for r#mod in &mods {
                                // L1177
                                let r#mod = setSource(r#mod, &source);
                                // L1178
                                modList.AddMod(Arc::new(r#mod));
                            }
                        }
                    }
                }
                i += 1;
            }
        }
        // Fallback for tests/headless
        // L1187
        let customMods = input.get(n!("customMods"));
        if !hasBlockText && customMods.truthy() && len_of(&customMods) > 0.0 {
            // L1188
            for line in lines_of(&str_of(&customMods)) {
                // L1189
                let strippedLine = strip_line(&line);
                // L1190
                let (mods, extra) = pob_modparser::parseMod(&strippedLine, false);
                // L1191
                if let (Some(mods), None) = (mods, extra) {
                    // L1192
                    let source = "Custom";
                    // L1193-1195 (`mods` has no holes)
                    for r#mod in &mods {
                        // L1196
                        let r#mod = setSource(r#mod, source);
                        // L1197
                        modList.AddMod(Arc::new(r#mod));
                    }
                }
            }
        }
    }
}
