# `tests/u30/user1` (owner U30)

The user dir of the U30 scenarios that need a build with two skill sets: `Settings.xml` is a
copy of `fixtures/user1`'s, and `Builds/two_sets.xml` is `fixtures/user1`'s
`allflame_exp_001.xml` with a second `<SkillSet id="2" title="Leap set">` holding copies of its
Boots and Body Armour groups (generated once; the scenarios never modify it).

`Builds/item_source.xml` (`f_item_source`) is `.parity/builds/allflame_exp_028.xml` with its
first two `<Skill>` elements swapped (the item-granted Severed in Sleep group first,
`mainSocketGroup` 1 -> 2), so the group displayed on load has a `sourceItem` (generated once).
