// Zound UI — vanilla JS, IPC через window.__TAURI__.core.invoke.

const invoke = (cmd, args) => window.__TAURI__.core.invoke(cmd, args);

const state = {
  engineRunning: false,
  loopbackSource: null, // имя default-устройства, источник захвата
  devices: [], // устройства, отображённые сейчас в верхнем списке
  active: [], // {id, name, volume, latency_ms, balance, muted, channels}
  lang: "ru",
  dict: {},
  showAllDevices: false, // false → list_outputs, true → list_all_devices
  theme: "auto", // "auto" | "dark" | "light"
  testRunning: new Map(), // device_name → { kind, bpm? }
  activePopover: null, // открытый test-popover (DOM-элемент) или null
  suspendPersist: false, // true пока идёт restoreSession (race-guard)
  master: { gain: 1.0, muted: false },
  peakBars: new Map(), // device_id → DOM .peak-fill
  latencyLinked: false, // если true, движение одного latency-слайдера двигает все
  // device_id → { low, mid, high } gain в dB. Не сохраняется в session (UI-only),
  // отправляется в backend через set_output_eq.
  eq: new Map(),
  // device_id → group name (string). UI-only, хранится в localStorage.
  deviceGroups: new Map(),
};

const SHOW_ALL_KEY = "zound.showAllDevices";
const THEME_KEY = "zound.theme";
const LATENCY_LINK_KEY = "zound.latencyLinked";
const GROUPS_KEY = "zound.deviceGroups";
const EQ_KEY = "zound.deviceEq";
const SESSION_KEY = "zound.lastSession.outputs"; // legacy — мигрируем в backend profile
const PROFILE_PERSIST_DEBOUNCE_MS = 250;

const DEVICE_REFRESH_INTERVAL_MS = 2000;
const PEAK_REFRESH_INTERVAL_MS = 80;

// ------------- i18n -------------

async function loadDictionary(lang) {
  state.dict = await invoke("load_dictionary", { lang });
}

function t(key) {
  return state.dict[key] ?? key;
}

function tFmt(key, args) {
  // Fluent возвращает плейсхолдеры в виде "{ $name }"; подставляем сами,
  // backend.format_message работает только для float-аргументов.
  let raw = t(key);
  if (args) {
    for (const [k, v] of Object.entries(args)) {
      const re = new RegExp("\\{\\s*\\$" + k + "\\s*\\}", "g");
      raw = raw.replace(re, v);
    }
  }
  return raw;
}

function applyTranslations() {
  document.querySelectorAll("[data-i18n]").forEach((el) => {
    const key = el.getAttribute("data-i18n");
    el.textContent = t(key);
  });
  document.querySelectorAll("[data-i18n-title]").forEach((el) => {
    const key = el.getAttribute("data-i18n-title");
    el.title = t(key);
  });
  document.documentElement.lang = state.lang;
  refreshEngineButton();
  renderDevices();
  renderActives();
  refreshTargetLatency();
  refreshDefaultSourceWarning();
}

function refreshEngineButton() {
  const btn = document.getElementById("engine-toggle");
  btn.textContent = state.engineRunning ? t("engine-stop") : t("engine-start");
  btn.classList.toggle("danger", state.engineRunning);
}

// ------------- theme -------------

function applyTheme(theme) {
  state.theme = theme;
  if (theme === "auto") {
    document.documentElement.dataset.theme = "";
  } else {
    document.documentElement.dataset.theme = theme;
  }
  if (theme === "auto") {
    localStorage.removeItem(THEME_KEY);
  } else {
    localStorage.setItem(THEME_KEY, theme);
  }
  const btn = document.getElementById("theme-toggle");
  if (btn) {
    // Иконка отражает «текущую» тему: солнце = сейчас светлая, луна = тёмная,
    // полумесяц = auto. Клик пойдёт по кругу auto → dark → light → auto.
    btn.textContent = theme === "light" ? "☀" : theme === "dark" ? "🌙" : "🌓";
  }
}

