// Синхронизация: drift status, target latency, default-source warning,
// linked latency.

import { state } from "./state.js";
import { invoke } from "./ipc.js";
import { t, tFmt } from "./i18n.js";
import { setStatus } from "./status.js";

export async function refreshTargetLatency() {
  const el = document.getElementById("target-latency");
  if (!el) return;
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

export async function refreshSyncStatus() {
  const el = document.getElementById("sync-status");
  if (!el) return;
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
  // Backend отдаёт camelCase: inSync / driftMs / activeCount.
  if (snap.activeCount < 2) {
    el.textContent = t("sync-status-na");
    el.classList.remove("synced", "warn");
    return;
  }
  if (snap.inSync) {
    el.textContent = t("sync-status-synced");
    el.classList.add("synced");
    el.classList.remove("warn");
  } else {
    el.textContent = tFmt("sync-status-drift", { ms: snap.driftMs });
    el.classList.add("warn");
    el.classList.remove("synced");
  }
}

export function refreshDefaultSourceWarning() {
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

/// Применить одну и ту же задержку ко всем активным устройствам сразу.
/// На backend идёт один `set_all_latencies` — атомарно для SyncEngine.
export function applyLinkedLatency(ms, persist) {
  for (const a of state.active) a.latencyMs = ms;
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
      persist?.();
    })
    .catch((err) => setStatus(String(err), "err"));
}
