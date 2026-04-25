# Zound

> English version: [README.en.md](./README.en.md)

Мультиустройственный аудио-хаб: «Spotify Connect для любого системного звука».
Захватывает loopback системы и синхронно раздаёт его на выбранные
устройства (наушники, колонки, Bluetooth) с независимыми громкостями,
ручной компенсацией задержки и переключаемым UI на русском / английском.

## Что умеет сейчас (MVP)

- 🎧 Системный loopback: WASAPI на Windows, ScreenCaptureKit на macOS 13+,
  monitor-source PulseAudio/PipeWire на Linux.
- 🔊 Параллельный вывод на **N устройств** одновременно.
- 🎚 Per-device **громкость, mute и balance L/R** (атомарно в realtime-
  callback, без дрожания).
- ⏱ Ручная **задержка** на устройство; общая цель автоматически
  пересчитывается через SyncEngine.
- 🧭 **Drift-индикатор**: бейдж в шапке показывает рассинхрон между
  активными устройствами в реальном времени (порог 50 мс).
- 🎵 **Тест-сигнал**: щелчок / 1 кГц синус / метроном 40-240 BPM на
  любое устройство для калибровки на слух.
- 🔁 **Автоматическое ресемплирование** (`rubato`) при разных частотах
  захвата и выхода (например, 44.1 kHz → 48 kHz).
- 🛡 **Блокировка feedback-loop**: устройство-источник нельзя добавить
  как output (иначе обратная связь). Работает на Windows и macOS.
- 🔄 **Автообновление списка устройств** каждые 2 секунды.
- 🌗 **Тема** (Dark/Light/Auto) с переключателем в шапке.
- 💾 **Авто-переподключение последних устройств** при старте движка.
- 🔍 Переключатель **«показывать все устройства»** (по умолчанию — только
  выходы; при включении видны и входы, неактивные для добавления).
- ▶️⏹ Запуск/остановка pipeline из UI без перезапуска приложения.
- 🌐 Переключатель языка ru/en (Project Fluent, `.ftl` словари).

## Что нового в 0.3.0

- **Mute и Balance per device.** В активных устройствах теперь
  переключатель «без звука» (без щелчка через atomic в cpal callback)
  и слайдер баланса L/R с constant-power pan law (perceptual loudness
  не проседает в центре). Mute моментальный, balance — на уровне
  worker-thread без аллокаций.
- **Тест-сигнал** (закрывает MVP-гэп). Кнопка 🔊 у каждого устройства
  → popover с тремя источниками: одиночный щелчок 5 мс, тон 1 кГц
  на 5 секунд, метроном 40-240 BPM. Идёт через отдельный cpal-стрим
  параллельно с обычным output, не попадая в loopback.
- **Drift indicator.** В шапке панели «Устройства» бейдж: «синхронно»
  при ≤50 мс drift между timestamp последних push-ей в ringbuf,
  «дрейф X мс» жёлтым иначе. Скрыт при <2 активных.
- **Dark/Light theme.** Кнопка-переключатель в шапке (☀ / 🌙 / 🌓).
  Тема запоминается; светлая палитра подобрана под градиенты
  лого, тёмная немного перекрашена под фиолетово-голубой акцент.
- **Авто-переподключение последних устройств.** При запуске движка
  Zound восстанавливает активные устройства из прошлой сессии с теми
  же volume / latency / balance / mute. Если устройство пропало —
  пишется статус с количеством непереподключённых, остальное
  продолжает работать.
- **Realtime-safety фикс.** Убрана аллокация `Vec::with_capacity` в
  `adapt_channels` и `push_to_output` (worker-loop) — теперь
  pre-allocated буферы в WorkerOutput.

## Что нового в 0.2.2

- **macOS — все устройства видны.** До этого `cpal` фильтровал
  output_devices через `default_output_config().is_ok()`, и idle-выходы
  (DisplayPort, встроенные динамики, когда выбран другой default)
  пропадали из списка. Теперь обходим cpal-фильтр через прямой обход
  CoreAudio.
