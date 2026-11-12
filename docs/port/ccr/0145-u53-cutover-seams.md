# CCR-0145: U53 cut-over seams (Rust calc in every scenario, the e2e harness, the last stand-ins)

Status: proposed by U53 (ui doc §12.7 U53 row), applied on `port/U53`.

U53 integrates the whole app and runs it end to end against PoB with the Rust calc. Doing so
touched files of merged units; each change below is the smallest one that makes the full app
equal PoB, and each site is marked `CCR-0145 §n`.

## 1. The unit scenarios run on the Rust calc (`POB_UI_CALC`)

Every UI unit's acceptance test builds its Rust side with `LuaCalcBridge::new(..)` (ui doc §11.3:
"Rust-side numbers come from LuaCalcBridge until the Rust calc reaches parity"). The calc is at
parity (docs/port/STATUS.md, INT-1), so the same scenarios now run on the Rust calc.

- `crates/pob-ui-oracle/src/lua_calc.rs` (U15): `LuaCalcBridge::new` delegates `build_output` /
  `try_build_output` and `misc_calculator` to `pob_app::calc::RustCalc` (calc:PWR/CA) unless
  `POB_UI_CALC=lua` (then PoB in a second VM, as before). `LuaCalcBridge::lua(src)` (new field
  `force_lua`) always runs PoB: `u15_lua_calc` (which tests the bridge itself) and the desktop
  feature `lua-calc` use it.
- Measured with the Rust calc: every app unit harness u16-u52 green (STATUS.md, U53), and about
  10x faster than through the bridge (u41: 106 s against 1,943 s in the last full suite).

## 2. `scenarios/e2e` and its harness

`tests/parity.rs` (U8) discovers `scenarios/<unit>/` as plain parity runs, but its generic Rust
side neither initialises the game data nor records platform effects, and its T4 is byte-exact
(PoB's `pairs` order makes attribute order differ). U53's `scenarios/e2e/*.pobscript` run under
`tests/u53_e2e.rs` instead (no stand-ins on either side; `PobApp::new`, i.e. `RustCalc`; u20's T4
normalisation without its nil-default rule; u23's multiset buckets for the tree's sub-layers in
frames that draw the tree). `tests/parity.rs` skips the `e2e` directory.

## 3. Socket group Save: nil `mainActiveSkill` / `mainActiveSkillCalcs`

`crates/pob-calc-core/src/model/skills_tab.rs` (calc:BM-B): the groups CalcSetup creates
(CalcSetup.lua:1480 granted skills, :1518 explode) have nil `mainActiveSkill` and
`mainActiveSkillCalcs`, which the model holds as NaN (CCR-0019 §6). Save wrote `tostring_num(NaN)`
= `"nan"`; SkillsTab.lua:596-597 writes `tostring(nil)` = `"nil"`. Now `tostring_opt_nan`, as for
the gem level and quality. Found by the e2e T4 (`f_calcs`, `h_notes_save`).

## 4. Gem literals of CalcSetup: nil `enableGlobal1` / `enableGlobal2`

The gem instance literals of CalcSetup.lua:1488-1493 (granted skill; :1497 then sets
`enableGlobal1 = true`) and :1535-1542 (explode) have no `enableGlobal2` (and the explode one no
`enableGlobal1`), so SkillsTab.lua:609-610 saves `"nil"`. The model fields are `Cell<bool>` and
saved `"false"`.

- `crates/pob-calc-setup/src/modules/calc_setup/init_env_skills.rs` (calc:CS3): the literals set
  `extra["$enableGlobal1Nil"]` / `extra["$enableGlobal2Nil"]`; L1497 clears the first.
- `crates/pob-calc-core/src/model/skills_tab.rs`: `pub fn gemInstance_enableGlobal(gem, n) -> Val`
  (nil when the field is false and the marker is set, as `socketGroup_includeInFullDPS`), used by
  Save.

Proposed contract change: `GemInstance.enableGlobal1/2: Cell<Option<bool>>` (as CCR-0019 §6
proposed for `includeInFullDPS`); until then the markers.

## 5. The last stand-ins in the shipping defaults

Hook tables whose owners never replaced the stand-ins they were born with (the units landed in
another order). The full app ran them; PoB runs the real methods.

| Hook (file, owner) | Was | Now |
|---|---|---|
| `SkillsHooks::items_get_socket_description_line` (`tabs/skills/core.rs`, U30) | `"<n> sockets"` | U38's `ItemsTab:GetSocketDescriptionLine` (ItemsTab.lua:4185-4212) |
| `SkillsHooks::items_add_item_tooltip` | the item name | U38's `ItemsTab:AddItemTooltip(tooltip, item, slot)` (:4214) |
| `SkillsHooks::items_add_undo_state` | nothing | U38's `itemsTab:AddUndoState()` |
| `CoreHooks::undo` / `redo` (`tabs/items/core.rs`, U34) | a Lua error "not ported yet (U38)" | U38's UndoHandler (`tooltip::undo`/`redo`) |
| `CoreHooks::reset_undo` | nothing | U38's `tooltip::reset_undo` |
| `BuildHooks::list_sub_path` (`build/init.rs`, U19) | `None` | `main.modes.LIST.subPath` (U18's `ListMode::sub_path`), Build.lua:71 |
| `BreakdownHooks::node_viewer_draw` / `node_viewer_highlight_ring` (`tabs/calcs/breakdown.rs`, U40) | draws nothing / no image | U23's `PassiveTreeView:Draw` and `viewer.highlightRing` (CalcBreakdownControl.lua:631-640); the viewer is built on its first draw (DIVERGENCE(load-order): the constructor has no `Cx`) |
| `TabSeams` (`tabs/import/character.rs`, U44): `items_delete_item`, `items_add_undo_state`, `skills_add_undo_state`, `skills_clear_group_list_selection`, `skills_set_display_group` | model-only stand-ins (CCR-0117 §1) | U34's `DeleteItem`, U38's/U30's `AddUndoState`, U31's group list selection, U30's `SetDisplayGroup` (model half only when there is no Skills tab control) |

Found by the e2e runs (`d_skills`: the sockets label) and by auditing every `default_*hooks`.

## 6. Cut-over renames

`app/pob-port-desktop` → `app/pob-desktop` (package and bin `pob-desktop`), `app/pob-port-web` →
`app/pob-web` (package `pob-web`); the legacy `crates/pob-ui`, `app/pob-desktop`, `app/pob-web`
(egui) are deleted. `pob-engine`/`pob-data` stay for calc:INT-4. The worker's default glue name
follows (`./pob-web.js`, `calc/worker_web.rs`). The web page runs `PobApp` by default
(`?app=testcard` for the test card); the desktop feature `lua-calc` now injects the bridge.

## 7. Consequences for the unit harnesses (`crates/pob-ui-oracle/tests/u*.rs`)

- T4 normalisers: the rule "a Lua `nil` gem/group field equals the frozen Rust default" now also
  accepts a Rust `"nil"` (§3, §4 make the model write what Lua writes): `lua && v == "nil"` →
  `v == "nil"` in 16 harnesses.
- u30 keeps its Lua-side ItemsTab stand-ins (`GetSocketDescriptionLine`, `AddItemTooltip`,
  `AddUndoState`) and now installs the matching Rust ones itself (they were the defaults, §5).
- u39/u40 keep `nodeViewer = { Draw = function() end }` on the Lua side and install a no-op
  `node_viewer_draw` and no highlight ring on the Rust side (§5).