function cycleTheme() {
  const next =
    state.theme === "auto"
      ? "dark"
      : state.theme === "dark"
        ? "light"
        : "auto";
  applyTheme(next);
}

// ------------- статус -------------

function setStatus(msg, kind = "") {
  const el = document.getElementById("status");
  el.textContent = msg;
  el.className = "status " + kind;
}

// ------------- устройства (верхний список) -------------

function devicesEqual(a, b) {
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) {
    if (
      a[i].id !== b[i].id ||
      a[i].name !== b[i].name ||
      a[i].sample_rate !== b[i].sample_rate ||
      a[i].channels !== b[i].channels ||
      a[i].is_default !== b[i].is_default ||
      a[i].is_input_only !== b[i].is_input_only
    ) {
      return false;
    }
  }
  return true;
}

async function refreshDevices(options = {}) {
  const { silent = false } = options;
  const cmd = state.showAllDevices ? "list_all_devices" : "list_outputs";
  let next;
  try {
    next = await invoke(cmd);
  } catch (e) {
    if (!silent) setStatus(String(e), "err");
    return;
  }
  const changed = !devicesEqual(state.devices, next);
  state.devices = next;
  if (changed) {
    renderDevices();
    if (!silent) setStatus(t("status-devices-refreshed"), "ok");
  }
}

function renderDevices() {
  const root = document.getElementById("devices");
  root.innerHTML = "";
  state.activePopover = null;
  for (const d of state.devices) {
    const active = state.active.find((a) => a.id === d.id);
    const isSource =
      state.loopbackSource !== null && d.name === state.loopbackSource;
    const inputOnly = !!d.is_input_only;

    const row = document.createElement("div");
    row.className =
      "device-row" +
      (d.is_default ? " default" : "") +
      (isSource ? " source" : "") +
      (inputOnly ? " input-only" : "");
    row.innerHTML = `
      <div class="dot"></div>
      <div class="info">
        <div class="name"></div>
        <div class="meta"></div>
      </div>
      <button class="test-btn icon-btn" data-i18n-title="test-button-title">🔊</button>
      <button class="action-btn"></button>
    `;
    const nameEl = row.querySelector(".name");
    nameEl.textContent = d.name;
    nameEl.title = d.name;

    const meta = row.querySelector(".meta");
    meta.textContent = `${d.sample_rate} Hz · ${d.channels} ch${d.is_default ? " · default" : ""}`;
    if (isSource) {
      const badge = document.createElement("span");
      badge.className = "source-badge";
      badge.textContent = " · " + t("device-source-badge");
      meta.appendChild(badge);
    }
    if (inputOnly) {
      const badge = document.createElement("span");
      badge.className = "input-badge";
      badge.textContent = " · " + t("device-input-only-badge");
      meta.appendChild(badge);
    }

    const testBtn = row.querySelector(".test-btn");
    testBtn.title = t("test-button-title");
    if (inputOnly) {
      testBtn.disabled = true;
    } else if (isSource) {
      testBtn.disabled = true;
      testBtn.title = t("test-source-disabled");
    } else {
      const running = state.testRunning.get(d.name);
      if (running) {
        testBtn.textContent = "⏹";
        testBtn.title = formatTestRunning(running);
        testBtn.addEventListener("click", () => stopTest(d.name));
      } else {
        testBtn.addEventListener("click", (ev) => {
          ev.stopPropagation();
          openTestPopover(row, d.name);
        });
      }
    }

    const btn = row.querySelector(".action-btn");
    if (inputOnly) {
      btn.textContent = t("device-input-only-badge");
      btn.disabled = true;
      btn.title = t("device-input-only-note");
    } else if (isSource) {
      btn.textContent = t("device-source-badge");
      btn.disabled = true;
      btn.title = t("device-source-note");
    } else if (active) {
      btn.textContent = t("device-remove");
      btn.addEventListener("click", () => removeOutput(d.id));
    } else {
      btn.textContent = t("device-add");
      btn.addEventListener("click", () => addOutput(d.name));
    }
    root.appendChild(row);
  }
}

// ------------- активные (слайдеры) -------------

