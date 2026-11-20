// Build Doctor interface. Every number shown comes from the Rust side, which
// gets it from Path of Building; this file only lays it out.

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const state = {
  engine: null,          // { installed, pinned }
  busy: null,            // { title, detail, progress: 0..1 | null }
  error: null,
  view: "brief",         // brief | diagnosis | gear | item | plan | realistic | stats
  brief: null,           // { status: "running" | "done" | "error", result, error }
  overview: null,
  diagnosis: null,       // { status: "running" | "done" | "error", result, error }
  itemText: "",
  itemCheck: null,
  gear: null,            // { slots, values: { [slot]: SlotValues }, selected, link, copied }
  trade: loadTrade(),    // { league, goal, maxPrice, currency, anyBase }
  loadSeq: 0,            // which load a running diagnosis belongs to
  kept: new Set(),       // condition vars the player keeps up
  realistic: null,
  realisticKept: null,   // kept set the realistic result was computed for
  goal: "damage",
  steps: 5,
  maxLoss: 0,            // % of the other side the plan may give up; null = any
  plan: null,
  planProgress: null,
  copied: false,
  app: null,             // { version, commit }
  update: null,          // { release, status: "available" | "installing" | "error", progress, error }
};

// The app's mark (the icon without its tile): spectacles, one lens lit.
const MARK = `<svg class="mark" viewBox="0 0 64 24" aria-hidden="true"><rect x="0.5" y="10" width="7" height="4.5" rx="1" fill="#f6ead0"/><rect x="56.5" y="10" width="7" height="4.5" rx="1" fill="#f6ead0"/><circle cx="44" cy="12.25" r="5.6" fill="#ffb30f"/><circle cx="20" cy="12.25" r="8.4" fill="none" stroke="#f6ead0" stroke-width="3"/><circle cx="44" cy="12.25" r="8.4" fill="none" stroke="#f6ead0" stroke-width="3"/><path d="M28 9.6 Q32 7 36 9.6" fill="none" stroke="#f6ead0" stroke-width="2.6"/></svg>`;

// ---------------------------------------------------------------- helpers

const esc = (s) => String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));

function fmt(v) {
  if (v == null || Number.isNaN(v)) return "–";
  const abs = Math.abs(v);
  const digits = abs >= 1000 ? 0 : abs >= 100 ? 1 : 2;
  return v.toLocaleString("en-US", { maximumFractionDigits: digits });
}

function pct(before, after) {
  return before === 0 ? 0 : ((after - before) / before) * 100;
}

function signed(change) {
  if (Math.abs(change) < 0.05) return "0%";
  return (change > 0 ? "+" : "−") + Math.abs(change).toFixed(1) + "%";
}

// A change in percentage points ("+3 pts"), for values that are percentages.
function pointChange(points) {
  if (Math.abs(points) < 0.05) return "0";
  return `${points > 0 ? "+" : "−"}${fmt(Math.abs(points))} pts`;
}

function tone(change) {
  return Math.abs(change) < 0.05 ? "faint" : change > 0 ? "pos" : "neg";
}

function damage(plan, s) {
  return plan.usesFullDps ? s.fullDps : s.combinedDps + s.minionDps;
}

function sameSet(a, b) {
  return a && b && a.size === b.size && [...a].every((x) => b.has(x));
}

function oneLine(v) {
  return String(v).split(/[\n\t]+/).map((s) => s.trim()).filter(Boolean).join("; ");
}

// Search settings survive restarts (per-viewer convenience only).
function loadTrade() {
  const defaults = { league: "", goal: "balanced", maxPrice: "", currency: "exalted", anyBase: false };
  try {
    return { ...defaults, ...JSON.parse(localStorage.getItem("trade") ?? "{}") };
  } catch {
    return defaults;
  }
}

function saveTrade() {
  try {
    localStorage.setItem("trade", JSON.stringify(state.trade));
  } catch {
    // Storage can be unavailable; the settings then last for this session.
  }
}

// ---------------------------------------------------------------- actions

// Whether the work in progress was started on the page being shown.
const busyHere = () => state.busy && state.busy.view === state.view;

// Runs `task` as the one piece of work in progress, shown on the page that
// started it; other pages say it is running. Buttons that start work are
// disabled while it runs.
async function run(title, detail, task) {
  state.busy = { title, detail, progress: null, view: state.view };
  state.error = null;
  render();
  try {
    return await task();
  } catch (err) {
    state.error = String(err);
    return null;
  } finally {
    state.busy = null;
    render();
  }
}

async function checkForUpdate() {
  state.app = await invoke("app_info");
  render();
  try {
    const release = await invoke("check_update");
    if (release) state.update = { release, status: "available", progress: null };
  } catch {
    // Offline or GitHub unavailable: say nothing, try again next start.
  }
  render();
}

async function installUpdate() {
  state.update = { ...state.update, status: "installing", progress: 0, error: null };
  render();
  try {
    await invoke("install_update", { release: state.update.release });
  } catch (err) {
    state.update = { ...state.update, status: "error", error: String(err) };
    render();
  }
}

listen("update-progress", (event) => {
  const [done, total] = event.payload;
  if (state.update) state.update.progress = total ? done / total : null;
  render();
});

function renderUpdate() {
  const u = state.update;
  if (!u) return "";
  if (u.status === "installing") {
    const pctText = u.progress != null ? ` ${Math.round(u.progress * 100)}%` : "";
    return `<div class="update">Updating to ${esc(u.release.version)}…${pctText}<div class="track ${u.progress == null ? "indeterminate" : ""}"><i style="${u.progress == null ? "" : `width:${(u.progress * 100).toFixed(1)}%`}"></i></div></div>`;
  }
  if (u.status === "error") {
    return `<div class="update">Update failed: ${esc(u.error)} <button class="link" data-action="update">Try again</button></div>`;
  }
  return `<div class="update">Version ${esc(u.release.version)} is available <button class="link accent" data-action="update">Update and restart</button></div>`;
}

function versionLabel() {
  return state.app ? `Build Doctor ${esc(state.app.version)}` : "";
}

async function refreshEngine() {
  state.engine = await invoke("engine_status");
  render();
}

async function installEngine() {
  await run("Installing Path of Building", `Downloading PoB2 ${state.engine.pinned} from GitHub and checking every file. About 50 MB; this happens once.`, async () => {
    await invoke("install_engine");
    await refreshEngine();
  });
}

async function loadBuild(input) {
  const overview = await run("Calculating your build", "Starting Path of Building and loading the build.", () => invoke("load_build", { input }));
  if (!overview) return;
  state.overview = overview;
  runDiagnosis();
  state.kept = new Set();
  state.realistic = null;
  state.realisticKept = null;
  state.plan = null;
  state.itemCheck = null;
  state.gear = null;
  state.gearLoading = false;
  state.view = "brief";
  render();
}

