# CCR-0119: U31 SkillsTab B seams (the SkillsTab UI calls, wiring with U30, the New group's nil numbers, a bridge finding)

Unit: **U31** (SkillsTab.lua:743-1080 `CreateGemSlot`; `SkillListControl` 1-267,
`SkillSetListControl` 1-127, `MinionListControl` 1-107, `MinionSearchListControl` 1-71; files
`pob-app/src/tabs/skills/{gem_slots,skill_list,skill_set_list,minion_list,minion_search_list}.rs`).
Status: **pending** (code on `port/U31` follows it; §2 is applied there). Number
reserved with `scripts/port/next_ccr.sh`.

No frozen contract changes. This CCR records the seams U31 introduced in its own files, how they
meet U30's seams (U30 merged while U31 was in flight; §2), a model-side note for calc:BM-B (§4)
and a LuaCalcBridge finding for U15 (§6).

## 1. `SkillsTabUiHooks`: the SkillsTab UI state and methods U31's controls use

Evidence: CreateGemSlot reads `self.gemSlots[index-1]` (SkillsTab.lua:771), `self.anchorGemSlots`
(:769), `self.controls.groupLabel` (:838, :857, :948) and calls `self:AddUndoState()` (12 sites);
SkillListControl calls `skillsTab:SetDisplayGroup(..)` (SkillListControl.lua:49, :68, :152, :189,
:213, :228), `:AddUndoState()` (8 sites), `:CopySocketGroup(..)` (:157), reads
`skillsTab.gemSlots[1].nameSpec` (:71); SkillSetListControl calls `:SetActiveSkillSet(..)` (:101,
:115), `:AddUndoState()` (:74, :102, :117), writes `skillsTab.modFlag` (:68, :96) and calls
`build:SyncLoadouts()` (:75, :118). `gemSlots`, `anchorGemSlots`, the UndoHandler, the UI tail of
SetDisplayGroup and CopySocketGroup are U30's (SkillsTab.lua:1-742, 1081-1502), unmerged when
U31 started; the ui doc has no seam for a control unit that precedes its tab (as CCR-0107 §1 for
U32).

