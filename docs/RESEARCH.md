# Build Doctor: research report, design and feature cuts

Date: 2026-11-19. PoE2 1.0 launches 2026-12-11.
Status: research complete, waiting for go-ahead before the main build.

Everything below was either read from a primary source (linked) or measured in
this repo's spikes. Items marked **unverified** come from secondary coverage.

---

## 1. Findings

### 1.1 Path of Building PoE2 (the calculation engine)

| Question | Finding | Source |
|---|---|---|
| Current version | v0.23.1, released 2026-07-28. `dev` last commit 2026-11-10; a weekly `beta` branch was cut 2026-11-13. Data is for PoE2 0.5 (tree version `0_5`). | PoB2 repo `CHANGELOG.md`, `git log` |
| Licence | MIT | PoB2 README |
| Runs headless? | **Yes, verified here.** `src/HeadlessWrapper.lua` stubs the graphics layer and exposes `newBuild()`, `loadBuildFromXML()`, `loadBuildFromJSON()`. PoB2's own test suite (Busted, ~50 specs) runs this way in CI. | `src/HeadlessWrapper.lua`, `.busted`, `.github/workflows/test.yml` |
| Runtime catch | PoB2 now uses compound assignment (`count += 1` in `Modules/Main.lua`, `-=` in `Classes/Item.lua`). Ubuntu's LuaJIT (2023) fails to parse it. Upstream LuaJIT added it in 2026. PoB2 pins LuaJIT commit `2460b3ff` (2026-07-20); built from that commit it works. Also needs the `lua-utf8` C module. | `PathOfBuilding-SimpleGraphic/vcpkg-ports/ports/luajit/2026-07-20_1/portfile.cmake`, spike |
| Speed (measured) | Boot ~2.7 s. Full recalculation ~20 ms. "What if I add this node" via PoB's `GetMiscCalculator()` ~2.2 ms per node. **Test build was trivial** (Fireball, 100 nodes, no gear); real builds will be slower. To be measured on real fixtures in M0. 4 cores in this container. | `research/spikes/bench.lua` |
| Exactness (measured) | Saved a build to XML, reloaded it, recomputed: **107 of 107 stored stats identical as strings**. First attempt showed 12 mismatches (life 212 vs 817) because the build was saved before PoB finished recalculating after auto-levelling. Lesson: always "settle" (recalculate until stable) before reading numbers. | `research/spikes/roundtrip.lua` |
| Free precision oracle | Every PoB code embeds ~100+ `<PlayerStat>` values computed by the PoB app at save time (`Modules/Build.lua:1222`). Any code exported from the same PoB version is a ready-made exactness test. | `Modules/Build.lua` |
| Already built into PoB2 | Node power report (single-stat), weighted trade query generator ("find best item" per slot, with stat weights), header-driven trade rate limiter, OAuth character import under PoB's own client id `pob`, build import from pobb.in, maxroll, poe.ninja and poe2db links. We should drive these, not rewrite them. | `Classes/CalcsTab.lua:551`, `Classes/TradeQueryGenerator.lua`, `Classes/TradeQueryRateLimiter.lua`, `Classes/PoEAPI.lua`, `Modules/BuildSiteTools.lua` |
| Boss damage data | `Data/BossSkills.lua` contains **PoE1** bosses only (Shaper, Atziri Flameblast…). PoE2 has the monster base damage table by level and generic Boss / Pinnacle / Uber multipliers, but no per-boss PoE2 skill data. | `Data/BossSkills.lua`, `Modules/ConfigOptions.lua:1982-2210` |
| Crafting weights | `Data/ModItem.lua` has tiers, level requirements and ranges (good: "realistic rolls"), but every `weightVal` is 1. Spawn weights are placeholders, so slam/craft probabilities **cannot** be computed from PoB data. | `Data/ModItem.lua` |

### 1.2 How PoB2 will reach 1.0

Past patches, using GGG's API changelog dates as patch dates:

| Patch | Patch date | First PoB2 release with support |
|---|---|---|
| 0.2 | 2025-04-05 | v0.6.0, 2025-04-06 |
| 0.3 | 2025-08-27/29 | v0.10.0, 2025-08-30 |
| 0.4 | 2025-12-10/12 | v0.13.0, 2025-12-15 |
| 0.5 | GGG tree export "0.5.0 Preview 2" on 2026-05-28 | v0.16.0, 2026-05-30 |

