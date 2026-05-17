// Top-level список устройств (с кнопками Add/Remove + test 🔊).

import { state } from "./state.js";
import { invoke, classifyError } from "./ipc.js";
import { t } from "./i18n.js";
import { setStatus } from "./status.js";
import { openTestPopover, formatTestRunning, stopTest } from "./tests.js";
import { renderActives } from "./mixer.js";
import { refreshTargetLatency } from "./sync.js";
import { persistSession } from "./session.js";
import { displayName } from "./aliases.js";
import { openDeviceContextMenu } from "./device-menu.js";

function devicesEqual(a, b) {
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) {
    if (
      a[i].id !== b[i].id ||
      a[i].name !== b[i].name ||
      a[i].sampleRate !== b[i].sampleRate ||
      a[i].channels !== b[i].channels ||
      a[i].isDefault !== b[i].isDefault ||
      a[i].isInputOnly !== b[i].isInputOnly ||
      a[i].endpointId !== b[i].endpointId
    ) {
      return false;
    }
  }
  return true;
}

export async function refreshDevices(options = {}) {
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

export function renderDevices() {
  const root = document.getElementById("devices");
  if (!root) return;
  root.innerHTML = "";
  state.activePopover = null;
  for (const d of state.devices) {
    const active = state.active.find((a) => a.id === d.id);
    const isSource =
      state.loopbackSource !== null && d.name === state.loopbackSource;
    const inputOnly = !!d.isInputOnly;

    const row = document.createElement("div");
    row.className =
      "device-row" +
      (d.isDefault ? " default" : "") +
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
    const shown = displayName(d);
    nameEl.textContent = shown;
    nameEl.title = shown === d.name ? d.name : `${shown}\n(${d.name})`;

    const meta = row.querySelector(".meta");
    meta.textContent = `${d.sampleRate} Hz · ${d.channels} ch${
      d.isDefault ? " · default" : ""
    }`;
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
        testBtn.addEventListener("click", () =>
          stopTest(d.name).then(renderDevices),
        );
      } else {
        testBtn.addEventListener("click", (ev) => {
          ev.stopPropagation();
          openTestPopover(row, d.name, renderDevices);
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
      btn.addEventListener("click", () => addOutput(d.name, d.endpointId));
    }

    // ПКМ → контекстное меню переименования. Только для устройств со
    // стабильным endpointId — без него алиас не к чему привязать.
    if (d.endpointId) {
      row.addEventListener("contextmenu", (ev) => {
        ev.preventDefault();
        openDeviceContextMenu(ev.clientX, ev.clientY, d);
      });
    }

    root.appendChild(row);
  }
}

export async function addOutput(deviceName, endpointId) {
  try {
    if (!state.engineRunning) {
      // startEngine импортируется ленивым require, чтобы не делать
      // циклическую зависимость; вместо этого жёсткий call через invoke
      // и обновим state ниже.
      await invoke("start_engine");
      const status = await invoke("engine_status");
      state.engineRunning = !!status.running;
      state.loopbackSource = status.loopbackSource ?? null;
    }
    const res = await invoke("add_output", {
      deviceName,
      endpointId: endpointId ?? null,
    });
    const devInfo = state.devices.find((d) => d.name === deviceName);
    const channels = devInfo ? devInfo.channels : 2;
    state.active.push({
      id: res.id,
      name: deviceName,
      endpointId: res.endpointId ?? endpointId ?? null,
      volume: 1.0,
      latencyMs: 20,
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
    const c = classifyError(e);
    const msg =
      c.kind === "feedbackBlocked" ? t("feedback-default-blocked") : c.message;
    setStatus(msg, "err");
  }
}

export async function removeOutput(id) {
  try {
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
