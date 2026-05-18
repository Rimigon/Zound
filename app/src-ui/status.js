// Footer-status: timeline последних 3 сообщений с opacity-fade.
// Каждое новое сообщение пушится в очередь; самое старое уезжает влево
// и затухает. Без автотаймера — текст висит, пока не будет вытеснен.

const HISTORY = [];
const MAX = 3;

function now() {
  const d = new Date();
  const pad = (n) => String(n).padStart(2, "0");
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}

function escapeHtml(s) {
  return String(s ?? "")
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

function render() {
  const el = document.getElementById("status");
  if (!el) return;
  el.innerHTML = HISTORY.map((m) => `
    <span class="status-msg" data-lvl="${m.lvl}">
      <span class="lvl-dot" aria-hidden="true"></span>
      <span>${escapeHtml(m.msg)}</span>
      <span class="ts">${m.ts}</span>
    </span>
  `).join("");
}

export function setStatus(msg, kind = "") {
  if (!msg) return;
  HISTORY.push({ msg, lvl: kind || "info", ts: now() });
  while (HISTORY.length > MAX) HISTORY.shift();
  render();
}
