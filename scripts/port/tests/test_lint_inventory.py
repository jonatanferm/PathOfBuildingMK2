import contextlib
import io
import os
import shutil
import tempfile
import unittest

import _util
import calcscope
import globals_lint
import luaast
import pairs_inventory


def mini_pob(files):
    root = tempfile.mkdtemp(prefix="p4-pob-")
    for rel, text in files.items():
        full = os.path.join(root, rel)
        os.makedirs(os.path.dirname(full), exist_ok=True)
        with open(full, "w") as fh:
            fh.write(text)
    return root


class GlobalsLintFixtureTest(unittest.TestCase):
    def setUp(self):
        self.root = mini_pob({
            "src/Modules/Defs.lua": (
                "definedGlobal = 1\n"
                "function globalFn() end\n"
                "_G.viaG = 1\n"
                'rawset(_G, "viaRawset", 1)\n'
                "local function f() leakedLater = 2 end\n"),
            "src/Modules/Calc.lua": (
                "local localName = 1\n"                                   # 1
                "local function calc(cfg)\n"                              # 2
                "\tlocal x = definedGlobal + globalFn() + viaG + viaRawset\n"  # 3
                "\tlocal y = string.format('%d', cfg) .. tostring(localName)\n"  # 4
                "\tif x then local dotCfg = 1 end\n"                       # 5
                "\treturn dotCfg, missingGlobal, leakedLater\n"            # 6
                "end\n"                                                    # 7
                "function topLevelFn() implicitOne = 3; return implicitOne end\n"),  # 8
            "src/Export/Skip.lua": "missingGlobal = 1\n",   # Export/ is not a runtime file
            "src/TreeData/x/tree.lua": "return {}\n",
        })
        self.scope = calcscope.Scope([("Modules/Calc.lua", [(1, 8)], "")], "test")

    def tearDown(self):
        shutil.rmtree(self.root)

    def test_reads_and_implicit_globals(self):
        reads, implicit, implicit_reads = globals_lint.lint(self.root, self.scope)
        self.assertEqual([(r["line"], r["name"]) for r in reads], [(6, "dotCfg"), (6, "missingGlobal")])
        self.assertEqual(reads[0]["function"], "calc")
        self.assertEqual([(r["line"], r["name"]) for r in implicit], [(8, "implicitOne")])
        self.assertEqual([(r["line"], r["name"]) for r in implicit_reads], [(8, "implicitOne")])

    def test_runtime_files_exclude_export_and_treedata(self):
        rels = sorted(globals_lint.src_rel(self.root, f) for f in globals_lint.runtime_files(self.root))
        self.assertEqual(rels, ["Modules/Calc.lua", "Modules/Defs.lua"])


@_util.needs_pob
class GlobalsLintPinTest(unittest.TestCase):
    """Acceptance: globals_lint reports exactly the calc doc §7.3 sites (CCR-0004 applied)."""

    def test_check(self):
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            rc = globals_lint.main(["--check"])
        self.assertEqual(rc, 0, buf.getvalue())
        self.assertIn("all 15 calc doc §7.3 sites reported", buf.getvalue())

    def test_doc_sites_parsed_from_doc(self):
        doc = os.path.join(_util.pobpaths.docs_port(), "calc-architecture.md")
        self.assertEqual(globals_lint.doc_sites(doc, _util.real_pob()), globals_lint.KNOWN_DOC_SITES)

    def test_check_follows_doc_edits(self):
        doc = os.path.join(_util.pobpaths.docs_port(), "calc-architecture.md")
        with open(doc, encoding="utf-8") as fh:
            text = fh.read()
        tmp = tempfile.mkdtemp(prefix="p4-doc-")
        try:
            edited = os.path.join(tmp, "calc.md")
            # Drop a listed site: --check must now report it as unexpected.
            with open(edited, "w", encoding="utf-8") as fh:
                fh.write(text.replace("`dotCfg` (CalcOffence 3527, 3538)", "`dotCfg` (CalcOffence 3527)"))
            self.assertNotIn(("Modules/CalcOffence.lua", 3538, "dotCfg"),
                             globals_lint.doc_sites(edited, _util.real_pob()))
            buf = io.StringIO()
            with contextlib.redirect_stdout(buf):
                rc = globals_lint.main(["--check", "--doc", edited])
            self.assertEqual(rc, 1, buf.getvalue())
            self.assertIn("not listed in §7.3: Modules/CalcOffence.lua:3538 dotCfg", buf.getvalue())
        finally:
            shutil.rmtree(tmp)

    def test_doc_parser_forms(self):
        """Path-style refs, `;`-separated modules, and remarks after a site list."""
        tmp = tempfile.mkdtemp(prefix="p4-doc-")
        try:
            edited = os.path.join(tmp, "calc.md")
            with open(edited, "w", encoding="utf-8") as fh:
                fh.write("### 7.3 Globals\n\n- The undeclared-global read sites are: "
                         "`a` (CalcPerform 915; CalcBreakdown 212, 232), "
                         "`b` (CalcDefence 1606, so `nil or 1` takes 2), "
                         "`c` (Data/Skills/act_dex.lua:12991, Scourge Arrow 3), "
                         "`d` (CalcPerform 3464; `pairs(nil)` raises at 7), "
                         "`e` (ModDB.lua:262, ModList.lua:207).\n\n## 8. Next\n")
            self.assertEqual(globals_lint.doc_sites(edited, _util.real_pob()), {
                ("Modules/CalcPerform.lua", 915, "a"), ("Modules/CalcBreakdown.lua", 212, "a"),
                ("Modules/CalcBreakdown.lua", 232, "a"), ("Modules/CalcDefence.lua", 1606, "b"),
                ("Data/Skills/act_dex.lua", 12991, "c"), ("Modules/CalcPerform.lua", 3464, "d"),
                ("Classes/ModDB.lua", 262, "e"), ("Classes/ModList.lua", 207, "e"),
            })
        finally:
            shutil.rmtree(tmp)


