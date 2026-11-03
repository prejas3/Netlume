/// <reference types="vite/client" />
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getVersion } from "@tauri-apps/api/app";
import { getCurrentWindow } from "@tauri-apps/api/window";
import appIcon from "../src-tauri/icons/icon.svg";
import { icon, type IconName } from "./icons";
import { dialog, select, tooltips } from "./sj-ui.js";
import "./sj-ui.css";
import "./style.css";

type Limit = { down_bps: number | null; up_bps: number | null };
type Global = { limit: Limit; paused: boolean };
type AppStat = {
  id: string; name: string; path: string; down_bps: number; up_bps: number;
  down_total: number; up_total: number; limit: Limit | null; blocked: boolean;
};
type Page = "live" | "history" | "settings" | "about";
type Lang = "en" | "pl";
type Theme = "system" | "light" | "dark";

const en = {
  live: "Live", history: "History", settings: "Settings", about: "About", menu: "Navigation",
  apps: (n: number) => `${n} ${n === 1 ? "application" : "applications"}`,
  globalOff: "no global limit", globalOn: (v: string) => `global limit ${v}`,
  pause: "Pause limits", resume: "Resume limits",
  pausedTitle: "Limits are paused.", pausedText: "Traffic is unrestricted until you resume them.",
  search: "Find an application", app: "Application", down: "Download", up: "Upload", total: "Total",
  limit: "Limit", block: "Blocked", blockApp: (n: string) => `Block ${n}`, limitApp: (n: string) => `Limit for ${n}`,
  waiting: "Waiting for network activity.", noMatch: "No application matches the search.",
  off: "Off", on: "On", downloaded: "downloaded", uploaded: "uploaded",
  prevDay: "Previous day", nextDay: "Next day", noRecords: "No traffic recorded on this day.",
  loading: "Loading…", historyError: "History could not be loaded. Try another day or restart Netlume.",
  general: "General", startup: "Start with Windows", startupText: "Start in the tray when you sign in, without a UAC prompt",
  language: "Language", languageText: "Language of the window", theme: "Theme", themeText: "Follow Windows or choose one",
  themes: { system: "Use system setting", light: "Light", dark: "Dark" },
  limits: "Limits", globalLimit: "Global limit", globalText: "Shared by every application",
  edit: "Edit", pauseAll: "Pause all limits", pauseText: "Lift every limit and block until you resume; resets on restart",
  advanced: "Advanced", dataFolder: "Data folder", openFolder: "Open folder",
  version: (v: string) => `Version ${v}`, aboutText: "Per-application network monitor and limiter for Windows.",
  projectPage: "Project page", source: "Source code", issues: "Report a problem", licence: "MIT licence",
  limitTitle: (n: string) => `Speed limit for ${n}`, globalTitle: "Global speed limit",
  limitText: "Leave a direction empty to keep it unlimited.", save: "Save limit", turnOff: "Turn off", cancel: "Cancel",
  needValue: "Enter a speed greater than 0 in at least one direction, or turn the limit off.",
  saveError: "The change was not saved.", saveErrorText: "Netlume could not write its settings. Try again.",
  autostartError: "Start with Windows was not changed.", autostartErrorText: "Windows refused the scheduled task. Run Netlume as administrator and try again.",
  openError: "The link could not be opened.", dismiss: "Close",
};
const pl: typeof en = {
  live: "Na żywo", history: "Historia", settings: "Ustawienia", about: "O aplikacji", menu: "Nawigacja",
  apps: (n) => `${n} ${n === 1 ? "aplikacja" : n % 10 >= 2 && n % 10 <= 4 && (n % 100 < 12 || n % 100 > 14) ? "aplikacje" : "aplikacji"}`,
  globalOff: "bez limitu globalnego", globalOn: (v) => `limit globalny ${v}`,
  pause: "Wstrzymaj limity", resume: "Wznów limity",
  pausedTitle: "Limity są wstrzymane.", pausedText: "Ruch nie jest ograniczany, dopóki ich nie wznowisz.",
  search: "Znajdź aplikację", app: "Aplikacja", down: "Pobieranie", up: "Wysyłanie", total: "Razem",
  limit: "Limit", block: "Blokada", blockApp: (n) => `Zablokuj ${n}`, limitApp: (n) => `Limit dla ${n}`,
  waiting: "Czekam na ruch sieciowy.", noMatch: "Żadna aplikacja nie pasuje do wyszukiwania.",
  off: "Wył.", on: "Wł.", downloaded: "pobrano", uploaded: "wysłano",
  prevDay: "Poprzedni dzień", nextDay: "Następny dzień", noRecords: "Tego dnia nie zapisano ruchu.",
  loading: "Wczytywanie…", historyError: "Nie udało się wczytać historii. Wybierz inny dzień albo uruchom Netlume ponownie.",
  general: "Ogólne", startup: "Uruchamiaj z Windows", startupText: "Startuje w zasobniku po zalogowaniu, bez pytania UAC",
  language: "Język", languageText: "Język okna", theme: "Motyw", themeText: "Jak w Windows albo wybrany",
  themes: { system: "Jak w systemie", light: "Jasny", dark: "Ciemny" },
  limits: "Limity", globalLimit: "Limit globalny", globalText: "Wspólny dla wszystkich aplikacji",
  edit: "Zmień", pauseAll: "Wstrzymaj wszystkie limity", pauseText: "Znosi limity i blokady do wznowienia; wraca po ponownym uruchomieniu",
  advanced: "Zaawansowane", dataFolder: "Folder danych", openFolder: "Otwórz folder",
  version: (v) => `Wersja ${v}`, aboutText: "Monitor i limiter ruchu sieciowego dla poszczególnych aplikacji w Windows.",
  projectPage: "Strona projektu", source: "Kod źródłowy", issues: "Zgłoś problem", licence: "Licencja MIT",
  limitTitle: (n) => `Limit prędkości dla ${n}`, globalTitle: "Globalny limit prędkości",
  limitText: "Zostaw kierunek pusty, aby go nie ograniczać.", save: "Zapisz limit", turnOff: "Wyłącz", cancel: "Anuluj",
  needValue: "Wpisz prędkość większą od 0 w co najmniej jednym kierunku albo wyłącz limit.",
  saveError: "Zmiana nie została zapisana.", saveErrorText: "Netlume nie mógł zapisać ustawień. Spróbuj ponownie.",
  autostartError: "Nie zmieniono uruchamiania z Windows.", autostartErrorText: "Windows odrzucił zadanie harmonogramu. Uruchom Netlume jako administrator i spróbuj ponownie.",
  openError: "Nie udało się otworzyć linku.", dismiss: "Zamknij",
};

