#![allow(non_snake_case)]
//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Data/Global.lua:7-100 (colorCodes,
//! defaultColorCodes, updateColorCode, hexToRGB). Ui doc §6.11. Owner: U4.
//!
//! Moved from the pre-port `pob-sg/src/color.rs` (`ColorCodes` as a name->value list) by UI-U0
//! and reshaped to the §6.11 contract: one `pub String` field per Lua key, exact Lua names.
//! The alias assignments (Global.lua:65-77) copy the string, so a later edit of MARAUDER does
//! not change LIFE, as in Lua.

/// ^0..^9 from SimpleGraphic.dll @0x185380 (re-exported from pob_sg::color)
pub use pob_sg::color::PALETTE;

macro_rules! color_codes {
    ($($name:ident = $val:literal,)* ; $($alias:ident = $target:ident,)*) => {
        /// The mutable global `colorCodes` table.
        #[allow(non_snake_case)]
        #[derive(Clone, Debug, PartialEq)]
        pub struct ColorCodes { $(pub $name: String,)* $(pub $alias: String,)* }

        impl Default for ColorCodes {
            /// Global.lua:7-77 (also `defaultColorCodes = copyTable(colorCodes)`, :79).
            fn default() -> Self {
                $(let $name = String::from($val);)*
                $(let $alias = $target.clone();)*
                ColorCodes { $($name,)* $($alias,)* }
            }
        }

        impl ColorCodes {
            /// `colorCodes[name]`.
            pub fn get(&self, name: &str) -> Option<&String> {
                match name {
                    $(stringify!($name) => Some(&self.$name),)*
                    $(stringify!($alias) => Some(&self.$alias),)*
                    _ => None,
                }
            }
            /// `colorCodes[name] = ...`.
            pub fn get_mut(&mut self, name: &str) -> Option<&mut String> {
                match name {
                    $(stringify!($name) => Some(&mut self.$name),)*
                    $(stringify!($alias) => Some(&mut self.$alias),)*
                    _ => None,
                }
            }
            /// Every key with its value (struct order: the table, then the aliases).
            pub fn iter(&self) -> impl Iterator<Item = (&'static str, &String)> {
                [$((stringify!($name), &self.$name),)* $((stringify!($alias), &self.$alias),)*].into_iter()
            }
        }
    };
}

color_codes! {
    NORMAL = "^xC8C8C8",
    MAGIC = "^x8888FF",
    RARE = "^xFFFF77",
    UNIQUE = "^xAF6025",
    RELIC = "^x60C060",
    GEM = "^x1AA29B",
    PROPHECY = "^xB54BFF",
    CURRENCY = "^xAA9E82",
    CRAFTED = "^xB8DAF1",
    CUSTOM = "^x5CF0BB",
    SOURCE = "^x88FFFF",
    UNSUPPORTED = "^xF05050",
    DISABLED = "^x7F7F7F",
    WARNING = "^xFF9922",
    TIP = "^x80A080",
    FIRE = "^xB97123",
    COLD = "^x3F6DB3",
    LIGHTNING = "^xADAA47",
    CHAOS = "^xD02090",
    POSITIVE = "^x33FF77",
    NEGATIVE = "^xDD0022",
    HIGHLIGHT = "^xFF0000",
    OFFENCE = "^xE07030",
    DEFENCE = "^x8080E0",
    SCION = "^xFFF0F0",
    MARAUDER = "^xE05030",
    RANGER = "^x70FF70",
    WITCH = "^x7070FF",
    DUELIST = "^xE0E070",
    TEMPLAR = "^xC040FF",
    SHADOW = "^x30C0D0",
    MAINHAND = "^x50FF50",
    MAINHANDBG = "^x071907",
    OFFHAND = "^xB7B7FF",
    OFFHANDBG = "^x070719",
    SHAPER = "^x55BBFF",
    ELDER = "^xAA77CC",
    FRACTURED = "^xA29160",
    MUTATED = "^xCD2285",
    ADJUDICATOR = "^xE9F831",
    BASILISK = "^x00CB3A",
    CRUSADER = "^x2946FC",
    EYRIE = "^xAAB7B8",
    CLEANSING = "^xF24141",
    TANGLE = "^x038C8C",
    CHILLBG = "^x151e26",
    FREEZEBG = "^x0c262b",
    SHOCKBG = "^x191732",
    SCORCHBG = "^x270b00",
    BRITTLEBG = "^x00122b",
    SAPBG = "^x261500",
    SCOURGE = "^xFF6E25",
    CRUCIBLE = "^xFFA500",
    SPLITPERSONALITY = "^xFFD62A",
    VESTIGIAL = "^xCBA5F1",
    INTANGIBILITY = "^x9BF4BD",
    MEMORY = "^xBFE2FA",
    ;
    STRENGTH = MARAUDER,
    DEXTERITY = RANGER,
    INTELLIGENCE = WITCH,
    LIFE = MARAUDER,
    MANA = WITCH,
    ES = SOURCE,
    WARD = RARE,
    ARMOUR = NORMAL,
    EVASION = POSITIVE,
    RAGE = WARNING,
    PHYS = NORMAL,
}

