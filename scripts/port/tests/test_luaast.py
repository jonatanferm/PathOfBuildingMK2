import os
import unittest

import _util
import luaast


def decls_named(res, name):
    return [d for d in res.decls if d.name == name]


def global_refs(res, name):
    return [r for n, r in res.globals if n == name]


class TokenizerTest(unittest.TestCase):
    def toks(self, src):
        return [(t.kind, t.value) for t in luaast.tokenize(src)][:-1]

    def test_numbers(self):
        vals = [v for k, v in self.toks("0x1F 1e3 .5 0x1p4 3LL 12 1.5e-2 0xA.8p0") if k == "num"]
        self.assertEqual(vals, [31, 1000.0, 0.5, 16.0, 3, 12, 0.015, 10.5])

    def test_strings_and_escapes(self):
        vals = [v for k, v in self.toks(r'"a\tb" ' + r"'\65\x42\z   C' " + '"\\u{48}"') if k == "str"]
        self.assertEqual(vals, ["a\tb", "ABC", "H"])

    def test_long_strings_and_comments(self):
        src = "--[==[ comment ]] still\n]==] x = [[\nline1\nline2]] --tail\ny"
        toks = luaast.tokenize(src)
        self.assertEqual([(t.kind, t.value, t.line) for t in toks[:-1]],
                         [("name", "x", 2), ("op", "=", 2), ("str", "line1\nline2", 2), ("name", "y", 5)])
        self.assertEqual(toks[2].endline, 4)

    def test_ops_longest_first(self):
        self.assertEqual([v for _, v in self.toks("a...b..c==d~=e<=f::g:: h+=1")],
                         ["a", "...", "b", "..", "c", "==", "d", "~=", "e", "<=", "f", "::", "g", "::", "h", "+=", 1])

    def test_shebang_skipped(self):
        self.assertEqual(self.toks("#!/bin/lua\nx"), [("name", "x")])

    def test_syntax_error(self):
        with self.assertRaises(luaast.LuaSyntaxError):
            luaast.parse("x = = 1")
        with self.assertRaises(luaast.LuaSyntaxError):
            luaast.parse("f(")
        with self.assertRaises(luaast.LuaSyntaxError):
            luaast.parse("x")


class ParserTest(unittest.TestCase):
    def test_statement_kinds(self):
        src = """
local a, b = 1, 2
local function f(x, ...) return ... end
function g.h:i(y) end
a, b = b, a
f(1)
do end
while a do break end
repeat until true
if a then elseif b then else end
for i = 1, 10, 2 do end
for k, v in pairs(t) do end
goto c
::c::
a += 1
return a
"""
        blk = luaast.parse(src)
        self.assertEqual([s.kind for s in blk.stmts],
                         ["Local", "LocalFunction", "Function", "Assign", "CallStat", "Do", "While", "Repeat",
                          "If", "NumFor", "GenFor", "Goto", "Label", "Assign", "Return"])
        self.assertTrue(blk.stmts[2].is_method)
        self.assertEqual(blk.stmts[2].path, ["g", "h", "i"])
        self.assertEqual(blk.stmts[13].compound, "+=")

    def test_precedence(self):
        e = luaast.parse("x = 1 + 2 * 3 ^ 2 ^ 1 .. 'a' .. 'b' or not y and z").stmts[0].exprs[0]
        self.assertEqual(e.op, "or")
        self.assertEqual(e.left.op, "..")            # concat binds looser than +
        self.assertEqual(e.left.right.op, "..")      # right associative
        self.assertEqual(e.left.left.op, "+")
        pw = e.left.left.right.right                 # 3 ^ (2 ^ 1)
        self.assertEqual((pw.op, pw.right.op), ("^", "^"))
        self.assertEqual(e.right.op, "and")
        self.assertEqual(e.right.left.kind, "UnOp")
        u = luaast.parse("x = -a ^ 2").stmts[0].exprs[0]
        self.assertEqual((u.kind, u.operand.op), ("UnOp", "^"))  # -(a^2)

    def test_line_spans(self):
        blk = luaast.parse("local x = {\n 1,\n 2,\n}\nfunction f()\n  return 1\nend\n")
        self.assertEqual((blk.stmts[0].line, blk.stmts[0].endline), (1, 4))
        self.assertEqual((blk.stmts[1].func.line, blk.stmts[1].func.endline), (5, 7))

    def test_source_text(self):
        blk = luaast.parse("for k, v in pairs(a.b[c] or  {}) do end")
        self.assertEqual(luaast.source_text(blk, blk.stmts[0].exprs[0].args[0]), "a.b[c] or {}")

    @_util.needs_pob
    def test_parses_every_calc_file(self):
        pob = _util.real_pob()
        for rel in ("Modules/CalcOffence.lua", "Modules/CalcPerform.lua", "Modules/CalcSetup.lua",
                    "Modules/CalcDefence.lua", "Modules/ModParser.lua", "Classes/ModStore.lua",
                    "Modules/Main.lua"):
            chunk = luaast.parse_file(os.path.join(pob, "src", rel))
            luaast.resolve(chunk)


class ResolverTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.chunk = luaast.parse_file(_util.fixture("scoping.lua"))
        cls.res = luaast.resolve(cls.chunk)

    def test_local_rhs_sees_outer(self):
        x3, x4 = [d for d in decls_named(self.res, "x") if d.line in (3, 4)]
        self.assertEqual([r.line for r in x3.refs], [4])
        # `undeclaredGlobal = x` (line 27) is after the do-block: the line-4 x
        self.assertEqual([r.line for r in x4.refs], [27, 28])

    def test_local_function_recursion(self):
        rec = decls_named(self.res, "rec")[0]
        self.assertEqual(rec.kind, "localfn")
        self.assertEqual([r.line for r in rec.refs], [6])

    def test_local_function_expr_is_not_recursive(self):
        self.assertEqual([r.line for r in global_refs(self.res, "f")], [9])

    def test_for_scope(self):
        self.assertEqual([r.line for r in global_refs(self.res, "i")], [13])
        self.assertEqual([r.line for r in global_refs(self.res, "y")], [13])
        i = decls_named(self.res, "i")[0]
        self.assertEqual((i.kind, [r.line for r in i.refs]), ("for", [11]))

    def test_repeat_until_sees_body(self):
        z = decls_named(self.res, "z")[0]
        self.assertEqual([r.line for r in z.refs], [16])
        self.assertEqual(global_refs(self.res, "z"), [])

    def test_method_self(self):
        selfs = decls_named(self.res, "self")
        self.assertEqual(len(selfs), 1)
        self.assertEqual((selfs[0].kind, selfs[0].line, [r.line for r in selfs[0].refs]), ("self", 18, [19]))
        b = decls_named(self.res, "b")
        self.assertEqual(b[0].kind, "param")

    def test_inner_block_shadow(self):
        inner = [d for d in decls_named(self.res, "x") if d.line == 23][0]
        self.assertEqual(inner.refs, [])

    def test_global_write_and_compound(self):
        w = global_refs(self.res, "undeclaredGlobal")
        self.assertEqual([(r.line, r.write) for r in w], [(27, True)])
        c = global_refs(self.res, "count")
        self.assertEqual(sorted((r.line, r.write) for r in c), [(35, False), (35, True)])

    def test_multi_local_rhs_is_outer(self):
        self.assertEqual(sorted(r.line for r in global_refs(self.res, "a")), [36])
        self.assertEqual(sorted(r.line for r in global_refs(self.res, "b")), [36])
        a = [d for d in decls_named(self.res, "a") if d.kind == "local"][0]
        self.assertEqual([r.line for r in a.refs], [37])

    def test_table_keys_are_not_refs(self):
        t = luaast.parse("local t = { a = a, [b] = c }\nreturn t.a")
        r = luaast.resolve(t)
        self.assertEqual(sorted(n for n, _ in r.globals), ["a", "b", "c"])

    def test_string_value(self):
        s = decls_named(self.res, "s")[0]
        self.assertEqual(s.init.left.value, "long ]] string\n")
        self.assertEqual(s.init.right.left.value, "AAB")
        self.assertEqual(s.init.right.right.value, "'q'")


if __name__ == "__main__":
    unittest.main()
