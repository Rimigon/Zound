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

## Платформы

| ОС | Статус | Что именно |
|---|---|---|
| Windows 10/11 | ✅ работает | WASAPI loopback + вывод на N устройств |
| macOS 13+ | ⚠️ только UI | окно открывается, capture = no-op (надо ScreenCaptureKit) |
| Linux (PipeWire) | ⚠️ только UI | окно открывается, capture = no-op (надо PipeWire monitor) |

## Установка из GitHub Releases

Билды **не подписаны** (закрытый репо, тестовая раздача). ОС ругнётся —
это нормально, обходится в два клика.

### Windows

1. Скачай `Zound_x.y.z_x64_en-US.msi` (или `_x64-setup.exe`).
2. Запусти — появится синее окно **Защитник Windows SmartScreen**:
   «Защитник Windows защитил ваш компьютер».
3. Нажми **Подробнее** → **Выполнить в любом случае**.
4. Если вместо SmartScreen «Неизвестный издатель» — **Выполнить**.

Альтернатива через PowerShell (если вообще не хочешь видеть
предупреждение):

```powershell
Unblock-File -Path .\Zound_0.1.0_x64_en-US.msi
```

### macOS

1. Скачай `Zound_x.y.z_aarch64.dmg` (Apple Silicon) или
   `Zound_x.y.z_x64.dmg` (Intel).
2. Открой DMG, перетащи **Zound.app** в `Applications`.
3. При первом запуске Gatekeeper скажет «Zound повреждён и не может быть
   открыт» или «не удаётся проверить разработчика». Закрой окно.
4. В терминале сними карантин:

   ```bash
   xattr -cr /Applications/Zound.app
   ```

5. Запусти ещё раз — **Правый клик → Открыть** → **Открыть** в диалоге.

(Первые два шага — один раз на машину; после `xattr -cr` приложение
запускается как обычно двойным кликом.)

### Linux

AppImage:

```bash
chmod +x Zound_0.1.0_amd64.AppImage
./Zound_0.1.0_amd64.AppImage
```

`.deb`:

```bash
sudo dpkg -i Zound_0.1.0_amd64.deb
```

Требования: WebKitGTK 4.1 (обычно уже стоит), PipeWire либо PulseAudio.

## Релиз: как собрать и опубликовать

Workflow в `.github/workflows/release.yml` собирает под Windows, macOS
(Intel + ARM) и Linux через `tauri-action`. Запускается на push тега
`v*`:

```bash
git tag v0.1.0
git push origin v0.1.0
```

GitHub Actions создаст **draft release**, ты откроешь его в UI репо и
нажмёшь Publish. Артефакты (`.msi`, `.exe`, `.dmg`, `.AppImage`, `.deb`)
прикрепляются автоматически.

Локальная сборка под текущую ОС:

```bash
cargo install tauri-cli --version '^2' --locked
cd app && cargo tauri build
```

Результат — в `target/release/bundle/`.

## Лицензия

MIT OR Apache-2.0.
