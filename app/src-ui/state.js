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
  theme: "auto",
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
};

export const KEYS = {
  showAll: "zound.showAllDevices",
  theme: "zound.theme",
  latencyLink: "zound.latencyLinked",
  groups: "zound.deviceGroups",
  eq: "zound.deviceEq",
  legacySession: "zound.lastSession.outputs",
};

export const INTERVALS = {
  deviceRefreshMs: 2000,
  peakRefreshMs: 80,
  /// Backend events poll (watchdog disconnect, default change, panic) —
  /// 250 ms ровно вписывается между peak (80) и device-list (2000).
  eventsPollMs: 250,
  profilePersistDebounceMs: 250,
};