// Runs in the background after a build loads; the overview fills it in when
// it arrives, unless another build was loaded meanwhile.
// Runs in the background after a build loads: the diagnosis, then the
// coach's brief from it, then the realistic numbers and the brief again with
// them. Results for a build loaded meanwhile are dropped.
async function runDiagnosis() {
  const seq = ++state.loadSeq;
  const current = () => seq === state.loadSeq;
  state.diagnosis = { status: "running" };
  state.brief = { status: "running" };
  try {
    const result = await invoke("diagnose");
    if (!current()) return;
    state.diagnosis = { status: "done", result };
  } catch (err) {
    if (!current()) return;
    state.diagnosis = { status: "error", error: String(err) };
    state.brief = { status: "error", error: String(err) };
    render();
    return;
  }
  await refreshBrief(current);
  try {
    if (!state.realistic) {
      const realistic = await invoke("realistic", { kept: [] });
      if (!current()) return;
      if (!state.realistic) {
        state.realistic = realistic;
        state.realisticKept = new Set();
      }
    }
  } catch {
    // The brief stands without the realistic numbers.
  }
  if (current()) await refreshBrief(current);
}

async function refreshBrief(current) {
  try {
    const result = await invoke("coach");
    if (current()) state.brief = { status: "done", result, withRealistic: !!state.realistic };
  } catch (err) {
    if (current()) state.brief = { status: "error", error: String(err) };
  }
  if (current()) render();
}

// Reads the worn items (instant) the first time the page opens. Not a
// `run`: it must not replace work in progress.
async function openGear() {
  if (state.gear || state.gearLoading) return;
  state.gearLoading = true;
  try {
    const result = await invoke("gear");
    state.gear = { slots: result.slots, values: {}, selected: null, copied: false, opened: false };
  } catch (err) {
    state.error = String(err);
  } finally {
    state.gearLoading = false;
    render();
  }
}

async function measureSlot(slot) {
  state.gear.selected = slot;
  state.gear.copied = state.gear.opened = false;
  if (!state.gear.values[slot]) {
    const result = await run(`Measuring every stat for ${slot}`, "Path of Building puts each stat that can roll there on a blank item of your base and recalculates your build. About 10 to 30 seconds.", () => invoke("slot_values", { slot }));
    if (!result) { render(); return; }
    state.gear.values[slot] = result;
  }
  render();
}

function tradeOptions() {
  const t = state.trade;
  const max = parseFloat(String(t.maxPrice).replace(",", "."));
  return {
    league: t.league,
    goal: t.goal,
    maxPrice: Number.isFinite(max) && max > 0 ? max : null,
    currency: t.currency,
    anyBase: t.anyBase,
  };
}

async function tradeLink() {
  state.error = null;
  try {
    return await invoke("trade_link", { slot: state.gear.selected, options: tradeOptions() });
  } catch (err) {
    state.error = String(err);
    render();
    return null;
  }
}

async function openTrade() {
  const url = await tradeLink();
  if (!url) return;
  try {
    await invoke("open_link", { url });
    state.gear.opened = true;
    render();
  } catch (err) {
    state.error = String(err);
    render();
  }
}

async function copyTrade() {
  const url = await tradeLink();
  if (!url) return;
  await navigator.clipboard.writeText(url);
  state.gear.copied = true;
  render();
}

// Search settings changed: earlier confirmations no longer apply.
function tradeChanged() {
  saveTrade();
  if (state.gear) state.gear.copied = state.gear.opened = false;
}

async function checkItem() {
  const item = (document.getElementById("item-paste")?.value ?? "").trim();
  if (!item) return;
  state.itemText = item;
  state.itemCheck = null;
  const result = await run("Checking the item", "Path of Building equips it in each slot it fits and recalculates your build.", () => invoke("check_item", { item }));
  if (result) state.itemCheck = result;
  render();
}

async function runRealistic() {
  const kept = [...state.kept];
  const result = await run("Testing each condition", "Path of Building recalculates the build with every assumed condition switched off, one at a time.", () => invoke("realistic", { kept }));
  if (!result) return;
  state.realistic = result;
  state.realisticKept = new Set(kept);
  render();
}

async function runPlan() {
  state.plan = null;
  state.planProgress = { step: 1, steps: state.steps, phase: "Starting" };
  const result = await run("Building your passive tree plan", "", () => invoke("plan", { kept: [...state.kept], goal: state.goal, steps: state.steps, maxLoss: state.maxLoss }));
  state.planProgress = null;
  if (result) state.plan = result;
  render();
}

function resetBuild() {
  state.loadSeq++;
  state.overview = null;
  state.diagnosis = null;
  state.brief = null;
  state.realistic = null;
  state.plan = null;
  state.error = null;
  render();
}

listen("install-progress", (event) => {
  const p = event.payload;
  if (!state.busy) return;
  state.busy.detail = `${p.done.toLocaleString("en-US")} of ${p.total.toLocaleString("en-US")} files · ${(p.bytes / 1e6).toFixed(1)} MB, each checked against Path of Building's own list`;
  state.busy.progress = p.total ? p.done / p.total : null;
  render();
});

listen("plan-progress", (event) => {
  state.planProgress = event.payload;
  if (state.busy) {
    const p = event.payload;
    state.busy.detail = `Step ${p.step} of ${p.steps}: ${p.phase}`;
    state.busy.progress = Math.min(1, (p.step - 1 + phaseShare(p.phase)) / p.steps);
    render();
  }
});

function phaseShare(phase) {
  if (phase.startsWith("Screening every")) return 0.1;
  if (phase.startsWith("Screening the best")) return 0.6;
  if (phase.startsWith("Trying")) return 0.7;
  if (phase.startsWith("Reloading")) return 0.95;
  return 0;
}

// ---------------------------------------------------------------- views

function render() {
  const root = document.getElementById("app");
  if (!state.overview) {
    root.innerHTML = renderStart();
    const area = root.querySelector("textarea");
    if (area && !state.busy) area.focus();
    return;
  }
  const scroll = root.querySelector(".main")?.scrollTop ?? 0;
  root.innerHTML = `<div class="shell">${renderRail()}<main class="main"><div class="page">${renderView()}</div></main></div>`;
  const main = root.querySelector(".main");
  if (main) main.scrollTop = scroll;
}

function renderStart() {
  const e = state.engine;
  const ready = e && e.installed;
  const engineLine = !e
    ? ""
    : ready
      ? `Path of Building ${esc(e.installed)}`
      : `Path of Building ${esc(e.pinned)} is not installed yet.`;
  return `
    <div class="start"><div class="start-card">
      <div class="wordmark">${MARK}Build Doctor</div>
      <h1>Load a build</h1>
      <p>Paste a Path of Building code, or a pobb.in, poe.ninja, maxroll or poe2db link. You get a diagnosis first: what to fix, each fix measured by Path of Building.</p>
      <p class="faint where">Where to get one: in Path of Building, Import/Export Build, then Generate and Copy. On poe.ninja, open a character and use its Path of Building export.</p>
      <textarea class="paste" id="paste" spellcheck="false" placeholder="Paste here" ${state.busy || !ready ? "disabled" : ""}></textarea>
      <div class="start-actions">
        <span class="engine-line">${engineLine}${state.app ? ` · ${versionLabel()}` : ""}</span>
        ${ready
          ? `<button class="btn primary" data-action="load" ${state.busy ? 'aria-disabled="true"' : ""}>Analyse build</button>`
          : `<button class="btn primary" data-action="install" ${state.busy || !e ? 'aria-disabled="true"' : ""}>Install Path of Building</button>`}
      </div>
      ${state.busy ? renderBusy() : ""}
      ${state.error ? `<div class="error">${esc(state.error)}</div>` : ""}
      ${state.update ? `<div style="margin-top:18px">${renderUpdate()}</div>` : ""}
    </div></div>`;
}

