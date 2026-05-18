// Автообновление через tauri-plugin-updater. Floating toast в правом
// верхнем углу — не двигает контент. «Установить» → загрузка → перезапуск.
// «Позже» → версия записывается в localStorage и больше не всплывает,
// пока updater не найдёт более новую.

import { state, KEYS } from "./state.js";
import { t, tFmt } from "./i18n.js";
import { ic } from "./icons.js";

const tauri = () => window.__TAURI__ || {};
const updater = () => tauri().updater || {};
const proc = () => tauri().process || {};

let currentUpdate = null;

function dismissedVersion() {
  try {
    return localStorage.getItem(KEYS.updateDismissedVersion);
  } catch (_) {
    return null;
  }
}
function setDismissed(version) {
  try {
    localStorage.setItem(KEYS.updateDismissedVersion, version);
  } catch (_) {}
}

export async function checkForUpdate() {
  const check = updater().check;
  if (typeof check !== "function") return;
  try {
    const upd = await check();
    if (!upd || !upd.available) return;
    if (dismissedVersion() === upd.version) return;
    currentUpdate = upd;
    state.updateAvailable = { version: upd.version, body: upd.body || "" };
    renderToast();
  } catch (e) {
    console.warn("update check failed", e);
  }
}

function renderToast() {
  const toast = document.getElementById("update-toast");
  if (!toast) return;
  const av = state.updateAvailable;
  if (!av) {
    toast.hidden = true;
    toast.innerHTML = "";
    return;
  }
  toast.hidden = false;
  toast.innerHTML = `
    <div class="toast-head">
      <div class="toast-icon">${ic("i-download")}</div>
      <div class="toast-text">
        <div class="toast-title">${t("update-available-title")}</div>
        <div class="toast-version num">v${av.version}</div>
      </div>
      <button class="toast-close" type="button" aria-label="${escapeAttr(t("update-later"))}">${ic("i-x")}</button>
    </div>
    <div class="toast-progress" hidden>
      <div class="bar-fill" style="width:0%"></div>
    </div>
    <div class="toast-progress-label" hidden></div>
    <div class="toast-actions">
      <button type="button" class="secondary">${t("update-later")}</button>
      <button type="button" class="primary">${t("update-install")}</button>
    </div>
  `;
  toast.querySelector(".toast-close").addEventListener("click", () => dismiss());
  toast.querySelector(".secondary").addEventListener("click", () => dismiss());
  toast.querySelector(".primary").addEventListener("click", () => installCurrent());
}

function dismiss() {
  const av = state.updateAvailable;
  if (av) setDismissed(av.version);
  state.updateAvailable = null;
  renderToast();
}

async function installCurrent() {
  if (!currentUpdate) return;
  const toast = document.getElementById("update-toast");
  const progress = toast?.querySelector(".toast-progress");
  const fill = progress?.querySelector(".bar-fill");
  const label = toast?.querySelector(".toast-progress-label");
  const buttons = toast?.querySelectorAll(".toast-actions button");
  buttons?.forEach((b) => (b.disabled = true));
  if (progress) progress.hidden = false;
  if (label) label.hidden = false;

  let total = 0;
  let downloaded = 0;
  try {
    await currentUpdate.downloadAndInstall((event) => {
      if (!event) return;
      switch (event.event) {
        case "Started":
          total = event.data?.contentLength ?? 0;
          downloaded = 0;
          if (fill) fill.style.width = "0%";
          if (label) label.textContent = tFmt("update-downloading", { percent: 0 });
          break;
        case "Progress": {
          downloaded += event.data?.chunkLength ?? 0;
          const pct = total > 0 ? Math.floor((downloaded / total) * 100) : 0;
          if (fill) fill.style.width = pct + "%";
          if (label) label.textContent = tFmt("update-downloading", { percent: pct });
          break;
        }
        case "Finished":
          if (fill) fill.style.width = "100%";
          if (label) label.textContent = t("update-installing");
          break;
      }
    });
    const relaunch = proc().relaunch;
    if (typeof relaunch === "function") {
      await relaunch();
    }
  } catch (e) {
    console.warn("update install failed", e);
    if (label) label.textContent = t("update-failed");
    buttons?.forEach((b) => (b.disabled = false));
  }
}

export function startUpdateChecker() {
  setTimeout(() => checkForUpdate(), 5_000);
  setInterval(() => {
    if (!state.updateAvailable) checkForUpdate();
  }, 6 * 60 * 60 * 1000);
}

function escapeAttr(s) {
  return String(s ?? "")
    .replace(/&/g, "&amp;")
    .replace(/"/g, "&quot;")
    .replace(/\n/g, "&#10;");
}
