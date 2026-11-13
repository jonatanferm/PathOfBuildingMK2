//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/PassiveTree.lua:75-794 (NewImageHandle/sprite blocks), 909-1040 (LoadImage, BuildConnector, BuildArc); src/Modules/Main.lua:352-364.
//! Owner: U23. BuildConnector/BuildArc and the connector loop are in `connectors.rs`.
//!
//! @pob-file Classes/PassiveTree.lua @16de4b82d
//! @pob-file Modules/Main.lua @16de4b82d
//!
//! The UI half of `PassiveTreeClass:PassiveTree(treeVersion)`: the calc plan's PT built the
//! tree model (`model::PassiveTree`; it names the blocks below in plain comments, the markers
//! are this file's, FS1); this file builds the image parts the Lua constructor interleaves with
//! it, after the model exists:
//!
//! | Lua lines | here |
//! |---|---|
//! | 234-258 (`self.assets`: the tree's own, else `require("TreeData.3_19.Assets")`) | [`TreeAssets::build`] |
//! | 260-264 (`LoadImage` for every asset) | [`TreeAssets::build`] |
//! | 266-329 (sprite sheets and `self.spriteMap`) | [`TreeAssets::build`] |
//! | 331-370 (bloodline sprites), 372-387 (alternate ascendancy emblems) | [`TreeAssets::build`] |
//! | 389-435 (legion sprite sheets) | [`TreeAssets::build`] |
//! | 626-645 (connector polygons) | `connectors::build_tree_connectors` |
//! | 748-753, 771-777, 876-885 (`node.sprites`, `effectSprites`, `masterySprites`) | [`TreeAssets::node_sprites`] and friends (read lazily, see below) |
//! | 909-935 `LoadImage` | [`load_image`] |
//! | Main.lua:352-364 `main:LoadTree` | [`main_load_tree`] |
//!
//! Data sources: `tree.lua`'s members (`assets`, `skillSprites`, `sprites`, `imageZoomLevels`,
//! `imageRoot`) come from the dumped `trees["<ver>"]` (the same bytes PT decodes,
//! `pob_gamedata::tree_bytes`); `TreeData/<ver>/sprites.lua`, `TreeData/3_19/Assets.lua` and
//! `TreeData/legion/tree-legion.lua`, which PoB reads with `io.open`/`require` (they are
//! `return { ... }` data literals), are the game data's tree files `<ver>.sprites`,
//! `3_19.assets` and `legion` (`pob_gamedata::tree_bytes` too: the dump's `treeSprites`,
//! `treeAssets` and `trees["legion"]` roots, compiled into the asset bundle's `gamedata/trees/`;
//! VENDOR: the app reads no PoB Lua file).
//!
//! Lua semantics kept:
//! * `copyTable(require("TreeData.3_19.Assets").assets, true)` is a shallow copy of a table the
//!   `require` cache keeps for the process: every 3.19+ tree without assets of its own shares the
//!   asset tables, so `LoadImage` of a later tree replaces `handle`/`width`/`height` in the tables
//!   an earlier tree holds too. [`Asset`] is shared by `Rc` with interior mutability, and the
//!   `require` result is cached per thread ([`REQUIRE_ASSETS_3_19`]).
//! * `unpack(data)` in `DrawAsset` (PassiveTreeView.lua:1310) passes the table's sequence part:
//!   the texture coordinates of a sprite, nothing for a `{ [zoom] = url }` asset, and the URL
//!   strings of the few assets that are URL lists ([`AssetSeq`]).
//!
//! DIVERGENCE(sprites-lazy): Lua stores `node.sprites`, `node.masterySprites` and
//! `node.effectSprites` on the node objects (PassiveTree.lua:748-753, 771-777, 876-885;
//! PassiveSpec.lua:285-286, 1709-1710). The model's `Node` has no slot for UI images, so they
//! are looked up when drawn, from `node.icon` / `node.activeIcon` / `node.inactiveIcon` /
//! `node.activeEffectImage` (which `ReplaceNode` copies, PassiveSpec.lua:1714-1716) and the
//! fallbacks of the assigning site ([`TreeAssets::node_sprites`]). The one case the lookup cannot
//! tell apart: a node replaced by a legion node whose icon has no sprite gets `{ }` in Lua
//! (:750-752) and the MasteryBlank fallback here; every legion icon has a sprite.
//! DIVERGENCE(load-order): PoB loads a spec's tree (and its images) when the spec is created
//! (PassiveSpec.lua:41 `main:LoadTree`). The calc plan's spec calls the model's
//! `passive_spec::main_LoadTree`, which has no UI images, so the images of a tree other than the
//! latest one are loaded by the first draw ([`tree_assets`]). Only the frame of the `ImageLoad`
//! side effects moves; nothing is drawn in between.
//! DIVERGENCE(download): `main.allowTreeDownload` is never set (PassiveTree.lua:97/918 "currently
//! disabled"), so the URL argument of `LoadImage` is not built.
//! ORDER: `pairs(self.assets)` (:262), `pairs(self.skillSprites)` (:296), `pairs(maxZoom.coords)`
//! (:311, :420), `pairs(bloodlineSpriteTypes)` (:357), `pairs(legionSprites)` (:416) iterate
//! string-keyed tables in the dump's order (string keys bytewise; VENDOR: the source order of the
//! `.lua` files before; PoB's order is random per process, and the values are the same, checked
//! over every sprites.lua, Assets.lua and tree-legion.lua when they were vendored). The only
//! observable effect is the order of the `ImageLoad` side effects (compared as a multiset by the
//! U23 test): every write is keyed and the keys of one loop are distinct. `next(spriteSet)`
//! (:381) picks the first entry of a `Classes*` sprite set; no tree version has a `Classes*`
//! sprite in more than one sprite type (checked over 3_25-3_29), so the pick is order-free (c).

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use pob_lua::{n, Name};
use pob_sg::{ImageFlags, ImageHandle};
use pob_ui_core::prelude::*;

