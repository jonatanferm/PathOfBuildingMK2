# CCR-0132: U27 timeless jewel search seams (FetchLeagues, the viewer tooltip, fallback-weight nodes, re-exports)

Unit: **U27** (`TreeTab.lua:1170-2888`, FindTimelessJewel; file `pob-app/src/tabs/tree/timeless.rs`).
Status: **pending** (code on `port/U27` follows it). Number reserved with `scripts/port/next_ccr.sh`.

No frozen contract changes. This CCR records the seams U27 introduced in its own file, the one-line
hook it installed in U25's file (as CCR-0123 §6 asks), and the gaps of other units it worked around.

## 1. `TradeQueryRequests:FetchLeagues` ported locally (U51 is deferred)

Evidence: `TreeTab.lua:2219` (`self.tradeQueryRequests = new("TradeQueryRequests"):TradeQueryRequests()`),
`:2370` (`self.tradeQueryRequests:FetchLeagues(currentRealmId, function(leagues, errMsg) .. end)`);
`TradeQueryRequests.lua:24-34` (constructor), `:468-495` (FetchLeagues: one `launch:DownloadPage` of
`self.hostName .. "api/trade/data/leagues"` with the `Authorization: Bearer` header). The TradeQuery
classes are U51 (ui doc §12.7, "optional, deferred").

What U27 did: `timeless::fetch_leagues(cx, &TradeQueryRequests, realm, callback)` ports L468-495 over
U16's `platform::net::download_page` and U43's `poe_api::{auth_token, dkjson_decode, json_get}`
(provenance marker on the function, `@pob-file Classes/TradeQueryRequests.lua` in the header).
`self.tradeQueryRequests` is `timeless::State::trade_query_requests: Option<TradeQueryRequests>`
holding `hostName` only (the rate limiter and request queues are never used by this caller).

Proposed for U51: own `TradeQueryRequests` (with the rate limiter) in `trade/`, keep `FetchLeagues`'s
signature, and replace U27's local function with a call (one line in `timeless.rs`, which U27 hands
over for it).

## 2. `self.viewer:AddNodeTooltip(tooltip, node, self.build, true)`

Evidence: `TreeTab.lua:1560, 1598, 1732` pass `returnEarly = true` (PassiveTreeView.lua:1476).
`tree_view::TreeViewHooks::add_node_tooltip` (CCR-0115) has no `returnEarly` parameter (its default
calls `add_node_tooltip(.., false)`).

What U27 did: `timeless::TimelessHooks { add_node_tooltip }` (default: U24's
`PassiveTreeView::add_node_tooltip(cx, tt, node, true)` on the lent TreeTab viewer), so the oracle
tests can stand in for U24 on both sides as u25 does.

Proposed for U23/U24: add `return_early: bool` to `TreeViewHooks::add_node_tooltip`; U27 then calls
that hook and drops its own.

## 3. U25's `TreeTabHooks::find_timeless_jewel` (CCR-0123 §6)

`tabs/tree/core.rs` `default_tree_tab_hooks`: the placeholder entry is replaced by
`Rc::new(TreeTab::FindTimelessJewel)` (one line, the pattern CCR-0123 §6 names).

## 4. The fallback-weight pseudo nodes and the LuaCalcBridge

Evidence: `TreeTab.lua:1962` `calcFunc({ addNodes = { [node] = true } })` where `node` is a table
`{ id = <legion id string>, modList = .., divisor = .. }` (not a tree node);
`CalcSetup.lua:717` keys it by its string id. `Override.addNodes` (calc doc, CCR-0082 §2) is an
id-keyed `LjIntMap<NodeRef>`.

What U27 did: the pseudo node is a `Node` with that `modList` and a NaN id (one node per call, so the
key the map gives it never collides). A `nil` modList (a line `parseMod` does not know) is an
empty list, as `ModList:AddList(nil)` adds nothing (CalcSetup.lua:141, ModList.lua:88-94).

Gap reported (no change requested of U27's range): U15's LuaCalcBridge rebuilds `addNodes` on the
Lua side as `__bridge_node(b, id)` (`lua_calc.rs` `override_lua`), i.e. by tree node id, so it
cannot carry a pseudo node. The "Generate" button is therefore not covered by the u27 oracle
scenarios (they run on the bridge). Proposed for U15: let `override_lua` pass a node whose id is
NaN as `{ id = .., modList = <the mods> }` (the bridge already serialises mods for `repItem`).

## 5. Re-exports (U15 `model.rs`)

U27 names `pob_calc::classes::{ModStore, passive_tree::{node_id_val, nodesInRadiusAt}}`,
`pob_calc::modules::power::{powerStat_table, GetFromOutput}` and
`pob_gamedata::timeless_lut::{readLUT, readAbyssJewelLUT}` directly (the CCR-0103 §4 / CCR-0115 /
CCR-0123 §9 gap). Proposed: re-export them from `model.rs`.

## 6. Local ports of LUT's private helpers

`data.resolveAbyssJewelComponent` and `data.getAbyssJewelComponentRoll` are ported by LUT against the
immutable LegionPassives `Value`; PS-B's versions over the tree's legion `Table` are private
(`passive_spec_paths.rs`). U27 has its own two (`resolve_component`, `component_roll`, against the
tree's legion `Table`, CCR-0020 §8). Proposed: PS-B (or LUT) exports one `Table` version that both
use.
