# INT-1-FRESH corpus manifest

Fresh parity corpus for calc doc §11.1 row INT-1 (the "100 fresh poe.ninja builds" half).
Fetched with `scripts/port/fetch_fresh_corpus.py fetch` from poe.ninja's public build pages
(`/poe1/api/builds/<version>/search` per ascendancy, then each character's
`pathOfBuildingExport`). The XML lives in `.parity/fresh/` (gitignored, never committed);
`CA_CORPUS_DIR=.parity/fresh` runs the CA harness on it. Triage: `INT-1-fresh-triage.md`.
This file is generated: `scripts/port/fetch_fresh_corpus.py manifest --probe target/tmp/ca-probe`.

**Fetched 105, kept 100, dropped 5.**

## Selection

- Source: poe.ninja's own API (the JSON/protobuf the build pages load), not pobb.in. Leagues:
  Standard (3.29 tree), Hardcore Allflame and SSF Allflame (3.29), Mirage (3.28). Allflame
  softcore trade is left out: the all57 corpus already comes from it.
- 21 ascendancies x 5 picks (Standard, HC Allflame, SSF Allflame, Mirage,
  Standard), from the first 100 characters of each class search. Each pick prefers a main
  skill not picked yet and a skill kind (attack, spell, minion, totem/trap/mine/brand, DoT) that
  is under-represented. A pick whose character API answers 404 falls back to the next 3 rows
  (`--alternates`).
- Politeness: at most one request per second, User-Agent `pob-port-parity-corpus/1.0`,
  robots.txt (only `/dev/` is disallowed) checked, `Retry-After` honoured (one 429 with
  `Retry-After: 3600` was hit and waited out). The two complete fetch runs sent 213 requests;
  about 20 more went to probing the API and to two runs that stopped early.
- `sha256` is the first 16 hex digits of the decoded XML's SHA-256. `kind` is the main skill's
  kind from PoB's gem tags (`other`: not a gem, e.g. an item or ascendancy skill; `+dot`: PoB's
  `TotalDot` is over half of `CombinedDPS`).

## PoB checks

