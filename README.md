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
- 🎛 **3-полосный EQ** per device (low-shelf 100 Гц, peak 1 кГц,
  high-shelf 8 кГц, ±12 dB) с zero-cost bypass при флаге gain==0.
- 🎚 **Master gain + master mute** (глобальная громкость / тишина для
  всех устройств), peak-meter per device в UI.
- ⏱ Ручная **задержка** на устройство; общая цель автоматически
  пересчитывается через SyncEngine.
- 🧭 **Drift-индикатор**: бейдж в шапке показывает рассинхрон между
  активными устройствами в реальном времени (порог 50 мс).
- 🛰 **Адаптивная коррекция дрейфа**: PI-контроллер на каждое устройство,
  меняет ratio ресемплера на ±0.1% (±100 ppm) для удержания общей цели.
- 🎵 **Тест-сигнал**: щелчок / 1 кГц синус / метроном 40-240 BPM на
  любое устройство для калибровки на слух.
- 📐 **Калибровочный chirp** (200 мс линейный sweep 200 Гц → 4 кГц) —
  через Tauri-команду `generate_calibration_chirp`, основа для будущей
  авто-калибровки latency.
- 💾 **Сессионные профили**: устройства, громкости, mute, balance,
  latency, EQ сериализуются в JSON и автоматически восстанавливаются
  при следующем запуске.
- 🔁 **Автоматическое ресемплирование** (`rubato`) при разных частотах
  захвата и выхода (например, 44.1 kHz → 48 kHz).
- 🛡 **Блокировка feedback-loop**: устройство-источник нельзя добавить
  как output (и нельзя запустить на нём тест-сигнал). Работает на
  Windows и macOS.
- 🔄 **Автообновление списка устройств** каждые 2 секунды.
- 🌗 **Тема** (Dark/Light/Auto) с переключателем в шапке.
- 🔍 Переключатель **«показывать все устройства»** (по умолчанию — только
  выходы; при включении видны и входы, неактивные для добавления).
- ▶️⏹ Запуск/остановка pipeline из UI без перезапуска приложения.
- 🌐 Переключатель языка ru/en (Project Fluent, `.ftl` словари).
- 🧪 **`--self-test`** — headless smoke-проверка пайплайна для CI.

## Что нового в 0.4.2

- **Переименование устройств в приложении.** ПКМ по строке устройства
  (как в списке, так и в активном миксере) → «Переименовать» / «Сбросить
  имя». Алиас — чисто display: системное имя WASAPI / CoreAudio /
  PipeWire не меняется, в `add_output` и `play_test_signal` уходит
  оригинал. Хранится в `session.json` под `deviceAliases:
  { endpoint_id → имя }`, поэтому переживает рестарт и не зависит от
  того, активно устройство или нет.
- **Авто-обновление через `tauri-plugin-updater`.** Через 5 секунд после
  старта (и далее раз в 6 часов) приложение опрашивает GitHub Releases
  `latest.json`, и при наличии новой версии сверху показывается плашка
  «Доступна версия X» → «Установить и перезапустить». Кнопка «Позже»
  скрывает плашку до следующей более новой версии (запоминание через
  `localStorage`). Подпись артефактов отдельной парой ключей (формат
  minisign), `latest.json` генерируется `tauri-action` в CI при наличии
  секретов `TAURI_SIGNING_PRIVATE_KEY{,_PASSWORD}`.
- **CSP расширен** на `github.com` / `api.github.com` /
  `*.githubusercontent.com` (нужно для fetch манифеста и скачивания
  bundle-а), capabilities дополнены `updater:default` и
  `process:allow-restart`. Никакие `fs/http/shell/dialog` плагины не
  включены.

## Что нового в 0.4.1

Аудит проекта закрыл 15 пунктов P1 (баги) + P2 (структурные улучшения)
одним релизом:

- **Стабильный backend-id для устройств.** На Windows тянем
  WASAPI endpoint id (`{0.0.0.00000000}.{guid}`) через
  `IMMDeviceEnumerator`; на macOS — стрингифицированный
  `AudioObjectID`. Session profile теперь матчит устройство сначала по
  endpoint_id, потом по имени — переименование/одинаковые имена больше
  не ломают восстановление.
- **Watchdog отключения устройства.** Если push в ringbuf >50 % теряет
  на протяжении 200 worker-tick'ов подряд (~2 сек), engine снимает
  output, эмитит `OutputDisconnected`-event, фронт убирает плашку.
- **Подписка на смену системного default (Windows).** Поллер на
  `IMMDeviceEnumerator::GetDefaultAudioEndpoint`, soft-restart capture
  при изменении. UI получает баннер «default changed».
