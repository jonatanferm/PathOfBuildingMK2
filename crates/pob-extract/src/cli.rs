use std::path::PathBuf;

pub struct Args {
    pub pob: PathBuf,
    pub out: PathBuf,
}

impl Args {
    pub fn parse() -> Self {
        let mut pob = default_pob_path();
        let mut out = PathBuf::from("data");
        let mut iter = std::env::args().skip(1);
        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "--pob" => {
                    pob = iter.next().expect("--pob requires a value").into();
                }
                "--out" => {
                    out = iter.next().expect("--out requires a value").into();
                }
                "-h" | "--help" => {
                    println!("usage: pob-extract [--pob DIR] [--out DIR]");
                    std::process::exit(0);
                }
                other => {
                    eprintln!("unknown argument: {other}");
                    std::process::exit(1);
                }
            }
        }
        Self { pob, out }
    }
}

/// Find the upstream PoB checkout. Preferred layout: `.PathOfBuilding/`
/// inside the MK2 repo root (hidden subdir, gitignored). Falls back to the
/// legacy sibling layout (`../PathOfBuilding/`) so older clones still work.
pub fn default_pob_path() -> PathBuf {
    let local = PathBuf::from(".PathOfBuilding");
    if local.join("src").is_dir() {
        return local;
    }
    PathBuf::from("../PathOfBuilding")
}