const stored = (key: string) => { try { return localStorage.getItem(key); } catch { return null; } };
const store = (key: string, value: string) => { try { localStorage.setItem(key, value); } catch { /* preference only */ } };

const state = {
  page: "live" as Page, query: "", apps: [] as AppStat[], global: { limit: { down_bps: null, up_bps: null }, paused: false } as Global,
  historyDay: today(), historyApps: null as AppStat[] | null | "error", autostart: false, version: "",
  lang: (stored("lang") ?? (navigator.language.toLowerCase().startsWith("pl") ? "pl" : "en")) as Lang,
  theme: (stored("theme") ?? "system") as Theme,
};
let t = state.lang === "pl" ? pl : en;
const rows = new Map<string, HTMLDivElement>();
let barBox: HTMLDivElement;
const byId = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;
const DOWN = "↓︎", UP = "↑︎"; // text arrows, not the emoji font
const SITE = "https://szymonjankiewicz.com", REPO = "https://github.com/prejas3/Netlume";

const compact = matchMedia("(width < 640px)");
compact.addEventListener("change", navTips);
tooltips();
applyTheme();
void (navigator as any).userAgentData?.getHighEntropyValues(["platformVersion"]).then((ua: { platformVersion: string }) => {
  document.documentElement.classList.toggle("mica", parseInt(ua.platformVersion) >= 13); // Windows 11
});
void getVersion().then((v) => { state.version = v; if (state.page === "about") renderAbout(); });
void invoke<boolean>("get_autostart").then((on) => { state.autostart = on; if (state.page === "settings") renderSettings(); });
build();
// The saved choice lives in the app (rules.json) so the tray reads it too; localStorage only avoids a flash.
void invoke<Lang>("get_language").then((lang) => { if (lang !== state.lang) setLang(lang, false); }).catch(() => {});

