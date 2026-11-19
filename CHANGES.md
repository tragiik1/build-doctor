# CHANGES

Running log so any session can pick up where the last stopped.
Newest first.

## 2026-11-19: M0 precision harness (Rust)

**Decisions (from user):** Windows 11; all our code in Rust, lightweight; trade
fetch (L1) on; coach yes, but only grounded in computed numbers. No user
fixture builds yet, so public builds are used.

**What changed**
- Cargo workspace: `crates/pob` (engine library), `crates/doctor` (CLI).
- `vendor/luajit-src`: LuaJIT pinned to `2460b3ff`, the commit inside PoB2
  v0.23.1's `lua51.dll` (version string `LuaJIT 2.1.1784580905`), wired in via
  `[patch.crates-io]`. `vendor/luautf8` at `d65ebfa` (PoB's submodule commit),
  linked statically and preloaded. See `vendor/README.md`.
- `crates/pob/lua/bridge.lua`: runs PoB's own `HeadlessWrapper.lua` in a
  sandbox. Relative paths resolve to PoB's src, so the process working
  directory is untouched. Writes, deletes, renames, `os.execute` and `io.popen`
  fail. `os.getenv` returns nil, which also keeps ModCache on as in the app.
  Settles by recalculating until PoB's saved output is identical twice (max 8
  passes).
- `Install`: downloads a PoB2 release archive from GitHub, keeps code and data
  only (~46 MB instead of ~400 MB), and verifies every file against PoB's
  `manifest.xml` SHA-1. Text files are hashed with CRLF endings, as PoB's
  `update_manifest.py` does.
- `StatSheet`: parses `<PlayerStat>`, `<MinionStat>` and `<FullDPSSkill>`
  regardless of attribute order. Compares as exact strings.
- CLI: `doctor engine install|list|verify`, `doctor check`, `doctor verify [--sweep]`.
- `fixtures/`: 13 public builds (6 classes, incl. minion, totem, attack,
  spell). 11 are oracles, 2 are quarantined with reasons.
- `.github/workflows/ci.yml`: lint, and on Windows and Linux: build, unit
  tests, PoB download and verify, engine tests, precision gate.

**What was tested**
- `doctor verify`: **34 exact, 0 failed**, over PoB2 0.17.1, 0.18.0, 0.19.0,
  0.20.0, 0.21.0, 0.21.1, 0.22.0, 0.23.0 and 0.23.1 (pinned: 1 fixture, the
  0.5.5 Shaman). 98 to 119 stats per build, string-identical. Engine reuse
  across builds gives identical output.
- Engine tests: sandbox blocks writes, deletes and exec and hides the
  environment; PoB files stay readable; five fresh engines give identical stats
  on two real builds (guards against LuaJIT's per-state hash seed leaking into
  results); unparseable XML is a reported error.
- 14 unit tests (code decode/encode, SHA-1 rule, manifest filtering and path
  traversal, stat diffing, links, number formatting).
- Load time per build 0.5 to 2.4 s (load and settle, release build). Not yet
  measured: the per-change what-if cost on real builds (needed for M2).
- Not run here: `doctor engine install`, because GitHub archive downloads are
  blocked by this container's proxy. CI exercises it. Windows only runs in CI.

**Findings**
- **PoB2 bug (0.22.0 up to current dev/beta):** with Darkness Enthroned ("91%
  increased effect of Socketed Augment Items"), every recalculation multiplies
  the socketed rune lines again (+95 Spirit grows to +345, +658, +1256 … +16706).
  Any recalculation in the PoB app inflates numbers the same way. The doctor
  refuses such builds and shows the lines that keep changing. Not reported
  upstream yet (needs the user's OK).
- LuaJIT seeds string hashing per Lua state, so PoB's XML attribute order
  differs between engines. Values do not; compare semantically across engines.
- PoB stats must be read through PoB's own save path: some saved stats are
  nested (e.g. `MainHandAccuracy` = `output.MainHand.Accuracy`).
- Public build codes don't record which PoB version saved them; a fixture is
  only an oracle on the release(s) that reproduce it. `--sweep` finds them.

**Open questions**
- User's own builds as fixtures (exported from PoB2 0.23.1) when available.
- More fixtures on 0.23.1: only 1 so far.
- Report the Darkness Enthroned bug to PoB2? (user's call)
- OAuth registration watch: not answered.

## 2026-11-19: Research phase

**What changed**
- `docs/RESEARCH.md`: research report, design, judged feature list, plan, open questions.
- `research/spikes/`: `setup_engine.sh` builds the pinned LuaJIT and lua-utf8 and clones PoB2. `bench.lua` measures calc speed. `roundtrip.lua` is the save/reload exactness check.
- Created a personal cloud environment `build-doctor` that allows pathofexile.com, web.poecdn.com, poe.ninja, poe2db.tw, poe2scout.com, poe2wiki, pobb.in, pastebin, maxroll and raw GitHub.

**What was tested**
- PoB2 `dev` @ 601768b (v0.23.1 data, tree 0_5) runs headless under LuaJIT 2460b3ff. Distro LuaJIT 2.1 (2023) fails on `+=`.
- Speed on a trivial build: boot 2.7 s, full recalc ~20 ms, node what-if ~2.2 ms.
- Save → reload → recompute: 107/107 PlayerStat values identical, but only after letting recalculation settle. Without settling there were 12 mismatches (stale output after auto-level).
- poe.ninja PoE2 exchange overview (`type=Currency`, `type=Runes`) returns data. GGG `web.poecdn.com/api/currency-exchange/poe2` responds.

**Open questions** (see RESEARCH.md §5)
- User OS, fixture builds, L1 trade fetch opt-in, OAuth watch, coach mode.
- Real-build calc speed (needs fixtures).
- PoE2 respec gold cost source.
- Any validated source for PoE2 boss skill damage.
- poe2scout API terms.
