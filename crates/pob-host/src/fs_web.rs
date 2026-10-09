//! Web [`Fs`] over IndexedDB (ui doc §5.2; ideas from `crates/pob-ui/src/build_store_wasm.rs`;
//! wasm32 only). Owner: U7.

use pob_sg::{FileEntry, Fs};

#[derive(Default)]
pub struct WebFs;

impl Fs for WebFs {
    fn read(&self, path: &str) -> std::io::Result<Vec<u8>> {
        let _ = path;
        todo!("U7: web Fs read")
    }
    fn write(&mut self, path: &str, data: &[u8]) -> std::io::Result<()> {
        let _ = (path, data);
        todo!("U7: web Fs write")
    }
    fn search(&self, spec: &str, find_dirs: bool) -> Vec<FileEntry> {
        let _ = (spec, find_dirs);
        todo!("U7: web Fs search")
    }
    fn make_dir(&mut self, path: &str) -> std::io::Result<()> {
        let _ = path;
        todo!("U7: web Fs make_dir")
    }
    fn remove_dir(&mut self, path: &str, recursive: bool) -> std::io::Result<()> {
        let _ = (path, recursive);
        todo!("U7: web Fs remove_dir")
    }
    fn rename(&mut self, from: &str, to: &str) -> std::io::Result<()> {
        let _ = (from, to);
        todo!("U7: web Fs rename")
    }
    fn remove(&mut self, path: &str) -> std::io::Result<()> {
        let _ = path;
        todo!("U7: web Fs remove")
    }
}
