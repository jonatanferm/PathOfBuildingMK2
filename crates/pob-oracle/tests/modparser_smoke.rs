use std::path::Path;

#[test]
fn modparser_smoke() {
    let (Some(pob), Some(env)) = (
        pob_oracle::oracle::find_pob_src(),
        pob_oracle::oracle::find_up("data/lua/modparser_env.lua"),
    ) else {
        eprintln!("skipped: no PoB checkout / data/lua/modparser_env.lua");
        return;
    };
    let (pob, env): (&Path, &Path) = (&pob, &env);
    let t = std::time::Instant::now();
    let p =
        pob_oracle::mod_parser::ModParser::from_paths(pob, env).unwrap_or_else(|e| panic!("{e}"));
    eprintln!("load: {:?}", t.elapsed());
    for line in [
        "+43 to Strength",
        "10% increased maximum Life",
        "Adds 5 to 10 Physical Damage to Attacks",
        "+(20-30)% to Fire Resistance",
        "Chaos Inoculation",
        "Your hits can't be Evaded",
        "15% increased Damage per Power Charge",
        "Minions deal 20% increased Damage",
        "totally not a mod",
    ] {
        let t = std::time::Instant::now();
        let r = p.parse(line).unwrap_or_else(|e| panic!("{e}"));
        eprintln!(
            "{line:?} ({:?}) -> {:?} extra={:?}",
            t.elapsed(),
            r.mods,
            r.extra
        );
    }
}