- GGG publishes the official tree (`grindinggear/poe2-skilltree-export`) shortly before launch. PoB2 follows within **1–3 days**, then ships accuracy fixes for about 2 weeks.
- 1.0 is larger than any previous patch: tree redesign, ~60 jewel sockets, crit chance removed from regular tree nodes, support gems that adapt to the supported skill, Shadow and Duelist, 200+ bases, 110+ new or reworked uniques, a Duelist Squire that wears items (unverified PoB support). Patch notes are due about one week before launch (~4 Dec, per maxroll).
- **What this means for launch day.** On 11 December, PoB2 will very likely still have 0.5 data. I won't show 0.5 numbers as if they were 1.0. The tool will refuse to show numbers until the installed PoB2 data version matches the game version. Campaign (Acts 1–6) takes the first day or two anyway; the doctor matters most from maps onward, by which time PoB2 1.0 should exist.

### 1.3 Official API and OAuth

From pathofexile.com/developer/docs (read directly):

- **New app registration is closed:** "We are currently unable to process new applications." Applying is not possible right now. The landing page also warns that low-effort or LLM-written applications get rejected.
- **Desktop apps:** "While not encouraged, these are permitted. They must use a public OAuth client." Public clients use PKCE and a `http://127.0.0.1:…` redirect only. Access tokens last 10 h, refresh tokens 7 days. Rate limits are shared with every other public client, and users see an "authenticity unverifiable" warning.
- **Scopes:** `account:profile`, `account:leagues`, `account:stashes` (stash endpoint is PoE1 only), `account:characters` (supports `poe2` realm; since 3.27 it returns equipped items only, no inventory), `account:league_accounts`, `account:item_filter`. The `service:*` scopes (ladder, public stash, currency exchange) need a confidential client.
- **PoE2 ladder:** top 1000 only, `service:leagues:ladder` (confidential client, so closed to us).
- **Public Stash API:** PoE1 only. No PoE2 equivalent exists, so nobody gets full PoE2 listing data except through the trade site.
- **Currency Exchange:** public, no auth: `GET https://web.poecdn.com/api/currency-exchange/poe2[/<hour>]`, hourly digests of volume and ratios. Verified reachable.
- **Rules that bind us:**
  - User-Agent must be `OAuth {clientId}/{version} (contact: …)`.
  - Too many 4xx responses (including 429) gets you restricted.
  - "It is against our Terms of Use (section 7i) to reverse-engineer endpoints outside of this documentation."
  - Overlay rule: screen or clipboard reads only once per manual key press.
  - Macro rule: one action per manual key press.
  - Third-party notice: "This product isn't affiliated with or endorsed by Grinding Gear Games in any way."
- **Build Planner file format (PoE2 only, official):** the game watches `Documents/My Games/Path of Exile 2/BuildPlanner/*.build`. These are JSON files listing passives, skills, supports and slot hints, each with `level_interval` and hover text (`additional_text`). **A tool may write these files, and the game shows them in-game.**
- **Character import today, with no app of our own:** use PoB2's own OAuth import in the PoB2 app, then paste the code into the doctor. We must not reuse PoB's client id. That would break "one product per registered application" and would be impersonation.

### 1.4 Trade site

