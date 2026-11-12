//! The desktop app (ui doc §3): `pob_host::run_desktop(cfg, pob_app::PobApp::new)`, whose calc is
//! the Rust calc (`RustCalc`, calc:PWR/CA). Feature `lua-calc` (dev only) runs PoB's Lua calc in a
//! second VM instead (pob-ui-oracle's LuaCalcBridge, ui doc D14, §10.4). Owner: U7; U53's cut-over
//! renamed the package from `pob-port-desktop` and wired the `lua-calc` injection. The
//! SimpleGraphic test card is `sg-demo`.

use std::path::PathBuf;

fn locate_pob_root() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("POB_ROOT") {
        return Some(PathBuf::from(p));
    }
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(PathBuf::from));
    let starts = [std::env::current_dir().ok(), exe_dir];
    for s in starts.into_iter().flatten() {
        for d in s.ancestors() {
            let p = d.join(".PathOfBuilding");
            if p.join("src/Launch.lua").is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// The compiled game data (`pob-dump compile`: `data.bin` and `trees/`): `$POB_DUMP_DIR`, else
/// the first `data/dump` with a `data.bin` above the working directory or the executable.
/// CCR-0106 §5 (U22's end-to-end gate): nothing initialised `pob_gamedata` in the desktop
/// binary, so Launch failed with "pob_gamedata::init not called".
fn locate_dump_dir() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("POB_DUMP_DIR") {
        return Some(PathBuf::from(p));
    }
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(PathBuf::from));
    let starts = [std::env::current_dir().ok(), exe_dir];
    for s in starts.into_iter().flatten() {
        for d in s.ancestors() {
            let p = d.join("data/dump");
            if p.join("data.bin").is_file() {
                return Some(p);
            }
        }
    }
    None
}

fn main() -> anyhow::Result<()> {
    let root = locate_pob_root().ok_or_else(|| {
        anyhow::anyhow!("PoB not found (set POB_ROOT to a .PathOfBuilding directory)")
    })?;
    // CCR-0106 §5
    let dump = locate_dump_dir().ok_or_else(|| {
        anyhow::anyhow!("game data not found (set POB_DUMP_DIR to a data/dump directory)")
    })?;
    pob_gamedata::init_from_dir(&dump)
        .map_err(|e| anyhow::anyhow!("{}: {e}", dump.join("data.bin").display()))?;
    let cfg = pob_host::HostConfig {
        title: "Path of Building".into(),
        size: (1280, 800),
        min_size: (800, 600),
        pob_src: root.join("src").to_string_lossy().into_owned(),
        sg_runtime: root
            .join("runtime/SimpleGraphic")
            .to_string_lossy()
            .into_owned(),
    };
    #[cfg(feature = "lua-calc")]
    {
        let src = root.join("src");
        pob_host::run_desktop(cfg, move |parts| {
            pob_app::PobApp::with_calc(
                parts,
                std::rc::Rc::new(move || {
                    Box::new(pob_ui_oracle::lua_calc::LuaCalcBridge::new(Some(src.clone())))
                }),
            )
        })
    }
    #[cfg(not(feature = "lua-calc"))]
    pob_host::run_desktop(cfg, pob_app::PobApp::new)
}