function renderBusy() {
  const b = state.busy;
  const determinate = b.progress != null;
  return `
    <div class="panel progress" style="margin-top:16px">
      <div class="phase"><strong>${esc(b.title)}</strong><span class="muted">${esc(b.detail)}</span></div>
      <div class="track ${determinate ? "" : "indeterminate"}"><i style="${determinate ? `width:${(b.progress * 100).toFixed(1)}%` : ""}"></i></div>
    </div>`;
}

function renderRail() {
  const b = state.overview.build;
  const conditions = state.overview.assumptions.filter((a) => a.kind === "condition").length;
  const findings = state.diagnosis?.status === "done" ? state.diagnosis.result.findings.filter((f) => f.severity !== "note").length : null;
  const nav = [
    ["brief", "Your build", "What to do, and why", ""],
    ["diagnosis", "Diagnosis", "Every finding, measured", findings ? `${findings}` : ""],
    ["gear", "Gear upgrades", "What to buy next", ""],
    ["item", "Check an item", "Is it an upgrade?", ""],
    ["plan", "Passive tree plan", "Free gains from respecs", state.plan ? `${state.plan.steps.length} steps` : ""],
    ["realistic", "Realistic numbers", "Without assumed buffs", conditions ? `${conditions}` : ""],
    ["stats", "Stats", "Every number PoB calculated", ""],
  ];
  return `
    <aside class="rail">
      <div class="wordmark">${MARK}Build Doctor</div>
      <div class="build-id">
        <div class="asc">${esc(b.ascendancy ?? b.class)}</div>
        <div class="meta">${b.ascendancy ? esc(b.class) + " · " : ""}Level ${esc(b.level ?? "–")}</div>
        <div class="meta">${esc(b.mainSkill ?? "")}</div>
      </div>
      <nav class="nav">
        ${nav.map(([id, label, hint, note]) => `<button class="${state.view === id ? "active" : ""}" data-view="${id}"><span class="nav-text">${label}<span class="hint">${hint}</span></span><small>${esc(note)}</small></button>`).join("")}
      </nav>
      <div class="rail-foot">
        ${renderUpdate()}
        <span class="engine">Path of Building ${esc(b.pobVersion)} · tree ${esc(b.treeVersion ?? "–")}</span>
        <span class="engine">${versionLabel()}</span>
        <button class="link" data-action="reset">Load another build</button>
      </div>
    </aside>`;
}

const PAGES = { brief: "Your build", diagnosis: "Diagnosis", gear: "Gear upgrades", item: "Check an item", plan: "Passive tree plan", realistic: "Realistic numbers", stats: "Stats" };

function renderView() {
  const here = !state.busy || state.busy.view === state.view;
  const busy = state.busy && here ? renderBusy() : "";
  const elsewhere = state.busy && !here
    ? `<div class="verdict"><strong>Working</strong><span class="muted">${esc(state.busy.title)} on ${esc(PAGES[state.busy.view])}. You can keep reading; new work starts when it is done.</span></div>`
    : "";
  const error = state.error ? `<div class="error">${esc(state.error)}</div>` : "";
  if (state.view === "realistic") return elsewhere + renderRealistic() + busy + error;
  if (state.view === "plan") return elsewhere + renderPlan() + error;
  if (state.view === "item") return elsewhere + renderItem() + error;
  if (state.view === "gear") return elsewhere + renderGear() + error;
  if (state.view === "stats") return elsewhere + renderStats() + error;
  if (state.view === "brief") return elsewhere + renderBrief() + error;
  return elsewhere + renderDiagnosisPage() + error;
}

function stat(group, name) {
  for (const g of state.overview.groups) for (const r of g.rows) if (r.stat === name) return r.value;
  return null;
}

// The coach: what the numbers mean and what to do, in order.
const TOOL_PAGES = { gear: "Gear upgrades", item: "Check an item", plan: "Passive tree plan", realistic: "Realistic numbers" };

const TERMS = [
  ["Max hit", "The biggest single hit of one damage type you survive from full, as Path of Building calculates it against the enemy set in its configuration."],
  ["Total EHP", "Path of Building's effective hit pool: one number for how much damage it takes to kill you, counting life, energy shield and mitigation."],
  ["Full DPS", "Damage per second of every skill the build includes in Full DPS, added together. Builds without it use the main skill's DPS."],
  ["Conditions", "Settings in Path of Building that say a combat state is always true, such as an enemy being shocked. They only count if you keep them up."],
  ["Resistance penalty", "From the endgame on, the game lowers your elemental resistances by 60%, so gear has to make up for it."],
  ["Spirit", "The resource that pays for persistent skills: auras, buffs and permanent minions."],
];

function renderBrief() {
  const head = `
    <div class="page-head"><div>
      <h1>Your build</h1>
      <p>What your numbers mean and what to do, in order. Every number is Path of Building's; every explanation of the game links its source.</p>
    </div></div>`;
  const b = state.brief;
  if (!b || b.status === "running") {
    return head + `<div class="panel progress">
      <div class="phase"><strong>Working out your plan</strong><span class="muted">Path of Building is testing fixes, skill levels and combat conditions on your build. About half a minute.</span></div>
      <div class="track indeterminate"><i></i></div>
    </div>`;
  }
  if (b.status === "error") {
    return head + `<div class="panel"><div class="panel-body" style="padding-top:16px"><p class="muted" style="margin:0 0 10px">The plan could not be worked out. The other pages still work.</p><div class="error" style="margin:0">${esc(b.error)}</div></div></div>`;
  }
  const r = b.result;
  const verdict = `<div class="panel brief-verdict"><div class="panel-body">${r.verdict.map((v) => `<p>${esc(v)}</p>`).join("")}</div></div>`;
  const steps = `<div class="section-head"><h2>What to do, in order</h2>${b.withRealistic ? "" : `<span class="meta">Adding the realistic numbers…</span>`}</div>
    <ol class="plan-steps">${r.steps.map((s, i) => `
      <li class="panel plan-step">
        <div class="step-n">${i + 1}</div>
        <div class="step-body">
          <h3>${esc(s.title)}</h3>
          <dl>
            <dt>Why</dt><dd>${esc(s.why)}</dd>
            <dt>How</dt><dd>${esc(s.how)}</dd>
            ${s.gain ? `<dt>Path of Building</dt><dd class="gain">${esc(s.gain)}</dd>` : ""}
          </dl>
          ${s.tool || s.source ? `<div class="step-actions">
            ${s.tool ? `<button class="btn small" data-view="${s.tool}">Open ${esc(TOOL_PAGES[s.tool])}</button>` : ""}
            ${s.source ? `<button class="link" data-open="${esc(s.source.url)}">${esc(s.source.label)}</button>` : ""}
          </div>` : ""}
        </div>
      </li>`).join("")}
    </ol>`;
  const points = (title, list) => `<div class="panel"><div class="panel-head"><h2>${title}</h2></div><div class="panel-body">
    ${list.map((p) => `<div class="point"><div class="point-title">${esc(p.title)}</div><div class="muted">${esc(p.text)}</div></div>`).join("")}
  </div></div>`;
  const how = `<div class="section-head"><h2>How your build works</h2></div>
    <div class="grid cols-2">${points("Damage", r.damage)}${points("Defence", r.defence)}</div>`;
  const terms = `<details class="terms"><summary>What the terms mean</summary><dl>${TERMS.map(([t, d]) => `<dt>${t}</dt><dd>${esc(d)}</dd>`).join("")}</dl></details>`;
  return head + verdict + steps + how + terms;
}

