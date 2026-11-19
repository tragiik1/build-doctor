# CHANGES

Running log so any session can pick up where the last stopped.
Newest first.

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