use crate::app::CxApp;
use crate::model::gamedata::{DataTable, DataValue};
use crate::model::{self, Node, PassiveTreeRef, Val};

/// The asset and sprite maps: an `IndexMap` (insertion order, which the ORDER notes above rely
/// on) with ahash: the tree draw looks names up in them per node per frame, and std's SipHash
/// was a tenth of the draw (U53 perf pass).
pub type IndexMap<K, V> = indexmap::IndexMap<K, V, ahash::RandomState>;

pub use super::connectors::Connector;

/// One `self.assets[name]` or sprite table: `{ handle = .., width = .., height = .., [1..4] }`.
#[derive(Default)]
pub struct Asset {
    /// `data.handle` (nil until `LoadImage`).
    pub handle: RefCell<Option<ImageHandle>>,
    /// `data.width` / `data.height` (`NaN` = nil, before `LoadImage`).
    pub width: Cell<f64>,
    pub height: Cell<f64>,
    /// `data[1..]`, what `unpack(data)` passes.
    pub seq: AssetSeq,
}

/// The sequence part of an asset table.
#[derive(Default, Clone)]
pub enum AssetSeq {
    /// no `[1]`: `unpack(data)` passes nothing
    #[default]
    Empty,
    /// a sprite's texture coordinates `[1..4]`
    Tc([f64; 4]),
    /// anything else (the URL lists of a few `tree.lua` assets)
    Other(Vec<DataValue>),
}

impl Asset {
    fn from_data(t: &DataTable) -> Asset {
        Asset {
            handle: RefCell::new(None),
            width: Cell::new(f64::NAN),
            height: Cell::new(f64::NAN),
            seq: if t.seq.is_empty() {
                AssetSeq::Empty
            } else {
                AssetSeq::Other(t.seq.clone())
            },
        }
    }

    fn sprite(handle: Option<ImageHandle>, w: f64, h: f64, tc: [f64; 4]) -> Asset {
        Asset {
            handle: RefCell::new(handle),
            width: Cell::new(w),
            height: Cell::new(h),
            seq: AssetSeq::Tc(tc),
        }
    }

    /// `data.handle`.
    pub fn handle(&self) -> Option<ImageHandle> {
        self.handle.borrow().clone()
    }

    /// `asset[i]` for i in 1..=4 as a number (`nil` → `None`); a non-number raises where Lua
    /// would do arithmetic on it.
    pub fn index_num(&self, i: usize) -> Option<f64> {
        match &self.seq {
            AssetSeq::Empty => None,
            AssetSeq::Tc(tc) => tc.get(i - 1).copied(),
            AssetSeq::Other(v) => match v.get(i - 1) {
                None | Some(DataValue::Nil) => None,
                Some(DataValue::Num(x)) => Some(*x),
                Some(other) => lua_error!(
                    "attempt to perform arithmetic on field '?' (a {} value)",
                    data_type(other)
                ),
            },
        }
    }
}

fn data_type(v: &DataValue) -> &'static str {
    match v {
        DataValue::Nil => "nil",
        DataValue::Bool(_) => "boolean",
        DataValue::Num(_) => "number",
        DataValue::Str(_) => "string",
        DataValue::Fn(_) => "function",
        DataValue::Table(_) | DataValue::Ref(_) => "table",
    }
}

/// `self.spriteMap[name]`: sprite type → sprite.
pub type SpriteSet = IndexMap<String, Rc<Asset>>;

/// The UI images of one tree version (`main.tree[version]`'s `assets`, `spriteMap`,
/// `bloodlineSpritePrefixes` and `connectors`).
#[derive(Default)]
pub struct TreeAssets {
    /// `tree.treeVersion`
    pub tree_version: String,
    /// `treeVersions[treeVersion].num`
    pub version_num: f64,
    /// `self.assets`
    pub assets: IndexMap<String, Rc<Asset>>,
    /// `self.spriteMap`
    pub sprite_map: IndexMap<String, Rc<SpriteSet>>,
    /// `self.bloodlineSpritePrefixes`
    pub bloodline_sprite_prefixes: IndexMap<String, String>,
    /// `self.connectors` (PassiveTree.lua:627-645)
    pub connectors: Vec<Rc<Connector>>,
    /// The polygons of each cluster subgraph's `subGraph.connectors` (PassiveSpec.lua:2021-2025),
    /// keyed by that pair list's identity and length: PoB builds them once in `BuildSubgraph`;
    /// the port builds them on the first draw that meets the subgraph
    /// ([`TreeAssets::subgraph_connectors`]; rebuilding them every frame was most of the tree
    /// draw of cluster-jewel builds, U53 perf pass).
    pub subgraph_cache: RefCell<std::collections::HashMap<usize, SubgraphConnectors>>,
    /// `self.spriteMap[icon]` memoised per icon string object (its `Rc` address; the string is
    /// kept, so the address is not reused): the per-node lookups of every frame hash a pointer
    /// instead of the icon path (U53 perf pass).
    pub icon_cache: RefCell<std::collections::HashMap<usize, (model::LStr, Option<Rc<SpriteSet>>)>>,
}

