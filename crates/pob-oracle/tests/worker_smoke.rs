//! Boot the PoB worker, load a build, check the snapshot.
#[test]
#[cfg(not(target_arch = "wasm32"))]
fn worker_snapshot() {
    use pob_oracle::worker::{Command, Event, PobHandle};
    let (Some(src), Some(build)) = (
        pob_oracle::oracle::find_pob_src(),
        pob_oracle::oracle::find_up(".parity/builds/upstream_OccVortex.xml"),
    ) else {
        eprintln!("skipped: no PoB checkout / .parity build corpus");
        return;
    };
    let mut h = PobHandle::spawn(src);
    h.send(Command::LoadXml {
        xml: std::fs::read_to_string(build).unwrap(),
        name: "t".into(),
    });
    loop {
        match h.recv().expect("event") {
            Event::Booted { millis } => eprintln!("booted in {millis} ms"),
            Event::BootFailed(e) => panic!("{e}"),
            Event::QueryResult { .. } => {}
            Event::Snapshot(s) => {
                eprintln!(
                    "error={:?} sidebar={} warnings={} xml={} outputs={}",
                    s.error,
                    s.sidebar.len(),
                    s.warnings.len(),
                    s.xml.len(),
                    s.outputs.numbers.len()
                );
                for l in s.sidebar.iter().take(5) {
                    eprintln!("  {:?}", l);
                }
                assert!(!s.sidebar.is_empty());
                eprintln!(
                    "points={:?} skill_select={}",
                    s.point_display, s.skill_select
                );
                assert!(s.skill_select.contains("mainSocketGroup"));
                eprintln!("sets={}", s.sets);
                assert!(s.sets.contains("\"spec\""));
                assert_eq!(s.outputs.get("Life"), Some(6728.0));
                break;
            }
        }
    }
}