impl ColorCodes {
    /// lua: Global.lua:80-87 updateColorCode(code, color): only existing keys are updated;
    /// `color:gsub("^0", "^")` turns a leading `0` (as in `0xRRGGBB`) into `^`. Returns the new
    /// global `rgbColor` when `code == "HIGHLIGHT"` (`hexToRGB(color)`, which may be nil).
    pub fn update_color_code(&mut self, code: &str, color: &str) -> Option<Option<[f64; 3]>> {
        let entry = self.get_mut(code)?;
        *entry = match color.strip_prefix('0') {
            Some(rest) => format!("^{rest}"),
            None => color.to_owned(),
        };
        if code == "HIGHLIGHT" {
            Some(hex_to_rgb(color))
        } else {
            None
        }
    }
}

/// lua: Global.lua:89-100 hexToRGB: strips every `^x`, `0x` and `#` (gsub), then requires exactly
/// 6 bytes. A non-hex pair makes `tonumber(.., 16)` nil, and `nil / 255` is a Lua error.
pub fn hex_to_rgb(hex: &str) -> Option<[f64; 3]> {
    let h = hex.replace("^x", "").replace("0x", "").replace('#', "");
    if h.len() != 6 {
        return None;
    }
    let mut out = [0.0; 3];
    for (i, o) in out.iter_mut().enumerate() {
        let pair = h.get(i * 2..i * 2 + 2).unwrap_or("");
        let v = u8::from_str_radix(pair, 16)
            .unwrap_or_else(|_| crate::lua_error!("attempt to perform arithmetic on a nil value"));
        *o = f64::from(v) / 255.0;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Moved from the pre-port pob-sg color.rs `color_codes_table`.
    #[test]
    fn color_codes_table() {
        let mut cc = ColorCodes::default();
        assert_eq!(cc.NORMAL, "^xC8C8C8");
        assert_eq!(cc.LIFE, "^xE05030");
        assert_eq!(cc.PHYS, "^xC8C8C8");
        assert_eq!(cc.update_color_code("POSITIVE", "0x112233"), None); // only HIGHLIGHT sets rgbColor
        assert_eq!(cc.POSITIVE, "^x112233");
        // The alias copied the old string.
        assert_eq!(cc.EVASION, "^x33FF77");
        let rgb = cc
            .update_color_code("HIGHLIGHT", "^x00FF00")
            .unwrap()
            .unwrap();
        assert_eq!(rgb, [0.0, 1.0, 0.0]);
        assert_eq!(cc.update_color_code("NOPE", "^x000000"), None);
        assert_eq!(cc.get("NOPE"), None);
        assert_eq!(cc.iter().count(), 57 + 11);
    }
}
