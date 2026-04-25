## Zound — русская локализация
## Синтаксис: Project Fluent (https://projectfluent.org/)

app-title = Zound
app-subtitle = Мультиустройственный аудио-хаб

nav-devices = Устройства
nav-sync = Синхронизация
nav-settings = Настройки

engine-start = Запустить
engine-stop = Остановить

device-add = Добавить
device-remove = Убрать
device-source-badge = источник
device-source-note = Источник захвата (loopback). Добавлять нельзя — будет обратная связь.
device-input-only-badge = только вход
device-input-only-note = Это устройство только для записи (микрофон). Как output его добавить нельзя.
show-all-devices = Показывать все устройства (включая входы)
no-active-outputs = Нет активных устройств. Добавьте хотя бы одно слева.
doubling-note =
    Устройство по умолчанию продолжает играть системно. Дополнительные
    устройства дублируют тот же звук. Чтобы слышать только через Zound,
    убавьте громкость на исходном устройстве.

feedback-default-blocked = Это устройство — источник захвата. Его нельзя добавить как output, возникнет feedback loop.

sync-hint =
    Для каждого активного устройства можно вручную подстраивать задержку.
    Ползунки влияют на общую целевую задержку.
sync-target-latency = Общая: { $ms } мс
sync-out-of-sync-warning = Устройства рассинхронизированы
sync-recalibrate = Перекалибровать

device-count = { $count ->
    [one] { $count } устройство
    [few] { $count } устройства
   *[other] { $count } устройств
}

volume-label = Громкость
mute-button = Выкл
unmute-button = Вкл
balance-label = Баланс

latency-label = Задержка
latency-calibrate = Калибровать
latency-test-signal = Тест-сигнал

language-label = Язык
language-ru = Русский
language-en = English

status-ready = Готово
status-engine-started = Движок запущен
status-engine-stopped = Движок остановлен
status-output-added = Устройство добавлено
status-output-removed = Устройство убрано
status-devices-refreshed = Список устройств обновлён

# v0.3.0 — микшер, тест-сигнал, тема, drift, auto-reconnect.
theme-toggle-title = Сменить тему
mute-label = Без звука
balance-l = Л
balance-c = Ц
balance-r = П
balance-mono-note = Mono-устройство — баланс недоступен
test-button-title = Тест-сигнал
test-kind-click = Щелчок
test-kind-sine = Синус 1 кГц
test-kind-metronome = Метроном
test-bpm-label = Темп, BPM
test-start = Старт
test-stop = Стоп
test-running = { $kind } играет
test-running-bpm = { $kind } { $bpm } BPM
test-source-disabled = Нельзя — это источник захвата (loopback)
sync-status-synced = синхронно
sync-status-drift = дрейф { $ms } мс
sync-status-na = —
status-restore-ok = Сессия восстановлена
status-restore-failed-one = Не найдено: { $name }
status-restore-partial = Не восстановлено устройств: { $count }
