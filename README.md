# build-doctor

Path of Exile 2 build doctor. Every number comes from Path of Building PoE2's
own calculation code, run headless on the exact LuaJIT build the PoB2 app
ships, and is checked against builds saved by the PoB2 app.

- Research, design and plan: [`docs/RESEARCH.md`](docs/RESEARCH.md)
- Running log of changes, tests and open questions: [`CHANGES.md`](CHANGES.md)

## Desktop app

Download `build-doctor.exe` from
[the latest release](https://github.com/tragiik1/build-doctor/releases/latest)
and run it. It updates itself: when a newer release exists it offers
"Update and restart", checks the download's SHA-256 against the release and
swaps itself in place. Windows 11 already has the WebView2 runtime it uses. On first
start it offers to install Path of Building (one download, ~390 MB, every
file verified). Then paste a PoB code or link:

- **Your build** (the first page): the coach. What your numbers mean and
  what to do, in order: each step says why (the mechanic, with your
  numbers), how (including where the stat rolls on gear), what PoB says it
  changes, and links its source.
- **Diagnosis**: every finding, measured. Elemental resistances
  below their maximum, chaos resistance one mod at a time, hit chance below
  100%, each with the amount needed and what PoB says it changes; a
  resistance penalty that is not the endgame one; PoB's warnings that
  concern skills you use; and what one more skill level of each kind is
  worth.
- **Gear upgrades**: pick a slot; PoB measures what every stat that can
  roll there is worth to your build, then opens a trade search that lists
  items for sale best first. Type your league once; the app itself makes no
  request to GGG.
- **Check an item**: paste an item copied in game with Ctrl+C. PoB equips
  it, exactly as written, in each slot it fits and shows what changes:
  damage, survival, resistances, and any requirement it breaks.
- **Stats**: your stats as PoB calculates them, whether PoB reproduces the
  build's saved numbers exactly, the conditions they assume and the mods PoB
  cannot read.
- **Realistic numbers**: what each assumed condition is worth; tick the ones
  you keep up.
- **Passive tree plan**: the best point-neutral swaps, step by step, each
  confirmed by PoB, with the final tree as a PoB code to copy.

To build it yourself: `cargo build --release -p build-doctor`.

## Build

Needs Rust 1.88+ and a C compiler (MSVC on Windows, via Visual Studio Build
Tools; gcc or clang elsewhere).

```
cargo build --release
```

The binary is `target/release/doctor` (`doctor.exe` on Windows).

## Use

```
doctor engine install                 # download PoB2 0.23.1 and verify every file
doctor check <code | file | link>     # calculate a build, compare with its saved stats
doctor check <...> --all              # every stat PoB produced, exactly as PoB wrote it
doctor coach <build>                  # what to do, in order, and why
doctor diagnose <build>               # what to fix first, each fix measured by PoB
doctor item <build> <item.txt>        # is this item an upgrade? (- reads it from stdin)
doctor gear <build> [--slot "Ring 1" --league Standard]
                                      # what each gear stat is worth, and a trade search link
doctor realistic <build>              # numbers without the combat conditions the build assumes
doctor realistic <build> --keep shocked --keep "skill recently"
                                      # ...keeping only the ones you really sustain
doctor todo <build> [--keep …]        # passive tree plan: best point-neutral swaps, step by step
doctor todo <build> --goal survival --export plan.pob
                                      # ...for survival, final tree saved as a PoB code
doctor verify                         # precision gate over fixtures/
doctor verify --sweep                 # which PoB releases reproduce which fixtures
```

Links from pobb.in, poe.ninja (`/poe2/pob/`), maxroll (`/poe2/pob/`) and
poe2db (`/pob/`) are accepted. PoB installs live in `%LOCALAPPDATA%\build-doctor`
on Windows (override with `--data-dir` or `DOCTOR_DATA_DIR`).

## Precision

`doctor verify` loads every fixture build and requires PoB's freshly computed
stats to equal the stats the PoB2 app saved in it, string for string. A build
whose numbers keep changing between recalculations (a PoB bug), or that PoB
cannot load properly (for example an unknown passive tree), is refused with
PoB's reason, never reported.

`doctor check` also shows what every number depends on: the configuration
the build assumes (enemy Shocked, Frozen, boss type …) and the item mods PoB
cannot parse, which no number includes.

## Tests

```
cargo test                                                     # unit tests
DOCTOR_POB_DIR=<install> cargo test -p pob -- --include-ignored  # engine: sandbox, determinism
doctor verify                                                  # precision gate
```

CI runs all three on Windows and Linux.

This product isn't affiliated with or endorsed by Grinding Gear Games in any way.
