# Zound

> Russian version (primary): [README.md](./README.md)

A multi-device audio hub: "Spotify Connect for any system audio".
Captures system loopback and streams it synchronously to the chosen
devices (headphones, speakers, Bluetooth) with independent volumes,
manual latency compensation, and a switchable UI (Russian / English).

## What it does today (MVP)

- 🎧 WASAPI loopback capture from the current default output.
- 🔊 Parallel output to **N devices** at the same time.
- 🎚 Per-device **volume**, updated atomically inside the realtime
  callback.
- ⏱ Manual per-device **latency**; target latency is recomputed
  automatically by the Sync Engine.
- 🔁 Automatic **resampling** (`rubato`) when capture and output
  sample rates differ (e.g. 44.1 kHz → 48 kHz).
- 🛡 **Feedback-loop protection**: the capture source device cannot be
  added as an output.
- 🔄 **Auto device refresh** every 2 seconds.
- ▶️⏹ Start/stop pipeline from the UI without restarting the app.
- 🌐 Language switch ru/en (Project Fluent, `.ftl` dictionaries).

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
| macOS 13+ | ⚠️ UI only | window boots, capture is a no-op (needs ScreenCaptureKit) |
| Linux (PipeWire) | ⚠️ UI only | window boots, capture is a no-op (needs PipeWire monitor) |

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
Unblock-File -Path .\Zound_0.1.0_x64_en-US.msi
```

### macOS

1. Download `Zound_x.y.z_aarch64.dmg` (Apple Silicon) or
   `Zound_x.y.z_x64.dmg` (Intel).
2. Open the DMG, drag **Zound.app** into `Applications`.
3. On first launch Gatekeeper will say "Zound is damaged and can't be
   opened" or "cannot be verified". Dismiss the dialog.
4. In Terminal, clear the quarantine attribute:

   ```bash
   xattr -cr /Applications/Zound.app
   ```

5. Launch again — **right click → Open → Open** in the dialog.

(The first two steps are one-time per machine; after `xattr -cr` the
app launches normally via double-click.)

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

Requirements: WebKitGTK 4.1 (usually already installed), PipeWire or
PulseAudio.

## Releasing: build and publish

The workflow at `.github/workflows/release.yml` builds for Windows,
macOS (Intel + ARM) and Linux via `tauri-action`. It triggers on a
`v*` tag push:

```bash
git tag v0.1.0
git push origin v0.1.0
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
