# CHANGES

Running log so any session can pick up where the last stopped.
Newest first.

## 2026-11-20: Icon redrawn to the user's preferred version

The user preferred a bolder generated variant: thick cream frames, straight
arms, a plain glowing amber lens inset with a dark gap, no gem. Redrawn as
clean vectors to its proportions (the generated image had soft, uneven
edges), with the small 16–32 px version and the in-app mark to match.

## 2026-11-20: New app icon: spectacles, one lens lit

**Decision (from user):** after several rounds of concepts, refine the
"spectacles with one glowing lens" mark (it passed the 16 px and
black-and-white tests best).

**What changed**
- `crates/app/icons/source/icon.svg` (48 px and up: flat bone-white frames,
  dark glass left lens, lit right lens holding a round brilliant-cut gem,
  a soft amber halo on a dark tile) and `icon-small.svg` (16–32 px: thicker
  frames, plain lit lens, no detail that turns to noise).
- `icon.ico` holds 16, 24 and 32 px from the small version and 48, 64, 128
  and 256 px from the full one (checked pixel for pixel); the PNGs are
  rendered from the same sources.
- The mark (the icon without its tile) sits beside "Build Doctor" in the
  menu and on the start page, drawn inline so it stays sharp at any display
  scaling.

## 2026-11-20: Cleaner chrome; item-granted skills on imported characters

**From the user (screenshots of 0.1.27):** The Stars Answer still listed;
dislikes the active menu tab, the two-colour "Build Doctor" wordmark and
the app icon.

**What changed**
- An imported character lists item-granted skills as ordinary skill
  groups, so the cost-warning check missed them. It now also reads PoB's
  skill data flag (`fromItem`/`fromTree` on the granted effect; The Stars
  Answer has `fromItem = true`).
- Menu: no gold edge on the active item; the active page has a solid
  background, hover only a faint one, so the two are easy to tell apart.
- Wordmark: one colour, one weight.
- One more skill level: the damage label is a column header, not repeated
  on every row.
- The Life tile always has a sub-line ("No energy shield").
- Icon: nine original concepts drawn as SVG (plague doctor, rod and
  serpent, gem cross, vital rune, life flask, stethoscope, tree cross,
  scalpel and sigil, pulse heart), shown to the user at 176 to 16 px;
  the chosen one replaces the current icon next.

## 2026-11-20: Restore check no longer trips on config order (user report)