// The landing page: can the numbers be trusted, the key numbers, what to
// fix first, and where to go next.
function renderDiagnosisPage() {
  const o = state.overview;
  const full = stat("Offence", "FullDPS");
  const dmg = full ?? stat("Offence", "CombinedDPS");
  const hits = (o.groups.find((g) => g.title === "Largest hit survived")?.rows ?? []).slice().sort((a, b) => a.value - b.value);
  const smallest = hits[0];
  const life = stat("Pools", "Life");
  const es = stat("Pools", "EnergyShield");
  const r = o.reproduction;

  const verdict = r.saved === 0
    ? ""
    : r.differences.length === 0
      ? `<div class="verdict ok"><strong>Exact</strong><span class="muted">All ${r.saved} numbers saved in this build reproduced by Path of Building ${esc(o.build.pobVersion)}.</span></div>`
      : `<div class="verdict warn"><strong>${r.differences.length} of ${r.saved} saved numbers differ</strong><span class="muted">The build was probably saved by a different Path of Building version. Every number here is ${esc(o.build.pobVersion)}'s.</span></div>`;
  const outdated = o.build.treeOutdated
    ? `<div class="verdict warn"><strong>Old passive tree</strong><span class="muted">This build uses tree ${esc(o.build.treeVersion)}; the newest is ${esc(o.build.latestTreeVersion)}. Numbers describe the older game version.</span></div>`
    : "";

  return `
    <div class="page-head"><div>
      <h1>Diagnosis</h1>
      <p>What to fix first. Every fix was applied to your build and recalculated by Path of Building.</p>
    </div></div>
    ${verdict}${outdated}
    <div class="tiles">
      ${tile(full ? "Full DPS" : "Combined DPS", fmt(dmg), o.minionDps ? `Minions ${fmt(o.minionDps)}` : "Main skill")}
      ${tile("Total EHP", fmt(stat("Pools", "TotalEHP")), "Effective hit pool")}
      ${tile("Smallest max hit", smallest ? fmt(smallest.value) : "–", smallest ? `${smallest.label} damage` : "")}
      ${tile(es && es > (life ?? 0) ? "Energy shield" : "Life", fmt(es && es > (life ?? 0) ? es : life), es && life ? `Life ${fmt(life)} · ES ${fmt(es)}` : es ? "Energy shield only" : "No energy shield")}
    </div>
    ${renderDiagnosis()}
    ${o.warnings.length && state.diagnosis?.status === "error" ? `<div class="verdict warn"><strong>Path of Building warns</strong><span class="muted">${o.warnings.map(esc).join(" · ")}</span></div>` : ""}
    ${renderNext()}`;
}

// Where to go after the diagnosis, with what each page will show for this build.
function renderNext() {
  const o = state.overview;
  const conditions = o.assumptions.filter((a) => a.kind === "condition").length;
  const rows = [
    ["gear", "Gear upgrades", "Pick a slot: Path of Building measures what every stat that can roll there is worth to you, then opens a trade search ranked by it."],
    ["item", "Check an item", "Paste an item copied in game with Ctrl+C. Path of Building equips it in each slot it fits and shows exactly what changes."],
    ["plan", "Passive tree plan", "The best passive swaps for your build, same point count, each one recalculated. Respecs cost only gold."],
    ["realistic", "Realistic numbers", conditions
      ? `Your numbers assume ${conditions} combat condition${conditions === 1 ? "" : "s"} (enemy Shocked, recently hit…). See what each is worth and keep only the ones you really sustain.`
      : "This build assumes no combat conditions, so its numbers are already the realistic ones."],
    ["stats", "Stats", "Every number Path of Building calculated, what they assume, and the gear mods it cannot read."],
  ];
  return `<div class="panel next">
    <div class="panel-head"><h2>Next</h2></div>
    <div class="panel-body">${rows.map(([view, title, text]) => `
      <button class="next-row" data-view="${view}"><span class="next-title">${title}</span><span class="muted">${text}</span></button>`).join("")}
    </div>
  </div>`;
}

