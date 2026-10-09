//! Native [`ImageLoader`]: a pool of 4 loader threads (ui doc §4.5). Owner: U3.
//!
//! Port of SimpleGraphic `r_texture.cpp` `t_manager_c` (research copy master @ 8694a07):
//! `runnersWanted = 4`; `ThreadProc` picks the queued texture with the highest `loadPri`, ties
//! in queue order. Non-ASYNC loads decode on the calling thread. Moved from the threaded part of
//! the pre-port `pob-sg/src/images.rs` by UI-U0.
//!
//! Deterministic mode (oracle, headless, tests: ui doc §4.5) completes every load synchronously.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

use pob_sg::image::{decode_image, read_image_size};
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

fn load_event(id: TexId, path: &Path, name: &str, flags: ImageFlags) -> LoadEvent {
    match std::fs::read(path)
        .ok()
        .and_then(|b| decode_image(&b, name, flags))
    {
        Some(d) => LoadEvent::Done(id, d),
        None => LoadEvent::Failed(id),
    }
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

fn worker(sh: &Shared) {
    loop {
        let job = {
            let mut q = lock(sh);
            loop {
                if q.shutdown {
                    return;
                }
                if !q.jobs.is_empty() {
                    break;
                }
                q = sh
                    .cv
                    .wait(q)
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
            }
            // ThreadProc: pick the highest loadPri; ties keep queue order.
            let mut best = 0;
            for (i, j) in q.jobs.iter().enumerate() {
                if j.pri > q.jobs[best].pri {
                    best = i;
                }
            }
            q.jobs.remove(best)
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
        self.ensure_workers();
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
        let full = self.resolve(path);
        if path.to_ascii_lowercase().ends_with(".tga") {
            let mut hdr = [0u8; 18];
            let mut f = std::fs::File::open(&full).ok()?;
            std::io::Read::read_exact(&mut f, &mut hdr).ok()?;
            return read_image_size(&hdr, path);
        }
        image::ImageReader::open(&full)
            .ok()?
            .with_guessed_format()
            .ok()?
            .into_dimensions()
            .ok()
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
    use pob_sg::{ImageHandle, ImageRegistry};

    fn png_bytes(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_fn(w, h, |x, y| image::Rgba([x as u8, y as u8, 7, 255]));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    /// Moved from the pre-port images.rs `async_load_and_mips`.
    #[test]
    fn async_load_and_mips() {
        let dir = std::env::temp_dir().join(format!("pob-host-loader-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("m.png"), png_bytes(8, 4)).unwrap();
        let mut images = ImageRegistry::new(Box::new(NativeLoader::new(&dir, false)));
        images.take_uploads();
        let a = ImageHandle::default();
        images.load(&a, "m.png", ImageFlags::ASYNC | ImageFlags::MIPMAP);
        // ImageSize blocks (reads the header) even for ASYNC loads.
        assert_eq!(images.image_size(a.tex().unwrap()), (8, 4));
        let start = std::time::Instant::now();
        let ups = loop {
            images.pump();
            let ups = images.take_uploads();
            if !ups.is_empty() {
                break ups;
            }
            assert!(start.elapsed().as_secs() < 10, "async load timed out");
            std::thread::sleep(std::time::Duration::from_millis(1));
        };
        assert_eq!(ups[0].1.levels.len(), 4); // 8x4, 4x2, 2x1, 1x1
        assert_eq!(ups[0].1.levels[3].len(), 4);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
