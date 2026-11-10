# CCR-0038: CO-B — control methods on `ConfigTab.varControls` (SetPlaceholder, SelByValue)

**Status: applied 2026-11-10 by ORC-X2 (`ConfigTab::{varControl, SetPlaceholder, SelByValue}` in `model/config_tab.rs`; `apply_b.rs` delegates to them, `apply_a.rs` `enemyRadius` uses `SetPlaceholder`). See AMENDMENTS A9.**

**Filed status: proposed (unit CO-B).** No frozen signature changes. Builds on CCR-0037 (CO-A: the
model never populates `varControls`). CO-B works around both locally in
`crates/pob-calc/src/modules/config_options/apply_b.rs` (helpers `varControl`,
`SetPlaceholder`, `SelByValue`, lines 79-265).

## Evidence

Two `apply` closures in CO-B's range drive UI controls rather than only adding mods:

- `enemyIsBoss` (Modules/ConfigOptions.lua:2086-2222, `ConfigOptions[659].apply`) calls
  `build.configTab.varControls[<var>]:SetPlaceholder(v, true)` 65 times (19 controls) and
  `build.configTab:UpdateLevel()` between them.
- `presetBossSkills` (Modules/ConfigOptions.lua:2272-2351, `ConfigOptions[670].apply`) calls
  `SetPlaceholder(v, true)` on 11 named controls plus dynamically named ones
  (`'enemy'..damageType..'Damage'`, `'enemy'..penType`), reads
  `varControls['enemyIsBoss'].list[varControls['enemyIsBoss'].selIndex].val` and
  `varControls['enemyDamageRollRange'].placeholder`, calls
  `varControls['enemyDamageType']:SelByValue(x, "val")` and writes
  `varControls['enemyDamageType'].enabled`.

`SetPlaceholder(text, true)` is not a plain field write: Classes/EditControl.lua:111-116 sets
`self.placeholder = tostring(text)` and then calls `self.changeFunc(self.placeholder, true)`,
and the `changeFunc` the ConfigTab constructor installs (Classes/ConfigTab.lua:305-314 for
count/integer/countAllowZero/float, 323-343 for text) writes
`configSets[activeConfigSetId].placeholder[var] = tonumber(buf)` (`tostring(buf)` for text) and
sets `build.buildFlag = true`. That write is what `ConfigTab:UpdateLevel` (ConfigTab.lua:1118-1128)
and BuildModList's placeholder branch (ConfigTab.lua:1147-1148) read later, so the round trip
`number → tostring → tonumber` is observable (`SetPlaceholder("", true)` clears the entry).
`SelByValue` is Classes/DropDownControl.lua:136-150.

The model's control-state tables (`ConfigTab.varControls: Table`, crates/pob-calc/src/model/mod.rs:180)
cannot hold methods, and the model has no notion of which `changeFunc` a control has.

## Proposed change (owner: BM-B `model/config_tab.rs`)

Together with CCR-0037's constructor population (which should also store `list = varData.list`
for `DropDownControl`s and `enabled = true`, Control.lua:60), add model methods that the apply
closures (CO-A, CO-B) and the UI port call instead of local helpers:

```diff
+impl ConfigTab {
+    /// `self.varControls[var]` (nil → Lua index error).
+    pub fn varControl(&self, var: Name) -> Table;
+    /// `self.varControls[var]:SetPlaceholder(text, notify)`: EditControl.lua:111-116 plus the
+    /// constructor's changeFunc (ConfigTab.lua:305-314 / 323-343) chosen by the option's type.
+    pub fn SetPlaceholder(&self, build: &BuildRef, var: Name, text: &Val, notify: bool);
+    /// `self.varControls[var]:SelByValue(value, key)` (DropDownControl.lua:136-150).
+    pub fn SelByValue(&self, var: Name, value: &Val, key: Name);
+}
```

Once these exist, CO-B's `varControl` / `SetPlaceholder` / `SelByValue` in apply_b.rs become
thin calls to them (behaviour already verified against PoB by `crates/pob-oracle/tests/CO-B.rs`,
which builds the control-state tables itself from PoB's state until CCR-0037 lands).
