// Build Doctor interface. Every number shown comes from the Rust side, which
// gets it from Path of Building; this file only lays it out.

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const state = {
  engine: null,          // { installed, pinned }
  busy: null,            // { title, detail, progress: 0..1 | null }
  error: null,
  view: "overview",      // overview | realistic | plan
  overview: null,
  kept: new Set(),       // condition vars the player keeps up
  realistic: null,
  realisticKept: null,   // kept set the realistic result was computed for
  goal: "damage",
  steps: 5,
  plan: null,
  planProgress: null,
  copied: false,
};

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

// ---------------------------------------------------------------- actions

async function run(title, detail, task) {
  state.busy = { title, detail, progress: null };
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
  state.kept = new Set();
  state.realistic = null;
  state.realisticKept = null;
  state.plan = null;
  state.view = "overview";
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
  const result = await run("Building your passive tree plan", "", () => invoke("plan", { kept: [...state.kept], goal: state.goal, steps: state.steps }));
  state.planProgress = null;
  if (result) state.plan = result;
  render();
}

function resetBuild() {
  state.overview = null;
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
      <div class="wordmark">Build <span>Doctor</span></div>
      <h1>Load a build</h1>
      <p>Paste a Path of Building code, or a pobb.in, poe.ninja, maxroll or poe2db link.</p>
      <textarea class="paste" id="paste" spellcheck="false" placeholder="Paste here" ${state.busy || !ready ? "disabled" : ""}></textarea>
      <div class="start-actions">
        <span class="engine-line">${engineLine}</span>
        ${ready
          ? `<button class="btn primary" data-action="load" ${state.busy ? 'aria-disabled="true"' : ""}>Analyse build</button>`
          : `<button class="btn primary" data-action="install" ${state.busy || !e ? 'aria-disabled="true"' : ""}>Install Path of Building</button>`}
      </div>
      ${state.busy ? renderBusy() : ""}
      ${state.error ? `<div class="error">${esc(state.error)}</div>` : ""}
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
  const nav = [
    ["overview", "Overview", ""],
    ["realistic", "Realistic numbers", conditions ? `${conditions}` : ""],
    ["plan", "Passive tree plan", state.plan ? `${state.plan.steps.length} steps` : ""],
  ];
  return `
    <aside class="rail">
      <div class="wordmark">Build <span>Doctor</span></div>
      <div class="build-id">
        <div class="asc">${esc(b.ascendancy ?? b.class)}</div>
        <div class="meta">${b.ascendancy ? esc(b.class) + " · " : ""}Level ${esc(b.level ?? "–")}</div>
        <div class="meta">${esc(b.mainSkill ?? "")}</div>
      </div>
      <nav class="nav">
        ${nav.map(([id, label, note]) => `<button class="${state.view === id ? "active" : ""}" data-view="${id}">${label}<small>${esc(note)}</small></button>`).join("")}
      </nav>
      <div class="rail-foot">
        <span class="engine">Path of Building ${esc(b.pobVersion)} · tree ${esc(b.treeVersion ?? "–")}</span>
        <button class="link" data-action="reset">Load another build</button>
      </div>
    </aside>`;
}

function renderView() {
  const busy = state.busy ? renderBusy() : "";
  const error = state.error ? `<div class="error">${esc(state.error)}</div>` : "";
  if (state.view === "realistic") return renderRealistic() + busy + error;
  if (state.view === "plan") return renderPlan() + error;
  return renderOverview() + error;
}

function stat(group, name) {
  for (const g of state.overview.groups) for (const r of g.rows) if (r.stat === name) return r.value;
  return null;
}

