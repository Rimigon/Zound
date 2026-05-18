// Микшер: новый layout — .mix-row[data-id] с .active-head / .active-primary
// (Volume crossfeed + Latency) / .advanced[data-open] (Balance + EQ vertical
// faders). Группы — .group с цветной планкой .group-rail слева.

import { state, KEYS } from "./state.js";
import { invoke } from "./ipc.js";
import { t } from "./i18n.js";
import { setStatus } from "./status.js";
import { applyLinkedLatency, refreshTargetLatency, renderSyncWidget } from "./sync.js";
import { persistSession } from "./session.js";
import { displayName } from "./aliases.js";
import { openDeviceContextMenu } from "./device-menu.js";
import { ic } from "./icons.js";

const GROUP_COLORS = [
  "oklch(0.72 0.14 195)", // teal
  "oklch(0.72 0.18 320)", // magenta
  "oklch(0.78 0.15 145)", // green
  "oklch(0.74 0.16 50)",  // orange
  "oklch(0.70 0.14 260)", // blue
  "oklch(0.72 0.14 100)", // chartreuse
];
function groupColorFor(name) {
  // stable hash by name → один и тот же цвет каждый рендер.
  let h = 0;
  for (let i = 0; i < name.length; i++) h = (h * 31 + name.charCodeAt(i)) & 0xffff;
  return GROUP_COLORS[h % GROUP_COLORS.length];
}

function balanceLabel(b) {
  if (Math.abs(b) < 0.025) return "C";
  const pct = Math.round(Math.abs(b) * 100);
  return `${b < 0 ? "L" : "R"}${pct}`;
}

// ---- groups (UI-only, persists в localStorage) ----
export function loadDeviceGroups() {
  try {
    const raw = localStorage.getItem(KEYS.groups);
    if (!raw) return;
    const obj = JSON.parse(raw);
    if (obj && typeof obj === "object") {
      state.deviceGroups = new Map(Object.entries(obj));
    }
  } catch (_) {}
}
function persistDeviceGroups() {
  const obj = Object.fromEntries(state.deviceGroups.entries());
  try {
    localStorage.setItem(KEYS.groups, JSON.stringify(obj));
  } catch (_) {}
}
function uniqueGroups() {
  const set = new Set();
  for (const a of state.active) {
    const g = state.deviceGroups.get(a.id);
    if (g) set.add(g);
  }
  return [...set].sort();
}

// ---- EQ ----
function getDeviceEq(id) {
  let e = state.eq.get(id);
  if (!e) {
    e = { low: 0, mid: 0, high: 0 };
    state.eq.set(id, e);
  }
  return e;
}
function pushEqToBackend(a) {
  const e = getDeviceEq(a.id);
  invoke("set_output_eq", {
    id: a.id,
    lowDb: e.low,
    midDb: e.mid,
    highDb: e.high,
  });
}

// ---- per-device reset ----
async function resetDevice(a) {
  a.volume = 1.0;
  a.latencyMs = 20;
  a.balance = 0;
  a.muted = false;
  state.eq.set(a.id, { low: 0, mid: 0, high: 0 });
  invoke("set_output_volume", { id: a.id, volume: 1.0 });
  try {
    await invoke("set_output_latency", { id: a.id, latencyMs: 20 });
  } catch (_) {}
  invoke("set_output_balance", { id: a.id, balance: 0 });
  invoke("set_output_muted", { id: a.id, muted: false });
  invoke("set_output_eq", { id: a.id, lowDb: 0, midDb: 0, highDb: 0 });
  persistSession();
  refreshTargetLatency();
  renderActives();
}

// ---- empty states ----
function tplEmptyEngine() {
  return `
    <div class="empty">
      <div class="empty-inner">
        <div class="empty-art" aria-hidden="true">
          <span class="ring"></span><span class="ring ring-2"></span><span class="ring ring-3"></span>
          <svg width="56" height="56" viewBox="0 0 24 24" class="glyph" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round">
            <path d="M4 12h2l2-4 3 8 3-12 2 8h4"/>
          </svg>
        </div>
        <div class="empty-title">${t("empty-engine-title")}</div>
        <div class="empty-desc">${t("empty-engine-desc")}</div>
        <div class="empty-kbd">
          <span style="color:var(--muted);font-size:var(--t-12);">${t("empty-engine-hint")}</span>
          <span class="kbd">Space</span>
        </div>
      </div>
    </div>
  `;
}
function tplEmptyActive() {
  return `
    <div class="empty">
      <div class="empty-inner">
        <div class="empty-arrow">
          <svg class="ar" viewBox="0 0 80 40">
            <path d="M76 20H8 M14 14L8 20l6 6"/>
          </svg>
          <div class="empty-title">${t("empty-active-title")}</div>
          <div class="empty-desc">${t("empty-active-desc")}</div>
        </div>
      </div>
    </div>
  `;
}

