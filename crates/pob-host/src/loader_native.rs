//! Native [`ImageLoader`]: a pool of 4 loader threads (ui doc §4.5). Owner: U3.
//!
//! Port of SimpleGraphic `3b1a346` `engine/render/r_texture.cpp` `t_manager_c`:
//! - `:94-114` the constructor starts `runnersWanted = 4` worker threads (here: on the first ASYNC
//!   request);
//! - `:148-157` `AsyncAdd` queues an ASYNC texture (`IN_QUEUE`);
//! - `:197-233` `ThreadProc`: each worker takes the queued texture with the highest `loadPri`
//!   (the first one in queue order on ties: `curTex->loadPri > maxPri`) and runs `LoadFile`;
//! - non-ASYNC loads (`r_tex_c` ctor, `ForceLoad`) run `LoadFile` on the calling thread.
//!
//! `LoadFile`'s decoding (`LoaderForFile`, the per-format loaders, `BuildMipSet`) is
//! `pob_sg::image::decode_image`. Relative paths resolve against the script directory (PoB's
//! `src/`, `imgHandleLoad`'s `scriptWorkDir / fileName`); absolute paths are used as they are.
//!
//! Deterministic mode (oracle, headless, tests: ui doc §4.5) completes every load synchronously,
//! pixels included. Moved from the threaded part of the pre-port `pob-sg/src/images.rs` by UI-U0.
//!
//! Not modelled: the worker's mid-load `SIZE_KNOWN` (the registry reads the header itself when
//! `ImageSize` needs it), `AbortLoad` (textures are never released, see pob-sg `image.rs`), and
//! `GetAsyncCount` (unused by PoB).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

use pob_sg::image::{decode_image, read_image_size_from};
use pob_sg::{ImageFlags, ImageLoader, LoadEvent, TexId};

struct Job {
    id: TexId,
    path: PathBuf,
    name: String,
    flags: ImageFlags,
    pri: i32,
}

#[derive(Default)]
struct Queue {
    jobs: Vec<Job>,
    done: Vec<LoadEvent>,
    shutdown: bool,
}

struct Shared {
    q: Mutex<Queue>,
    cv: Condvar,
}

fn lock(sh: &Shared) -> MutexGuard<'_, Queue> {
    sh.q.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Reads images from disk relative to PoB's `src/`.
pub struct NativeLoader {
    base: PathBuf,
    deterministic: bool,
    sh: Arc<Shared>,
    workers: Vec<std::thread::JoinHandle<()>>,
}

/// `r_tex_c::LoadFile` minus the GL upload: read, decode, mipmap.
///
/// lua: r_texture.cpp:553-608
fn load_event(id: TexId, path: &Path, name: &str, flags: ImageFlags) -> LoadEvent {
    match std::fs::read(path)
        .ok()
        .and_then(|b| decode_image(&b, name, flags))
    {
        Some(d) => LoadEvent::Done(id, d),
        None => LoadEvent::Failed(id),
    }
}

/// `ThreadProc`'s choice: the highest `loadPri`; the earliest queued among equals.
///
/// lua: r_texture.cpp:204-219
fn pick(jobs: &[Job]) -> Option<usize> {
    let mut best: Option<usize> = None;
    let mut max_pri = 0;
    for (i, j) in jobs.iter().enumerate() {
        if best.is_none() || j.pri > max_pri {
            max_pri = j.pri;
            best = Some(i);
        }
    }
    best
}

impl NativeLoader {
    /// `base` is the script dir (PoB `src/`); `deterministic` makes ASYNC loads synchronous.
    pub fn new(base: impl Into<PathBuf>, deterministic: bool) -> NativeLoader {
        NativeLoader {
            base: base.into(),
            deterministic,
            sh: Arc::new(Shared {
                q: Mutex::new(Queue::default()),
                cv: Condvar::new(),
            }),
            workers: Vec::new(),
        }
    }

    /// `scriptWorkDir / fileName` unless `fileName` is absolute (ui_api.cpp:402-405).
    fn resolve(&self, path: &str) -> PathBuf {
        let p = Path::new(path);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.base.join(p)
        }
    }

    fn ensure_workers(&mut self) {
        if !self.workers.is_empty() {
            return;
        }
        // t_manager_c: runnersWanted = 4.
        for _ in 0..4 {
            let sh = self.sh.clone();
            if let Ok(h) = std::thread::Builder::new()
                .name("pob-image-loader".into())
                .spawn(move || worker(&sh))
            {
                self.workers.push(h);
            }
        }
    }
}

impl Drop for NativeLoader {
    fn drop(&mut self) {
        lock(&self.sh).shutdown = true;
        self.sh.cv.notify_all();
        for h in self.workers.drain(..) {
            let _ = h.join();
        }
    }
}