function renderOverview() {
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
      : `<div class="verdict warn"><strong>${r.differences.length} of ${r.saved} saved numbers differ</strong><span class="muted">The build was probably saved by a different Path of Building version. The numbers below are ${esc(o.build.pobVersion)}'s.</span></div>`;
  const outdated = o.build.treeOutdated
    ? `<div class="verdict warn"><strong>Old passive tree</strong><span class="muted">This build uses tree ${esc(o.build.treeVersion)}; the newest is ${esc(o.build.latestTreeVersion)}. Numbers describe the older game version.</span></div>`
    : "";

  const conditions = o.assumptions.filter((a) => a.kind === "condition");
  const scenario = o.assumptions.filter((a) => a.kind === "scenario" && a.section !== "Quest Rewards");
  const quests = o.assumptions.filter((a) => a.section === "Quest Rewards");
  const gear = o.unsupported.filter((m) => !m.usable);
  const usables = o.unsupported.filter((m) => m.usable);

  return `
    <div class="page-head"><div>
      <h1>Overview</h1>
      <p>Your build as Path of Building calculates it, with everything those numbers depend on.</p>
    </div></div>
    ${verdict}${outdated}
    <div class="tiles">
      ${tile(full ? "Full DPS" : "Combined DPS", fmt(dmg), o.minionDps ? `Minions ${fmt(o.minionDps)}` : "Main skill")}
      ${tile("Total EHP", fmt(stat("Pools", "TotalEHP")), "Effective hit pool")}
      ${tile("Smallest max hit", smallest ? fmt(smallest.value) : "–", smallest ? `${smallest.label} damage` : "")}
      ${tile(es && es > (life ?? 0) ? "Energy shield" : "Life", fmt(es && es > (life ?? 0) ? es : life), es && life ? `Life ${fmt(life)} · ES ${fmt(es)}` : "")}
    </div>
    ${o.warnings.length ? `<div class="verdict warn"><strong>Path of Building warns</strong><span class="muted">${o.warnings.map(esc).join(" · ")}</span></div>` : ""}
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
      <p>Each condition the build assumes, switched off one at a time by Path of Building. Tick the ones you really keep up on a boss; the plan uses the same choice.</p>
    </div>
    <button class="btn primary" data-action="realistic" ${state.busy || !conditions.length ? 'aria-disabled="true"' : ""}>${r ? (stale ? "Recalculate" : "Recalculate") : "Calculate"}</button></div>`;
  if (!conditions.length) return head + `<div class="panel empty">This build assumes no combat conditions, so its numbers already are the realistic ones.</div>`;
  if (!r) {
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
      ${stale ? `<div class="note">Your ticks changed since this was calculated. Recalculate to update “Keeping yours”.</div>` : ""}
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
      <span class="field-label" style="margin-left:8px">Steps</span>
      <div class="seg">${[3, 5, 8].map((n) => `<button class="${state.steps === n ? "on" : ""}" data-steps="${n}">${n}</button>`).join("")}</div>
      <span class="spacer"></span>
      <span class="faint">${total ? `${keptCount} of ${total} conditions kept · <button class="link" data-view="realistic">change</button>` : ""}</span>
      <button class="btn primary" data-action="plan" ${state.busy ? 'aria-disabled="true"' : ""}>${p ? "Rebuild plan" : "Build plan"}</button>
    </div>`;

  if (state.busy) {
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
    : `<div class="panel empty">No point-neutral swap within reach improves your ${p.goal} by 0.1% or more. Your tree is already locally optimal for this goal.</div>`;

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
  const target = event.target.closest("[data-action],[data-view],[data-goal],[data-steps]");
  if (!target || target.getAttribute("aria-disabled") === "true") return;
  if (target.dataset.view) {
    state.view = target.dataset.view;
    state.copied = false;
    render();
    document.querySelector(".main")?.scrollTo(0, 0);
    return;
  }
  if (target.dataset.goal) { state.goal = target.dataset.goal; render(); return; }
  if (target.dataset.steps) { state.steps = Number(target.dataset.steps); render(); return; }
  switch (target.dataset.action) {
    case "install": return installEngine();
    case "load": {
      const input = document.getElementById("paste").value.trim();
      if (input) return loadBuild(input);
      return;
    }
    case "reset": return resetBuild();
    case "realistic": return runRealistic();
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

document.addEventListener("change", (event) => {
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
});

refreshEngine().catch((err) => { state.error = String(err); render(); });
render();