// ---- public render ----
export function renderActives() {
  const root = document.getElementById("active-outputs");
  const countEl = document.getElementById("actives-count");
  if (!root) return;
  root.innerHTML = "";
  state.peakBars.clear();

  if (countEl) countEl.textContent = String(state.active.length);

  // sync widget rebuild (header)
  renderSyncWidget();

  if (!state.engineRunning) {
    root.innerHTML = tplEmptyEngine();
    return;
  }
  if (state.active.length === 0) {
    root.innerHTML = tplEmptyActive();
    return;
  }

  // Группируем активные по группе. Ungrouped — отдельным списком (без рейла).
  const groups = new Map();
  const ungrouped = [];
  for (const a of state.active) {
    const g = state.deviceGroups.get(a.id);
    if (g) {
      if (!groups.has(g)) groups.set(g, []);
      groups.get(g).push(a);
    } else {
      ungrouped.push(a);
    }
  }

  for (const [name, members] of groups) {
    root.appendChild(buildGroup(name, members));
  }
  if (ungrouped.length) {
    const list = document.createElement("div");
    list.className = "active-list";
    list.style.paddingLeft = "0";
    for (const a of ungrouped) list.appendChild(buildMixRow(a));
    root.appendChild(list);
  }
}

function buildGroup(name, members) {
  const color = groupColorFor(name);
  const wrap = document.createElement("div");
  wrap.className = "group";
  wrap.style.setProperty("--group-color", color);
  const head = document.createElement("div");
  head.className = "group-head";
  head.innerHTML = `
    ${ic("i-group")}
    <span class="group-name">${escapeHtml(name)}</span>
    <span class="group-controls">
      <span class="num">${members.length} ${t("group-members")}</span>
      <button type="button" class="ctrl-btn" data-grp-mute>${t("group-mute")}</button>
    </span>
  `;
  const allMuted = members.every((a) => a.muted);
  head.querySelector("[data-grp-mute]").addEventListener("click", () => {
    const next = !allMuted;
    for (const a of members) {
      a.muted = next;
      invoke("set_output_muted", { id: a.id, muted: next });
    }
    renderActives();
    persistSession();
  });
  wrap.appendChild(head);

  const rail = document.createElement("span");
  rail.className = "group-rail";
  rail.setAttribute("aria-hidden", "true");
  wrap.appendChild(rail);

  const list = document.createElement("div");
  list.className = "active-list";
  for (const a of members) list.appendChild(buildMixRow(a, color));
  wrap.appendChild(list);
  return wrap;
}