- **`catch_unwind` в audio-thread.** Panic больше не убивает процесс
  и не вешает `Drop::join`: ловим, помечаем `engine_status.alive=false`,
  фронт показывает «engine died, please restart».
- **Drift через `rubato::set_resample_ratio_relative`.** Убран
  drop/dup кадров; на тон-сигналах нет pitch-плывения. Fallback на
  линейный stretch остался для устройств без ресемплера.
- **Fire-and-forget Tauri-команды.** Volume / mute / balance / EQ /
  master больше не блокируют UI на channel reply — слайдеры остаются
  отзывчивыми даже если audio-thread занят большим chunk'ом.
- **EQ в session profile.** Полосы low/mid/high (dB) сериализуются
  вместе с volume / mute / balance / latency и переживают перезапуск.
- **`CommandError` enum.** Tauri-команды отдают
  `{kind, message}` вместо плоских строк; фронт делает `switch` по
  `kind`, парсинг подстрок ушёл.
- **Один `DevicePreset`.** Слили `ProfileDeviceDto` и `DevicePreset` в
  одну структуру с `serde(rename_all = "camelCase")`.
- **i18n keys из FTL.** `app/build.rs` парсит `locales/*.ftl`, генерит
  `KEYS` в `OUT_DIR`, проверяет паритет ru↔en на этапе сборки.
- **Frontend: ES-модули.** `app.js` (1100+ строк) разбит на
  `state / ipc / i18n / theme / status / devices / mixer / sync /
  tests / session / events`. Бандлер не подключаем — `<script type="module">`.
- **Tauri capabilities.** Минимальный набор: только `core:default`,
  никаких `fs/http/shell/dialog`. Окно `main` фигурирует явно.
- **Content Security Policy.** `default-src 'self'`, `script-src 'self'`,
  `connect-src 'self' ipc: http://ipc.localhost`, `object-src 'none'`.
- **DriftCorrector kp/ki: расчёт-обоснование + step-response тест.**
  Комментарий с расчётом устойчивости и unit-тест, проверяющий
  монотонную раскачку коррекции и cap.
- **Rate-limited drop-warning.** Накопленные dropped-семплы логируются
  раз в секунду на устройство, а не на каждый тик.

## Что нового в 0.4.0

- **3-полосный EQ per device.** Low-shelf 100 Гц, peak 1 кГц, high-shelf
  8 кГц (Q=0.707 / 1.0, формулы RBJ Audio EQ Cookbook). Биквады в Direct
  Form I, состояние per-channel, обновление коэффициентов через
  `ArcSwap` без блокировок в callback. Bypass при gain==0 — один `if`
  на сэмпл, нулевой DSP-cost.
- **Master gain + master mute.** Глобальный регулятор поверх per-device
  громкостей. Mute моментальный (atomic в worker), gain — линейная
  шкала 0–1.5x с per-device peak-meter в UI (полупрозрачная заливка
  под слайдером громкости, обновление 30 fps).
- **Адаптивная коррекция дрейфа часов.** PI-контроллер per device:
  входной error — `device_t − reference_t`, выход — корректировка ratio
  для `rubato` (`±0.001`, deadband 5 мс). Этого хватает на типичный
  кварцевый дрейф ±20–50 ppm; pitch-плывения на тон-сигналах нет.
- **Сессионные профили.** Состояние (список устройств + volume / mute /
  balance / latency / EQ + master gain / mute) сериализуется в JSON
  (`session.json` в data-dir платформы) и восстанавливается при
  следующем запуске. Поле `version` для будущих миграций.
- **Калибровочный chirp.** Линейный sweep 200 Гц → 4 кГц длиной 200 мс,
  доступен через Tauri-команду `generate_calibration_chirp(sample_rate,
  duration_ms)`. Полноценный pipeline cross-correlation через loopback —
  следующий инкремент; сейчас это базовый API для UI-кнопки
  «откалибровать».
- **`--self-test` smoke-режим.** Headless-проверка: открыть capture,
  завести один dummy-output, проиграть N сэмплов, корректно
  остановиться. Гоняется в CI на linux/windows/macos в debug и release.
- **CI workflow.** `.github/workflows/ci.yml`: rustfmt, `cargo doc`
  с `-D rustdoc::broken-intra-doc-links`, проверка `Cargo.lock` через
  `--locked`, clippy + tests + smoke на трёх ОС, оба профиля
  (debug + release).
- **Тесты `zound-output`.** Появилась интеграционная папка
  `crates/zound-output/tests/` — first-class регрессионные сценарии
  для AudioEngine.

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
git tag v0.4.0
git push origin v0.4.0
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