// Everything PoB calculated and everything those numbers depend on.
function renderStats() {
  const o = state.overview;
  const conditions = o.assumptions.filter((a) => a.kind === "condition");
  const scenario = o.assumptions.filter((a) => a.kind === "scenario" && a.section !== "Quest Rewards");
  const quests = o.assumptions.filter((a) => a.section === "Quest Rewards");
  const gear = o.unsupported.filter((m) => !m.usable);
  const usables = o.unsupported.filter((m) => m.usable);
  return `
    <div class="page-head"><div>
      <h1>Stats</h1>
      <p>Your build as Path of Building calculates it, with everything those numbers depend on.</p>
    </div></div>
    <div class="grid cols-2">
      <div class="panel">
        <div class="panel-head"><h2>Stats</h2><span class="meta">Path of Building ${esc(o.build.pobVersion)}</span></div>
        <div class="panel-body"><table class="kv">
          ${o.groups.map((g) => `<tr class="group"><td colspan="2" class="label">${esc(g.title)}</td></tr>` + g.rows.map((row) => `<tr><td class="muted">${esc(row.label)}</td><td title="${esc(row.exact)}">${fmt(row.value)}${/Resist/.test(row.stat) || /Chance/.test(row.stat) ? "%" : ""}</td></tr>`).join("")).join("")}
        </table></div>
      </div>
      <div class="stack">
        <div class="panel">
          <div class="panel-head"><h2>Assumed conditions</h2><span class="meta">${conditions.length ? `${conditions.length} · <button class="link" data-view="realistic">see what they are worth</button>` : "none"}</span></div>
          <div class="panel-body">${conditions.length ? assumptionList(conditions) : `<div class="muted">The build's numbers assume no combat conditions.</div>`}</div>
        </div>
        <div class="panel">
          <div class="panel-head"><h2>Fight simulated</h2></div>
          <div class="panel-body">${assumptionList(scenario)}${quests.length ? `<div class="faint" style="padding-top:8px">${quests.length} campaign rewards set</div>` : ""}</div>
        </div>
        <div class="panel">
          <div class="panel-head"><h2>Not counted</h2><span class="meta">mods Path of Building cannot read</span></div>
          <div class="panel-body">
            ${gear.length ? `<ul class="lines">${gear.map((m) => `<li><span class="slot">${esc(m.slot)}</span><span class="selectable">${esc(m.line)}<div class="item">${esc(m.item)}</div></span></li>`).join("")}</ul>` : `<div class="muted">Every gear mod is counted.</div>`}
            ${usables.length ? `<details><summary>${usables.length} charm and flask lines, which Path of Building does not model</summary><ul class="lines">${usables.map((m) => `<li><span class="slot">${esc(m.slot)}</span><span>${esc(m.line)}</span></li>`).join("")}</ul></details>` : ""}
          </div>
        </div>
      </div>
    </div>`;
}

const GOALS = [["damage", "Damage"], ["survival", "Survival"], ["balanced", "Both"]];

function goalScore(share, goal) {
  if (goal === "damage") return share.damage;
  if (goal === "survival") return share.ehp + share.minHit;
  return share.damage + share.ehp + share.minHit;
}

function share(v) {
  if (Math.abs(v) < 0.005) return `<span class="faint">–</span>`;
  return `<span class="${v > 0 ? "pos" : "neg"}">${v > 0 ? "+" : "−"}${Math.abs(v).toFixed(2)}%</span>`;
}

// "What should I buy for this slot?"
function renderGear() {
  const g = state.gear;
  const t = state.trade;
  const head = `
    <div class="page-head"><div>
      <h1>Gear upgrades</h1>
      <p>Pick a slot. Path of Building measures what every stat that can roll there is worth to your build, then builds a trade search that lists items for sale best first.</p>
    </div></div>`;
  if (!g) return head + (state.gearLoading ? `<div class="panel empty">Reading your gear…</div>` : "");

  const settings = `
    <div class="panel" style="margin-bottom:16px"><div class="panel-body settings">
      <label class="field"><span class="field-label">League</span><input class="input" id="league" value="${esc(t.league)}" placeholder="As on the trade site, e.g. Standard" spellcheck="false"></label>
      <div class="field"><span class="field-label">Rank by</span><div class="seg">${GOALS.map(([v, l]) => `<button class="${t.goal === v ? "on" : ""}" data-trade-goal="${v}">${l}</button>`).join("")}</div></div>
      <label class="field"><span class="field-label">Max price</span><span class="price"><input class="input" id="max-price" value="${esc(t.maxPrice)}" placeholder="Any" inputmode="decimal">
        <select class="input" id="currency">${[["exalted", "Exalted"], ["divine", "Divine"], ["chaos", "Chaos"]].map(([v, l]) => `<option value="${v}" ${t.currency === v ? "selected" : ""}>${l}</option>`).join("")}</select></span></label>
    </div></div>`;

  const slots = `<div class="slot-grid">${g.slots.map((s) => `
    <button class="slot-card ${g.selected === s.slot ? "on" : ""}" data-slot="${esc(s.slot)}" ${state.busy ? 'aria-disabled="true"' : ""}>
      <span class="slot-name">${esc(s.slot)}</span><span class="slot-item">${esc(s.item.split(", ")[0])}</span><span class="slot-base">${esc(s.base)}</span>
    </button>`).join("")}</div>`;

  let detail = "";
  if (busyHere()) detail = renderBusy();
  else if (g.selected && g.values[g.selected]) detail = renderSlotValues(g.values[g.selected]);
  else detail = `<div class="panel empty">Pick a slot above. Measuring takes 10 to 30 seconds; switching between measured slots is instant.</div>`;
  return head + settings + slots + detail;
}

function renderSlotValues(v) {
  const t = state.trade;
  const rows = v.stats.slice().sort((a, b) => goalScore(b.share, t.goal) - goalScore(a.share, t.goal)).filter((s) => goalScore(s.share, t.goal) >= 0.05);
  const shown = rows.slice(0, 15);
  const w = v.wornItem;
  const worn = [["damage", w.damage], ["Total EHP", w.ehp], ["smallest max hit", w.minHit]]
    .filter(([, x]) => Math.abs(x) >= 0.05)
    .map(([l, x]) => `${x > 0 ? "+" : "−"}${Math.abs(x).toFixed(1)}% ${l}`);
  const sameBase = v.slot.tradeCategory.startsWith("weapon.") || v.slot.tradeCategory.startsWith("armour.");
  const table = (list) => `<table class="data stats-table">
    <tr><th>Stat, at a mid roll</th><th>Damage</th><th>Total EHP</th><th>Smallest max hit</th></tr>
    ${list.map((s) => `<tr><td>${esc(s.line)}${s.implicit ? ` <span class="faint">implicit</span>` : ""}</td><td>${share(s.share.damage)}</td><td>${share(s.share.ehp)}</td><td>${share(s.share.minHit)}</td></tr>`).join("")}
  </table>`;
  return `<div class="panel">
    <div class="panel-head"><h2>${esc(v.slot.slot)} <span class="faint" style="font-weight:400">${esc(v.slot.item)}</span></h2><span class="meta">${v.measured} stats measured</span></div>
    <div class="panel-body">
      <p class="muted" style="margin:0 0 14px">Your ${esc(v.slot.item.split(",")[0])} ${worn.length ? `adds ${worn.join(", ")}` : "adds nothing measurable"} compared with a blank ${esc(v.slot.base)}.</p>
      <div class="toolbar">
        <button class="btn primary" data-action="open-trade" ${rows.length ? "" : 'aria-disabled="true"'}>${state.gear.opened ? "Opened in your browser" : "Open trade search"}</button>
        <button class="btn" data-action="copy-trade" ${rows.length ? "" : 'aria-disabled="true"'}>${state.gear.copied ? "Link copied" : "Copy link"}</button>
        ${sameBase ? `<label class="check-label"><input type="checkbox" class="check" id="any-base" ${t.anyBase ? "checked" : ""}> Include other bases</label>` : ""}
      </div>
      <ol class="how">
        <li>The search lists items for sale, best for your build first. Its weighted sum is the estimated gain in percent, adding up each stat's value below.</li>
        <li>Copy an item you like (the trade site's copy button) and paste it into <button class="link accent" data-view="item">Check an item</button> for the exact result: stats measured alone can be worth more or less together.</li>
        ${sameBase && !t.anyBase ? `<li>Only ${esc(v.slot.base)} is searched: ${v.slot.tradeCategory.startsWith("weapon.") ? "a base's own damage is" : "a base's own defences are"} not in the weights, so other bases would rank wrongly.</li>` : ""}
      </ol>
      ${rows.length ? table(shown) : `<div class="muted">No stat that can roll here adds to ${t.goal === "damage" ? "damage" : t.goal === "survival" ? "survival" : "damage or survival"} for this build.</div>`}
      ${rows.length > shown.length ? `<details><summary>${rows.length - shown.length} more stats</summary>${table(rows.slice(shown.length))}</details>` : ""}
    </div>
  </div>`;
}

// "Is this item an upgrade?"
function renderItem() {
  const c = state.itemCheck;
  const head = `
    <div class="page-head"><div>
      <h1>Check an item</h1>
      <p>Copy an item in game with Ctrl+C (or from the trade site) and paste it here. Path of Building equips it, exactly as written, in each slot it fits and recalculates your build.</p>
    </div></div>
    <div class="panel" style="margin-bottom:16px"><div class="panel-body" style="padding-top:14px">
      <textarea class="paste item-paste" id="item-paste" spellcheck="false" placeholder="Paste an item here" ${state.busy ? "disabled" : ""}>${esc(state.itemText)}</textarea>
      <div class="toolbar" style="margin-top:10px">
        <span class="faint">Ctrl+Enter to check</span>
        <span class="spacer"></span>
        <button class="btn primary" data-action="check-item" ${state.busy ? 'aria-disabled="true"' : ""}>Check item</button>
      </div>
    </div></div>`;
  if (busyHere()) return head + renderBusy();
  if (!c) return head;
  const unread = c.unread.length
    ? `<div class="verdict warn"><strong>Not counted</strong><span class="muted">Path of Building cannot read ${c.unread.length === 1 ? "this line" : "these lines"}, so no number includes ${c.unread.length === 1 ? "it" : "them"}: ${c.unread.map(esc).join(" · ")}</span></div>`
    : "";
  if (!c.slots.length) {
    return head + unread + `<div class="panel empty">${esc(c.item)} fits no slot this build uses: no equipment slot of its kind, and no allocated jewel socket that takes it. Weapon-swap slots are not checked.</div>`;
  }
  const verdicts = {
    better: ["Better", "pos", "More damage or survival, and less of neither."],
    worse: ["Worse", "neg", "Less damage or survival, and more of neither."],
    mixed: ["Trade-off", "warn", "Gains on one side, losses on the other."],
    same: ["No change", "faint", "Damage and Total EHP stay the same."],
  };
  const slots = c.slots.map((s) => {
    const [word, toneClass, meaning] = verdicts[s.verdict];
    return `<div class="panel slot-result">
      <div class="panel-head"><h2>${esc(s.label)} <span class="faint" style="font-weight:400">instead of ${esc(s.replaces ?? "nothing")}</span></h2><span class="verdict-word ${toneClass}" title="${esc(meaning)}">${word}</span></div>
      <div class="panel-body">
        <div class="tiles three">${s.headline.map((e) => `<div class="tile"><div class="label">${esc(e.label)}</div><div class="value">${fmt(e.after)}</div><div class="sub ${tone(e.change)}">${signed(e.change)} <span class="faint">from ${fmt(e.before)}</span></div></div>`).join("")}</div>
        ${s.newWarnings.length ? `<div class="item-warnings">${s.newWarnings.map((w) => `<div><span class="neg">Path of Building warns</span> ${esc(w)}</div>`).join("")}</div>` : ""}
        ${s.changes.length ? `<table class="kv changes">${s.changes.map((e) => `<tr><td class="muted">${esc(e.label)}</td><td>${e.percent ? `${fmt(e.before)}% → ${fmt(e.after)}%` : `${fmt(e.before)} → ${fmt(e.after)}`}</td><td class="change ${e.percent ? tone(e.after - e.before) : tone(e.change)}">${e.percent ? pointChange(e.after - e.before) : signed(e.change)}</td></tr>`).join("")}</table>` : ""}
      </div>
    </div>`;
  }).join("");
  return head + `<div class="item-name">${esc(c.item)}</div>` + unread + `<div class="stack">${slots}</div>`;
}

function renderDiagnosis() {
  const d = state.diagnosis;
  if (!d) return "";
  const head = (meta) => `<div class="panel-head"><h2>What to fix</h2><span class="meta">${meta}</span></div>`;
  if (d.status === "running") {
    return `<div class="panel diagnosis">${head("checking")}<div class="panel-body">
      <div class="muted">Path of Building is testing resistances, accuracy and its own warnings, recalculating the build in full for each fix.</div>
      <div class="track indeterminate" style="margin-top:12px"><i></i></div>
    </div></div>`;
  }
  if (d.status === "error") {
    return `<div class="panel diagnosis">${head("")}<div class="panel-body"><p class="muted" style="margin:0 0 10px">The diagnosis could not finish. Your numbers above and the other pages are not affected. Path of Building's warnings are listed below.</p><div class="error" style="margin:0">${esc(d.error)}</div></div></div>`;
  }
  const findings = d.result.findings;
  const count = (severity, one, many) => {
    const n = findings.filter((f) => f.severity === severity).length;
    return n ? `${n} ${n === 1 ? one : many}` : null;
  };
  const meta = [count("problem", "problem", "problems"), count("gain", "fix", "fixes"), count("note", "note", "notes")].filter(Boolean).join(" · ");
  return `<div class="panel diagnosis">${head(meta)}<div class="panel-body">
    ${findings.length
      ? `<ol class="findings">${findings.map(finding).join("")}</ol>`
      : `<div class="muted">Nothing to fix. Path of Building finds no problems and no measurable gains.</div>`}
  </div></div>${renderLevels(d.result.levels)}`;
}

