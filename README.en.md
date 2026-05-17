# Zound

> Russian version (primary): [README.md](./README.md)

A multi-device audio hub: "Spotify Connect for any system audio".
Captures system loopback and streams it synchronously to the chosen
devices (headphones, speakers, Bluetooth) with independent volumes,
manual latency compensation, and a switchable UI (Russian / English).

## What it does today (MVP)

- 🎧 System loopback capture: WASAPI on Windows, ScreenCaptureKit on
  macOS 13+, PulseAudio/PipeWire monitor source on Linux.
- 🔊 Parallel output to **N devices** at the same time.
- 🎚 Per-device **volume, mute and L/R balance**, updated atomically
  inside the realtime callback.
- 🎛 **3-band EQ** per device (low-shelf 100 Hz, peak 1 kHz, high-shelf
  8 kHz, ±12 dB) with zero-cost bypass when gain == 0.
- 🎚 **Master gain + master mute** (global volume / mute across all
  outputs), per-device peak meter in the UI.
- ⏱ Manual per-device **latency**; target latency is recomputed
  automatically by the Sync Engine.
- 🧭 **Drift indicator** badge in the header showing live sync delta
  between active outputs (threshold 50 ms).
- 🛰 **Adaptive drift correction**: PI controller per device, nudges the
  resampler ratio by up to ±0.1% (±100 ppm) to track the shared target.
- 🎵 **Test signal**: click / 1 kHz sine / metronome 40-240 BPM on any
  device — for by-ear calibration.
- 📐 **Calibration chirp** (200 ms linear sweep 200 Hz → 4 kHz) exposed
  via the `generate_calibration_chirp` Tauri command — the foundation
  for upcoming automatic latency calibration.
- 💾 **Session profiles**: devices, volume, mute, balance, latency, EQ
  serialized to JSON and auto-restored on the next launch.
- 🔁 Automatic **resampling** (`rubato`) when capture and output
  sample rates differ (e.g. 44.1 kHz → 48 kHz).
- 🛡 **Feedback-loop protection**: the capture source device cannot be
  added as an output, and test signals on it are blocked too. Works on
  Windows and macOS.
- 🔄 **Auto device refresh** every 2 seconds.
- 🌗 **Dark/Light/Auto theme** toggle in the header.
- 🔍 **"Show all devices"** toggle (off by default — outputs only; on —
  inputs are also listed but cannot be added).
- ▶️⏹ Start/stop pipeline from the UI without restarting the app.
- 🌐 Language switch ru/en (Project Fluent, `.ftl` dictionaries).
- 🧪 **`--self-test`** — headless smoke check used by CI.

## What's new in 0.4.3

- **Seven themes instead of two.** On top of `dark` and `light` you now
  have `midnight` (deep blue-purple), `sunset` (warm peach/coral light),
  `forest` (dark green), `ocean` (teal + mint dark) and `mono` (high
  contrast — for screenshots and stress-testing the palette).
- **Theme picker replaces the cycle button.** A 🎨 icon in the header
  opens a popover with a grid of swatches (three-color preview per
  theme) — pick one to apply it.
- **Auto mode moved out of the button** into a separate "Follow system"
  checkbox inside the picker. When enabled, the `data-theme` attribute
  is removed from `<html>` and the palette is chosen entirely by
  `@media (prefers-color-scheme)` in CSS — instant reaction, manual
  presets are shown as disabled.
- **Test-signal button works.** `.device-row` had `contain: layout
  paint` — the `paint` containment was clipping the test-signal popover
  (absolutely positioned below the row), so clicking 🔊 had no visible
  effect. Reduced to `contain: layout`, popover is visible again.
- **Device alias is now used in the default-source warning** and in
  the "output added" status, formatted as "<alias> (<system name>)"
  so the user sees both the custom name and the original side by
  side. After renaming the banner refreshes immediately.

## What's new in 0.4.2

- **In-app device rename.** Right-click on a device row (in the list or
  the active mixer) → "Rename" / "Reset name". The alias is display only:
  the system WASAPI / CoreAudio / PipeWire name is left untouched, and
  `add_output` / `play_test_signal` still use the original. Stored in
  `session.json` under `deviceAliases: { endpoint_id → name }`, so it
  survives restarts and works for any visible device, active or not.
- **Auto-update via `tauri-plugin-updater`.** Five seconds after start
  (and every 6 hours afterwards) the app polls a `latest.json` manifest
  on GitHub Releases. When a newer version is available a banner appears
  at the top — "Install and restart" downloads + installs + relaunches.
  "Later" hides the banner until an even newer version ships
  (persisted in `localStorage`). Bundle artifacts are signed with a
  separate minisign-format key pair; `latest.json` is produced by
  `tauri-action` in CI when `TAURI_SIGNING_PRIVATE_KEY{,_PASSWORD}`
  secrets are set.
