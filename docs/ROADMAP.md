# Zound — план работ после v0.4.2

Источник — аудит проекта от 2026-04-28. Пункты P1+P2 закрываются в
v0.4.1 одним коммитом. Остальное расписано здесь.

Приоритеты:
- **P3** — UX-полировка. Делается без архитектурных изменений.
- **P4** — крупные фичи, либо требующие внешних ресурсов
  (сертификаты / endpoint), либо отдельных недель работы.
- **P5** — тесты и качество.

Порядок исполнения по умолчанию: **P3 → P5 → P4** (UX и тесты раньше
крупных фич, потому что они страхуют рефакторы под фичи).

---

## P3 — UX-полировка (целевой релиз v0.5.0)

| # | Пункт | Где трогать |
|---|-------|-------------|
| 16 | Скроллбары под светлую тему | `app/src-ui/style.css` (`::-webkit-scrollbar*` под `:root[data-theme="light"]`) |
| 17 | EQ-полосы с подписями частот (100 / 1k / 8k Hz) | `app/src-ui/index.html`, `app/src-ui/app.js` (`renderEqBands`) |
| 18 | Слайдер амплитуды тест-сигнала (-30..0 dB) | `app/src/commands.rs::play_test_signal`, `crates/zound-platform/src/test_signal.rs`, popover в `app.js` |
| 19 | Latency-пресеты (BT ~200, Wired ~5, Network ~50) | Кнопки рядом со слайдером latency в `app.js`, ключи i18n |
| 20 | Хоткеи: Space=start/stop, M=master mute, Ctrl+R=reset | `app/src-ui/app.js` (window keydown), документировать в README |
| 21 | Tray icon + minimize-to-tray | `tauri.conf.json` plugin tray, `app/src/main.rs` (window_event), иконка |
| 22 | Опциональный sparkline drift-индикатора | Settings-флаг, canvas в `app.js` |
| 23 | Confirm/undo на device-reset | Toast компонент, очередь действий в `app.js` |
| 24 | Buffer-health индикатор (% ringbuf) | `engine.rs` экспортирует fill ratio, `peaks` команда расширяется, UI рендерит полоску |
| 25 | Очистка статус-сообщений при смене контекста | `app.js` (`setStatus` с автотаймером + clear на route change) |

---

## P5 — тесты и качество (целевой релиз v0.5.1)

| # | Пункт | Что делать |
|---|-------|------------|
| 36 | Criterion-бенчмарки | `crates/zound-core/benches/eq.rs`, `crates/zound-output/benches/push.rs`, `criterion = "0.5"` dev-deps. CI job `bench` (без gating). |
| 37 | Fuzz-тесты | `cargo-fuzz` на `Biquad::process` (NaN/Inf/денормали) и `apply_compensation_delta`. Папка `fuzz/`. |
| 38 | Mock-backend | Trait `AudioBackend` уже есть — добавить `MockBackend` в `zound-platform/src/mock.rs` под `#[cfg(any(test, feature = "mock"))]`. AudioEngine-тесты с ним. |
| 39 | E2E через Tauri WebDriver | `tauri-driver`, Playwright-сценарии в `tests/e2e/`, отдельная CI-job `e2e` на linux. |
| 40 | CHANGELOG.md | Создать, заполнить v0.1.0 → v0.4.1 ретроспективно. Дальше — обязательно при каждом релизе. |

---

## P4 — крупные фичи и внешние зависимости (релизы v0.6+)

### Требуют внешних ресурсов / решений пользователя

Эти пункты **не блокируют** друг друга кодом — блокируют только
организационные решения.

| # | Пункт | Что нужно от вас |
|---|-------|------------------|
| 33 | Code signing | Apple Developer ID Application ($99/год), Windows EV/OV cert ($200–500/год). После получения — секреты в Actions, обновить `tauri-action` → `app/tauri.conf.json::bundle::*`. |
| 32 | Tauri auto-updater | Зависит от #33 (без подписи updater откажется). После — генерация update keypair, endpoint (GitHub Releases JSON либо свой), `updater` секция в `tauri.conf.json`. |
| 34 | Crash reporter / telemetry | Решение: Sentry ($26/мес для команды) vs свой endpoint. Opt-in UI, privacy policy в README. Sentry SDK или `tracing-subscriber` с http-appender. |
| 35 | Расширение языков | Список нужных языков. Перевод `.ftl` руками или через ревью; машинный перевод не катит для UI. |

### Большие технические фичи (каждая ≥ неделя)

| # | Пункт | Скоуп |
|---|-------|-------|
| 26 | Per-app capture | Win11 ProcessLoopback API (новые WASAPI флаги в Windows 11 22H2+), macOS SCK с PID-фильтром, PipeWire — фильтр по `application.process.id` на monitor source. По крейту на платформу + UI выбора процесса (`zound-platform` расширяется). |
| 27 | Сетевой режим (Zound→Zound) | Новый крейт `zound-net`. Свой UDP-протокол поверх `tokio`, формат фреймов с timestamp + opus или PCM. Discovery через mDNS (`mdns-sd`). Sync через NTP-подобный handshake. |
| 28 | Полноценная авто-калибровка | Уже есть `generate_chirp` и `cross_correlate` в `zound-sync::calibration`. Нужно: (а) UI flow с инструкцией, (б) воспроизведение chirp на target-устройстве через `test_signal`-механизм, (в) одновременная запись через loopback или второй input, (г) корреляция → запись в `intrinsic_latency`. На MVP — только Windows. |
| 29 | ASIO / JACK | ASIO — отдельный feature flag, бинарь `zound-asio` (лицензия Steinberg ASIO SDK, бесплатная но регистрация). JACK — Linux-only, простой `jack` крейт. Обе — отдельные `AudioBackend` impl. |
| 30 | Запись на файл (capture-to-wav) | `hound` + новый блок UI, кнопка REC. Не блокирующий, отдельный поток, ringbuf от capture. |
| 31 | Audio file playback | Опциональный плеер, `symphonia` декодер. Источник как альтернатива loopback. Большая UI-секция. |

---

## Принципы при работе по списку

1. **Один P-блок = один коммит** (P1+P2 в v0.4.1 — исключение; разовое слияние).
2. **Каждый коммит** проходит `cargo fmt --check`, `cargo clippy
   --workspace -- -D warnings`, `cargo test --workspace` локально.
3. **CI не отключаем** для протаскивания PR. Если тест падает —
   разбираемся в причине, не `--no-verify`.
4. **i18n паритет** — каждый новый ключ сразу в `ru.ftl` и `en.ftl`.
5. **Realtime-safety** — любой PR, трогающий `engine.rs` worker
   loop / cpal callback, проверяется на отсутствие alloc / Mutex /
   syscall в hot path.
