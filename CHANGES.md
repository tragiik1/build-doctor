# CHANGES

Running log so any session can pick up where the last stopped.
Newest first.

## 2026-11-19: Plan keeps the other side from falling (user report)

**Problem (from the user's first real run, Blood Mage):** step 1 of the
damage plan dropped Mind Over Matter for +17.3% DPS at −57.5% EHP and
−57.3% smallest max hit. The damage goal accepted any survival loss.

**What changed**
- `PlanOptions::max_other_loss` (default 0%): over the whole plan, the side
  not being optimised may not fall below its starting value by more than
  this (EHP and smallest max hit for the damage goal, damage for the
  survival goal), with 0.05% rounding slack. Applied to screened pairs
  before exact trials and to every exact trial.
- CLI `doctor todo --max-loss <percent|any>`; app: "Survival loss allowed:
  None · 5% · Any" (reads "Damage loss" for the survival goal).

**What was tested**
- Unit test for the guard (strict, tolerant, any, survival goal).
- Shaman fixture, 3 steps with the default: +52.8% DPS with EHP +1.0% and
  smallest max hit +1.1%. 34/34 exact; all tests pass.
- Cross-check from the user: poe.ninja's figures for their character (life,
  mana, spirit, armour, evasion, resistances, EHP, max hits) match the app.

## 2026-11-19: Faster PoB install with progress

**Problem (reported):** the app's first-start install sat for 15+ minutes
with no progress. It downloaded GitHub's archive of the whole PoB repository
(~390 MB, mostly art) before using 47 MB of it.

**What changed**
- `Install::download` fetches only the files the engine uses, from
  raw.githubusercontent.com, 8 at a time with 3 tries each: 1,005 files,
  45.5 MB, each SHA-1-checked against PoB's manifest as it arrives. Paths
  are percent-encoded (PoB has file names with spaces). The `runtime` part
  is limited to `runtime/lua` (the rest is GUI runtime).
- Progress callback (`InstallProgress`): the CLI prints files and MB; the app
  shows a determinate bar with the same.
- Leftover `.download-*.zip` files from the old installer are deleted.
- `from_archive` (offline install) shares the same finishing step.

**What was tested**
- `doctor engine install` from scratch: 5.5 s here, `engine verify` passes,
  precision gate exact on the new install.
- The real app's Install button under Xvfb: live progress, finished in a few
  seconds, removed a planted stale archive, then ready to analyse.

## 2026-11-19: Desktop app

**What changed**
- `crates/app` (binary `build-doctor`): Tauri 2 desktop app. Native window
  rendered by WebView2 on Windows; plain HTML/CSS/JS interface (`ui/`, no
  framework, no Node build) that only presents what the Rust commands return.
  Commands: `engine_status`, `install_engine`, `load_build`, `realistic`,
  `plan` (progress events, cancellable), `cancel_plan`. One engine per core
  (max 8) started on the first build and kept warm.
- Screens: start (paste a code or link; install PoB if missing), Overview
  (tiles, exact-reproduction verdict, PoB warnings, stats, assumed
  conditions, fight simulated, mods not counted), Realistic numbers
  (tick-to-keep, shared with the plan), Passive tree plan (goal, steps, live
  progress, steps as Take/Drop chips with deltas, copy final PoB code).
- `crates/analysis` (new library): overview, realistic and plan logic moved
  out of the CLI, returning serialisable data with plan progress and
  cancellation. The CLI and the app share it; the CLI only prints.
- `pob`: build input (codes, links, files) and the data directory moved in
  from the CLI; `pob::PINNED_VERSION`; `Pool::install()`; `Summary` and
  `NodeRef` serialise.
- CLI: `--json` on check, realistic and todo.
- CI: Linux jobs install the WebKitGTK libraries; the Windows job uploads
  `build-doctor.exe` and `doctor.exe` as the `build-doctor-windows` artifact.

**What was tested**
- Refactor regression: CLI output unchanged on the Shaman fixture (401,771 /
  87,326 / 135,462; all 105 saved stats reproduced). 31 tests and
  `doctor verify` (34 exact) pass.
