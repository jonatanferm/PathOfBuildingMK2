# CCR-0094: PF-E — shared-mod writes in the buff loop, `notBuff`, and two frozen signatures

**Status: applied 2026-11-10 by CCR2 (§1, §2 contracts.md#mod-mutation; §3 = CCR-0030 #7, §4 superseded by CCR-0085 §1); §1's Tag field and §5 (= CCR-0063) need code. Renumbered from 0034 by CCR2 (0034 stays U4's). See AMENDMENTS A8.**

Filed status: proposed by PF-E (2026-11-09)

Unit PF-E ports CalcPerform.lua 2308-2996 (`crates/pob-calc/src/modules/calc_perform/buffs.rs`).
Five places where the Lua does something the frozen contracts cannot say directly. Each is
worked around inside `buffs.rs` (marked `CCR-0094 §n`); nothing outside PF-E's files is edited.

## §1 `warcryPowerBonus`: a tag write into a mod PF-E does not own (CCR-0014 §3)

CalcPerform.lua:2380-2384 writes `warcryBuff[1].warcryPowerBonus` into the first tag of every
warcry buff mod. The mod is `buff.modList[i]`, which CalcActiveSkill.lua:862 inserted without a
copy (`t_insert(modList, skillModList[i])`); MS-B's CCR-0015 §5 found the same tag in PoB's
parser cache, i.e. the write reaches process-global data. Reads: 2398, 2411 (PF-E) and
3090, 3099 (PF-F, through `buffExports.Warcry[name].modList`, which `AddList`s these mods at 2438).

`pob_gamedata::Tag` has no `warcryPowerBonus` field (CCR-0014 §3 proposed one; not applied), but
its `extra` table round-trips the key (`pob_oracle::mod_json` puts it there on injection).

**Port:** the mod is rebuilt with `tags[0].extra.warcryPowerBonus` set and replaces the entry of
`buff.modList` (the list every later read goes through). Readers use
`tag.extra_get("warcryPowerBonus")` (PF-F must do the same until CCR-0014 §3 adds the field).
**Known difference:** other holders of the same Lua mod table (another skill's list, the parser
cache) keep the old tag. The bonus is recomputed before every read in the same branch, so only a
dump of such an alias could show it.

**Proposed:** apply CCR-0014 §3 (`pub warcryPowerBonus: Option<f64>` on `Tag`) and add this site
to contracts.md#mod-mutation as a shared-target write ported as "rebuild and replace in
`buff.modList`".

## §2 `setSpectreSource`: a source write into shared mods (new site, not in contracts.md#mod-mutation)

CalcPerform.lua:2780-2792 (a closure inside the minion-skill loop) does
`modList[i].source = source` on every mod of its argument (only for spectres). It is called on
`srcList` (2870, 2885), `newModList` (2895) and the Totem `srcList` (2928). `newModList:AddList(
buff.modList)` (2893) and `srcList:ScaleAddList(buff.modList, mult)` with `mult == 1`
(ModStore.lua:100-101 → `AddList`) store **references** to `buff.modList`'s mods (and to
`extraAuraModList`'s fresh copies), so the write changes those mods in place: after 2895 every
mod of the minion skill's `buff.modList` has the spectre source, and later reads of
`buff.modList`/`extraAuraModList` in the same branch see it.

contracts.md#mod-mutation lists no such site; §3.6 requires a CCR for a shared target.

**Port:** `setSpectreSource` builds each re-sourced mod, replaces it in the list it was given (the
same store object, its entries rebuilt in order) and in the aliases the branch reads afterwards
(`buff.modList` entries and `extraAuraModList` entries that are the same `Arc`).
**Known difference:** a Lua mod table also referenced from elsewhere (two spectres whose
`buff.modList` share a data mod; a mod already merged into `buffs` keeps the identical source
anyway) is not updated.

**Proposed:** add CalcPerform.lua 2788-2790 to contracts.md#mod-mutation with this treatment.

## §3 `buffs[name].notBuff = true` (CCR-0030 #7, not applied)

CalcPerform.lua 2331, 2810, 2834 set `notBuff` on the ModList `mergeBuff` stored in `buffs`;
3346 and 3425 (PF-F) read it. `ModStore` has no field for it.

**Port (workaround):** `set_notBuff` records the key where the probe export and injection keep
mod-store side fields (`pob_calc::probe` registry, `push_side_extra`), so the L4 diff sees it.
Without the `probe` feature the write is dropped: **the shipped calc loses `notBuff`** until
CCR-0030 #7 (`pub notBuff: Cell<bool>` on `ModStore`) is applied. After that, `set_notBuff`
becomes `store.notBuff.set(true)`.

**Proposed:** apply CCR-0030 #7.

## §4 `mergeBuff(src: &Table, …)` receives ModLists

The frozen PF-A signature takes `src: &Table`, but 18 of the 20 PF-E call sites pass a ModList
(`srcList`/`newModList`: 2329, 2341, 2360, 2401, 2432, 2475, 2490, 2515, 2546, 2582, 2651,
2750, 2807, 2831, 2871, 2886, 2929, 2990); only 2808 and 2832 pass the plain `buff.modList`.
`mergeBuff` only walks `src` with `ipairs` (CalcPerform.lua:50), which sees a ModList's mods in
list order. **Port:** a ModList source is passed as a fresh table of its mods (`list_table`).
**Proposed (optional):** `src: &[ModRef]` in the PF-A signature.

## §5 `NewMod` without a source

`ModStore::NewMod` takes `source: &str`; CalcPerform.lua 2372 (`NewMod("Num…Exerts", "BASE", v)`)
and 2762 (`NewMod("WitherEffectStack", "MAX", effect)`) pass none, so `createMod` leaves
`source` nil (ModTools.lua:38-41). **Port:** `AddMod(createMod(…, None, …))`
(`new_mod_no_source`). **Proposed (optional):** `source: Option<&str>` or a `NewMod_nosource`.

## Acceptance note (test policy)

`crates/pob-oracle/tests/PF-E.rs` runs L4 (`perform:2308` → `perform:2997`, one PoB process per
pair) and reports a hit whose run calls an `unported!` callee (PF-A `mergeBuff` /
`determineCursePriority`, OF-A `calcSkillDuration`) as **pending** with the stub sites; the gate
(curated 100 %, all57 ≥ 98 %) applies to the other hits. With those three callees stubbed (the
state of `rust-port` when PF-E was written), every buff build is pending.

Measured at PF-E's commit (dumps: `oracle.sh run --stage 'perform:2308@main,perform:2997@main'`
for every `.parity/builds` build):
- as committed (callees stubbed): curated 24 hits, 24 pending; all57 57 hits, 2 pass (no buff
  merge reached a stub), 55 pending, 0 fail, 0 error.
- with temporary, uncommitted ports of `mergeBuff`, `determineCursePriority` and
  `calcSkillDuration` (local check only, reverted): curated 24/24 exact, all57 57/57 exact.
- mutation check: dropping one write (`modDB.conditions.AffectedByAura`, 2467) fails every
  aura build of the run.
