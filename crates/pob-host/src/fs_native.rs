//! Native [`Fs`] and [`AssetSource`] (ui doc §4.6, §5.2). Owner: U7.
//!
//! UI-U0: the plain file operations are direct `std::fs` calls; `search` (`NewFileSearch`
//! wildcard semantics, sys_main.cpp) is U7's.

use std::path::PathBuf;

use pob_sg::{AssetSource, FileEntry, Fs};

/// io.open / MakeDir / RemoveDir / os.rename / os.remove over the real file system.
#[derive(Default)]
pub struct NativeFs;

impl Fs for NativeFs {
    fn read(&self, path: &str) -> std::io::Result<Vec<u8>> {
        std::fs::read(path)
    }
    fn write(&mut self, path: &str, data: &[u8]) -> std::io::Result<()> {
        std::fs::write(path, data)
    }
    fn search(&self, spec: &str, find_dirs: bool) -> Vec<FileEntry> {
        let _ = (spec, find_dirs);
        todo!("U7: NewFileSearch (wildcard spec, files or dirs)")
    }
    fn make_dir(&mut self, path: &str) -> std::io::Result<()> {
        std::fs::create_dir(path)
    }
    fn remove_dir(&mut self, path: &str, recursive: bool) -> std::io::Result<()> {
        if recursive {
            std::fs::remove_dir_all(path)
        } else {
            std::fs::remove_dir(path)
        }
    }
    fn rename(&mut self, from: &str, to: &str) -> std::io::Result<()> {
        std::fs::rename(from, to)
    }
    fn remove(&mut self, path: &str) -> std::io::Result<()> {
        std::fs::remove_file(path)
    }
}

/// PoB's `src/` and `runtime/SimpleGraphic/` on disk.
pub struct DirAssets {
    pub src: PathBuf,
    pub runtime: PathBuf,
}

impl AssetSource for DirAssets {
    fn read_src(&self, path: &str) -> std::io::Result<Vec<u8>> {
        std::fs::read(self.src.join(path))
    }
    fn read_runtime(&self, path: &str) -> std::io::Result<Vec<u8>> {
        std::fs::read(self.runtime.join(path))
    }
}