function balanceLabel(b) {
  if (Math.abs(b) < 0.025) return t("balance-c");
  const pct = Math.round(Math.abs(b) * 100);
  return `${b < 0 ? t("balance-l") : t("balance-r")} ${pct}%`;
}

function renderActives() {
  const root = document.getElementById("active-outputs");
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
          <input type="range" min="0" max="500" value="${a.latency_ms}" data-kind="latency" />
          <span class="value" data-row="latency-v">${a.latency_ms} ms</span>

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
    nameEl.textContent = a.name;
    nameEl.title = a.name;
    nameEl.appendChild(buildGroupPill(a));
    row.querySelector('label[data-row="volume"]').textContent = t("volume-label");
    row.querySelector('label[data-row="latency"]').textContent = t("latency-label");
    const balLabel = row.querySelector('label[data-row="balance"]');
    balLabel.textContent = t("balance-label");
    if (!isStereo) {
      balLabel.title = t("balance-mono-note");
    }
    row.querySelector('label[data-row="mute"]').textContent = t("mute-label");

    const volEl = row.querySelector('input[data-kind="volume"]');
    const latEl = row.querySelector('input[data-kind="latency"]');
    const balEl = row.querySelector('input[data-kind="balance"]');
    const muteEl = row.querySelector('input[data-kind="mute"]');

    const volV = row.querySelector('[data-row="volume-v"]');
    const latV = row.querySelector('[data-row="latency-v"]');
    const balV = row.querySelector('[data-row="balance-v"]');
    balV.textContent = balanceLabel(a.balance ?? 0);

    volEl.addEventListener("input", (e) => {
      const v = parseInt(e.target.value, 10) / 100;
      a.volume = v;
      volV.textContent = `${e.target.value}%`;
      invoke("set_output_volume", { id: a.id, volume: v })
        .then(persistSession)
        .catch((err) => setStatus(String(err), "err"));
    });
    latEl.addEventListener("input", (e) => {
      const ms = parseInt(e.target.value, 10);
      if (state.latencyLinked) {
        applyLinkedLatency(ms);
      } else {
        a.latency_ms = ms;
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
      invoke("set_output_balance", { id: a.id, balance: b })
        .then(persistSession)
        .catch((err) => setStatus(String(err), "err"));
    });
    muteEl.addEventListener("change", (e) => {
      a.muted = e.target.checked;
      invoke("set_output_muted", { id: a.id, muted: a.muted })
        .then(persistSession)
        .catch((err) => setStatus(String(err), "err"));
    });

    // Peak-meter row
    const peakRow = document.createElement("div");
    peakRow.className = "peak-row";
    peakRow.innerHTML = `
      <span class="peak-label"></span>
      <div class="peak-bar"><div class="peak-fill"></div></div>
    `;
    peakRow.querySelector(".peak-label").textContent = t("peak-label");
    row.querySelector(".sliders").appendChild(peakRow);
    state.peakBars.set(a.id, peakRow.querySelector(".peak-fill"));

    // EQ row
    row.querySelector(".sliders").appendChild(buildEqRow(a));

    const reset = row.querySelector("button.device-reset-btn");
    reset.title = t("device-reset-title");
    reset.setAttribute("aria-label", t("device-reset"));
    reset.addEventListener("click", () => resetDevice(a));
    root.appendChild(row);
  }
}

async function resetDevice(a) {
  const defaults = {
    volume: 1.0,
    latency_ms: 20,
    balance: 0,
    muted: false,
  };
  a.volume = defaults.volume;
  a.latency_ms = defaults.latency_ms;
  a.balance = defaults.balance;
  a.muted = defaults.muted;
  state.eq.set(a.id, { low: 0, mid: 0, high: 0 });
  try {
    await Promise.all([
      invoke("set_output_volume", { id: a.id, volume: defaults.volume }),
      invoke("set_output_latency", {
        id: a.id,
        latencyMs: defaults.latency_ms,
      }),
      invoke("set_output_balance", { id: a.id, balance: defaults.balance }),
      invoke("set_output_muted", { id: a.id, muted: defaults.muted }),
      invoke("set_output_eq", { id: a.id, lowDb: 0, midDb: 0, highDb: 0 }),
    ]);
  } catch (e) {
    setStatus(String(e), "err");
  }
  persistDeviceEq();
  persistSession();
  refreshTargetLatency();
  renderActives();
}