- **CSP widened** to include `github.com` / `api.github.com` /
  `*.githubusercontent.com` (for manifest fetch and bundle download),
  and capabilities gained `updater:default` + `process:allow-restart`.
  Still no `fs/http/shell/dialog` plugins.

## What's new in 0.4.1

Project audit closed 15 items (P1 bugs + P2 structural improvements) in
one release:

- **Stable backend device id.** WASAPI endpoint id
  (`{0.0.0.00000000}.{guid}`) on Windows via `IMMDeviceEnumerator`;
  stringified `AudioObjectID` on macOS. Session profile matches by
  endpoint_id first and falls back to name — renames and duplicate
  names no longer break restore.
- **Output disconnect watchdog.** If a device drops >50 % of pushes
  for 200 worker ticks (~2 s) in a row, the engine drops the output,
  emits an `OutputDisconnected` event, and the UI removes the row.
- **System default device change subscription (Windows).** Poller on
  `IMMDeviceEnumerator::GetDefaultAudioEndpoint`, soft-restart of
  capture on change. The UI shows a "default changed" banner.
- **`catch_unwind` around the audio thread.** Panics no longer kill
  the process or hang `Drop::join`: caught, `engine_status.alive=false`
  exposed, UI shows "engine died, please restart".
- **Drift via `rubato::set_resample_ratio_relative`.** Drop/dup of
  frames is gone; tone signals don't drift in pitch. Linear stretch
  fallback remains for devices without a resampler.
- **Fire-and-forget Tauri commands.** Volume / mute / balance / EQ /
  master no longer block the UI on a channel reply — sliders stay
  responsive even when the audio thread is mid-chunk.
- **EQ in the session profile.** Low/mid/high gain (dB) is serialized
  alongside volume / mute / balance / latency and survives restarts.
- **`CommandError` enum.** Tauri commands return `{kind, message}`
  instead of flat strings; the frontend `switch`es on `kind`, no more
  substring parsing.
- **Unified `DevicePreset`.** `ProfileDeviceDto` is gone — one struct
  with `serde(rename_all = "camelCase")` covers both wire and disk.
- **i18n keys generated from FTL.** `app/build.rs` parses
  `locales/*.ftl`, emits `KEYS` into `OUT_DIR`, and verifies ru↔en
  parity at build time.
- **Frontend ES modules.** `app.js` (1100+ lines) is split into
  `state / ipc / i18n / theme / status / devices / mixer / sync /
  tests / session / events`. No bundler — just `<script type="module">`.
- **Minimal Tauri capabilities.** Only `core:default`, no
  `fs/http/shell/dialog` plugins. The `main` window is named explicitly.
- **Content Security Policy.** `default-src 'self'`, `script-src 'self'`,
  `connect-src 'self' ipc: http://ipc.localhost`, `object-src 'none'`.
- **DriftCorrector kp/ki documented + step-response test.** A comment
  with the stability derivation and a unit test that asserts the
  ramp is monotonic and capped.
- **Rate-limited drop warning.** Accumulated dropped samples are
  reported once per second per device, not on every tick.

## What's new in 0.4.0

- **3-band EQ per device.** Low-shelf 100 Hz, peak 1 kHz, high-shelf
  8 kHz (Q=0.707 / 1.0, RBJ Audio EQ Cookbook). Biquads in Direct
  Form I, per-channel state, coefficients hot-swapped via `ArcSwap`
  with no locking in the callback. Bypass at gain == 0 is a single
  `if` per sample — zero DSP cost.
- **Master gain + master mute.** A global trim sitting on top of
  per-device volume. Mute is instant (atomic in the worker); gain is
  a linear 0–1.5x scale, with a per-device peak meter under each
  volume slider (30 fps).
- **Adaptive clock drift correction.** A PI controller per device
  feeds an error signal `device_t − reference_t` into a small
  resampler-ratio nudge (`±0.001`, 5 ms deadband). Enough to absorb
  typical ±20–50 ppm crystal drift without audible pitch wander on
  test tones.
- **Session profiles.** State (active devices + their volume / mute /
  balance / latency / EQ + master gain / mute) is serialized to JSON
  (`session.json` in the platform data dir) and restored on the next
  launch. A `version` field reserves space for future migrations.
- **Calibration chirp.** A 200 ms linear sweep from 200 Hz to 4 kHz
  available via the `generate_calibration_chirp(sample_rate,
  duration_ms)` Tauri command. The full cross-correlation pipeline
  through loopback is the next increment; this release ships the base
  API a UI "calibrate" button can sit on.
