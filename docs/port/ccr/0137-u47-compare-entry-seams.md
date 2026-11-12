# CCR-0137: U47 CompareTab B seams (CompareEntry's tab halves, the entry's ConfigTab controls, the Draw* passes, the power list placeholder)

Unit: **U47** (`Classes/CompareTab.lua:1650-2515`, `Classes/CompareEntry.lua` 1-552; files
`pob-app/src/tabs/compare/{layout.rs,entry.rs}`).
Status: **pending** (code on `port/U47` follows it, with the local workarounds marked `// CCR-0137`).
Number reserved with `scripts/port/next_ccr.sh`.

No frozen contract changes. This CCR records what U47 repeats from other units' private code, one
gap it cannot close, the seam it defines for U49/U50, and two one-line installs in other units' files.

## 1. calc:BM-A's private constructor halves (`build_xml.rs`)

Evidence: CompareEntry.lua:106-116 runs the tab constructors on the entry and :174-176
`configTab:ImportCalcSettings()`; :34-40 sets `timelessData` as Build.lua:81 does. Their model halves
exist in `pob-calc-core/src/model/build_xml.rs` but are private: `skillsTab_ctor_model` (SkillsTab.lua:89,
95-96, 423-426), `ImportCalcSettings` (ConfigTab.lua:1205-1240), `new_timelessData` (Build.lua:81's table).
`ensure_latestTree` is `pub(crate)` (U47 calls `passive_spec::main_LoadTree`, as it does).

Workaround (`entry.rs`, marked `// CCR-0137 §1`): the bodies are repeated (statement for statement,
with `lua:` comments, no `@pob` markers so the coverage of those Lua lines stays single).
SkillsTab.lua:96 reads `main.defaultGemQuality` (U47 passes `MainState::default_gem_quality`; BM-A's copy
reads nil headless).

Proposed (calc:BM-A's file): make the three functions `pub` (the SkillsTab one taking the default gem
quality); U47 then calls them.

## 2. The entry's ConfigTab controls (`entry.configTab.varControls`, U41)

Evidence: CompareTab.lua:1208 `compareEntry.configTab.varControls[varData.var]` is the source of the
comparison column's config controls' `tooltipText`/`tooltipFunc` (:1170-1173). In PoB every entry has a
full ConfigTab (CompareEntry.lua:112). U41's ConfigTab constructor (`tabs/config/ctor.rs`) builds its
controls against the open build (`cx.build()`, 13 sites) and its tooltip closures read that build's
model; it cannot be instantiated for a second build. The same holds for the entry's TreeTab (U25),
ItemsTab (U33), SkillsTab (U30) and CalcsTab (U39) UI halves, which CompareTab does not read except
`treeTab.viewer` (U47 keeps a `PassiveTreeView` per entry), `treeTab:GetSpecList()` and
`treeTab:OpenVersionConvertPopup` (U47 runs TreeTab.lua:492-498, 594-618, 655-672 on the entry's model,
`entry.rs`).

GAP: `CompareHooks::entry_config_var_control` stays `None`: the comparison build's config controls have
no tooltip (PoB shows the option's help and, for options with a `tooltipFunc`, the entry build's stat
comparison). The u47 scenarios do not hover them.

Proposed (U41): a constructor variant `ConfigTab::new_for(build: model::BuildRef, ...)` whose controls
and tooltip closures read the given model, owned by the compare entry (U47 keeps it in
`entry::CompareEntry`); U47 then answers `entry_config_var_control` from it.

## 3. The Draw* passes of U49/U50 (`layout::CompareDrawHooks`)

Evidence: CompareTab.lua:1754, 1837-1845, 1851 call `DrawTree`, `DrawSummary`, `DrawItems`,
`DrawSkills`, `DrawCalcs`, `DrawConfig` (U49 `draw_a.rs`, U50 `draw_b.rs`).

Applied (U47's file): `layout::CompareDrawHooks` (`draw_hooks()` / `set_draw_hooks(..)` /
`default_draw_hooks()`), one entry per pass with the Lua parameters (`me` the tab); each default draws
nothing, so the tab and every layout pass run before U49/U50 land. `layout::State` holds the fields
those passes read or write: `treeLayout`, `configSectionLayout` (`ConfigSectionLayout` with `rows`,
`diffCount`, `x`, `y`), `calcsSkillHeaderHeight`, and `itemsColWidth` (DrawItems writes it,
CompareTab.lua:3778; Draw reads it, :1796).

Proposed: U49/U50 replace their entries in `default_draw_hooks` (one line each).

## 4. One-line changes in other units' files

* `build/on_frame.rs` `default_shell_hooks`: the `BuildTab::Compare` arm
  (`crate::tabs::compare::layout::draw_tab`, CCR-0136 §2); every `BuildTab` now has an arm, so the
  `_ => {}` arm is gone.
* `tabs/compare/init.rs` (U46): `default_compare_hooks` installs U47's entry methods (CCR-0136 §1, one
  line each); `not_ported` carries `#[allow(dead_code)]`. U46's `PendingPowerReportList` gets ListControl's
  `IsMouseOver` (ListControl.lua:119-124 without the scroll bar child): HandleScrollInput
  (CompareTab.lua:2418) calls `listControl:IsMouseOver()` while the list is shown, which raised "attempt
  to call method 'IsMouseOver'" on the placeholder in the Summary view. U48's real control replaces it.
