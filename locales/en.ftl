## Zound — English localization
## Syntax: Project Fluent (https://projectfluent.org/)

app-title = Zound
app-subtitle = Multi-device audio hub

nav-devices = Devices
nav-sync = Sync
nav-settings = Settings

engine-start = Start
engine-stop = Stop

device-add = Add
device-remove = Remove
device-source-badge = source
device-source-note = Loopback capture source. Cannot be added — feedback would occur.
no-active-outputs = No active outputs. Add at least one from the list.
doubling-note =
    The system default device keeps playing natively. Extra outputs
    mirror the same sound. To hear it only through Zound, lower the
    volume on the source device.

feedback-default-blocked = This device is the capture source. Adding it as an output would create a feedback loop.

sync-hint =
    For every active device you can tune latency manually. Sliders
    affect the overall target latency.
sync-target-latency = Target: { $ms } ms
sync-out-of-sync-warning = Devices are out of sync
sync-recalibrate = Recalibrate

device-count = { $count ->
    [one] { $count } device
   *[other] { $count } devices
}

volume-label = Volume
mute-button = Mute
unmute-button = Unmute
balance-label = Balance

latency-label = Latency
latency-calibrate = Calibrate
latency-test-signal = Test signal

language-label = Language
language-ru = Русский
language-en = English

status-ready = Ready
status-engine-started = Engine started
status-engine-stopped = Engine stopped
status-output-added = Output added
status-output-removed = Output removed
status-devices-refreshed = Device list refreshed
