# CCR-0114: U45 PartyTab seams (model-held control state, build sites, Inflate, re-exports)

Unit: **U45** (PartyTab UI, `Classes/PartyTab.lua:16-515, 670-717`). Status: **pending**
(code on `port/U45` follows it). Number reserved with `scripts/port/next_ccr.sh`.

## 1. The controls' state lives in two places (CCR-0019 control tables)

Evidence: CCR-0019 §1/§3 moved the state that `PartyTab:Load`, `:Save` and `:ParseBuffs` read
and write into the model as Lua-like tables (`pob-calc/src/model/party_tab.rs:172-260`,
`PartyTab::ctor_model`: `extra.controls.<edit>.buf`, `<simple*>.label`,
`importCodeDestination.{list,selIndex}`, `appendNotReplace.state`, `ShowAdvanceTools.state`;
`extra.lastContent`). In PoB these are the fields of the very controls the tab draws
(PartyTab.lua:287-511). The UI's `EditControl`/`CheckBoxControl`/`DropDownControl` keep their own
fields, and calc:BM-B's `Load` runs inside `build_xml::Init_model` (CCR-0080 §1), i.e. before
the UI constructor exists.

Applied (U45's file only, `pob-app/src/tabs/party.rs`):

* the six `simple*` labels' `label` is a `Prop::f` that reads the model's `{ label }` table, so
  the model's ParseBuffs and the UI's `clearInputText` write the one field;
* every `SetText` on the seven model-backed edits also writes the model's `buf`; the end of
  `PartyTab::draw` copies the edits' `buf`, the check boxes' `state` and the destination's
  `selIndex` into the model tables (input is the only other writer, and it happens inside Draw;
  OnFrame recalculates before the tab draws, Build.lua:1258-1291, and SaveDB/Ctrl+S run outside
  Draw), so the model's `Save` writes what the controls hold;
* `ctor_hook` replays Load's UI statements after the constructor (PartyTab.lua:522-571: the
  edits' `SetText`, `SelByValue(destination)`, the two `state`s) from the model tables.

Proposed (no change to frozen code): keep this split. If calc:BM-B later wants the UI to own
the controls, `PartyTab::Load/Save` would take a trait for the seven buffers instead of the
tables.

## 2. `buildSites` and `UrlDecode` (U43/U44) have no Rust API yet

Evidence: PartyTab.lua:137-148 reads `buildSites.websiteList[j].matchURL`/`.label`; :306-317
calls `buildSites.DownloadBuild(link, site, callback)` (Modules/BuildSiteTools.lua:96-118, U43,
`tabs/import/build_sites.rs`, still the UI-U0 stub); :134 calls the global `UrlDecode`
(ImportTab.lua:1917-1924, U44's range). Their files are other units'.

Applied: `tabs/party.rs` declares `pub struct SiteHooks { website_list, download_build,
url_decode }` with `site_hooks()` / `set_site_hooks(..)` (thread-local, as `BuildHooks`). The
defaults know no site (a URL then reads "Invalid input", as a non-code would) and the two calls
raise `lua_error!("… is not ported yet (U43|U44)")`.

Proposed: when U43 lands, it installs `website_list` (its `websiteList` labels and `matchURL`
patterns, in list order) and `download_build` (`buildSites.DownloadBuild` with the callback
`(isSuccess, data)`), and U44 `url_decode`, with one `set_site_hooks` call each in the place
U43 installs its own hooks (or replaces the defaults in `default_site_hooks`, one line each).

## 3. SimpleGraphic's `Inflate` has no owner

Evidence: PartyTab.lua:150 (and ImportTab.lua, U43) call `Inflate(…)`, a SimpleGraphic host
function (`l_Inflate`: zlib `inflate` until `Z_STREAM_END`, nil otherwise). Neither `pob_sg::Sys`
nor `pob-host` provides it; the ui doc's API table has no row for it.

Applied: `tabs/party.rs::inflate(&[u8]) -> Option<Vec<u8>>` (flate2's `Decompress` with the
zlib header; nil when the stream does not end, e.g. a truncated or non-zlib input; trailing
bytes ignored). `flate2` (workspace dependency, pure Rust backend) added to `pob-app`.

Note for the oracle: `pob-oracle`'s LuaJIT `Inflate` (`jit.rs:375-398`) returns `""` instead of
nil when decoding fails, and a truncated zlib stream decodes to its prefix (flate2's read
adapter stops at EOF without an error). So every partial or invalid code reads "Code is valid"
on the Lua side of the UI oracle, where PoB says "Invalid input". The U45 scenarios therefore
paste whole codes only.

Proposed: move `inflate` (and a `deflate` for U43's export) into `pob_sg::Sys` or a small
`pob-host` helper owned by U7, and make the oracle's `Inflate` return nil on failure. U43
(CCR-0109 §7) has since landed its own `tabs::import::core::{inflate, deflate}` (miniz_oxide);
the move should leave one `Inflate` for both tabs.

The inflated bytes become a `String` with `from_utf8_lossy` (DIVERGENCE(bytes), as U43's
import), so a code whose XML is not UTF-8 reads U+FFFD where PoB keeps the raw bytes.

## 4. `model.rs` (U15) lacks two re-exports

Evidence: the "Disable Party Effects" / "Rebuild All" / "Clear" handlers (PartyTab.lua:340,
353-357, 363-367) create `new("ModDB"):ModDB()` and `new("ModList"):ModList()`, i.e.
`pob_calc::classes::ModStore::{ModDB, ModList}`; `model.rs` re-exports the `ModDB`/`ModList`
aliases but not `ModStore`. `common.base64.decode` (PartyTab.lua:150) is
`pob_calc::classes::passive_spec::base64_decode`, not re-exported either.

Work-around: `tabs/party.rs` names both through `pob_calc::` directly, marked CCR-0114 §4.
Proposed:

```diff
 pub use pob_calc::classes::{
-    Item, ItemRef, ModDB, ModList, Node, NodeRef, PassiveSpec, PassiveSpecRef, PassiveTree,
+    Item, ItemRef, ModDB, ModList, ModStore, Node, NodeRef, PassiveSpec, PassiveSpecRef, PassiveTree,
     PassiveTreeRef,
 };
+/// `common.base64` (runtime/lua/base64.lua)
+pub use pob_calc::classes::passive_spec::{base64_decode, base64_encode};
```

## 5. The hook arms (CCR-0106 §2)

Applied (one line each): `init::default_hooks` `party_tab: Rc::new(crate::tabs::party::ctor_hook)`;
`on_frame::default_shell_hooks` `BuildTab::Party => party::draw_tab`, `TabFlag::Party =>
party::mod_flag` / `party::set_mod_flag`. The Party section has no `load_ui_section` /
`save_ui_section` arm: calc:BM-B's saver owns it.

## 6. The LuaCalcBridge's `SaveDB` clears the Party tab's `modFlag`

Evidence: `pob-ui-oracle/src/lua_calc.rs:367-371` (U15) serialises the live model with
`build.SaveDB("")` before every calc; calc:BM-B's `PartyTab::Save` (PartyTab.lua:654-667) writes
`lastContent`, which `PartyTab:Draw` (L707-715) compares with the edits to set `modFlag`. PoB's
calc never saves, so with the bridge every recalc after an import, Clear or Rebuild made the Rust
side read "saved" (the Save button disabled) for the frames until the next real save, where PoB
reads "unsaved" (`u45_a_import`: 12 divergences in 6 frames before the work-around).

Work-around (U45's test only): `tests/u45_party.rs::KeepPartyLastContent` wraps the bridge and
restores `partyTab.lastContent` around `build_output`, `misc_calculator` and `MiscCalc::call`.
Proposed (a follow-up for U15, which owns `lua_calc.rs`): the bridge does the same for every
saver whose `Save` has side effects (`partyTab.lastContent`; ConfigTab's and the others' when
their UIs land), or serialises through a side-effect-free variant of SaveDB. Until then each UI
unit whose tab reads `lastContent` needs a test-side wrapper like this one.
