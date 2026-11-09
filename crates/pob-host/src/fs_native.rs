//! Native [`Fs`] and [`AssetSource`] (ui doc §4.6, §5.2). Owner: U7.
//!
//! Port of PathOfBuilding-SimpleGraphic `3b1a346`: `NewFileSearch` (`ui_api.cpp:1341-1430`,
//! `sys_main.cpp:139-245`), `MakeDir`/`RemoveDir` (`ui_api.cpp:1762-1810`), and the file
//! access LuaJIT's `io`/`os` do while SG has `chdir`ed into the script directory
//! (`ui_main.cpp:151` `PCall`: `SetWorkDir(scriptWorkDir)` around every Lua call). Relative
//! paths therefore resolve against PoB's `src/`.
//!
//! DIVERGENCE(listing order): see [`crate::sort_file_entries`].

use std::path::{Path, PathBuf};

use pob_sg::{AssetSource, FileEntry, Fs};

/// io.open / NewFileSearch / MakeDir / RemoveDir / os.rename / os.remove over the real file
/// system, relative paths resolved against `base` (the script directory).
#[derive(Default)]
pub struct NativeFs {
    pub base: PathBuf,
}

impl NativeFs {
    pub fn new(base: impl Into<PathBuf>) -> NativeFs {
        NativeFs { base: base.into() }
    }

    fn resolve(&self, path: &str) -> PathBuf {
        let p = Path::new(path);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.base.join(p)
        }
    }
}

/// lua: ui_api.cpp:1341-1430 `NewFileSearch` + `NextFile`: every entry of the spec's directory
/// whose name matches the pattern and whose kind (directory or not) equals `find_dirs`.
pub fn search_dir(dir: &Path, pattern: &str, find_dirs: bool) -> Vec<FileEntry> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name == "." || name == ".." || !crate::glob_match(pattern, &name) {
            continue;
        }
        // directory_entry::is_directory follows symlinks.
        let Ok(md) = std::fs::metadata(e.path()) else {
            continue;
        };
        if md.is_dir() != find_dirs {
            continue;
        }
        out.push(FileEntry {
            name,
            is_dir: md.is_dir(),
            size: if md.is_dir() { 0 } else { md.len() },
            modified: md.modified().map_or(0, crate::filetime_ticks),
        });
    }
    crate::sort_file_entries(&mut out);
    out
}

impl Fs for NativeFs {
    fn read(&self, path: &str) -> std::io::Result<Vec<u8>> {
        std::fs::read(self.resolve(path))
    }
    fn write(&mut self, path: &str, data: &[u8]) -> std::io::Result<()> {
        std::fs::write(self.resolve(path), data)
    }
    fn search(&self, spec: &str, find_dirs: bool) -> Vec<FileEntry> {
        let Some((dir, pattern)) = crate::search_split_spec(spec) else {
            return Vec::new();
        };
        search_dir(&self.resolve(dir), pattern, find_dirs)
    }
    /// `create_directory`: not recursive.
    fn make_dir(&mut self, path: &str) -> std::io::Result<()> {
        std::fs::create_dir(self.resolve(path))
    }
    /// `remove_all` when recursive, else `remove` (an empty directory).
    fn remove_dir(&mut self, path: &str, recursive: bool) -> std::io::Result<()> {
        let p = self.resolve(path);
        if recursive {
            std::fs::remove_dir_all(p)
        } else {
            std::fs::remove_dir(p)
        }
    }
    fn rename(&mut self, from: &str, to: &str) -> std::io::Result<()> {
        std::fs::rename(self.resolve(from), self.resolve(to))
    }
    fn remove(&mut self, path: &str) -> std::io::Result<()> {
        std::fs::remove_file(self.resolve(path))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pob-host-fs-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn search_and_files() {
        let d = tmp("search");
        let mut fs = NativeFs::new(&d);
        fs.make_dir("Builds").unwrap();
        assert!(fs.make_dir("Builds").is_err()); // create_directory: exists => nil, msg
        fs.make_dir("Builds/Sub").unwrap();
        fs.write("Builds/b.xml", b"bb").unwrap();
        fs.write("Builds/A.XML", b"a").unwrap();
        fs.write("Builds/c.txt", b"").unwrap();
        let names = |v: Vec<FileEntry>| v.into_iter().map(|e| e.name).collect::<Vec<_>>();
        assert_eq!(names(fs.search("Builds/*.xml", false)), ["A.XML", "b.xml"]);
        assert_eq!(names(fs.search("Builds/*", true)), ["Sub"]);
        assert_eq!(
            names(fs.search("Builds/*", false)),
            ["A.XML", "b.xml", "c.txt"]
        );
        assert!(fs.search("*.xml", false).is_empty()); // no directory part
        let abs = format!("{}/Builds/b.xml", d.display());
        let e = fs.search(&abs, false);
        assert_eq!((e[0].size, e[0].is_dir), (2, false));
        assert!(e[0].modified > 116_444_736_000_000_000);
        assert_eq!(fs.read("Builds/b.xml").unwrap(), b"bb");
        fs.rename("Builds/b.xml", "Builds/d.xml").unwrap();
        fs.remove("Builds/d.xml").unwrap();
        assert!(fs.remove_dir("Builds", false).is_err()); // not empty
        fs.remove_dir("Builds", true).unwrap();
        assert!(fs.search("Builds/*", false).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }
}
