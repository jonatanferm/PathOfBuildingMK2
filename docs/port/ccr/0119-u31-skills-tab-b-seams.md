# CCR-0119: U31 SkillsTab B seams (the SkillsTab UI calls, U30's WIP seam, the New group's nil numbers)

Unit: **U31** (SkillsTab.lua:743-1080 `CreateGemSlot`; `SkillListControl` 1-267,
`SkillSetListControl` 1-127, `MinionListControl` 1-107, `MinionSearchListControl` 1-71; files
`pob-app/src/tabs/skills/{gem_slots,skill_list,skill_set_list,minion_list,minion_search_list}.rs`).
Status: **pending** (code on `port/U31` follows it, local workarounds marked `// CCR-0119`). Number
reserved with `scripts/port/next_ccr.sh`.

No frozen contract changes. This CCR records the seams U31 introduced in its own files, how they
meet U30's (unmerged, WIP on `port/U30`) seams, and one model-side note for calc:BM-B.

## 1. `SkillsTabUiHooks`: the SkillsTab UI state and methods U31's controls use

Evidence: CreateGemSlot reads `self.gemSlots[index-1]` (SkillsTab.lua:771), `self.anchorGemSlots`
(:769), `self.controls.groupLabel` (:838, :857, :948) and calls `self:AddUndoState()` (12 sites);
SkillListControl calls `skillsTab:SetDisplayGroup(..)` (SkillListControl.lua:49, :68, :152, :189,
:213, :228), `:AddUndoState()` (8 sites), `:CopySocketGroup(..)` (:157), reads
`skillsTab.gemSlots[1].nameSpec` (:71); SkillSetListControl calls `:SetActiveSkillSet(..)` (:101,
:115), `:AddUndoState()` (:74, :102, :117), writes `skillsTab.modFlag` (:68, :96) and calls
`build:SyncLoadouts()` (:75, :118). `gemSlots`, `anchorGemSlots`, the UndoHandler, the UI tail of
SetDisplayGroup and CopySocketGroup are U30's (SkillsTab.lua:1-742, 1081-1502), which is not
merged; the ui doc has no seam for a control unit that precedes its tab (as CCR-0107 §1 for U32).

Applied locally (U31's own file `gem_slots.rs`), in the CCR-0098 §3 / CCR-0107 §1 form:
`SkillsTabUiHooks` + `skills_tab_ui_hooks()` / `set_skills_tab_ui_hooks(..)` /
`default_skills_tab_ui_hooks()`, every entry taking the SkillsTab node:

| entry | Lua | default |
|---|---|---|
| `gem_slot(cx, tab, i) -> Option<GemSlot>` | `self.gemSlots[i]` | raises "… not ported yet (U30)" |
| `set_gem_slot(cx, tab, i, slot)` | `self.gemSlots[i] = slot` | raises |
| `anchor_gem_slots(cx, tab) -> CtrlId` | `self.anchorGemSlots` | raises |
| `add_undo_state(cx, tab)` | `self:AddUndoState()` | raises |
| `set_display_group(cx, tab, group)` | `self:SetDisplayGroup(group)` | calc:BM-B's model half |
| `copy_socket_group(cx, tab, group)` | `self:CopySocketGroup(group)` | raises |

The other calls already have seams and are used as is: `SetActiveSkillSet` and
`AddSocketGroupTooltip` are `build::init::BuildHooks::{skills_set_active_skill_set,
skills_add_socket_group_tooltip}`, `skillsTab.modFlag = true` is
`on_frame::ShellHooks::set_tab_flag(TabFlag::Skills, true)`, `SyncLoadouts` is
`BuildHooks::sync_loadouts`, `NewSkillSet` / `RebuildImbuedSupportBySlot` / `ProcessSocketGroup` /
`ProcessGemLevel` / `UpdateSocketGroups` are calc:BM-B's. `self.controls.groupLabel` is read from
the tab node's controls (`cx.child(tab, "groupLabel")`).

Nothing in the product constructs the SkillsTab yet (`BuildHooks::skills_tab` is `pending_tab()` on
`rust-port`), so the raising defaults are never reached there; `tests/u31_skills.rs` installs
stand-ins.

## 2. Meeting U30's WIP seam (`port/U30`, `tabs/skills/core.rs`, its CCR-0118)

U30's WIP declares `SkillsHooks { skill_list_ctor, create_gem_slot, skill_set_list_ctor, … }` with
stand-ins, plus `core::GemSlot` and `SkillsTab::{gem_slot, set_gem_slot}` over
`core::State::gem_slots`. U31 provides the three constructors with exactly those shapes and a
`GemSlot` with the same fields. Proposed, for whichever of U30/U31 merges second (one line each):

```rust
// tabs/skills/core.rs, default_skills_hooks()
skill_list_ctor: Rc::new(super::skill_list::skill_list_ctor),
create_gem_slot: Rc::new(super::gem_slots::CreateGemSlot),
skill_set_list_ctor: Rc::new(super::skill_set_list::skill_set_list_ctor),
// tabs/skills/core.rs: `pub use super::gem_slots::GemSlot;` replaces its own declaration;
// the three stand-ins (standin_skill_list, standin_create_gem_slot, standin_skill_set_list) go.

// tabs/skills/gem_slots.rs, default_skills_tab_ui_hooks()
gem_slot: Rc::new(|cx, tab, i| SkillsTab::has_gem_slot(cx, tab, i).then(|| SkillsTab::gem_slot(cx, tab, i))),
set_gem_slot: Rc::new(SkillsTab::set_gem_slot),
anchor_gem_slots: Rc::new(|cx, tab| cx.w::<SkillsTab>(tab).core.anchor_gem_slots),
add_undo_state: Rc::new(SkillsTab::add_undo_state),       // (cx, tab)
set_display_group: Rc::new(SkillsTab::set_display_group), // model half + UI tail
copy_socket_group: Rc::new(SkillsTab::copy_socket_group),
```

and SkillsTab.lua:1495 (`self.controls.groupList.list = self.socketGroupList`, U30's
SetActiveSkillSet UI tail) calls `skill_list::SkillListControl::set_list(cx, groupList, list)`.
The signatures U30's WIP has for these methods (`(cx, me, …)`) may need a closure adapter.

## 3. `ShellHooks` minion entries

`on_frame::default_shell_hooks()` (U20's file; CCR-0104 §1 lets owners replace their entry) now
points `minion_list_control` / `minion_search_list_control` at
`minion_list::spectre_library_list` / `minion_search_list::spectre_library_source` (two lines).

## 4. calc:BM-B note: Save of a nil `mainActiveSkill`

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
