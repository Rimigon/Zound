# Zound

> English version: [README.en.md](./README.en.md)

Мультиустройственный аудио-хаб: «Spotify Connect для любого системного звука».
Захватывает loopback системы и синхронно раздаёт его на выбранные
устройства (наушники, колонки, Bluetooth) с независимыми громкостями,
ручной компенсацией задержки и переключаемым UI на русском / английском.

## Что умеет сейчас (MVP)

- 🎧 WASAPI loopback: снимает звук с текущего default-устройства.
- 🔊 Параллельный вывод на **N устройств** одновременно.
- 🎚 Per-device **громкость** (атомарно в realtime-callback, без дрожания).
- ⏱ Ручная **задержка** на устройство; общая цель автоматически
  пересчитывается через SyncEngine.
- 🔁 **Автоматическое ресемплирование** (`rubato`) при разных частотах
  захвата и выхода (например, 44.1 kHz → 48 kHz).
- 🛡 **Блокировка feedback-loop**: устройство-источник нельзя добавить
  как output (иначе обратная связь).
- 🔄 **Автообновление списка устройств** каждые 2 секунды.
- ▶️⏹ Запуск/остановка pipeline из UI без перезапуска приложения.
- 🌐 Переключатель языка ru/en (Project Fluent, `.ftl` словари).

## Стек

- **Ядро**: Rust (cargo workspace)
- **Аудио**: `cpal` + `rubato` + `ringbuf`
- **UI**: Tauri 2, vanilla HTML/CSS/JS (без bundler-а, без Node-билда)
- **i18n**: `fluent-bundle` (concurrent)
- **Логирование**: `tracing`

## Структура репозитория

```
Cargo.toml                 # workspace root
crates/
  zound-core/              # DeviceId, AudioFrame, SampleFormat, конвертеры
  zound-platform/          # AudioBackend + cpal loopback/output
  zound-sync/              # Sync Engine (target latency, компенсация)
  zound-output/            # OutputManager + AudioEngine (actor-паттерн)
app/                       # Tauri-приложение
  src/                     # Rust backend: main, commands, i18n
  src-ui/                  # frontend: index.html, style.css, app.js
  tauri.conf.json          # конфиг Tauri
  build.rs                 # включая генерацию placeholder-иконки
locales/
  ru.ftl                   # русский словарь (источник правды)
  en.ftl                   # английский словарь
rustfmt.toml
.gitignore
README.md / README.en.md
```

## Быстрый старт

```bash
# Проверка
cargo check --workspace
cargo test --workspace
cargo fmt --check
cargo clippy --workspace -- -D warnings

# Запуск UI
cargo run -p app

# Headless CLI (для проверки пайплайна без окна)
cargo run -p app -- --list
cargo run -p app -- --play "Динамики (Realtek(R) Audio)" --duration 5
cargo run -p app -- --play-default --duration 10
```

Минимальные требования: Rust 1.75+. Для сборки Tauri-окна Node.js **не
нужен** — фронтенд статический.

## Как устроен AudioEngine

`cpal::Stream` на WASAPI не `Send`, поэтому все потоки (capture + output)
живут в одном выделенном `zound-audio` thread-е. Внешний `AudioEngine` —
тонкий handle, который общается с этим потоком через `mpsc`-канал
команд. Благодаря этому `AudioEngine` получается `Send + Sync` и
хранится в `tauri::State` без дополнительных обёрток.

Поток данных:

```
[loopback capture]
       │ consumer
       ▼
[audio-thread tick]
       │ copy into per-device chain
       ▼
[resampler?] → [ringbuf producer] → [cpal output callback] → 🎧
```

## Важное ограничение: «двоение» звука

Loopback — это копия того, что играет на системном default-устройстве.
Система продолжает играть на нём нативно **плюс** Zound дублирует звук
на добавленные устройства. Это не баг — так устроен сам loopback.

Чтобы слышать звук только через Zound-выходы, убавь громкость на
исходном (default) устройстве. Фундаментально избавиться от дублирования
можно только через виртуальный аудио-драйвер (VB-Cable, BlackHole,
собственный kext/DriverKit). Это пост-MVP.

## Документация

- [`CLAUDE.md`](./CLAUDE.md) — архитектура, стек, конвенции, ссылки на
  Skills (локальный, в `.gitignore`).
- `.claude/skills/zound-*` — глубокие знания: синхронизация, WASAPI/CoreAudio/
  PipeWire, Bluetooth, Rust-аудио-стек (локально).

## Лицензия

MIT OR Apache-2.0.