class PairsInventoryFixtureTest(unittest.TestCase):
    def test_fixture_sites(self):
        chunk = luaast.parse_file(_util.fixture("pairs_sites.lua"))
        res = luaast.resolve(chunk)
        scope = calcscope.Scope([("pairs_sites.lua", [(1, 100)], "")], "test")
        sites, next_calls = pairs_inventory.find_sites("pairs_sites.lua", chunk, res, scope)
        got = [(s.line, s.via, s.cls, s.shape) for s in sites]
        self.assertEqual(got, [
            (4, "pairs", "int", "map"),          # spec.allocNodes (node-id map)
            (5, "pairs", "int", "list"),         # positional constructor
            (6, "pairs", "string", ""),          # named constructor
            (10, "pairs", "identity", ""),       # obj.name
            (11, "pairs", "string", ""),         # name:match()
            (12, "pairs", "int", ""),            # i + 1
            (13, "next-for", None, ""),          # unknown table via next
            (16, "pairs", "int", "list"),        # t_insert(list, ...)
            (17, "pairs", None, ""),             # nothing known
        ])
        got = sorted((c.line, c.text, c.use) for c in next_calls)
        self.assertEqual(got, [
            (18, "next(list)", "empty-test"),                  # if next(t)
            (19, "next(spec.allocNodes)", "first-entry"),       # first key: order-dependent
            (20, "next(seen)", "first-entry"),                  # first value: order-dependent
            (21, "next(list)", "empty-test"),                   # not next(t)
            (21, "next(seen)", "empty-test"),                   # next(t) == nil
            (22, "next(seen, key)", "step"),
            (23, "next(seen)", "empty-test"),                   # next(t) and x
            (24, "next(seen)", "empty-test"),                   # assert(next(t), msg)
            (25, "next(seen)", "first-entry"),                  # next(t) or default
        ])

    def test_decide(self):
        self.assertEqual(pairs_inventory.decide([("int", ""), ("string", "")]), "mixed")
        self.assertEqual(pairs_inventory.decide([("string?", "")]), "string")
        self.assertEqual(pairs_inventory.decide([("int", ""), ("string?", "")]), "int")
        self.assertIsNone(pairs_inventory.decide([("int", ""), ("identity", "")]))
        self.assertIsNone(pairs_inventory.decide([]))


@_util.needs_pob
class PairsInventoryPinTest(unittest.TestCase):
    """Acceptance: every pairs() site in scope is classified int/string/identity (or mixed/generic)."""

    def test_check(self):
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            rc = pairs_inventory.main(["--check"])
        self.assertEqual(rc, 0, buf.getvalue())

    def test_known_node_maps(self):
        pairs_inventory._register_overrides() if not pairs_inventory.OVERRIDES else None
        # docs/port/scope.toml is authoritative (calcscope.py docstring); DEFAULT_SCOPE is only the
        # P4 seed, so overrides for ranges added later (U0 corrections, CCR2) would read as stale.
        scope = calcscope.load_scope(os.path.join(_util.pobpaths.docs_port(), "scope.toml"),
                                     calcscope.default_closures_json(_util.pobpaths.main_checkout()),
                                     required=True)
        sites, _, stale = pairs_inventory.inventory(_util.real_pob(), scope)
        self.assertEqual(stale, [])
        by = {(s.path, s.line): s for s in sites}
        # calc doc §3.4 names these node-id maps
        for key in [("Modules/CalcSetup.lua", 284), ("Modules/CalcSetup.lua", 747), ("Modules/CalcSetup.lua", 1439),
                    ("Modules/CalcSetup.lua", 273), ("Modules/CalcSetup.lua", 898)]:
            self.assertEqual(by[key].cls, "int", key)
        self.assertEqual(by[("Modules/CalcPerform.lua", 3464)].cls, "identity")  # pairs(affectedByAura)


if __name__ == "__main__":
    unittest.main()