**Problem (user's Blood Mage, 0.1.26):** the diagnosis stopped with "PoB
did not return to the build's original state after a what-if", listing
config inputs in a different order. PoB saves config inputs in Lua
hash-table order (`ConfigTab:Save` iterates `pairs(configSet.input)`); a
Custom Modifiers what-if adds the `customMods` key to a build that had
none, which can rehash the table and reorder the saved lines for good. No
value changed. The skill-level checks run many such what-ifs, which made
it likely; reproduced on 2 of 13 fixtures (NMF0l4yHDix9, OkpIHJAvHQ_R).

**What changed**
- The restore check's comparison form sorts runs of `<Input>` and
  `<Placeholder>` lines; every name and value must still match.
- If the diagnosis ever fails, the app says so in a sentence and that the
  rest of the app is unaffected, above PoB's message.

**What was tested**
- Engine test: config lines in a different order compare equal, a changed
  value does not.
- All 13 fixtures diagnose without a restore error (the 3 Darkness
  Enthroned builds are refused as before).

## 2026-11-20: Advice that matches how the game is played; skill levels; alignment

**From the user (on their own build):** "it's pretty rare to get 75% chaos
res … this isn't accurate or good info"; The Stars Answer warning is noise
("no one actually ever uses this … it comes with the Runic Fork", which is
valued for other reasons); some UI elements are misaligned; "do your
research".

**Research (web, 2026-11-20)**
- Chaos resistance caps at 75% like elemental resistances but is much
  harder to stack; guides treat it as a secondary target, not a cap to
  reach. (poe2wiki Chaos resistance; lootlore chaos resistance guide;
  forum thread "Prevalence of Chaos Damage vs Resistance in the Endgame".)
- The Stars Answer is a spell the Runic Fork (Runeseeker's Call quest
  reward) grants, costing 10,000 mana per cast; guides call it not useful
  in practice. (game8 "The Stars Answer"; poe2wiki Runic Fork.)

**What changed**
- Chaos resistance: the fix is one chaos resistance mod at a mid roll
  (PoB's roll data: +4% to +27%, so +16%), not the gap to 75%, with what
  that mod is worth. Elemental resistances still go to their maximum.
- Cost warnings ("You do not have enough Mana to use: …") are checked per
  skill against the build's skill groups (`Engine::skill_usage`): a skill
  the damage numbers include (main skill or Full DPS) is a problem; a gem
  the player socketed but the numbers leave out is a note; a skill only an
  item grants that the build does not use is left out (The Stars Answer).
  Duplicate names are merged. The same wording is used for warnings an
  item or an extra skill level adds: only newly affected used skills, as
  "Not enough Mana to use Spark".
- **One more skill level** (diagnosis section, CLI and app): +1 of every
  "+# to Level of all … Skills" mod in PoB's roll data, applied through
  Custom Modifiers and recalculated: what each does to damage, best first,
  with any PoB warning it adds (on the Spark Infernalist, +1 level to all
  skills is +15.7% Full DPS but would make Spark cost more mana than the
  build has). Losses are shown too.
- `bd_with_mods` also returns PoB's warnings (`Engine::with_mods_warned`).
- UI: finding labels sit on the title's baseline; links inherit their
  line's size; the diagnosis panel is "What to fix" (the page is
  Diagnosis); skill-level rows align to their first line; item-check
  changes have separate value and change columns, resistances as
  "+1 pts"; chips are rounded rectangles so wrapped node text stays tidy;
  the start page's "where to get a code" sits under the intro.

**Found, not changed**
- PoB is not consistent about item-granted skills: on the Snap Shaman, a
  saved build with "+1 to Level of all Skills" in Custom Modifiers loads at
  −10.8% DPS, while typing the same line into Custom Modifiers (what the
  app does) changes nothing. The app keeps PoB's typing path, which is what
  a player would see doing it in PoB.

**What was tested**
- Unit tests: cost warnings per skill use, new-warning wording, the chaos
  step. Analysis 16, engine 31/31, `doctor verify` exact on 0.23.1.
- Spark Infernalist: chaos +12% (gap, under one mod), Frost Bomb etc. as a
  note, skill levels +15.7% to +0.1%. Thorns Legionnaire: chaos +16% (one
  mod) instead of +29%.
- Screenshots of every page after the alignment fixes.

## 2026-11-20: Gear upgrades, jewels, bug fixes, clarity

**Goal (from user):** build the trade feature (league typed once, no
requests to GGG), hunt bugs and limitations, keep it clean, clear and
precise.

**What changed**
- **Gear upgrades** (`analysis::gear`, `slot_values`, `trade_url`; `doctor
  gear <build> [--slot S --league L --goal --max-price --currency]`; app
  page after Diagnosis). For a worn item, every trade stat that can roll in
  its slot (PoB's `Data/QueryMods.lua`, roll ranges per item category) is
  put alone, at the middle of its range, on a blank rare of the worn base
  and measured with PoB's calculator, split across the engines: what it
  adds to damage, Total EHP and smallest max hit, in percent of the
  build's current numbers. Also what the worn item adds over a blank of its
  base. The trade link (opened in the browser through Tauri's opener
  plugin, only for `https://www.pathofexile.com/trade2/search/` links; or
  copied) searches the slot's category, non-unique, not mirrored, sellers
  available now, sorted by a weighted sum of the top 20 stats for the goal
  (Damage; Survival = EHP + smallest max hit; Both). Weight = share per
  point, so the sum estimates the gain in percent. Weapons and armour keep
  to the worn base by default (base damage and defences are not mods, so
  other bases would rank wrongly); jewellery searches every base (its
  implicit is weighted). Optional price cap in exalted, divine or chaos. The
  league is typed once and remembered; the app makes no request to GGG.
- **Jewels in Check an item**: allocated passive tree jewel sockets are
  slots too ("Jewel socket 2", in tree order), equipped and restored like
  equipment.
- **Realistic numbers** calculates on the first visit (the answer is the
  page), and only offers "Update with your ticks" once ticks change.
- Start page says where to get a PoB code.

**Bugs fixed**
- An error from one page stayed on screen on the next.
- Work in progress showed its progress on whichever page was open; now on
  its own page, and other pages say what is running. Opening Gear
  upgrades during other work replaced that work's progress.
- A gear measurement finishing after another build was loaded could be
  kept for the new build (load counter now guards the cache).
- Messages written for the user by the bridge read "PoB reported an
  error: …"; they now show as written (`Error::Message`).
- "PoB's numbers never settled" dumped raw XML lines; it now says in plain
  words what happened and names what keeps changing.
- A doc comment had drifted onto the wrong function.

**Limitations removed**
- Memory: engines collect PoB's start-up garbage (about 100 MB less each),
  and the app runs at most 4 engines (peak about 1.6 GB instead of nearly
  3 GB with 8), since it runs next to the game.
- Jewels can be checked (above).

**What was tested**
- Unit tests: trade link (weights, goal, price, base filter, refusals),
  significant-digit rounding, shares ignore immunities, unsettled message.
- Engine test: a jewel in each of three allocated sockets, restored
  byte-identical. Engine tests 31/31; `doctor verify` exact on 0.23.1.
- Real builds: Snap Shaman Ring 1 (91 stats measured; cold resistance
  +13% EHP, 10% cooldown recovery +9.9% damage), Ice Shot Deadeye body
  armour and helmet; link length 0.7-3 KB.
- App screenshots: Diagnosis, Gear upgrades (slot grid, measured slot),
  Realistic numbers opening with results.
- Not tested here: the trade link in a browser (the trade site is not
  reachable from this environment); check it opens the intended search.

## 2026-11-19: Item check, and a layout that says what to do next

**Goal (from user):** "it must actually help, i don't want to feel lost
when using this app."

**What changed**
- **Check an item** (`analysis::item_check`, `doctor item <build> <file|->`,
  app page). Paste an item copied in game with Ctrl+C (or from the trade
  site). PoB reads it exactly as given (quality and rolls unchanged, unlike
  PoB's own paste, which raises quality to 20), equips it in each slot it
  fits that PoB shows for the build (not weapon swap or jewel sockets), and
  recalculates fully. Per slot: what it replaces; damage, Total EHP and
  smallest max hit before → after; every other changed stat (life, ES,
  mana, Spirit, resistances, max hits, armour, evasion, block, attributes);
  PoB warnings the item adds (an unmet requirement elsewhere, losing an
  Unset Ring's skill slot); a verdict (better, worse, trade-off, no change)
  from damage and Total EHP. Lines PoB cannot read are listed as not
  counted. Text PoB cannot read as an item gives "copy it in game with
  Ctrl+C" instead of a Lua error. CRLF from the Windows clipboard is
  normalised.
- Engine: `Engine::item_info`, `Engine::with_item`. The item is added,
  equipped, settled, then unequipped and removed; the build is checked
  byte-identical before anything else is read.
- **Layout.** Diagnosis is the landing page (exactness check, key numbers,
  findings, then a Next panel saying what each other page will show for
  this build). Navigation shows each page with its question: Diagnosis
  (what to fix first), Check an item (is it an upgrade?), Passive tree plan
  (free gains from respecs), Realistic numbers (without assumed buffs),
  Stats (every number PoB calculated). A measured fix links straight to the
  item check.

**What was tested**
- Engine test: a game-format ring is read (name, the one unreadable line,
  slots Ring 1 and Ring 2 only), equipping it changes PoB's numbers, the
  build is restored, a wrong slot and unreadable text are refused with the
  clear message. Engine tests 30/30; `doctor verify` exact on 0.23.1.
- Real items from another fixture build checked against the Snap Shaman:
  findings match what the items carry (chaos resistance 74% → 49% when the
  replaced ring held it; the Unset Ring's skill slot and the buckler's
  Dexterity requirement surfaced as new PoB warnings).
- App: screenshots of the Diagnosis landing page and an item check.

## 2026-11-19: Diagnosis

**Goal (from user):** start turning "your numbers" into "your problems",
genuinely useful and exact, no filler.

**What changed**
- `analysis::diagnose` and `doctor diagnose <build>`; in the app it runs in
  the background after a build loads and fills a Diagnosis panel at the top
  of the overview. Findings, most important first:
  - **Problems:** a resistance penalty that is not the endgame one (with
    what the endgame penalty costs: each resistance and Total EHP), and
    PoB's own warnings (unmet requirements, negative Spirit, too many gem
    groups, not enough mana): things the game would not allow, so the
    numbers include something the character cannot have.
  - **Fixes**, ranked by their largest effect: each resistance below its
    maximum (needs +N%, with that element's max hit and Total EHP before →
    after), and hit chance below 100% (the flat accuracy that reaches 100%,
    with the damage before → after).
  - **Notes:** resistance over the maximum, unreserved Spirit (only when it
    is at least the cheapest Spirit skill or support in PoB's data), spare
    weapon-set passives, mods PoB ignores.
- How fixes are measured: PoB recalculates the build with the fix typed into
  its own Custom Modifiers box (`+28% to Chaos Resistance`,
  `+15155 to Accuracy Rating`). Every line must be one PoB reads
  completely, or the analysis fails. The build is restored afterwards and
  checked byte-identical, as for every what-if.
- A fix that changes none of PoB's numbers (raising chaos resistance on a
  chaos-immune build) is not reported. A resistance that does not move when
  raised (something sets it) is reported without a gain.
- Accuracy search: quick one-pass probes in parallel rounds (doubling, then
  evenly spaced between the largest miss and the smallest hit) until the
  answer is within 1% of the least that works; then both ends are confirmed
  with full settled recalculations. If they disagree, no amount is given.
- Engine: `Engine::outputs` (PoB output values that are not saved, such as
  `MissingFireResist`), `with_mods`, `probe_mods`, `cheapest_spirit`.

**Left out on purpose**
- Mana sustain: PoB's regeneration numbers leave out leech, flasks and
  on-kill recovery, so a "you run dry" finding would be wrong for many
  builds.
- Unspent passive points: auto-level builds derive the level from the
  points spent, so it cannot be told reliably.

**What was tested**
- Unit tests: ranking, warning severity, finite-only effects, mod lines,
  the accuracy search on thresholds 1 to 25,600 (within 1%, at most 4
  rounds).
- Engine test: a Custom Modifiers what-if raises chaos resistance exactly to
  its maximum, the quick probe agrees with the settled run, the build is
  restored, an unreadable line is refused.
- All 13 fixtures: sensible findings; the 3 Darkness Enthroned builds are
  refused as before. An Act 3 penalty variant reports 75% → 56% cold and
  lightning, −15.6% EHP. A 95%-reduced-accuracy variant: hit chance 39%,
  needs +15,155 (true least, by an exact sequential search: 15,075),
  Full DPS +317.3%, identical to "hits can't be evaded" (PoB applies hit
  chance to crits too, so the gain is all hit chance). Results across 2-8
  engines: 15,082 to 15,155, all within 1%.
- Timing on this 4-core container: 6-15 s per build; 30 s for the heavy
  accuracy case (9 skills, 2.8M Full DPS).
- `doctor verify` still exact on 0.23.1; engine tests 29/29.
- App: screenshot with the diagnosis panel, checking state and results.

## 2026-11-19: Versions, releases and self-update

**Decision (from user):** make the repository public so the app can update
itself without credentials. (Changing visibility is done by the user on
GitHub.) Checked before: no personal data in the tree; every commit is
authored with the GitHub no-reply address.

**What changed**
- Every CI build is stamped `0.1.<run number>` and its commit
  (`BUILD_DOCTOR_VERSION`, `BUILD_DOCTOR_COMMIT`); local builds say
  `0.1.0-dev` and never offer updates. The app shows its version.
- CI `release` job: every green push to `main` publishes release
  `v0.1.<run>` marked Latest, with `build-doctor.exe`, `doctor.exe` and
  `version.json` (version, commit, SHA-256 of the exe).
- `crates/app/src/update.rs`: on start, reads
  `releases/latest/download/version.json`; if its build number is higher,
  the app offers "Update and restart", downloads the exe with progress,
  checks the SHA-256, renames the running exe to `.old`, moves the new one
  in, restarts; `.old` is removed on the next start. Offline or errors: the
  check stays silent.

**What was tested**
- Unit tests: build numbers, side-file names, SHA-256 matches `sha256sum`.
- Update notice rendered with mocked data. The real update path needs a
  published release and a public repo; it runs for the first time after
  this is merged.

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
registration watch" (twice a week, read-only, push
notification).

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
