// Top-level список устройств (с кнопками Add/Remove + test).
// DOM-контракт: .device[data-id][data-active][data-state] →
//   .dev-status, .dev-meta (.dev-name-row + .dev-tech), .dev-test, .action-btn,
//   опциональный .dev-warning (для state=source).

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
import { ic, deviceIcon } from "./icons.js";

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

/// Возвращает state-метку устройства для data-state. Приоритет важен:
/// source > input > default > neutral. Active рисуется отдельным атрибутом.
function deviceState(d, isSource) {
  if (isSource) return "source";
  if (d.isInputOnly) return "input";
  if (d.isDefault) return "default";
  return "neutral";
}

function tagsFor(d, isSource) {
  const tags = [];
  if (d.isDefault) tags.push(`<span class="tag tag-default">${t("tag-default")}</span>`);
  if (isSource) tags.push(`<span class="tag tag-source">${t("tag-source")}</span>`);
  if (d.isInputOnly) tags.push(`<span class="tag tag-input">${t("tag-input")}</span>`);
  return tags.join("");
}

function actionFor(d, active, isSource) {
  if (d.isInputOnly) {
    return { variant: "input", label: t("action-input-only"), disabled: true, title: t("device-input-only-note") };
  }
  if (isSource) {
    return { variant: "source", label: t("tag-source"), disabled: true, title: t("device-source-note") };
  }
  if (active) {
    return { variant: "remove", label: t("device-remove"), disabled: false, title: "" };
  }
  return { variant: "add", label: t("device-add"), disabled: false, title: "" };
}

export function renderDevices() {
  const root = document.getElementById("devices");
  if (!root) return;
  state.activePopover = null;
  root.innerHTML = "";

  const visible = state.devices.filter(
    (d) => state.showAllDevices || !d.isInputOnly,
  );
  const countEl = document.getElementById("devices-count");
  if (countEl) countEl.textContent = String(visible.length);

  for (const d of visible) {
    const active = state.active.find((a) => a.id === d.id);
    const isSource =
      state.loopbackSource !== null && d.name === state.loopbackSource;
    const stateAttr = deviceState(d, isSource);
    const shown = displayName(d);
    const titleAttr =
      shown === d.name ? d.name : `${shown}\n(${d.name})`;
    const aliasFragment =
      shown === d.name
        ? ""
        : ` <span class="dev-alias">· ${escapeHtml(d.name)}</span>`;
    const tech = `${(d.sampleRate / 1000).toFixed(1)} kHz · ${
      d.channels === 1 ? "mono" : "stereo"
    } · ${d.isInputOnly ? "input" : "output"}`;

    const action = actionFor(d, active, isSource);
    const running = state.testRunning.get(d.name);
    const testDisabled = d.isInputOnly || isSource;
    const testIcon = running ? "i-stop" : "i-test";

    const row = document.createElement("div");
    row.className = "device";
    row.dataset.id = d.id;
    row.dataset.active = active ? "true" : "false";
    row.dataset.state = stateAttr;
    row.innerHTML = `
      <div class="dev-status" aria-hidden="true">${ic(deviceIcon(d))}</div>
      <div class="dev-meta">
        <div class="dev-name-row">
          <span class="dev-name" title="${escapeAttr(titleAttr)}">${escapeHtml(shown)}${aliasFragment}</span>
          <span class="dev-tags">${tagsFor(d, isSource)}</span>
        </div>
        <div class="dev-tech">${escapeHtml(tech)}</div>
      </div>
      <button class="dev-test" type="button"
              data-test="${d.id}" data-playing="${running ? "true" : "false"}"
              ${testDisabled ? "disabled" : ""}
              title="${escapeAttr(
                running
                  ? formatTestRunning(running)
                  : isSource
                  ? t("test-source-disabled")
                  : t("test-button-title"),
              )}">
        ${ic(testIcon)}
      </button>
      <button class="action-btn" type="button"
              data-action="${d.id}" data-variant="${action.variant}"
              ${action.disabled ? "disabled" : ""}
              ${action.title ? `title="${escapeAttr(action.title)}"` : ""}>
        ${escapeHtml(action.label)}
      </button>
      ${
        isSource
          ? `<div class="dev-warning" role="note">
              ${ic("i-alert")}
              <div>
                <span class="w-title">${t("warning-source-title")}</span>
                ${t("warning-source-desc")}
              </div>
             </div>`
          : ""
      }
    `;

    // Test-button: либо открыть popover, либо остановить уже играющее.
    const testBtn = row.querySelector(".dev-test");
    if (!testDisabled) {
      if (running) {
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

    // Action: Add / Remove. source/input — disabled.
    if (!action.disabled) {
      const actBtn = row.querySelector(".action-btn");
      actBtn.addEventListener("click", () => {
        if (active) removeOutput(d.id);
        else addOutput(d.name, d.endpointId);
      });
    }

    // ПКМ → контекстное меню переименования. Только если есть endpointId.
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
    const shown = displayName({ name: deviceName, endpointId });
    setStatus(`${t("status-output-added")}: ${shown}`, "ok");
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

function escapeHtml(s) {
  return String(s)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}
function escapeAttr(s) {
  return escapeHtml(s).replace(/\n/g, "&#10;");
}
