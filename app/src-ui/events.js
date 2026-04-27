// Polling backend-эвентов. Не Tauri events (они тоже есть, но мы пока
// идём через простой `engine_events` poll — backend накапливает Vec
// и отдаёт батчами). Обрабатываем три типа:
//   - outputDisconnected: watchdog снял output (BT-наушники ушли).
//     Удаляем из state.active, перерисовываем, показываем статус.
//   - enginePanicked: audio-thread умер. UI блокируется баннером, на
//     все sliders disabled.
//   - defaultDeviceChanged: системный default render endpoint
//     поменялся. Подскажем в статусе и предложим перезапустить engine.

import { state } from "./state.js";
import { invoke } from "./ipc.js";
import { t } from "./i18n.js";
import { setStatus } from "./status.js";

export async function pollEngineEvents(renderDevices, renderActives) {
  let events;
  try {
    events = await invoke("engine_events");
  } catch (_) {
    return;
  }
  if (!Array.isArray(events) || events.length === 0) return;
  let needRerender = false;
  for (const ev of events) {
    if (ev.kind === "outputDisconnected") {
      const a = state.active.find((x) => x.id === ev.id);
      const name = a?.name ?? ev.id;
      state.active = state.active.filter((x) => x.id !== ev.id);
      setStatus(`${t("device-remove")} · ${name} (${ev.reason})`, "warn");
      needRerender = true;
    } else if (ev.kind === "enginePanicked") {
      state.engineAlive = false;
      state.engineRunning = false;
      setStatus(`engine died: ${ev.message}`, "err");
      needRerender = true;
    } else if (ev.kind === "defaultDeviceChanged") {
      // Soft-restart capture уже инициирован watcher'ом → engine
      // закрыл capture, мы синхронизируем UI.
      state.engineRunning = false;
      state.loopbackSource = null;
      setStatus("default device changed — restart engine to pick it up", "warn");
      needRerender = true;
    }
  }
  if (needRerender) {
    renderDevices();
    renderActives();
  }
}

export async function pollEngineStatus() {
  try {
    const status = await invoke("engine_status");
    state.engineAlive = !!status.alive;
    state.engineRunning = !!status.running;
    state.loopbackSource = status.loopbackSource ?? null;
  } catch (_) {}
}