void listen<AppStat[]>("stats", ({ payload }) => { state.apps = payload; renderLive(); });
void listen<Global>("global", ({ payload }) => { state.global = payload; renderGlobal(); });
void invoke<Global>("get_global").then((g) => { state.global = g; renderGlobal(); })
  .catch(() => infoBar("err", t.saveError, t.saveErrorText));

document.addEventListener("keydown", (e) => {
  if (e.ctrlKey && e.key.toLowerCase() === "k") { e.preventDefault(); setPage("live"); byId("search").focus(); }
  if (e.key === "Escape") byId("nav").classList.remove("open");
});

function build() {
  document.documentElement.lang = state.lang;
  rows.clear();
  barBox = document.createElement("div"); barBox.className = "bars";
  const navItem = (page: Page, glyph: IconName) =>
    `<button class="nav-item" data-page="${page}" aria-label="${t[page]}">${icon(glyph).replace('class="icon"', 'class="icon off"')}${icon(`${glyph}On` as IconName).replace('class="icon"', 'class="icon on"')}<span>${t[page]}</span></button>`;
  document.body.innerHTML = `
    <nav class="nav" id="nav" aria-label="Netlume">
      <button class="btn subtle square menu-btn" id="menuBtn" aria-label="${t.menu}" data-tip="${t.menu}" aria-expanded="false">${icon("menu")}</button>
      ${navItem("live", "live")}${navItem("history", "history")}
      <div class="nav-spacer"></div>
      ${navItem("settings", "settings")}${navItem("about", "about")}
    </nav>
    <main class="content">
      <div class="page">
        <section id="live">
          <header class="page-head"><div><h1 class="t-title">${t.live}</h1><p class="t-2 num" id="liveStatus"></p></div>
            <button class="btn" id="pauseBtn"></button></header>
          <label class="field search">${icon("search")}<input id="search" type="search" placeholder="${t.search}" aria-label="${t.search}" autocomplete="off" spellcheck="false"><kbd>Ctrl K</kbd></label>
          <div class="list apps" role="table" aria-label="${t.live}">
            <div class="list-row list-head" role="row"><span class="name" role="columnheader">${t.app}</span><span role="columnheader">${t.down}</span><span role="columnheader">${t.up}</span><span role="columnheader">${t.total}</span><span role="columnheader">${t.limit}</span><span role="columnheader">${t.block}</span></div>
            <div id="appRows" role="rowgroup"></div>
          </div>
          <p class="empty t-2" id="liveEmpty"></p>
        </section>
        <section id="history" hidden>
          <header class="page-head"><div><h1 class="t-title">${t.history}</h1><p class="t-2 num" id="historyStatus"></p></div>
            <div class="day"><button class="btn subtle square" id="prevDay" aria-label="${t.prevDay}" data-tip="${t.prevDay}">${icon("prev")}</button>
            <span class="num" id="dayLabel"></span>
            <button class="btn subtle square" id="nextDay" aria-label="${t.nextDay}" data-tip="${t.nextDay}">${icon("next")}</button></div></header>
          <div class="list history" role="table" aria-label="${t.history}">
            <div class="list-row list-head" role="row"><span class="name" role="columnheader">${t.app}</span><span role="columnheader">${t.down}</span><span role="columnheader">${t.up}</span><span role="columnheader">${t.total}</span></div>
            <div id="historyRows" role="rowgroup"></div>
          </div>
          <p class="empty t-2" id="historyEmpty"></p>
        </section>
        <section id="settings" hidden></section>
        <section id="about" hidden></section>
      </div>
    </main>`;
  document.querySelectorAll<HTMLButtonElement>("[data-page]").forEach((b) =>
    b.addEventListener("click", () => { setPage(b.dataset.page as Page); byId("nav").classList.remove("open"); }));
  byId("menuBtn").addEventListener("click", () => {
    const open = byId("nav").classList.toggle("open"); byId("menuBtn").ariaExpanded = String(open);
  });
  byId<HTMLInputElement>("search").addEventListener("input", (e) => {
    state.query = (e.target as HTMLInputElement).value.trim().toLocaleLowerCase(); renderLive(true);
  });
  byId("pauseBtn").addEventListener("click", () => void setPaused(!state.global.paused));
  byId("prevDay").addEventListener("click", () => void changeDay(-1));
  byId("nextDay").addEventListener("click", () => void changeDay(1));
  navTips(); setPage(state.page); renderGlobal(); renderLive(true);
}

