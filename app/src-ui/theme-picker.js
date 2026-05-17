// Theme picker: открывается клавишей-палитрой в шапке. Сетка свотчей с
// превью трёх цветов на тему + отдельная строка «следить за системой».
// Закрывается клавишей Esc / mousedown снаружи / scroll / resize.

import { state } from "./state.js";
import { t } from "./i18n.js";
import { THEMES, setTheme, setThemeAuto } from "./theme.js";

let activePicker = null;

function close() {
  if (activePicker) {
    activePicker.remove();
    activePicker = null;
    document.removeEventListener("mousedown", onOutside, true);
    document.removeEventListener("keydown", onKey, true);
    window.removeEventListener("scroll", close, true);
    window.removeEventListener("resize", close);
    window.removeEventListener("blur", close);
  }
}

function onOutside(ev) {
  if (activePicker && !activePicker.contains(ev.target)) {
    // Кнопка-триггер сама ре-открывает picker — игнорируем mousedown
    // прямо по ней, иначе close → openThemePicker мгновенно бы открыл
    // снова, и стало бы похоже на «ничего не происходит».
    const trigger = document.getElementById("theme-toggle");
    if (trigger && trigger.contains(ev.target)) return;
    close();
  }
}

function onKey(ev) {
  if (ev.key === "Escape") close();
}

function buildSwatch(name) {
  const btn = document.createElement("button");
  btn.type = "button";
  btn.className = "theme-swatch";
  btn.dataset.themeName = name;
  if (!state.themeAuto && state.theme === name) btn.classList.add("selected");
  if (state.themeAuto) btn.setAttribute("disabled", "");

  const preview = document.createElement("div");
  preview.className = "preview";
  preview.innerHTML = "<span></span><span></span><span></span>";
  btn.appendChild(preview);

  const label = document.createElement("div");
  label.className = "label";
  label.textContent = t(`theme-name-${name}`);
  btn.appendChild(label);

  btn.addEventListener("click", () => {
    setTheme(name);
    refresh();
  });
  return btn;
}

function refresh() {
  if (!activePicker) return;
  const grid = activePicker.querySelector(".theme-grid");
  grid.innerHTML = "";
  for (const name of THEMES) grid.appendChild(buildSwatch(name));
  activePicker.querySelector(".auto-input").checked = state.themeAuto;
}

export function openThemePicker() {
  if (activePicker) {
    close();
    return;
  }
  const picker = document.createElement("div");
  picker.className = "theme-picker";
  picker.innerHTML = `
    <div class="title"></div>
    <div class="theme-grid"></div>
    <label class="auto-row">
      <input type="checkbox" class="auto-input" />
      <span class="auto-label"></span>
    </label>
    <div class="auto-hint"></div>
  `;
  picker.querySelector(".title").textContent = t("theme-pick-title");
  picker.querySelector(".auto-label").textContent = t("theme-auto-label");
  picker.querySelector(".auto-hint").textContent = t("theme-auto-hint");

  picker.querySelector(".auto-input").addEventListener("change", (ev) => {
    setThemeAuto(ev.target.checked);
    refresh();
  });

  document.body.appendChild(picker);
  activePicker = picker;
  refresh();

  // Якорим под кнопку-палитру в правом верхнем углу. Если её не нашли —
  // просто в правый-верхний экрана.
  const trigger = document.getElementById("theme-toggle");
  const rect = trigger
    ? trigger.getBoundingClientRect()
    : { left: window.innerWidth - 280, bottom: 12, right: window.innerWidth };
  const w = picker.offsetWidth;
  const h = picker.offsetHeight;
  const left = Math.max(8, Math.min(rect.right - w, window.innerWidth - w - 8));
  const top = Math.min(rect.bottom + 6, window.innerHeight - h - 8);
  picker.style.left = left + "px";
  picker.style.top = top + "px";

  document.addEventListener("mousedown", onOutside, true);
  document.addEventListener("keydown", onKey, true);
  window.addEventListener("scroll", close, true);
  window.addEventListener("resize", close);
  window.addEventListener("blur", close);
}

export function closeThemePicker() {
  close();
}