function buildMixRow(a, groupColor) {
  const id = a.id;
  const isStereo = (a.channels ?? 2) === 2;
  const eq = getDeviceEq(id);
  const advOpen = state.advancedOpen?.has(id) || hasNonDefault(a, eq);
  if (!state.advancedOpen) state.advancedOpen = new Set();
  if (advOpen) state.advancedOpen.add(id);
  const vol = Math.round(a.volume * 100);
  const lat = Math.round(a.latencyMs);
  const balPct = Math.round((a.balance ?? 0) * 100);
  const balText = balanceLabel(a.balance ?? 0);
  const shown = displayName(a);
  const titleAttr = shown === a.name ? a.name : `${shown}\n(${a.name})`;
  const aliasFragment =
    shown === a.name ? "" : ` <span class="alias">· ${escapeHtml(a.name)}</span>`;
  const groupName = state.deviceGroups.get(id) || null;
  const color = groupColor || (groupName ? groupColorFor(groupName) : null);

  const row = document.createElement("article");
  row.className = "mix-row";
  row.dataset.id = id;
  if (color) row.style.setProperty("--group-color", color);

  const groupPill = groupName
    ? `<span class="group-pill" data-group-edit><span class="dot"></span>${escapeHtml(groupName)}</span>`
    : `<button type="button" class="group-pill" data-group-add>${ic("i-plus")} ${t("group-add")}</button>`;

  row.innerHTML = `
    <header class="active-head">
      <div class="active-title">
        <span class="active-name" title="${escapeAttr(titleAttr)}">${escapeHtml(shown)}${aliasFragment}</span>
        ${groupPill}
      </div>
      <div class="active-actions">
        <button type="button" class="act-btn" data-mute data-muted="${a.muted}" title="${escapeAttr(t("mute-label"))}">
          ${ic(a.muted ? "i-mute" : "i-unmute")}
        </button>
        <button type="button" class="act-btn" data-rename title="${escapeAttr(t("device-rename"))}">
          ${ic("i-rename")}
        </button>
        <button type="button" class="act-btn" data-reset title="${escapeAttr(t("device-reset-title"))}">
          ${ic("i-restart")}
        </button>
      </div>
    </header>

    <div class="active-primary">
      <div class="ctrl">
        <div class="ctrl-head">
          <span class="ctrl-label">${t("volume-label")}</span>
          <span class="ctrl-value"><span class="num" data-vol-v>${a.muted ? "—" : vol}</span><span class="u">%</span></span>
        </div>
        <div class="slider-fill" data-vol-fill style="--val:${a.muted ? 0 : vol};">
          <span class="fill"></span>
          <input type="range" class="slider" min="0" max="100" value="${vol}" data-vol aria-label="${escapeAttr(t("volume-label"))}"/>
        </div>
        <div class="meter">
          <span class="meter-label">${isStereo ? "L" : "M"}</span>
          <div class="meter-bars">
            <div class="meter-bar" data-peak="L" style="--peak:0"></div>
            ${isStereo ? `<div class="meter-bar" data-peak="R" style="--peak:0"></div>` : ""}
          </div>
          ${isStereo ? `<span class="meter-label" style="grid-row:2">R</span>` : ""}
        </div>
        <div class="meter-scale">
          <span>−∞</span><span>−24</span><span>−12</span><span>−6</span><span>0 dB</span>
        </div>
      </div>

      <div class="ctrl">
        <div class="ctrl-head">
          <span class="ctrl-label">${t("latency-label")}</span>
          <span class="ctrl-value"><span class="num" data-lat-v>${lat}</span><span class="u">ms</span></span>
        </div>
        <div class="latency-track">
          <input type="range" class="slider" min="0" max="500" step="2" value="${lat}" data-lat aria-label="${escapeAttr(t("latency-label"))}"/>
          <div class="latency-ticks" aria-hidden="true">
            ${Array.from({ length: 11 }).map(() => "<span></span>").join("")}
          </div>
        </div>
        <div class="meter-scale">
          <span>0</span><span>250</span><span>500 ms</span>
        </div>
      </div>
    </div>

    <div class="advanced" data-open="${advOpen}">
      <button type="button" class="advanced-toggle" data-adv aria-expanded="${advOpen}">
        <span>${t("ctrl-advanced")}</span>
        ${ic("i-chevron", "icon chev")}
      </button>
      <div class="advanced-body">
        <div class="advanced-grid">
          <div class="ctrl">
            <div class="ctrl-head">
              <span class="ctrl-label">${t("balance-label")}</span>
              <span class="ctrl-value"><span class="num" data-bal-v>${escapeHtml(balText)}</span></span>
            </div>
            <div class="balance">
              <span class="center-tick" aria-hidden="true"></span>
              <input type="range" class="slider" min="-100" max="100" step="5" value="${balPct}" data-bal ${isStereo ? "" : "disabled"}
                     aria-label="${escapeAttr(t("balance-label"))}"/>
              <div class="bal-labels"><span>L</span><span>C</span><span>R</span></div>
            </div>
          </div>

          <div class="eq" aria-label="${escapeAttr(t("eq-toggle"))}">
            <div class="eq-bands">
              ${["low", "mid", "high"]
                .map((band) => {
                  const v = eq[band];
                  const label =
                    band === "low" ? t("eq-low") : band === "mid" ? t("eq-mid") : t("eq-high");
                  const freq =
                    band === "low" ? "100 Hz" : band === "mid" ? "1 kHz" : "8 kHz";
                  return `
                    <div class="eq-band">
                      <div>
                        <div class="band-name">${escapeHtml(label)}</div>
                        <div class="band-freq">${freq}</div>
                      </div>
                      <input type="range" min="-12" max="12" step="0.5" value="${v}"
                             data-eq="${band}" aria-label="${escapeAttr(label + " " + freq)}"/>
                      <span class="band-value ${v === 0 ? "zero" : ""}" data-eq-v="${band}">${v > 0 ? "+" : ""}${v.toFixed(1)} dB</span>
                    </div>
                  `;
                })
                .join("")}
            </div>
          </div>
        </div>
        <div style="display:flex;justify-content:flex-end;">
          <button type="button" class="eq-reset" data-eq-reset>${ic("i-restart")} ${t("eq-reset")}</button>
        </div>
      </div>
    </div>
  `;

  wireMixRow(row, a);
  if (a.endpointId) {
    row.addEventListener("contextmenu", (ev) => {
      ev.preventDefault();
      openDeviceContextMenu(ev.clientX, ev.clientY, {
        name: a.name,
        endpointId: a.endpointId,
      });
    });
  }
  state.peakBars.set(id, row);
  return row;
}