function navTips() {
  document.querySelectorAll<HTMLButtonElement>(".nav-item").forEach((b) => {
    if (compact.matches) b.dataset.tip = b.ariaLabel ?? ""; else delete b.dataset.tip;
  });
}

function setPage(page: Page) {
  state.page = page;
  for (const p of ["live", "history", "settings", "about"] as Page[]) byId(p).hidden = p !== page;
  document.querySelectorAll<HTMLButtonElement>("[data-page]").forEach((b) => {
    if (b.dataset.page === page) b.setAttribute("aria-current", "page"); else b.removeAttribute("aria-current");
  });
  if (page === "history") void loadHistory();
  if (page === "settings") renderSettings();
  if (page === "about") renderAbout();
  bars();
  document.querySelector(".content")!.scrollTop = 0;
}

// ---- InfoBar (replaces toasts) ----
function infoBar(kind: "ok" | "warn" | "err" | "info", title: string, text: string, action?: { label: string; run: () => void }, id?: string) {
  if (id) barBox.querySelector(`#${id}`)?.remove();
  const bar = document.createElement("div");
  bar.className = `infobar ${kind}`; bar.setAttribute("role", kind === "err" ? "alert" : "status");
  if (id) bar.id = id;
  bar.innerHTML = `${icon(kind === "ok" ? "ok" : kind === "warn" ? "warn" : kind === "err" ? "err" : "info")}<p class="infobar-text"><b></b><span></span></p>`;
  bar.querySelector("b")!.textContent = title; bar.querySelector(".infobar-text span")!.textContent = text;
  if (action) {
    const b = document.createElement("button"); b.className = "btn"; b.textContent = action.label;
    b.addEventListener("click", action.run); bar.append(b);
  }
  if (!id) {
    const close = document.createElement("button");
    close.className = "btn subtle square"; close.ariaLabel = t.dismiss; close.dataset.tip = t.dismiss; close.innerHTML = icon("dismiss");
    close.addEventListener("click", () => bar.remove()); bar.append(close);
  }
  bars().append(bar);
}

/** The InfoBar stack, kept right under the current page's header. */
function bars() {
  const head = byId(state.page).querySelector(".page-head, .about-head");
  if (head && head.nextElementSibling !== barBox) head.after(barBox);
  return barBox;
}

// ---- Live ----
function renderGlobal() {
  const paused = state.global.paused;
  byId("pauseBtn").innerHTML = `${icon("pause")}<span>${paused ? t.resume : t.pause}</span>`;
  if (paused) infoBar("warn", t.pausedTitle, t.pausedText, undefined, "pausedBar");
  else barBox.querySelector("#pausedBar")?.remove();
  renderLive();
  if (state.page === "settings") renderSettings();
}