/// lua: r_texture.cpp:197-233 (ThreadProc; SG polls with Sleep(1), this waits on a condvar)
fn worker(sh: &Shared) {
    loop {
        let job = {
            let mut q = lock(sh);
            loop {
                if q.shutdown {
                    return;
                }
                if let Some(best) = pick(&q.jobs) {
                    break q.jobs.remove(best);
                }
                q = sh
                    .cv
                    .wait(q)
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
            }
        };
        let ev = load_event(job.id, &job.path, &job.name, job.flags);
        lock(sh).done.push(ev);
    }
}

impl ImageLoader for NativeLoader {
    fn request(
        &mut self,
        id: TexId,
        path: &str,
        flags: ImageFlags,
        sync: bool,
        priority: i32,
    ) -> Option<LoadEvent> {
        let full = self.resolve(path);
        if sync || self.deterministic {
            return Some(load_event(id, &full, path, flags));
        }
        // AsyncAdd
        self.ensure_workers();
        if self.workers.is_empty() {
            // `if (runnersRunning == 0) return true;`: no worker, the caller loads it now.
            return Some(load_event(id, &full, path, flags));
        }
        lock(&self.sh).jobs.push(Job {
            id,
            path: full,
            name: path.to_owned(),
            flags,
            pri: priority,
        });
        self.sh.cv.notify_one();
        None
    }

    fn read_size(&mut self, _id: TexId, path: &str) -> Option<(u32, u32)> {
        let f = std::fs::File::open(self.resolve(path)).ok()?;
        read_image_size_from(std::io::BufReader::new(f), path)
    }

    fn set_priority(&mut self, id: TexId, priority: i32) {
        for j in &mut lock(&self.sh).jobs {
            if j.id == id {
                j.pri = priority;
            }
        }
    }