function hasNonDefault(a, eq) {
  // Открывать Advanced, если у устройства не дефолтные balance/EQ.
  if (Math.abs(a.balance || 0) > 0.001) return true;
  if (eq.low !== 0 || eq.mid !== 0 || eq.high !== 0) return true;
  return false;
}

function wireMixRow(row, a) {
  const id = a.id;

  // mute
  row.querySelector("[data-mute]").addEventListener("click", () => {
    a.muted = !a.muted;
    invoke("set_output_muted", { id, muted: a.muted });
    persistSession();
    renderActives();
  });

  // rename → переиспользуем context-menu rename action
  row.querySelector("[data-rename]").addEventListener("click", () => {
    if (!a.endpointId) return;
    const rect = row.getBoundingClientRect();
    openDeviceContextMenu(rect.left + 80, rect.top + 32, {
      name: a.name,
      endpointId: a.endpointId,
    });
  });

  // reset
  row.querySelector("[data-reset]").addEventListener("click", () => resetDevice(a));

  // group pill — circular through unique groups + новый.
  const grpAdd = row.querySelector("[data-group-add]");
  if (grpAdd) {
    grpAdd.addEventListener("click", () => promptGroup(a));
  }
  const grpEdit = row.querySelector("[data-group-edit]");
  if (grpEdit) {
    grpEdit.addEventListener("click", () => promptGroup(a));
  }

  // advanced toggle
  const adv = row.querySelector("[data-adv]");
  adv.addEventListener("click", () => {
    const open = !(state.advancedOpen.has(id));
    if (open) state.advancedOpen.add(id);
    else state.advancedOpen.delete(id);
    const wrap = row.querySelector(".advanced");
    wrap.dataset.open = String(open);
    adv.setAttribute("aria-expanded", String(open));
  });

  // volume
  const volEl = row.querySelector("[data-vol]");
  const volV = row.querySelector("[data-vol-v]");
  const volFill = row.querySelector("[data-vol-fill]");
  volEl.addEventListener("input", (e) => {
    const v = parseInt(e.target.value, 10) / 100;
    a.volume = v;
    volV.textContent = String(Math.round(v * 100));
    if (volFill) volFill.style.setProperty("--val", Math.round(v * 100));
    invoke("set_output_volume", { id, volume: v });
    persistSession();
  });

  // latency
  const latEl = row.querySelector("[data-lat]");
  const latV = row.querySelector("[data-lat-v]");
  latEl.addEventListener("input", (e) => {
    const ms = parseInt(e.target.value, 10);
    if (state.latencyLinked) {
      applyLinkedLatency(ms, persistSession);
    } else {
      a.latencyMs = ms;
      latV.textContent = String(ms);
      invoke("set_output_latency", { id, latencyMs: ms })
        .then(() => {
          refreshTargetLatency();
          persistSession();
        })
        .catch((err) => setStatus(String(err), "err"));
    }
  });

  // balance
  const balEl = row.querySelector("[data-bal]");
  const balV = row.querySelector("[data-bal-v]");
  if (balEl) {
    balEl.addEventListener("input", (e) => {
      const b = parseInt(e.target.value, 10) / 100;
      a.balance = b;
      balV.textContent = balanceLabel(b);
      invoke("set_output_balance", { id, balance: b });
      persistSession();
    });
  }

  // EQ — три горизонтальных слайдера, по полосе на каждый.
  const eq = getDeviceEq(id);
  row.querySelectorAll("[data-eq]").forEach((input) => {
    input.addEventListener("input", (e) => {
      const band = e.target.dataset.eq;
      const v = parseFloat(e.target.value);
      eq[band] = v;
      const valEl = row.querySelector(`[data-eq-v="${band}"]`);
      if (valEl) {
        valEl.textContent = `${v > 0 ? "+" : ""}${v.toFixed(1)} dB`;
        valEl.classList.toggle("zero", v === 0);
      }
      pushEqToBackend(a);
      persistSession();
    });
  });
  row.querySelector("[data-eq-reset]").addEventListener("click", () => {
    eq.low = 0;
    eq.mid = 0;
    eq.high = 0;
    pushEqToBackend(a);
    persistSession();
    renderActives();
  });
}

