// Zound UI entry. Vanilla ES-modules, без bundler. Импорты по доменным
// файлам: state / ipc / i18n / theme / devices / mixer / sync / tests /
// session / events / updater. Здесь — только wiring + init().

import { state, KEYS, INTERVALS } from "./state.js";
import { invoke, classifyError } from "./ipc.js";
import { loadDictionary, applyStaticTranslations, t } from "./i18n.js";
import { applyTheme, loadThemeFromStorage } from "./theme.js";
import { openThemePicker } from "./theme-picker.js";
import { setStatus } from "./status.js";
import {
  refreshDevices,
  renderDevices,
} from "./devices.js";
import {
  renderActives,
  refreshPeaks,
  loadMasterState,
  applyMasterUI,
  loadDeviceGroups,
} from "./mixer.js";
import {
  refreshTargetLatency,
  refreshSyncStatus,
  refreshDefaultSourceWarning,
  renderSyncWidget,
} from "./sync.js";
import { closePopover, stopAllTests } from "./tests.js";
import { restoreSession, persistSession } from "./session.js";
import { pollEngineEvents, pollEngineStatus } from "./events.js";
import { loadAliases } from "./aliases.js";
import { startUpdateChecker } from "./updater.js";
import { injectIconSprite, ic } from "./icons.js";

function refreshEngineButton() {
  const btn = document.getElementById("engine-toggle");
  if (!btn) return;
  const running = state.engineRunning;
  btn.dataset.state = running ? "running" : "stopped";
  btn.innerHTML = running
    ? `<span class="live-dot" aria-hidden="true"></span><span class="engine-label">${t(
        "engine-stop",
      )}</span>`
    : `${ic("i-play")}<span class="engine-label">${t("engine-start")}</span>`;
}

function refreshLangToggle() {
  const btn = document.getElementById("lang-toggle");
  if (!btn) return;
  const ru = btn.querySelector(".ru-opt");
  const en = btn.querySelector(".en-opt");
  if (ru) ru.classList.toggle("active", state.lang === "ru");
  if (en) en.classList.toggle("active", state.lang === "en");
}

function applyAllTranslations() {
  applyStaticTranslations();
  refreshEngineButton();
  refreshLangToggle();
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
    renderActives();
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
    setStatus(t("status-engine-stopped"), "ok");
  } catch (e) {
    setStatus(String(e), "err");
  }
}

async function init() {
  injectIconSprite();

  loadThemeFromStorage();
  applyTheme();
  document
    .getElementById("theme-toggle")
    .addEventListener("click", openThemePicker);

  // Language toggle — кнопка-сегмент RU/EN.
  document.getElementById("lang-toggle").addEventListener("click", async () => {
    state.lang = state.lang === "ru" ? "en" : "ru";
    await loadDictionary(state.lang);
    applyAllTranslations();
  });

  // Engine.
  document
    .getElementById("engine-toggle")
    .addEventListener("click", async () => {
      if (state.engineRunning) await stopEngine();
      else await startEngine();
    });

  // Linked latencies — теперь живёт внутри sync-widget. Здесь — только
  // загрузка стартового состояния из localStorage. Тогл вешается при
  // каждом renderSyncWidget() в sync.js.
  state.latencyLinked = localStorage.getItem(KEYS.latencyLink) === "1";

  // Master controls — теперь .master в шапке.
  const masterGain = document.getElementById("master-gain");
  const masterMute = document.getElementById("master-mute");
  const masterFill = document.getElementById("master-fill");
  const masterValue = document.getElementById("master-gain-value");
  const masterMuteWrap = document.getElementById("master-mute-wrap");
  const masterMuteUse = masterMuteWrap?.querySelector(".master-mute-icon use");

  masterGain.addEventListener("input", (e) => {
    const pct = parseInt(e.target.value, 10);
    const g = pct / 100;
    state.master.gain = g;
    state.master.muted = false;
    if (masterMute.checked) masterMute.checked = false;
    if (masterMuteWrap) masterMuteWrap.dataset.muted = "false";
    if (masterMuteUse) masterMuteUse.setAttribute("href", "#i-unmute");
    masterValue.textContent = String(pct);
    if (masterFill) masterFill.style.setProperty("--val", pct);
    invoke("set_master_gain", { gain: g });
    invoke("set_master_muted", { muted: false });
    persistSession();
  });
  masterMute.addEventListener("change", (e) => {
    state.master.muted = e.target.checked;
    if (masterMuteWrap) masterMuteWrap.dataset.muted = String(state.master.muted);
    if (masterMuteUse) {
      masterMuteUse.setAttribute("href", state.master.muted ? "#i-mute" : "#i-unmute");
    }
    const pct = Math.round(state.master.gain * 100);
    masterValue.textContent = state.master.muted ? "—" : String(pct);
    if (masterFill)
      masterFill.style.setProperty("--val", state.master.muted ? 0 : pct);
    invoke("set_master_muted", { muted: state.master.muted });
    persistSession();
  });

  // Filter-chips «Outputs / Show inputs» в шапке панели устройств.
  const outBtn = document.getElementById("filter-outputs");
  const allBtn = document.getElementById("filter-all");
  state.showAllDevices = localStorage.getItem(KEYS.showAll) === "1";
  outBtn.setAttribute("aria-pressed", String(!state.showAllDevices));
  allBtn.setAttribute("aria-pressed", String(state.showAllDevices));
  outBtn.addEventListener("click", async () => {
    if (!state.showAllDevices) return;
    state.showAllDevices = false;
    localStorage.setItem(KEYS.showAll, "0");
    outBtn.setAttribute("aria-pressed", "true");
    allBtn.setAttribute("aria-pressed", "false");
    await refreshDevices({ silent: true });
    renderDevices();
  });
  allBtn.addEventListener("click", async () => {
    if (state.showAllDevices) return;
    state.showAllDevices = true;
    localStorage.setItem(KEYS.showAll, "1");
    outBtn.setAttribute("aria-pressed", "false");
    allBtn.setAttribute("aria-pressed", "true");
    await refreshDevices({ silent: true });
    renderDevices();
  });

  // Закрытие popover-а: mousedown снаружи / Esc.
  document.addEventListener("mousedown", (e) => {
    if (state.activePopover && !state.activePopover.contains(e.target)) {
      if (!e.target.closest(".dev-test")) closePopover();
    }
  });
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape") closePopover();
    // Space — toggle engine, если фокус не на input.
    if (e.code === "Space") {
      const tag = (document.activeElement?.tagName || "").toLowerCase();
      if (["input", "select", "textarea", "button"].includes(tag)) return;
      e.preventDefault();
      if (state.engineRunning) stopEngine();
      else startEngine();
    }
  });

  await loadDictionary(state.lang);
  await loadAliases();
  loadDeviceGroups();
  await pollEngineStatus();
  await loadMasterState();
  applyMasterUI();
  applyAllTranslations();
  await refreshDevices({ silent: true });
  renderActives();
  refreshTargetLatency();
  renderSyncWidget();
  setStatus(t("status-ready"));

  if (state.engineRunning && state.active.length === 0) {
    await restoreSession(renderDevices, renderActives);
  }

  setInterval(async () => {
    await refreshDevices({ silent: true });
    await refreshSyncStatus();
  }, INTERVALS.deviceRefreshMs);

  setInterval(refreshPeaks, INTERVALS.peakRefreshMs);

  setInterval(
    () => pollEngineEvents(renderDevices, renderActives),
    INTERVALS.eventsPollMs,
  );

  startUpdateChecker();
}

document.addEventListener("DOMContentLoaded", init);
