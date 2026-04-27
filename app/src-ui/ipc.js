// Тонкая обёртка над `window.__TAURI__.core.invoke`. Берёт на себя
// нормализацию структурированных ошибок (P2.8): backend возвращает
// `CommandError` JSON-объектом `{kind, message}`, Tauri сериализует его
// в exception без типа. Здесь распознаём такие ошибки, чтобы UI
// мог делать switch по `kind` без подстрочного парсинга.

const tauriInvoke = (cmd, args) => window.__TAURI__.core.invoke(cmd, args);

/// Прямой вызов; ошибка пробрасывается как есть.
export function invoke(cmd, args) {
  return tauriInvoke(cmd, args);
}

/// Нормализованная ошибка для UI-обработки.
/// `kind` ∈ feedbackBlocked | deviceNotFound | engineNotStarted |
/// engineDead | testAlreadyPlaying | badRequest | backend | "unknown".
export function classifyError(err) {
  if (err && typeof err === "object" && "kind" in err) {
    return { kind: err.kind, message: err.message ?? "" };
  }
  // Tauri иногда отдаёт ошибки строкой (legacy String error). Парсим
  // ключевые маркеры для совместимости.
  const s = String(err);
  if (s.includes("feedback-default-blocked")) {
    return { kind: "feedbackBlocked", message: s };
  }
  return { kind: "unknown", message: s };
}
