//! VENDOR acceptance: the app runs on the asset bundle (`assets/`) alone, with no PoB checkout.
//!
//! * `e2e_scenarios_run_on_the_bundle`: the 15 U53 e2e scenarios (copied to
//!   `tests/standalone/scenarios`, the fixture user dir to `tests/standalone/user1`) run headless
//!   on the real `PobApp` with the Rust calc. The bundle is located as the desktop app locates it
//!   and loaded by `pob_desktop::init_bundle`. Per scenario: no error prompt, the `#! expect-text`
//!   lines in the last frame, every image the frames draw read from the bundle, and no asset
//!   read that fails except the [`KNOWN_MISSES`]. The scenarios open builds and switch every tab
//!   (tree, skills, items, calcs, config, notes, party, import/export, compare); the tree
//!   scenario must draw node icons from the skill sprite sheets.
//! * `every_tree_version_loads_its_images_from_the_bundle`: `main:LoadTree` of every tree version
//!   (images probed, not decoded): every image it asks for is in the bundle, and the bundle's tree
//!   images are exactly the ones asked for.
//! * `timeless_jewel_tables_load_from_the_bundle`: a `.zip`, a split `.zip.partN` and an abyss
//!   table load through `BundleJewelFs`, with the `.bin` cache written outside the bundle and read
//!   back.
//! * `launch_runs_as_a_release_on_the_bundle`: `launch:OnInit` on the desktop app's host
//!   configuration ([`pob_desktop::host_config`], the native platform) reads the bundle's
//!   `manifest.xml`: `launch.versionNumber` is PoB's 2.67.2, dev mode is off, and the user data
//!   goes to the platform's user folder, outside the bundle.
//! * `options_save_leaves_the_bundle_unchanged`: Options > Save in release mode
//!   (`main:SetManifestBranch`, which rewrites `manifest.xml`) on the desktop host's `NativeFs`
//!   over a copy of the bundle: the bundle is read-only, its manifest keeps its bytes.
//! * `bundle_matches_its_index` and `text_files_and_listings_resolve`: the index and the files
//!   agree (sizes and sha256); `NativeFs` reads and lists the logical tree.
//!
//! Run with `--test-threads=1` (the scenario test holds an app with decoded images). The proof
//! that nothing else is read: run this binary with the PoB checkout hidden (VENDOR's report).

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use pob_app::app::CxApp;
use pob_host::bundle::Bundle;
use pob_host::fs_native::BundleAssets;
use pob_sg::trace::ApiCall;
use pob_sg::{AssetSource, ImageFlags, ImageLoader, LoadEvent, TexId};

/// Image paths `PassiveTree:LoadImage` asks for that neither PoB's `TreeData/` nor
/// `TreeData/<ver>/` holds (the load fails in PoB too): recorded from the app on the PoB checkout
/// when the bundle was vendored.
const KNOWN_MISSES: [&str; 9] = [
    "TreeData/PassiveMasteryConnectedButton.png",
    "TreeData/ascendancy-3.png",
    "TreeData/ascendancy-background-3.jpg",
    "TreeData/background-3.png",
    "TreeData/frame-3.png",
    "TreeData/group-background-3.png",
    "TreeData/jewel-3.png",
    "TreeData/jewel-radius.png",
    "TreeData/line-3.png",
];

fn here() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/standalone")
}

/// The bundle, located and loaded once per process like the desktop app does it.
fn assets() -> PathBuf {
    static INIT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    INIT.get_or_init(|| {
        let a = pob_desktop::locate_assets().expect("asset bundle");
        assert!(
            !a.components().any(|c| c.as_os_str() == ".PathOfBuilding")
                && !a.join("Launch.lua").exists(),
            "the bundle is not a PoB checkout: {}",
            a.display()
        );
        let b = pob_desktop::init_bundle(&a).expect("init bundle");
        // the decompressed-LUT cache of the tests: a temp dir, not the user's cache
        let cache = std::env::temp_dir().join(format!("pob-standalone-lut-{}", std::process::id()));
        pob_gamedata::timeless_lut::set_jewel_fs(Arc::new(pob_desktop::BundleJewelFs::new(
            b,
            Some(cache),
        )));
        a
    })
    .clone()
}

