# Third-party notices for `assets/`

Everything in this directory except this file and `README.md` was copied or compiled by
`tools/pob-vendor` from one snapshot of Path of Building Community and its data dump. The
snapshot is recorded in `index.json` (`source`): Path of Building Community v2.67.2, commit
`16de4b82d57f1c0de6eb40f37143c32d4da36a02`.

## Path of Building Community

`ui/`, `tree/`, `timeless/`, `text/`, `fonts/` and the game data in `gamedata/` come from
[Path of Building Community](https://github.com/PathOfBuildingCommunity/PathOfBuilding)
(`src/Assets`, `src/TreeData`, `src/Data/TimelessJewelData`, `changelog.txt`, `help.txt`,
`runtime/SimpleGraphic/Fonts`, and the data PoB's `src/Data`, `src/Modules` and
`src/TreeData/*.lua` build at load time, as dumped by `crates/pob-dump`).

Path of Building is MIT-licensed:

```text
Copyright (c) 2016 David Gowor

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

PoB's complete `LICENSE.md`, including the notices of the components it bundles, is
`licenses/PathOfBuilding-LICENSE.md`.

## Path of Exile game art and data (Grinding Gear Games)

The passive tree art (`tree/`), the item and UI art (`ui/`), the ascendancy portraits
(`ui/ascendants/`) and the game data (`gamedata/`, `timeless/`: skills, items, mods, passive
trees, timeless jewel tables) are Path of Exile content. Path of Exile is © Grinding Gear Games.
Path of Building extracted them from the game client and the official passive tree export
(<https://github.com/grindinggear/skilltree-export>); they are included here as Path of
Building ships them. This project is not affiliated with or endorsed by Grinding Gear Games.

## Fonts

The bitmap fonts in `fonts/` are SimpleGraphic's renderings of:

- **Bitstream Vera Sans Mono**: Copyright (c) 2003 by Bitstream, Inc. All Rights Reserved.
  Bitstream Vera is a trademark of Bitstream, Inc. (Bitstream Vera Fonts license; full text in
  `licenses/PathOfBuilding-LICENSE.md`.)
- **Liberation Sans**, **Liberation Sans Bold**: Digitized data copyright (c) 2010 Google
  Corporation with Reserved Font Arimo, Tinos and Cousine; Copyright (c) 2012 Red Hat, Inc. with
  Reserved Font Name Liberation. SIL Open Font License 1.1 (full text in
  `licenses/PathOfBuilding-LICENSE.md`).
- **Fontin**, **Fontin Italic**, **Fontin SmallCaps**, **Fontin SmallCaps Italic**: by Jos
  Buivenga (exljbris), a free font. PoB's `LICENSE.md` carries no notice for it; it is
  included as PoB ships it.