function renderLive(reorder = false) {
  const down = state.apps.reduce((s, a) => s + a.down_bps, 0), up = state.apps.reduce((s, a) => s + a.up_bps, 0);
  const g = state.global.limit;
  byId("liveStatus").textContent = `${t.apps(state.apps.length)} · ${DOWN} ${formatRate(down)} · ${UP} ${formatRate(up)} · ${hasLimit(g) ? t.globalOn(formatLimit(g)) : t.globalOff}`;
  const list = byId("appRows");
  const apps = state.apps.filter((a) => `${a.name} ${a.path}`.toLocaleLowerCase().includes(state.query))
    .sort((a, b) => b.down_bps + b.up_bps - a.down_bps - a.up_bps);
  const live = new Set(apps.map((a) => a.path));
  for (const [path, row] of rows) if (!live.has(path)) { row.remove(); rows.delete(path); }
  for (const app of apps) updateRow(rows.get(app.path) ?? createRow(app), app);
  // Hold the order while the pointer or focus is in the list, so rows don't move under the user.
  const busy = list.matches(":hover") || list.contains(document.activeElement);
  const order = apps.map((a) => rows.get(a.path)!);
  if (reorder || !busy) { if (order.some((row, i) => list.children[i] !== row)) list.replaceChildren(...order); }
  else for (const row of order) if (!row.isConnected) list.append(row);
  byId("liveEmpty").textContent = apps.length ? "" : state.apps.length ? t.noMatch : t.waiting;
}

function createRow(app: AppStat) {
  const row = document.createElement("div");
  row.className = "list-row"; row.setAttribute("role", "row");
  row.innerHTML = `<div class="name" role="cell"><span></span><span class="t-caption t-2"></span></div>
    <span role="cell" class="down"></span><span role="cell" class="up"></span><span role="cell" class="total t-2"></span>
    <span role="cell" class="lim"><button class="btn subtle"></button></span>
    <span role="cell" class="blk"><button class="toggle" role="switch"><i></i></button></span>`;
  row.querySelector(".name span")!.textContent = app.name;
  row.querySelector(".name .t-caption")!.textContent = compactPath(app.path);
  (row.querySelector(".name") as HTMLElement).dataset.tip = app.path;
  const limit = row.querySelector<HTMLButtonElement>(".lim button")!, block = row.querySelector<HTMLButtonElement>(".toggle")!;
  limit.ariaLabel = t.limitApp(app.name); block.ariaLabel = t.blockApp(app.name);
  limit.addEventListener("click", () => limitDialog(app.path, app.name));
  block.addEventListener("click", () => void toggleBlock(app.path, block));
  rows.set(app.path, row);
  return row;
}

function updateRow(row: HTMLDivElement, app: AppStat) {
  row.querySelector(".down")!.textContent = formatRate(app.down_bps);
  row.querySelector(".up")!.textContent = formatRate(app.up_bps);
  row.querySelector(".total")!.textContent = formatBytes(app.down_total + app.up_total);
  const limit = row.querySelector<HTMLButtonElement>(".lim button")!;
  limit.textContent = formatLimit(app.limit); limit.classList.toggle("is-set", hasLimit(app.limit));
  const block = row.querySelector<HTMLButtonElement>(".toggle")!;
  if (!block.dataset.pending) block.ariaChecked = String(app.blocked);
}

async function toggleBlock(path: string, button: HTMLButtonElement) {
  const blocked = button.ariaChecked !== "true";
  button.ariaChecked = String(blocked); button.dataset.pending = "1";
  try { await invoke("set_block", { path, blocked }); const app = state.apps.find((a) => a.path === path); if (app) app.blocked = blocked; }
  catch { button.ariaChecked = String(!blocked); infoBar("err", t.saveError, t.saveErrorText); }
  finally { delete button.dataset.pending; }
}

async function setPaused(paused: boolean) {
  const previous = state.global; state.global = { ...previous, paused }; renderGlobal();
  try { await invoke("set_global", { global: state.global }); }
  catch { state.global = previous; renderGlobal(); infoBar("err", t.saveError, t.saveErrorText); }
}

