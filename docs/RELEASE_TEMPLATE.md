# Release notes template

Шаблон для описания GitHub Release. Такой же текст автоматически
подставляется в draft release из `.github/workflows/release.yml` →
`releaseBody:`. Этот файл — источник правды, чтобы редактировать удобно.
После правок синхронизируй текст в yaml.

Для нового релиза:

1. Скопируй блок ниже.
2. Замени `vX.Y.Z` на версию, `Zound_*` на реальные имена файлов.
3. Обнови секцию «Что нового» под изменения релиза.
4. Вставь в yaml (`releaseBody:` с `|`-многострочкой) или в Draft Release
   на GitHub вручную.

---

## Шаблон v0.1.x (ранние preview-сборки)

```markdown
**Zound** — мультиустройственный аудио-хаб на Rust + Tauri.
Снимает системный звук через loopback и синхронно раздаёт на
несколько устройств одновременно, с per-device громкостью и
ручной компенсацией задержки.

---

## Что нового в vX.Y.Z

- <пункт 1>
- <пункт 2>

## Какую сборку качать

| ОС | Файл | Что делать дальше |
|---|---|---|
| Windows 10/11 | `Zound_*_x64_en-US.msi` или `*_x64-setup.exe` | см. Bypass ниже |
| macOS Apple Silicon | `Zound_*_aarch64.dmg` | см. Bypass ниже |
| macOS Intel | `Zound_*_x64.dmg` | см. Bypass ниже |
| Linux | `Zound_*_amd64.deb` или `*_amd64.AppImage` | `chmod +x` и запуск |

## Bypass: обход блокировок (сборка не подписана)

### 🪟 Windows — SmartScreen
Двойной клик на `.msi` → **Подробнее** → **Выполнить в любом случае**.

PowerShell:
\`\`\`powershell
Unblock-File -Path .\Zound_X.Y.Z_x64_en-US.msi
\`\`\`

### 🍎 macOS — Gatekeeper
\`\`\`bash
xattr -cr /Applications/Zound.app
\`\`\`
Затем **правый клик → Открыть → Открыть**.

### 🐧 Linux
\`\`\`bash
chmod +x Zound_X.Y.Z_amd64.AppImage
./Zound_X.Y.Z_amd64.AppImage
# или
sudo dpkg -i Zound_X.Y.Z_amd64.deb
\`\`\`

## Статус платформ

| ОС | Статус | Что работает |
|---|---|---|
| Windows 10/11 | ✅ полноценно | WASAPI loopback + вывод на N устройств + микшер (mute/balance/EQ/master) + тест-сигнал + drift-индикатор + адаптивная коррекция дрейфа + сессионные профили + блокер feedback |
| macOS 13+ | 🧪 experimental | ScreenCaptureKit loopback + полная энумерация через CoreAudio + микшер + EQ + тест-сигнал + блокер feedback |
| Linux (PipeWire/Pulse) | 🧪 experimental | monitor-source default sink + вывод на N устройств + микшер + EQ + тест-сигнал (блокер feedback не работает — namespace ALSA vs Pulse) |

## Известные ограничения

- «Двоение» звука — by design loopback-подхода.
- Latency пока калибруется на слух (chirp-API уже есть, авто-pipeline — следующий релиз).

## Баги

Issue с ОС, версией, выводом `zound --list`, шагами репро и логом
`RUST_LOG=debug`.
```

---

## Шаблон v1.0.0+ (публичный релиз — после того, как macOS/Linux работают)

TODO: перепиши под стабильный публичный релиз. Убери «preview»-тон,
обнови таблицу платформ (все ✅), добавь раздел про сигнатуру
(если пропишете code signing), ссылки на сайт / поддержку.
