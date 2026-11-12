# CCR-0103: U21 build stats seams (hooks, SetDisplayStat owner, minionList, re-exports)

**Status: applied (CCRDOC, 2026-11-12; code `977648e` Merge port/U21): §1, §2 (with nothing registered, RefreshStatList calls U20's `loadouts::estimate_player_progress`, CCR-0106 §4) and §6. Rejected because they were not built: §3 (the schema keeps `minionList: Vec<Name>`; `calc_active_skill::minionList_is_set` is the accessor), §4 (the re-exports; ui doc §10.2) and §5 (U21 keeps its own section list, DIVERGENCE(section-list)).**

Unit: **U21** (Build stats: `Build.lua:1618-2221`, `Modules/BuildDisplayStats.lua`). Filed status:
**pending** (code on `port/U21` follows it). Number reserved with `scripts/port/next_ccr.sh`.

## 1. U21's entries in `init::default_hooks` and the save-stats hook (CCR-0098 §3, CCR-0080 §8)

Applied, as CCR-0098 §3 proposes (one line each in U19's `pob-app/src/build/init.rs`):

```diff
-        refresh_stat_list: Rc::new(|_| {}),
+        // U21 (CCR-0098 §3)
+        refresh_stat_list: Rc::new(super::stats::refresh_stat_list),
 ...
     install_title_hook();
+    // U21: Build:Save's display stats (CCR-0080 §8)
+    super::stats::install_save_stats_hook();
```

`stats::save_stats` (Build.lua:1033-1080, the display-stat part of BM-A's `@pob 1017-1098`
segment; no second `@pob` marker) writes `PlayerStat` / `FullDPSSkill` / `MinionStat`.
DIVERGENCE(no-env): with no `calcsTab.mainEnv` it writes nothing; PoB never saves before Init's
first BuildOutput, but `LuaCalcBridge` serialises the model before its first calc.

Consequence for U19's test: `tests/u19_build.rs` stubs `RefreshStatList` on its Lua side, so its
Rust side now sets `h.refresh_stat_list = Rc::new(|_| {})` explicitly (one line, applied).

`SetDisplayStat` / `ClearDisplayStat` (Build.lua:1353-1391) stay in `BuildHooks` with their
`not_ported` defaults: CCR-0098 §3 labels them U21, but the lines are in U20's range (ui doc §12.5
U20 row: "display stat"). U20 ports them and calls `stats::get_sidebar_breakdown` (L1371).
**Proposed:** fix the owner label in CCR-0098 §3 / `init.rs` to U20.

## 2. `EstimatePlayerProgress` (U20) from RefreshStatList (Build.lua:2118)

RefreshStatList's last statement is `self:EstimatePlayerProgress()`, a U20 method that has no
Rust signature yet and no `BuildHooks` entry (BuildHooks lists only the calls U19's lines make).
Applied (U21's file): `stats::set_estimate_player_progress(Option<Rc<dyn Fn(&mut Cx)>>)`, a
thread-local hook that RefreshStatList calls; unset, nothing runs. **Proposed:** U20 registers
`loadouts::estimate_player_progress` there (or replaces the call with a direct one and removes
the hook).

## 3. `grantedEffect.minionList`: nil vs `{ }` (pob-gamedata schema)

Evidence: Build.lua:1678 `(activeEffect.grantedEffect.minionList or activeSkill.minionList[1])`
and :1690 `minionList and not minionList[1]` tell nil from `{ }` (Raise Spectre); the schema has
`#[serde(default)] pub minionList: Vec<Name>` (`pob-gamedata/src/schema/skills.rs`), which loses
the difference. Work-around (as calc:AS, CCR-0040): `skill_select.rs` calls
`pob_calc::modules::calc_active_skill::minionList_is_set` directly (not re-exported by
`model.rs`). **Proposed:** `minionList: Option<Vec<Name>>` in the schema, or re-export the
helper from `pob-app/src/model.rs`.

## 4. `model.rs` (U15) lacks re-exports

`pob-app/src/model.rs` re-exports `Env`/`EnvRef` but not the types their fields hold:
`pob_calc::actor::{Actor, ActorRef}` (`env.player`, `env.minion`) and
`pob_calc::active_skill::ActiveSkillRef` (`actor.mainSkill`, `socketGroup.displaySkillList`).
Work-around: `stats.rs` / `skill_select.rs` name them through `pob_calc` directly (marked
CCR-0103 §4). **Proposed:**

```diff
+pub use pob_calc::active_skill::{ActiveSkill, ActiveSkillRef, SkillEffect, SkillEffectRef};
+pub use pob_calc::actor::{Actor, ActorRef};
+pub use pob_calc::modules::calc_active_skill::minionList_is_set;
```

## 5. BuildBreakdownIndex without the CalcsTab UI (U39)

Evidence: Build.lua:1769 reads `self.calcsTab.sectionList`, the UI `CalcSectionControl`s
(CalcsTab.lua:45-155, U39); the model's `CalcsTab::sectionList` (BM-A) keeps only ids and
collapsed flags. Work-around (`stats.rs`, DIVERGENCE(section-list)): the index is built from
the same data, the SkillSelect section's two breakdown rows (CalcsTab.lua:142-144) followed by
`data().calcSections` converted to calc tables. **Proposed:** once U39 lands, it exposes the
section list (`subSection[i].data` as calc tables) and `build_breakdown_index` reads it.

## 6. Stat-box row payload (for U20's SetDisplayStat)

The stat box rows carry `stats::StatLine` in `TextListRow::extra` (`breakdown`, `modNames`,
`actorName`, `ignoredSections`, `breakdownColour`); `hoveredLine.line.extra` downcasts to it.
`get_sidebar_breakdown` returns the display data as a calc `Table` (a list of cell tables), the
value SetDisplayStat stores in `self.sidebarBreakdownData` and passes to
`CalcBreakdownControl:SetBreakdownData` (U40).