function limitDialog(key: string, name: string) {
  const current = key === "global" ? state.global.limit : state.apps.find((a) => a.path === key)?.limit ?? null;
  const body = document.createElement("div");
  body.className = "limit-form";
  body.innerHTML = `<p class="t-2">${key === "global" ? t.globalText + ". " : ""}${t.limitText}</p>`;
  const fields = (["down", "up"] as const).map((dir) => {
    const bps = current?.[`${dir}_bps`] ?? null;
    const mb = !bps || bps >= 1_000_000;
    const row = document.createElement("div"); row.className = "limit-row";
    row.innerHTML = `<label for="lim-${dir}">${dir === "down" ? t.down : t.up}</label><span class="field"><input id="lim-${dir}" inputmode="decimal" autocomplete="off" placeholder="${t.off}" class="num"></span>`;
    const input = row.querySelector("input")!;
    input.value = bps ? String(bps / (mb ? 1_000_000 : 1_000)) : "";
    const unit = select({ options: [{ value: "KB/s", label: "KB/s" }, { value: "MB/s", label: "MB/s" }], value: mb ? "MB/s" : "KB/s", label: `${dir === "down" ? t.down : t.up}: ${t.limit}`, onChange: () => {} });
    row.append(unit); body.append(row);
    return () => {
      const value = Number(input.value.replace(",", "."));
      return input.value.trim() && Number.isFinite(value) && value > 0 ? Math.round(value * (unit.value === "MB/s" ? 1_000_000 : 1_000)) : null;
    };
  });
  const error = document.createElement("p"); error.className = "form-error"; error.setAttribute("role", "alert"); body.append(error);
  const save = async (limit: Limit) => {
    try {
      if (key === "global") { await invoke("set_global", { global: { ...state.global, limit } }); state.global = { ...state.global, limit }; renderGlobal(); }
      else { await invoke("set_limit", { path: key, limit }); const app = state.apps.find((a) => a.path === key); if (app) app.limit = hasLimit(limit) ? limit : null; renderLive(); }
    } catch { infoBar("err", t.saveError, t.saveErrorText); }
  };
  dialog({
    title: key === "global" ? t.globalTitle : t.limitTitle(name), body,
    actions: [
      { label: t.save, accent: true, run: async () => {
        const limit = { down_bps: fields[0](), up_bps: fields[1]() };
        if (!hasLimit(limit)) { error.textContent = t.needValue; return false; }
        await save(limit);
      } },
      { label: t.turnOff, run: () => save({ down_bps: null, up_bps: null }) },
      { label: t.cancel },
    ],
  });
}

// ---- History ----
async function changeDay(offset: number) {
  const day = new Date(`${state.historyDay}T12:00:00`); day.setDate(day.getDate() + offset);
  const next = isoDay(day); if (next > today()) return; state.historyDay = next; await loadHistory();
}

async function loadHistory() {
  const day = state.historyDay;
  state.historyApps = null; renderHistory();
  try { const apps = await invoke<AppStat[]>("history", { day }); if (day === state.historyDay) state.historyApps = apps; }
  catch { state.historyApps = "error"; }
  renderHistory();
}

function renderHistory() {
  byId("dayLabel").textContent = new Intl.DateTimeFormat(state.lang, { weekday: "short", day: "numeric", month: "short" }).format(new Date(`${state.historyDay}T12:00:00`));
  byId<HTMLButtonElement>("nextDay").disabled = state.historyDay >= today();
  const apps = Array.isArray(state.historyApps) ? state.historyApps : [];
  const down = apps.reduce((s, a) => s + a.down_total, 0), up = apps.reduce((s, a) => s + a.up_total, 0);
  byId("historyStatus").textContent = `${t.apps(apps.length)} · ${formatBytes(down)} ${t.downloaded} · ${formatBytes(up)} ${t.uploaded}`;
  byId("historyRows").replaceChildren(...[...apps].sort((a, b) => b.down_total + b.up_total - a.down_total - a.up_total).map((app) => {
    const row = document.createElement("div"); row.className = "list-row"; row.setAttribute("role", "row");
    row.innerHTML = `<div class="name" role="cell"><span></span><span class="t-caption t-2"></span></div><span role="cell">${formatBytes(app.down_total)}</span><span role="cell">${formatBytes(app.up_total)}</span><span role="cell" class="t-2">${formatBytes(app.down_total + app.up_total)}</span>`;
    row.querySelector(".name span")!.textContent = app.name;
    row.querySelector(".name .t-caption")!.textContent = compactPath(app.path);
    (row.querySelector(".name") as HTMLElement).dataset.tip = app.path;
    return row;
  }));
  byId("historyEmpty").textContent = state.historyApps === null ? t.loading : state.historyApps === "error" ? t.historyError : apps.length ? "" : t.noRecords;
}