// What one more level of each kind of skill does to the build's damage.
function renderLevels(levels) {
  if (!levels.length) return "";
  return `<div class="panel levels">
    <div class="panel-head"><h2>One more skill level</h2><span class="meta">each measured by Path of Building</span></div>
    <div class="panel-body">
      <p class="muted intro">What +1 level of each kind of skill does to your damage. It is why gear with “+ to Level of all … Skills” can be worth so much: check such an item under Check an item for its exact value.</p>
      <table class="data level-table">
        <tr><th>Mod</th><th>${esc(levels[0].damage.label)}</th></tr>${levels.map((l) => `
        <tr><td>${esc(l.line)}${l.newWarnings.length ? `<div class="level-warning">Path of Building warns: ${l.newWarnings.map(esc).join(" · ")}</div>` : ""}</td>
        <td class="${tone(l.damage.change)}">${signed(l.damage.change)}</td></tr>`).join("")}
      </table>
    </div>
  </div>`;
}

function finding(f) {
  const tag = { problem: "Problem", gain: "Fix", note: "Note" }[f.severity];
  const effects = f.effects.map((e) => e.percent
    ? `<span>${esc(e.label)} <span class="num">${fmt(e.before)}% → ${fmt(e.after)}%</span></span>`
    : `<span>${esc(e.label)} <span class="num">${fmt(e.before)} → ${fmt(e.after)}</span> <span class="${tone(e.change)}">${signed(e.change)}</span></span>`,
  ).join('<span class="sep">·</span>');
  return `<li class="finding is-${f.severity}">
    <span class="tag">${tag}</span>
    <div>
      <div class="title selectable">${esc(f.title)}</div>
      ${f.fix ? `<div class="fix">Needs <span class="selectable">${esc(f.fix)}</span>${f.severity === "gain" ? ` <span class="sep">·</span> <button class="link accent" data-view="item">Check an item that has it</button>` : ""}</div>` : ""}
      ${effects ? `<div class="effects">${effects}</div>` : ""}
      ${f.detail ? `<div class="detail">${esc(f.detail)}</div>` : ""}
    </div>
  </li>`;
}

function tile(label, value, sub) {
  return `<div class="tile"><div class="label">${esc(label)}</div><div class="value">${value}</div><div class="sub">${esc(sub)}</div></div>`;
}

function assumptionList(rows) {
  let section = null;
  const out = [];
  for (const a of rows) {
    if (a.section !== section) {
      section = a.section;
      out.push(`<div class="section-title">${esc(section)}</div>`);
    }
    out.push(`<div style="display:flex;justify-content:space-between;gap:16px;padding:3px 0"><span>${esc(a.label)}</span><span class="muted" style="text-align:right">${esc(oneLine(a.value))}</span></div>`);
  }
  return out.join("");
}