// ------------- groups + EQ helpers -------------

function loadDeviceGroups() {
  try {
    const raw = localStorage.getItem(GROUPS_KEY);
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
    localStorage.setItem(GROUPS_KEY, JSON.stringify(obj));
  } catch (_) {}
}

function loadDeviceEq() {
  try {
    const raw = localStorage.getItem(EQ_KEY);
    if (!raw) return;
    const obj = JSON.parse(raw);
    if (obj && typeof obj === "object") {
      for (const [id, v] of Object.entries(obj)) {
        if (v && typeof v === "object") state.eq.set(id, {
          low: +v.low || 0, mid: +v.mid || 0, high: +v.high || 0,
        });
      }
    }
  } catch (_) {}
}

function persistDeviceEq() {
  const obj = Object.fromEntries(state.eq.entries());
  try {
    localStorage.setItem(EQ_KEY, JSON.stringify(obj));
  } catch (_) {}
}

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
  }).catch((err) => setStatus(String(err), "err"));
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
      members.reduce((s, a) => s + a.latency_ms, 0) / members.length;
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
          invoke("set_output_volume", { id: a.id, volume: v }).catch(() => {});
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
          a.latency_ms = ms;
          invoke("set_output_latency", { id: a.id, latencyMs: ms }).catch(() => {});
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
        invoke("set_output_muted", { id: a.id, muted: next }).catch(() => {});
      }
      renderActives();
      persistSession();
    });
    panel.appendChild(row);
  }
  if (panel.children.length > 0) root.appendChild(panel);
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
      persistDeviceEq();
    });
  };
  setBand("low", "eq-low");
  setBand("mid", "eq-mid");
  setBand("high", "eq-high");
  const reset = row.querySelector(".eq-reset");
  reset.textContent = t("eq-reset");
  reset.addEventListener("click", () => {
    eq.low = 0; eq.mid = 0; eq.high = 0;
    pushEqToBackend(a);
    persistDeviceEq();
    renderActives();
  });
  return row;
}

// Применить одну и ту же задержку ко всем активным устройствам
// (UI-режим «связать задержки»). Обновляет state, DOM и backend одним
// вызовом set_all_latencies — это атомарно с точки зрения SyncEngine
// (один пересчёт target_latency).
function applyLinkedLatency(ms) {
  for (const a of state.active) {
    a.latency_ms = ms;
  }
  // Обновляем все слайдеры/значения в DOM, не трогая фокус.
  document
    .querySelectorAll('#active-outputs input[data-kind="latency"]')
    .forEach((el) => {
      if (parseInt(el.value, 10) !== ms) el.value = String(ms);
    });
  document
    .querySelectorAll('#active-outputs [data-row="latency-v"]')
    .forEach((el) => {
      el.textContent = `${ms} ms`;
    });
  invoke("set_all_latencies", { latencyMs: ms })
    .then(() => {
      refreshTargetLatency();
      persistSession();
    })
    .catch((err) => setStatus(String(err), "err"));
}

