// Глобальное состояние UI. Один объект, который все модули мутируют —
// vanilla-JS заменитель магазина. Реактивности нет: модули вызывают
// render-функции явно после изменений (см. devices.js, mixer.js).
//
// localStorage-ключи и интервалы тоже здесь, чтобы не дублировались.

export const state = {
  engineRunning: false,
  /// Engine alive-флаг: false → audio-thread поймал panic, нужен рестарт.
  engineAlive: true,
  /// Имя default-устройства, источник захвата. Backend отдаёт camelCase
  /// (`loopbackSource`), мы храним под привычным именем.
  loopbackSource: null,
  devices: [], // см. devicesEqual
  /// {id, name, endpointId, volume, latencyMs, balance, muted, channels}
  active: [],
  lang: "ru",
  dict: {},
  showAllDevices: false,
  /// Выбранная вручную палитра. Применяется, когда themeAuto === false.
  /// Значения: dark | light | midnight | sunset | forest | ocean | mono.
  theme: "dark",
  /// Если true — фронт снимает атрибут data-theme и палитра определяется
  /// `prefers-color-scheme` системы (см. style.css media query).
  themeAuto: false,
  testRunning: new Map(), // device_name → { kind, bpm? }
  activePopover: null,
  suspendPersist: false, // race-guard под restoreSession
  master: { gain: 1.0, muted: false },
  peakBars: new Map(), // device_id → DOM .peak-fill
  latencyLinked: false,
  /// device_id → { low, mid, high } EQ gain в dB. UI mirror; backend —
  /// источник правды (через session.json), здесь — для рендера слайдеров.
  eq: new Map(),
  deviceGroups: new Map(),
  /// endpointId → пользовательский алиас. Чисто display, системное имя
  /// устройства не меняется. Источник правды — session.json через команды
  /// `list_device_aliases` / `set_device_alias`.
  aliases: new Map(),
  /// Доступное обновление: { version, body } | null. Источник — Tauri
  /// updater plugin (см. updater.js).
  updateAvailable: null,
  /// Открытость «Advanced» секции на конкретном устройстве. UI-only,
  /// без persist — рендер делает за пользователя умолчания (auto-open,
  /// если есть нестандартный balance/EQ).
  advancedOpen: new Set(),
};

export const KEYS = {
  showAll: "zound.showAllDevices",
  theme: "zound.theme",
  themeAuto: "zound.themeAuto",
  latencyLink: "zound.latencyLinked",
  groups: "zound.deviceGroups",
  eq: "zound.deviceEq",
  legacySession: "zound.lastSession.outputs",
  /// Версия апдейта, который пользователь скрыл кнопкой «Позже». Плашка
  /// не показывается, пока updater не обнаружит более новую версию.
  updateDismissedVersion: "zound.update.dismissedVersion",
};

export const INTERVALS = {
  deviceRefreshMs: 2000,
  peakRefreshMs: 80,
  /// Backend events poll (watchdog disconnect, default change, panic) —
  /// 250 ms ровно вписывается между peak (80) и device-list (2000).
  eventsPollMs: 250,
  profilePersistDebounceMs: 250,
};
