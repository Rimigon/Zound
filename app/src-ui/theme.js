// Темы: 7 палитр + auto-режим. Кнопка-палитра в шапке открывает picker
// с свотчами и отдельным чекбоксом «следить за системой». Если auto
// включен — атрибут data-theme снимается, и палитру выбирает
// `prefers-color-scheme` в style.css. Иначе data-theme = state.theme.
//
// Inline-script в index.html применяет тему до загрузки CSS, чтобы не
// мигало; этот модуль владеет полным жизненным циклом дальше.

import { state, KEYS } from "./state.js";

export const THEMES = [
  "dark",
  "light",
  "midnight",
  "sunset",
  "forest",
  "ocean",
  "mono",
];

export function loadThemeFromStorage() {
  try {
    state.themeAuto = localStorage.getItem(KEYS.themeAuto) === "1";
    const saved = localStorage.getItem(KEYS.theme);
    state.theme = THEMES.includes(saved) ? saved : "dark";
  } catch (_) {
    state.themeAuto = false;
    state.theme = "dark";
  }
}

export function applyTheme() {
  if (state.themeAuto) {
    delete document.documentElement.dataset.theme;
  } else {
    document.documentElement.dataset.theme = state.theme;
  }
  syncToggleButton();
}

export function setTheme(name) {
  if (!THEMES.includes(name)) return;
  state.theme = name;
  state.themeAuto = false;
  try {
    localStorage.setItem(KEYS.theme, name);
    localStorage.removeItem(KEYS.themeAuto);
  } catch (_) {}
  applyTheme();
}

export function setThemeAuto(enabled) {
  state.themeAuto = !!enabled;
  try {
    if (state.themeAuto) localStorage.setItem(KEYS.themeAuto, "1");
    else localStorage.removeItem(KEYS.themeAuto);
  } catch (_) {}
  applyTheme();
}

function syncToggleButton() {
  const btn = document.getElementById("theme-toggle");
  if (!btn) return;
  // Кнопка-палитра одинаковая (🎨) — picker раскрывается по клику; пусть
  // и активный пресет, и auto-режим читаются текстом подсказки.
  btn.textContent = state.themeAuto ? "🖥" : "🎨";
}
