// i18n — один словарь (`state.dict`) на текущий язык, ключи читаются
// через `t()` / `tFmt()`. Backend отдаёт уже отформатированные строки
// без переменных; placeholder'ы вида `{ $name }` подставляем здесь.

import { state } from "./state.js";
import { invoke } from "./ipc.js";

export async function loadDictionary(lang) {
  state.dict = await invoke("load_dictionary", { lang });
}

export function t(key) {
  return state.dict[key] ?? key;
}

export function tFmt(key, args) {
  let raw = t(key);
  if (args) {
    for (const [k, v] of Object.entries(args)) {
      const re = new RegExp("\\{\\s*\\$" + k + "\\s*\\}", "g");
      raw = raw.replace(re, v);
    }
  }
  return raw;
}

/// Применить переводы ко всем `[data-i18n]` / `[data-i18n-title]`. Отдельные
/// рендеры (devices, active rows) сами читают `t()` при перерисовке.
export function applyStaticTranslations() {
  document.querySelectorAll("[data-i18n]").forEach((el) => {
    const key = el.getAttribute("data-i18n");
    el.textContent = t(key);
  });
  document.querySelectorAll("[data-i18n-title]").forEach((el) => {
    const key = el.getAttribute("data-i18n-title");
    el.title = t(key);
  });
  document.documentElement.lang = state.lang;
}
