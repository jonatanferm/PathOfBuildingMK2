//! Web [`ImageLoader`] (ui doc §4.5 "Web", §5.2 "Web host"; wasm32 only). Owner: U3.
//!
//! `image_size` cannot block on the web, so the host prefetches all synchronously loaded assets
//! (fonts, `Assets/*`, the active tree version's sprite sheets) into a VFS before `on_init` and
//! before a tree version switch ([`WebLoader::prefetch`]); only ASYNC images are fetched lazily,
//! and at most one image is decoded per frame ([`ImageLoader::poll`]).
//!
//! Counterpart of SimpleGraphic `3b1a346` r_texture.cpp `t_manager_c` (`AsyncAdd`, `ThreadProc`'s
//! highest-`loadPri`-first choice) with the browser's `fetch` in place of the file system and the
//! frame loop in place of the worker threads. Decoding is `pob_sg::image::decode_image`.
//!
//! Divergences (inherent to the platform, ui doc §13 item 6):
//! - a non-ASYNC load of a file that was not prefetched fails (default texture, size `(0,0)`)
//!   instead of blocking; // DIVERGENCE(web: sync loads need the prefetch)
//! - `ImageSize` of an ASYNC image whose fetch has not finished returns `(0,0)` (SG blocks until
//!   the header is parsed). PoB re-queries a zero width before drawing tree assets
//!   (PassiveTreeView.lua:548-550, :1290-1295), so they appear once fetched.
//!   // DIVERGENCE(web: ImageSize does not block on a lazy fetch)

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use pob_sg::image::{decode_image, read_image_size};
use pob_sg::{ImageFlags, ImageLoader, LoadEvent, TexId};

/// Fetched files keyed by their path relative to PoB's `src/` (`/` separators).
pub type Vfs = Rc<RefCell<HashMap<String, Vec<u8>>>>;

/// Where a queued ASYNC load stands.
#[derive(Clone, Copy, PartialEq, Eq)]
enum JobState {
    /// The bytes are being fetched.
    Fetching,
    /// The bytes are in the VFS: decode when picked.
    Ready,
    /// The fetch failed: report `Failed` when picked.
    Missing,
}

struct Job {
    id: TexId,
    key: String,
    path: String,
    flags: ImageFlags,
    pri: i32,
    state: JobState,
}

/// Completed lazy fetches: `(vfs key, bytes or None)`.
type Fetched = Rc<RefCell<Vec<(String, Option<Vec<u8>>)>>>;

/// Images from a prefetched VFS, ASYNC ones fetched lazily from `base_url`.
pub struct WebLoader {
    base_url: String,
    /// VENDOR: the asset bundle's index ([`crate::bundle`]): a logical key is fetched from the
    /// bundle file that holds it.
    bundle: Option<Rc<crate::bundle::Bundle>>,
    vfs: Vfs,
    queue: Vec<Job>,
    fetched: Fetched,
}

/// The VFS key of a Lua path (`generic_u8string` separators).
fn vfs_key(path: &str) -> String {
    path.replace('\\', "/")
}

impl WebLoader {
    /// `base_url` is where the script dir is served (the asset bundle, e.g. `"assets/"`, with the
    /// trailing slash).
    pub fn new(base_url: impl Into<String>) -> WebLoader {
        WebLoader::with_vfs(base_url, Vfs::default())
    }

    /// Share a VFS the host already filled.
    pub fn with_vfs(base_url: impl Into<String>, vfs: Vfs) -> WebLoader {
        WebLoader {
            base_url: base_url.into(),
            bundle: None,
            vfs,
            queue: Vec::new(),
            fetched: Fetched::default(),
        }
    }

    /// The VFS (shared: the host's prefetch and `AssetSource` read it too).
    pub fn vfs(&self) -> Vfs {
        self.vfs.clone()
    }

    /// Put a file into the VFS (prefetch results, embedded assets, tests).
    pub fn insert(&self, path: &str, bytes: Vec<u8>) {
        self.vfs.borrow_mut().insert(vfs_key(path), bytes);
    }

    /// Resolve logical keys through the asset bundle's index (VENDOR).
    pub fn set_bundle(&mut self, bundle: Rc<crate::bundle::Bundle>) {
        self.bundle = Some(bundle);
    }

    fn url(&self, key: &str) -> String {
        let file = self
            .bundle
            .as_deref()
            .and_then(|b| b.script_file(key))
            .unwrap_or(key);
        format!("{}{}", self.base_url, file)
    }

    /// Fetch bundle files (paths relative to the bundle root) into the VFS under every logical
    /// key the index gives each (VENDOR: [`crate::bundle::WEB_PREFETCH_FILE`]); concurrently,
    /// one request per file. Without a bundle this is [`WebLoader::prefetch`].
    pub async fn prefetch_bundle_files(&self, files: &[String]) {
        let Some(b) = self.bundle.clone() else {
            return self.prefetch(files).await;
        };
        let pending: Vec<_> = files
            .iter()
            .map(|f| (f.clone(), start_fetch(&format!("{}{f}", self.base_url))))
            .collect();
        for (f, started) in pending {
            if let Some(bytes) = finish_fetch(started).await {
                let mut vfs = self.vfs.borrow_mut();
                for key in b.script_paths_of(&f) {
                    vfs.insert(key.to_owned(), bytes.clone());
                }
            }
        }
    }

