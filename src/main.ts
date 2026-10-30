import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./style.css";

type Limit = { down_bps: number | null; up_bps: number | null };
type Global = { limit: Limit; paused: boolean };
type AppStat = {
  id: string; name: string; path: string; down_bps: number; up_bps: number;
  down_total: number; up_total: number; limit: Limit | null; blocked: boolean;
};
type Unit = "KB/s" | "MB/s";
type View = "live" | "history";

const state = {
  apps: [] as AppStat[], view: "live" as View, query: "", paused: false,
  historyDay: today(), historyApps: [] as AppStat[],
  limits: new Map<string, Limit>(), blocked: new Map<string, boolean>(),
  openEditor: null as string | null,
};

document.body.innerHTML = `
  <main class="app-shell">
    <header class="topbar">
      <a class="brand" href="#" aria-label="Netlume live view">
        <span class="brand-mark" aria-hidden="true"><i></i><i></i><i></i></span><span>Netlume</span>
      </a>
      <nav class="view-switch" aria-label="Primary view">
        <button class="nav-item is-active" data-view="live">Live</button>
        <button class="nav-item" data-view="history">History</button>
      </nav>
      <button class="pause-control" id="pauseControl" role="switch" aria-checked="false">
        <span class="pause-glyph" aria-hidden="true"></span><span id="pauseLabel">Pause limits</span>
      </button>
    </header>

    <section class="live-view" id="liveView">
      <div class="traffic-header">
        <div class="traffic-copy">
          <p class="section-kicker">Traffic now</p><h1>Your network, at a glance.</h1>
          <p class="connection-state"><span></span><b id="appCount">Waiting for data</b></p>
        </div>
        <div class="traffic-totals" aria-label="Current traffic totals">
          <div><span class="direction down" aria-hidden="true">↓</span><p>Download</p><strong id="globalDown">—</strong></div>
          <div><span class="direction up" aria-hidden="true">↑</span><p>Upload</p><strong id="globalUp">—</strong></div>
        </div>
        <div class="global-limit-wrap">
          <button class="limit-summary" id="globalLimitButton" aria-expanded="false" aria-controls="globalLimitPanel">
            <span>Global limit</span><strong id="globalLimitText">Off</strong><i aria-hidden="true"></i>
          </button>
          <div class="global-limit-panel" id="globalLimitPanel" hidden></div>
        </div>
      </div>
      <div class="traffic-rail" aria-hidden="true"><span id="trafficRail"></span></div>

      <section class="app-section" aria-labelledby="appsTitle">
        <div class="section-heading">
          <div><h2 id="appsTitle">Applications</h2><p>Most active first</p></div>
          <label class="search-box">
            <span aria-hidden="true"></span>
            <input id="searchInput" type="search" placeholder="Find an app" autocomplete="off" />
            <kbd>Ctrl K</kbd>
          </label>
        </div>
        <div class="app-table" role="table" aria-label="Application traffic">
          <div class="table-head" role="row">
            <span role="columnheader">Application</span><span role="columnheader">Download</span>
            <span role="columnheader">Upload</span><span role="columnheader">Total</span>
            <span role="columnheader">Limit</span><span role="columnheader">Block</span>
          </div>
          <div id="appRows"></div>
        </div>
      </section>
    </section>

    <section class="history-view" id="historyView" hidden>
      <div class="history-heading">
        <div><p class="section-kicker">Daily record</p><h1>Where your data went.</h1></div>
        <div class="day-control" aria-label="History day">
          <button id="previousDay" aria-label="Previous day">‹</button>
          <div><span>Showing</span><strong id="historyDay"></strong></div>
          <button id="nextDay" aria-label="Next day">›</button>
        </div>
      </div>
      <div class="history-summary" id="historySummary"></div>
      <section class="history-list" aria-labelledby="historyTitle">
        <div class="section-heading"><div><h2 id="historyTitle">Application totals</h2><p id="historyCount">No records</p></div></div>
        <div id="historyRows"></div>
      </section>
    </section>
  </main>
  <div class="toast" id="toast" role="status" aria-live="polite"></div>
`;

const byId = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
const appRows = byId<HTMLDivElement>("appRows");
const historyRows = byId<HTMLDivElement>("historyRows");
const searchInput = byId<HTMLInputElement>("searchInput");

