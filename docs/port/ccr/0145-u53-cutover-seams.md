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

- `crates/pob-app/src/tabs/skills/gem_slots.rs` (U31, fix round 1): the two "Enable" check boxes
  (SkillsTab.lua:1043-1046, :1062-1065) clear the marker when they set the field, so an explicit
  `false` saves as `"false"`.

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
| `BreakdownHooks::node_viewer_draw` / `node_viewer_highlight_ring` (`tabs/calcs/breakdown.rs`, U40) | draws nothing / no image | U23's `PassiveTreeView:Draw` and `viewer.highlightRing` (CalcBreakdownControl.lua:631-640); the viewer is built on its first draw (DIVERGENCE(load-order): the constructor has no `Cx`; the field is `Option<PassiveTreeView>`, `None` until then) |
| `TabSeams` (`tabs/import/character.rs`, U44): `items_delete_item`, `items_add_undo_state`, `skills_add_undo_state`, `skills_clear_group_list_selection`, `skills_set_display_group` | model-only stand-ins (CCR-0117 §1) | U34's `DeleteItem`, U38's/U30's `AddUndoState`, U31's group list selection, U30's `SetDisplayGroup` (model half only when there is no Skills tab control) |
| `ItemsTabHooks::socket_viewer_draw` (`tabs/items/slot_helper.rs`, U32) | a Lua error "PassiveTreeView:Draw is not ported yet (U23)" | U23's `PassiveTreeView:Draw` on `itemsTab.socketViewer` (ItemSlotHelper.lua:19-30: zoom, zoomX, zoomY, Draw), lent through `cx.lend` |
| `ItemsTab` constructor, L100 `self.socketViewer = new("PassiveTreeView"):PassiveTreeView()` (`tabs/items/ctor.rs`, U33) | the field's `Default` | U23's `PassiveTreeView::new` (its image loads now happen in the constructor, as in PoB) |
| `ItemsUiHooks::socket_viewer_add_node_name` (`tabs/items/ctor.rs`, U33; ItemsTab.lua:870) | a Lua error naming U24 | U24's `AddNodeName` on `self.socketViewer` |
| `ItemsUiHooks::tree_open_spec_manage_popup` (ItemsTab.lua:216, the "Manage..." spec button) | a Lua error naming U25 | U25's `TreeTab:OpenSpecManagePopup` |
| `TreeViewHooks::socket_right_click` (`tree_view/mod.rs`, U23; PassiveTreeView.lua:522-529) | a Lua error naming U32/U34 | `slot = itemsTab.sockets[id]`; if `slot:IsEnabled()`: `slot.dropped = true`, `itemsTab:SelectControl(slot)`, `viewMode = "ITEMS"` |
| `AppTimelessBuild` (`tabs/tree/timeless_list.rs`, U28): `set_sel_item_id`, `populate_slots`, `add_undo_state` | Lua errors naming U32/U34; the model half of PopulateSlots | U32's `SetSelItemId` (ItemSlotControl.lua:72-84, on the socket's model table), U32's full `PopulateSlots` hook (model and dropdown lists), U38's `AddUndoState` |

Found by the e2e runs (`d_skills`: the sockets label) and by auditing every `default_*hooks`;
the second group (fix round 1) by grepping every `not_ported(` call of `pob-app`. Only U51's
trade hooks (`TradeQuery:PriceItem`, `CompareBuySimilar.openPopup`; U51 is on hold) still raise.
e2e `m_tree_socket_rclick` (the right click: Items tab, slot dropped, its viewer drawn) and
`n_items_socket_hover` (the slot hover's viewer) cover the socket viewer.

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
- Review note (fix round 1, not changed): the `v == "nil"` rule also hides a Rust nil where PoB
  writes false in those harnesses; `u53_e2e` compares nil verbatim. FOLLOWUPS "U53" item 4.

## 8. The first spec undo state is the one `PassiveSpec:Load` takes (CCR-0123 §2)

`PassiveSpec:Load` ends in `self:ResetUndo()` (PassiveSpec.lua:194), before PostLoad's
`BuildClusterJewelGraphs` allocates the cluster subgraph nodes. The port built the TreeTab's
stacks in its constructor (`DIVERGENCE(reset-undo)`) from the allocation after PostLoad. e2e
`c_tree_hover` (allocate, then Ctrl+Z on a cluster-jewel build) found the consequence: PoB
restores its 94-node load state (and re-runs the subgraphs), the port its 133-node one, and the
saved `nodes` lists came out in different orders.

- `crates/pob-calc-core/src/classes/passive_spec.rs` (calc:PS-A) L194: the state
  `CreateUndoState()` gives there is kept in `extra["$loadUndoState"]`.
- `crates/pob-app/src/tabs/tree/core.rs` (U25) `spec_reset_undo`: pushes that state (once) when
  it exists, else `CreateUndoState()` as before.

## 9. Tree draw performance (ui doc §13 item 7)

`crates/pob-app/src/tree_view/` (U23): the U53 gate is the Tree tab draw < 4 ms at 1080p.
Behaviour is unchanged (e2e and u23-u28 traces equal); what changed:

- `TreeAssets::subgraph_connectors`: the polygons of a cluster subgraph's `subGraph.connectors`
  are built once per pair list (PoB builds them once in `BuildSubgraph`), not every frame.
- The asset, sprite and bloodline maps hash with ahash (std's SipHash was a tenth of the draw).
- Lookup keys built without `format!`: a reused buffer for `a .. b` asset names, pre-interned
  names for the `node.overlay[state..]` keys, static `"<type>Active"` sprite keys.
- `node.sprites` memoised per icon string (`TreeAssets::icon_cache`, keyed by the `Rc<str>`).
- The hover loop (PassiveTreeView.lua:307-316) tests the distance before the two `isProxy`
  lookups (pure reads; a node it lets through takes Lua's order, nil positions included).

Release, 1920x1080, 72 frames per build, two runs on the shared machine: p50 1.65-2.41 ms, p95
1.86-3.55 ms (before: 2.6-3.7 and 2.7-4.5). `tests/u53_perf.rs` asserts the gate in every build without debug assertions.

## 10. Web: `NewFileSearch` sees the fetched `src/` files

`crates/pob-host/src/fs_web.rs` (U7): `WebFs::search` listed only its IndexedDB tree, while
`read` falls back to the VFS of PoB's `src/`. The tree view probes `TreeData/<version>/` by a
search (`DirListing`, PassiveTree.lua:909-935 `io.open`), so on the web every sprite sheet of
the version directory was requested as `TreeData/<sheet>`, failed, and drew as the default
texture (the grey squares of FOLLOWUPS "U53" item 3). The search now adds the VFS's files (and
the directories its keys imply) under the searched directory, the tree's own entries first.