Applied locally (U31's own file `gem_slots.rs`), in the CCR-0098 §3 / CCR-0107 §1 form:
`SkillsTabUiHooks` + `skills_tab_ui_hooks()` / `set_skills_tab_ui_hooks(..)` /
`default_skills_tab_ui_hooks()`, every entry taking the SkillsTab node:

| entry | Lua | default (U30's `core.rs`) |
|---|---|---|
| `gem_slot(cx, tab, i) -> Option<GemSlot>` | `self.gemSlots[i]` | `SkillsTab::has_gem_slot` / `gem_slot` |
| `set_gem_slot(cx, tab, i, slot)` | `self.gemSlots[i] = slot` | `SkillsTab::set_gem_slot` |
| `anchor_gem_slots(cx, tab) -> CtrlId` | `self.anchorGemSlots` | `core.anchor_gem_slots` |
| `add_undo_state(cx, tab)` | `self:AddUndoState()` | `SkillsTab::add_undo_state` |
| `set_display_group(cx, tab, group)` | `self:SetDisplayGroup(group)` | `SkillsTab::set_display_group` |
| `copy_socket_group(cx, tab, group)` | `self:CopySocketGroup(group)` | `SkillsTab::copy_socket_group` |

The other calls already have seams and are used as is: `SetActiveSkillSet` and
`AddSocketGroupTooltip` are `build::init::BuildHooks::{skills_set_active_skill_set,
skills_add_socket_group_tooltip}`, `skillsTab.modFlag = true` is
`on_frame::ShellHooks::set_tab_flag(TabFlag::Skills, true)`, `SyncLoadouts` is
`BuildHooks::sync_loadouts`, `NewSkillSet` / `RebuildImbuedSupportBySlot` / `ProcessSocketGroup` /
`ProcessGemLevel` / `UpdateSocketGroups` are calc:BM-B's. `self.controls.groupLabel` is read from
the tab node's controls (`cx.child(tab, "groupLabel")`). The hooks stay (rather than direct calls)
so `tests/u31_skills.rs` can run the controls on a stand-in tab.

## 2. Meeting U30's seam (CCR-0118 §1) — applied on `port/U31` after merging `rust-port`

U30 merged while U31 was in flight. `port/U31` merges `rust-port` and wires both directions:

* U30's `default_skills_hooks()` (`tabs/skills/core.rs`) now constructs U31's controls, as
  CCR-0118 §1 asks (three lines): `skill_list_ctor: skill_list::skill_list_ctor`,
  `create_gem_slot: gem_slots::CreateGemSlot`, `skill_set_list_ctor:
  skill_set_list::skill_set_list_ctor`. The three stand-ins (`standin_skill_list`,
  `standin_create_gem_slot`, `standin_skill_set_list`) are made `pub` and stay, because
  `tests/u30_skills.rs` runs the tab with U31's controls stubbed on the Lua side; that test now
  installs the three stand-ins explicitly after `default_skills_hooks()` (three lines).
* `gem_slots::GemSlot` is `pub use super::core::GemSlot` (U31's own declaration had the same
  fields), and `default_skills_tab_ui_hooks()` (§1) calls U30's methods.
* SkillsTab.lua:1495 is U30's `set_group_list_list`, which writes the `ListControl` part of
  `controls.groupList` (U31's SkillListControl) with U30's `GroupListSource`;
  `SkillListControl::set_list` does the same for callers outside the tab.

## 3. `ShellHooks` minion entries

`on_frame::default_shell_hooks()` (U20's file; CCR-0104 §1 lets owners replace their entry) now
points `minion_list_control` / `minion_search_list_control` at
`minion_list::spectre_library_list` / `minion_search_list::spectre_library_source` (two lines).

## 4. calc:BM-B note: Save of a nil `mainActiveSkill` (the same as CCR-0118 §3)

A group the SkillListControl "New" button creates (SkillListControl.lua:60-64) has
`mainActiveSkill` / `mainActiveSkillCalcs` nil. U31 stores NaN, the CCR-0019 nil convention calc:CS3
already uses for the groups it creates (CCR-0083 §3; `init_env_skills.rs:554/635`), so the calc's
`or 1` readers (`or1`) see nil. calc:BM-B's Save writes them with `tostring_num`
(`model/skills_tab.rs:737-744`), i.e. `"nan"` where PoB writes `"nil"`. Proposed (BM-B's file):
write them with the file's `tostring_opt_nan`, as it does for `level` / `quality`. Until then
`tests/u31_skills.rs`' T4 normalisation reads Lua `"nil"` and Rust `"nan"` of these two attributes
as equal and counts them among the set-aside fields.

## 5. Shared helpers reused (no new copies)

`tabs::items::item_db::match_or_pattern` (CCR-0107 §3) for MinionSearchListControl's
`PCall(string.matchOrPattern, ..)`; `tabs::items::item_list::{IdListSource, id_value, value_id}`
for the skill set id list; `controls::gem_select::{wiki_matches_key, wiki_open_gem, wiki_open}`
(CCR-0108 §2) for SkillListControl's F1; `tabs::items::slot::item_by_id` for GetRowIcon.

## 6. Found by the U31 scenarios (U15, LuaCalcBridge)

`adopt_build_output` re-points a skill effect's `srcInstance` at the live gem instances
(`pob-ui-oracle/src/lua_calc.rs`, `Rebinder::effect`), but not the keys of
`supportEffect.isSupporting` (CalcSetup.lua: `isSupporting[srcInstance] = true`), which stay the
injected copies. GemSelectControl's `CheckSupporting(gemA, gemB)` (GemSelectControl.lua:701-704,
U29) looks the live `gemB` up there, so through the bridge it is always false and the support
highlight (L631-638: a translucent green box on a focused or hovered gem selector of a group whose
gems support each other) never draws on the Rust side (found by `u31_c_slots`: 3 draws per frame
while a gem selector of allflame_exp_048's Vaal Double Strike group had focus). The RustCalc
facade writes the live instances and is not affected. Proposed (U15's file): rebind the
`isSupporting` keys in `Rebinder::effect` as `srcInstance` is. Until then the U31 scripts do not
focus or hover a gem selector of a group with support links.
