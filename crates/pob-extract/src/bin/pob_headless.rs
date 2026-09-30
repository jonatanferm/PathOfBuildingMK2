//! Boot Path of Building headless inside pob-calc's interpreter, load
//! builds, print headline stats and timings.
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let pob_root = std::env::var_os("POB_ROOT").map_or_else(
        || std::path::PathBuf::from(".PathOfBuilding"),
        std::path::PathBuf::from,
    );
    let t = Instant::now();
    let mut pob = match pob_calc::headless::Pob::boot_from_dir(&pob_root) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("boot failed: {e}");
            std::process::exit(1);
        }
    };
    println!("boot: {:?}", t.elapsed());
    for path in &args {
        let xml = std::fs::read_to_string(path).expect("read build");
        let t = Instant::now();
        if let Err(e) = pob.load_build_xml(&xml, path) {
            println!("{path}: load failed: {e}");
            continue;
        }
        let load = t.elapsed();
        let t = Instant::now();
        if std::env::var("HEADLESS_RECALC").is_ok() {
            pob.recalc().expect("recalc");
        }
        let recalc = t.elapsed();
        if let Ok(snippet) = std::env::var("HEADLESS_EXEC") {
            match pob.exec(&snippet) {
                Ok(v) => println!(
                    "{}",
                    v.iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join("\n")
                ),
                Err(e) => println!("exec error: {e}"),
            }
        }
        let out = pob.player_output().expect("output");
        if let Ok(dir) = std::env::var("HEADLESS_JSON_OUT") {
            let stem = std::path::Path::new(path)
                .file_stem()
                .unwrap()
                .to_string_lossy()
                .into_owned();
            let map: serde_json::Map<String, serde_json::Value> = out
                .numbers
                .iter()
                .filter(|(_, v)| v.is_finite())
                .map(|(k, v)| (k.clone(), serde_json::json!(v)))
                .collect();
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                format!("{dir}/{stem}.json"),
                serde_json::to_string(&map).unwrap(),
            )
            .unwrap();
        }
        println!(
            "{path}: load {load:?}, recalc {recalc:?} | Life {:?} ES {:?} Str {:?} FireRes {:?} TotalDPS {:?} FullDPS {:?} ({} keys)",
            out.get("Life"),
            out.get("EnergyShield"),
            out.get("Str"),
            out.get("FireResist"),
            out.get("TotalDPS"),
            out.get("FullDPS"),
            out.numbers.len()
        );
    }
}