fn bundle() -> Arc<Bundle> {
    Bundle::detect(&assets()).expect("bundle index")
}

/// Every asset read: (runtime?, path, ok).
type Log = Rc<RefCell<Vec<(bool, String, bool)>>>;

struct Recording {
    inner: BundleAssets,
    log: Log,
}

impl AssetSource for Recording {
    fn read_src(&self, path: &str) -> std::io::Result<Vec<u8>> {
        let r = self.inner.read_src(path);
        self.log
            .borrow_mut()
            .push((false, path.to_owned(), r.is_ok()));
        r
    }
    fn read_runtime(&self, path: &str) -> std::io::Result<Vec<u8>> {
        let r = self.inner.read_runtime(path);
        self.log
            .borrow_mut()
            .push((true, path.to_owned(), r.is_ok()));
        r
    }
}

/// The app on the bundle, as `pob-desktop` builds it, with headless services.
fn app(parts: pob_host::PlatformParts) -> pob_app::PobApp {
    let a = assets();
    let mut parts = parts;
    let state = pob_host::headless::HeadlessState::new(
        &a.to_string_lossy(),
        &a.parent().unwrap().to_string_lossy(),
    );
    let (p, f, n) = pob_host::headless::services(&state);
    parts.sys = pob_sg::Sys::new(p, f, n);
    pob_app::PobApp::new(parts)
}

fn expectations(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| l.trim().strip_prefix("#! expect-text "))
        .map(|t| t.trim().to_owned())
        .collect()
}

