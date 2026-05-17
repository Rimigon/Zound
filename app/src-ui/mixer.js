// Микшер: per-device row (volume / latency / balance / mute / EQ /
// peak), groups panel, master controls, peak-meter polling.

import { state, KEYS } from "./state.js";
import { invoke } from "./ipc.js";
import { t, tFmt } from "./i18n.js";
import { setStatus } from "./status.js";
import { applyLinkedLatency, refreshTargetLatency } from "./sync.js";
import { persistSession } from "./session.js";
import { displayName } from "./aliases.js";
import { openDeviceContextMenu } from "./device-menu.js";

function balanceLabel(b) {
  if (Math.abs(b) < 0.025) return t("balance-c");
  const pct = Math.round(Math.abs(b) * 100);
  return `${b < 0 ? t("balance-l") : t("balance-r")} ${pct}%`;
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

function buildGroupPill(a) {
  const wrap = document.createElement("span");
  wrap.className = "group-pill";
  const sel = document.createElement("select");
  const cur = state.deviceGroups.get(a.id) || "";
  const options = [["", t("group-none")]];
  for (const g of uniqueGroups()) options.push([g, g]);
  options.push(["__new__", t("group-new")]);
  for (const [val, label] of options) {
    const opt = document.createElement("option");
    opt.value = val;
    opt.textContent = label;
    if (val === cur) opt.selected = true;
    sel.appendChild(opt);
  }
  sel.addEventListener("change", (e) => {
    let v = e.target.value;
    if (v === "__new__") {
      const name = (window.prompt(t("group-new-prompt"), "") || "").trim();
      if (!name) {
        e.target.value = cur;
        return;
      }
      v = name;
    }
    if (v) state.deviceGroups.set(a.id, v);
    else state.deviceGroups.delete(a.id);
    persistDeviceGroups();
    renderActives();
  });
  const label = document.createElement("span");
  label.textContent = t("group-label") + ":";
  wrap.appendChild(label);
  wrap.appendChild(sel);
  return wrap;
}

function renderGroupsPanel(root) {
  const groups = uniqueGroups();
  if (groups.length === 0) return;
  const panel = document.createElement("div");
  panel.className = "groups-panel";
  for (const name of groups) {
    const members = state.active.filter(
      (a) => state.deviceGroups.get(a.id) === name,
    );
    if (members.length === 0) continue;
    const avgVol = members.reduce((s, a) => s + a.volume, 0) / members.length;
    const avgLat =
      members.reduce((s, a) => s + a.latencyMs, 0) / members.length;
    const allMuted = members.every((a) => a.muted);

    const row = document.createElement("div");
    row.className = "group-row";
    row.innerHTML = `
      <div class="name"></div>
      <input type="range" min="0" max="100" value="${Math.round(avgVol * 100)}" data-kind="g-vol" />
      <input type="range" min="0" max="500" value="${Math.round(avgLat)}" data-kind="g-lat" />
      <button class="group-mute"></button>
    `;
    row.querySelector(".name").textContent = `${name} (${members.length})`;
    const muteBtn = row.querySelector("button.group-mute");
    muteBtn.textContent = allMuted ? t("unmute-button") : t("group-mute");
    muteBtn.title = t("group-mute");

    row.querySelector('input[data-kind="g-vol"]').addEventListener(
      "input",
      (e) => {
        const v = parseInt(e.target.value, 10) / 100;
        for (const a of members) {
          a.volume = v;
          invoke("set_output_volume", { id: a.id, volume: v });
        }
        renderActives();
        persistSession();
      },
    );
    row.querySelector('input[data-kind="g-lat"]').addEventListener(
      "input",
      (e) => {
        const ms = parseInt(e.target.value, 10);
        for (const a of members) {
          a.latencyMs = ms;
          invoke("set_output_latency", { id: a.id, latencyMs: ms }).catch(
            () => {},
          );
        }
        renderActives();
        refreshTargetLatency();
        persistSession();
      },
    );
    muteBtn.addEventListener("click", () => {
      const next = !allMuted;
      for (const a of members) {
        a.muted = next;
        invoke("set_output_muted", { id: a.id, muted: next });
      }
      renderActives();
      persistSession();
    });
    panel.appendChild(row);
  }
  if (panel.children.length > 0) root.appendChild(panel);
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

function buildEqRow(a) {
  const eq = getDeviceEq(a.id);
  const row = document.createElement("div");
  row.className = "eq-row";
  row.innerHTML = `
    <span class="eq-label"></span>
    <div class="eq-band" data-band="low">
      <span class="eq-band-name"></span>
      <input type="range" min="-12" max="12" step="0.5" />
      <span class="eq-band-value"></span>
    </div>
    <div class="eq-band" data-band="mid">
      <span class="eq-band-name"></span>
      <input type="range" min="-12" max="12" step="0.5" />
      <span class="eq-band-value"></span>
    </div>
    <div class="eq-band" data-band="high">
      <span class="eq-band-name"></span>
      <input type="range" min="-12" max="12" step="0.5" />
      <span class="eq-band-value"></span>
    </div>
    <button class="eq-reset"></button>
  `;
  row.querySelector(".eq-label").textContent = t("eq-toggle");
  const setBand = (band, key) => {
    const div = row.querySelector(`[data-band="${band}"]`);
    div.querySelector(".eq-band-name").textContent = t(key);
    const input = div.querySelector("input");
    const value = div.querySelector(".eq-band-value");
    input.value = String(eq[band]);
    value.textContent = `${eq[band] > 0 ? "+" : ""}${eq[band].toFixed(1)} dB`;
    input.addEventListener("input", (e) => {
      const v = parseFloat(e.target.value);
      eq[band] = v;
      value.textContent = `${v > 0 ? "+" : ""}${v.toFixed(1)} dB`;
      pushEqToBackend(a);
      persistSession();
    });
  };
  setBand("low", "eq-low");
  setBand("mid", "eq-mid");
  setBand("high", "eq-high");
  const reset = row.querySelector(".eq-reset");
  reset.textContent = t("eq-reset");
  reset.addEventListener("click", () => {
    eq.low = 0;
    eq.mid = 0;
    eq.high = 0;
    pushEqToBackend(a);
    persistSession();
    renderActives();
  });
  return row;
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

// ---- public render ----

export function renderActives() {
  const root = document.getElementById("active-outputs");
  if (!root) return;
  root.innerHTML = "";
  state.peakBars.clear();
  if (state.active.length === 0) {
    const empty = document.createElement("p");
    empty.className = "hint";
    empty.textContent = t("no-active-outputs");
    root.appendChild(empty);
    return;
  }
  const note = document.createElement("p");
  note.className = "hint note";
  note.textContent = t("doubling-note");
  root.appendChild(note);

  renderGroupsPanel(root);

  for (const a of state.active) {
    const isStereo = (a.channels ?? 2) === 2;
    const row = document.createElement("div");
    row.className = "active-row";
    row.innerHTML = `
      <div class="info">
        <div class="name"></div>
        <div class="sliders with-mixer">
          <label data-row="volume"></label>
          <input type="range" min="0" max="100" value="${Math.round(a.volume * 100)}" data-kind="volume" />
          <span class="value" data-row="volume-v">${Math.round(a.volume * 100)}%</span>

          <label data-row="latency"></label>
          <input type="range" min="0" max="500" value="${a.latencyMs}" data-kind="latency" />
          <span class="value" data-row="latency-v">${a.latencyMs} ms</span>

          <label data-row="balance"></label>
          <input type="range" min="-100" max="100" step="5" value="${Math.round(
            (a.balance ?? 0) * 100,
          )}" data-kind="balance" class="balance-slider" ${
            isStereo ? "" : "disabled"
          } />
          <span class="value" data-row="balance-v"></span>

          <label data-row="mute"></label>
          <label class="switch">
            <input type="checkbox" data-kind="mute" ${a.muted ? "checked" : ""} />
            <span class="track"></span>
          </label>
          <span class="value"></span>
        </div>
      </div>
      <button class="device-reset-btn icon-btn" title="" aria-label="reset">⟲</button>
    `;
    const nameEl = row.querySelector(".name");
    const shown = displayName(a);
    nameEl.textContent = shown;
    nameEl.title = shown === a.name ? a.name : `${shown}\n(${a.name})`;
    nameEl.appendChild(buildGroupPill(a));

    if (a.endpointId) {
      row.addEventListener("contextmenu", (ev) => {
        ev.preventDefault();
        openDeviceContextMenu(ev.clientX, ev.clientY, {
          name: a.name,
          endpointId: a.endpointId,
        });
      });
    }
    row.querySelector('label[data-row="volume"]').textContent = t("volume-label");
    row.querySelector('label[data-row="latency"]').textContent = t("latency-label");
    const balLabel = row.querySelector('label[data-row="balance"]');
    balLabel.textContent = t("balance-label");
    if (!isStereo) balLabel.title = t("balance-mono-note");
    row.querySelector('label[data-row="mute"]').textContent = t("mute-label");

    const volEl = row.querySelector('input[data-kind="volume"]');
    const latEl = row.querySelector('input[data-kind="latency"]');
    const balEl = row.querySelector('input[data-kind="balance"]');
    const muteEl = row.querySelector('input[data-kind="mute"]');

    const volV = row.querySelector('[data-row="volume-v"]');
    const latV = row.querySelector('[data-row="latency-v"]');
    const balV = row.querySelector('[data-row="balance-v"]');
    balV.textContent = balanceLabel(a.balance ?? 0);

    // P1.6 — set_volume / set_balance / set_muted / set_eq fire-and-forget
    // (Tauri command возвращает () немедленно). UI не ждёт audio-thread.
    volEl.addEventListener("input", (e) => {
      const v = parseInt(e.target.value, 10) / 100;
      a.volume = v;
      volV.textContent = `${e.target.value}%`;
      invoke("set_output_volume", { id: a.id, volume: v });
      persistSession();
    });
    latEl.addEventListener("input", (e) => {
      const ms = parseInt(e.target.value, 10);
      if (state.latencyLinked) {
        applyLinkedLatency(ms, persistSession);
      } else {
        a.latencyMs = ms;
        latV.textContent = `${ms} ms`;
        invoke("set_output_latency", { id: a.id, latencyMs: ms })
          .then(() => {
            refreshTargetLatency();
            persistSession();
          })
          .catch((err) => setStatus(String(err), "err"));
      }
    });
    balEl.addEventListener("input", (e) => {
      const b = parseInt(e.target.value, 10) / 100;
      a.balance = b;
      balV.textContent = balanceLabel(b);
      invoke("set_output_balance", { id: a.id, balance: b });
      persistSession();
    });
    muteEl.addEventListener("change", (e) => {
      a.muted = e.target.checked;
      invoke("set_output_muted", { id: a.id, muted: a.muted });
      persistSession();
    });

    const peakRow = document.createElement("div");
    peakRow.className = "peak-row";
    peakRow.innerHTML = `
      <span class="peak-label"></span>
      <div class="peak-bar"><div class="peak-fill"></div></div>
    `;
    peakRow.querySelector(".peak-label").textContent = t("peak-label");
    row.querySelector(".sliders").appendChild(peakRow);
    state.peakBars.set(a.id, peakRow.querySelector(".peak-fill"));

    row.querySelector(".sliders").appendChild(buildEqRow(a));

    const reset = row.querySelector("button.device-reset-btn");
    reset.title = t("device-reset-title");
    reset.setAttribute("aria-label", t("device-reset"));
    reset.addEventListener("click", () => resetDevice(a));
    root.appendChild(row);
  }
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
    const fill = state.peakBars.get(id);
    if (!fill) continue;
    const pct = Math.min(100, Math.round(peak * 100));
    fill.style.width = pct + "%";
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
  const value = document.getElementById("master-gain-value");
  if (!range || !mute || !value) return;
  range.value = String(Math.round(state.master.gain * 100));
  mute.checked = state.master.muted;
  value.textContent = `${Math.round(state.master.gain * 100)}%`;
}