document.querySelectorAll<HTMLButtonElement>("[data-view]").forEach((button) => {
  button.addEventListener("click", () => setView(button.dataset.view as View));
});
document.querySelector<HTMLAnchorElement>(".brand")?.addEventListener("click", (event) => {
  event.preventDefault(); setView("live");
});
searchInput.addEventListener("input", () => {
  state.query = searchInput.value.trim().toLocaleLowerCase(); renderApps();
});
document.addEventListener("keydown", (event) => {
  if (event.ctrlKey && event.key.toLocaleLowerCase() === "k") {
    event.preventDefault(); searchInput.focus();
  }
});
byId<HTMLButtonElement>("pauseControl").addEventListener("click", async () => {
  const previous = state.paused; state.paused = !state.paused; renderPause();
  try {
    await invoke("set_global", { global: { limit: currentGlobalLimit(), paused: state.paused } });
    showToast(state.paused ? "All limits paused" : "Limits resumed");
  } catch {
    state.paused = previous; renderPause(); showToast("Could not change the global state", true);
  }
});
byId<HTMLButtonElement>("globalLimitButton").addEventListener("click", () => {
  const panel = byId<HTMLDivElement>("globalLimitPanel"); panel.hidden = !panel.hidden;
  byId<HTMLButtonElement>("globalLimitButton").ariaExpanded = String(!panel.hidden);
  if (!panel.hidden) renderLimitForm(panel, "global", currentGlobalLimit());
});
byId<HTMLButtonElement>("previousDay").addEventListener("click", () => void changeDay(-1));
byId<HTMLButtonElement>("nextDay").addEventListener("click", () => void changeDay(1));

void listen<AppStat[]>("stats", ({ payload }) => {
  state.apps = payload; renderLiveSummary();
  if (state.openEditor === null) renderApps();
});
void listen<Global>("global", ({ payload }) => {
  state.paused = payload.paused; state.limits.set("global", payload.limit);
  byId("globalLimitText").textContent = formatLimit(payload.limit); renderPause();
});

function setView(view: View) {
  state.view = view;
  byId("liveView").hidden = view !== "live"; byId("historyView").hidden = view !== "history";
  document.querySelectorAll<HTMLButtonElement>("[data-view]").forEach((button) => {
    button.classList.toggle("is-active", button.dataset.view === view);
  });
  if (view === "history") void loadHistory();
}

function renderLiveSummary() {
  const down = state.apps.reduce((sum, app) => sum + app.down_bps, 0);
  const up = state.apps.reduce((sum, app) => sum + app.up_bps, 0);
  byId("globalDown").textContent = formatRate(down); byId("globalUp").textContent = formatRate(up);
  byId("appCount").textContent = `${state.apps.length} applications reporting`;
  byId<HTMLSpanElement>("trafficRail").style.setProperty("--traffic", `${Math.min(94, 18 + Math.log10(Math.max(1, down)) * 10)}%`);
}

function renderApps() {
  const apps = [...state.apps]
    .filter((app) => `${app.name} ${app.path}`.toLocaleLowerCase().includes(state.query))
    .sort((a, b) => b.down_bps + b.up_bps - a.down_bps - a.up_bps);
  appRows.replaceChildren(...apps.map(createAppRow));
  if (!apps.length) {
    const empty = document.createElement("p"); empty.className = "empty-state";
    empty.textContent = state.apps.length ? "No applications match your search." : "Waiting for network activity…";
    appRows.append(empty);
  }
}

function createAppRow(app: AppStat) {
  const wrap = document.createElement("div"); wrap.className = "app-row-wrap";
  const row = document.createElement("div"); row.className = "app-row"; row.setAttribute("role", "row");
  const blocked = state.blocked.get(app.path) ?? app.blocked;
  const limit = state.limits.get(app.path) ?? app.limit;
  row.innerHTML = `
    <div class="app-identity" role="cell"><span class="app-icon"></span><span><strong></strong><small></small></span></div>
    <strong class="metric down-metric" role="cell">${formatRate(app.down_bps)}</strong>
    <strong class="metric" role="cell">${formatRate(app.up_bps)}</strong>
    <span class="metric total" role="cell">${formatBytes(app.down_total + app.up_total)}</span>
    <div role="cell"><button class="limit-button ${limit ? "is-set" : ""}" aria-expanded="${state.openEditor === app.path}">${formatLimit(limit)}</button></div>
    <div role="cell"><button class="block-toggle ${blocked ? "is-blocked" : ""}" role="switch" aria-checked="${blocked}"><i></i><span>${blocked ? "Blocked" : "Allowed"}</span></button></div>`;
  row.querySelector(".app-identity strong")!.textContent = app.name;
  row.querySelector(".app-identity small")!.textContent = compactPath(app.path);
  row.querySelector(".app-icon")!.textContent = initials(app.name);
  row.querySelector<HTMLButtonElement>(".limit-button")!.addEventListener("click", () => {
    state.openEditor = state.openEditor === app.path ? null : app.path; renderApps();
  });
  row.querySelector<HTMLButtonElement>(".block-toggle")!.addEventListener("click", () => void toggleBlock(app, blocked));
  wrap.append(row);
  if (state.openEditor === app.path) {
    const editor = document.createElement("div"); editor.className = "limit-editor";
    renderLimitForm(editor, app.path, limit ?? { down_bps: null, up_bps: null }); wrap.append(editor);
  }
  return wrap;
}

