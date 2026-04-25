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
- 🎚 Per-device **volume**, updated atomically inside the realtime
  callback.
- ⏱ Manual per-device **latency**; target latency is recomputed
  automatically by the Sync Engine.
- 🔁 Automatic **resampling** (`rubato`) when capture and output
  sample rates differ (e.g. 44.1 kHz → 48 kHz).
- 🛡 **Feedback-loop protection**: the capture source device cannot be
  added as an output. Works on Windows and macOS.
- 🔄 **Auto device refresh** every 2 seconds.
- 🔍 **"Show all devices"** toggle (off by default — outputs only; on —
  inputs are also listed but cannot be added).
- ▶️⏹ Start/stop pipeline from the UI without restarting the app.
- 🌐 Language switch ru/en (Project Fluent, `.ftl` dictionaries).

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
git tag v0.2.0
git push origin v0.2.0
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