    /// Fetch `paths` (relative to `src/`) into the VFS, concurrently; files that fail to fetch are
    /// left out (their loads fail like missing files). The host awaits this before `on_init` and
    /// before a tree version switch.
    pub async fn prefetch(&self, paths: &[String]) {
        let pending: Vec<_> = paths
            .iter()
            .map(|p| {
                let key = vfs_key(p);
                let started = start_fetch(&self.url(&key));
                (key, started)
            })
            .collect();
        for (key, started) in pending {
            if let Some(bytes) = finish_fetch(started).await {
                self.vfs.borrow_mut().insert(key, bytes);
            }
        }
    }

    /// Start a lazy fetch of `key` unless one is already running for it.
    fn fetch_lazily(&self, key: &str) {
        if self
            .queue
            .iter()
            .any(|j| j.key == key && j.state == JobState::Fetching)
        {
            return;
        }
        // VENDOR: a path the bundle's index does not list does not exist: fail without a request
        if self
            .bundle
            .as_deref()
            .is_some_and(|b| b.script_file(key).is_none())
        {
            self.fetched.borrow_mut().push((key.to_owned(), None));
            return;
        }
        let started = start_fetch(&self.url(key));
        let fetched = self.fetched.clone();
        let key = key.to_owned();
        wasm_bindgen_futures::spawn_local(async move {
            let bytes = finish_fetch(started).await;
            fetched.borrow_mut().push((key, bytes));
        });
    }

    fn decode(&self, id: TexId, key: &str, path: &str, flags: ImageFlags) -> LoadEvent {
        match self
            .vfs
            .borrow()
            .get(key)
            .and_then(|b| decode_image(b, path, flags))
        {
            Some(d) => LoadEvent::Done(id, d),
            None => LoadEvent::Failed(id),
        }
    }
}

/// `ThreadProc`'s choice among the jobs that can run now: the highest `loadPri`, the earliest
/// queued among equals (r_texture.cpp:204-219).
fn pick(queue: &[Job]) -> Option<usize> {
    let mut best: Option<usize> = None;
    let mut max_pri = 0;
    for (i, j) in queue.iter().enumerate() {
        if j.state == JobState::Fetching {
            continue;
        }
        if best.is_none() || j.pri > max_pri {
            max_pri = j.pri;
            best = Some(i);
        }
    }
    best
}

impl ImageLoader for WebLoader {
    fn request(
        &mut self,
        id: TexId,
        path: &str,
        flags: ImageFlags,
        sync: bool,
        priority: i32,
    ) -> Option<LoadEvent> {
        let key = vfs_key(path);
        if sync {
            return Some(self.decode(id, &key, path, flags));
        }
        let state = if self.vfs.borrow().contains_key(&key) {
            JobState::Ready
        } else {
            self.fetch_lazily(&key);
            JobState::Fetching
        };
        self.queue.push(Job {
            id,
            key,
            path: path.to_owned(),
            flags,
            pri: priority,
            state,
        });
        None
    }

    fn read_size(&mut self, _id: TexId, path: &str) -> Option<(u32, u32)> {
        self.vfs
            .borrow()
            .get(&vfs_key(path))
            .and_then(|b| read_image_size(b, path))
    }

    fn set_priority(&mut self, id: TexId, priority: i32) {
        for j in &mut self.queue {
            if j.id == id {
                j.pri = priority;
            }
        }
    }

    fn poll(&mut self) -> Vec<LoadEvent> {
        let done: Vec<_> = self.fetched.borrow_mut().drain(..).collect();
        for (key, bytes) in done {
            let ok = bytes.is_some();
            if let Some(b) = bytes {
                self.vfs.borrow_mut().insert(key.clone(), b);
            }
            for j in &mut self.queue {
                if j.key == key && j.state == JobState::Fetching {
                    j.state = if ok {
                        JobState::Ready
                    } else {
                        JobState::Missing
                    };
                }
            }
        }
        // One decode per frame.
        let Some(i) = pick(&self.queue) else {
            return Vec::new();
        };
        let j = self.queue.remove(i);
        let ev = match j.state {
            JobState::Missing => LoadEvent::Failed(j.id),
            _ => self.decode(j.id, &j.key, &j.path, j.flags),
        };
        vec![ev]
    }
}

/// `window.fetch(url)`, started now (`None` without a window).
fn start_fetch(url: &str) -> Option<web_sys::js_sys::Promise> {
    web_sys::window().map(|w| w.fetch_with_str(url))
}

/// The body of a started fetch when the response is `ok`.
async fn finish_fetch(started: Option<web_sys::js_sys::Promise>) -> Option<Vec<u8>> {
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;
    use web_sys::js_sys::{Function, Promise, Reflect, Uint8Array};

    let resp = JsFuture::from(started?).await.ok()?;
    if !Reflect::get(&resp, &"ok".into()).ok()?.as_bool()? {
        return None;
    }
    let array_buffer: Function = Reflect::get(&resp, &"arrayBuffer".into())
        .ok()?
        .dyn_into()
        .ok()?;
    let body: Promise = array_buffer.call0(&resp).ok()?.dyn_into().ok()?;
    let buf = JsFuture::from(body).await.ok()?;
    Some(Uint8Array::new(&buf).to_vec())
}
