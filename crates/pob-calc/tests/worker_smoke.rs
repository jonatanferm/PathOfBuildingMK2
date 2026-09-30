//! Boot the PoB worker, load a build, check the snapshot.
#[test]
#[cfg(not(target_arch = "wasm32"))]
fn worker_snapshot() {
    use pob_calc::worker::{Command, Event, PobHandle};
    let src = std::path::PathBuf::from("../../.PathOfBuilding/src");
    let build = std::path::Path::new("../../.parity/builds/upstream_OccVortex.xml");
    if !src.exists() || !build.exists() {
        return;
    }
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
                assert_eq!(s.outputs.get("Life"), Some(6728.0));
                break;
            }
        }
    }
}