- Every kept build loads in the oracle (PoB v2.67.2 under LuaJIT, the CA harness's `pob_run`)
  without a PoB error: no `error.txt`, no `promptMsg`.
- Newer game data: none needed. A Lua check run in PoB after each load (2026-11-12, all 100
  kept builds) found no tree node missing from PoB's tree (allocated = XML node count on all 100),
  no item without a base, every `<Item>` loaded, and every gem resolved except one: fresh_048's
  `Summon Sacred Wisps` is a gem entry given by name only (PoB: "Unrecognised gem name"; PoB still
  loads the build without error). 111 unique items did not match the check's lookup of
  `data.uniques` titles: 40 Foulborn (mutated) uniques, which PoB 2.67.2 parses
  (`Classes/Item.lua:1603`), and 71 Forbidden Flame/Flesh, Bound By Destiny, That Which Was
  Taken and Doppelganger Guise items, which PoB generates or lists in a form the check did not
  match (Bound By Destiny is in `Data/Uniques/jewel.lua`). So no build was dropped
  for game data. The 5 drops are all fetch failures (character API 404).
- fresh_054 (Standard Inquisitor) exports with only 2 allocated tree nodes. PoB loads it as is,
  and it is kept as an edge case.

## Builds

| stem | league | tree | class | main skill (poe.ninja) | kind | level | fetched | sha256 | source |
|---|---|---|---|---|---|---|---|---|---|
| fresh_000 | standard | 3.29 | Ascendant | Cyclone of Tumult | attack | 100 | 2026-11-11 | `be512b7ae0db1e27` | [poe.ninja](https://poe.ninja/poe1/builds/standard/character/Selveste-4477/murderen) |
| fresh_001 | allflamehc | 3.29 | Ascendant | Winter Orb | spell | 100 | 2026-11-11 | `1ceecd29b6766d6f` | [poe.ninja](https://poe.ninja/poe1/builds/allflamehc/character/Chainluck-5686/CHAIN_SEVEN) |
| fresh_002 | allflamessf | 3.29 | Ascendant | Volatile Dead | dot | 100 | 2026-11-11 | `8cc2626012ca4da9` | [poe.ninja](https://poe.ninja/poe1/builds/allflamessf/character/Raegore-4879/WesAscendant) |
| fresh_003 | mirage | 3.28 | Ascendant | Animate Guardian | minion | 100 | 2026-11-11 | `d04aba6d49d9c683` | [poe.ninja](https://poe.ninja/poe1/builds/mirage/character/Redpaws-3764/RedCactuar) |
| fresh_004 | standard | 3.29 | Ascendant | Icestorm | other | 100 | 2026-11-11 | `a8894407b5094975` | [poe.ninja](https://poe.ninja/poe1/builds/standard/character/Akisailent-5635/AkiS_Heist) |
| fresh_005 | standard | 3.29 | Assassin | Ethereal Knives | spell | 100 | 2026-11-11 | `be9db4006ee5700b` | [poe.ninja](https://poe.ninja/poe1/builds/standard/character/Splasharka-4084/br_br_xuilapim) |
| fresh_006 | allflamehc | 3.29 | Assassin | Icicle Mine of Fanning | totem/trap/mine | 100 | 2026-11-11 | `cdbda913e679081c` | [poe.ninja](https://poe.ninja/poe1/builds/allflamehc/character/zlobin21-6581/HcAllShad) |
| fresh_007 | allflamessf | 3.29 | Assassin | Lightning Strike | attack | 100 | 2026-11-11 | `da25053116baf9e0` | [poe.ninja](https://poe.ninja/poe1/builds/allflamessf/character/Eliqq-2301/JaceAllFlame) |
| fresh_008 | mirage | 3.28 | Assassin | Penance Brand of Dissipation | dot | 100 | 2026-11-11 | `0970bda29c074eeb` | [poe.ninja](https://poe.ninja/poe1/builds/mirage/character/gdm12322-5451/Yethello_Mirage) |
| fresh_009 | standard | 3.29 | Assassin | Summon Holy Relic of Conviction | minion | 100 | 2026-11-11 | `781f029e3f7907ea` | [poe.ninja](https://poe.ninja/poe1/builds/standard/character/theberghain-7354/Grruwi) |
| fresh_010 | standard | 3.29 | Berserker | Cyclone | attack | 100 | 2026-11-11 | `a9d11765198d5b43` | [poe.ninja](https://poe.ninja/poe1/builds/standard/character/Brenugae-0470/Stringer_Bell) |
| fresh_011 | allflamehc | 3.29 | Berserker | Sunder | dot | 100 | 2026-11-11 | `e12256a45ffb27a2` | [poe.ninja](https://poe.ninja/poe1/builds/allflamehc/character/Kabajisan-2683/KabonkaLovesFlonkina) |
| fresh_012 | allflamessf | 3.29 | Berserker | Earthshatter | attack | 100 | 2026-11-11 | `2d5710a8e96e815b` | [poe.ninja](https://poe.ninja/poe1/builds/allflamessf/character/Velkr0-0092/HAHAAHIAMSOSLOW) |
| fresh_013 | mirage | 3.28 | Berserker | Blade Vortex | spell | 100 | 2026-11-11 | `952e9a8a83ac0eb7` | [poe.ninja](https://poe.ninja/poe1/builds/mirage/character/OhHeyCindy-5100/%D0%94%D0%B5%D0%BD%D0%B8%D1%81%D0%A5%D0%B0%D0%BC%D0%B8%D1%82%D0%BE%D0%B2) |
| fresh_014 | standard | 3.29 | Berserker | Wintertide Brand | totem/trap/mine+dot | 100 | 2026-11-11 | `4183af90db1dc9e3` | [poe.ninja](https://poe.ninja/poe1/builds/standard/character/12gido-5261/dffdgdfgdfg) |
| fresh_015 | standard | 3.29 | Champion | Reap | dot | 100 | 2026-11-11 | `c0b4ff476c4e7ddb` | [poe.ninja](https://poe.ninja/poe1/builds/standard/character/greef964-1072/_PainOfExile_) |
| fresh_016 | allflamehc | 3.29 | Champion | Desecrate | spell+dot | 100 | 2026-11-11 | `781d5096fc9e063e` | [poe.ninja](https://poe.ninja/poe1/builds/allflamehc/character/MassacrisM-2837/ErrandoDiscimus) |
| fresh_017 | allflamessf | 3.29 | Champion | Lacerate of Butchering | attack | 100 | 2026-11-11 | `a0843469d7a9c957` | [poe.ninja](https://poe.ninja/poe1/builds/allflamessf/character/Animalcrossing-7959/POE_II___________BAD) |
| fresh_018 | mirage | 3.28 | Champion | Corrupting Fever | dot | 100 | 2026-11-11 | `bca9f07f419a1bfd` | [poe.ninja](https://poe.ninja/poe1/builds/mirage/character/longwill-6636/LongwillRoundTen) |
| fresh_019 | standard | 3.29 | Champion | Herald of Agony | minion | 100 | 2026-11-11 | `67b471f28c9836e9` | [poe.ninja](https://poe.ninja/poe1/builds/standard/character/MABorges-3321/BordesD) |
| fresh_020 | standard | 3.29 | Chieftain | Searing Bond of Detonation | totem/trap/mine+dot | 100 | 2026-11-11 | `70765395a88bea0f` | [poe.ninja](https://poe.ninja/poe1/builds/standard/character/%ED%98%B8%EA%B9%80%EC%9D%B4-2851/Hoyy_Mirage) |
| fresh_021 | allflamehc | 3.29 | Chieftain | Armageddon Brand of Volatility | totem/trap/mine+dot | 100 | 2026-11-11 | `71a650fedee39809` | [poe.ninja](https://poe.ninja/poe1/builds/allflamehc/character/Dragol-2845/BigNoseSkippyNTheMogres) |
| fresh_022 | allflamessf | 3.29 | Chieftain | Righteous Fire | dot | 100 | 2026-11-11 | `e3702d731d0836ec` | [poe.ninja](https://poe.ninja/poe1/builds/allflamessf/character/vokasss-4008/Vokasss_TheDeepdiver) |
| fresh_023 | mirage | 3.28 | Chieftain | Dominating Blow | minion | 100 | 2026-11-11 | `7f4e3a79fa028456` | [poe.ninja](https://poe.ninja/poe1/builds/mirage/character/Razleraz-1044/RankOneDomBlowDemon) |
| fresh_024 | standard | 3.29 | Chieftain | Spark of the Nova | spell | 100 | 2026-11-11 | `cd4fd542cfa63e09` | [poe.ninja](https://poe.ninja/poe1/builds/standard/character/Andrikoui-2805/ColtFortyFiveTwoZigZag) |
| fresh_025 | standard | 3.29 | Deadeye | Kinetic Blast | attack | 100 | 2026-11-11 | `a65f608e5ae8c473` | [poe.ninja](https://poe.ninja/poe1/builds/standard/character/Kriskras4423-1793/Kris_Unkillabledemonkin) |
| fresh_026 | allflamehc | 3.29 | Deadeye | Cobra Lash | dot | 95 | 2026-11-11 | `88dddfe32b389463` | [poe.ninja](https://poe.ninja/poe1/builds/allflamehc/character/helikopterm%C3%A5-0580/HCFadilsFrun) |
| fresh_027 | allflamessf | 3.29 | Deadeye | Fireball | spell | 100 | 2026-11-11 | `475d941006030a3f` | [poe.ninja](https://poe.ninja/poe1/builds/allflamessf/character/MusicalTradeMark-7898/FireyRanger) |
| fresh_028 | mirage | 3.28 | Deadeye | Vaal Caustic Arrow | attack | 100 | 2026-11-11 | `fcf10d1a062dde65` | [poe.ninja](https://poe.ninja/poe1/builds/mirage/character/bisrota-2482/Deadeye_DeDustTwo) |
| fresh_029 | standard | 3.29 | Deadeye | Tornado Shot | attack | 100 | 2026-11-11 | `c455d939fa5a1876` | [poe.ninja](https://poe.ninja/poe1/builds/standard/character/hijacal-0073/O%EB%B0%B1%ED%99%94M) |
| fresh_030 | standard | 3.29 | Elementalist | Death Wish | other+dot | 100 | 2026-11-11 | `b6bf859f075e9e6b` | [poe.ninja](https://poe.ninja/poe1/builds/standard/character/zyf844-1908/%E6%AD%A3%E4%B9%89%E7%83%88%E7%84%B0) |
| fresh_031 | allflamehc | 3.29 | Elementalist | Penance Brand | totem/trap/mine+dot | 100 | 2026-11-11 | `d1117ddac12a538c` | [poe.ninja](https://poe.ninja/poe1/builds/allflamehc/character/Voracity99-5037/Voracity_DramaFarmer) |
| fresh_032 | allflamessf | 3.29 | Elementalist | Frostblink of Wintry Blast | spell | 100 | 2026-11-11 | `6cd1b3ba589e93c8` | [poe.ninja](https://poe.ninja/poe1/builds/allflamessf/character/Attesmatte-6668/Uwuplaster) |
| fresh_033 | mirage | 3.28 | Elementalist | Summon Stone Golem of Hordes | minion | 100 | 2026-11-11 | `e2a78506c4c61a2d` | [poe.ninja](https://poe.ninja/poe1/builds/mirage/character/orlp-2505/orlp_delicious_product) |
| fresh_034 | standard | 3.29 | Elementalist | Leap Slam of Groundbreaking | attack+dot | 100 | 2026-11-11 | `897504a7db2a417e` | [poe.ninja](https://poe.ninja/poe1/builds/standard/character/smeagol01-6091/smewiitch) |
| fresh_035 | standard | 3.29 | Gladiator | Blood Sacrament | other | 100 | 2026-11-11 | `8358d0136ef8a98a` | [poe.ninja](https://poe.ninja/poe1/builds/standard/character/JENEYABOO-0300/%E0%B9%8FBloods_YABOO%E0%B9%8F) |
| fresh_036 | allflamehc | 3.29 | Gladiator | Boneshatter of Complex Trauma | attack+dot | 100 | 2026-11-11 | `73b7f30564066835` | [poe.ninja](https://poe.ninja/poe1/builds/allflamehc/character/Kira225-4947/BoneKira) |
| fresh_037 | allflamessf | 3.29 | Gladiator | Divine Retribution | spell | 100 | 2026-11-11 | `4a29992d6fa3b91d` | [poe.ninja](https://poe.ninja/poe1/builds/allflamessf/character/WhitePlight-1411/GnosticHorseplaySSF) |
| fresh_038 | mirage | 3.28 | Gladiator | Animate Guardian of Smiting | minion | 100 | 2026-11-11 | `e3f9bc021c7ddd75` | [poe.ninja](https://poe.ninja/poe1/builds/mirage/character/j1epexa666-0931/Askonkas) |
| fresh_039 | standard | 3.29 | Gladiator | Doom Blast | other+dot | 100 | 2026-11-11 | `371e3f6e271151e6` | [poe.ninja](https://poe.ninja/poe1/builds/standard/character/TheBlueMagoo-5502/EthicalBuildConnoiseur) |
| fresh_040 | standard | 3.29 | Guardian | Summon Raging Spirit | minion | 100 | 2026-11-11 | `452f22b4708f35bb` | [poe.ninja](https://poe.ninja/poe1/builds/standard/character/vechnomolodoyy-2458/VMyy_Affliction) |
| fresh_041 | allflamehc | 3.29 | Guardian | Stormblast Mine | totem/trap/mine | 96 | 2026-11-11 | `f3f64ef660b9b63a` | [poe.ninja](https://poe.ninja/poe1/builds/allflamehc/character/deepshadow-7336/TwoGuardOrNotToGuard) |
| fresh_042 | allflamessf | 3.29 | Guardian | Holy Sweep | attack | 100 | 2026-11-11 | `495661fa4410f48f` | [poe.ninja](https://poe.ninja/poe1/builds/allflamessf/character/therealslimspidy-1880/HesperusTheDevout) |
| fresh_043 | mirage | 3.28 | Guardian | Spark | spell | 100 | 2026-11-11 | `c5f401050b467689` | [poe.ninja](https://poe.ninja/poe1/builds/mirage/character/Floxyyz-7166/Floxyy_) |
| fresh_044 | standard | 3.29 | Guardian | Storm Brand | totem/trap/mine | 100 | 2026-11-11 | `5bb9fe3e51fb32f2` | [poe.ninja](https://poe.ninja/poe1/builds/standard/character/Sharek99-7961/Sharek_MirageSB) |
| fresh_045 | standard | 3.29 | Hierophant | Kinetic Fusillade | attack | 100 | 2026-11-11 | `b9be20eb1123ca9b` | [poe.ninja](https://poe.ninja/poe1/builds/standard/character/Yogghy-0398/NeckAndLags) |
| fresh_046 | allflamehc | 3.29 | Hierophant | Ball Lightning of Orbiting | spell | 100 | 2026-11-11 | `0a552e29daef3e79` | [poe.ninja](https://poe.ninja/poe1/builds/allflamehc/character/SymDonB-1956/SymBerserkerSlams) |
| fresh_047 | allflamessf | 3.29 | Hierophant | Shockwave Totem | totem/trap/mine | 100 | 2026-11-11 | `1748b2d0bc7ea815` | [poe.ninja](https://poe.ninja/poe1/builds/allflamessf/character/junhaoxu-2144/SBHoboTotem) |
| fresh_048 | mirage | 3.28 | Hierophant | Kinetic Blast of Clustering | attack | 100 | 2026-11-11 | `529adc0e3ce22032` | [poe.ninja](https://poe.ninja/poe1/builds/mirage/character/MeMirrorglhf-5883/KiTicTo) |
| fresh_049 | standard | 3.29 | Hierophant | Righteous Fire of Arcane Devotion | dot | 100 | 2026-11-11 | `bf6c5d7fe8318a3e` | [poe.ninja](https://poe.ninja/poe1/builds/standard/character/Mr_Zion_Must-4643/ZionMustManaMan) |
| fresh_050 | standard | 3.29 | Inquisitor | Arc | spell | 100 | 2026-11-11 | `ea97e978b0895806` | [poe.ninja](https://poe.ninja/poe1/builds/standard/character/ampplaygame-2207/MR_AMP) |
| fresh_051 | allflamehc | 3.29 | Inquisitor | Static Strike | attack | 97 | 2026-11-11 | `e79b1c2c93724175` | [poe.ninja](https://poe.ninja/poe1/builds/allflamehc/character/DFarough-7947/Deadweight_FreshGo) |
| fresh_052 | allflamessf | 3.29 | Inquisitor | Scorching Ray of Immolation | dot | 99 | 2026-11-11 | `26ff3f1b203f931f` | [poe.ninja](https://poe.ninja/poe1/builds/allflamessf/character/Bobertoooo-6916/OldManFireGood) |
| fresh_053 | mirage | 3.28 | Inquisitor | Ice Spear of Splitting | spell | 100 | 2026-11-11 | `47d2c5b363d8c13b` | [poe.ninja](https://poe.ninja/poe1/builds/mirage/character/iDei-1549/CreatinyHeroRankMogged) |
| fresh_054 | standard | 3.29 | Inquisitor | Siege Ballista | totem/trap/mine | 100 | 2026-11-11 | `e0ec6ce452972294` | [poe.ninja](https://poe.ninja/poe1/builds/standard/character/GrindingPOV-2564/deadluckkkkk) |
| fresh_055 | standard | 3.29 | Juggernaut | Flicker Strike | attack | 100 | 2026-11-11 | `97310b6038e3526c` | [poe.ninja](https://poe.ninja/poe1/builds/standard/character/At0za-4848/Atwoza) |
| fresh_056 | allflamehc | 3.29 | Juggernaut | Fire Trap | dot | 100 | 2026-11-11 | `e17441d9de81ab94` | [poe.ninja](https://poe.ninja/poe1/builds/allflamehc/character/svenniss-2114/SchweenCharging) |
| fresh_057 | allflamessf | 3.29 | Juggernaut | Kinetic Bolt | attack | 100 | 2026-11-11 | `6a0ccdb93ad46f7d` | [poe.ninja](https://poe.ninja/poe1/builds/allflamessf/character/Inexorable_PoE-4224/%E3%81%84%E3%81%A3%E3%81%9F%E3%82%93%E4%B8%80%E6%97%A6%E3%81%AD) |
| fresh_058 | mirage | 3.28 | Juggernaut | Molten Strike of the Zenith | attack | 100 | 2026-11-11 | `0d749c05d6206c1e` | [poe.ninja](https://poe.ninja/poe1/builds/mirage/character/MugglyPOE-3993/MugglyZen) |
| fresh_059 | standard | 3.29 | Juggernaut | Detonate Dead of Scavenging | spell+dot | 100 | 2026-11-11 | `bab5f2f242152f7e` | [poe.ninja](https://poe.ninja/poe1/builds/standard/character/cg1937-6411/xxcxxdd) |
| fresh_060 | standard | 3.29 | Luminary | Bane | dot | 100 | 2026-11-11 | `80d1557a82648413` | [poe.ninja](https://poe.ninja/poe1/builds/standard/character/Pohuishka-5286/%D0%A1%D1%82%D0%90%D0%BA_%D0%91%D0%B5%D0%94%D0%BD%D0%9E%D1%81%D0%A2%D0%B8) |
| fresh_061 | allflamehc | 3.29 | Luminary | Molten Strike | attack | 100 | 2026-11-11 | `43e0a6a4199e59e3` | [poe.ninja](https://poe.ninja/poe1/builds/allflamehc/character/hrakks-6661/UndeathableMe) |
| fresh_062 | allflamessf | 3.29 | Luminary | Shield Charge | attack | 100 | 2026-11-12 | `d03369f22560374e` | [poe.ninja](https://poe.ninja/poe1/builds/allflamessf/character/fourk-7788/fourk_allflame) |
| fresh_063 | standard | 3.29 | Luminary | Pyroclast Mine of Sabotage | totem/trap/mine | 100 | 2026-11-12 | `ec16e5d704d2427e` | [poe.ninja](https://poe.ninja/poe1/builds/standard/character/%ED%95%B4%EA%B8%88%EA%B0%95-2272/%ED%95%B4%EA%B8%88%EA%B0%95_%EB%8B%AC%EB%8B%AC%EB%9E%99) |
| fresh_064 | standard | 3.29 | Luminary | Lightning Tendrils of Eccentricity | spell | 100 | 2026-11-12 | `e016f26e7f0f7026` | [poe.ninja](https://poe.ninja/poe1/builds/standard/character/Arsonist510-1990/Onionokuberinama) |
| fresh_066 | allflamehc | 3.29 | Necromancer | Animate Weapon of Ranged Arms | minion | 100 | 2026-11-12 | `4a467f04436b4c75` | [poe.ninja](https://poe.ninja/poe1/builds/allflamehc/character/demeter94-5208/Ash_TentangRasa) |
| fresh_067 | allflamessf | 3.29 | Necromancer | Unearth | spell | 100 | 2026-11-12 | `6882df592f1ce174` | [poe.ninja](https://poe.ninja/poe1/builds/allflamessf/character/Potatomato-5514/CORPSEMOLESTER) |
| fresh_068 | mirage | 3.28 | Necromancer | Summon Carrion Golem of Hordes | minion | 100 | 2026-11-12 | `716e0ed869bb36d7` | [poe.ninja](https://poe.ninja/poe1/builds/mirage/character/zen5amd-6213/miragezom) |
| fresh_071 | allflamehc | 3.29 | Occultist | Blight of Contagion | dot | 100 | 2026-11-12 | `03d9b42c2c8cfd91` | [poe.ninja](https://poe.ninja/poe1/builds/allflamehc/character/JustElliott-2169/BARISTABARRY) |
| fresh_072 | allflamessf | 3.29 | Occultist | Creeping Frost of Floes | spell+dot | 100 | 2026-11-12 | `7c9e83af72036478` | [poe.ninja](https://poe.ninja/poe1/builds/allflamessf/character/Mardibasalo-2004/owinbuadsf) |
| fresh_073 | mirage | 3.28 | Occultist | Ice Trap | totem/trap/mine | 100 | 2026-11-12 | `5473d9d2b6853d44` | [poe.ninja](https://poe.ninja/poe1/builds/mirage/character/Bolins-1467/Troulins) |
| fresh_074 | standard | 3.29 | Occultist | Blade Blast | spell+dot | 100 | 2026-11-12 | `10bcf4ec56fa060d` | [poe.ninja](https://poe.ninja/poe1/builds/standard/character/djazr-1038/YourOutieEnjoysPOE) |
| fresh_075 | standard | 3.29 | Pathfinder | Hexblast of Contradiction | spell | 100 | 2026-11-12 | `89243fc037d1a9e6` | [poe.ninja](https://poe.ninja/poe1/builds/standard/character/TahSword74-3894/MF_Boomer_) |
| fresh_076 | allflamehc | 3.29 | Pathfinder | Will of the Lords | other+dot | 93 | 2026-11-12 | `2b3fedcf19102a64` | [poe.ninja](https://poe.ninja/poe1/builds/allflamehc/character/damianosbbx-2171/recesmiercibakzdupy) |
| fresh_077 | allflamessf | 3.29 | Pathfinder | Toxic Rain | dot | 100 | 2026-11-12 | `95b9ba482a69a914` | [poe.ninja](https://poe.ninja/poe1/builds/allflamessf/character/Gerlox01-2485/Jebe_xoxo) |
| fresh_078 | mirage | 3.28 | Pathfinder | Poisonous Concoction of Bouncing | attack+dot | 100 | 2026-11-12 | `9e7bff95d6860984` | [poe.ninja](https://poe.ninja/poe1/builds/mirage/character/MrKinGG-1250/Ckoyba) |
| fresh_079 | standard | 3.29 | Pathfinder | Viper Strike of the Mamba | attack+dot | 100 | 2026-11-12 | `079964259c7702e3` | [poe.ninja](https://poe.ninja/poe1/builds/standard/character/WhySoDark-0513/WhySoDarkXVIII) |
| fresh_080 | standard | 3.29 | Reliquarian | Smite | attack | 100 | 2026-11-12 | `b0ad2e6ceaac8468` | [poe.ninja](https://poe.ninja/poe1/builds/standard/character/ricoclab-4325/StormBrand_Stacker) |
| fresh_081 | allflamehc | 3.29 | Reliquarian | Cold Snap | spell+dot | 100 | 2026-11-12 | `622a5d5d0f1f47a1` | [poe.ninja](https://poe.ninja/poe1/builds/allflamehc/character/bloodstain77-0213/BloodstainBlondeLover) |
| fresh_082 | allflamessf | 3.29 | Reliquarian | Viper Strike | dot | 99 | 2026-11-12 | `b1594f4a23d189ab` | [poe.ninja](https://poe.ninja/poe1/builds/allflamessf/character/H3rB1985-0551/Vipirina) |
| fresh_083 | mirage | 3.28 | Reliquarian | Dark Pact | other | 100 | 2026-11-12 | `c09f6f9847475f86` | [poe.ninja](https://poe.ninja/poe1/builds/mirage/character/UranosTheSkyBorn-5388/StraightJorkingIt) |
| fresh_084 | standard | 3.29 | Reliquarian | Dark Bargain | minion | 100 | 2026-11-12 | `8df7e216fa3f6177` | [poe.ninja](https://poe.ninja/poe1/builds/standard/character/wuttdan-6758/__KxyKxmKxd) |
| fresh_085 | standard | 3.29 | Saboteur | Hexblast | spell | 100 | 2026-11-12 | `af437426d63f9b6d` | [poe.ninja](https://poe.ninja/poe1/builds/standard/character/%EB%B6%95%EB%B6%95%EC%B9%98%ED%83%80-2091/%ED%8D%BC%ED%8D%BC%EB%9F%AC%ED%8D%BD%ED%8D%BD) |
| fresh_086 | allflamehc | 3.29 | Saboteur | Fire Trap of Blasting | totem/trap/mine | 97 | 2026-11-12 | `ef0310d894e8aa53` | [poe.ninja](https://poe.ninja/poe1/builds/allflamehc/character/flyingaxekill-1813/FloralDeal) |
| fresh_087 | allflamessf | 3.29 | Saboteur | Wild Strike | attack | 100 | 2026-11-12 | `64ab3eb772eda48c` | [poe.ninja](https://poe.ninja/poe1/builds/allflamessf/character/NoCheatingDevice-7585/DOGSHOCKCOLLARWEEKLY) |
| fresh_088 | mirage | 3.28 | Saboteur | Exsanguinate | dot | 100 | 2026-11-12 | `8dcff83b3ab50575` | [poe.ninja](https://poe.ninja/poe1/builds/mirage/character/%D0%9C%D0%B0%D0%BC%D0%B8%D0%BD%D0%B0%D0%A7%D0%B5%D1%80%D0%B5%D1%88%D0%BD%D1%8F-6996/vova_kobra) |
| fresh_089 | standard | 3.29 | Saboteur | Summon Skeletons | minion | 100 | 2026-11-12 | `7991e73c8fd9cce9` | [poe.ninja](https://poe.ninja/poe1/builds/standard/character/_Tranner-6376/_ZaWardo) |
| fresh_090 | standard | 3.29 | Slayer | Elemental Hit | attack | 100 | 2026-11-12 | `4924b755662ab227` | [poe.ninja](https://poe.ninja/poe1/builds/standard/character/PilchasX-5638/Uncle__Kuncle) |
| fresh_091 | allflamehc | 3.29 | Slayer | Pestilent Strike | dot | 99 | 2026-11-12 | `9cf46b3c01755add` | [poe.ninja](https://poe.ninja/poe1/builds/allflamehc/character/Elemelentalist-1949/LMFrenzyMan) |
| fresh_092 | allflamessf | 3.29 | Slayer | Elemental Hit | attack | 100 | 2026-11-12 | `ff8d631491555e34` | [poe.ninja](https://poe.ninja/poe1/builds/allflamessf/character/seanpw-2617/Mal_Slayzz) |
| fresh_093 | mirage | 3.28 | Slayer | Herald of Ice | spell | 100 | 2026-11-12 | `2ba471b0521b38a1` | [poe.ninja](https://poe.ninja/poe1/builds/mirage/character/_tsM-6218/tsM_hole) |
| fresh_094 | standard | 3.29 | Slayer | Frost Blades of Katabasis | attack | 100 | 2026-11-12 | `f57d2b60f180afb0` | [poe.ninja](https://poe.ninja/poe1/builds/standard/character/Zulugoon-2480/Zoolaaa) |
| fresh_096 | allflamehc | 3.29 | Trickster | Arc of Oscillating | spell | 94 | 2026-11-12 | `a67cdc85a9849a4d` | [poe.ninja](https://poe.ninja/poe1/builds/allflamehc/character/flyingaxekill-1813/MustUnleash) |
| fresh_097 | allflamessf | 3.29 | Trickster | Blight | dot | 100 | 2026-11-12 | `8217ce508bd9f0d8` | [poe.ninja](https://poe.ninja/poe1/builds/allflamessf/character/The_Marihumana-9942/Marihumana) |
| fresh_098 | mirage | 3.28 | Trickster | Wave of Conviction of Trarthus | spell | 100 | 2026-11-12 | `3a6f78217ae4fcb0` | [poe.ninja](https://poe.ninja/poe1/builds/mirage/character/SammytheGerbil-8808/Kaboom_kablow_Kaboom) |
| fresh_100 | standard | 3.29 | Warden | Ice Shot of Penetration | attack | 100 | 2026-11-12 | `617a1655f35faf02` | [poe.ninja](https://poe.ninja/poe1/builds/standard/character/NT_xSUBx-6092/%D0%92%D0%B8%D1%85%D1%80%D1%8C%D0%A1%D1%82%D1%80%D0%B5%D0%BB%D0%A1%D0%A2%D0%94) |
| fresh_101 | allflamehc | 3.29 | Warden | Freezing Pulse | spell | 99 | 2026-11-12 | `71eb136f75abde6a` | [poe.ninja](https://poe.ninja/poe1/builds/allflamehc/character/_Hughezy-0130/SpellWardenIsBack) |
| fresh_102 | allflamessf | 3.29 | Warden | Artillery Ballista | totem/trap/mine | 90 | 2026-11-12 | `a435e0c9a4990bc0` | [poe.ninja](https://poe.ninja/poe1/builds/allflamessf/character/gogogospeed11-4584/neversinkssdeey) |
| fresh_103 | mirage | 3.28 | Warden | Ethereal Knives of the Massacre | spell | 100 | 2026-11-12 | `a3eabfa04111bd7e` | [poe.ninja](https://poe.ninja/poe1/builds/mirage/character/heygyus-0416/ResurrectSanest) |
| fresh_104 | standard | 3.29 | Warden | Lightning Arrow | attack | 100 | 2026-11-12 | `ff6ed92dca965c94` | [poe.ninja](https://poe.ninja/poe1/builds/standard/character/4aka4aka-0332/Bad_Im) |

## Dropped

| stem | league | class | main skill | reason | source |
|---|---|---|---|---|---|
| fresh_065 | standard | Necromancer | Summon Carrion Golem of Scavenging | fetch: poe.ninja character API HTTP Error 404: Not Found for the pick and its 3 fall-backs | [poe.ninja](https://poe.ninja/poe1/builds/standard/character/DreadHed-3485/NippySixx) |
| fresh_069 | standard | Necromancer | Summon Flame Golem of Hordes | fetch: poe.ninja character API HTTP Error 404: Not Found for the pick and its 3 fall-backs | [poe.ninja](https://poe.ninja/poe1/builds/standard/character/smyzi-2395/______________YoOO) |
| fresh_070 | standard | Occultist | Forbidden Rite of Soul Sacrifice | fetch: poe.ninja character API HTTP Error 404: Not Found for the pick and its 3 fall-backs | [poe.ninja](https://poe.ninja/poe1/builds/standard/character/SleepySleepingSleeper-0743/GolemsSuck) |
| fresh_095 | standard | Trickster | Lightning Strike of Arcing | fetch: poe.ninja character API HTTP Error 404: Not Found for the pick and its 3 fall-backs | [poe.ninja](https://poe.ninja/poe1/builds/standard/character/ZamaWompWomp-4634/ZamaRingJail) |
| fresh_099 | standard | Trickster | Raise Zombie of Falling | fetch: poe.ninja character API HTTP Error 404: Not Found for the pick and its 3 fall-backs | [poe.ninja](https://poe.ninja/poe1/builds/standard/character/Troceleng-3589/Varjolukki) |
