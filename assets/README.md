# The asset bundle

Every file the desktop and web apps read at run time, vendored from one Path of Building snapshot
(VENDOR; the snapshot is `index.json`'s `source`). The apps need no PoB checkout. Licenses and
sources: `THIRD_PARTY_NOTICES.md`.

| path | what | from (PoB) |
|---|---|---|
| `index.json` | format 1: the logical → bundle path maps (`script`, `runtime`), every file's size and sha256 (`files`), the snapshot (`source`) | (written by `tools/pob-vendor`) |
| `gamedata/data.bin`, `gamedata/trees/<ver>.bin` | the frozen game data (postcard, `pob_gamedata`) | `data/dump` (`pob-dump compile`) |
| `gamedata/trees/<ver>.sprites.bin`, `3_19.assets.bin` | the tree UI data PoB reads from `TreeData/<ver>/sprites.lua`, `TreeData/3_19/Assets.lua` | `data/dump/trees/*.json`, compiled |
| `ui/` | item, tooltip and tree-overlay art; `ui/ascendants/` | `src/Assets/` |
| `tree/` | passive tree art, one directory per tree version, `tree/legion/` | `src/TreeData/` (images only) |
| `timeless/` | timeless and abyss jewel lookup tables (`.zip`, `.zip.partN`) | `src/Data/TimelessJewelData/` |
| `fonts/` | SimpleGraphic's bitmap fonts (`.tgf` + `.tga` atlases) | `runtime/SimpleGraphic/Fonts/` |
| `text/` | `changelog.txt`, `help.txt` (the About popup); `manifest.xml` (below) | the checkout root; the manifest is written by `tools/pob-vendor` |
| `licenses/` | PoB's `LICENSE.md` | the checkout root |
| `web-prefetch.txt` | what the web app fetches before it starts | (written by `tools/pob-vendor`) |

## How the apps read it

The app still names files the way PoB's script and runtime directories hold them
(`Assets/ring.png`, `TreeData/3_27/skills-3.jpg`, `Fonts/Fontin.tgf`). `pob_host::bundle` maps
each such *logical* path to its bundle file through `index.json`; identical files of different
tree versions are stored once (the index points several logical paths at one file). Paths the
index does not list do not exist for the app.

- Desktop: `pob_desktop::locate_assets` takes `$POB_ASSETS`, else `assets/` next to the
  executable, else this directory (compile-time default). The decompressed timeless tables are
  cached outside the bundle (`pob_desktop::lut_cache_dir`); user data goes to the platform's user
  folder, never here.
- `manifest.xml` (in `text/`) is a release-style PoB manifest, `<Version number="2.67.2"
  branch="master" platform="win32"/>`: `launch:OnInit` takes `launch.versionNumber` (PoB's
  version: the HTTP user agent, build-site and archive requests, the error prompt) from it, and
  with a branch and a platform set the app runs as an installed release, so dev mode is off
  (Launch.lua:59-62; PoB's checkout manifest has neither, which would turn dev mode on and put
  the user data into the bundle, Main.lua:89-91).
- Web: the page serves this directory as `assets/` (`app/pob-web/index.html` copies the parts the
  web needs into `dist/assets/`).

## Updating

Only while a PoB checkout and its dump exist:

```sh
scripts/port/cargo.sh build -p pob-vendor
target/debug/pob-vendor --pob .PathOfBuilding --dump data/dump --out assets
```

`tools/pob-vendor/inventory.txt` lists the files (recorded from the running app, see its header).
The app's standalone tests (`app/pob-desktop/tests/standalone.rs`) check the bundle against the
index and run the app on it.