/// One entry of [`TreeAssets::subgraph_cache`]: the pair list (kept alive, so its address is not
/// reused), its length when built, and the polygons.
pub struct SubgraphConnectors {
    pairs: model::Table,
    len: usize,
    connectors: Rc<Vec<Rc<Connector>>>,
}

thread_local! {
    /// `require("TreeData.3_19.Assets").assets`: Lua's `require` cache keeps one table per
    /// process, whose entries every 3.19+ tree shares (module notes).
    static REQUIRE_ASSETS_3_19: RefCell<Option<Rc<IndexMap<String, Rc<Asset>>>>> = const { RefCell::new(None) };
    /// `require("TreeData.legion.tree-legion")`
    static REQUIRE_LEGION: RefCell<Option<Arc<DataTable>>> = const { RefCell::new(None) };
}

/// `t[0.3835] or t[1]`
fn zoom_or_first(t: &DataTable) -> &DataValue {
    let v = t.index(0.3835);
    if v.truthy() {
        v
    } else {
        t.index(1.0)
    }
}

fn dtable(v: &DataValue) -> Option<&Arc<DataTable>> {
    v.table()
}

/// `dofile`/`require` of a tree data file (module notes): the file's `return` value, from the
/// game data's tree files (`pob_gamedata::tree_bytes(name)`; `lua` is the PoB path the Lua
/// reads, for the error message).
fn read_tree_data(name: &str, lua: &str) -> DataValue {
    let bytes = pob_gamedata::tree_bytes(name)
        .unwrap_or_else(|| lua_error!("module '{lua}' not found: no tree data file '{name}'"));
    let dump = pob_gamedata::loader::decode_bin(&bytes)
        .unwrap_or_else(|e| lua_error!("{lua}: tree data file '{name}': {e}"));
    dump.roots.values().next().cloned().unwrap_or_default()
}

fn require_assets_3_19() -> Rc<IndexMap<String, Rc<Asset>>> {
    if let Some(a) = REQUIRE_ASSETS_3_19.with(|r| r.borrow().clone()) {
        return a;
    }
    let root = read_tree_data(
        &pob_gamedata::data::tree_assets_file("3_19"),
        "TreeData/3_19/Assets.lua",
    );
    let assets = root.field("assets");
    let mut out = IndexMap::default();
    if let Some(t) = dtable(assets) {
        for (k, v) in &t.fields {
            if let Some(vt) = dtable(v) {
                out.insert(k.to_string(), Rc::new(Asset::from_data(vt)));
            }
        }
    }
    let out = Rc::new(out);
    REQUIRE_ASSETS_3_19.with(|r| *r.borrow_mut() = Some(out.clone()));
    out
}

fn require_legion() -> Arc<DataTable> {
    if let Some(t) = REQUIRE_LEGION.with(|r| r.borrow().clone()) {
        return t;
    }
    let root = read_tree_data(
        pob_gamedata::data::TREE_LEGION_FILE,
        "TreeData/legion/tree-legion.lua",
    );
    let t = dtable(&root)
        .cloned()
        .unwrap_or_else(|| lua_error!("TreeData/legion/tree-legion.lua: not a table"));
    REQUIRE_LEGION.with(|r| *r.borrow_mut() = Some(t.clone()));
    t
}

/// The dumped `tree.lua` table of a version (`assert(loadstring(treeText))()`).
fn tree_lua(version: &str) -> Arc<DataTable> {
    let bytes = pob_gamedata::tree_bytes(version)
        .unwrap_or_else(|| lua_error!("PassiveTree: no tree data for version '{version}'"));
    let dump = pob_gamedata::loader::decode_bin(&bytes)
        .unwrap_or_else(|e| lua_error!("PassiveTree: tree data for '{version}': {e}"));
    dump.roots
        .values()
        .next()
        .and_then(DataValue::table)
        .cloned()
        .unwrap_or_else(|| lua_error!("PassiveTree: tree data for '{version}' is not a table"))
}

/// `io.open(path, "r")` succeeds: the file names of a directory, listed once per load (the
/// listing is case-sensitive like `fopen` on the PoB hosts).
struct DirListing {
    files: std::collections::HashSet<String>,
}

impl DirListing {
    fn new(cx: &Cx, dir: &str) -> DirListing {
        let files = cx
            .sys
            .fs
            .search(&format!("{dir}/*"), false)
            .into_iter()
            .map(|e| e.name)
            .collect();
        DirListing { files }
    }
    fn has(&self, name: &str) -> bool {
        self.files.contains(name)
    }
}

/// The two directories `LoadImage` probes.
struct ImageDirs {
    top: DirListing,
    version: DirListing,
}