async function refreshPeaks() {
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

function refreshDefaultSourceWarning() {
  const el = document.getElementById("default-source-warning");
  if (!el) return;
  if (state.engineRunning && state.loopbackSource) {
    el.textContent = tFmt("default-source-warning", {
      source: state.loopbackSource,
    });
    el.classList.remove("hidden");
  } else {
    el.classList.add("hidden");
    el.textContent = "";
  }
}

async function refreshTargetLatency() {
  const el = document.getElementById("target-latency");
  if (!state.engineRunning || state.active.length === 0) {
    el.classList.add("hidden");
    el.textContent = "";
    return;
  }
  try {
    const ms = await invoke("target_latency_ms");
    el.textContent = tFmt("sync-target-latency", { ms });
    el.classList.remove("hidden");
  } catch (_) {
    el.classList.add("hidden");
    el.textContent = "";
  }
}

// ------------- drift indicator -------------

async function refreshSyncStatus() {
  const el = document.getElementById("sync-status");
  if (!state.engineRunning || state.active.length < 2) {
    el.classList.add("hidden");
    return;
  }
  let snap;
  try {
    snap = await invoke("sync_status");
  } catch (_) {
    el.classList.add("hidden");
    return;
  }
  el.classList.remove("hidden");
  if (snap.active_count < 2) {
    el.textContent = t("sync-status-na");
    el.classList.remove("synced", "warn");
    return;
  }
  if (snap.in_sync) {
    el.textContent = t("sync-status-synced");
    el.classList.add("synced");
    el.classList.remove("warn");
  } else {
    el.textContent = tFmt("sync-status-drift", { ms: snap.drift_ms });
    el.classList.add("warn");
    el.classList.remove("synced");
  }
}

// ------------- test signal -------------

function formatTestRunning(running) {
  const kind = t("test-kind-" + running.kind);
  if (running.kind === "metronome" && running.bpm != null) {
    return tFmt("test-running-bpm", { kind, bpm: running.bpm });
  }
  return tFmt("test-running", { kind });
}

function closePopover() {
  if (state.activePopover) {
    state.activePopover.remove();
    state.activePopover = null;
  }
}

function openTestPopover(row, deviceName) {
  closePopover();
  const pop = document.createElement("div");
  pop.className = "test-popover";
  pop.innerHTML = `
    <button data-kind="click"></button>
    <button data-kind="sine"></button>
    <button data-kind="metronome"></button>
    <div class="metronome-form hidden">
      <label></label>
      <div class="bpm-row">
        <input type="range" min="40" max="240" value="120" />
        <span class="bpm-value">120 BPM</span>
      </div>
      <button class="bpm-start"></button>
    </div>
  `;
  pop.querySelector('[data-kind="click"]').textContent = t("test-kind-click");
  pop.querySelector('[data-kind="sine"]').textContent = t("test-kind-sine");
  pop.querySelector('[data-kind="metronome"]').textContent = t(
    "test-kind-metronome",
  );
  pop.querySelector(".metronome-form label").textContent = t("test-bpm-label");
  pop.querySelector(".metronome-form .bpm-start").textContent = t("test-start");

  pop.querySelector('[data-kind="click"]').addEventListener("click", () => {
    startTest(deviceName, "click");
    closePopover();
  });
  pop.querySelector('[data-kind="sine"]').addEventListener("click", () => {
    startTest(deviceName, "sine");
    closePopover();
  });
  pop.querySelector('[data-kind="metronome"]').addEventListener("click", () => {
    pop.querySelector(".metronome-form").classList.remove("hidden");
  });
  const bpmInput = pop.querySelector(".metronome-form input[type=range]");
  const bpmValue = pop.querySelector(".bpm-value");
  bpmInput.addEventListener("input", () => {
    bpmValue.textContent = `${bpmInput.value} BPM`;
  });
  pop.querySelector(".bpm-start").addEventListener("click", () => {
    const bpm = parseInt(bpmInput.value, 10);
    startTest(deviceName, "metronome", bpm);
    closePopover();
  });

  // Не закрываемся по клику внутри popover
  pop.addEventListener("click", (e) => e.stopPropagation());
  row.appendChild(pop);
  state.activePopover = pop;
}

async function startTest(deviceName, kind, bpm = null) {
  try {
    await invoke("play_test_signal", {
      deviceName,
      kind,
      bpm: kind === "metronome" ? bpm : null,
    });
    state.testRunning.set(deviceName, { kind, bpm });
    renderDevices();
  } catch (e) {
    const msg = String(e);
    const friendly = msg.includes("feedback-default-blocked")
      ? t("feedback-default-blocked")
      : msg;
    setStatus(friendly, "err");
  }
}

async function stopTest(deviceName) {
  try {
    await invoke("stop_test_signal", { deviceName });
  } catch (_) {
    // ignore
  }
  state.testRunning.delete(deviceName);
  renderDevices();
}

async function stopAllTests() {
  const names = [...state.testRunning.keys()];
  for (const n of names) {
    try {
      await invoke("stop_test_signal", { deviceName: n });
    } catch (_) {}
  }
  state.testRunning.clear();
}

// ------------- master controls -------------

async function loadMasterState() {
  try {
    const m = await invoke("master_state");
    state.master.gain = m.gain;
    state.master.muted = m.muted;
  } catch (_) {
    state.master = { gain: 1.0, muted: false };
  }
  applyMasterUI();
}

function applyMasterUI() {
  const range = document.getElementById("master-gain");
  const mute = document.getElementById("master-mute");
  const value = document.getElementById("master-gain-value");
  if (!range || !mute || !value) return;
  range.value = String(Math.round(state.master.gain * 100));
  mute.checked = state.master.muted;
  value.textContent = `${Math.round(state.master.gain * 100)}%`;
}

// ------------- session persist / restore -------------
//
// Раньше сессия лежала в localStorage; в v0.4 переехала в backend
// (session.json в app data dir). localStorage используется как fallback
// при первом запуске после миграции — читаем legacy-payload, сохраняем
// в backend, очищаем localStorage.

let persistTimer = null;

function buildProfile() {
  return {
    devices: state.active.map((a) => ({
      name: a.name,
      volume: a.volume,
      latency_ms: a.latency_ms,
      balance: a.balance ?? 0,
      muted: !!a.muted,
    })),
    master_gain: state.master.gain,
    master_muted: state.master.muted,
  };
}

function persistSession() {
  if (state.suspendPersist) return;
  if (persistTimer) clearTimeout(persistTimer);
  persistTimer = setTimeout(async () => {
    persistTimer = null;
    const profile = buildProfile();
    try {
      await invoke("save_session_profile", profile);
    } catch (e) {
      console.warn("session save failed", e);
    }
  }, PROFILE_PERSIST_DEBOUNCE_MS);
}

async function loadSession() {
  // 1. Backend-профайл (новый путь).
  try {
    const profile = await invoke("load_session_profile");
    if (profile && Array.isArray(profile.devices)) {
      state.master.gain = profile.master_gain ?? 1.0;
      state.master.muted = !!profile.master_muted;
      applyMasterUI();
      // Применить master до restore — устройства сразу заиграют с нужным
      // gain.
      try { await invoke("set_master_gain", { gain: state.master.gain }); } catch (_) {}
      try { await invoke("set_master_muted", { muted: state.master.muted }); } catch (_) {}
      return profile.devices.filter((d) => d && typeof d.name === "string");
    }
  } catch (e) {
    console.warn("load_session_profile failed", e);
  }
  // 2. Legacy: localStorage. Если есть — мигрируем и удаляем.
  try {
    const raw = localStorage.getItem(SESSION_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    localStorage.removeItem(SESSION_KEY);
    if (!Array.isArray(parsed)) return [];
    return parsed.filter((d) => d && typeof d.name === "string");
  } catch (_) {
    localStorage.removeItem(SESSION_KEY);
    return [];
  }
}

async function restoreSession() {
  const saved = await loadSession();
  if (saved.length === 0) return;
  state.suspendPersist = true;
  const failures = [];
  for (const item of saved) {
    try {
      const id = await invoke("add_output", { deviceName: item.name });
      // Channels — берём из state.devices, чтобы знать stereo/mono.
      const devInfo = state.devices.find((d) => d.name === item.name);
      const channels = devInfo ? devInfo.channels : 2;
      const newActive = {
        id,
        name: item.name,
        volume: item.volume ?? 1.0,
        latency_ms: item.latency_ms ?? 20,
        balance: item.balance ?? 0,
        muted: !!item.muted,
        channels,
      };
      state.active.push(newActive);
      try {
        await invoke("set_output_volume", { id, volume: newActive.volume });
      } catch (_) {}
      try {
        await invoke("set_output_latency", {
          id,
          latencyMs: newActive.latency_ms,
        });
      } catch (_) {}
      if (channels === 2) {
        try {
          await invoke("set_output_balance", {
            id,
            balance: newActive.balance,
          });
        } catch (_) {}
      }
      if (newActive.muted) {
        try {
          await invoke("set_output_muted", { id, muted: true });
        } catch (_) {}
      }
      // Push EQ если он сохранён в localStorage для этого id.
      const eq = state.eq.get(id);
      if (eq && (eq.low || eq.mid || eq.high)) {
        try {
          await invoke("set_output_eq", {
            id,
            lowDb: eq.low,
            midDb: eq.mid,
            highDb: eq.high,
          });
        } catch (_) {}
      }
    } catch (e) {
      failures.push(item.name);
    }
  }
  state.suspendPersist = false;
  renderDevices();
  renderActives();
  refreshTargetLatency();

  if (failures.length === 1) {
    setStatus(tFmt("status-restore-failed-one", { name: failures[0] }), "warn");
  } else if (failures.length >= 2) {
    setStatus(
      tFmt("status-restore-partial", { count: failures.length }),
      "warn",
    );
  } else if (saved.length > 0) {
    setStatus(t("status-restore-ok"), "ok");
  }
  persistSession();
}

// ------------- actions -------------

async function addOutput(deviceName) {
  try {
    if (!state.engineRunning) {
      await startEngine({ skipRestore: true });
    }
    const id = await invoke("add_output", { deviceName });
    const devInfo = state.devices.find((d) => d.name === deviceName);
    const channels = devInfo ? devInfo.channels : 2;
    state.active.push({
      id,
      name: deviceName,
      volume: 1.0,
      latency_ms: 20,
      balance: 0,
      muted: false,
      channels,
    });
    setStatus(t("status-output-added") + ": " + deviceName, "ok");
    renderDevices();
    renderActives();
    refreshTargetLatency();
    persistSession();
  } catch (e) {
    const msg = String(e);
    const friendly =
      msg.includes("feedback-default-blocked")
        ? t("feedback-default-blocked")
        : msg;
    setStatus(friendly, "err");
  }
}

async function removeOutput(id) {
  try {
    // Если на этом устройстве играет тест — остановить.
    const a = state.active.find((x) => x.id === id);
    if (a && state.testRunning.has(a.name)) {
      await stopTest(a.name);
    }
    await invoke("remove_output", { id });
    state.active = state.active.filter((a) => a.id !== id);
    setStatus(t("status-output-removed"), "ok");
    renderDevices();
    renderActives();
    refreshTargetLatency();
    persistSession();
  } catch (e) {
    setStatus(String(e), "err");
  }
}

async function startEngine(options = {}) {
  const { skipRestore = false } = options;
  try {
    await invoke("start_engine");
    const status = await invoke("engine_status");
    state.engineRunning = status.running;
    state.loopbackSource = status.loopback_source;
    refreshEngineButton();
    renderDevices();
    refreshDefaultSourceWarning();
    setStatus(t("status-engine-started"), "ok");
    if (!skipRestore && state.active.length === 0) {
      await restoreSession();
    }
  } catch (e) {
    setStatus(String(e), "err");
  }
}

async function stopEngine() {
  try {
    // Остановить все тесты до остановки engine — чтобы не утечь stream'ы.
    await stopAllTests();
    // Сохранить session ДО очистки active.
    persistSession();
    await invoke("stop_engine");
    state.engineRunning = false;
    state.loopbackSource = null;
    state.active = [];
    refreshEngineButton();
    renderDevices();
    renderActives();
    refreshTargetLatency();
    refreshDefaultSourceWarning();
    setStatus(t("status-engine-stopped"), "ok");
  } catch (e) {
    setStatus(String(e), "err");
  }
}

// ------------- init -------------

async function init() {
  // Тема: theme inline-script уже выставил data-theme до загрузки CSS.
  // Здесь подгружаем сохранённое значение в state и обновляем кнопку.
  const savedTheme = localStorage.getItem(THEME_KEY);
  state.theme =
    savedTheme === "light" || savedTheme === "dark" ? savedTheme : "auto";
  applyTheme(state.theme);
  document
    .getElementById("theme-toggle")
    .addEventListener("click", cycleTheme);

  const langSelect = document.getElementById("lang");
  langSelect.addEventListener("change", async (e) => {
    state.lang = e.target.value;
    await loadDictionary(state.lang);
    applyTranslations();
  });

  document
    .getElementById("engine-toggle")
    .addEventListener("click", async () => {
      if (state.engineRunning) {
        await stopEngine();
      } else {
        await startEngine();
      }
    });

  // Latency link toggle. UI-only state (не пишем в profile — это
  // pref интерфейса, не часть аудио-сессии).
  const linkEl = document.getElementById("latency-link");
  state.latencyLinked = localStorage.getItem(LATENCY_LINK_KEY) === "1";
  linkEl.checked = state.latencyLinked;
  linkEl.addEventListener("change", (e) => {
    state.latencyLinked = e.target.checked;
    localStorage.setItem(LATENCY_LINK_KEY, state.latencyLinked ? "1" : "0");
    if (state.latencyLinked && state.active.length > 0) {
      // При включении сразу выравниваем по максимуму — иначе цель
      // компенсации скакнёт неожиданно.
      const target = state.active.reduce(
        (m, a) => Math.max(m, a.latency_ms || 0),
        0,
      );
      applyLinkedLatency(target);
    }
  });

  // Master controls
  const masterGain = document.getElementById("master-gain");
  const masterMute = document.getElementById("master-mute");
  const masterValue = document.getElementById("master-gain-value");
  masterGain.addEventListener("input", (e) => {
    const g = parseInt(e.target.value, 10) / 100;
    state.master.gain = g;
    masterValue.textContent = `${e.target.value}%`;
    invoke("set_master_gain", { gain: g })
      .then(persistSession)
      .catch((err) => setStatus(String(err), "err"));
  });
  masterMute.addEventListener("change", (e) => {
    state.master.muted = e.target.checked;
    invoke("set_master_muted", { muted: state.master.muted })
      .then(persistSession)
      .catch((err) => setStatus(String(err), "err"));
  });

  // Тоггл «показать все устройства»
  const showAll = document.getElementById("show-all-devices");
  state.showAllDevices = localStorage.getItem(SHOW_ALL_KEY) === "1";
  showAll.checked = state.showAllDevices;
  showAll.addEventListener("change", async (e) => {
    state.showAllDevices = e.target.checked;
    localStorage.setItem(SHOW_ALL_KEY, state.showAllDevices ? "1" : "0");
    await refreshDevices({ silent: true });
  });

  // Закрытие test-popover по клику вне или Esc
  document.addEventListener("mousedown", (e) => {
    if (state.activePopover && !state.activePopover.contains(e.target)) {
      // Не закрываем если кликнули по test-кнопке (она сама перерисует)
      if (!e.target.classList.contains("test-btn")) {
        closePopover();
      }
    }
  });
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape") closePopover();
  });

  loadDeviceGroups();
  loadDeviceEq();
  await loadDictionary(state.lang);
  // Считываем текущее состояние движка — на случай hot reload.
  try {
    const status = await invoke("engine_status");
    state.engineRunning = status.running;
    state.loopbackSource = status.loopback_source;
  } catch (_) {
    // no-op
  }

  await loadMasterState();
  applyTranslations();
  await refreshDevices({ silent: true });
  renderActives();
  refreshTargetLatency();
  setStatus(t("status-ready"));

  // Если engine уже запущен (hot reload) и active пусто — попробуем
  // восстановить сохранённую сессию.
  if (state.engineRunning && state.active.length === 0) {
    await restoreSession();
  }

  // Авто-обновление списка устройств + drift status.
  setInterval(async () => {
    await refreshDevices({ silent: true });
    await refreshSyncStatus();
  }, DEVICE_REFRESH_INTERVAL_MS);

  // Peak meters — отдельный быстрый interval (~12 fps).
  setInterval(refreshPeaks, PEAK_REFRESH_INTERVAL_MS);
}

document.addEventListener("DOMContentLoaded", init);
