//! The asset bundle (`assets/`, VENDOR): the files the app reads, vendored into the repository in
//! a layout of their own, so that the shipped apps need no PoB checkout.
//!
//! The app still names its files as SimpleGraphic's script and runtime directories hold them
//! (`Assets/ring.png`, `TreeData/3_27/skills-3.jpg`, `Data/TimelessJewelData/…`,
//! `Fonts/Fontin.tgf`): these *logical* paths are what the Lua-faithful code builds and what the
//! draw traces record. The bundle's [`INDEX_FILE`] maps every logical path to the file that holds
//! it (`ui/ring.png`, `tree/3_27/skills-3.jpg`, `timeless/…`, `fonts/Fontin.tgf`; identical files
//! of different tree versions share one copy). The hosts resolve the script and runtime
//! directories through the index when that directory is a bundle ([`Bundle::detect`]): reads,
//! `NewFileSearch` listings and image loads see the logical tree, nothing else changes. A
//! directory without an index (a PoB checkout, the oracle tests) is used as it is.
//!
//! `manifest.xml` is in the index (a release-style manifest that tools/pob-vendor writes:
//! PoB's version number, with a branch and a platform so that the app does not run in dev
//! mode). Paths the index does not know (`installed.cfg`, `first.run`) resolve against the
//! bundle root as before, so they are simply absent.
//!
//! The bundle is read-only, as an installed PoB under a read-only directory is: the native and
//! headless hosts refuse every write, rename, removal and `MakeDir` at or below the bundle root
//! ([`Bundle::contains`], [`read_only`]). PoB's own writes there are dev-mode or updater ones
//! (`ModCache.lua`, `first.run`), the Options popup's `main:SetManifestBranch` (Main.lua:1221-1222,
//! :1317-1334, whose save error is ignored, so the bundle's manifest keeps `branch="master"`),
//! and caches the port keeps elsewhere (the timeless `.bin` files, `pob_desktop::lut_cache_dir`).

use std::collections::BTreeMap;

/// The index file at the bundle root.
pub const INDEX_FILE: &str = "index.json";

/// The web prefetch list at the bundle root (bundle paths, one per line; `crate::web`).
pub const WEB_PREFETCH_FILE: &str = "web-prefetch.txt";

/// One entry of a logical directory listing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The logical file or directory name.
    pub name: String,
    pub is_dir: bool,
    /// The bundle path of a file (`None` for a directory).
    pub file: Option<String>,
}

/// A parsed [`INDEX_FILE`].
#[derive(Clone, Debug, Default)]
pub struct Bundle {
    /// The bundle root (a directory, or a URL base on the web), without a trailing `/`.
    pub root: String,
    /// The snapshot the bundle was vendored from (`source.pobCommit`), e.g. for cache keys.
    pub source_id: String,
    /// logical script-relative path → bundle path
    script: BTreeMap<String, String>,
    /// logical runtime-relative path → bundle path
    runtime: BTreeMap<String, String>,
}

/// A logical path in canonical form: `/` separators, no `.` components, `..` applied, no
/// leading `/`.
pub fn logical(path: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for c in path.split(['/', '\\']) {
        match c {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            c => out.push(c),
        }
    }
    out.join("/")
}

impl Bundle {
    /// Parse the index text of the bundle at `root`.
    pub fn parse(root: &str, index: &str) -> Result<Bundle, String> {
        let v: serde_json::Value =
            serde_json::from_str(index).map_err(|e| format!("{INDEX_FILE}: {e}"))?;
        if v["format"].as_u64() != Some(1) {
            return Err(format!("{INDEX_FILE}: unknown format {}", v["format"]));
        }
        let map = |k: &str| -> Result<BTreeMap<String, String>, String> {
            let o = v[k]
                .as_object()
                .ok_or_else(|| format!("{INDEX_FILE}: no '{k}' map"))?;
            o.iter()
                .map(|(l, p)| {
                    p.as_str()
                        .map(|p| (l.clone(), p.to_owned()))
                        .ok_or_else(|| format!("{INDEX_FILE}: {k}.{l} is not a string"))
                })
                .collect()
        };
        Ok(Bundle {
            root: root.trim_end_matches('/').to_owned(),
            source_id: v["source"]["pobCommit"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
            script: map("script")?,
            runtime: map("runtime")?,
        })
    }

    /// The bundle at `dir`, when `dir` holds an [`INDEX_FILE`] (a bundle with a broken index is
    /// an error: it is reported and the directory is used as it is).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn detect(dir: &std::path::Path) -> Option<std::sync::Arc<Bundle>> {
        let text = std::fs::read_to_string(dir.join(INDEX_FILE)).ok()?;
        let root = std::path::absolute(dir).unwrap_or_else(|_| dir.to_path_buf());
        match Bundle::parse(&crate::platform_native::generic(&root), &text) {
            Ok(b) => Some(std::sync::Arc::new(b)),
            Err(e) => {
                eprintln!("{}: {e}", dir.display());
                None
            }
        }
    }