/// lua: PassiveTree.lua:909-935 `PassiveTreeClass:LoadImage(imgName, url, data, ...)`
/// @pob Classes/PassiveTree.lua:909-935 h=38ec75ce
fn load_image(
    cx: &mut Cx,
    dirs: &ImageDirs,
    tree_version: &str,
    img_name: &str,
    data: &Asset,
    flags: ImageFlags,
) {
    let mut img_name = img_name.to_owned();
    // L910 local imgFile = io.open("TreeData/"..imgName, "r")
    // L911 (`imgName` may hold a `/`: the legion sheets are loaded directly, not here)
    if dirs.top.has(&img_name) {
        // L912 imgFile:close()
    } else {
        // L914
        // L915
        if dirs.version.has(&img_name) {
            // L916-917
            img_name = format!("{tree_version}/{img_name}");
        }
        // L918-929 main.allowTreeDownload: never set (DIVERGENCE(download))
    }
    // L931 data.handle = NewImageHandle()
    let handle = cx.sg.new_image_handle();
    // L932 data.handle:Load("TreeData/"..imgName, ...)
    handle.load(&mut cx.sg, &format!("TreeData/{img_name}"), flags);
    // L933 data.width, data.height = data.handle:ImageSize()
    let (w, h) = handle.image_size(&mut cx.sg);
    *data.handle.borrow_mut() = Some(handle);
    data.width.set(f64::from(w));
    data.height.set(f64::from(h));
}

/// `name:match("[OL][ri][bn][ie][tC]")` (the Orbit*/Line* connector art loads without ASYNC).
fn is_connector_art(name: &str) -> bool {
    pob_lua::lmatch(name, "[OL][ri][bn][ie][tC]", None).is_some()
}

/// `s:gsub(p, r)` as the first result.
fn gsub1(s: &str, p: &str, r: &str) -> String {
    pob_lua::gsub(s, p, r, None).0
}

/// `type(v) == "table" and v[#v] or v` for a sprite coordinate.
fn last_or(v: &DataValue) -> f64 {
    match v {
        DataValue::Table(t) => t
            .seq
            .last()
            .and_then(DataValue::num)
            .unwrap_or_else(|| lua_error!("attempt to perform arithmetic on a nil value")),
        DataValue::Num(x) => *x,
        other => lua_error!(
            "attempt to perform arithmetic on a {} value",
            data_type(other)
        ),
    }
}

fn num_field(t: &DataValue, k: &str) -> f64 {
    t.field(k)
        .num()
        .unwrap_or_else(|| lua_error!("attempt to perform arithmetic on field '{k}' (a nil value)"))
}

impl TreeAssets {
    /// The image parts of `new("PassiveTree"):PassiveTree(treeVersion)` (module notes), for the
    /// model `tree` PT built.
    pub fn build(cx: &mut Cx, tree: &PassiveTreeRef) -> TreeAssets {
        let tree_version = tree.treeVersion.to_string();
        let version_num = cx
            .data()
            .gameVersions
            .treeVersions
            .get(&tree.treeVersion)
            .map_or_else(
                || lua_error!("attempt to index field '?' (a nil value)"),
                |v| v.num,
            );
        let raw = tree_lua(&tree_version);
        let mut me = TreeAssets {
            tree_version: tree_version.clone(),
            version_num,
            ..TreeAssets::default()
        };
        let dirs = ImageDirs {
            top: DirListing::new(cx, "TreeData"),
            version: DirListing::new(cx, &format!("TreeData/{tree_version}")),
        };
        me.build_assets(tree, &raw);
        me.load_assets(cx, &dirs);
        let mut sprite_sheets: IndexMap<String, Rc<Asset>> = IndexMap::default();
        let mut sprite_map: IndexMap<String, SpriteSet> = IndexMap::default();
        me.build_sprite_map(cx, &raw, &dirs, &mut sprite_sheets, &mut sprite_map);
        me.build_bloodlines(tree, &sprite_map);
        me.build_legion(cx, &mut sprite_sheets, &mut sprite_map);
        me.sprite_map = sprite_map
            .into_iter()
            .map(|(k, v)| (k, Rc::new(v)))
            .collect();
        me.connectors = super::connectors::build_tree_connectors(&me, tree);
        me
    }

    /// lua: PassiveTree.lua:234-258 (`self.assets`)
    /// @pob Classes/PassiveTree.lua:234-258 h=99af4dab
    fn build_assets(&mut self, tree: &PassiveTreeRef, raw: &DataTable) {
        // L234 if not self.assets then
        if let Some(own) = dtable(raw.field("assets")) {
            for (k, v) in &own.fields {
                if let Some(vt) = dtable(v) {
                    self.assets
                        .insert(k.to_string(), Rc::new(Asset::from_data(vt)));
                }
            }
            return;
        }
        // L235 self.assets = require("TreeData.3_19.Assets")
        let shared = require_assets_3_19();
        // L236 self.assets = copyTable(self.assets.assets, true)
        self.assets = (*shared).clone();
        // L237 if self.alternate_ascendancies then
        if tree.alternate_ascendancies.get().is_some() {
            // L238-256: `{[0.3835]=url}` tables (the URL is only for the disabled download)
            for name in [
                // backgrounds
                "ClassesPrimalist",
                "ClassesWarlock",
                "ClassesWarden",
                // ascendancy nodes
                "AzmeriAscendancyMiddle",
                "AzmeriAscendancyFrameLargeNormal",
                "AzmeriAscendancyFrameLargeCanAllocate",
                "AzmeriAscendancyFrameLargeAllocated",
                "AzmeriAscendancyFrameSmallNormal",
                "AzmeriAscendancyFrameSmallCanAllocate",
                "AzmeriAscendancyFrameSmallAllocated",
                // jewel sockets
                "AzmeriJewelFrameUnallocated",
                "AzmeriJewelFrameCanAllocate",
                "AzmeriJewelFrameAllocated",
                "CharmSocketActiveStr",
                "CharmSocketActiveInt",
                "CharmSocketActiveDex",
            ] {
                self.assets.insert(name.to_owned(), Rc::new(Asset::default()));
            }
        }
    }

