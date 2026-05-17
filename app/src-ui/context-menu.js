// Простое DOM-контекстное меню. Один экземпляр на страницу: открытие
// нового меню закрывает предыдущее. Закрывается также по mousedown
// снаружи / Esc / scroll / resize. UI-нейтрально, без зависимости от
// домена устройств — items пробрасываются параметром.

let activeMenu = null;

function closeMenu() {
  if (activeMenu) {
    activeMenu.remove();
    activeMenu = null;
    document.removeEventListener("mousedown", onOutsideDown, true);
    document.removeEventListener("keydown", onKeyDown, true);
    window.removeEventListener("scroll", closeMenu, true);
    window.removeEventListener("resize", closeMenu);
    window.removeEventListener("blur", closeMenu);
  }
}

function onOutsideDown(ev) {
  if (activeMenu && !activeMenu.contains(ev.target)) closeMenu();
}

function onKeyDown(ev) {
  if (ev.key === "Escape") closeMenu();
}

/// items: Array<{ label: string, onClick: () => void, hidden?: boolean,
///                 disabled?: boolean }>
export function openContextMenu(x, y, items) {
  closeMenu();
  const visible = items.filter((i) => !i.hidden);
  if (visible.length === 0) return;

  const menu = document.createElement("div");
  menu.className = "context-menu";
  for (const item of visible) {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = "context-menu-item";
    btn.textContent = item.label;
    if (item.disabled) btn.disabled = true;
    btn.addEventListener("click", (ev) => {
      ev.stopPropagation();
      closeMenu();
      try {
        item.onClick();
      } catch (e) {
        console.error("context menu action failed", e);
      }
    });
    menu.appendChild(btn);
  }
  document.body.appendChild(menu);
  activeMenu = menu;

  // Сначала добавили в DOM (получили реальные размеры) — потом позиция.
  // Без этого clamp по правому/нижнему краю не сработает.
  const w = menu.offsetWidth;
  const h = menu.offsetHeight;
  const maxX = window.innerWidth - w - 4;
  const maxY = window.innerHeight - h - 4;
  menu.style.left = Math.max(4, Math.min(x, maxX)) + "px";
  menu.style.top = Math.max(4, Math.min(y, maxY)) + "px";

  document.addEventListener("mousedown", onOutsideDown, true);
  document.addEventListener("keydown", onKeyDown, true);
  window.addEventListener("scroll", closeMenu, true);
  window.addEventListener("resize", closeMenu);
  window.addEventListener("blur", closeMenu);
}

export function closeContextMenu() {
  closeMenu();
}