    fn poll(&mut self) -> Vec<LoadEvent> {
        std::mem::take(&mut lock(&self.sh).done)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pob_sg::{ImageHandle, ImageRegistry, TexStatus};

    fn png_bytes(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_fn(w, h, |x, y| image::Rgba([x as u8, y as u8, 7, 255]));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pob-host-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn wait_uploads(
        images: &mut ImageRegistry,
        n: usize,
    ) -> Vec<(TexId, std::rc::Rc<pob_sg::ImageData>)> {
        let start = std::time::Instant::now();
        let mut all = Vec::new();
        while all.len() < n {
            all.extend(images.take_uploads());
            assert!(start.elapsed().as_secs() < 60, "async load timed out");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        all
    }

    /// Moved from the pre-port images.rs `async_load_and_mips`.
    #[test]
    fn async_load_and_mips() {
        let dir = temp_dir("loader");
        std::fs::write(dir.join("m.png"), png_bytes(8, 4)).unwrap();
        let mut images = ImageRegistry::new(Box::new(NativeLoader::new(&dir, false)));
        images.take_uploads();
        let a = ImageHandle::default();
        images.load(&a, "m.png", ImageFlags::ASYNC | ImageFlags::MIPMAP);
        // ImageSize blocks (reads the header) even for ASYNC loads.
        assert_eq!(images.image_size(a.tex().unwrap()), (8, 4));
        let ups = wait_uploads(&mut images, 1);
        assert_eq!(ups[0].1.levels.len(), 4); // 8x4, 4x2, 2x1, 1x1
        assert_eq!(ups[0].1.levels[3].len(), 4);
        assert_eq!(images.status(a.tex().unwrap()), TexStatus::Done);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn async_missing_file_stays_loading() {
        let dir = temp_dir("missing");
        let mut images = ImageRegistry::new(Box::new(NativeLoader::new(&dir, false)));
        images.take_uploads();
        let a = ImageHandle::default();
        images.load(&a, "nope.png", ImageFlags::ASYNC);
        let id = a.tex().unwrap();
        assert_eq!(images.image_size(id), (0, 0));
        let ups = wait_uploads(&mut images, 1);
        assert_eq!(*ups[0].1, pob_sg::image::default_texture());
        assert_eq!(images.status(id), TexStatus::Done);
        // IsLoading stays true forever (fileWidth == 0).
        assert_eq!(images.image_size(id), (0, 0));
        // Empty path: scriptWorkDir / "" is the directory itself; the load fails.
        let e = ImageHandle::default();
        images.load(&e, "", ImageFlags::empty());
        assert!(e.is_valid());
        assert_eq!(images.image_size(e.tex().unwrap()), (0, 0));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn deterministic_mode_is_synchronous() {
        let dir = temp_dir("det");
        std::fs::write(dir.join("d.png"), png_bytes(3, 5)).unwrap();
        let mut images = ImageRegistry::new(Box::new(NativeLoader::new(&dir, true)));
        images.take_uploads();
        let a = ImageHandle::default();
        images.load(&a, "d.png", ImageFlags::ASYNC);
        assert_eq!(images.status(a.tex().unwrap()), TexStatus::Done);
        let ups = images.take_uploads();
        assert_eq!(ups.len(), 1);
        assert_eq!((ups[0].1.width, ups[0].1.height), (3, 5));
        // Absolute paths bypass the script dir.
        let b = ImageHandle::default();
        let abs = dir.join("d.png");
        images.load(&b, abs.to_str().unwrap(), ImageFlags::empty());
        assert_eq!(images.image_size(b.tex().unwrap()), (3, 5));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn thread_proc_priority_order() {
        let job = |id: u32, pri: i32| Job {
            id: TexId(id),
            path: PathBuf::new(),
            name: String::new(),
            flags: ImageFlags::empty(),
            pri,
        };
        assert_eq!(pick(&[]), None);
        // Ties: queue order.
        assert_eq!(pick(&[job(1, 0), job(2, 0)]), Some(0));
        // Highest priority wins; the first among equals.
        assert_eq!(pick(&[job(1, 0), job(2, 5), job(3, 5), job(4, 1)]), Some(1));
        // Negative priorities: the first entry seeds maxPri.
        assert_eq!(pick(&[job(1, -3), job(2, -5), job(3, -1)]), Some(2));
    }

    #[test]
    fn set_priority_reorders_queue() {
        let mut l = NativeLoader::new(PathBuf::from("/nonexistent"), false);
        // Queue jobs without starting workers (direct queue manipulation).
        for i in 0..3 {
            lock(&l.sh).jobs.push(Job {
                id: TexId(10 + i),
                path: PathBuf::new(),
                name: String::new(),
                flags: ImageFlags::ASYNC,
                pri: 0,
            });
        }
        l.set_priority(TexId(12), 7);
        let q = lock(&l.sh);
        assert_eq!(pick(&q.jobs).map(|i| q.jobs[i].id), Some(TexId(12)));
    }

    /// Every image under `src/Assets` and `src/TreeData/3_27`, loaded ASYNC through the
    /// registry: `ImageSize` before completion equals the decoded size, and every load finishes
    /// without falling back to the default texture.
    #[test]
    fn pob_assets_load_async() {
        let Some(src) = locate_src() else {
            eprintln!("SKIP: PoB sources not found");
            return;
        };
        let mut files = Vec::new();
        for dir in ["Assets", "TreeData/3_27"] {
            collect(&src, &src.join(dir), &mut files);
        }
        files.retain(|f| {
            let l = f.to_ascii_lowercase();
            [".png", ".jpg", ".jpeg", ".webp"]
                .iter()
                .any(|e| l.ends_with(e))
        });
        assert!(files.len() >= 130, "{} files", files.len());
        let mut images = ImageRegistry::new(Box::new(NativeLoader::new(&src, false)));
        images.take_uploads();
        let mut handles = Vec::new();
        for (i, f) in files.iter().enumerate() {
            let h = ImageHandle::default();
            images.load(&h, f, ImageFlags::ASYNC);
            if i % 3 == 0 {
                images.set_priority(h.tex().unwrap(), i as i32);
            }
            let size = images.image_size(h.tex().unwrap());
            assert!(size.0 > 0 && size.1 > 0, "{f}: {size:?}");
            handles.push((h, size));
        }
        let ups = wait_uploads(&mut images, files.len());
        let default = pob_sg::image::default_texture();
        for (id, d) in &ups {
            assert!(
                **d != default,
                "{:?} fell back to the default texture",
                images.source(*id)
            );
        }
        for (h, size) in &handles {
            let (w, hh) = images.tex_size(h.tex().unwrap()).unwrap();
            assert_eq!((w as i32, hh as i32), *size);
        }
        eprintln!("loaded {} PoB images asynchronously", files.len());
    }

    fn locate_src() -> Option<PathBuf> {
        let mut starts = Vec::new();
        if let Some(p) = std::env::var_os("POB_ROOT") {
            starts.push(PathBuf::from(p));
        }
        starts.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")));
        for s in starts {
            for d in s.ancestors() {
                let src = d.join(".PathOfBuilding/src");
                if src.join("Launch.lua").is_file() {
                    return Some(src);
                }
            }
        }
        None
    }

    fn collect(root: &Path, dir: &Path, out: &mut Vec<String>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        let mut v: Vec<_> = rd.flatten().map(|e| e.path()).collect();
        v.sort();
        for p in v {
            if p.is_dir() {
                collect(root, &p, out);
            } else {
                out.push(p.strip_prefix(root).unwrap().to_string_lossy().into_owned());
            }
        }
    }
}
