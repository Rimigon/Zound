// Zound UI entry. ES-modules, импорты разбиты по доменным файлам:
// state / ipc / i18n / theme / devices / mixer / sync / tests / session /
// events. Сюда сведена только wiring-логика init().

import { state, KEYS, INTERVALS } from "./state.js";
import { invoke, classifyError } from "./ipc.js";
import { loadDictionary, applyStaticTranslations, t } from "./i18n.js";
import { applyTheme, cycleTheme } from "./theme.js";
import { setStatus } from "./status.js";
import {
  refreshDevices,
  renderDevices,
  addOutput,
  removeOutput,
} from "./devices.js";
import {
  renderActives,
  refreshPeaks,
  loadMasterState,
  applyMasterUI,
} from "./mixer.js";
import {
  refreshTargetLatency,
  refreshSyncStatus,
  refreshDefaultSourceWarning,
  applyLinkedLatency,
} from "./sync.js";
import {
  closePopover,
  stopAllTests,
} from "./tests.js";
import { restoreSession, persistSession } from "./session.js";
import { pollEngineEvents, pollEngineStatus } from "./events.js";
import { loadAliases } from "./aliases.js";
import { startUpdateChecker } from "./updater.js";

function refreshEngineButton() {
  const btn = document.getElementById("engine-toggle");
  btn.textContent = state.engineRunning ? t("engine-stop") : t("engine-start");
  btn.classList.toggle("danger", state.engineRunning);
}

function applyAllTranslations() {
  applyStaticTranslations();
  refreshEngineButton();
  renderDevices();
  renderActives();
  refreshTargetLatency();
  refreshDefaultSourceWarning();
}

async function startEngine(opts = {}) {
  const { skipRestore = false } = opts;
  try {
    await invoke("start_engine");
    const status = await invoke("engine_status");
    state.engineRunning = !!status.running;
    state.engineAlive = !!status.alive;
    state.loopbackSource = status.loopbackSource ?? null;
    refreshEngineButton();
    renderDevices();
    refreshDefaultSourceWarning();
    setStatus(t("status-engine-started"), "ok");
    if (!skipRestore && state.active.length === 0) {
      await restoreSession(renderDevices, renderActives);
    }
  } catch (e) {
    const c = classifyError(e);
    setStatus(c.message, "err");
  }
}

async function stopEngine() {
  try {
    await stopAllTests();
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

async function init() {
  // Тема: inline-script в index.html уже выставил атрибут до загрузки CSS.
  const savedTheme = localStorage.getItem(KEYS.theme);
  state.theme =
    savedTheme === "light" || savedTheme === "dark" ? savedTheme : "auto";
  applyTheme(state.theme);
  document.getElementById("theme-toggle").addEventListener("click", cycleTheme);

  document.getElementById("lang").addEventListener("change", async (e) => {
    state.lang = e.target.value;
    await loadDictionary(state.lang);
    applyAllTranslations();
  });

  document
    .getElementById("engine-toggle")
    .addEventListener("click", async () => {
      if (state.engineRunning) await stopEngine();
      else await startEngine();
    });

  // Linked latencies — UI-only.
  const linkEl = document.getElementById("latency-link");
  state.latencyLinked = localStorage.getItem(KEYS.latencyLink) === "1";
  linkEl.checked = state.latencyLinked;
  linkEl.addEventListener("change", (e) => {
    state.latencyLinked = e.target.checked;
    localStorage.setItem(KEYS.latencyLink, state.latencyLinked ? "1" : "0");
    if (state.latencyLinked && state.active.length > 0) {
      const target = state.active.reduce(
        (m, a) => Math.max(m, a.latencyMs || 0),
        0,
      );
      applyLinkedLatency(target, persistSession);
    }
  });

  // Master controls.
  const masterGain = document.getElementById("master-gain");
  const masterMute = document.getElementById("master-mute");
  const masterValue = document.getElementById("master-gain-value");
  masterGain.addEventListener("input", (e) => {
    const g = parseInt(e.target.value, 10) / 100;
    state.master.gain = g;
    masterValue.textContent = `${e.target.value}%`;
    invoke("set_master_gain", { gain: g });
    persistSession();
  });
  masterMute.addEventListener("change", (e) => {
    state.master.muted = e.target.checked;
    invoke("set_master_muted", { muted: state.master.muted });
    persistSession();
  });

  // «Показать все устройства».
  const showAll = document.getElementById("show-all-devices");
  state.showAllDevices = localStorage.getItem(KEYS.showAll) === "1";
  showAll.checked = state.showAllDevices;
  showAll.addEventListener("change", async (e) => {
    state.showAllDevices = e.target.checked;
    localStorage.setItem(KEYS.showAll, state.showAllDevices ? "1" : "0");
    await refreshDevices({ silent: true });
  });

  // Закрытие popover-а по mousedown снаружи / Esc.
  document.addEventListener("mousedown", (e) => {
    if (state.activePopover && !state.activePopover.contains(e.target)) {
      if (!e.target.classList.contains("test-btn")) closePopover();
    }
  });
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape") closePopover();
  });

  await loadDictionary(state.lang);
  await loadAliases();
  await pollEngineStatus();
  await loadMasterState();
  applyMasterUI();
  applyAllTranslations();
  await refreshDevices({ silent: true });
  renderActives();
  refreshTargetLatency();
  setStatus(t("status-ready"));

  if (state.engineRunning && state.active.length === 0) {
    await restoreSession(renderDevices, renderActives);
  }

  setInterval(async () => {
    await refreshDevices({ silent: true });
    await refreshSyncStatus();
  }, INTERVALS.deviceRefreshMs);

  setInterval(refreshPeaks, INTERVALS.peakRefreshMs);

  // P1.6 — engine event poll: watchdog disconnect, default-device
  // changed, panic. UI реагирует автоматически.
  setInterval(
    () => pollEngineEvents(renderDevices, renderActives),
    INTERVALS.eventsPollMs,
  );

  startUpdateChecker();
}

document.addEventListener("DOMContentLoaded", init);
