# CCR-0144: U50 CompareTab E seams (the entry's group-list icons, nested breakdowns in GetBreakdownLines, the bridge's isSupporting keys)

Unit: **U50** (`Classes/CompareTab.lua:4049-5036`; file `pob-app/src/tabs/compare/draw_b.rs`).
Status: **pending** (code on `port/U50` follows it, with the local workaround marked `// CCR-0144`).
Number reserved with `scripts/port/next_ccr.sh`.

No frozen contract changes. One local workaround for a private method of another unit, one bug
fix applied in U48's file (two lines plus a helper), one test-harness fix in U15's bridge, and the
one-line hook installs CCR-0137 §3 asks for.

## 1. The comparison build's `skillsTab.controls.groupList:GetRowIcon` (U30/U31)

Evidence: DrawSkills' `getGroupSlotIcon(skillsTab, idx, group)` (CompareTab.lua:4330-4336) calls
`skillsTab.controls.groupList:GetRowIcon(1, idx, group)` for the primary build's and the
comparison build's socket groups (`drawGroupHeader`, :4345-4353, called at :4476 and :4479). In
PoB the CompareEntry builds a full SkillsTab (CompareEntry.lua:115), so its group list is a
SkillListControl whose `GetRowIcon` (SkillListControl.lua:241-267) reads the *entry's*
`itemsTab` (`self.skillsTab.build.itemsTab`) and the module's `slot_map` icons. The port's entry
has the model half only (CCR-0137 §2); U30's `SkillListControl::GetRowIcon`
(`tabs/skills/skill_list.rs:617`) reads `cx.build()` (the primary build) and its `slot_map`
handles (`SLOT_ICONS`, `slot_icon`) are private.

Workaround (`draw_b.rs`, marked `// CCR-0144`): the primary build's icon goes through the real
SkillListControl's ListControl hook (`ov.get_row_icon`, else `hooks.get_row_icon`). For the
comparison build, `entry_row_icon` repeats SkillListControl.lua:241-267 on the entry's model
(statement for statement as U30's port, no `@pob` marker, the range stays U30's), with this
file's own handles for `slot_map`'s 17 paths, loaded on first use. Observable difference: none in
the draw trace (images are identified by path); the ImageLoad effects of those 17 paths come at
the first DrawSkills instead of at the entry's construction (PoB reloads the shared handles in
every SkillListControl constructor, SkillListControl.lua:73-75).

Proposed (U30's file): `pub fn row_icon_for(cx: &mut Cx, build: &model::BuildRef, socketGroup:
&SocketGroupRef) -> Option<ImageHandle>` (the GetRowIcon body with the build as a parameter;
`GetRowIcon` calls it with `cx.build().model`), and U50 calls it for both builds and drops
`SLOT_MAP`/`SLOT_ICONS`/`slot_icon`/`entry_row_icon`.

## 2. `GetBreakdownLines` and the nested breakdown instances (U48), applied

Evidence: CompareCalcsHelpers.lua:153 `breakdown = calcsActor.breakdown[ns] and
calcsActor.breakdown[ns][name]` with `ns` = `MainHand`/`OffHand` (CalcSections.lua `breakdown =
"MainHand.…"` cells). `breakdown.MainHand`/`.OffHand` are breakdown module instances
(CalcOffence.lua:2006/2023), plain tables in Lua; the calc stores them as `Val::Obj(Breakdown)`
(costs_passes.rs `newSubBreakdown`; `build/stats.rs:705-715` `bd_val` documents and handles the
same case). U48's port (`calcs_helpers.rs:404-405`) indexed the `Val::Obj` with `Val::get`, which
raises "attempt to index a userdata value". DrawCalcs reaches it through
`calcRowMatchesBetweenBuilds` (CompareTab.lua:4664-4665, with "show only differences" on, the
default) on every frame of the Calcs view of a build with a weapon, and the hover tooltip
(CompareCalcsHelpers.lua:203-204) on every `MainHand.*` row: the Calcs view raised the error
prompt on its first frame.

Applied (U48's `calcs_helpers.rs`, marked `CCR-0144 §2`): a private `bd_val` (the
`build::stats` helper) unwraps the nested instance to its data table before the `[name]` index.
Two changed lines and the helper; no other behaviour changes.

## 3. LuaCalcBridge: `supportEffect.isSupporting` keys (U15, test harness), applied

Evidence: DrawSkills' `checkSupporting` (CompareTab.lua:4177-4185) tests
`gemA.supportEffect.isSupporting[gemB]` with the live socket-group gems (GemSelectControl's
`CheckSupporting` does the same). CalcSetup keys `isSupporting` with the group's gem instances;
the bridge (`pob-ui-oracle/src/lua_calc.rs`, `Rebind::effect`) re-pointed each injected skill
effect's `srcInstance` at the live gem but left the `isSupporting` set holding the injected
copies, so no identity test succeeded on the Rust side of a parity run (the hover highlight of
supported gems never drew: 40-44 divergences per u50 skills run).

Applied (U15's `lua_calc.rs`, `Rebind::effect`): the set's members are re-pointed at the live gems
through the same injected → live map. Dev-only code (no product change); it also makes
`GemSelectControl::check_supporting` see PoB's relation under the bridge.

## 4. The hook installs (CCR-0137 §3)

`layout.rs` `default_draw_hooks`: `draw_skills: Rc::new(CompareTab::DrawSkills)`, `draw_calcs:
Rc::new(CompareTab::DrawCalcs)`, `draw_config: Rc::new(CompareTab::DrawConfig)` (three lines).

## 5. Notes

* `self.calcSections` (CompareTab.lua:192, `LoadModule("Modules/CalcSections")`): U46's
  constructor notes it as U50's read; U50 keeps it in `draw_b::State` (U39's
  `sections_data::load`, fresh tables per tab), loaded on the first DrawCalcs. The module is pure
  data, so the load time is not observable.
* `self.calcsSkillHeaderHover` is `draw_b::State` (only U50's functions read and write it). Like
  the Lua field it survives frames and view switches until the next header pass resets it.
* `self.calcsTooltip` (U46's `init::State`) is lent to U48's `DrawCalcsTooltip` for the call.
* U48's tooltip sorts the modifier rows with Lua 5.1's unstable sort (its ORDER note): rows tied
  on the value keep the order of `Tabulate`, which follows the mod lists' insertion order of the
  calc that built them. Under the bridge that calc is a second LuaJIT PoB, whose order for tied
  rows can differ from the oracle's (u50 c_calcs at 1280 hovered "Total Increased", whose ten
  `+10%` rows came out in another order on the Rust side). The u50 scenarios hover rows without
  such ties. Most likely not a U50 or U48 code difference (the Rust side's tooltip reads the mod
  lists the bridge's PoB built, the Lua side's those of the oracle's own PoB), but this was not
  traced to the mod insertion that differs.
