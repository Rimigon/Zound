// Zound UI — vanilla JS, IPC через window.__TAURI__.core.invoke.

const invoke = (cmd, args) => window.__TAURI__.core.invoke(cmd, args);

const state = {
  engineRunning: false,
  loopbackSource: null, // имя default-устройства, источник захвата
  devices: [], // устройства, отображённые сейчас в верхнем списке
  active: [], // {id, name, volume, latency_ms}
  lang: "ru",
  dict: {},
  showAllDevices: false, // false → list_outputs, true → list_all_devices
};

const SHOW_ALL_KEY = "zound.showAllDevices";

const DEVICE_REFRESH_INTERVAL_MS = 2000;

// ------------- i18n -------------

async function loadDictionary(lang) {
  state.dict = await invoke("load_dictionary", { lang });
}

function t(key) {
  return state.dict[key] ?? key;
}

function applyTranslations() {
  document.querySelectorAll("[data-i18n]").forEach((el) => {
    const key = el.getAttribute("data-i18n");
    el.textContent = t(key);
  });
  document.documentElement.lang = state.lang;
  refreshEngineButton();
  renderDevices();
  renderActives();
  refreshTargetLatency();
}

function refreshEngineButton() {
  const btn = document.getElementById("engine-toggle");
  btn.textContent = state.engineRunning ? t("engine-stop") : t("engine-start");
  btn.classList.toggle("danger", state.engineRunning);
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
      <button></button>
    `;
    row.querySelector(".name").textContent = d.name;

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

    const btn = row.querySelector("button");
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

function renderActives() {
  const root = document.getElementById("active-outputs");
  root.innerHTML = "";
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

  for (const a of state.active) {
    const row = document.createElement("div");
    row.className = "active-row";
    row.innerHTML = `
      <div class="info">
        <div class="name"></div>
        <div class="sliders">
          <label></label>
          <input type="range" min="0" max="100" value="${Math.round(a.volume * 100)}" data-kind="volume" />
          <span class="value">${Math.round(a.volume * 100)}%</span>

          <label></label>
          <input type="range" min="0" max="500" value="${a.latency_ms}" data-kind="latency" />
          <span class="value">${a.latency_ms} ms</span>
        </div>
      </div>
      <button></button>
    `;
    row.querySelector(".name").textContent = a.name;
    const labels = row.querySelectorAll(".sliders label");
    labels[0].textContent = t("volume-label");
    labels[1].textContent = t("latency-label");

    const volEl = row.querySelector('input[data-kind="volume"]');
    const latEl = row.querySelector('input[data-kind="latency"]');
    const values = row.querySelectorAll(".sliders .value");

    volEl.addEventListener("input", (e) => {
      const v = parseInt(e.target.value, 10) / 100;
      a.volume = v;
      values[0].textContent = `${e.target.value}%`;
      invoke("set_output_volume", { id: a.id, volume: v }).catch((err) =>
        setStatus(String(err), "err"),
      );
    });
    latEl.addEventListener("input", (e) => {
      const ms = parseInt(e.target.value, 10);
      a.latency_ms = ms;
      values[1].textContent = `${ms} ms`;
      invoke("set_output_latency", { id: a.id, latencyMs: ms })
        .then(refreshTargetLatency)
        .catch((err) => setStatus(String(err), "err"));
    });

    const rm = row.querySelector("button");
    rm.textContent = t("device-remove");
    rm.addEventListener("click", () => removeOutput(a.id));
    root.appendChild(row);
  }
}

async function refreshTargetLatency() {
  const ms = await invoke("target_latency_ms");
  const raw = t("sync-target-latency");
  // Fluent возвращает плейсхолдер как "{ $ms }" — заменяем вручную.
  const text = raw.replace(/\{\s*\$ms\s*\}/g, ms);
  document.getElementById("target-latency").textContent = text;
}

// ------------- actions -------------

async function addOutput(deviceName) {
  try {
    if (!state.engineRunning) {
      await startEngine();
    }
    const id = await invoke("add_output", { deviceName });
    state.active.push({ id, name: deviceName, volume: 1.0, latency_ms: 20 });
    setStatus(t("status-output-added") + ": " + deviceName, "ok");
    renderDevices();
    renderActives();
    refreshTargetLatency();
  } catch (e) {
    // Если бэк вернул код-маркер обратной связи — переводим его.
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
    await invoke("remove_output", { id });
    state.active = state.active.filter((a) => a.id !== id);
    setStatus(t("status-output-removed"), "ok");
    renderDevices();
    renderActives();
    refreshTargetLatency();
  } catch (e) {
    setStatus(String(e), "err");
  }
}

async function startEngine() {
  try {
    await invoke("start_engine");
    const status = await invoke("engine_status");
    state.engineRunning = status.running;
    state.loopbackSource = status.loopback_source;
    refreshEngineButton();
    renderDevices();
    setStatus(t("status-engine-started"), "ok");
  } catch (e) {
    setStatus(String(e), "err");
  }
}

async function stopEngine() {
  try {
    await invoke("stop_engine");
    state.engineRunning = false;
    state.loopbackSource = null;
    state.active = [];
    refreshEngineButton();
    renderDevices();
    renderActives();
    refreshTargetLatency();
    setStatus(t("status-engine-stopped"), "ok");
  } catch (e) {
    setStatus(String(e), "err");
  }
}

// ------------- init -------------

async function init() {
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

  // Тоггл «показать все устройства» (включая input-only — микрофоны
  // и т.п.). По умолчанию — только outputs. Состояние запоминаем
  // в localStorage, чтобы пережить перезапуск.
  const showAll = document.getElementById("show-all-devices");
  state.showAllDevices = localStorage.getItem(SHOW_ALL_KEY) === "1";
  showAll.checked = state.showAllDevices;
  showAll.addEventListener("change", async (e) => {
    state.showAllDevices = e.target.checked;
    localStorage.setItem(SHOW_ALL_KEY, state.showAllDevices ? "1" : "0");
    await refreshDevices({ silent: true });
  });

  await loadDictionary(state.lang);
  // Считываем текущее состояние движка — на случай hot reload.
  try {
    const status = await invoke("engine_status");
    state.engineRunning = status.running;
    state.loopbackSource = status.loopback_source;
  } catch (_) {
    // no-op
  }

  applyTranslations();
  await refreshDevices({ silent: true });
  renderActives();
  refreshTargetLatency();
  setStatus(t("status-ready"));

  // Авто-обновление списка устройств.
  setInterval(() => refreshDevices({ silent: true }), DEVICE_REFRESH_INTERVAL_MS);
}

document.addEventListener("DOMContentLoaded", init);
