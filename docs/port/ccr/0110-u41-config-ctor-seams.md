# CCR-0110: U41 ConfigTab constructor seams (U42 hooks, the var-controls bridge, ownership)

Unit: **U41** (ConfigTab A: `ConfigTab.lua:1-834`, CustomModBlockControl and the constructor).
Status: **pending** (code on `port/U41` follows it). Number reserved with
`scripts/port/next_ccr.sh`.

## 1. `ConfigHooks`: the U42 methods the constructor calls

Evidence: the constructor's closures call methods in U42's range (ui doc §12.6 U42 row,
`ConfigTab.lua:835-1362`, ConfigModBrowser.lua, ConfigVisibility.lua): `self:UpdateControls()`
(L180, the search box), `self:AddUndoState()` (UndoHandler.lua:34-41 with ConfigTab.lua:1242
CreateUndoState; L168, L300, L310, L318, L328, L339, L820, and CustomModBlockControl L39, L46,
L57, L81), `self:OpenConfigSetManagePopup()` (L176), `configModBrowser.OpenAddModPopup` (L52)
and `configVisibility.isShowAllExcluded` (L208). U42 depends on U41, so these cannot be called
statically yet.

Applied (U41's file `pob-app/src/tabs/config/ctor.rs`), the CCR-0098 §3 pattern:

```rust
#[derive(Clone)]
pub struct ConfigHooks {
    pub update_controls: Rc<dyn Fn(&mut Cx, CtrlId)>,              // default: see §3
    pub add_undo_state: Rc<dyn Fn(&mut Cx, CtrlId)>,               // default: nothing
    pub open_config_set_manage_popup: Rc<dyn Fn(&mut Cx, CtrlId)>, // default: "not ported (U42)"
    pub open_add_mod_popup: Rc<dyn Fn(&mut Cx, CtrlId, &Table)>,   // default: "not ported (U42)"
    pub is_show_all_excluded: Rc<dyn Fn(&ConfigOptionDef) -> bool>, // default: "not ported (U42)"
}
pub fn default_config_hooks() -> ConfigHooks;
pub fn config_hooks() -> ConfigHooks;
pub fn set_config_hooks(f: impl FnOnce(&mut ConfigHooks));
```

U42 replaces its entries (one line each in `default_config_hooks`, or `set_config_hooks`).
`self:SetActiveConfigSet(id)` (L167) goes through U19's existing
`BuildHooks::config_set_active_config_set` (a nil id, `configSetOrderList[index]` out of range,
passes the active id, which is what SetActiveConfigSet L1343-1345 does with nil).

## 2. Lines of U42's range that U41 ports

* `UpdateCustomModsControls` (1302-1334, UI half; the model half 1306-1316 is calc:BM-B's
  `ConfigTab::UpdateCustomModsControls`): the constructor (L828) and the custom mod blocks'
  delete button (L38) and the add button (L819) call it, it only builds U41's
  `CustomModBlockControl`, and the U41 row's acceptance includes "custom mod blocks". Ported as
  `ConfigTab::update_custom_mods_controls(cx, me)`.
* `IsSectionCollapsed` (835-837): the constructor's `isCollapsed` (L189-191) is a call to it;
  ported as `ConfigTab::is_section_collapsed(cx, me, section)`.

Proposed: the U42 row drops these two from its range (or calls U41's functions).

## 3. The var-controls bridge (model control tables <-> real controls)

Evidence: the ConfigOptions applies (calc:CO-A/B, run by BuildModList) read and write
`build.configTab.varControls[var]` (`selIndex`, `list`, `enabled`, `placeholder`,
`SetPlaceholder`, `SelByValue`; contracts.md#model-census), which the model represents as
control-state tables (`ConfigTab::ctor_controls_model`, CCR-0037/CCR-0080 §1); the model's
`UpdateControls` (BM-B, 1008-1022) writes `buf`/`state`/`selIndex`/`placeholder` into those
tables too. In Lua these are the real controls; in the port there are two objects per option.

Rule (U41's `ctor.rs`): the model tables are canonical for that state.

* UI -> model: `ConfigTab::push_var_control(cx, me, var, VarSource)` at the start of the option
  control's own callback (check `state`, edit `buf`, dropdown `selIndex`), before the callback's
  BuildModList reads them;
* model -> UI: `ConfigTab::pull_var_controls(cx, me)` copies every table's `buf` (through
  `EditControl::set_text` only when it differs), `placeholder`, `state`, `selIndex` and
  `enabled` into the controls. U41 runs it after every BuildModList it runs and at the end of
  the constructor (DIVERGENCE(init-order): the model's Load/UpdateControls/BuildModList ran on
  the tables before the controls existed).

Asked of U42: `ConfigTab:Draw` runs `pull_var_controls` first (a BuildModList elsewhere, e.g.
SyncLoadouts, changes the tables without the controls), and `UpdateControls` is the model half
plus the pull plus `update_custom_mods_controls` (the default hook already is exactly that:
`ConfigTab::update_controls_default`). `RestoreUndoState` (1250-1268) calls UpdateControls, so
the pull follows it too.

## 4. Where `impl Widget for ConfigTab` lives

ui doc §12.1: "The `Widget` impl lives with the unit that owns the class's `Draw`" (U42). A
tab's `Draw(viewPort, inputEvents)` is an inherent function called by BuildMode (as
`NotesTab::draw`), not a `Widget` method, and `New<ConfigTab>` (the constructor, U41) needs the
impl. Applied: `impl Widget for ConfigTab { NAME = "ConfigTab", CAPS = empty }` in `ctor.rs`.
Proposed: §12.1 says "the `Widget` impl of a tab drawn by BuildMode lives with its constructor".

## 5. ConfigOptions.lua 11-115 (the option tooltip helpers)

`applyPantheonDescription`, `banditTooltip`, `bossSkillsTooltip`, `LowLifeTooltip`,
`FullLifeTooltip` and `mapAffixTooltip` are UI code; calc:CO-A marks them `@pob-skip` ("UI
tooltip helper"). U41 ports them in `ctor.rs` (dispatched by the dump's `LuaFnRef.src` range)
with `/// lua:` doc lines but **no** `@pob` markers: `scope.toml` keeps ConfigOptions.lua 1-2380
in the calc scope, and a second marker would cover those lines twice (`provenance.py coverage`).
Proposed: CO-A's skip reasons name `pob-app/src/tabs/config/ctor.rs` as the port.

## 6. `string:matchOrPattern` (Common.lua:1038-1079)

The search filter's `PCall(string.matchOrPattern, label, searchStr)` (L197) needs it; no crate
had a port. Ported as `pob_app::tabs::config::ctor::match_or_pattern(s, pattern) ->
Result<bool, String>` (`Err` = what PCall reports), `@pob Modules/Common.lua:1038-1079` (out of
the calc scope). MinionSearchListControl (U31), ItemDBControl and CompareTab (U46) call the same
function and can reuse it.

## 7. Small surface notes

* `build/init.rs` `default_hooks`: `config_tab: Rc::new(crate::tabs::config::ctor::ctor_hook)`
  (one line, as CCR-0098 §3 asks of the tab owners). The draw/flag arms (`ShellHooks::draw_tab`,
  `tab_flag` for `BuildTab::Config`/`TabFlag::Config`) are U42's (Draw, the UndoHandler
  `modFlag`).
* `pob-app` depends on `pob-modparser` (already a dependency of pob-calc): the custom mod
  block's `inactiveText` calls `modLib.parseMod` (ConfigTab.lua:90).