    /// lua: PassiveTree.lua:260-264 (`LoadImage` for every asset)
    /// @pob Classes/PassiveTree.lua:260-264 h=281d2e21
    fn load_assets(&mut self, cx: &mut Cx, dirs: &ImageDirs) {
        // L260 local cdnRoot = ...: only for the disabled download
        // L261
        cx.sys.platform.con_print("Loading passive tree assets...");
        // L262 (ORDER: module notes)
        let list: Vec<(String, Rc<Asset>)> = self
            .assets
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        for (name, data) in list {
            // L263
            let flags = if is_connector_art(&name) {
                ImageFlags::empty()
            } else {
                ImageFlags::ASYNC
            };
            load_image(
                cx,
                dirs,
                &self.tree_version,
                &format!("{name}.png"),
                &data,
                flags,
            );
        }
    }

    /// lua: PassiveTree.lua:266-329 (sprite sheets and `self.spriteMap`)
    /// @pob Classes/PassiveTree.lua:266-329 h=9662b2d9
    fn build_sprite_map(
        &mut self,
        cx: &mut Cx,
        raw: &DataTable,
        dirs: &ImageDirs,
        sprite_sheets: &mut IndexMap<String, Rc<Asset>>,
        sprite_map: &mut IndexMap<String, SpriteSet>,
    ) {
        let version_num = self.version_num;
        // L267-268 self.spriteMap = { }; local spriteSheets = { }
        // L269 if not self.skillSprites then
        let skill_sprites: DataValue = if raw.field("skillSprites").truthy() {
            raw.field("skillSprites").clone()
        } else {
            // L270 if not self.sprites then
            let sprites = if raw.field("sprites").truthy() {
                raw.field("sprites").clone()
            } else {
                let display = cx
                    .data()
                    .gameVersions
                    .treeVersions
                    .get(&Name::from(self.tree_version.as_str()))
                    .map(|v| v.display.to_string())
                    .unwrap_or_default();
                // L271
                cx.sys.platform.con_print(&format!(
                    "Loading passive tree sprite data for version '{display}'..."
                ));
                // L272-289 io.open("TreeData/"..treeVersion.."/sprites.lua"); the json
                // conversion branch is never taken (every version ships sprites.lua)
                let sprite_text = read_tree_data(
                    &pob_gamedata::data::tree_sprites_file(&self.tree_version),
                    &format!("TreeData/{}/sprites.lua", self.tree_version),
                );
                // L290-292 for k, v in pairs(loadstring(spriteText)()) do self[k] = v end
                sprite_text.field("sprites").clone()
            };
            // L294 self.skillSprites = self.sprites
            sprites
        };
        let Some(skill_sprites) = dtable(&skill_sprites).cloned() else {
            lua_error!("bad argument #1 to 'for iterator' (table expected, got nil)")
        };
        let image_zoom_levels = raw.field("imageZoomLevels").truthy();
        let image_root = raw.field("imageRoot").as_str().map(str::to_owned);
        let _ = image_root; // L308: only the disabled download's URL uses it
                            // L296 (ORDER: module notes)
        for (sprite_type, data) in &skill_sprites.fields {
            let sprite_type = sprite_type.to_string();
            // L297-304
            let max_zoom: DataValue = if !image_zoom_levels {
                // L299
                data.clone()
            } else if version_num >= 3.19 {
                // L301
                match data {
                    DataValue::Table(t) => zoom_or_first(t).clone(),
                    _ => DataValue::Nil,
                }
            } else {
                // L303 data[#data]
                match data {
                    DataValue::Table(t) => t.seq.last().cloned().unwrap_or_default(),
                    _ => DataValue::Nil,
                }
            };
            // L305
            let filename = max_zoom
                .field("filename")
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| {
                    lua_error!("attempt to index local 'maxZoom' (filename is nil)")
                });
            let sheet = if let Some(sheet) = sprite_sheets.get(&filename) {
                sheet.clone()
            } else {
                // L306-308
                let sheet = Rc::new(Asset::default());
                let img_name = if version_num >= 3.16 {
                    gsub1(&gsub1(&filename, "%?%x+$", ""), ".*/", "")
                } else {
                    gsub1(&filename, "%?%x+$", "")
                };
                load_image(
                    cx,
                    dirs,
                    &self.tree_version,
                    &img_name,
                    &sheet,
                    ImageFlags::CLAMP,
                );
                // L309
                sprite_sheets.insert(filename.clone(), sheet.clone());
                sheet
            };
            // L311 (ORDER: module notes)
            let Some(coords) = dtable(max_zoom.field("coords")).cloned() else {
                lua_error!("bad argument #1 to 'for iterator' (table expected, got nil)")
            };
            for (name, c) in &coords.fields {
                // L312-314
                let set = sprite_map.entry(name.to_string()).or_default();
                // L315-318
                let x = last_or(c.field("x"));
                let y = last_or(c.field("y"));
                let w = last_or(c.field("w"));
                let h = last_or(c.field("h"));
                let (sw, sh) = (sheet.width.get(), sheet.height.get());
                // L319-327
                set.insert(
                    sprite_type.clone(),
                    Rc::new(Asset::sprite(
                        sheet.handle(),
                        w,
                        h,
                        [x / sw, y / sh, (x + w) / sw, (y + h) / sh],
                    )),
                );
            }
        }
    }

    /// lua: PassiveTree.lua:331-387 (bloodline sprites, alternate ascendancy emblems)
    /// @pob Classes/PassiveTree.lua:331-387 h=211d3c03
    fn build_bloodlines(&mut self, tree: &PassiveTreeRef, sprite_map: &IndexMap<String, SpriteSet>) {
        // L331-343
        let bloodline_sprite_types: [(&str, &str); 11] = [
            ("Trialmaster", "trialmasterBloodline"),
            ("Oshabi", "oshabiBloodline"),
            ("Olroth", "olrothBloodline"),
            ("Lycia", "lyciaBloodline"),
            ("KingInTheMists", "kingInTheMistsBloodline"),
            ("Farrul", "farrulBloodline"),
            ("Delirious", "deliriousBloodline"),
            ("Catarina", "catarinaBloodline"),
            ("Breachlord", "breachlordBloodline"),
            ("Aul", "aulBloodline"),
            ("Azmeri", "azmeriBloodline"),
        ];
        // L344-355
        let bloodline_asset_names = [
            "AscendancyButton",
            "AscendancyButtonHighlight",
            "AscendancyButtonPressed",
            "AscendancyFrameLargeNormal",
            "AscendancyFrameLargeCanAllocate",
            "AscendancyFrameLargeAllocated",
            "AscendancyFrameSmallNormal",
            "AscendancyFrameSmallCanAllocate",
            "AscendancyFrameSmallAllocated",
            "AscendancyMiddle",
        ];
        // L356 self.bloodlineSpritePrefixes = self.bloodlineSpritePrefixes or { }
        // L357 (ORDER: module notes)
        for (ascendancy_name, sprite_type) in bloodline_sprite_types {
            // L358
            let mut has_sprite = false;
            // L359
            for asset_name in bloodline_asset_names {
                // L360-361
                let sprite_data = sprite_map
                    .get(asset_name)
                    .and_then(|set| set.get(sprite_type));
                // L362
                if let Some(sprite_data) = sprite_data {
                    // L363
                    self.assets
                        .insert(format!("{ascendancy_name}{asset_name}"), sprite_data.clone());
                    // L364
                    has_sprite = true;
                }
            }
            // L367
            if has_sprite {
                // L368
                self.bloodline_sprite_prefixes
                    .insert(ascendancy_name.to_owned(), ascendancy_name.to_owned());
            }
        }

        // L372
        if tree.alternate_ascendancies.get().is_some() {
            // Use the bloodline sprite sheets for the remaining alternate ascendancy emblems
            // L374-378 legacyClasses
            let legacy_classes = ["ClassesPrimalist", "ClassesWarlock", "ClassesWarden"];
            // L379 (ORDER: module notes)
            for (sprite_name, sprite_set) in sprite_map {
                // L380
                if sprite_name.starts_with("Classes")
                    && !legacy_classes.contains(&sprite_name.as_str())
                    && !self.assets.contains_key(sprite_name)
                {
                    // L381 local _, sprite = next(spriteSet)
                    // L382
                    if let Some((_, sprite)) = sprite_set.first() {
                        // L383
                        self.assets.insert(sprite_name.clone(), sprite.clone());
                    }
                }
            }
        }
    }

    /// lua: PassiveTree.lua:389-435 (legion sprite sheets)
    /// @pob Classes/PassiveTree.lua:389-435 h=d0a65672
    fn build_legion(
        &mut self,
        cx: &mut Cx,
        sprite_sheets: &mut IndexMap<String, Rc<Asset>>,
        sprite_map: &mut IndexMap<String, SpriteSet>,
    ) {
        // L390 local legionSprites = require("TreeData.legion.tree-legion")
        let legion_sprites = require_legion();
        // L391-401 local function loadLegionSheet(data)
        let mut load_legion_sheet = |cx: &mut Cx, data: &DataValue| -> Rc<Asset> {
            let filename = data
                .field("filename")
                .as_str()
                .unwrap_or_else(|| lua_error!("attempt to concatenate field 'filename' (a nil value)"))
                .to_owned();
            if let Some(sheet) = sprite_sheets.get(&filename) {
                return sheet.clone();
            }
            // L394-397
            let sheet = Rc::new(Asset::default());
            let handle = cx.sg.new_image_handle();
            handle.load(
                &mut cx.sg,
                &format!("TreeData/legion/{filename}"),
                ImageFlags::empty(),
            );
            let (w, h) = handle.image_size(&mut cx.sg);
            *sheet.handle.borrow_mut() = Some(handle);
            sheet.width.set(f64::from(w));
            sheet.height.set(f64::from(h));
            // L398
            sprite_sheets.insert(filename, sheet.clone());
            sheet
        };
        let coord_sprite = |sheet: &Asset, c: &DataValue| -> Rc<Asset> {
            let (x, y, w, h) = (
                num_field(c, "x"),
                num_field(c, "y"),
                num_field(c, "w"),
                num_field(c, "h"),
            );
            let (sw, sh) = (sheet.width.get(), sheet.height.get());
            Rc::new(Asset::sprite(
                sheet.handle(),
                w,
                h,
                [x / sw, y / sh, (x + w) / sw, (y + h) / sh],
            ))
        };
        // L402
        if let Some(tree_assets) = dtable(legion_sprites.field("treeAssets")) {
            for data in &tree_assets.seq {
                // L403
                let sheet = load_legion_sheet(cx, data);
                // L404 (ORDER: module notes)
                if let Some(coords) = dtable(data.field("coords")) {
                    for (name, c) in &coords.fields {
                        // L405-413
                        self.assets
                            .insert(name.to_string(), coord_sprite(&sheet, c));
                    }
                }
            }
        }
        // L416 (ORDER: module notes)
        for (ty, data) in &legion_sprites.fields {
            // L417
            if &**ty != "treeAssets" {
                // L418 local maxZoom = data[#data]
                let max_zoom = dtable(data)
                    .and_then(|t| t.seq.last().cloned())
                    .unwrap_or_default();
                // L419
                let sheet = load_legion_sheet(cx, &max_zoom);
                // L420
                if let Some(coords) = dtable(max_zoom.field("coords")) {
                    for (name, c) in &coords.fields {
                        // L421-432
                        let set = sprite_map.entry(name.to_string()).or_default();
                        set.insert(ty.to_string(), coord_sprite(&sheet, c));
                    }
                }
            }
        }
    }

    /// `subGraph.connectors` as polygons ([`super::connectors::build_subgraph_connectors`],
    /// memoised per pair list; see [`TreeAssets::subgraph_cache`]).
    pub fn subgraph_connectors(&self, sub_graph: &model::Table) -> Rc<Vec<Rc<Connector>>> {
        let Some(pairs) = sub_graph.table(n!("connectors")) else {
            return Rc::default();
        };
        let key = pairs.as_ptr() as usize;
        let len = pairs.len();
        if let Some(e) = self.subgraph_cache.borrow().get(&key) {
            if e.len == len && e.pairs.ptr_eq(&pairs) {
                return e.connectors.clone();
            }
        }
        let connectors = Rc::new(super::connectors::build_subgraph_connectors(self, sub_graph));
        let mut cache = self.subgraph_cache.borrow_mut();
        // subgraphs come and go with the jewels: forget the old ones now and then
        if cache.len() >= 256 {
            cache.clear();
        }
        cache.insert(
            key,
            SubgraphConnectors {
                pairs,
                len,
                connectors: connectors.clone(),
            },
        );
        connectors
    }

    /// `tree.assets[name]`
    pub fn asset(&self, name: &str) -> Option<&Rc<Asset>> {
        self.assets.get(name)
    }

    /// `tree.assets[name].handle` (CCR-0099 A).
    pub fn asset_handle(&self, name: &str) -> Option<ImageHandle> {
        self.assets.get(name).and_then(|a| a.handle())
    }

    /// `self.spriteMap[name]`
    pub fn sprites(&self, name: Option<&str>) -> Option<Rc<SpriteSet>> {
        self.sprite_map.get(name?).cloned()
    }

    /// [`TreeAssets::sprites`] of a node's icon string, memoised ([`TreeAssets::icon_cache`]).
    fn icon_sprites(&self, icon: Option<&model::LStr>) -> Option<Rc<SpriteSet>> {
        let icon = icon?;
        let key = Rc::as_ptr(icon).cast::<u8>() as usize;
        if let Some((s, v)) = self.icon_cache.borrow().get(&key) {
            if Rc::ptr_eq(s, icon) {
                return v.clone();
            }
        }
        let v = self.sprites(Some(icon));
        let mut cache = self.icon_cache.borrow_mut();
        // a tree has a few thousand icon strings; replaced nodes add a few more
        if cache.len() >= 16384 {
            cache.clear();
        }
        cache.insert(key, (icon.clone(), v.clone()));
        v
    }

    /// `node.sprites` (DIVERGENCE(sprites-lazy) in the module notes): what the last of the
    /// assigning sites gave the node.
    /// lua: PassiveTree.lua:748-753 (legion nodes), 771-777 (tattoo nodes), 876-885
    /// (ProcessNode); PassiveSpec.lua:286, 1709 (ReplaceNode)
    /// @pob Classes/PassiveTree.lua:876-885 h=210e9140
    pub fn node_sprites(&self, node: &Node) -> Option<Rc<SpriteSet>> {
        let icon = node.extra.str(n!("icon"));
        // a node replaced by a tattoo override (PassiveSpec.lua:286 `override.sprites =
        // self.tree.spriteMap[override.icon]`, copied by ReplaceNode :1709): no fallback
        if node.isTattoo.get() {
            return self.icon_sprites(icon.as_ref());
        }
        // L877 if node.type == "Mastery" and node.masteryEffects then (masterySprites only)
        let mastery_with_effects =
            &*node.r#type.get() == "Mastery" && node.masteryEffects.get().is_some();
        // L880 else node.sprites = self.spriteMap[node.icon]
        let sprites = if mastery_with_effects {
            None
        } else {
            self.icon_sprites(icon.as_ref())
        };
        // L882-884 if not node.sprites then node.sprites = spriteMap[MasteryBlank]
        sprites.or_else(|| self.sprites(Some("Art/2DArt/SkillIcons/passives/MasteryBlank.png")))
    }

    /// `node.masterySprites` (PassiveTree.lua:877-879): `{ activeIcon, inactiveIcon,
    /// activeEffectImage }` for a mastery with effects, else nil.
    pub fn node_mastery_sprites(&self, node: &Node) -> Option<MasterySprites> {
        if &*node.r#type.get() != "Mastery" || node.masteryEffects.get().is_none() {
            return None;
        }
        Some(MasterySprites {
            active_icon: self.sprites(node.extra.str(n!("activeIcon")).as_deref()),
            inactive_icon: self.sprites(node.extra.str(n!("inactiveIcon")).as_deref()),
            active_effect_image: self.sprites(node.extra.str(n!("activeEffectImage")).as_deref()),
        })
    }

    /// `node.effectSprites`: a tattoo's (PassiveTree.lua:772; PassiveSpec.lua:285, copied by
    /// ReplaceNode :1710), nil for every other node.
    /// @pob Classes/PassiveTree.lua:771-777 h=567aee52
    pub fn node_effect_sprites(&self, node: &Node) -> Option<Rc<SpriteSet>> {
        if !node.isTattoo.get() {
            return None;
        }
        self.sprites(node.extra.str(n!("activeEffectImage")).as_deref())
    }
}

