// Тема: auto / dark / light. Inline-script в index.html ставит атрибут
// до загрузки CSS, чтобы избежать вспышки светлой; здесь — циклирование
// и persist в localStorage.

import { state, KEYS } from "./state.js";

export function applyTheme(theme) {
  state.theme = theme;
  document.documentElement.dataset.theme = theme === "auto" ? "" : theme;
  if (theme === "auto") {
    localStorage.removeItem(KEYS.theme);
  } else {
    localStorage.setItem(KEYS.theme, theme);
  }
  const btn = document.getElementById("theme-toggle");
  if (btn) {
    btn.textContent =
      theme === "light" ? "☀" : theme === "dark" ? "🌙" : "🌓";
  }
}

export function cycleTheme() {
  const next =
    state.theme === "auto"
      ? "dark"
      : state.theme === "dark"
        ? "light"
        : "auto";
  applyTheme(next);
}
