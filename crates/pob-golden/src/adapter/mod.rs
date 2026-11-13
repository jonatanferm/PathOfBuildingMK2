//! The adapter layer (GOLDEN-PLAN §3.4): the only code in this crate that names `pob_calc`
//! internals, so the refactor re-points only these files.
//!
//! An [`Adapter`] answers the lines of one golden file from the Rust side: it runs the calc (or
//! the parser, the tree, …) on the file's inputs and returns, per key, the value encoded as
//! §1.1 says ([`crate::encode`]), or the error the Rust side raised. The runner compares the
//! answers with the file ([`crate::compare`]); an adapter never compares.
//!
//! Each area module below owns the sets of one area and is filled by the GOLD unit that records
//! them (§6); GOLD1 lays out the modules and the registry. A set with no adapter fails in the
//! runner ("no adapter"), it is never skipped.
//!
//! | module | sets | unit |
//! |---|---|---|
//! | [`e2e`] | `e2e.ca`, `e2e.d13`, `e2e.levelcost`, `e2e.int3`, `e2e.bm`, `e2e.pwr`, `e2e.mcq` | GOLD3-GOLD6 |
//! | [`parser`] | `fn.parser.lines`, `fn.parser.fuzz`, `fn.parser.jewel` | GOLD7 |
//! | [`tree`], [`spec`], [`lut`] | `fn.tree`, `fn.spec`, `fn.lut` | GOLD8 |
//! | [`items`] | `fn.items` | GOLD9 |
//! | [`modstore`] | `fn.modstore` | GOLD10 |
//! | [`skills`], [`config`], [`tools`] | `fn.skills`, `fn.config`, `fn.tabs`, `fn.tools` | GOLD5, GOLD11 |
//! | [`fuzz`] | `fuzz.defence`, `fuzz.offence` | GOLD12 |
//!
//! **Rule for adapter changes** (§3.4): a PR that changes code here must show `golden diff` = 0,
//! or overlays with reasons. If an encoding changes, that is a golden-format change: it needs
//! the review of a bless and bumps the set's version.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value as J;

use crate::compare::Answer;
use crate::format::{GoldenFile, Line};

pub mod config;
pub mod e2e;
pub mod fuzz;
pub mod items;
pub mod lut;
pub mod modstore;
pub mod parser;
pub mod skills;
pub mod spec;
pub mod tools;
pub mod tree;

/// Flattened outputs: output key (`main.Life`, `calcs.BuffList`) to its encoded value.
pub type Flat = BTreeMap<String, J>;

/// Rust's answers for one file, by line key.
pub type Answers = BTreeMap<String, Answer>;

/// What an adapter may read besides the golden file.
#[derive(Debug, Clone)]
pub struct Ctx {
    /// The goldens directory (`tests/golden`): build XML paths in headers are relative to it.
    pub root: PathBuf,
}

impl Ctx {
    /// A path from a golden header (`builds/all57/x.xml`), under [`Self::root`].
    #[must_use]
    pub fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    /// The text of the build XML a header names.
    pub fn read_input(&self, rel: &str) -> crate::Result<String> {
        let p = self.path(rel);
        std::fs::read_to_string(&p).map_err(|e| crate::Error(format!("{}: {e}", p.display())))
    }
}

/// Answers the lines of a set's golden files from the Rust side.
pub trait Adapter: Sync {
    /// Rust's answer for every key the file asks (for a query line, from its input). A key the
    /// Rust side produces and the file does not list is a failure (§3.5: Rust-only keys), so an
    /// E2E adapter returns every key it computes. For a `$perOrder` line the answer is the list
    /// of results, one per order of the line's input. `Err` fails the whole file.
    fn answer(&self, file: &GoldenFile, ctx: &Ctx) -> Result<Answers, String>;

    /// The lines the recorder elided because their answer is fixed by the stored inputs (MS's
    /// trivial answers, MP7's empty calls; §2.3), regenerated from the file. The runner
    /// compares them with [`Self::answer`]'s answers like stored lines and counts them as
    /// regenerated. Default: none.
    fn elided(&self, _file: &GoldenFile, _ctx: &Ctx) -> Result<Vec<Line>, String> {
        Ok(Vec::new())
    }
}

/// The adapter of a set, or `None` (the runner fails the set's files).
#[must_use]
pub fn lookup(set: &str) -> Option<&'static dyn Adapter> {
    e2e::lookup(set)
        .or_else(|| parser::lookup(set))
        .or_else(|| tree::lookup(set))
        .or_else(|| spec::lookup(set))
        .or_else(|| lut::lookup(set))
        .or_else(|| items::lookup(set))
        .or_else(|| modstore::lookup(set))
        .or_else(|| skills::lookup(set))
        .or_else(|| config::lookup(set))
        .or_else(|| tools::lookup(set))
        .or_else(|| fuzz::lookup(set))
}

/// Every set name an adapter is registered for (the runner's report and `golden` list).
#[must_use]
pub fn registered_sets() -> Vec<&'static str> {
    [
        e2e::SETS,
        parser::SETS,
        tree::SETS,
        spec::SETS,
        lut::SETS,
        items::SETS,
        modstore::SETS,
        skills::SETS,
        config::SETS,
        tools::SETS,
        fuzz::SETS,
    ]
    .concat()
}

/// The goldens directory as a [`Ctx`].
#[must_use]
pub fn ctx(root: &Path) -> Ctx {
    Ctx {
        root: root.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn registry_is_consistent() {
        // every listed set resolves, and no set is listed twice
        let sets = super::registered_sets();
        let unique: std::collections::BTreeSet<_> = sets.iter().collect();
        assert_eq!(unique.len(), sets.len(), "{sets:?}");
        for s in &sets {
            assert!(super::lookup(s).is_some(), "{s} listed without an adapter");
        }
        assert!(super::lookup("no.such.set").is_none());
    }
}
