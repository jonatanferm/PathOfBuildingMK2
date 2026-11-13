//! Web [`Fs`] over IndexedDB (ui doc §5.2; ideas from `crates/pob-ui/src/build_store_wasm.rs`;
//! wasm32 only). Owner: U7.
//!
//! The frozen [`Fs`] is synchronous and IndexedDB is not, so the file tree lives in memory
//! ([`crate::MemFs`], native error behaviour) and IndexedDB is its write-behind store:
//! [`WebFs::open`] reads every record before `on_init`, and each change is queued as one
//! `put`/`delete` transaction (IndexedDB runs transactions on one store in creation order, so the
//! stored tree follows the in-memory one). Store `files` in database [`DB_NAME`]: key = absolute
//! path, value = `{ dir: bool, data: Uint8Array | undefined, modified: number }`, as
//! `build_store_wasm.rs` keeps its payloads.
//!
//! Reads of files the tree does not hold fall back to the fetched VFS of PoB's `src/`
//! (`io.open` of a script-relative path; SG has `chdir`ed into the script directory), and a
//! search (`NewFileSearch`) of a directory under `src/` lists the VFS's files there too: on
//! desktop both see the same disk. (Without the second, the tree view's `LoadImage` probe of
//! `TreeData/<version>/` found nothing, so every sprite sheet of the version directory was
//! requested as `TreeData/<sheet>`, which the VFS does not hold: the sync loads failed and the
//! node icons drew as the default texture's grey, U53.)
//!
//! DIVERGENCE(web storage): a write that reaches IndexedDB fails only asynchronously (quota,
//! private mode); the error is logged and the in-memory tree keeps the change.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use pob_sg::{FileEntry, Fs};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{IdbDatabase, IdbOpenDbRequest, IdbRequest, IdbTransactionMode};

use crate::{MemFs, MemFsChange};

/// The IndexedDB database.
pub const DB_NAME: &str = "pob_port_fs";
const DB_VERSION: u32 = 1;
const STORE: &str = "files";

/// Fetched files keyed by their path relative to PoB's `src/` (`/` separators).
pub type Vfs = Rc<RefCell<HashMap<String, Vec<u8>>>>;

/// Resolve when `req` succeeds (its `result`) or fails.
pub(crate) async fn idb_done(req: &IdbRequest) -> Result<JsValue, JsValue> {
    let req2 = req.clone();
    let p = js_sys::Promise::new(&mut |resolve, reject| {
        let r = req2.clone();
        let ok = Closure::once_into_js(move |_e: web_sys::Event| {
            let _ = resolve.call1(&JsValue::NULL, &r.result().unwrap_or(JsValue::UNDEFINED));
        });
        let err = Closure::once_into_js(move |_e: web_sys::Event| {
            let _ = reject.call1(
                &JsValue::NULL,
                &JsValue::from_str("IndexedDB request failed"),
            );
        });
        req2.set_onsuccess(Some(ok.unchecked_ref()));
        req2.set_onerror(Some(err.unchecked_ref()));
    });
    JsFuture::from(p).await
}

async fn open_db() -> Result<IdbDatabase, JsValue> {
    let w = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
    let factory = w
        .indexed_db()?
        .ok_or_else(|| JsValue::from_str("IndexedDB unavailable"))?;
    let open: IdbOpenDbRequest = factory.open_with_u32(DB_NAME, DB_VERSION)?;
    let o2 = open.clone();
    let upgrade = Closure::once_into_js(move |_e: web_sys::Event| {
        if let Ok(db) = o2.result().and_then(|r| r.dyn_into::<IdbDatabase>()) {
            if !db.object_store_names().contains(STORE) {
                let _ = db.create_object_store(STORE);
            }
        }
    });
    open.set_onupgradeneeded(Some(upgrade.unchecked_ref()));
    let db = idb_done(&open).await?;
    db.dyn_into::<IdbDatabase>()
}

/// Every record: `(path, data or None for a directory, modified)`.
async fn read_all(db: &IdbDatabase) -> Result<Vec<(String, Option<Vec<u8>>, i64)>, JsValue> {
    let tx = db.transaction_with_str(STORE)?;
    let store = tx.object_store(STORE)?;
    let keys = js_sys::Array::from(&idb_done(&store.get_all_keys()?).await?);
    let values = js_sys::Array::from(&idb_done(&store.get_all()?).await?);
    let mut out = Vec::new();
    for (k, v) in keys.iter().zip(values.iter()) {
        let Some(path) = k.as_string() else { continue };
        let dir = js_sys::Reflect::get(&v, &"dir".into())
            .ok()
            .and_then(|d| d.as_bool())
            .unwrap_or(false);
        let modified = js_sys::Reflect::get(&v, &"modified".into())
            .ok()
            .and_then(|m| m.as_f64())
            .unwrap_or(0.0) as i64;
        let data = if dir {
            None
        } else {
            let d = js_sys::Reflect::get(&v, &"data".into()).unwrap_or(JsValue::UNDEFINED);
            Some(js_sys::Uint8Array::new(&d).to_vec())
        };
        out.push((path, data, modified));
    }
    Ok(out)
}

