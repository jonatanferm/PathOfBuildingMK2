//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/ExtBuildListControl.lua 1-440, ExtBuildListProvider.lua 1-63, PoBArchivesProvider.lua 1-150 (optional).
//! Owner: U52 (ui doc §12.7 "Network extras").
//!
//! * [`provider`]: `ExtBuildListProvider` (the provider base class) and the dkjson value type.
//! * [`pob_archives`]: `PoBArchivesProvider` (pobarchives.com; DownloadPage / a POST sub-script).
//! * [`control`]: `ExtBuildListControl` and the global `ImportBuild` (Common.lua:1081-1105).
//! * [`similar_build_list`]: U20's `ShellHooks::similar_build_list` entry (Build.lua:1579-1599,
//!   inside U20's `open_similar_popup`; CCR-0104 §1).
//!
//! Reachability in PoB 2.67.2: BuildList mode creates the control only under `if false`
//! (BuildList.lua:20, :92; U18 skips `getPublicBuilds`), `buildMode:OpenSimilarPopup` has no
//! caller, and the control's `Draw` returns before drawing anything (`control` module notes).
#![allow(non_snake_case)] // Lua locals verbatim (calc doc §3.1 rule 1)

pub mod control;
pub mod pob_archives;
pub mod provider;

use std::rc::Rc;

use pob_ui_core::prelude::*;

pub use control::{ExtBuildListControl, ImportBuild, ProviderEntry};
pub use pob_archives::PoBArchivesProvider;
pub use provider::{ExtBuild, ExtBuildListProvider, ExtBuildListProviderClass, LVal, ProviderRef};

/// The similar-builds list of `buildMode:OpenSimilarPopup` in the popup `p` (U20's
/// `ShellHooks::similar_build_list`): the provider list, the control with its `shown` /
/// `height` / `width` overrides, `SetImportCode` of the build's code (`save_db` is U20's
/// `self:SaveDB("code")`) and `Init("PoB Archives")`. The lines are U20's range (its `@pob`
/// marker on `open_similar_popup`), so this carries `lua:` references only.
/// lua: Build.lua:1579-1599 (in buildMode:OpenSimilarPopup)
pub fn similar_build_list(cx: &mut Cx, p: CtrlId, save_db: &dyn Fn(&mut Cx) -> String) {
    // L1579-1584
    let buildProviders = vec![ProviderEntry {
        name: "PoB Archives".to_owned(),
        r#impl: PoBArchivesProvider::new("similar"),
    }];
    // L1585
    let width = 600.0;
    // L1586-1588
    let height = |cx: &Cx| cx.main.screen_h * 0.8;
    // L1589
    let padding = 50.0;
    // L1590
    let h0 = height(cx);
    let list = cx
        .add(
            p,
            "similarBuildList",
            ExtBuildListControl::new(
                Anchor::NONE,
                rect(0, padding, width, h0 - 2.0 * padding),
                buildProviders,
            ),
        )
        .id();
    // L1591
    cx.base_mut(list).shown = Prop::Val(true);
    // L1592-1594
    cx.base_mut(list).height = Prop::f(move |cx, _| height(cx) - 2.0 * padding);
    // L1595-1597
    cx.base_mut(list).width = Prop::f(move |_, _| width - padding);
    // L1598 SetImportCode(common.base64.encode(Deflate(self:SaveDB("code"))):gsub("+","-"):gsub("/","_"))
    let xml = save_db(cx);
    let code = crate::tabs::import::core::encode_build_code(&xml);
    ExtBuildListControl::set_import_code(cx, list, Some(code));
    // L1599
    ExtBuildListControl::init(cx, list, Some("PoB Archives"));
}

/// `listMode:getPublicBuilds()`: the "PoB Archives" list beside the build list, owned by the
/// list mode `list_mode`; the caller stores it as `self.controls.ExtBuildList` (BuildList.lua:21,
/// :93, both under `if false` in PoB 2.67.2, so U18 skips the function; this is its port for
/// when `main.showPublicBuilds` is re-enabled). The lines are U18's file, so this carries `lua:`
/// references only.
/// lua: BuildList.lua:111-127 listMode:getPublicBuilds()
pub fn get_public_builds(cx: &mut Cx, list_mode: CtrlId) -> CtrlId {
    // L112-117
    let buildProviders = vec![ProviderEntry {
        name: "PoB Archives".to_owned(),
        r#impl: PoBArchivesProvider::new("builds"),
    }];
    // L118
    let build_list = cx
        .child(list_mode, "buildList")
        .unwrap_or_else(|| lua_error!("attempt to index field 'buildList' (a nil value)"));
    let screen_w = cx.main.screen_w;
    let extBuildList = cx
        .add_free(
            Some(list_mode),
            ExtBuildListControl::new(
                Anchor::new(Left, build_list, Right),
                rect(25, 0, screen_w * 1.0 / 4.0 - 50.0, 0),
                buildProviders,
            ),
        )
        .id();
    // L119
    ExtBuildListControl::init(cx, extBuildList, Some("PoB Archives"));
    // L120-122
    cx.base_mut(extBuildList).height = Prop::f(|cx, _| cx.main.screen_h - 80.0);
    // L123-125
    cx.base_mut(extBuildList).width =
        Prop::f(|cx, _| pob_lua::math::m_max(cx.main.screen_w / 4.0 - 50.0, 400.0));
    // L126
    extBuildList
}

/// The hook as `ShellHooks` stores it.
pub fn similar_build_list_hook() -> Rc<dyn Fn(&mut Cx, CtrlId, &dyn Fn(&mut Cx) -> String)> {
    Rc::new(similar_build_list)
}