async function toggleBlock(app: AppStat, blocked: boolean) {
  state.blocked.set(app.path, !blocked); renderApps();
  try {
    await invoke("set_block", { path: app.path, blocked: !blocked });
    showToast(!blocked ? `${app.name} blocked` : `${app.name} allowed`);
  } catch {
    state.blocked.set(app.path, blocked); renderApps(); showToast("Could not update the block rule", true);
  }
}

function renderLimitForm(container: HTMLElement, key: string, limit: Limit) {
  const global = key === "global", down = toEditorValue(limit.down_bps), up = toEditorValue(limit.up_bps);
  container.innerHTML = `
    <div class="limit-form-copy"><strong>${global ? "Global speed limit" : "Speed limit"}</strong><span>${global ? "Shared by every application" : "Applied in both directions independently"}</span></div>
    <label><span>Download</span><input class="limit-input" data-direction="down" inputmode="decimal" value="${down.value}" placeholder="Off" aria-label="Download limit" /></label>
    <div class="unit-control" data-unit="down" role="group" aria-label="Download unit"><button class="${down.unit === "KB/s" ? "is-active" : ""}">KB/s</button><button class="${down.unit === "MB/s" ? "is-active" : ""}">MB/s</button></div>
    <label><span>Upload</span><input class="limit-input" data-direction="up" inputmode="decimal" value="${up.value}" placeholder="Off" aria-label="Upload limit" /></label>
    <div class="unit-control" data-unit="up" role="group" aria-label="Upload unit"><button class="${up.unit === "KB/s" ? "is-active" : ""}">KB/s</button><button class="${up.unit === "MB/s" ? "is-active" : ""}">MB/s</button></div>
    <button class="off-button">Turn off</button><button class="save-limit">Save limit</button>`;
  container.querySelectorAll<HTMLDivElement>(".unit-control").forEach((control) => {
    control.addEventListener("click", (event) => {
      const target = (event.target as HTMLElement).closest("button"); if (!target) return;
      control.querySelectorAll("button").forEach((button) => button.classList.toggle("is-active", button === target));
    });
  });
  container.querySelector<HTMLButtonElement>(".off-button")!.addEventListener("click", () => void saveLimit(key, container, true));
  container.querySelector<HTMLButtonElement>(".save-limit")!.addEventListener("click", () => void saveLimit(key, container, false));
}

async function saveLimit(key: string, container: HTMLElement, turnOff: boolean) {
  const limit: Limit = turnOff ? { down_bps: null, up_bps: null } : {
    down_bps: readLimit(container, "down"), up_bps: readLimit(container, "up"),
  };
  if (!turnOff && limit.down_bps === null && limit.up_bps === null) {
    showToast("Enter at least one speed, or turn the limit off", true); return;
  }
  try {
    if (key === "global") {
      state.limits.set("global", limit);
      await invoke("set_global", { global: { limit, paused: state.paused } });
      byId("globalLimitText").textContent = formatLimit(limit);
      byId<HTMLDivElement>("globalLimitPanel").hidden = true;
      byId<HTMLButtonElement>("globalLimitButton").ariaExpanded = "false";
    } else {
      state.limits.set(key, limit); await invoke("set_limit", { path: key, limit });
      state.openEditor = null; renderApps();
    }
    showToast(turnOff ? "Limit turned off" : "Limit saved");
  } catch { showToast("Could not save the limit", true); }
}

function readLimit(container: HTMLElement, direction: "down" | "up") {
  const raw = container.querySelector<HTMLInputElement>(`[data-direction="${direction}"]`)!.value.replace(",", ".");
  const value = Number(raw); if (!Number.isFinite(value) || value <= 0) return null;
  const unit = container.querySelector<HTMLDivElement>(`[data-unit="${direction}"]`)!.querySelector<HTMLButtonElement>(".is-active")!.textContent as Unit;
  return Math.round(value * (unit === "MB/s" ? 1_000_000 : 1_000));
}

function currentGlobalLimit(): Limit { return state.limits.get("global") ?? { down_bps: null, up_bps: null }; }
function renderPause() {
  const button = byId<HTMLButtonElement>("pauseControl"); button.ariaChecked = String(state.paused);
  button.classList.toggle("is-paused", state.paused);
  byId("pauseLabel").textContent = state.paused ? "Resume limits" : "Pause limits";
}

