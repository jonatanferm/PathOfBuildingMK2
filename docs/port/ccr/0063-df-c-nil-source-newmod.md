# CCR-0063: `ModStore::NewMod` / `ReplaceMod` cannot pass a nil source (DF-C)

**Status: applied 2026-11-10 by ORC-X2 (`NewMod`/`ReplaceMod` take `source: impl ModSourceArg` = any `&T: AsRef<str>` or `Option<&str>`; the local helpers in `calc_defence/recovery.rs`, `calc_perform/buffs.rs`, `calc_setup/init_env_base.rs`, `calc_triggers/{config_table,handlers}.rs` are deleted). See AMENDMENTS A9.**

Requested by: DF-C (CalcDefence.lua 1211-1673, `calc_defence/recovery.rs`).
Filed status: proposed (optional; the unit works around it locally).

## Problem

`ModStore::NewMod` and `ModStore::ReplaceMod` (`crates/pob-calc/src/classes/mod_store.rs`,
ModStore.lua:109-111 / 125-130) take `source: &str` and always call
`mod_tools::createMod(…, Some(source), …)`. In Lua the source argument can be nil, and
`createMod` then leaves `mod.source` nil (ModTools.lua:36-51). Passing `""` instead changes the
mod's `source` field (and so the dumped mod store and any `source:match(...)` filter).

## Lua evidence

- `Modules/CalcDefence.lua:1536` — `modDB:NewMod("ArmourAppliesToPhysicalDamageTaken", "BASE", 100)`
  (no source argument).
- `Modules/CalcDefence.lua:1416-1417` — `local flagMod = modDB:Tabulate("FLAG", nil, addFlag)[1].mod`
  then `modDB:NewMod(recoupType2 .. "Recoup", "BASE", baseRecoup, flagMod.source)`; the flag mod's
  `source` may be nil.
- `Modules/CalcDefence.lua:1484-1485` — the same with `modDB:ReplaceMod(…, flagMod.source)`.

The same gap was noted by PF-E (CalcPerform.lua 2372, 2762; `new_mod_no_source`) and by the
ConfigOptions applies (calc doc §6, "port as `modList.AddMod(Arc::new(createMod(…, None, …)))`").

## Workaround in DF-C (marked `// CCR-0063`)

`recovery.rs` has two private helpers that inline ModStore.lua's bodies with an
`Option<&str>` source: `NewMod_nosource` (`self:AddMod(mod_createMod(name, type, value))`,
ModStore.lua:110) and `NewMod_src` (ModStore.lua:110, or 126-129 for `ReplaceMod`, which calls
the crate-visible `ReplaceModInternal`).

## Proposed diff

```diff
 impl ModStore {
     pub fn NewMod(
         &self,
         name: Name,
         ty: ModType,
         value: impl Into<Value>,
-        source: &str,
+        source: impl Into<Option<&str>>,   // or a separate NewMod_opt(…, source: Option<&str>, …)
         flags: u32,
         keywordFlags: u32,
         tags: &[Tag],
     ) {
-        self.AddMod(Arc::new(mod_tools::createMod(name, ty, value, Some(source), flags, keywordFlags, tags)));
+        self.AddMod(Arc::new(mod_tools::createMod(name, ty, value, source.into(), flags, keywordFlags, tags)));
     }
```

and the same for `ReplaceMod`. Existing `&str` call sites keep compiling (`&str: Into<Option<&str>>`).
Once applied, DF-C's helpers become `modDB.NewMod(…, None, …)` / `modDB.ReplaceMod(…, src, …)`.
