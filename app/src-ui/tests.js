// Тест-сигнал: popover с Click / Sine / Metronome, start/stop.

import { state } from "./state.js";
import { invoke, classifyError } from "./ipc.js";
import { t, tFmt } from "./i18n.js";
import { setStatus } from "./status.js";

export function formatTestRunning(running) {
  const kind = t("test-kind-" + running.kind);
  if (running.kind === "metronome" && running.bpm != null) {
    return tFmt("test-running-bpm", { kind, bpm: running.bpm });
  }
  return tFmt("test-running", { kind });
}

export function closePopover() {
  if (state.activePopover) {
    state.activePopover.remove();
    state.activePopover = null;
  }
}

export function openTestPopover(row, deviceName, onStart) {
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
    startTest(deviceName, "click").then(onStart);
    closePopover();
  });
  pop.querySelector('[data-kind="sine"]').addEventListener("click", () => {
    startTest(deviceName, "sine").then(onStart);
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
    startTest(deviceName, "metronome", bpm).then(onStart);
    closePopover();
  });

  pop.addEventListener("click", (e) => e.stopPropagation());
  row.appendChild(pop);
  state.activePopover = pop;
}

export async function startTest(deviceName, kind, bpm = null) {
  try {
    await invoke("play_test_signal", {
      deviceName,
      kind,
      bpm: kind === "metronome" ? bpm : null,
    });
    state.testRunning.set(deviceName, { kind, bpm });
  } catch (e) {
    const c = classifyError(e);
    const msg =
      c.kind === "feedbackBlocked" ? t("feedback-default-blocked") : c.message;
    setStatus(msg, "err");
  }
}

export async function stopTest(deviceName) {
  try {
    await invoke("stop_test_signal", { deviceName });
  } catch (_) {}
  state.testRunning.delete(deviceName);
}

export async function stopAllTests() {
  const names = [...state.testRunning.keys()];
  for (const n of names) {
    try {
      await invoke("stop_test_signal", { deviceName: n });
    } catch (_) {}
  }
  state.testRunning.clear();
}
