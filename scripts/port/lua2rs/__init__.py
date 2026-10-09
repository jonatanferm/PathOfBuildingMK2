"""lua2rs: Lua -> Rust transpiler for the table-shaped parts of PoB (calc doc unit T1).

Inputs: ModParser.lua's pattern lists (formList, modNameList, modFlagList, preFlagList,
modTagList, specialModList) and ConfigOptions.lua's `apply` closures. Output: Rust in the
shapes of calc doc §6 (ModParser contract) and §8.1 (closure registries), with the
provenance markers of §3.1 rule 5. The Rust spellings are configurable (`shape.py`,
`--shape FILE.json`) because U0 has not frozen the types yet.

Run `python3 scripts/port/lua2rs --help`.
"""
