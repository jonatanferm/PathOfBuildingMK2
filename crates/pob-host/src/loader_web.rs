//! Web [`ImageLoader`] (ui doc §4.5 "Web"; wasm32 only). Owner: U3.
//!
//! `image_size` cannot block on the web, so the host prefetches all synchronously loaded assets
//! (fonts, `Assets/*`, the active tree version's sprite sheets) into a VFS before `on_init` and
//! before a tree version switch; only ASYNC images are fetched lazily, decoding one per frame.

use pob_sg::{ImageFlags, ImageLoader, LoadEvent, TexId};

#[derive(Default)]
pub struct WebLoader;

impl ImageLoader for WebLoader {
    fn request(
        &mut self,
        id: TexId,
        path: &str,
        flags: ImageFlags,
        sync: bool,
        priority: i32,
    ) -> Option<LoadEvent> {
        let _ = (id, path, flags, sync, priority);
        todo!("U3: web loader (prefetched VFS + lazy fetch)")
    }
    fn read_size(&mut self, id: TexId, path: &str) -> Option<(u32, u32)> {
        let _ = (id, path);
        todo!("U3: web loader read_size (prefetched VFS)")
    }
    fn set_priority(&mut self, id: TexId, priority: i32) {
        let _ = (id, priority);
    }
    fn poll(&mut self) -> Vec<LoadEvent> {
        todo!("U3: web loader poll")
    }
}
