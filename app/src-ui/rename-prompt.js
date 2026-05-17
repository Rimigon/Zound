// Маленькое модальное окно ввода для алиаса устройства. window.prompt
// не подходит: его не локализовать, не оформить под тему, и он
// синхронный (блокирует event-loop под Tauri-webview).

import { t } from "./i18n.js";

let activeOverlay = null;

function closeOverlay() {
  if (activeOverlay) {
    activeOverlay.remove();
    activeOverlay = null;
  }
}

/// Возвращает Promise<string | null>. null = пользователь отменил;
/// строка (возможно пустая) = подтвердил, вызывающий код сам решает
/// что считать «сбросом».
export function openRenamePrompt({ initialValue = "", systemName = "" } = {}) {
  closeOverlay();
  return new Promise((resolve) => {
    const overlay = document.createElement("div");
    overlay.className = "rename-overlay";
    overlay.innerHTML = `
      <div class="rename-dialog" role="dialog" aria-modal="true">
        <div class="rename-title"></div>
        <div class="rename-system"></div>
        <label class="rename-label"></label>
        <input type="text" maxlength="80" class="rename-input" />
        <div class="rename-hint"></div>
        <div class="rename-buttons">
          <button class="rename-cancel" type="button"></button>
          <button class="rename-ok primary" type="button"></button>
        </div>
      </div>
    `;
    overlay.querySelector(".rename-title").textContent = t(
      "device-rename-prompt-title",
    );
    overlay.querySelector(".rename-system").textContent = systemName;
    overlay.querySelector(".rename-label").textContent = t(
      "device-rename-prompt-label",
    );
    overlay.querySelector(".rename-hint").textContent = t(
      "device-rename-prompt-hint",
    );
    overlay.querySelector(".rename-ok").textContent = t("device-rename-ok");
    overlay.querySelector(".rename-cancel").textContent =
      t("device-rename-cancel");

    const input = overlay.querySelector(".rename-input");
    input.value = initialValue;
    input.placeholder = systemName;

    const finish = (value) => {
      closeOverlay();
      resolve(value);
    };

    overlay.querySelector(".rename-ok").addEventListener("click", () => {
      finish(input.value);
    });
    overlay.querySelector(".rename-cancel").addEventListener("click", () => {
      finish(null);
    });
    overlay.addEventListener("mousedown", (ev) => {
      if (ev.target === overlay) finish(null);
    });
    overlay.addEventListener("keydown", (ev) => {
      if (ev.key === "Escape") finish(null);
      else if (ev.key === "Enter") finish(input.value);
    });

    document.body.appendChild(overlay);
    activeOverlay = overlay;
    input.focus();
    input.select();
  });
}
