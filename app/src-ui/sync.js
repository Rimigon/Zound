// Синхронизация: drift sparkline + target latency + linked-toggle в одном
// виджете в шапке микшера. + default-source-warning (теперь рендерится
// inline на карточке устройства, в devices.js).

import { state } from "./state.js";
import { invoke } from "./ipc.js";
import { t, tFmt } from "./i18n.js";
import { setStatus } from "./status.js";
import { ic } from "./icons.js";

const SPARK_LEN = 20;
const driftHistory = new Array(SPARK_LEN).fill(0);

function pushDriftSample(ms) {
  driftHistory.shift();
  driftHistory.push(ms);
}

function tplSyncWidget(drift, target, status) {
  const linked = !!state.latencyLinked;
  const bars = driftHistory
    .map((v) => {
      const h = Math.min(100, Math.max(10, Math.abs(v) * 12));
      return `<span style="height:${h}%"></span>`;
    })
    .join("");
  const driftStr = (drift >= 0 ? "+" : "") + Math.round(drift);
  return `
    <div class="sync-widget" data-status="${status}">
      <span class="sync-label">${t("nav-sync")}</span>
      <div class="sync-spark" aria-hidden="true">${bars}</div>
      <span class="sync-readout">
        <span class="label">${t("sync-drift")}</span><span class="num">${driftStr}</span><span class="unit">ms</span><span class="gap"></span><span class="label">${t("sync-target")}</span><span class="num">${target == null ? "—" : Math.round(target)}</span><span class="unit">ms</span>
      </span>
      <button class="sync-link" id="latency-link" type="button" aria-pressed="${linked}" data-i18n-title="latency-link-title">
        ${ic(linked ? "i-link" : "i-unlink")} <span>${t("latency-link-label")}</span>
      </button>
    </div>
  `;
}

let lastTarget = null;
let lastStatus = "ok"; // 'ok' | 'drift' | 'na'

export function renderSyncWidget() {
  const mount = document.getElementById("sync-widget-mount");
  if (!mount) return;
  if (!state.engineRunning || state.active.length < 1) {
    mount.innerHTML = "";
    return;
  }
  const drift = driftHistory[driftHistory.length - 1] ?? 0;
  mount.innerHTML = tplSyncWidget(drift, lastTarget, lastStatus);
  const link = document.getElementById("latency-link");
  if (link) {
    link.addEventListener("click", () => {
      state.latencyLinked = !state.latencyLinked;
      try {
        localStorage.setItem("zound.latencyLinked", state.latencyLinked ? "1" : "0");
      } catch (_) {}
      link.setAttribute("aria-pressed", String(state.latencyLinked));
      if (state.latencyLinked && state.active.length > 0) {
        const target = state.active.reduce(
          (m, a) => Math.max(m, a.latencyMs || 0),
          0,
        );
        applyLinkedLatency(target);
      }
      renderSyncWidget();
    });
  }
}

/// Только обновить sparkline без полного rebuild — для частого polling.
function updateSparkInPlace() {
  const widget = document.querySelector(".sync-widget");
  if (!widget) return;
  widget.dataset.status = lastStatus;
  const bars = widget.querySelectorAll(".sync-spark span");
  bars.forEach((b, i) => {
    const v = driftHistory[i] ?? 0;
    const h = Math.min(100, Math.max(10, Math.abs(v) * 12));
    b.style.height = `${h}%`;
  });
  const readouts = widget.querySelectorAll(".sync-readout .num");
  if (readouts[0]) {
    const drift = driftHistory[driftHistory.length - 1] ?? 0;
    readouts[0].textContent = (drift >= 0 ? "+" : "") + Math.round(drift);
  }
  if (readouts[1]) {
    readouts[1].textContent = lastTarget == null ? "—" : String(Math.round(lastTarget));
  }
}

export async function refreshTargetLatency() {
  if (!state.engineRunning || state.active.length === 0) {
    lastTarget = null;
    updateSparkInPlace();
    return;
  }
  try {
    lastTarget = await invoke("target_latency_ms");
  } catch (_) {
    lastTarget = null;
  }
  updateSparkInPlace();
}

export async function refreshSyncStatus() {
  if (!state.engineRunning || state.active.length < 2) {
    lastStatus = "ok";
    pushDriftSample(0);
    updateSparkInPlace();
    return;
  }
  let snap;
  try {
    snap = await invoke("sync_status");
  } catch (_) {
    return;
  }
  if (snap.activeCount < 2) {
    lastStatus = "ok";
    pushDriftSample(0);
  } else if (snap.inSync) {
    lastStatus = "ok";
    pushDriftSample(snap.driftMs ?? 0);
  } else {
    lastStatus = "drift";
    pushDriftSample(snap.driftMs ?? 0);
  }
  updateSparkInPlace();
}

/// default-source-warning теперь рендерится inline на карточке source-
/// устройства в `devices.js`. Этот вызов сохранён для совместимости с
/// app.js → перерендерит список.
export function refreshDefaultSourceWarning() {
  // no-op: inline-warning рисуется в devices.js при renderDevices().
}

/// Применить одну и ту же задержку ко всем активным устройствам сразу.
/// Backend получает один `set_all_latencies` — атомарно для SyncEngine.
export function applyLinkedLatency(ms, persist) {
  for (const a of state.active) a.latencyMs = ms;
  document
    .querySelectorAll('#active-outputs input[data-lat]')
    .forEach((el) => {
      if (parseInt(el.value, 10) !== ms) el.value = String(ms);
    });
  document
    .querySelectorAll('#active-outputs [data-lat-v]')
    .forEach((el) => {
      el.textContent = String(ms);
    });
  invoke("set_all_latencies", { latencyMs: ms })
    .then(() => {
      refreshTargetLatency();
      persist?.();
    })
    .catch((err) => setStatus(String(err), "err"));
}