- **macOS — feedback-loop блокер.** Имя текущего системного default
  output теперь подставляется как `source_name` capture-сессии —
  блокер на «двоение звука» (когда сам default добавляют как Zound-
  output) начинает срабатывать так же, как на Windows.
- **UI — настройка «показывать все устройства».** В панели «Устройства»
  чекбокс: по умолчанию — только выходы; при включении в список
  добавляются входы (с пометкой «только вход» и без кнопки добавления).
- **Брендинг.** Обновлена иконка приложения и логотип в шапке.

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
| Linux (PipeWire/Pulse) | 🧪 experimental | monitor-source default sink-а через libpulse-simple |
| macOS 13+ | 🧪 experimental | ScreenCaptureKit: при первом запуске система попросит разрешение на запись экрана (нужно для аудио-потока тоже) |

Linux и macOS валидировались только CI-сборкой, на живом железе ещё не
гонялись. Если ловишь баг — issue с ОС, выводом `zound --list` и логом
`RUST_LOG=debug`.

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
Unblock-File -Path .\Zound_0.2.0_x64_en-US.msi
```

### macOS

Требуется Apple Silicon (M1/M2/M3/M4) и macOS 13 (Ventura) или новее.
Сборки под Intel сейчас нет.

1. Скачай `Zound_x.y.z_aarch64.dmg`.
2. Открой DMG, перетащи **Zound.app** в `Applications`.
3. При первом запуске Gatekeeper скажет «Zound повреждён и не может быть
   открыт» или «не удаётся проверить разработчика». Закрой окно.
4. В терминале сними карантин:

   ```bash
   xattr -cr /Applications/Zound.app
   ```

5. Запусти ещё раз — **Правый клик → Открыть** → **Открыть** в диалоге.
   Альтернатива без терминала: **System Settings → Privacy & Security**,
   внизу секции будет сообщение «Zound was blocked…» → **Open Anyway**.
6. macOS покажет диалог **«Zound would like to record this computer's
   screen»** — нажми **Open System Settings**, включи Zound в **Privacy
   & Security → Screen & System Audio Recording** и перезапусти
   приложение. Это обязательно: без разрешения ScreenCaptureKit не
   отдаст системный звук, и процесс будет сразу завершаться.

(Шаги 3–4 — один раз на машину; разрешение на запись экрана тоже
выдаётся один раз.)

**Если получаешь «Не удаётся открыть программу "Zound"» уже после
`xattr -cr`** — это почти всегда значит, что либо разрешение на запись
экрана ещё не выдано, либо macOS младше 13.0. Посмотреть причину
падения можно через:

```bash
log show --predicate 'process == "Zound"' --last 5m --info
```

### Альтернатива без Zound: штатный Multi-Output Device

Если нужно *только* зеркалить звук на несколько устройств, без
per-device громкости и слайдеров задержки, macOS умеет это сам:

1. **Audio MIDI Setup** (`⌘+Space → "Audio MIDI Setup"`).
2. «+» внизу слева → **Create Multi-Output Device**.
3. Отметь нужные устройства; для Bluetooth включи **Drift Correction**.
4. В меню громкости выбери созданное устройство как вывод.

Это не решает случай Bluetooth-наушников с разной задержкой (drift
correction компенсирует дрейф, но не стартовое смещение) и не даёт
независимой громкости — для этих кейсов и нужен Zound.

### Linux

AppImage:

```bash
chmod +x Zound_0.2.0_amd64.AppImage
./Zound_0.2.0_amd64.AppImage
```

`.deb`:

```bash
sudo dpkg -i Zound_0.2.0_amd64.deb
```

Требования: WebKitGTK 4.1 (обычно уже стоит), PipeWire либо PulseAudio.

## Релиз: как собрать и опубликовать

Workflow в `.github/workflows/release.yml` собирает под Windows, macOS
(Apple Silicon) и Linux через `tauri-action`. Запускается на push тега
`v*`:

```bash
git tag v0.2.0
git push origin v0.2.0
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