function renderRealistic() {
  const r = state.realistic;
  const stale = r && !sameSet(state.realisticKept, state.kept);
  const conditions = state.overview.assumptions.filter((a) => a.kind === "condition");
  const head = `
    <div class="page-head"><div>
      <h1>Realistic numbers</h1>
      <p>Your numbers assume combat conditions, such as an enemy being shocked. Path of Building switches each one off to show what it is worth. Tick the ones you really keep up in a boss fight: “Keeping yours” then shows your realistic numbers, and the passive tree plan uses the same ticks.</p>
    </div>
    ${!r || stale ? `<button class="btn primary" data-action="realistic" ${state.busy || !conditions.length ? 'aria-disabled="true"' : ""}>${r ? "Update with your ticks" : "Calculate"}</button>` : ""}</div>`;
  if (!conditions.length) return head + `<div class="panel empty">This build assumes no combat conditions, so its numbers already are the realistic ones.</div>`;
  if (!r) {
    if (busyHere()) return head;
    return head + `<div class="panel"><div class="panel-head"><h2>Conditions</h2><span class="meta">${conditions.length} assumed · tick what you sustain</span></div><div class="panel-body"><table class="data conds">${conditions.map((c) => `<tr class="${state.kept.has(c.var) ? "kept" : ""}"><td><label class="cond"><input type="checkbox" class="check" data-keep="${esc(c.var)}" ${state.kept.has(c.var) ? "checked" : ""}><span class="cond-text"><div>${esc(c.label)}</div><small>${condNote(c)}</small></span></label></td></tr>`).join("")}</table></div></div>`;
  }

  const columns = [["As configured", r.configured, 0], ["No conditions", r.floor, 1]];
  if (r.kept) columns.push(["Keeping yours", r.kept, 2]);
  const summary = `
    <div class="panel" style="margin-bottom:16px"><div class="panel-body" style="padding-top:12px">
      <table class="data">
        <tr><th></th>${columns.map(([t]) => `<th colspan="2">${t}</th>`).join("")}</tr>
        ${r.metrics.map((m, i) => `<tr><td>${esc(m.label)}</td>${columns.map(([, values, col], c) => {
          const v = values[i];
          const type = m.key === "minHit" && r.minHitType[col] ? ` <span class="faint">${esc(r.minHitType[col])}</span>` : "";
          const ch = c === 0 ? "" : signed(pct(r.configured[i], v));
          return `<td class="big">${fmt(v)}${type}</td><td class="change ${c === 0 ? "" : tone(pct(r.configured[i], v))}">${ch}</td>`;
        }).join("")}</tr>`).join("")}
      </table>
      ${stale ? `<div class="note">Your ticks changed since this was calculated. Use “Update with your ticks” to see your realistic numbers.</div>` : ""}
    </div></div>`;

  const dmgIndex = r.metrics.findIndex((m) => m.damage);
  const magnitude = (c) => Math.max(...c.changes.map((v) => Math.abs(v)));
  const rows = r.conditions.slice().sort((a, b) => magnitude(b) - magnitude(a));
  const moving = rows.filter((c) => magnitude(c) >= 0.05);
  const inert = rows.filter((c) => magnitude(c) < 0.05);
  const maxDmg = Math.max(1, ...moving.map((c) => Math.abs(c.changes[dmgIndex] ?? 0)));

  const table = `
    <div class="panel">
      <div class="panel-head"><h2>What each condition is worth</h2><span class="meta">conditions overlap, so rows don't add up</span></div>
      <div class="panel-body">
        <table class="data conds">
          <tr><th>Keep</th><th></th>${r.metrics.map((m) => `<th>${esc(m.label)}</th>`).join("")}</tr>
          ${moving.map((c) => {
            const a = c.condition;
            const kept = state.kept.has(a.var);
            const d = c.changes[dmgIndex] ?? 0;
            return `<tr class="${kept ? "kept" : ""}">
              <td><label class="cond"><input type="checkbox" class="check" data-keep="${esc(a.var)}" ${kept ? "checked" : ""}><span class="cond-text"><div title="${esc(a.label)}">${esc(a.label)}</div><small>${condNote(a)}</small></span></label></td>
              <td>${dmgIndex >= 0 ? `<div class="bar"><i class="${d > 0 ? "up" : ""}" style="width:${((Math.abs(d) / maxDmg) * 100).toFixed(1)}%"></i></div>` : ""}</td>
              ${c.changes.map((v) => `<td class="${tone(v)}">${signed(v)}</td>`).join("")}
            </tr>`;
          }).join("")}
        </table>
        ${inert.length ? `<details><summary>${inert.length} conditions change none of these numbers</summary><div class="muted" style="padding:6px 0 0">${inert.map((c) => esc(c.condition.label)).join(" · ")}</div></details>` : ""}
      </div>
    </div>
    ${r.scenario.length ? `<div class="note">Fight simulated, kept as configured: ${r.scenario.map((a) => `${esc(a.label)} ${esc(oneLine(a.value))}`).join(" · ")}</div>` : ""}`;
  return head + summary + table;
}

// "Section · value", without repeating the label or a plain "yes".
function condNote(a) {
  const parts = [];
  if (a.section !== a.label) parts.push(esc(a.section));
  if (a.value !== "yes") parts.push(esc(oneLine(a.value)));
  return parts.join(" · ");
}

