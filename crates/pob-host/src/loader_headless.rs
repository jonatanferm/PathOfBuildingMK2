//! Headless [`ImageLoader`] (ui doc §4.5 "Deterministic mode"): every load, ASYNC included,
//! completes synchronously, pixels included, read through an [`AssetSource`] (fixture dirs,
//! in-memory maps). Owner: U3.
//!
//! The decode is `r_tex_c::LoadFile` (SimpleGraphic `3b1a346` r_texture.cpp:553-608) as ported in
//! `pob_sg::image::decode_image`; `read_size` follows the per-format size callbacks
//! (`pob_sg::image::read_image_size`). Paths are relative to PoB's `src/` (`AssetSource::read_src`).
//! There is no queue, so `set_priority` has nothing to reorder and `poll` never has events.

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

#[cfg(test)]
mod tests {
    use super::*;
    use pob_sg::{ImageHandle, ImageRegistry, TexStatus};
    use std::collections::HashMap;

    struct Mem(HashMap<String, Vec<u8>>);
    impl AssetSource for Mem {
        fn read_src(&self, path: &str) -> std::io::Result<Vec<u8>> {
            self.0
                .get(path)
                .cloned()
                .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))
        }
        fn read_runtime(&self, path: &str) -> std::io::Result<Vec<u8>> {
            self.read_src(path)
        }
    }

    fn png(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba([1, 2, 3, 4]));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn async_loads_complete_synchronously() {
        let mut files = HashMap::new();
        files.insert("Assets/a.png".to_owned(), png(4, 2));
        let mut images = ImageRegistry::new(Box::new(HeadlessLoader::new(Box::new(Mem(files)))));
        images.take_uploads();
        let a = ImageHandle::default();
        images.load(&a, "Assets/a.png", ImageFlags::ASYNC | ImageFlags::MIPMAP);
        let id = a.tex().unwrap();
        assert_eq!(images.status(id), TexStatus::Done);
        let ups = images.take_uploads();
        assert_eq!(ups.len(), 1);
        assert_eq!(ups[0].1.levels.len(), 3); // 4x2, 2x1, 1x1
        assert_eq!(images.image_size(id), (4, 2));
        let m = ImageHandle::default();
        images.load(&m, "Assets/missing.png", ImageFlags::ASYNC);
        assert_eq!(images.status(m.tex().unwrap()), TexStatus::Done);
        assert_eq!(images.image_size(m.tex().unwrap()), (0, 0));
    }
}