#[test]
fn e2e_scenarios_run_on_the_bundle() {
    let a = assets();
    let mut scripts: Vec<PathBuf> = std::fs::read_dir(here().join("scenarios"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "pobscript"))
        .collect();
    scripts.sort();
    assert_eq!(scripts.len(), 15, "the U53 e2e scenarios");
    let mut failures = Vec::new();
    let mut tree_icons = 0usize;
    for path in &scripts {
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        let text = std::fs::read_to_string(path).unwrap();
        let script = pob_host::script::Script::load(path).unwrap();
        let log: Log = Rc::default();
        let parts = pob_host::headless::parts(Rc::new(Recording {
            inner: BundleAssets::open(&a).unwrap(),
            log: log.clone(),
        }))
        .expect("fonts from the bundle");
        let mut app = app(parts);
        let frames = pob_host::run_headless(&mut app, &script);
        let mut fail = |m: String| failures.push(format!("{stem}: {m}"));
        if let Some(p) = app.cx.app().launch.prompt_msg.clone() {
            fail(format!("error prompt: {p:?}"));
        }
        let Some(last) = frames.last() else {
            fail("no frames".to_owned());
            continue;
        };
        for t in expectations(&text) {
            let found = last
                .calls
                .iter()
                .any(|c| matches!(&c.call, ApiCall::DrawString { text, .. } if text.contains(&t)));
            if !found {
                fail(format!("expect-text {t:?} not in the last frame"));
            }
        }
        // what the frames drew, and what was read
        let mut drawn: BTreeSet<String> = BTreeSet::new();
        for f in &frames {
            for c in &f.calls {
                match &c.call {
                    ApiCall::DrawImage {
                        img: Some(i), tc, ..
                    } => {
                        drawn.insert(i.path.clone());
                        if stem == "b_open_tree" && tc.is_some() && i.path.contains("/skills") {
                            tree_icons += 1;
                        }
                    }
                    ApiCall::DrawImageQuad { img: Some(i), .. } => {
                        drawn.insert(i.path.clone());
                    }
                    _ => {}
                }
            }
        }
        let log = log.borrow();
        let ok: BTreeSet<&str> = log
            .iter()
            .filter(|(rt, _, ok)| !rt && *ok)
            .map(|(_, p, _)| p.as_str())
            .collect();
        for d in &drawn {
            if !ok.contains(d.as_str()) && !KNOWN_MISSES.contains(&d.as_str()) {
                fail(format!("drew {d}, which was not read from the bundle"));
            }
        }
        for (rt, p, good) in log.iter() {
            if !good && !KNOWN_MISSES.contains(&p.as_str()) {
                fail(format!(
                    "{} read failed: {p}",
                    if *rt { "runtime" } else { "script" }
                ));
            }
        }
        let fonts = log.iter().filter(|(rt, _, good)| *rt && *good).count();
        eprintln!(
            "{stem}: {} frames, {} images drawn, {} script reads, {fonts} font files",
            frames.len(),
            drawn.len(),
            log.iter().filter(|(rt, _, _)| !rt).count()
        );
        if fonts < 100 {
            fail(format!("only {fonts} font files read from the bundle"));
        }
    }
    eprintln!("b_open_tree: {tree_icons} node icon draws from the skill sprite sheets");
    if tree_icons < 100 {
        failures.push(format!("b_open_tree: only {tree_icons} node icons drawn"));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Probes image files (header sizes from the bundle) without decoding them.
struct ProbeLoader {
    assets: BundleAssets,
    asked: Rc<RefCell<BTreeSet<(String, bool)>>>,
}

impl ImageLoader for ProbeLoader {
    fn request(
        &mut self,
        id: TexId,
        path: &str,
        _flags: ImageFlags,
        _sync: bool,
        _priority: i32,
    ) -> Option<LoadEvent> {
        let ok = self
            .assets
            .0
            .script_file(path)
            .is_some_and(|f| Path::new(&self.assets.0.root).join(f).is_file());
        self.asked.borrow_mut().insert((path.to_owned(), ok));
        Some(LoadEvent::Failed(id))
    }
    fn read_size(&mut self, _id: TexId, path: &str) -> Option<(u32, u32)> {
        let b = self.assets.read_src(path).ok()?;
        pob_sg::image::read_image_size(&b, path)
    }
    fn set_priority(&mut self, _id: TexId, _priority: i32) {}
    fn poll(&mut self) -> Vec<LoadEvent> {
        Vec::new()
    }
}

#[test]
fn every_tree_version_loads_its_images_from_the_bundle() {
    let a = assets();
    let asked: Rc<RefCell<BTreeSet<(String, bool)>>> = Rc::default();
    let images = pob_sg::ImageRegistry::new(Box::new(ProbeLoader {
        assets: BundleAssets::open(&a).unwrap(),
        asked: asked.clone(),
    }));
    let sg = pob_sg::Sg::new(Rc::new(pob_sg::FontSet::empty()), images);
    let parts = pob_host::PlatformParts {
        sg,
        sys: pob_host::headless::parts_without_assets().sys,
    };
    let mut app = app(parts);
    let _ = pob_host::run_headless(
        &mut app,
        &pob_host::script::Script::parse("frames 1").unwrap(),
    );
    let versions: Vec<String> = pob_gamedata::data()
        .gameVersions
        .treeVersionList
        .iter()
        .map(ToString::to_string)
        .collect();
    assert!(versions.len() >= 40, "{versions:?}");
    for v in &versions {
        let r = pob_host::guarded(&mut app, |app| {
            assert!(
                pob_app::tree_view::assets::main_load_tree(&mut app.cx, v).is_some(),
                "tree {v}"
            );
        });
        assert!(r.is_ok(), "{v}: {r:?}");
    }
    assert!(app.cx.app().launch.prompt_msg.is_none());
    let asked = asked.borrow();
    let missing: Vec<&str> = asked
        .iter()
        .filter(|(p, ok)| !ok && !KNOWN_MISSES.contains(&p.as_str()))
        .map(|(p, _)| p.as_str())
        .collect();
    assert!(missing.is_empty(), "not in the bundle: {missing:?}");
    let asked_tree: BTreeSet<&str> = asked
        .iter()
        .filter(|(p, ok)| *ok && p.starts_with("TreeData/"))
        .map(|(p, _)| p.as_str())
        .collect();
    let b = bundle();
    let bundled_tree: BTreeSet<&str> = b
        .script_files()
        .map(|(l, _)| l)
        .filter(|l| l.starts_with("TreeData/"))
        .collect();
    eprintln!(
        "{} tree versions: {} tree images asked for, {} bundled",
        versions.len(),
        asked_tree.len(),
        bundled_tree.len()
    );
    assert_eq!(
        asked_tree, bundled_tree,
        "the bundle's tree images are exactly the ones used"
    );
}

#[test]
fn timeless_jewel_tables_load_from_the_bundle() {
    use pob_gamedata::timeless_lut;
    let _ = assets();
    let cache = std::env::temp_dir().join(format!("pob-vendor-lut-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cache);
    timeless_lut::set_jewel_fs(Arc::new(pob_desktop::BundleJewelFs::new(
        bundle(),
        Some(cache.clone()),
    )));
    // sizes recorded from PoB's own files when the bundle was vendored
    let first = timeless_lut::loadJewelFile("HeroicTragedy", true).map(|b| b.len());
    assert_eq!(first, Some(3_587_054), "HeroicTragedy.zip");
    let bin = cache.join("HeroicTragedy.bin");
    assert_eq!(
        std::fs::metadata(&bin).map(|m| m.len()).ok(),
        Some(3_587_054),
        "the .bin cache"
    );
    let again = timeless_lut::loadJewelFile("HeroicTragedy", true).map(|b| b.len());
    assert_eq!(again, Some(3_587_054), "read back from the cache");
    let gv = timeless_lut::loadJewelFile("GloriousVanity", false).map(|b| b.len());
    assert_eq!(gv, Some(51_651_890), "GloriousVanity.zip.part0-4");
    let abyss = timeless_lut::loadJewelFile("AbyssAmanamu", false).map(|b| b.len());
    assert_eq!(abyss, Some(72_995_595), "AbyssAmanamu.zip.part0-5");
    assert!(
        !assets().join("timeless/HeroicTragedy.bin").exists(),
        "nothing is written into the bundle"
    );
    let _ = std::fs::remove_dir_all(&cache);
}

#[test]
fn launch_runs_as_a_release_on_the_bundle() {
    let cfg = pob_desktop::host_config(&assets());
    // the desktop app's platform (script and runtime paths, the user folder), with the headless
    // copy-on-write file system: nothing main:Init writes reaches the disk
    let platform =
        pob_host::platform_native::NativePlatform::new(cfg.script_dir.clone(), cfg.sg_runtime);
    let platform_user = pob_sg::Platform::user_path(&platform).expect("a user folder");
    let state = pob_host::headless::HeadlessState::new(&cfg.script_dir, "");
    let (_, fs, net) = pob_host::headless::services(&state);
    let mut parts = pob_host::headless::parts_without_assets();
    parts.sys = pob_sg::Sys::new(Box::new(platform), fs, net);
    let mut app = pob_app::PobApp::new(parts);
    // `launch:OnInit` as the desktop host calls it (`run_headless` would swap in the headless
    // platform and its user folder)
    let r = pob_host::guarded(&mut app, <pob_app::PobApp as pob_host::App>::on_init);
    assert!(r.is_ok(), "{r:?}");
    let l = &app.cx.app().launch;
    assert!(l.prompt_msg.is_none(), "{:?}", l.prompt_msg);
    assert_eq!(l.version_number.as_deref(), Some("2.67.2"), "PoB's version");
    assert_eq!(l.version_branch.as_deref(), Some("master"));
    assert_eq!(l.version_platform.as_deref(), Some("win32"));
    assert!(!l.dev_mode, "a release manifest: no dev mode");
    assert!(!l.installed_mode);
    let user = app.cx.app().main.user_path.clone().expect("main.userPath");
    assert_eq!(user, format!("{platform_user}/Path of Building/"));
    let bundle_root = bundle().root.clone();
    assert!(
        !user.starts_with(&bundle_root) && !user.starts_with(&cfg.script_dir),
        "the user data is outside the bundle: {user} (bundle {bundle_root})"
    );
}

/// Options > Save on the desktop app's file system (`NativeFs`) over a copy of the bundle:
/// with dev mode off, Main.lua:1221-1222 calls `main:SetManifestBranch`, whose
/// `xml.SaveXMLFile(…, "manifest.xml")` (Main.lua:1334) would rewrite the bundle's manifest
/// (`branch="beta"` here). The bundle is read-only, so the save fails (its error is ignored, as
/// Lua's is) and the bundle's bytes stay as they are; the user folder is written as usual.
/// The copy holds `index.json` and `text/` (the manifest); the rest of the bundle is linked.
#[cfg(unix)]
#[test]
fn options_save_leaves_the_bundle_unchanged() {
    use pob_ui_core::controls::ButtonControl;
    let a = assets();
    let tmp = std::env::temp_dir().join(format!("pob-standalone-ro-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let (root, user) = (tmp.join("assets"), tmp.join("user"));
    std::fs::create_dir_all(root.join("text")).unwrap();
    std::fs::create_dir_all(&user).unwrap();
    for e in std::fs::read_dir(&a).unwrap().flatten() {
        let name = e.file_name();
        let dst = root.join(&name);
        if name == "text" {
            for f in std::fs::read_dir(e.path()).unwrap().flatten() {
                std::fs::copy(f.path(), root.join("text").join(f.file_name())).unwrap();
            }
        } else if name == "index.json" {
            std::fs::copy(e.path(), &dst).unwrap();
        } else {
            std::os::unix::fs::symlink(e.path(), &dst).unwrap();
        }
    }
    let manifest = std::fs::read(root.join("text/manifest.xml")).unwrap();
    let cfg = pob_desktop::host_config(&root);
    // the headless platform with a temp user folder (GetRuntimePath() the bundle's parent, as
    // NativePlatform has it); the desktop host's NativeFs
    let state = pob_host::headless::HeadlessState::new(&cfg.script_dir, &tmp.to_string_lossy());
    *state.user_path.borrow_mut() = user.to_string_lossy().into_owned();
    let (platform, _, net) = pob_host::headless::services(&state);
    let fs = Box::new(pob_host::fs_native::NativeFs::new(&cfg.script_dir));
    let mut parts = pob_host::headless::parts_without_assets();
    parts.sys = pob_sg::Sys::new(platform, fs, net);
    let mut app = pob_app::PobApp::new(parts);
    let r = pob_host::guarded(&mut app, <pob_app::PobApp as pob_host::App>::on_init);
    assert!(r.is_ok(), "{r:?}");
    assert!(
        !app.cx.app().launch.dev_mode,
        "release mode: Save sets the branch"
    );
    // the first frames enter the build list (main:SetMode runs in OnFrame)
    let screen = pob_sg::ScreenInfo {
        fb_w: 1280,
        fb_h: 800,
        win_h: 800,
        dpi_scale: 1.0,
    };
    for _ in 0..2 {
        let r = pob_host::run_frame(&mut app, screen, (0, 0));
        assert!(r.is_ok(), "{:?}", r.err());
    }
    let l = &app.cx.app().launch;
    assert!(l.prompt_msg.is_none(), "{:?}", l.prompt_msg);
    let r = pob_host::guarded(&mut app, |app| {
        let cx = &mut app.cx;
        pob_app::main_mode::options_popup::open_options_popup(cx, None);
        cx.app_mut().main.beta_test = Some(true); // the "Beta test" check box
        let popup = cx.main.popups[0].id();
        let save = cx.child(popup, "save").expect("the Save button");
        let on_click = cx.w::<ButtonControl>(save).on_click.clone();
        on_click(cx);
    });
    assert!(r.is_ok(), "{r:?}");
    assert!(app.cx.main.popups.is_empty(), "Save closed the popup");
    let l = &app.cx.app().launch;
    assert!(l.prompt_msg.is_none(), "{:?}", l.prompt_msg);
    assert_eq!(
        std::fs::read(root.join("text/manifest.xml")).unwrap(),
        manifest,
        "the bundle's manifest.xml is unchanged"
    );
    assert_eq!(
        std::fs::read_dir(&root).unwrap().count(),
        std::fs::read_dir(&a).unwrap().count(),
        "nothing new in the bundle root"
    );
    assert!(
        user.join("Path of Building/Settings.xml").is_file(),
        "Save wrote the settings to the user folder"
    );
    drop(app);
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn bundle_matches_its_index() {
    let a = assets();
    let index: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(a.join("index.json")).unwrap()).unwrap();
    let files = index["files"].as_object().unwrap();
    for (f, e) in files {
        use sha2::Digest as _;
        // one file in memory at a time (the largest is a few tens of MB)
        let bytes = std::fs::read(a.join(f)).unwrap_or_else(|e| panic!("{f}: {e}"));
        assert_eq!(Some(bytes.len() as u64), e["bytes"].as_u64(), "{f}: size");
        let sha = sha2::Sha256::digest(&bytes)
            .iter()
            .fold(String::new(), |mut s, b| {
                use std::fmt::Write as _;
                let _ = write!(s, "{b:02x}");
                s
            });
        assert_eq!(Some(sha.as_str()), e["sha256"].as_str(), "{f}: sha256");
    }
    let b = bundle();
    for (l, f) in b.script_files().chain(b.runtime_files()) {
        assert!(files.contains_key(f), "{l} -> {f} is not a bundle file");
    }
    // every file on disk is indexed, apart from the docs and the index itself
    let mut on_disk = Vec::new();
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(root, &p, out);
            } else {
                out.push(
                    p.strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
        }
    }
    walk(&a, &a, &mut on_disk);
    let docs = [
        "README.md",
        "THIRD_PARTY_NOTICES.md",
        ".gitattributes",
        "index.json",
    ];
    let stray: Vec<&String> = on_disk
        .iter()
        .filter(|f| !docs.contains(&f.as_str()) && !files.contains_key(f.as_str()))
        .collect();
    assert!(stray.is_empty(), "files the index does not list: {stray:?}");
    assert_eq!(index["source"]["pobVersion"], "2.67.2");
    assert!(index["source"]["pobCommit"]
        .as_str()
        .unwrap()
        .starts_with("16de4b82d"));
    eprintln!("{} bundle files, {} on disk", files.len(), on_disk.len());
}

#[test]
fn text_files_and_listings_resolve() {
    use pob_sg::Fs;
    let a = assets();
    let fs = pob_host::fs_native::NativeFs::new(&a);
    let changelog = String::from_utf8(fs.read("changelog.txt").unwrap()).unwrap();
    assert!(changelog.starts_with("VERSION["), "{}", &changelog[..40]);
    assert!(fs.read("help.txt").is_ok());
    assert!(fs.read("Assets/ring.png").is_ok());
    assert!(
        fs.read("Launch.lua").is_err(),
        "no PoB source in the bundle"
    );
    let manifest = String::from_utf8(fs.read("manifest.xml").unwrap()).unwrap();
    assert!(
        manifest.contains(r#"<Version number="2.67.2" branch="master" platform="win32" />"#),
        "{manifest}"
    );
    assert_eq!(
        fs.read("../manifest.xml").ok(),
        fs.read("manifest.xml").ok(),
        "the dev-mode path names the same file"
    );
    let names = |v: Vec<pob_sg::FileEntry>| v.into_iter().map(|e| e.name).collect::<Vec<_>>();
    let latest = pob_gamedata::data()
        .gameVersions
        .latestTreeVersion
        .to_string();
    let files = names(fs.search(&format!("TreeData/{latest}/*"), false));
    assert!(files.iter().any(|n| n.starts_with("skills")), "{files:?}");
    let dirs = names(fs.search("TreeData/*", true));
    assert!(
        dirs.contains(&latest) && dirs.contains(&"legion".to_owned()),
        "{dirs:?}"
    );
    assert!(names(fs.search("Data/TimelessJewelData/*.zip", false)).len() >= 5);
}
