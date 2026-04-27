// Footer-status: текст + 4 класса (ok / warn / err / empty). Без
// автотаймера — текст висит до следующего изменения.

export function setStatus(msg, kind = "") {
  const el = document.getElementById("status");
  if (!el) return;
  el.textContent = msg;
  el.className = "status " + kind;
}
