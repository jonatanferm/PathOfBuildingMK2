//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Modules/Main.lua:1795-1820 (main:WrapString
//! with its `do`-block `wrapTable`). Ui doc §6.11. Owner: U14.
//!
//! @pob-file Modules/Main.lua @16de4b82d
//!
//! DIVERGENCE(alias): Lua returns the one module-level `wrapTable`, wiped and refilled by every
//! call, so a caller that kept the previous result sees it change. The port returns a fresh
//! `Vec` per call. Every PoB caller (Tooltip.lua:106, ItemsTab, TreeTab, ...) iterates the result
//! immediately, before the next call (grep `WrapString`), so nothing observes the aliasing.
//!
//! Lua quirk kept: `lastBreak` is assigned `s - 1` before the width test, so the line pushed when
//! the text up to the current space is too wide INCLUDES the word that overflowed (the wrap
//! happens after it, not before it).

use pob_lua::string::sub;
use pob_sg::Font;

use crate::cx::Cx;

/// lua: Main.lua:1797-1819 main:WrapString(str, height, width)
/// @pob Modules/Main.lua:1795-1820 h=86ba891a
pub fn wrap_string(cx: &Cx, s: &str, height: f64, width: f64) -> Vec<String> {
    // L1796 local wrapTable = { } / L1798 wipeTable(wrapTable)
    let mut wrap_table: Vec<String> = Vec::new();
    let len = s.len() as i64;
    // L1799
    let mut line_start: i64 = 1;
    // L1800 local lastSpace, lastBreak
    let mut last_space: Option<i64> = None;
    // L1801
    loop {
        // L1802 local s, e = str:find("%s+", lastSpace)
        let found =
            pob_lua::find(s, "%s+", last_space, false).map(|(a, b, _)| (a as i64, b as i64));
        // L1803
        let (st, e) = match found {
            Some(p) => p,
            // L1804-1805
            None => (len + 1, len + 1),
        };
        // L1807
        if st > len {
            // L1808
            wrap_table.push(sub(s, line_start, Some(-1)).into_owned());
            // L1809
            break;
        }
        // L1811
        let last_break = st - 1;
        // L1812
        last_space = Some(e + 1);
        // L1813
        if cx
            .sg
            .draw_string_width(height, Font::Var, &sub(s, line_start, Some(st - 1)))
            > width
        {
            // L1814
            wrap_table.push(sub(s, line_start, Some(last_break)).into_owned());
            // L1815
            line_start = e + 1;
        }
    }
    // L1818
    wrap_table
}

#[cfg(test)]
mod tests {
    use super::*;

    /// With the inert font set every width is 0, so nothing wraps: one line, the whole string
    /// (including leading/trailing whitespace, as `str:sub(1, -1)`).
    #[test]
    fn no_wrap_when_everything_fits() {
        let cx = Cx::headless();
        assert_eq!(wrap_string(&cx, "a b  c", 16.0, 100.0), vec!["a b  c"]);
        assert_eq!(wrap_string(&cx, "", 16.0, 100.0), vec![""]);
        assert_eq!(wrap_string(&cx, "  x ", 16.0, 100.0), vec!["  x "]);
    }

    /// A negative width makes every segment "too wide": each break pushes the text up to the
    /// space (the overflowing word included) and the next line starts after the whitespace run.
    #[test]
    fn every_break_taken_when_nothing_fits() {
        let cx = Cx::headless();
        assert_eq!(
            wrap_string(&cx, "one two\tthree", 16.0, -1.0),
            vec!["one", "two", "three"]
        );
        // leading whitespace: the first segment is empty (sub(1, 0) == "")
        assert_eq!(wrap_string(&cx, " a", 16.0, -1.0), vec!["", "a"]);
        // trailing whitespace: the remainder after the last run is empty
        assert_eq!(wrap_string(&cx, "a  ", 16.0, -1.0), vec!["a", ""]);
    }
}
