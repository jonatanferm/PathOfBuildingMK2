# CCR-0149: CS3FIX: model markers in the exporter's `$keys` labels

Unit: **CS3FIX** (hygiene; test-only). Status: **proposed**. CS3 works around it in its own
diff (`crates/pob-oracle/tests/CS3.rs` `object_key_order`, class `$keys order (object keys)`,
marked `// CCR-0149`). The number was reserved with `scripts/port/next_ccr.sh`.

## 1. Problem

The full suite at `rust-port` b88490d6 (`.parity/fullsuite/20261113-0114/pob-oracle.log`
4211-4426) failed CS3 L4 (1 hit) and L5 (3 runs), all on allflame_exp_051. In each case both
sides held the same two keys at `….effectList[2].isSupporting` (the Aspect of Arakaali
skill's support effect), but at swapped `[@1:key]` / `[@2:key]` positions.

The order does **not** come from LuaJIT `pairs`:

- state_dump.lua:514-563 writes `$keys` sorted by `keyLabel`. A plain table key gets
  `"9:" .. shallowLabel(k, 1)`: its members sorted by key, bytewise. probe.rs `key_label` /
  `shallow_label` (crates/pob-calc-core/src/probe.rs:2234-2390) do the same for our export.
- PoB's order is stable. Three separately patched PoB runs of allflame_exp_051 (probe-cache
  source keys `494763825aaec213`, `e775da7d1fc6030d` and `0ce5008dbbd7fc85`, build key
  `e7c19247e06d55d0`, `state/initEnv:end#7`) all write Lightning Arrow first and Aspect of
  Arakaali second.
- Our label is not PoB's. CS3 creates the granted gem (CalcSetup.lua:1487-1503;
  init_env_skills.rs:567-590) with the CCR-0145 §4 marker
  `extra["$enableGlobal2Nil"] = true`. `members_of` → `struct_members` gives `shallow_label`
  the `extra` entries too, and `$` (0x24) sorts before every data member name. So the labels
  come out as follows:

  | key | PoB's label | our label |
  |---|---|---|
  | Aspect of Arakaali | `9:color="^xC8C8C8",…` | `9:$enableGlobal2Nil=true,color=…` |
  | Lightning Arrow | `9:color="^x70FF70",count=1,…` | `9:color="^x70FF70",count=1,…` |

  Ours therefore sorts Arakaali first. (The dump of ours is `CS3_DUMP_OURS`: `$id 50` carries
  `"$$enableGlobal2Nil": true`.)

No PoB code reads `isSupporting` in order. It is written at CalcActiveSkill.lua:150-151 and
CalcSetup.lua:1692. GemSelectControl.lua:702-703 and CompareTab.lua:4179-4185 index it with one
gem, and SkillsTab.lua:1380 only tests `next(...)`. The swap is therefore an export artifact
and not a port difference. It exists only on our side, because only the Rust model code sets the
markers: CS3's gem literals for the gem markers, and `model::skills_tab` for the group markers.
Injection from a PoB dump never sets them. That explains why it shows up in CS3 (L4 runs CS3's
code, L5 runs the whole initEnv) and in no later segment's L4 test, since those inject PoB's
gems.

## 2. Proposed change (probe.rs, `$keys` labels only)

The labels should leave out the CCR-0019 / CCR-0145 marker keys, which stand for a nil field
that PoB does not have. Then a key object's label is PoB's whenever its data members are:

```diff
     fn shallow_label(o: &PObj, depth: i32, owners: &HashMap<usize, &'static str>) -> String {
         let m = members_of(o, owners);
         let mut s_parts: Vec<(String, String)> = Vec::new();
         let mut n_parts: Vec<(f64, String)> = Vec::new();
         for (k, v) in &m.entries {
             match k {
+                // CCR-0149: the nil markers of CCR-0019 / CCR-0145 §4 (`$labelNil`,
+                // `$includeInFullDPSNil`, `$enableGlobal1Nil`, `$enableGlobal2Nil`) are not
+                // members of PoB's table: they must not order `$keys`
+                PKey::S(s) if is_nil_marker(s) => {}
                 PKey::S(s) => s_parts.push((s.clone(), label_one(v, depth, owners))),
```

`is_nil_marker` should match exactly the marker names that `model::skills_tab` defines
(`LABEL_NIL` and the `$enableGlobal<n>Nil` / `$includeInFullDPSNil` names). This changes no
`$keys` content, only its order. The markers' own export (CS3's `model-marker` class) is
unchanged.

Typed nil-defaults (`noSupports=false` / `triggered=false` where PoB has nil) also enter the
label, after `color`. On a tie of the earlier members they could still reorder keys. Fixing
that would need the `nil-default` knowledge in the exporter, so it is out of scope here. CS3's
class covers it.

## 3. Workaround (applied, CS3 only)

`CS3.rs` `object_key_order`: a mismatch at or below `T[@i:key]` / `T[@i]` counts as
`$keys order (object keys)` when, on both sides, every `$keys` key of `T` is a `$ref` (so the
key object is compared in full at the path the ref names), the keys are distinct, and the two
sides have the same keys with equal values (strict tolerance). Without §2 it stays necessary.
With §2 it still covers label ties (address order) and nil-default labels.