- **`--self-test` smoke mode.** Headless: open capture, attach one
  dummy output, push N samples, shut down cleanly. Used in CI on
  linux/windows/macos in both debug and release profiles.
- **CI workflow.** `.github/workflows/ci.yml`: rustfmt, `cargo doc`
  with `-D rustdoc::broken-intra-doc-links`, `Cargo.lock` check via
  `--locked`, clippy + tests + smoke on three OSes, both profiles.
- **`zound-output` integration tests.** New `crates/zound-output/tests/`
  folder for first-class AudioEngine regression scenarios.

## What's new in 0.3.0

- **Mute and Balance per device.** Each active output now has a Mute
  toggle (zero gain in the cpal callback, click-free) and an L/R
  Balance slider using a constant-power pan law (perceived loudness
  doesn't dip at center). Mute is instant; balance applies in the
  worker thread with no allocations.
- **Test signal** (closes the MVP gap). A 🔊 button per device opens
  a popover with three sources: a single 5 ms click, a 5-second
  1 kHz tone, and a metronome (40-240 BPM). Plays through a separate
  cpal stream alongside the regular output and does not enter the
  loopback.
- **Drift indicator.** A new badge in the Devices panel header:
  "synced" when last-push timestamps are within 50 ms across active
  outputs, "drift X ms" in amber otherwise. Hidden with fewer than
  2 active outputs.
- **Dark/Light theme.** A toggle in the header (☀ / 🌙 / 🌓). Persists.
  The light palette is tuned to the logo gradient; the dark one shifts
  the secondary accent toward violet for visual continuity.
- **Auto-reconnect last session.** On engine start, Zound restores the
  previously active outputs with their volume / latency / balance /
  mute. Missing devices are reported with a status message and the
  rest continue.
- **Realtime-safety fix.** The per-chunk `Vec::with_capacity` in
  `adapt_channels` and `push_to_output` (worker loop) is gone —
  WorkerOutput now holds pre-allocated buffers.

## What's new in 0.2.2

- **macOS — all devices now visible.** Previously `cpal` filtered
  `output_devices` through `default_output_config().is_ok()`, hiding
  idle outputs (DisplayPort, built-in speakers when another default
  was selected). We now bypass cpal's filter via direct CoreAudio
  enumeration.
- **macOS — feedback-loop blocker.** The current system default output
  is now used as the capture session's `source_name`, so adding it as
  a Zound output is blocked (avoiding doubled audio), the same way it
  already worked on Windows.
- **UI — "show all devices" toggle.** A checkbox in the Devices panel:
  off by default (outputs only); on lists inputs too (marked "input
  only", no add button).
- **Branding.** Updated application icon and header logo.

## Stack

- **Core**: Rust (cargo workspace)
- **Audio**: `cpal` + `rubato` + `ringbuf`
- **UI**: Tauri 2, vanilla HTML/CSS/JS (no bundler, no Node build step)
- **i18n**: `fluent-bundle` (concurrent)
- **Logging**: `tracing`

## Repository layout

```
Cargo.toml                 # workspace root
crates/
  zound-core/              # DeviceId, AudioFrame, SampleFormat, converters
  zound-platform/          # AudioBackend + cpal loopback/output
  zound-sync/              # Sync Engine (target latency, compensation)
  zound-output/            # OutputManager + AudioEngine (actor pattern)
app/                       # Tauri application
  src/                     # Rust backend: main, commands, i18n
  src-ui/                  # frontend: index.html, style.css, app.js
  tauri.conf.json
  build.rs                 # also generates a placeholder icon
locales/
  ru.ftl                   # Russian (source of truth)
  en.ftl                   # English
rustfmt.toml
.gitignore
README.md / README.en.md
```

## Quick start

```bash
# Sanity
cargo check --workspace
cargo test --workspace
cargo fmt --check
cargo clippy --workspace -- -D warnings

# Launch the UI
cargo run -p app

# Headless CLI (test the pipeline without a window)
cargo run -p app -- --list
cargo run -p app -- --play "Динамики (Realtek(R) Audio)" --duration 5
cargo run -p app -- --play-default --duration 10
```

Minimum requirements: Rust 1.75+. Node.js is **not** required for the
Tauri window — the frontend is static.

## How the AudioEngine works

`cpal::Stream` is not `Send` on WASAPI, so every stream (capture and
outputs) lives on a single dedicated `zound-audio` thread. The public
`AudioEngine` is a thin handle that talks to that thread over a `mpsc`
command channel. This makes `AudioEngine` itself `Send + Sync` and
fit for `tauri::State` without extra wrapping.

Data flow:

```
[loopback capture]
       │ consumer
       ▼
[audio-thread tick]
       │ copy into per-device chain
       ▼
[resampler?] → [ringbuf producer] → [cpal output callback] → 🎧
```

## A key limitation: "doubled" sound

Loopback is a copy of whatever is playing on the system default output.
The default device keeps playing natively, **and** Zound mirrors the
same sound to the added devices. This is not a bug — that's how the
loopback approach works.

To hear the sound only through Zound outputs, lower the volume on the
original (default) device. Eliminating the duplicate entirely requires
a virtual audio driver (VB-Cable, BlackHole, a custom kext/DriverKit),
which is post-MVP.

## Platforms

| OS | Status | Detail |
|---|---|---|
| Windows 10/11 | ✅ working | WASAPI loopback + multi-device output |
| Linux (PipeWire/Pulse) | 🧪 experimental | monitor source of the default sink via libpulse-simple |
| macOS 13+ | 🧪 experimental | ScreenCaptureKit — first launch asks for screen recording permission (required even for audio-only capture) |

Linux and macOS backends are validated via CI builds only, not yet on real
hardware. File issues with OS, `zound --list` output, and `RUST_LOG=debug`
logs if something breaks.

## Installing from GitHub Releases

Builds are **unsigned** (closed repo, test drop). The OS will warn you —
that's expected, bypass is a two-click thing.

### Windows

1. Download `Zound_x.y.z_x64_en-US.msi` (or `_x64-setup.exe`).
2. Run it — a blue **Windows Defender SmartScreen** window appears:
   "Microsoft Defender SmartScreen prevented an unrecognized app from
   starting".
3. Click **More info** → **Run anyway**.
4. If you see "Unknown publisher" instead — just click **Run**.

PowerShell alternative (pre-unblock so you never see the warning):

```powershell
Unblock-File -Path .\Zound_0.2.0_x64_en-US.msi
```

### macOS

Requires Apple Silicon (M1/M2/M3/M4) and macOS 13 (Ventura) or later.
No Intel build is published right now.

1. Download `Zound_x.y.z_aarch64.dmg`.
2. Open the DMG, drag **Zound.app** into `Applications`.
3. On first launch Gatekeeper will say "Zound is damaged and can't be
   opened" or "cannot be verified". Dismiss the dialog.
4. In Terminal, clear the quarantine attribute:

   ```bash
   xattr -cr /Applications/Zound.app
   ```

5. Launch again — **right click → Open → Open** in the dialog. No-Terminal
   alternative: **System Settings → Privacy & Security**, scroll to the
   bottom for "Zound was blocked…" → **Open Anyway**.
6. macOS will show **"Zound would like to record this computer's screen"**
   — click **Open System Settings**, enable Zound under **Privacy &
   Security → Screen & System Audio Recording**, then relaunch the app.
   This is mandatory: without the permission, ScreenCaptureKit cannot
   deliver system audio and the process terminates immediately.

(Steps 3–4 are one-time per machine; the screen recording permission
is also granted once.)

**If you still get "Can't open the app 'Zound'" after `xattr -cr`**,
that almost always means screen recording permission hasn't been
granted yet, or macOS is older than 13.0. Inspect the crash via:

```bash
log show --predicate 'process == "Zound"' --last 5m --info
```

### Alternative without Zound: built-in Multi-Output Device

If you only need to mirror audio to several devices — without per-device
volumes or latency sliders — macOS can do it natively:

1. **Audio MIDI Setup** (`⌘+Space → "Audio MIDI Setup"`).
2. "+" at the bottom left → **Create Multi-Output Device**.
3. Check the devices you want; enable **Drift Correction** for
   Bluetooth outputs.
4. Pick the new device as the output in the menu-bar volume control.

This does not solve Bluetooth headphones with different initial
latencies (drift correction compensates drift but not the starting
offset) and gives no independent per-device volume — those are the
cases where Zound helps.

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

Requirements: WebKitGTK 4.1 (usually already installed), PipeWire or
PulseAudio.

## Releasing: build and publish

The workflow at `.github/workflows/release.yml` builds for Windows,
macOS (Apple Silicon) and Linux via `tauri-action`. It triggers on a
`v*` tag push:

```bash
git tag v0.4.0
git push origin v0.4.0
```

GitHub Actions creates a **draft release** — open it in the repo UI
and hit Publish. Artifacts (`.msi`, `.exe`, `.dmg`, `.AppImage`,
`.deb`) are attached automatically.

Local build for the current OS:

```bash
cargo install tauri-cli --version '^2' --locked
cd app && cargo tauri build
```

Output lands in `target/release/bundle/`.

## License

MIT OR Apache-2.0.