function promptGroup(a) {
  const cur = state.deviceGroups.get(a.id) || "";
  const groups = uniqueGroups();
  const labels = [t("group-none"), ...groups, t("group-new")];
  const idx = groups.indexOf(cur);
  // Простой цикл: текущая → следующая в списке (или «новая…», или «без»).
  // Если нажали «новая» — prompt.
  const next = labels[(idx + 2) % labels.length]; // 0:none → idx=-1 → next=labels[1]
  if (next === t("group-none")) {
    state.deviceGroups.delete(a.id);
  } else if (next === t("group-new")) {
    const name = (window.prompt(t("group-new-prompt"), "") || "").trim();
    if (!name) return;
    state.deviceGroups.set(a.id, name);
  } else {
    state.deviceGroups.set(a.id, next);
  }
  persistDeviceGroups();
  renderActives();
}

// ---- peaks polling ----
export async function refreshPeaks() {
  if (!state.engineRunning || state.active.length === 0) return;
  let snap;
  try {
    snap = await invoke("peaks");
  } catch (_) {
    return;
  }
  for (const { id, peak } of snap) {
    const row = state.peakBars.get(id);
    if (!row) continue;
    const pct = Math.min(100, Math.round(peak * 100));
    const bars = row.querySelectorAll(".meter-bar");
    // backend отдаёт один peak per устройство — рисуем одинаково на обоих
    // каналах. Реальный стерео-сплит — отдельный backend-выход; стерео-UI
    // готов принять его без изменений.
    bars.forEach((b) => b.style.setProperty("--peak", pct));
  }
}

// ---- master ----
export async function loadMasterState() {
  try {
    const m = await invoke("master_state");
    state.master.gain = m.gain;
    state.master.muted = m.muted;
  } catch (_) {
    state.master = { gain: 1.0, muted: false };
  }
  applyMasterUI();
}

export function applyMasterUI() {
  const range = document.getElementById("master-gain");
  const mute = document.getElementById("master-mute");
  const muteWrap = document.getElementById("master-mute-wrap");
  const value = document.getElementById("master-gain-value");
  const fill = document.getElementById("master-fill");
  const muteIcon = muteWrap?.querySelector(".master-mute-icon use");
  if (!range || !mute || !value) return;
  const pct = Math.round(state.master.gain * 100);
  range.value = String(pct);
  mute.checked = state.master.muted;
  if (muteWrap) muteWrap.dataset.muted = String(state.master.muted);
  if (muteIcon) muteIcon.setAttribute("href", state.master.muted ? "#i-mute" : "#i-unmute");
  value.textContent = state.master.muted ? "—" : String(pct);
  if (fill) fill.style.setProperty("--val", state.master.muted ? 0 : pct);
}

function escapeHtml(s) {
  return String(s ?? "")
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}
function escapeAttr(s) {
  return escapeHtml(s).replace(/\n/g, "&#10;");
}
