//! The desktop app (ui doc §3): `pob_host::run_desktop(cfg, pob_app::PobApp::new)` with the command
//! line as `launch.arg`, whose calc is the Rust calc (`RustCalc`, calc:PWR/CA). Owner: U7; U53's
//! cut-over renamed the package from `pob-port-desktop`. The SimpleGraphic test card is
//! `sg-demo`.
//!
//! VENDOR: everything the app reads comes from the asset bundle (`assets/`, located by
//! [`pob_desktop::locate_assets`]): the script and runtime directories, the game data and the
//! timeless jewel files. No PoB checkout is read. (The dev-only `lua-calc` feature, which ran
//! PoB's Lua calc from a checkout, is gone with it; the oracle crates keep `LuaCalcBridge`.)

fn main() -> anyhow::Result<()> {
    let assets = pob_desktop::locate_assets()?;
    pob_desktop::init_bundle(&assets)?;
    let dir = assets.to_string_lossy().into_owned();
    let cfg = pob_host::HostConfig {
        title: "Path of Building".into(),
        size: (1280, 800),
        min_size: (800, 600),
        script_dir: dir.clone(),
        sg_runtime: dir,
    };
    // SimpleGraphic's `arg` global: the command line after the program name, e.g. the
    // `pob://pobbin/<id>` link a protocol handler passes (Main.lua:41-49, :71-83)
    let arg: Vec<String> = std::env::args().skip(1).collect();
    pob_host::run_desktop(cfg, move |parts| {
        let mut app = pob_app::PobApp::new(parts);
        with_arg(&mut app, arg);
        app
    })
}

/// `launch.arg` before `on_init` (Launch.lua:OnInit runs Main.lua, which reads it).
fn with_arg(app: &mut pob_app::PobApp, arg: Vec<String>) {
    use pob_app::app::CxApp;
    app.cx.app_mut().launch.arg = arg;
}