- The trade2 API is **not** in GGG's documented API. PoB2, Sidekick, Exiled Exchange 2 and the overlays all use it, and GGG tolerates them. But by the docs' own wording it is an undocumented endpoint and could be blocked at any time.
- **Rate limits are dynamic.** Read `X-Rate-Limit-{Rules,Ip,Account,…}` and their `-State` headers plus `Retry-After` on every response. Values in PoB2's limiter comments: search 8/10 s, 15/60 s, 60/300 s per IP, with lockouts of 60 s, 120 s and 1800 s; account 3/5 s. Treat these as stale examples, not documentation.
- **Zero-request option:** since September 2026, PoB2 builds trade links as `https://www.pathofexile.com/trade2/search/<league>/<gzip+base64url(query)>` (PR #2502). A full weighted search can open in your browser with **no automated request from us at all**.
- **Ban context:** Poe2Stash (a stash-value tool) was banned within a day with "does not comply with our third-party guidelines" (MeinMMO, unverified detail).

### 1.5 Price data

| Item class | Source | Status |
|---|---|---|
| Currency, runes, fragments etc. | poe.ninja economy API (`/poe2/api/economy/exchange/current/overview?league=…&type=…`) | Verified live. Current PoE2 leagues: Runes of Aldur, Forbidden Rites. |
| Currency (official, historical) | GGG Currency Exchange digest | Verified. Hourly, lags by ≤1 h. |
| Uniques | poe2scout (reachable, API not yet examined) | To check in M2. |
| Rares (the upgrades that matter) | Only the trade site. | Priced at the moment you search, via weighted query plus budget cap. |
| poe.ninja builds and profiles | **Internal, "not available for third-party use"** (per poe.ninja docs, read via search; the page itself is behind a Cloudflare check here). | Ruled out. |

---

## 2. Design

### 2.1 Principle

PoB does every number. The doctor only does three things: generates candidate changes, asks PoB for the exact before/after, and searches, ranks and explains. Every number on screen carries a reproducible "receipt": PoB version, data version, the config used, and the exact change applied. Re-running the receipt gives the same number.

### 2.2 Architecture

Revised on 2026-11-19 after the go-ahead: everything we write is Rust (your call), PoB stays unmodified Lua.

```
crates/pob      PoB2 headless: embeds LuaJIT (exact commit the PoB2 app ships) and
                lua-utf8 in-process, runs PoB's own HeadlessWrapper.lua in a sandbox,
                loads builds, settles, returns PoB's saved stats. Installs PoB releases
                from GitHub and verifies every file against PoB's own SHA-1 manifest.
crates/doctor   CLI today (check, verify, engine). Later: search/ranking engine,
                market data, trade links, Build Planner export, localhost UI, MCP.
fixtures/       real builds saved by the PoB2 app; their embedded stats are the oracle
vendor/         pinned LuaJIT and luautf8 sources (see vendor/README.md)
```

**Why in-process Lua instead of worker processes:** one 4 MB binary, no luajit.exe to ship on Windows, and an engine per thread for parallel what-ifs. The engine never changes the process working directory: PoB's relative file reads are resolved by the bridge.

### 2.3 Objective and ranking

- **Damage:** PoB's configured DPS (main skill, or Full DPS), with the config shown next to the number.
- **Survival:** PoB's max hit taken per damage type, the minimum across types, and total EHP.
- You choose the goal (damage, survival, or a weighted mix). The tool also shows the **Pareto front** (damage % vs survival %), so a +20% DPS item that costs 30% of your max chaos hit can't hide.
- **Value per currency** = change in goal ÷ price, in divines (priced from live exchange rates).
- Passive respecs in PoE2 cost gold, not currency, so tree changes rank separately as "free" gains. The gold cost isn't in PoB data, so it is shown as "not computed".

### 2.4 Patch-day robustness

1. `doctor update` bumps the pinned PoB2 tag.
2. It runs the fixture suite.
3. It refuses to report numbers if the fixture match drops below 100% or the PoB data version ≠ current game version.

Our code never parses game data itself. It reads everything through PoB. A patch only changes data, never our logic.

---

## 3. Feature list, judged

### Keep (launch target: 1–3 working by launch, 4 shortly after)

1. **M0, precision harness.** Pinned engine; bridge; fixture suite asserting exact string equality against embedded PoB stats; version guard. *Gate for everything else.*
2. **M1, diagnosis.** Max hit and EHP per damage type, ailment and stun thresholds, recovery, layer breakdown, damage breakdown. "Weakest link" is measured, not asserted: the marginal value of +1 unit of each defensive and offensive stat, ranked.
3. **M2, marginal value engine.**
   - Nodes within N points, including removals and "swap" pairs: drop your worst allocated node, take a better one, same point count.
   - Support gem swaps and levels.
   - Per-slot gear mod values at realistic tiers from PoB's mod table (mid and max roll).
   - Jewel mods.
   - All on one ranked table with cost and the reason.
4. **M3, trade links that find the upgrade.** Stat weights from M2 become a weighted trade2 query with your budget as a price cap, opened in your browser (zero requests from us). Paste any item's text into the page and get its exact change in damage and survival from PoB.
5. **M4, tree optimiser and respec path.** Local search plus beam over your point budget for your goal, under path-connectivity constraints. Output is an ordered respec sequence, best gain first, so a partial respec is still optimal. **Exports straight into the in-game Build Planner** as an official `.build` file. Hovering a node in-game shows "+3.1% DPS, step 4 of 12". This uses an officially documented, file-based feature with zero ban risk, and as far as I know no other tool does it.

### Changed (your idea, replaced with something compliant or more honest)

6. **"What top players do differently" becomes "Transplant".** Automatic scraping of top builds is ruled out: poe.ninja's builds API is internal and not for third-party use, and GGG's ladder needs an app we can't register. Instead you paste 1–N reference build links (poe.ninja "Export to PoB", pobb.in, maxroll). The engine computes **what each difference would be worth in your build**: every node, support and gear stat they have and you don't, ranked. That is causal for your character. Popularity statistics only say what's common.
7. **"What kills me" becomes thresholds plus Death Replay.** There is no sourced PoE2 per-boss damage data (PoB2's boss list is PoE1). So:
   - (a) Thresholds against PoB2's own Standard boss, Pinnacle and Uber presets (sourced from PoB data, labelled as generic).
   - (b) 1.0 adds an in-game **Death Summary** showing damage by type and mitigation. You type in the numbers, the engine reproduces the hit, and it finds the cheapest change that survives it.
   - (c) A named-boss table only if a datamined source can be validated. That is a research item, not a promise.
8. **Plain-language coach.** Templated explanations in the UI, built only from computed numbers. Plus an **MCP server**, so Claude (Desktop or Code) can call the engine and coach you, with no API spend from the tool itself.

### Cut

- **Crafting expected value (slam odds).** PoB2's spawn weights are placeholders (all 1). Any probability would be invented. Revisit only with a sourced weight table.
- **OAuth character import now.** GGG registration is closed. Use PoB2's import and paste the code. I can watch for registration reopening; applying is your decision.
- **Background price scraping of the trade site.** Ruled out: undocumented endpoint, plus the 4xx and rate-limit revocation rules.
- **In-game hotkey overlay.** Allowed under the overlay rules, but it adds ban surface for little gain over pasting.
- **Auto-picking "realistic" respec gold costs.** Not in any sourced data.

### Optional, your call

- **L1 trade fetch.** You press a button, and the doctor runs the search and fetches the top results at PoB-like volumes, through a header-driven limiter with safety margin. It then scores every listing exactly through PoB ("of these 10 rings, #7 is +14% for 1.2 div"). Risk: **low but non-zero.** Same behaviour as PoB2's built-in trader, but on an undocumented endpoint. Default off.

---

## 4. Plan to launch

| Window | Milestone | Done means |
|---|---|---|
| 19–26 Nov | M0 precision harness, M1 diagnosis | All fixtures match PoB-app stats exactly; diagnosis page on your build |
| 26 Nov–3 Dec | M2 engine, M3 trade links | Ranked change table with costs; trade links per slot; item paste scoring |
| 3–10 Dec | M4 optimiser + `.build` export; read patch notes (~4 Dec); patch-day rehearsal | Respec plan visible in-game Build Planner; update command tested on a PoB tag bump |
| 11 Dec → | Launch: guard holds until PoB2 1.0 ships, then `doctor update` | Fixtures re-exported from PoB2 1.0 pass |
| After | M5 Death Replay, M6 Transplant, M7 MCP coach | |

## 5. Open questions for you

1. Which OS do you play on? Linux and macOS work as-is. Windows needs either a LuaJIT build at the pinned commit (doable in CI) or WSL2.
2. Fixtures: send 3–5 PoB codes (or pobb.in links) exported from PoB2 v0.23.x. Ideally your own build plus varied archetypes: attack, spell, minion, DoT.
3. L1 trade fetch: on (opt-in button) or off?
4. OAuth: should I watch for GGG reopening registrations? The application would be in your name.
5. Coach: MCP via your existing Claude (no extra cost, recommended), or direct Claude API calls (costs money per use)?

## Sources

- GGG developer docs: https://www.pathofexile.com/developer/docs, `/authorization`, `/reference`, `/game` (Build Planner), `/data`, `/changelog`
- PoB2: https://github.com/PathOfBuildingCommunity/PathOfBuilding-PoE2 (CHANGELOG.md, HeadlessWrapper.lua, Data/, Classes/)
- PoB SimpleGraphic (LuaJIT pin): https://github.com/PathOfBuildingCommunity/PathOfBuilding-SimpleGraphic
- GGG tree export: https://github.com/grindinggear/poe2-skilltree-export
- poe.ninja API docs (via search summary): https://poe.ninja/docs/api
- 1.0 overview: https://maxroll.gg/poe2/news/everything-new-in-path-of-exile-2-1-0
- Death summary: https://pcgamer.com/games/rpg/path-of-exile-2s-game-director-thinks-a-death-recap-will-finally-allow-us-players-to-complain-about-the-right-things
- Poe2Stash ban: https://mein-mmo.de/en/player-invents-a-tool-that-makes-path-of-exile-2-much-better-a-day-later-it-is-already-banned,1238047