/// `node.masterySprites`
pub struct MasterySprites {
    pub active_icon: Option<Rc<SpriteSet>>,
    pub inactive_icon: Option<Rc<SpriteSet>>,
    pub active_effect_image: Option<Rc<SpriteSet>>,
}

// @pob Classes/PassiveTree.lua:748-753 h=fcbd6827
// (legion nodes' `node.sprites = self.spriteMap[node.icon] or { }`: [`TreeAssets::node_sprites`],
// DIVERGENCE(sprites-lazy))

/// lua: Main.lua:352-364 `main:LoadTree(treeVersion)`: the model tree (calc:PT, through the
/// model's own `main.tree` cache, `passive_spec::main_LoadTree`) and its UI images
/// (`cx.app().trees`).
/// @pob Modules/Main.lua:352-364 h=c9c5ea47
pub fn main_load_tree(cx: &mut Cx, tree_version: &str) -> Option<PassiveTreeRef> {
    // L353 if self.tree[treeVersion] then
    if let Some((tree, _)) = cx.app().trees.trees.get(tree_version).cloned() {
        // L354
        model_set_jewel_radii(tree_version);
        // L355
        return Some(tree);
    }
    // L356 elseif isValueInTable(treeVersionList, treeVersion) then
    if cx
        .data()
        .gameVersions
        .treeVersionList
        .iter()
        .any(|v| &**v == tree_version)
    {
        // L357
        model_set_jewel_radii(tree_version);
        // L359 self.tree[treeVersion] = new("PassiveTree"):PassiveTree(treeVersion)
        let tree = pob_calc::classes::passive_spec::main_LoadTree(Name::from(tree_version))?; // CCR-0115
        let assets = Rc::new(TreeAssets::build(cx, &tree));
        cx.app_mut()
            .trees
            .trees
            .insert(tree_version.to_owned(), (tree.clone(), assets));
        // L360
        return Some(tree);
    }
    // L362
    None
}

