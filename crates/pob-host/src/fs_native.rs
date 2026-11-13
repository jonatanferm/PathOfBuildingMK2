//! Native [`Fs`] and [`AssetSource`] (ui doc §4.6, §5.2). Owner: U7.
//!
//! Port of PathOfBuilding-SimpleGraphic `3b1a346`: `NewFileSearch` (`ui_api.cpp:1341-1430`,
//! `sys_main.cpp:139-245`), `MakeDir`/`RemoveDir` (`ui_api.cpp:1762-1810`), and the file
//! access LuaJIT's `io`/`os` do while SG has `chdir`ed into the script directory
//! (`ui_main.cpp:151` `PCall`: `SetWorkDir(scriptWorkDir)` around every Lua call). Relative
//! paths therefore resolve against PoB's `src/`.
//!
//! DIVERGENCE(listing order): see [`crate::sort_file_entries`].
//!
//! VENDOR: when the script directory is the asset bundle ([`crate::bundle`]), script-relative
//! paths resolve through its index: reads and listings see PoB's logical tree.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use pob_sg::{AssetSource, FileEntry, Fs};

use crate::bundle::Bundle;

/// io.open / NewFileSearch / MakeDir / RemoveDir / os.rename / os.remove over the real file
/// system, relative paths resolved against `base` (the script directory).
#[derive(Default)]
pub struct NativeFs {
    pub base: PathBuf,
    /// `base`'s index when it is the asset bundle.
    bundle: Option<Arc<Bundle>>,
}

impl NativeFs {
    pub fn new(base: impl Into<PathBuf>) -> NativeFs {
        let base = base.into();
        let bundle = Bundle::detect(&base);
        NativeFs { base, bundle }
    }

    fn resolve(&self, path: &str) -> PathBuf {
        if let Some(p) = self.bundle.as_deref().and_then(|b| bundle_file(b, path)) {
            return p;
        }
        let p = Path::new(path);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.base.join(p)
        }
    }
}

/// The bundle file holding the logical script path `path` (relative, or absolute under the
/// bundle root).
pub fn bundle_file(b: &Bundle, path: &str) -> Option<PathBuf> {
    let rel = b.relative(path)?;
    b.script_file(&rel).map(|f| Path::new(&b.root).join(f))
}

/// `NewFileSearch` of a logical directory of the bundle (`None`: the index has nothing there,
/// search the disk).
pub fn bundle_search(
    b: &Bundle,
    dir: &str,
    pattern: &str,
    find_dirs: bool,
) -> Option<Vec<FileEntry>> {
    let rel = b.relative(dir)?;
    let entries = b.script_dir(&rel)?;
    let mut out = Vec::new();
    for e in entries {
        if e.is_dir != find_dirs || !crate::glob_match(pattern, &e.name) {
            continue;
        }
        let md = e
            .file
            .as_ref()
            .and_then(|f| std::fs::metadata(Path::new(&b.root).join(f)).ok());
        out.push(FileEntry {
            name: e.name,
            is_dir: e.is_dir,
            size: md.as_ref().map_or(0, std::fs::Metadata::len),
            modified: md
                .and_then(|m| m.modified().ok())
                .map_or(0, crate::filetime_ticks),
        });
    }
    crate::sort_file_entries(&mut out);
    Some(out)
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
        if let Some(found) = self
            .bundle
            .as_deref()
            .and_then(|b| bundle_search(b, dir, pattern, find_dirs))
        {
            return found;
        }
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

/// The asset bundle as the script and runtime directories (VENDOR, [`crate::bundle`]).
pub struct BundleAssets(pub Arc<Bundle>);

impl BundleAssets {
    /// The bundle at `dir` (an error when `dir` has no readable index).
    pub fn open(dir: &Path) -> std::io::Result<BundleAssets> {
        Bundle::detect(dir).map(BundleAssets).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("{}: no asset bundle index", dir.display()),
            )
        })
    }

    fn read(&self, file: Option<&str>, path: &str) -> std::io::Result<Vec<u8>> {
        match file {
            Some(f) => std::fs::read(Path::new(&self.0.root).join(f)),
            None => Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("{path}: not in the asset bundle"),
            )),
        }
    }
}

impl AssetSource for BundleAssets {
    fn read_src(&self, path: &str) -> std::io::Result<Vec<u8>> {
        self.read(self.0.script_file(path), path)
    }
    fn read_runtime(&self, path: &str) -> std::io::Result<Vec<u8>> {
        self.read(self.0.runtime_file(path), path)
    }
}

/// The script/runtime assets of `script_dir` and `sg_runtime`: the bundle's when `script_dir`
/// is the asset bundle, else the two directories as they are.
pub fn dir_or_bundle_assets(script_dir: &Path, sg_runtime: &Path) -> Box<dyn AssetSource> {
    match Bundle::detect(script_dir) {
        Some(b) => Box::new(BundleAssets(b)),
        None => Box::new(DirAssets {
            src: script_dir.to_path_buf(),
            runtime: sg_runtime.to_path_buf(),
        }),
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
