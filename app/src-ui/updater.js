// Авто-обновление через tauri-plugin-updater. Глобал-доступ
// (`window.__TAURI__.updater`) — это бесплатно при `withGlobalTauri: true`,
// поэтому фронт не тянет bundler/npm.
//
// UX: один баннер сверху страницы. «Установить» → загрузка → перезапуск.
// «Позже» → записываем версию в localStorage и не показываем баннер для
// неё; следующая более новая снова всплывает.

import { state, KEYS } from "./state.js";
import { t, tFmt } from "./i18n.js";

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
  if (typeof check !== "function") {
    // Плагин не подключён (debug-сборка без updater, headless и т.п.).
    return;
  }
  try {
    const upd = await check();
    if (!upd || !upd.available) return;
    if (dismissedVersion() === upd.version) return;
    currentUpdate = upd;
    state.updateAvailable = { version: upd.version, body: upd.body || "" };
    renderBanner();
  } catch (e) {
    console.warn("update check failed", e);
  }
}

function renderBanner() {
  const banner = document.getElementById("update-banner");
  if (!banner) return;
  const av = state.updateAvailable;
  if (!av) {
    banner.hidden = true;
    banner.innerHTML = "";
    return;
  }
  banner.hidden = false;
  banner.innerHTML = `
    <span class="text"></span>
    <span class="progress" hidden></span>
    <button class="later" type="button"></button>
    <button class="install primary" type="button"></button>
  `;
  banner.querySelector(".text").textContent = tFmt("update-available", {
    version: av.version,
  });
  const later = banner.querySelector(".later");
  const install = banner.querySelector(".install");
  later.textContent = t("update-later");
  install.textContent = t("update-install");

  later.addEventListener("click", () => {
    setDismissed(av.version);
    state.updateAvailable = null;
    renderBanner();
  });
  install.addEventListener("click", () => installCurrent());
}

async function installCurrent() {
  if (!currentUpdate) return;
  const banner = document.getElementById("update-banner");
  const progress = banner?.querySelector(".progress");
  const later = banner?.querySelector(".later");
  const install = banner?.querySelector(".install");
  if (install) install.disabled = true;
  if (later) later.disabled = true;
  if (progress) progress.hidden = false;

  let total = 0;
  let downloaded = 0;
  try {
    await currentUpdate.downloadAndInstall((event) => {
      if (!event || !progress) return;
      switch (event.event) {
        case "Started":
          total = event.data?.contentLength ?? 0;
          downloaded = 0;
          progress.textContent = tFmt("update-downloading", { percent: 0 });
          break;
        case "Progress": {
          downloaded += event.data?.chunkLength ?? 0;
          const pct = total > 0 ? Math.floor((downloaded / total) * 100) : 0;
          progress.textContent = tFmt("update-downloading", { percent: pct });
          break;
        }
        case "Finished":
          progress.textContent = t("update-installing");
          break;
      }
    });
    const relaunch = proc().relaunch;
    if (typeof relaunch === "function") {
      await relaunch();
    }
  } catch (e) {
    console.warn("update install failed", e);
    if (progress) progress.textContent = t("update-failed");
    if (install) install.disabled = false;
    if (later) later.disabled = false;
  }
}

/// Запускает проверку через короткую паузу после старта и далее раз в
/// 6 часов. Пауза — чтобы не конкурировать за CPU с инициализацией
/// audio-engine на первых секундах.
export function startUpdateChecker() {
  setTimeout(() => {
    checkForUpdate();
  }, 5_000);
  setInterval(() => {
    if (!state.updateAvailable) checkForUpdate();
  }, 6 * 60 * 60 * 1000);
}