/// `data.setJewelRadiiGlobally(treeVersion)` (Data.lua:583-600, calc:PT).
fn model_set_jewel_radii(tree_version: &str) {
    pob_calc::classes::passive_tree::setJewelRadiiGlobally(tree_version); // CCR-0115
}

/// The UI images of `tree` (`tree.assets`, `tree.spriteMap`, ...): `main.tree[version]`'s,
/// built now when the model loaded the tree without the UI (DIVERGENCE(load-order)).
pub fn tree_assets(cx: &mut Cx, tree: &PassiveTreeRef) -> Rc<TreeAssets> {
    let version = tree.treeVersion.to_string();
    if let Some((t, a)) = cx.app().trees.trees.get(&version) {
        if Rc::ptr_eq(t, tree) {
            return a.clone();
        }
    }
    let assets = Rc::new(TreeAssets::build(cx, tree));
    cx.app_mut()
        .trees
        .trees
        .insert(version, (tree.clone(), assets.clone()));
    assets
}

/// `main.tree[latestTreeVersion]`'s UI images, if loaded.
pub fn latest_tree_assets(cx: &Cx) -> Option<Rc<TreeAssets>> {
    let latest = cx.data().gameVersions.latestTreeVersion.to_string();
    cx.app().trees.trees.get(&latest).map(|(_, a)| a.clone())
}

/// `Val` → node (`Val::Obj(NodeRef)` in dynamic tables, CCR-0024 §4).
pub fn val_node(v: &Val) -> Option<model::NodeRef> {
    v.obj::<Node>()
}