- The real app under Xvfb, driven by xdotool: pasting the maxroll link
  fetched the build and showed the Exact verdict; Realistic numbers and a
  5-step plan ran with live progress and matched the CLI (87,326 → 151,156).
- Interface reviewed in Chromium at 1360×860 on real `--json` data.
- Not run here: the Windows build of the app (CI builds it).

## 2026-11-19: Ranked to-do list, passive tree (`doctor todo`)

**What changed**
- `doctor todo <build> [--keep …] [--goal damage|survival] [--steps 5]
  [--max-distance 6] [--export FILE] [--jobs N]`: a step-by-step passive tree
  plan. Each step is the best point-neutral swap (take a node with its path,
  drop the same number of points), applied, then the next step searches the
  improved tree. Scored on the realistic build: the build with every
  condition not kept removed from its config (`pob::config_xml::without_inputs`).
- Per step: screen every unallocated node within `--max-distance` points and
  every allocated node with its dependents using PoB's own node calculator
  (behind its power report), split over the pool; pair the 32 best additions
  (gain per point) with the cheapest point-equal, non-overlapping removal set
  from the 24 cheapest removals; screen the pairs together; try the 8 best
  exactly (`Engine::try_tree`: DeallocNode/AllocNode as clicking in PoB, full
  recalculation, restore from PoB's undo state). A step is taken only if the
  exact result gains ≥ 0.1% on the goal, the point count is unchanged and PoB
  raises no new warning. The improved build is then reloaded fresh in a
  worker (what importing it into PoB gives) and every later engine must
  reproduce it.
- `--export` writes the final tree as a PoB code.
- Engine: persistent `pob::Pool` (long-lived engines on their own threads,
  build reloaded only when it changes, stats checked against the caller's
  expectation on every call, original state checked after every share of
  work). Replaces the per-call `Engine::map`. `realistic` uses it too.
- Restore check now compares a canonical form: PoB writes allocated nodes in
  hash-table order, so after an undo the same nodes can be listed in another
  order (`nodes`, `intNodes`, … and the tree URL). Node lists are sorted and
  tree URLs dropped; every other character, every stat included, must match.
- Removals only come from the main tree: weapon-set passives have their own
  point budgets in PoE2 (found when every early plan was rejected by PoB with
  "You have 4 Weapon set 2 passives available").

**What was tested**
- 25 engine/library tests and 8 CLI tests pass (new: a tree trial adds and
  removes what it should at equal points and restores exactly; pool results
  equal sequential ones and a wrong expectation is refused; removal pairing
  matches points exactly, never overlaps and avoids the path; node text).
- `doctor verify`: 34 exact, 0 failed.
- End to end on the 0.5.5 Shaman fixture: 5 steps, realistic DPS 87,326 →
  151,156 (+73.1%) with EHP +1.5% and smallest max hit +1.7%; the exported
  code, checked with `doctor check` in a fresh process, reproduces all 105
  saved stats exactly. With 4 conditions kept: 3 steps, 165,252 → 222,768
  (+34.8%), one step trading −1.8% EHP for +4.2% DPS, shown as such.
- Observed: on these builds PoB's fast node calculator agreed with the exact
  recalculation to two decimals for every tried pair. Not relied on; exact
  trials remain the only numbers shown.

**Limitations**
- Greedy: each step is the best swap found, not a global optimum (the tree
  optimiser is the global version). Steps are only as good as `--keep`.
- Speed: ~21 s per step on 4 cores (~10 s expected on 8), dominated by
  screening ~1,000 additions per step. Re-screening only what a step changes
  would cut this; not done yet.
- New path attribute nodes take PoB's default attribute choice.
- Not covered yet: support gems, gear, jewels, ascendancy.

## 2026-11-19: Realistic numbers (`doctor realistic`)

**What changed**
- `doctor realistic <build> [--keep TEXT]... [--jobs N]`. PoB recalculates the
  build with each assumed combat condition switched off on its own (what it is
  worth), with all of them off (the floor), and with all but the kept ones off
  (your realistic numbers). Reported for Full DPS, Combined DPS, Minion DPS,
  Total EHP and smallest max hit (with its damage type), whichever the build
  produces. Nothing is judged by the doctor: the player says what they keep up.
- Condition vs scenario: Enemy Stats, Quest Rewards and the General section's
  calculation-mode lists (resistance penalty, ailment/armour/… modes) describe
  the simulated fight and stay as configured. Everything else the build sets
  and PoB considers relevant, including custom modifiers, is a condition.
- Engine: `Engine::without_config(vars)` unsets config options exactly as
  unticking/clearing them in PoB's config tab does (`input[var] = nil`, then
  `BuildModList`, then a settled recalculation). The original is restored
  lazily: anything that reads PoB state afterwards (identity, assumptions,
  unsupported mods, `ensure_original`) first recalculates the original and
  checks PoB's output is byte-identical to the original load, else errors.
