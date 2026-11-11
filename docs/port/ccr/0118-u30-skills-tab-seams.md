# CCR-0118: U30 Skills tab seams (U31/U38 hooks, the BuildMode arms, nil `mainActiveSkill`)

Unit: **U30** (SkillsTab A, `Classes/SkillsTab.lua:1-742, 1081-1502`; the model halves are
calc:BM-A/BM-B). Status: **pending** (code on `port/U30` follows it). Number reserved with
`scripts/port/next_ccr.sh` (`U30-skills-tab-seams`).

## 1. `SkillsHooks`: what the tab calls in units not merged yet

Evidence: the constructor calls `new("SkillListControl")` (`SkillsTab.lua:113`) and
`self:CreateGemSlot(1)` (L431); UpdateGemSlots calls `self:CreateGemSlot(slotIndex)` (L1087);
the skill set manager builds `new("SkillSetListControl")` (L1446). All three are U31's
(`SkillsTab.lua:743-1080`, `SkillListControl.lua`, `SkillSetListControl.lua`). The tab also calls
`self.build.itemsTab:GetSocketDescriptionLine(item)` (L238), `:AddItemTooltip(tooltip, ttItem,
slot)` (L198) and `:AddUndoState()` (L262), which are U38's/U34's.

Applied in U30's own file (`pob-app/src/tabs/skills/core.rs`), the CCR-0098 §3 pattern: a
thread-local `SkillsHooks` (`skills_hooks()` / `set_skills_hooks(..)`) with the entries
`skill_list_ctor`, `create_gem_slot`, `skill_set_list_ctor`,
`items_get_socket_description_line`, `items_add_item_tooltip`, `items_add_undo_state`. The
defaults are stand-ins with the Lua geometry:

* the two lists are `ListStandIn` (a `ListControl` part over `socketGroupList` / an empty list,
  shown, drawing nothing, `IsMouseOver` false), so the controls anchored to the group list (the
  gem options, the group details) are shown and laid out as in PoB;
* `create_gem_slot` creates the row's controls (the Lua classes, `controls` keys
  `gemSlot<i>Delete`, `…Name`, `…Level`, `…Quality`, `…Enable`, `…Count`, `…ErrMsg`,
  `…EnableGlobal1`, `…EnableGlobal2`, anchors and rects of L765-1078) never shown, and stores the
  row with `SkillsTab::set_gem_slot`;
* the items-tab entries: `"<#sockets> sockets"`, one tooltip line with the item name, nothing.

`GemSlot` (`self.gemSlots[i]`, the nine control ids) is declared in U30's file because
UpdateGemSlots and SetDisplayGroup (U30) read it. U31 replaces the three U31 entries (one line
each in `default_skills_hooks`) and stores its rows with `set_gem_slot`; U38/U34 replace the
three items entries. The public methods U31/U44/U46 call are listed in the module notes.

## 2. The BuildMode arms

Applied (one arm each, the pattern of U22/U39, CCR-0106 §2): `init::default_hooks` gets
`skills_tab: ctor_hook`, `skills_set_active_skill_set: set_active_skill_set_hook` and
`skills_add_socket_group_tooltip: add_socket_group_tooltip_hook`; `on_frame::default_shell_hooks`
gets the `BuildTab::Skills` draw arm and the `TabFlag::Skills` get/set arms. Before the tab
exists (Build:Init's own SetActiveSkillSet calls), `set_active_skill_set_hook` runs the model half
only.

## 3. Save of a nil `mainActiveSkill` (proposal to calc:BM-B, not applied)

Evidence: `SkillsTab.lua:711` `local newGroup = { label = "", enabled = true, gemList = { } }`
(also `SkillListControl.lua` "New", and the groups CalcSetup creates): `mainActiveSkill` /
`mainActiveSkillCalcs` stay nil; the readers write `or 1` (e.g. CalcSetup.lua:1886), and
`SkillsTab:Save` writes `tostring(nil)` = `"nil"` (L596-597). The model's nil number is NaN
(CCR-0083 §3; `init_env_skills.rs` sets it for the groups it creates, the readers treat NaN as
nil), and U30's paste follows it, but calc:BM-B's Save writes `tostring_num(NaN)` (`"nan"` /
`"-nan"`) where PoB writes `"nil"` (DIVERGENCE(paste-main-active-skill)).

Proposed (calc:BM-B's file):

```diff
 // model/skills_tab.rs, Save (L596-597)
-                    Some(tostring_num(socketGroup.mainActiveSkill.get())),
+                    Some(if socketGroup.mainActiveSkill.get().is_nan() { "nil".to_owned() }
+                         else { tostring_num(socketGroup.mainActiveSkill.get()) }),
```

(and the same for `mainActiveSkillCalcs`). Until then U30's scenarios
(`pob-ui-oracle/tests/u30_skills.rs`, `mask`) compare a Lua `"nil"` and a Rust `"nan"`/`"-nan"`
of these two attributes as equal and count the masked attributes.

## 4. `data.skillColorMap[nil]`

`self.controls.imbuedSupport.inactiveCol = data.skillColorMap[gem.grantedEffect.color]` (L338,
L1290) assigns nil for a gem without a colour; `EditControl.inactive_col` is a `String`. U30
raises `lua_error!("CCR-0118: skillColorMap[nil]")` there (no support gem in the data has a nil
colour). Same for a `displayGemList` entry that is not a gem instance (L1361) and `tostring` of a
table value (`socketGroup.imbuedSupport` is always a string).