    /// Every logical script path and its bundle path.
    pub fn script_files(&self) -> impl Iterator<Item = (&str, &str)> {
        self.script.iter().map(|(l, p)| (l.as_str(), p.as_str()))
    }

    /// Every logical runtime path and its bundle path.
    pub fn runtime_files(&self) -> impl Iterator<Item = (&str, &str)> {
        self.runtime.iter().map(|(l, p)| (l.as_str(), p.as_str()))
    }

    /// The bundle path of a script-relative logical path.
    pub fn script_file(&self, path: &str) -> Option<&str> {
        self.script.get(&logical(path)).map(String::as_str)
    }

    /// The bundle path of a runtime-relative logical path (`Fonts/…`).
    pub fn runtime_file(&self, path: &str) -> Option<&str> {
        self.runtime.get(&logical(path)).map(String::as_str)
    }

    /// The logical script paths a bundle file holds (dedup: several tree versions can share one).
    pub fn script_paths_of(&self, file: &str) -> Vec<&str> {
        self.script
            .iter()
            .filter(|(_, p)| p.as_str() == file)
            .map(|(l, _)| l.as_str())
            .collect()
    }

    /// The path relative to the bundle root of `path` when it names a place inside the bundle:
    /// a relative path as it is, an absolute one when it starts with the root.
    pub fn relative<'a>(&self, path: &'a str) -> Option<std::borrow::Cow<'a, str>> {
        let generic = path.replace('\\', "/");
        let abs = generic.starts_with('/') || generic.as_bytes().get(1) == Some(&b':');
        if !abs {
            return Some(std::borrow::Cow::Borrowed(path));
        }
        let rest = generic.strip_prefix(&self.root)?;
        if !(rest.is_empty() || rest.starts_with('/')) {
            return None;
        }
        Some(std::borrow::Cow::Owned(
            rest.trim_start_matches('/').to_owned(),
        ))
    }

    /// Whether `path` names the bundle root or a place below it: a relative path (relative to
    /// the root) whose `..` components do not leave the root, or an absolute one under the root
    /// once `.`/`..` are applied. The hosts refuse every change there ([`read_only`]).
    pub fn contains(&self, path: &str) -> bool {
        let generic = path.replace('\\', "/");
        let abs = generic.starts_with('/') || generic.as_bytes().get(1) == Some(&b':');
        let full = if abs {
            generic
        } else {
            format!("{}/{generic}", self.root)
        };
        let (Some(full), Some(root)) = (lexical(&full), lexical(&self.root)) else {
            return false;
        };
        full.len() >= root.len()
            && full.iter().zip(&root).all(|(a, b)| {
                if cfg!(windows) {
                    a.eq_ignore_ascii_case(b)
                } else {
                    a == b
                }
            })
    }

    /// `NewFileSearch` of the logical script directory `dir` (relative to the script dir): its
    /// files and the directories the index implies, sorted by name; `None` when the index has
    /// nothing below `dir`.
    pub fn script_dir(&self, dir: &str) -> Option<Vec<Entry>> {
        let d = logical(dir);
        let prefix = if d.is_empty() {
            String::new()
        } else {
            format!("{d}/")
        };
        let mut out: BTreeMap<String, Entry> = BTreeMap::new();
        for (l, p) in self.script.range(prefix.clone()..) {
            let Some(rest) = l.strip_prefix(&prefix) else {
                break;
            };
            match rest.split_once('/') {
                Some((sub, _)) => {
                    out.entry(sub.to_owned()).or_insert_with(|| Entry {
                        name: sub.to_owned(),
                        is_dir: true,
                        file: None,
                    });
                }
                None => {
                    out.insert(
                        rest.to_owned(),
                        Entry {
                            name: rest.to_owned(),
                            is_dir: false,
                            file: Some(p.clone()),
                        },
                    );
                }
            }
        }
        (!out.is_empty()).then(|| out.into_values().collect())
    }
}