function renderPlan() {
  const p = state.plan;
  const keptCount = state.kept.size;
  const total = state.overview.assumptions.filter((a) => a.kind === "condition").length;
  const head = `
    <div class="page-head"><div>
      <h1>Passive tree plan</h1>
      <p>The best swap for your build, then the best on the improved tree, step by step. Same point count every step; respecs cost only gold.</p>
    </div></div>
    <div class="toolbar" style="margin-bottom:16px">
      <span class="field-label">Goal</span>
      <div class="seg">${[["damage", "Damage"], ["survival", "Survival"]].map(([v, l]) => `<button class="${state.goal === v ? "on" : ""}" data-goal="${v}">${l}</button>`).join("")}</div>
      <span class="field-label" style="margin-left:8px">${state.goal === "damage" ? "Survival" : "Damage"} loss allowed</span>
      <div class="seg">${[[0, "None"], [5, "5%"], [null, "Any"]].map(([v, l]) => `<button class="${state.maxLoss === v ? "on" : ""}" data-loss="${v ?? "any"}">${l}</button>`).join("")}</div>
      <span class="field-label" style="margin-left:8px">Steps</span>
      <div class="seg">${[3, 5, 8].map((n) => `<button class="${state.steps === n ? "on" : ""}" data-steps="${n}">${n}</button>`).join("")}</div>
      <span class="spacer"></span>
      <span class="faint">${total ? `${keptCount} of ${total} conditions kept · <button class="link" data-view="realistic">change</button>` : ""}</span>
      <button class="btn primary" data-action="plan" ${state.busy ? 'aria-disabled="true"' : ""}>${p ? "Rebuild plan" : "Build plan"}</button>
    </div>`;

  if (busyHere()) {
    const b = state.busy;
    return head + `<div class="panel progress">
      <div class="phase"><strong>${esc(b.detail || "Starting")}</strong><button class="btn small" data-action="cancel">Stop after this step</button></div>
      <div class="track ${b.progress == null ? "indeterminate" : ""}"><i style="${b.progress == null ? "" : `width:${(b.progress * 100).toFixed(1)}%`}"></i></div>
      <div class="note" style="margin-top:12px">Every candidate is screened with Path of Building's own calculator, then the best swaps are applied to your tree and fully recalculated. About 10 to 20 seconds per step.</div>
    </div>`;
  }
  if (!p) {
    return head + `<div class="panel empty">${total && !keptCount ? "No conditions kept: nodes that need one (an enemy Shocked, say) will count as worth nothing. Tick what you sustain under Realistic numbers first, or build the plan as is." : "Build the plan to see the best passive swaps for your build."}</div>`;
  }

  const label = p.usesFullDps ? "Full DPS" : "DPS";
  const end = p.steps.length ? p.steps[p.steps.length - 1].after : p.start;
  const dStart = damage(p, p.start);
  const dEnd = damage(p, end);
  const summary = p.steps.length
    ? `<div class="summary">
        <div class="tile"><div class="label">${label}, realistic</div><div class="value">${fmt(dStart)} <span class="faint" style="font-weight:400">→</span> ${fmt(dEnd)}</div><div class="sub ${tone(pct(dStart, dEnd))}">${signed(pct(dStart, dEnd))} after ${p.steps.length} steps</div></div>
        <div class="tile"><div class="label">Total EHP</div><div class="value">${fmt(end.ehp)}</div><div class="sub ${tone(pct(p.start.ehp, end.ehp))}">${signed(pct(p.start.ehp, end.ehp))}</div></div>
        <div class="tile"><div class="label">Smallest max hit</div><div class="value">${fmt(end.minHit)}</div><div class="sub ${tone(pct(p.start.minHit, end.minHit))}">${signed(pct(p.start.minHit, end.minHit))}</div></div>
      </div>`
    : `<div class="panel empty">No point-neutral swap within reach improves your ${p.goal} by 0.1% or more${p.maxOtherLoss != null ? ` without costing ${p.goal === "damage" ? "survival" : "damage"} beyond the limit you set` : ""}.</div>`;

  const steps = p.steps.map((s, i) => `
    <div class="panel step">
      <div class="n">${i + 1}</div>
      <div class="moves">
        <div class="move"><span class="verb take">Take</span><div class="chips">${chips(s.take, "")}</div></div>
        <div class="move"><span class="verb drop">Drop</span><div class="chips">${chips(s.drop, "drop")}</div></div>
      </div>
      <div class="deltas">
        ${delta(label, pct(damage(p, s.before), damage(p, s.after)))}
        ${delta("EHP", pct(s.before.ehp, s.after.ehp))}
        ${delta("Min hit", pct(s.before.minHit, s.after.minHit))}
      </div>
    </div>`).join("");

  const footer = p.steps.length ? `
    <div class="toolbar" style="margin-top:16px">
      <button class="btn" data-action="copy">${state.copied ? "Copied" : "Copy PoB code of the final tree"}</button>
      <span class="faint">Import it in Path of Building to check every number.</span>
    </div>
    <div class="note">
      Each step was applied to your tree and fully recalculated by Path of Building, then reloaded fresh, as importing it would. No step spends extra points or raises a Path of Building warning.
      ${p.maxOtherLoss === 0 ? ` ${p.goal === "damage" ? "Survival" : "Damage"} never drops below where it started.` : p.maxOtherLoss == null ? ` Any ${p.goal === "damage" ? "survival" : "damage"} loss was allowed.` : ` ${p.goal === "damage" ? "Survival" : "Damage"} stays within ${p.maxOtherLoss}% of where it started.`}
      ${p.keptConditions < p.totalConditions ? ` Judged without the ${p.totalConditions - p.keptConditions} conditions you did not keep.` : ""}
      New path attribute nodes use Path of Building's default attribute; pick what you need in game.
      ${p.cancelled ? " Stopped early on request." : ""}
    </div>` : "";
  return head + summary + `<div class="steps">${steps}</div>` + footer;
}

function chips(nodes, kind) {
  const named = nodes.filter((n) => n.kind === "Notable" || n.kind === "Keystone").map((n) => n.name).sort();
  const small = new Map();
  for (const n of nodes.filter((n) => n.kind !== "Notable" && n.kind !== "Keystone")) {
    const text = n.stats.length ? n.stats.join(", ") : n.name;
    small.set(text, (small.get(text) ?? 0) + 1);
  }
  return [
    ...named.map((name) => `<span class="chip named ${kind}">${esc(name)}</span>`),
    ...[...small].map(([text, count]) => `<span class="chip ${kind}">${count > 1 ? `${count}× ` : ""}${esc(text)}</span>`),
  ].join("") || `<span class="faint">nothing</span>`;
}

function delta(label, change) {
  return `<div class="delta"><div class="label">${esc(label)}</div><div class="v ${tone(change)}">${signed(change)}</div></div>`;
}

// ---------------------------------------------------------------- events

document.addEventListener("click", async (event) => {
  const target = event.target.closest("[data-action],[data-view],[data-goal],[data-steps],[data-loss],[data-slot],[data-trade-goal],[data-open]");
  if (!target || target.getAttribute("aria-disabled") === "true") return;
  if (target.dataset.view) {
    state.view = target.dataset.view;
    state.copied = false;
    state.error = null;
    render();
    document.querySelector(".main")?.scrollTo(0, 0);
    if (state.view === "gear") openGear();
    // What each condition is worth is the page's answer: calculate it on
    // the first visit rather than wait for a click.
    if (state.view === "realistic" && !state.realistic && !state.busy
        && state.overview.assumptions.some((a) => a.kind === "condition")) runRealistic();
    return;
  }
  if (target.dataset.goal) { state.goal = target.dataset.goal; render(); return; }
  if (target.dataset.slot) { measureSlot(target.dataset.slot); return; }
  if (target.dataset.open) {
    invoke("open_link", { url: target.dataset.open }).catch((err) => { state.error = String(err); render(); });
    return;
  }
  if (target.dataset.tradeGoal) { state.trade.goal = target.dataset.tradeGoal; tradeChanged(); render(); return; }
  if (target.dataset.steps) { state.steps = Number(target.dataset.steps); render(); return; }
  if (target.dataset.loss) { state.maxLoss = target.dataset.loss === "any" ? null : Number(target.dataset.loss); render(); return; }
  switch (target.dataset.action) {
    case "install": return installEngine();
    case "update": return installUpdate();
    case "load": {
      const input = document.getElementById("paste").value.trim();
      if (input) return loadBuild(input);
      return;
    }
    case "reset": return resetBuild();
    case "realistic": return runRealistic();
    case "check-item": return checkItem();
    case "open-trade": return openTrade();
    case "copy-trade": return copyTrade();
    case "plan": return runPlan();
    case "cancel":
      target.textContent = "Stopping…";
      target.setAttribute("aria-disabled", "true");
      return invoke("cancel_plan");
    case "copy":
      await navigator.clipboard.writeText(state.plan.code);
      state.copied = true;
      render();
      return;
  }
});

document.addEventListener("input", (event) => {
  const id = event.target.id;
  if (id === "item-paste") state.itemText = event.target.value;
  if (id === "league" || id === "max-price") {
    state.trade[id === "league" ? "league" : "maxPrice"] = event.target.value;
    tradeChanged();
  }
});

document.addEventListener("change", (event) => {
  if (event.target.id === "currency" || event.target.id === "any-base") {
    if (event.target.id === "currency") state.trade.currency = event.target.value;
    else state.trade.anyBase = event.target.checked;
    tradeChanged();
    render();
    return;
  }
  const box = event.target.closest("[data-keep]");
  if (!box) return;
  if (box.checked) state.kept.add(box.dataset.keep);
  else state.kept.delete(box.dataset.keep);
  render();
});

document.addEventListener("keydown", (event) => {
  if (event.key === "Enter" && (event.ctrlKey || event.metaKey) && event.target.id === "paste") {
    document.querySelector('[data-action="load"]')?.click();
  }
  if (event.key === "Enter" && (event.ctrlKey || event.metaKey) && event.target.id === "item-paste") {
    document.querySelector('[data-action="check-item"]')?.click();
  }
});

refreshEngine().catch((err) => { state.error = String(err); render(); });
checkForUpdate().catch(() => {});
render();