// ---- Settings ----
function card(glyph: IconName, title: string, text: string, control: HTMLElement | string) {
  const el = document.createElement("div"); el.className = "card";
  el.innerHTML = `${icon(glyph)}<div class="card-text"><h3 class="t-body"></h3><p></p></div><div class="card-ctl"></div>`;
  el.querySelector("h3")!.textContent = title; el.querySelector("p")!.textContent = text;
  el.querySelector(".card-ctl")!.append(control);
  return el;
}

function toggle(on: boolean, label: string, run: (on: boolean) => void) {
  const b = document.createElement("button");
  b.className = "toggle"; b.setAttribute("role", "switch"); b.ariaChecked = String(on); b.ariaLabel = label;
  b.innerHTML = `<span>${on ? t.on : t.off}</span><i></i>`;
  b.addEventListener("click", () => run(b.ariaChecked !== "true"));
  return b;
}

function group(title: string, ...cards: HTMLElement[]) {
  const el = document.createElement("div"); el.className = "group"; el.setAttribute("role", "group");
  if (title) { el.innerHTML = `<h2 class="group-title"></h2>`; el.firstElementChild!.textContent = title; el.setAttribute("aria-label", title); }
  el.append(...cards);
  return el;
}

function renderSettings() {
  const page = byId("settings");
  const focusedId = document.activeElement?.closest("[data-key]")?.getAttribute("data-key");
  const lang = select({ options: [{ value: "pl", label: "Polski" }, { value: "en", label: "English" }], value: state.lang, label: t.language, onChange: (v) => {
    setLang(v as Lang, true); (document.querySelector('[data-key="lang"]') as HTMLElement)?.focus();
  } });
  const theme = select({ options: (["system", "light", "dark"] as Theme[]).map((v) => ({ value: v, label: t.themes[v] })), value: state.theme, label: t.theme, onChange: (v) => {
    state.theme = v as Theme; store("theme", v); applyTheme();
  } });
  const edit = document.createElement("span"); edit.className = "card-ctl";
  edit.innerHTML = `<span class="t-2 num"></span><button class="btn">${t.edit}</button>`;
  edit.querySelector("span")!.textContent = formatLimit(state.global.limit);
  edit.querySelector("button")!.addEventListener("click", () => limitDialog("global", ""));
  const folder = document.createElement("button"); folder.className = "btn"; folder.textContent = t.openFolder;
  folder.addEventListener("click", () => void invoke("open_external", { target: "data" }).catch(() => infoBar("err", t.openError, "")));
  const startup = toggle(state.autostart, t.startup, async (on) => {
    try { state.autostart = await invoke<boolean>("set_autostart", { on }); }
    catch { state.autostart = await invoke<boolean>("get_autostart").catch(() => state.autostart); infoBar("err", t.autostartError, t.autostartErrorText); }
    renderSettings();
  });
  const pause = toggle(state.global.paused, t.pauseAll, (on) => void setPaused(on));
  lang.dataset.key = "lang"; theme.dataset.key = "theme"; startup.dataset.key = "startup"; pause.dataset.key = "pause";
  edit.querySelector("button")!.dataset.key = "edit"; folder.dataset.key = "folder";
  page.innerHTML = `<header class="page-head"><h1 class="t-title">${t.settings}</h1></header>`;
  page.append(
    group(t.general, card("power", t.startup, t.startupText, startup), card("language", t.language, t.languageText, lang), card("theme", t.theme, t.themeText, theme)),
    group(t.limits, card("gauge", t.globalLimit, t.globalText, edit), card("pause", t.pauseAll, t.pauseText, pause)),
    group(t.advanced, card("folder", t.dataFolder, "%APPDATA%\\Netlume", folder)),
  );
  bars();
  if (focusedId) (page.querySelector(`[data-key="${focusedId}"]`) as HTMLElement | null)?.focus();
}