async function changeDay(offset: number) {
  const day = new Date(`${state.historyDay}T12:00:00`); day.setDate(day.getDate() + offset);
  const next = isoDay(day); if (next > today()) return; state.historyDay = next; await loadHistory();
}

async function loadHistory() {
  byId("historyDay").textContent = formatDay(state.historyDay);
  byId<HTMLButtonElement>("nextDay").disabled = state.historyDay >= today();
  historyRows.innerHTML = `<p class="empty-state">Loading daily totals…</p>`;
  try { state.historyApps = await invoke<AppStat[]>("history", { day: state.historyDay }); renderHistory(); }
  catch { historyRows.innerHTML = `<p class="empty-state is-error">History could not be loaded.</p>`; }
}

function renderHistory() {
  const down = state.historyApps.reduce((sum, app) => sum + app.down_total, 0);
  const up = state.historyApps.reduce((sum, app) => sum + app.up_total, 0);
  byId("historySummary").innerHTML = `<div><span>Downloaded</span><strong>${formatBytes(down)}</strong></div><div><span>Uploaded</span><strong>${formatBytes(up)}</strong></div><div><span>Total traffic</span><strong>${formatBytes(down + up)}</strong></div>`;
  byId("historyCount").textContent = `${state.historyApps.length} applications recorded`;
  historyRows.replaceChildren(...[...state.historyApps].sort((a, b) => b.down_total + b.up_total - a.down_total - a.up_total).map((app) => {
    const row = document.createElement("div"); row.className = "history-row";
    row.innerHTML = `<span class="app-icon"></span><span class="history-app"><strong></strong><small></small></span><span><small>Downloaded</small><strong>${formatBytes(app.down_total)}</strong></span><span><small>Uploaded</small><strong>${formatBytes(app.up_total)}</strong></span>`;
    row.querySelector(".history-app strong")!.textContent = app.name;
    row.querySelector(".history-app small")!.textContent = compactPath(app.path);
    row.querySelector(".app-icon")!.textContent = initials(app.name);
    return row;
  }));
}

let toastTimer = 0;
function showToast(message: string, error = false) {
  const toast = byId<HTMLDivElement>("toast"); toast.textContent = message;
  toast.classList.toggle("is-error", error); toast.classList.add("is-visible");
  window.clearTimeout(toastTimer); toastTimer = window.setTimeout(() => toast.classList.remove("is-visible"), 2600);
}
function formatRate(bytes: number) {
  return bytes >= 1_000_000 ? `${(bytes / 1_000_000).toFixed(bytes >= 10_000_000 ? 1 : 2)} MB/s` : `${Math.round(bytes / 1_000)} KB/s`;
}
function formatBytes(bytes: number) {
  if (bytes >= 1_000_000_000) return `${(bytes / 1_000_000_000).toFixed(2)} GB`;
  return bytes >= 1_000_000 ? `${(bytes / 1_000_000).toFixed(1)} MB` : `${Math.round(bytes / 1_000)} KB`;
}
function formatLimit(limit: Limit | null) {
  if (!limit || (!limit.down_bps && !limit.up_bps)) return "Off";
  if (limit.down_bps && limit.up_bps && limit.down_bps === limit.up_bps) return formatRate(limit.down_bps);
  return `${limit.down_bps ? `↓ ${formatRate(limit.down_bps)}` : "↓ Off"} · ${limit.up_bps ? `↑ ${formatRate(limit.up_bps)}` : "↑ Off"}`;
}
function toEditorValue(value: number | null): { value: string; unit: Unit } {
  if (!value) return { value: "", unit: "MB/s" };
  return value >= 1_000_000 ? { value: String(value / 1_000_000), unit: "MB/s" } : { value: String(value / 1_000), unit: "KB/s" };
}
function compactPath(path: string) {
  const parts = path.split("\\"); return parts.length > 2 ? `…\\${parts.slice(-2).join("\\")}` : path;
}
function initials(name: string) { return name.split(/\s+/).slice(0, 2).map((part) => part[0]).join("").toLocaleUpperCase(); }
function today() { return isoDay(new Date()); }
function isoDay(day: Date) { return `${day.getFullYear()}-${String(day.getMonth() + 1).padStart(2, "0")}-${String(day.getDate()).padStart(2, "0")}`; }
function formatDay(day: string) {
  return new Intl.DateTimeFormat(undefined, { weekday: "short", month: "short", day: "numeric" }).format(new Date(`${day}T12:00:00`));
}
