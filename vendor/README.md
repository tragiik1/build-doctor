# Vendored native sources

Both are pinned to exactly what the Path of Building PoE2 app ships, so the
engine runs PoB's Lua on the same runtime the app does.

| Directory | Upstream | Pinned commit | Why this commit |
|---|---|---|---|
| `luajit-src/luajit2` | https://github.com/LuaJIT/LuaJIT (branch v2.1) | `2460b3ff93a1c955de3d62cfc825de7d68dc272e` (2026-07-20) | `runtime/lua51.dll` in PoB2 v0.23.1 reports `LuaJIT 2.1.1784580905`, the commit time of 2460b3ff. Also the commit pinned in PathOfBuilding-SimpleGraphic `vcpkg-ports/ports/luajit/2026-07-20_1`. |
| `luautf8` | https://github.com/starwing/luautf8 | `d65ebfa4a00d6e582970ab729013bb2a629e74c9` | Submodule commit in PathOfBuilding-SimpleGraphic `libs/luautf8`. |

`luajit-src/` is the `luajit-src` crate (MIT, mlua-rs) at 210.7.5 with its
`luajit2/` source tree swapped for the pinned commit and one `#![allow(deprecated)]`
added to `src/lib.rs`. It is wired in through
`[patch.crates-io]` in the workspace `Cargo.toml`.

To move to a new pin: replace `luajit2/` with `COPYRIGHT Makefile README dynasm src`
from the new LuaJIT commit, write its `git show -s --format=%ct` into
`luajit_relver.txt`, then run `cargo test` and `doctor verify`.