function setLang(lang: Lang, save: boolean) {
  state.lang = lang; t = lang === "pl" ? pl : en; store("lang", lang); build();
  if (save) void invoke("set_language", { lang }).catch(() => infoBar("err", t.saveError, t.saveErrorText));
}

function applyTheme() {
  if (state.theme === "system") delete document.documentElement.dataset.theme;
  else document.documentElement.dataset.theme = state.theme;
  try { void getCurrentWindow().setTheme(state.theme === "system" ? null : state.theme).catch(() => {}); } catch { /* outside Tauri */ }
}

// ---- About (spec 10.3) ----
function renderAbout() {
  const page = byId("about");
  page.innerHTML = `<header class="about-head"><img src="${appIcon}" width="48" height="48" alt="">
      <div><h1 class="t-subtitle">Netlume</h1><p class="t-caption t-2 num">${t.version(state.version || "…")}</p></div></header>
    <p class="about-text">${t.aboutText}</p>
    <p><button class="link" data-url="${SITE}">Szymon Jankiewicz</button></p>`;
  const links: [IconName, string, string][] = [
    ["globe", t.projectPage, `${SITE}/p/netlume`], ["code", t.source, REPO],
    ["bug", t.issues, `${REPO}/issues`], ["document", t.licence, `${REPO}/blob/main/LICENSE`],
  ];
  const cards = links.map(([glyph, title, url]) => {
    const b = document.createElement("button"); b.className = "card"; b.dataset.url = url;
    b.innerHTML = `${icon(glyph)}<span class="card-text"><span></span><p></p></span>${icon("open")}`;
    b.querySelector(".card-text span")!.textContent = title; b.querySelector("p")!.textContent = url.replace("https://", "");
    return b;
  });
  page.append(group("", ...cards));
  bars();
  page.querySelectorAll<HTMLElement>("[data-url]").forEach((el) => el.addEventListener("click", () =>
    void invoke("open_external", { target: el.dataset.url }).catch(() => infoBar("err", t.openError, ""))));
}

// ---- Formatting ----
function hasLimit(limit: Limit | null): limit is Limit { return !!limit && (!!limit.down_bps || !!limit.up_bps); }
function formatRate(bytes: number) {
  return bytes >= 1_000_000 ? `${(bytes / 1_000_000).toFixed(bytes >= 10_000_000 ? 1 : 2)} MB/s` : `${Math.round(bytes / 1_000)} KB/s`;
}
function formatBytes(bytes: number) {
  if (bytes >= 1_000_000_000) return `${(bytes / 1_000_000_000).toFixed(2)} GB`;
  return bytes >= 1_000_000 ? `${(bytes / 1_000_000).toFixed(1)} MB` : `${Math.round(bytes / 1_000)} KB`;
}
function formatLimit(limit: Limit | null) {
  if (!hasLimit(limit)) return t.off;
  if (limit.down_bps && limit.down_bps === limit.up_bps) return formatRate(limit.down_bps);
  return `${DOWN} ${limit.down_bps ? formatRate(limit.down_bps) : t.off} · ${UP} ${limit.up_bps ? formatRate(limit.up_bps) : t.off}`;
}
function compactPath(path: string) { const parts = path.split("\\"); return parts.length > 2 ? `…\\${parts.slice(-2).join("\\")}` : path; }
function today() { return isoDay(new Date()); }
function isoDay(d: Date) { return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`; }
