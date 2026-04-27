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
device-input-only-badge = input only
device-input-only-note = This is a recording-only device (microphone). It cannot be added as an output.
show-all-devices = Show all devices (including inputs)
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

# v0.3.0 — mixer, test signal, theme, drift, auto-reconnect.
theme-toggle-title = Toggle theme
mute-label = Mute
balance-l = L
balance-c = C
balance-r = R
balance-mono-note = Mono device — balance unavailable
test-button-title = Test signal
test-kind-click = Click
test-kind-sine = 1 kHz Sine
test-kind-metronome = Metronome
test-bpm-label = Tempo, BPM
test-start = Start
test-stop = Stop
test-running = { $kind } playing
test-running-bpm = { $kind } { $bpm } BPM
test-source-disabled = Cannot — this is the capture source
sync-status-synced = synced
sync-status-drift = drift { $ms } ms
sync-status-na = —
status-restore-ok = Session restored
status-restore-failed-one = Not found: { $name }
status-restore-partial = { $count } devices not restored

# v0.4 — master gain/mute, peak meters
master-label = Master volume
master-mute = Master mute
peak-label = Level
latency-link-label = Link latencies
latency-link-title = Move all latency sliders together
default-source-warning =
    The capture source ({ $source }) is the Windows default device. It is
    played directly by the system, bypassing Zound, so its latency cannot
    be tuned. To synchronise all devices, change the default in Windows
    Sound Settings to a placeholder (or a virtual cable) and add the
    targets as Zound outputs.

# v0.5 — EQ, groups, drift sparkline
eq-toggle = Equalizer
eq-low = Low
eq-mid = Mid
eq-high = High
eq-reset = Reset
device-reset = Reset device
device-reset-title = Restore volume, latency, balance, mute and EQ to defaults
group-label = Group
group-none = No group
group-new = New…
group-new-prompt = New group name:
group-volume = Group volume
group-mute = Mute
group-latency = Group latency
