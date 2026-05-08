//! Quick perf benchmark for the calc engine. Runs `compute_full_with_env`
//! repeatedly on a realistic Witch L90 + Arc + amulet build and reports
//! min/median/mean wall-clock time per run plus throughput.
//!
//! Usage:
//!   cargo run --release -p pob-engine --bin perf_bench [iters]

use std::path::PathBuf;
use std::time::Instant;

use pob_data::{load_passive_tree, load_skill_file};
use pob_engine::{
    perform::compute_full_with_env, Character, MainSkill, SkillRegistry,
};

const RARE_AMULET: &str = r"Item Class: Amulets
Rarity: RARE
Soul Charm
Onyx Amulet
--------
Quality: +20% (augmented)
--------
Requirements:
Level: 70
--------
Item Level: 84
--------
+10 to all Attributes
--------
+62 to maximum Life
+39% to all Elemental Resistances
20% increased Light Radius
--------";

fn data_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("data")
}

fn main() {
    let iters: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(1000);

    let tree = {
        let path = data_root().join("trees/3_25.json");
        let json = std::fs::read_to_string(&path).expect("read tree json");
        load_passive_tree(&json).expect("parse tree")
    };
    let skills = {
        let dir = data_root().join("skills");
        let mut sets = Vec::new();
        for entry in std::fs::read_dir(&dir).expect("skills dir") {
            let entry = entry.expect("dir entry");
            let p = entry.path();
            if p.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            if p.file_stem().and_then(|s| s.to_str()) == Some("index") {
                continue;
            }
            let json = std::fs::read_to_string(&p).expect("read skill file");
            if let Ok(set) = load_skill_file(&json) {
                sets.push(set);
            }
        }
        SkillRegistry::from_files(sets)
    };

    let mut c = Character::new(pob_engine::ClassRef::witch(), 90);
    let amulet = pob_engine::parse_item(RARE_AMULET).expect("parse amulet");
    c.items.equip(pob_data::Slot::Amulet, amulet);
    c.main_skill = Some(MainSkill {
        skill_id: "Arc".to_owned(),
        level: 20,
        quality: 20,
        enabled: true,
    });
    // Allocate a connected ring of nodes from the Witch start so the env has a
    // realistic mod count. We BFS from the class start and grab the first ~60
    // reachable nodes. This roughly mirrors a low-investment build.
    {
        let class_idx = tree
            .classes
            .iter()
            .position(|cls| cls.name.eq_ignore_ascii_case("Witch"))
            .unwrap() as u32;
        let start = tree
            .nodes
            .iter()
            .find_map(|(id, n)| (n.class_start_index == Some(class_idx)).then_some(*id))
            .expect("witch start");
        let mut visited: std::collections::HashSet<pob_data::NodeId> =
            std::collections::HashSet::new();
        let mut queue: std::collections::VecDeque<pob_data::NodeId> = [start].into();
        let mut allocated = 0;
        while let Some(n) = queue.pop_front() {
            if !visited.insert(n) {
                continue;
            }
            if n != start {
                c.allocate(n);
                allocated += 1;
                if allocated >= 60 {
                    break;
                }
            }
            if let Some(node) = tree.nodes.get(&n) {
                for &nb in node.out_edges.iter().chain(node.in_edges.iter()) {
                    if !visited.contains(&nb) {
                        queue.push_back(nb);
                    }
                }
            }
        }
    }

    if std::env::var("PERF_DUMP").is_ok() {
        let (out, _env) = compute_full_with_env(&c, &tree, Some(&skills), None);
        let mut keys: Vec<_> = out.iter().collect();
        keys.sort_by(|a, b| a.0.cmp(b.0));
        for (k, v) in keys {
            println!("{k} = {v}");
        }
        return;
    }

    // Warm-up: skip first few iterations.
    for _ in 0..50 {
        let _ = compute_full_with_env(&c, &tree, Some(&skills), None);
    }

    let mut samples: Vec<u128> = Vec::with_capacity(iters);
    let total_start = Instant::now();
    for _ in 0..iters {
        let start = Instant::now();
        let (out, _env) = compute_full_with_env(&c, &tree, Some(&skills), None);
        // Make sure the optimiser doesn't elide the call.
        std::hint::black_box(out);
        samples.push(start.elapsed().as_nanos());
    }
    let total = total_start.elapsed();

    samples.sort_unstable();
    let n = samples.len() as u128;
    let min = samples[0];
    let median = samples[(n / 2) as usize];
    let p99 = samples[((n * 99 / 100).min(n - 1)) as usize];
    let mean = samples.iter().sum::<u128>() / n;
    let max = samples[samples.len() - 1];

    let mods = {
        let (_out, env) = compute_full_with_env(&c, &tree, Some(&skills), None);
        let mut count = 0usize;
        use pob_engine::ModStore;
        for _ in env.mod_db.iter_all() {
            count += 1;
        }
        count
    };

    println!("perf_bench (Witch L90 + Arc + amulet)");
    println!("  iterations:    {iters}");
    println!("  mods in env:   {mods}");
    println!("  total elapsed: {:?}", total);
    println!("  min:    {:>9.3} µs", min as f64 / 1000.0);
    println!("  median: {:>9.3} µs", median as f64 / 1000.0);
    println!("  mean:   {:>9.3} µs", mean as f64 / 1000.0);
    println!("  p99:    {:>9.3} µs", p99 as f64 / 1000.0);
    println!("  max:    {:>9.3} µs", max as f64 / 1000.0);
    println!("  ops/s:  {:>9.0}", iters as f64 / total.as_secs_f64());
}