/// Queue one change as its own transaction (fire and forget; failures are logged).
fn persist(db: &IdbDatabase, change: &MemFsChange) {
    let run = || -> Result<IdbRequest, JsValue> {
        let tx = db.transaction_with_str_and_mode(STORE, IdbTransactionMode::Readwrite)?;
        let store = tx.object_store(STORE)?;
        match change {
            MemFsChange::Put {
                path,
                data,
                modified,
            } => {
                let v = js_sys::Object::new();
                js_sys::Reflect::set(&v, &"dir".into(), &JsValue::from_bool(data.is_none()))?;
                if let Some(d) = data {
                    let arr = js_sys::Uint8Array::from(d.as_slice());
                    js_sys::Reflect::set(&v, &"data".into(), &arr)?;
                }
                js_sys::Reflect::set(&v, &"modified".into(), &JsValue::from_f64(*modified as f64))?;
                store.put_with_key(&v, &JsValue::from_str(path))
            }
            MemFsChange::Delete { path } => store.delete(&JsValue::from_str(path)),
        }
    };
    match run() {
        Ok(req) => {
            let path = match change {
                MemFsChange::Put { path, .. } | MemFsChange::Delete { path } => path.clone(),
            };
            wasm_bindgen_futures::spawn_local(async move {
                if idb_done(&req).await.is_err() {
                    web_sys::console::error_1(&format!("IndexedDB: saving {path} failed").into());
                }
            });
        }
        Err(e) => web_sys::console::error_1(&e),
    }
}

/// `GetFileModifiedTime` ticks (100 ns since 1601) of `Date.now()`.
fn now_ticks() -> i64 {
    const UNIX_TO_1601: i64 = 116_444_736_000_000_000;
    UNIX_TO_1601 + (js_sys::Date::now() * 10_000.0) as i64
}

/// The browser's file system: [`MemFs`] persisted into IndexedDB.
pub struct WebFs {
    pub tree: MemFs,
    /// The fetched files of PoB's `src/` (read-only; see the module docs).
    vfs: Vfs,
    /// `src/`'s absolute path with a trailing `/` (the VFS keys are relative to it).
    vfs_prefix: String,
}

impl WebFs {
    /// Open the database, load the stored tree, make sure `user_path` exists, and resolve
    /// script-relative reads of absent files from `vfs` (keys relative to `script_path`).
    /// Without IndexedDB (private mode, blocked storage) the tree lives in memory only.
    pub async fn open(script_path: &str, user_path: &str, vfs: Vfs) -> WebFs {
        let mut tree = MemFs::new(script_path);
        let db = match open_db().await {
            Ok(db) => Some(db),
            Err(e) => {
                web_sys::console::warn_2(&"IndexedDB unavailable, files are not kept:".into(), &e);
                None
            }
        };
        if let Some(db) = &db {
            match read_all(db).await {
                Ok(records) => {
                    for (p, d, m) in records {
                        tree.restore(&p, d, m);
                    }
                }
                Err(e) => web_sys::console::error_1(&e),
            }
        }
        tree.clock = Box::new(now_ticks);
        if let Some(db) = db {
            tree.on_change = Box::new(move |c| persist(&db, &c));
        }
        let prefix = format!("{}/", tree.normalize(""));
        let (fallback_vfs, fallback_prefix) = (vfs.clone(), prefix.clone());
        tree.fallback = Box::new(move |p| {
            let rel = p.strip_prefix(&fallback_prefix)?;
            fallback_vfs.borrow().get(rel).cloned()
        });
        tree.make_dir_all(user_path);
        WebFs {
            tree,
            vfs,
            vfs_prefix: prefix,
        }
    }

    /// The VFS entries a search of `spec` lists (files, or the directories the keys imply),
    /// except those named in `have` (the tree's own entries win).
    fn vfs_search(&self, spec: &str, find_dirs: bool, have: &[FileEntry]) -> Vec<FileEntry> {
        let Some((dir, pattern)) = crate::search_split_spec(spec) else {
            return Vec::new();
        };
        let abs = format!("{}/", self.tree.normalize(dir).trim_end_matches('/'));
        // (`src/` itself gives "")
        let Some(rel) = abs.strip_prefix(&self.vfs_prefix) else {
            return Vec::new();
        };
        let mut out: Vec<FileEntry> = Vec::new();
        for (key, data) in self.vfs.borrow().iter() {
            let Some(rest) = key.strip_prefix(rel) else { continue };
            let (name, is_dir) = match rest.split_once('/') {
                Some((d, _)) => (d, true),
                None => (rest, false),
            };
            if name.is_empty()
                || is_dir != find_dirs
                || !crate::glob_match(pattern, name)
                || have.iter().any(|e| e.name == name)
                || out.iter().any(|e| e.name == name)
            {
                continue;
            }
            out.push(FileEntry {
                name: name.to_owned(),
                is_dir,
                size: if is_dir { 0 } else { data.len() as u64 },
                modified: 0,
            });
        }
        out
    }
}

impl Fs for WebFs {
    fn read(&self, path: &str) -> std::io::Result<Vec<u8>> {
        self.tree.read(path)
    }
    fn write(&mut self, path: &str, data: &[u8]) -> std::io::Result<()> {
        self.tree.write(path, data)
    }
    fn search(&self, spec: &str, find_dirs: bool) -> Vec<FileEntry> {
        let mut out = self.tree.search(spec, find_dirs);
        let extra = self.vfs_search(spec, find_dirs, &out);
        if !extra.is_empty() {
            out.extend(extra);
            crate::sort_file_entries(&mut out);
        }
        out
    }
    fn make_dir(&mut self, path: &str) -> std::io::Result<()> {
        self.tree.make_dir(path)
    }
    fn remove_dir(&mut self, path: &str, recursive: bool) -> std::io::Result<()> {
        self.tree.remove_dir(path, recursive)
    }
    fn rename(&mut self, from: &str, to: &str) -> std::io::Result<()> {
        self.tree.rename(from, to)
    }
    fn remove(&mut self, path: &str) -> std::io::Result<()> {
        self.tree.remove(path)
    }
}
