import io
import os
import contextlib
import unittest

import _util
import locals_audit
import segments


def audit_fixture(header, end, starts, loops=()):
    chunk, res = segments.load(_util.FIXTURES, "segfunc.lua")
    sf = segments.SplitFunction("fx", "segfunc.lua", "M.fx", header, end, starts, loops)
    return sf, locals_audit.audit_function(sf, chunk, res), chunk, res


class LocalsAuditFixtureTest(unittest.TestCase):
    def test_crossing_locals_and_loop(self):
        sf, a, _, _ = audit_fixture(2, 19, [2, 9, 11, 17], [segments.Loop("loop", 11, 16, [14])])
        self.assertEqual([(d.name, d.line) for d in a.params], [("p1", 2), ("p2", 2), ("unused", 2)])
        self.assertEqual([d.name for d in a.uncrossed_params], ["p1", "unused"])
        self.assertEqual([(d.name, d.line) for d in a.top], [("b", 4), ("helper", 5), ("c", 9), ("d", 10)])
        self.assertEqual([(d.name, d.line) for d in a.loops[11]], [("e", 12)])
        self.assertEqual(a.other, [])
        self.assertEqual(locals_audit.render_decls(a.top, a.params),
                         "params `p1 p2 unused`; `b`4 `helper`5 (fn) `c`9 `d`10")
        rows = locals_audit.table_rows([a])
        self.assertEqual(rows[1], "| loop body 11-16, split at 14 | | `e`12 |")
        # `a` is used only inside helper, which is defined in the same segment: not crossing
        self.assertNotIn("a", [d.name for d in a.top])

    def test_shadowed_param(self):
        sf, a, _, _ = audit_fixture(20, 25, [20, 23])
        self.assertEqual(locals_audit.render_decls(a.top, a.params), "params `k`; `o`21 `q`22")

    def test_local_fns_group(self):
        class D:
            def __init__(self, n, l, k):
                self.name, self.line, self.kind = n, l, k
        ds = [D("x", 1, "local"), D("f", 2, "localfn"), D("g", 3, "localfn"), D("h", 4, "localfn"),
              D("y", 5, "local"), D("k", 6, "localfn")]
        self.assertEqual(locals_audit.render_decls(ds),
                         "`x`1; local fns `f`2 `g`3 `h`4; `y`5 `k`6 (fn)")

    def test_boundary_inside_block_is_rejected(self):
        chunk, res = segments.load(_util.FIXTURES, "segfunc.lua")
        sf = segments.SplitFunction("bad", "segfunc.lua", "M.bad", 26, 31, [26, 28],
                                    units=[("X", 26, 31, "")])
        probs = segments.check_boundaries(sf, segments.find_function(res, 26))
        self.assertEqual(len(probs), 1)
        self.assertIn("boundary 28 splits the statement 27-30", probs[0])

    def test_unit_tiling(self):
        chunk, res = segments.load(_util.FIXTURES, "segfunc.lua")
        sf = segments.SplitFunction("fx", "segfunc.lua", "M.big", 2, 19, [2, 9],
                                    units=[("A", 2, 8, ""), ("B", 10, 19, "")])
        probs = segments.check_boundaries(sf, segments.find_function(res, 2))
        self.assertTrue(any("gap/overlap at 9" in p for p in probs), probs)


@_util.needs_pob
class LocalsAuditPinTest(unittest.TestCase):
    """Acceptance: locals_audit reproduces calc doc §7.1 exactly at the pin."""

    def test_reproduces_doc_7_1(self):
        doc = os.path.join(_util.PORT, "..", "..", "docs", "port", "calc-architecture.md")
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            rc = locals_audit.main(["--check", doc])
        self.assertEqual(rc, 0, buf.getvalue())
        self.assertIn("7 rows, 88 crossing locals identical", buf.getvalue())

    def test_segments_check(self):
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            rc = segments.main(["check"])
        self.assertEqual(rc, 0, buf.getvalue())

    def test_stages_draft(self):
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            segments.main(["stages"])
        text = buf.getvalue()
        self.assertIn('id = "offence:3346"', text)
        self.assertIn('id = "initEnv:out"', text)
        # initEnv ends with `return env, ...` at 1974: the out probe goes before it
        i = text.index('id = "initEnv:out"')
        self.assertIn("line = 1974", text[i:i + 200])


if __name__ == "__main__":
    unittest.main()
