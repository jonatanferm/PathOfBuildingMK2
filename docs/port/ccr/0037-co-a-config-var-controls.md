# CCR-0037: CO-A — `ConfigTab.varControls` is never populated

**Status: proposed (unit CO-A).** No frozen signature changes. CO-A's applies read and write
the model's control-state tables as Lua does; the gap is who creates those tables.

## Evidence

Two `apply` closures in CO-A's range touch `build.configTab.varControls`:

- `mapAffixDropDownFunction` (Modules/ConfigOptions.lua:124, :126; used by the eight map affix
  options, `ConfigOptions[278..286].apply`) reads
  `build.configTab.varControls['multiplierMapModTier'].selIndex`.
- `enemySizePreset` (Modules/ConfigOptions.lua:789-798) calls
  `build.configTab.varControls['enemyRadius']:SetPlaceholder(n, false)`
  (EditControl.lua:111-116: `self.placeholder = tostring(text)`).

In PoB these controls always exist: the ConfigTab constructor makes `self.varControls = { }`
(Classes/ConfigTab.lua:153) and stores one control per option with a `var`
(ConfigTab.lua:672-680: `self.varControls[varData.var] = control`, with `state`, `placeholder`
and, for lists, `selIndex = varData.defaultIndex`).

The Rust model (`crates/pob-calc/src/model/mod.rs:180`, `ConfigTab.varControls: Table`) starts
empty (`Default`, model/mod.rs:194), and nothing in pob-calc inserts into it
(`grep varControls crates/pob-calc/src` shows only the field, its default and the
`UpdateControls` loop, config_tab.rs:596). `apply_a` follows Lua: a missing control raises
Lua's index error (`attempt to index a nil value`). So once BuildModList (CO-C) runs, a build
that sets `enemySizePreset`, or selects a `list`/`count` map affix, raises in Rust where PoB does
not.

## Proposed change (owner: BM-B `config_tab.rs`, or U0 for the `Default` impl)

When the model `ConfigTab` is constructed (the `Build::new` path that already models
ConfigTab.lua:145-148 for config set 1, CCR-0019 §10), also model ConfigTab.lua:672-680 for
every `data.configOptions` entry with a `var`: insert a control-state table

```diff
+ for varData in &data.configOptions { let Some(var) = varData.var else { continue };
+     let control = Table::new();
+     control.set(n!("_className"), <EditControl | CheckBoxControl | DropDownControl by varData.type>);
+     control.set(n!("state"), varData.defaultState);
+     control.set(n!("placeholder"), varData.defaultPlaceholderState);
+     if let Some(i) = varData.defaultIndex { control.set(n!("selIndex"), i); control.set(n!("list"), …) }
+     self.varControls.set(var, control);
+ }
```

so that `UpdateControls` (config_tab.rs:586) and the CO-A applies find the same tables PoB has.
The CO-A acceptance test (`crates/pob-oracle/tests/CO-A.rs`) creates the two controls it needs
itself (`multiplierMapModTier.selIndex = 2`, `enemyRadius`), matching the probe's PoB state.
