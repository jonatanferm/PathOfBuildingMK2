# CCR-0108: U29 gem select seams (StatDescriber has no owner, `itemLib.wiki`, bridge notes)

**Status: applied (CCRDOC, 2026-11-12; code `6e06682` Merge port/U29). The ui doc U29 row and §13 item 15 record `Modules/StatDescriber.lua` as U29's (`controls/gem_tooltip.rs::stat_describer`, also called by U27) and the `itemLib.wiki` helpers in `controls/gem_select.rs`. §3 is informational.**

Unit: **U29** (GemSelectControl `Classes/GemSelectControl.lua:1-854`, `Classes/GemTooltip.lua:1-261`).
Filed status: **pending** (code on `port/U29` follows it). Number reserved with `scripts/port/next_ccr.sh`.

## 1. `Modules/StatDescriber.lua` (`data.describeStats`) has no owner

Evidence: `GemTooltip.lua:172-204` describes the gem's stats with `build.data.describeStats(stats,
grantedEffect.statDescriptionScope)`, which is `LoadModule("Modules/StatDescriber")`
(`Data.lua:646`). `TreeTab.lua:2007` calls it too. Neither unit table (calc doc §11, ui doc §12)
names `StatDescriber.lua` (grep of both docs: no hit), `docs/port/scope.toml` does not list it,
and nothing in `crates/` ports it (the dumped `StatDescriptions.<scope>` roots sit untyped in
`pob_gamedata::Data::statDescriptions`, whose doc comment says "the stat describer (UI / item
tooltips) types them"). Without it the gem tooltip loses every stat line.

Worked around (U29's file): the whole module (L1-299: `getScope` with the scope cache and
`scopeList`, `matchLimit`, `applySpecial` with all 47 description functions and the
`Data/PearlSupports` lookup, the describer itself) is ported as
`pob_app::controls::gem_tooltip::stat_describer` (`describe_stats(cx, stats, scope, quality)`,
`get_scope`, `forget_scopes`), with `@pob` markers. `ConPrintf` of an unknown description
function (L203; the data has 10 such names, e.g. `old_leech_percent`) goes to
`cx.sys.platform.con_print`. Verified through GemTooltip: `u29_gem_tooltips` compares 1771
tooltips per fixture (every gem of `data.gems` at two level/quality pairs plus the build's gems,
about 37 000 lines) with PoB's.

Proposed: record the module under U29 (ui doc §12 U29 row: "Owns … `controls/gem_tooltip.rs`
(incl. `Modules/StatDescriber.lua`)"), add `Modules/StatDescriber.lua` 1-299 to the ui doc's
port list, and have the TreeTab unit (TreeTab.lua:2007) call
`crate::controls::gem_tooltip::stat_describer::describe_stats`. A later move to its own file
(`pob-app/src/stat_describer.rs`) needs a UI-U0-style module declaration, so it is left to the
orchestrator.

## 2. `itemLib.wiki` (`ItemTools.lua:384-413`) is left to "the UI port", which has no row for it

Evidence: `crates/pob-calc/src/classes/item_tools.rs:979` skips the range with
`reason="itemLib.wiki: … UI only, the UI port owns it"`; no ui doc row names it.
GemSelectControl.lua:833-849 (F1 opens the wiki for the hovered / selected gem) needs
`itemLib.wiki.matchesKey`, `openGem` and `open`; ItemsTab/ItemSlot/ItemDB need `openItem`.
`main.itemLib_wiki_triggered` already exists (`MainState::item_lib_wiki_triggered`, read by
Main's OnFrame).

Worked around: `pob_app::controls::gem_select::{wiki_open_gem, wiki_open, wiki_matches_key}`
port L385-412 except `openItem` (carrying `lua:` doc lines but no second `@pob` marker, so the
coverage of the range stays IT-B's skip). Proposed: the items unit that needs `openItem` adds it
next to these (or the orchestrator gives the four functions a file of their own).

## 3. LuaCalcBridge notes (U15; informational, no contract change)

a. `ProcessSocketGroup` of an item-granted, `triggered` group writes `levels[level].cost = {}`
   into PoB's data (SkillsTab.lua:1188, `skills_tab::levelCostCleared`; CalcSetup runs it for the
   groups it creates). Through the bridge that write happens in the bridge's VM only, so after a
   bridge `build_output` the Rust process still sees the cost: the gem tooltip of such a skill
   shows a "Cost:" line PoB does not (allflame_exp_001: Despair level 1, granted by "Curse Enemies
   with Despair on Hit"). `u29_gem_tooltips` replays `ProcessSocketGroup` over the build's groups
   before comparing. A bridge fix would replay it in `adopt_build_output` (or dump the cleared
   set). The Rust calc (`RustCalc`) writes the set in-process and is not affected.
b. `CalcOutputWithThisGem` on an occupied slot changes only the instance's `gemData`, `level` and
   `displayEffect`; the bridge serialises the model, and BM-A's Save writes `gemId` from
   `gemData.gameId` but `nameSpec`/`skillId` from the instance, which the bridge VM's
   `LoadSkill` then resolves (variant choice by `skillId`). The DPS acceptance therefore uses the
   empty slot (`index = #gemList + 1`), where the instance is created whole (L82-92) and the
   bridge round trip is exact.