/// The components of an absolute generic path with `.`/`..` applied (`None` when `..` climbs
/// above the top).
fn lexical(path: &str) -> Option<Vec<&str>> {
    let mut out = Vec::new();
    for c in path.split('/') {
        match c {
            "" | "." => {}
            ".." => {
                out.pop()?;
            }
            c => out.push(c),
        }
    }
    Some(out)
}

/// The error of a change to the read-only bundle ([`Bundle::contains`]): what `io.open(…, "w")`,
/// `os.remove`, `os.rename` and `MakeDir` give in a read-only install (`EACCES`), so the Lua-
/// faithful callers take their failure paths (e.g. `main:SetManifestBranch` ignores it).
pub fn read_only(path: &str) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::PermissionDenied,
        format!("{path}: Permission denied"),
    )
}

/// The bundle paths of a [`WEB_PREFETCH_FILE`] (`#` comments and blank lines skipped).
pub fn prefetch_list(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const INDEX: &str = r#"{"format":1,"source":{"pobCommit":"abc"},
        "script":{"Assets/ring.png":"ui/ring.png","TreeData/3_27/a.png":"tree/3_26/a.png",
                  "TreeData/3_26/a.png":"tree/3_26/a.png","TreeData/b.png":"tree/b.png",
                  "changelog.txt":"text/changelog.txt"},
        "runtime":{"Fonts/Fontin.tgf":"fonts/Fontin.tgf"}}"#;

    #[test]
    fn maps_and_lists_the_logical_tree() {
        let b = Bundle::parse("/x/assets/", INDEX).unwrap();
        assert_eq!(b.root, "/x/assets");
        assert_eq!(b.source_id, "abc");
        assert_eq!(b.script_file("Assets/ring.png"), Some("ui/ring.png"));
        assert_eq!(
            b.script_file("./TreeData\\3_27/../3_27/a.png"),
            Some("tree/3_26/a.png")
        );
        assert_eq!(b.script_file("Classes/Launch.lua"), None);
        assert_eq!(b.runtime_file("Fonts/Fontin.tgf"), Some("fonts/Fontin.tgf"));
        assert_eq!(
            b.script_paths_of("tree/3_26/a.png"),
            ["TreeData/3_26/a.png", "TreeData/3_27/a.png"]
        );
        let names = |v: Vec<Entry>| {
            v.into_iter()
                .map(|e| format!("{}{}", e.name, if e.is_dir { "/" } else { "" }))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            names(b.script_dir("TreeData").unwrap()),
            ["3_26/", "3_27/", "b.png"]
        );
        assert_eq!(
            names(b.script_dir("").unwrap()),
            ["Assets/", "TreeData/", "changelog.txt"]
        );
        assert!(b.script_dir("Data").is_none());
        assert_eq!(
            b.relative("/x/assets/TreeData/b.png").as_deref(),
            Some("TreeData/b.png")
        );
        assert_eq!(
            b.relative("TreeData/b.png").as_deref(),
            Some("TreeData/b.png")
        );
        assert!(b.relative("/x/assetsy/b.png").is_none());
        assert!(b.relative("/home/u/Builds/a.xml").is_none());
    }

    #[test]
    fn contains_the_root_and_below_only() {
        let b = Bundle::parse("/x/assets/", INDEX).unwrap();
        for p in [
            "manifest.xml",
            "./text/manifest.xml",
            "Data/New/x.lua",
            "",
            ".",
            "TreeData/../index.json",
            "/x/assets",
            "/x/assets/",
            "/x/assets/text/manifest.xml",
            "/x/y/../assets/a",
            "\\x\\assets\\a",
        ] {
            assert!(b.contains(p), "{p}");
        }
        for p in [
            "../manifest.xml",
            "TreeData/../../x",
            "/x/assetsy/b.png",
            "/x/assets/../b.png",
            "/x",
            "/home/u/Path of Building/Settings.xml",
            "/..",
        ] {
            assert!(!b.contains(p), "{p}");
        }
    }

    #[test]
    fn rejects_unknown_formats() {
        assert!(Bundle::parse("/", r#"{"format":2,"script":{},"runtime":{}}"#).is_err());
        assert!(Bundle::parse("/", "{").is_err());
    }
}
