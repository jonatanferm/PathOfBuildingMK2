//! Headless [`ImageLoader`] (ui doc §4.5 "Deterministic mode"): every load, ASYNC included,
//! completes synchronously, pixels included, read through an [`AssetSource`] (fixture dirs,
//! in-memory maps). Owner: U3.

use pob_sg::image::{decode_image, read_image_size};
use pob_sg::{AssetSource, ImageFlags, ImageLoader, LoadEvent, TexId};

pub struct HeadlessLoader {
    pub src: Box<dyn AssetSource>,
}

impl HeadlessLoader {
    pub fn new(src: Box<dyn AssetSource>) -> HeadlessLoader {
        HeadlessLoader { src }
    }
}

impl ImageLoader for HeadlessLoader {
    fn request(
        &mut self,
        id: TexId,
        path: &str,
        flags: ImageFlags,
        _sync: bool,
        _priority: i32,
    ) -> Option<LoadEvent> {
        Some(
            match self
                .src
                .read_src(path)
                .ok()
                .and_then(|b| decode_image(&b, path, flags))
            {
                Some(d) => LoadEvent::Done(id, d),
                None => LoadEvent::Failed(id),
            },
        )
    }
    fn read_size(&mut self, _id: TexId, path: &str) -> Option<(u32, u32)> {
        self.src
            .read_src(path)
            .ok()
            .and_then(|b| read_image_size(&b, path))
    }
    fn set_priority(&mut self, _id: TexId, _priority: i32) {}
    fn poll(&mut self) -> Vec<LoadEvent> {
        Vec::new()
    }
}
