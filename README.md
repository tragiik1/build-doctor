# build-doctor

Path of Exile 2 build doctor. Every number comes from Path of Building PoE2's
own calculation code, run headless on the exact LuaJIT build the PoB2 app
ships, and is checked against builds saved by the PoB2 app.

- Research, design and plan: [`docs/RESEARCH.md`](docs/RESEARCH.md)
- Running log of changes, tests and open questions: [`CHANGES.md`](CHANGES.md)

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