- Engine pool: `Engine::map(xml, extra_workers, items, job)`. Extra engines
  load the same build, must reproduce the first engine's saved output
  exactly before taking work, and must be back in the original state at the
  end. `pob::Error` is now `Send + Sync` (Lua errors kept as text).
- CLI exits quietly on a closed pipe (`| head`) instead of panicking.
- Display: numbers from 1,000 up are shown whole (exact values: `check --all`).

**What was tested**
- 22 engine/library tests, 5 CLI tests: new ones check a config what-if
  changes DPS, that unsetting nothing equals the original exactly, that a
  fresh load agrees after what-ifs, and that parallel what-ifs equal
  sequential ones in order. `doctor verify`: 34 exact, 0 failed.
- Timing: a config what-if needs a full uncached recalculation (PoB wipes its
  skill cache when anything changes: ~400–570 ms vs 63 ms cached), so the
  speed comes from parallelism: 23 conditions took 59 s on one engine and
  21 s on 4 engines here (expect ~10 s on an 8-core PC), identical numbers.

**What it shows on the fixtures**
- Snap Shaman (0.5.5 forum Build of the Week): 401,771 → 87,326 Combined DPS
  (−78.3%) without conditions; "Total Resonance Count = 200" alone −32.5%,
  enemy Shocked −21.8%, Frozen −15.0%, a custom modifier −10.8%.
- Ice Shot Deadeye: 11.9M → 1.67M Full DPS (−85.9%) and Total EHP −60.1%;
  "Is the enemy Blinded?" alone is −21.9% EHP.
- Navira minion Disciple: none of its 11 conditions change minion DPS.

## 2026-11-19: Bug and limitation review

**Decisions (from user):** do not report the Darkness Enthroned bug to PoB2.
User fixtures later. Watch GGG API registration: scheduled task "GGG API
registration watch" (Mon and Thu 18:33 Australia/Brisbane, read-only, push
notification; timezone inferred from the repo's +10:00 timestamps).

**Bugs found and fixed**
- *Wrong numbers instead of an error for an unknown tree* (the 1.0 launch-day
  case: a 1.0 build in a 0.5-era PoB). PoB reports it only as a popup, then
  crashed later with "mainOutput is nil". The bridge now records PoB's popups
  during a load and fails with PoB's own message ("Unknown Passive Tree
  Version …").
- *PoB dev mode.* PoB treats any non-installed copy as a developer checkout,
  and dev mode auto-saves a temp build into its own folder whenever a build is
  replaced (blocked by the sandbox, which raised a popup on PoB 0.17–0.19).
  The desktop app never runs in dev mode; the bridge now turns it off after
  start-up. Dev mode touches no calculation (all uses checked).
- *Non-ASCII Windows paths.* PoB's own LuaJIT carries `pob-wide-crt.patch` so
  files open under paths like `C:\Users\Noé`; ours did not. Applied PoB's
  patch (plus a NULL fix in its non-Windows macro). See vendor/README.md.
