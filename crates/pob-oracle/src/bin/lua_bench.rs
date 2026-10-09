//! Compare pob_oracle's interpreter with PUC Lua (mlua) on a PoB-like workload.
fn main() {
    let src = std::fs::read_to_string(std::env::args().nth(1).expect("script path")).unwrap();
    let lua = mlua::Lua::new();
    let t = std::time::Instant::now();
    let v: f64 = lua.load(&src).eval().unwrap();
    let puc = t.elapsed();
    let mut it = pob_oracle::lua::Interp::new();
    let t = std::time::Instant::now();
    let r = it.exec_chunk(&src, "bench", Vec::new()).unwrap();
    let ours = t.elapsed();
    println!(
        "PUC Lua 5.4: {puc:?} (={v})   pob_oracle: {ours:?} (={})   ratio {:.1}x",
        r[0],
        ours.as_secs_f64() / puc.as_secs_f64()
    );
}
