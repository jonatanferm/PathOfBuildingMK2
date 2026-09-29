//! A small Lua 5.1 interpreter (pure Rust) for running PoB's rule tables —
//! chiefly `Modules/ModParser.lua` — unchanged.
//!
//! Only the language subset and standard library PoB's data/rule code uses
//! are implemented: no metatables, coroutines, `goto` or string.dump.

pub mod host;
pub mod interp;
pub mod lexer;
pub mod parser;
pub mod stdlib;
pub mod value;

pub use interp::{Interp, LuaError};
pub use value::{Func, TableRef, Val};

#[cfg(test)]
mod tests {
    use super::*;

    fn run(src: &str) -> Vec<Val> {
        let mut it = Interp::new();
        it.exec_chunk(src, "test", Vec::new())
            .unwrap_or_else(|e| panic!("{e}"))
    }

    fn run1(src: &str) -> String {
        run(src)
            .first()
            .map(ToString::to_string)
            .unwrap_or_default()
    }

    #[test]
    fn arithmetic_and_strings() {
        assert_eq!(run1("return 1 + 2 * 3 ^ 2"), "19");
        assert_eq!(run1("return -2 ^ 2"), "-4");
        assert_eq!(run1("return 7 % 3, 1"), "1");
        assert_eq!(run1("return 'a' .. 1 .. 'b'"), "a1b");
        assert_eq!(run1("return ('Hello'):lower()"), "hello");
        assert_eq!(run1("return #'abc' + #{1,2}"), "5");
        assert_eq!(run1("return 10 / 4"), "2.5");
        assert_eq!(run1("return 1 < 2 and 'y' or 'n'"), "y");
        assert_eq!(run1("return nil or false"), "false");
    }

    #[test]
    fn closures_varargs_and_loops() {
        let src = r#"
            local function counter()
                local n = 0
                return function() n = n + 1; return n end
            end
            local c = counter()
            c(); c()
            local fns = {}
            for i = 1, 3 do fns[i] = function() return i end end
            local sum = 0
            for _, f in ipairs(fns) do sum = sum + f() end
            local function va(...) return select('#', ...), ... end
            local n, a, b = va(4, 5)
            return c() * 100 + sum * 10 + n + a + b
        "#;
        // 3*100 + 6*10 + 2 + 4 + 5
        assert_eq!(run1(src), "371");
    }

    #[test]
    fn tables_and_stdlib() {
        let src = r#"
            local t = { "b", "c", x = 1, ["y z"] = 2 }
            table.insert(t, 1, "a")
            table.insert(t, "d")
            local keys = 0
            for k, v in pairs(t) do keys = keys + 1 end
            table.sort(t, function(p, q) return p > q end)
            local s = table.concat(t, ",")
            local caps = { ("+43 to strength"):match("^([%+%-]?%d+) to (%a+)") }
            local g = ("a,b"):gsub(",", ";")
            return s .. "|" .. keys .. "|" .. caps[1] .. caps[2] .. "|" .. g .. "|" .. string.format("%d%%/%.2f/%s", 5, 1.005, "x")
        "#;
        assert_eq!(run1(src), "d,c,b,a|6|+43strength|a;b|5%/1.00/x");
    }

    #[test]
    fn bit_ops() {
        assert_eq!(run1("return bit.bor(1, 4, 16)"), "21");
        assert_eq!(
            run1("return bit.band(0x40000001, bit.bnot(0x40000000))"),
            "1"
        );
    }
}