- *Sandbox gaps.* `io.output(file)` / `io.input(file)` could open files around
  the `io.open` guard, and `package.loadlib` was reachable. Closed. Audit
  result: PoB only `loadstring`s its own tree files, so a shared build cannot
  execute code; the sandbox exists to stop side effects.
- *No timeouts* on the PoB download and on build links. Now 20 s connect /
  30 min for the archive, 30 s for links.
- *Install folder trust.* `doctor check --pob X` now refuses a folder whose
  manifest says another version.
- Errors no longer include Lua tracebacks; PoB's output tail is shown only
  for start-up failures.

**Added (trust, not features)**
- `check` lists the *assumptions* every number depends on: the config
  options the build sets that PoB itself considers relevant (PoB's config-tab
  visibility rules), plus the enemy type. Example from the 0.5.5 Shaman
  fixture: enemy Frozen, Ignited, Shocked 20%, Chilled 10%, Cursed, Hindered,
  Armour Broken, all at once, and custom "12% more Elemental Damage".
- `check` lists mods PoB cannot parse on equipped items and allocated jewels
  ("Not counted"). Every one of the 11 oracle builds has some, including
  Headhunter, "% increased Effect of Suffixes" jewels and Runic Ward.
- `check` warns when a build's passive tree is older than PoB's newest.

**What was tested**
- `doctor verify`: 34 exact, 0 failed (unchanged after all fixes).
- 20 engine and library tests (new: unknown tree version fails with PoB's
  reason; assumptions include the fixture's Frozen = yes and the enemy type;
  unparseable mods found once each; io.output and loadlib blocked) and 3 CLI
  unit tests, all passing.

**Known limitations (not fixed, by design or for later)**
- Reference platform is Windows. LuaJIT calls the C runtime for `pow`, `exp`,
  `log` and friends; on Linux that is glibc instead of the MSVC UCRT the PoB
  app uses, which could differ in the last bit. Not observed: all fixtures
  pass on both. The doctor's numbers should be taken from Windows.
- About 300 MB RAM per engine (PoB's data per Lua state). A parallel pool for
  M2 must be sized by free memory, not just cores.
- PoB accuracy is not game accuracy. The doctor now shows what PoB ignores
  and what it assumes, but cannot know what PoB gets wrong.
- Fixtures on the pinned PoB: still one. User builds will fix this.

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
- Load time per build 0.5 to 2.4 s (load and settle, release build).
- What-if cost on real builds (Lua spike, one core, PoB's own
  `GetMiscCalculator`): full recalc 69–78 ms; add one node 16–17 ms; remove one
  node 9–10 ms. On an 8-core PC that is about 500 what-ifs per second, so
  ranking every node within 5 points takes about a second, and a tree search of
  tens of thousands of evaluations takes under a minute.
- `doctor engine install --archive` on a `git archive` of v0.23.1 (same layout
  as GitHub's zip): installs 53 MB, verifies, and the gate passes on it. The
  download itself is blocked by this container's proxy; CI exercises it.
- CI run 1: Windows and Linux both compiled (LuaJIT and lua-utf8 under MSVC)
  and passed unit tests; install failed because PoB's manifest omits
  `src/HeadlessWrapper.lua` (PoB's `manifest.cfg` excludes it). Fixed: the
  installer takes it (and `_SimpleGraphic.def.lua` when present) from the same
  archive and records its SHA-1 in `unlisted.sha1` for `engine verify`.
- CI run 2 (commit 37f5e29): all green. On windows-latest: download from
  GitHub and SHA-1 verify of PoB2 0.23.1 and 0.21.0, 17/17 engine tests
  (sandbox, determinism), precision gate **9 exact, 0 failed** (8 builds on
  0.21.0, the Shaman on 0.23.1), engine reuse clean. `doctor.exe` is 4.2 MB.
  Same on ubuntu-latest.

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
- Darkness Enthroned bug: user chose not to report it.
- OAuth registration: watched by a scheduled task.

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
