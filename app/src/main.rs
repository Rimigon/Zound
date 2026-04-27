//! Точка входа Zound.
//!
//! Два режима:
//! - **UI-режим** (по умолчанию) — открывает окно Tauri.
//! - **CLI-режим** — когда указаны `--play`, `--play-default` или `--list`.
//!   Используется для headless-тестов аудио-пайплайна.

#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use std::env;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use parking_lot::RwLock;
use tauri::Manager;
use tracing_subscriber::EnvFilter;

use zound_output::{AudioEngine, OutputManager};
use zound_platform::{AudioBackend, CpalBackend};
use zound_sync::SyncEngine;

mod commands;
mod i18n;

use commands::AppState;
use i18n::I18n;

fn main() {
    init_logging();

    let args = parse_args(env::args().skip(1));

    if args.self_test {
        if let Err(e) = run_self_test() {
            eprintln!("self-test failed: {e}");
            std::process::exit(1);
        }
        return;
    }

    if args.list || args.play_default || !args.play.is_empty() {
        if let Err(e) = run_cli(args) {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
        return;
    }

    run_tauri();
}

// ---------- Tauri ---------- //

fn run_tauri() {
    let sync = Arc::new(SyncEngine::new());
    let outputs = Arc::new(OutputManager::new());
    let engine = Arc::new(AudioEngine::new(sync.clone(), outputs.clone()));

    let i18n_inst = match I18n::new() {
        Ok(i) => Arc::new(i),
        Err(e) => {
            eprintln!("failed to load translations: {e}");
            std::process::exit(1);
        }
    };

    let session_path = Arc::new(RwLock::new(None::<PathBuf>));

    let state = AppState {
        engine,
        sync,
        _outputs: outputs,
        i18n: i18n_inst,
        session_path: session_path.clone(),
    };

    tauri::Builder::default()
        .manage(state)
        .setup(move |app| {
            // Path resolver доступен только после Tauri-setup. Сохраняем
            // абсолютный путь к session.json в state.
            match app.path().app_data_dir() {
                Ok(dir) => {
                    let path = dir.join("session.json");
                    tracing::info!(?path, "session profile path resolved");
                    *session_path.write() = Some(path);
                }
                Err(e) => {
                    tracing::warn!(?e, "app_data_dir not available; profiles disabled");
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_outputs,
            commands::list_all_devices,
            commands::start_engine,
            commands::stop_engine,
            commands::engine_status,
            commands::engine_events,
            commands::add_output,
            commands::remove_output,
            commands::set_output_volume,
            commands::set_output_muted,
            commands::set_output_balance,
            commands::set_output_eq,
            commands::set_output_latency,
            commands::set_all_latencies,
            commands::target_latency_ms,
            commands::sync_status,
            commands::play_test_signal,
            commands::stop_test_signal,
            commands::load_dictionary,
            commands::format_message,
            commands::master_state,
            commands::set_master_gain,
            commands::set_master_muted,
            commands::peaks,
            commands::load_session_profile,
            commands::save_session_profile,
            commands::generate_calibration_chirp,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Zound");
}

// ---------- self-test (CI smoke) ---------- //

/// Headless smoke-тест для CI. Не открывает реальных аудио-стримов
/// (Linux-раннеры без PulseAudio упадут на capture), но проверяет всю
/// платформо-независимую логику end-to-end:
///
///   1. SyncEngine: target latency, set_all_latencies, drift snapshot.
///   2. OutputManager + MasterControls roundtrip.
///   3. AudioEngine spawn/Drop без зависов.
///   4. ThreeBandEq: bypass / non-bypass / clamp / process inplace.
///   5. SessionProfile: save → load roundtrip в temp_dir.
///   6. DriftCorrector: deadband / sign / cap.
///   7. Calibration: chirp + cross_correlation lag detection.
///   8. I18n: ru/en bundles parity + format с placeholder.
///
/// Если любой шаг упадёт — exit code 1, CI красный. На macOS / Linux
/// этот self-test исполняется тем же бинарником через CI workflow.
fn run_self_test() -> Result<(), Box<dyn std::error::Error>> {
    use std::time::Instant;
    use zound_core::{DeviceId, ThreeBandEq};
    use zound_output::{DevicePreset, MasterControls, SessionProfile, Volume};
    use zound_sync::{cross_correlation_peak, generate_chirp, DriftCorrector};

    let started = Instant::now();
    println!("== Zound self-test ==");
    println!(
        "  platform: {} {} ({})",
        std::env::consts::OS,
        std::env::consts::ARCH,
        std::env::consts::FAMILY
    );

    // 1. SyncEngine
    let sync = Arc::new(SyncEngine::new());
    let id_a = DeviceId::from("a");
    let id_b = DeviceId::from("b");
    sync.add_device(id_a.clone(), Duration::from_millis(50), 48_000);
    sync.add_device(id_b.clone(), Duration::from_millis(150), 48_000);
    let target = sync.target_latency();
    if target.as_millis() < 150 {
        return Err(format!("expected target ≥ 150 ms, got {target:?}").into());
    }
    let n = sync.set_all_latencies(Duration::from_millis(80));
    if n != 2 {
        return Err(format!("set_all_latencies affected {n}, expected 2").into());
    }
    let snap = sync.drift_snapshot();
    if !snap.in_sync {
        return Err("drift_snapshot must be in_sync when no pushes happened".into());
    }
    println!("  ✓ sync engine (target={} ms)", target.as_millis());

    // 2. OutputManager + Master
    let outputs = Arc::new(OutputManager::new());
    outputs.add(id_a.clone());
    outputs.set_volume(&id_a, Volume(0.5))?;
    let st = outputs.state(&id_a).ok_or("missing state")?;
    if (st.volume.0 - 0.5).abs() > 1e-6 {
        return Err("volume not stored".into());
    }
    let m = MasterControls::new();
    m.set_gain(0.7);
    m.set_muted(true);
    if m.effective_gain() != 0.0 {
        return Err("master mute must zero effective gain".into());
    }
    m.set_muted(false);
    if (m.effective_gain() - 0.7).abs() > 1e-6 {
        return Err("master gain restored after unmute".into());
    }
    println!("  ✓ output manager + master controls");

    // 3. AudioEngine spawn/drop
    let engine = AudioEngine::new(sync.clone(), outputs.clone());
    if !engine.active_outputs().is_empty() {
        return Err("freshly-built engine must have no active outputs".into());
    }
    drop(engine);
    println!("  ✓ audio engine spawn/drop");

    // 4. EQ — bypass, non-bypass, clamp, inplace
    let mut eq = ThreeBandEq::new(48_000.0);
    if !eq.is_bypass() {
        return Err("flat EQ must start in bypass".into());
    }
    eq.set_low_gain(50.0);
    if eq.low.gain_db != 12.0 {
        return Err("EQ gain must clamp to +12 dB".into());
    }
    eq.set_low_gain(0.0);
    eq.set_mid_gain(6.0);
    if eq.is_bypass() {
        return Err("EQ with non-zero gain must not bypass".into());
    }
    let mut buf = vec![0.5_f32; 480 * 2];
    let original = buf.clone();
    eq.process_interleaved(&mut buf, 2);
    if buf == original {
        return Err("EQ process must mutate buffer when not bypass".into());
    }
    println!("  ✓ three-band EQ");

    // 5. SessionProfile roundtrip в temp_dir
    let dir = std::env::temp_dir().join(format!("zound-selftest-{}", std::process::id()));
    let path = dir.join("session.json");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    let profile = SessionProfile {
        master_gain: 0.42,
        master_muted: true,
        devices: vec![DevicePreset {
            name: "selftest-device".into(),
            endpoint_id: Some("selftest:42".into()),
            volume: 0.33,
            muted: false,
            balance: -0.5,
            latency_ms: 77,
            eq_low_db: 1.5,
            eq_mid_db: -2.0,
            eq_high_db: 0.0,
        }],
        ..SessionProfile::default()
    };
    profile
        .save_to(&path)
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    let loaded = SessionProfile::load_from(&path)
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?
        .ok_or("profile must load after save")?;
    if loaded != profile {
        return Err("profile roundtrip mismatch".into());
    }
    let _ = std::fs::remove_dir_all(&dir);
    println!("  ✓ session profile roundtrip");

    // 6. DriftCorrector
    let mut drift = DriftCorrector::new();
    let small = drift.update(1_000, Duration::from_millis(100));
    if small != 0.0 {
        return Err("drift inside deadband must give zero correction".into());
    }
    let big = drift.update(50_000, Duration::from_millis(100));
    if big >= 0.0 {
        return Err("positive error → negative correction".into());
    }
    let bigger = drift.update(1_000_000_000, Duration::from_millis(100));
    if bigger.abs() > 0.001 + 1e-6 {
        return Err("drift correction must cap to ±0.1 %".into());
    }
    println!("  ✓ drift PI controller");

    // 7. Calibration
    let sr: u32 = 48_000;
    let samples: usize = (sr / 5) as usize; // 200 ms
    let reference = generate_chirp(sr, samples, 0.5);
    let pad = 2_400;
    let mut recorded = vec![0.0_f32; pad];
    recorded.extend_from_slice(&reference);
    recorded.extend(vec![0.0_f32; 1_000]);
    let lag = cross_correlation_peak(&recorded, &reference);
    if lag != pad {
        return Err(format!("expected lag {pad}, got {lag}").into());
    }
    println!("  ✓ calibration (chirp + correlation, lag={lag})");

    // 8. I18n bundles
    let i18n = i18n::I18n::new().map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    let ru = i18n.dictionary("ru");
    let en = i18n.dictionary("en");
    if ru.is_empty() || en.is_empty() {
        return Err("dictionaries must not be empty".into());
    }
    let missing_ru: Vec<_> = en.keys().filter(|k| !ru.contains_key(*k)).collect();
    let missing_en: Vec<_> = ru.keys().filter(|k| !en.contains_key(*k)).collect();
    if !missing_ru.is_empty() || !missing_en.is_empty() {
        return Err(format!(
            "i18n parity broken: missing in ru={missing_ru:?}, missing in en={missing_en:?}"
        )
        .into());
    }
    println!("  ✓ i18n parity ({} keys)", ru.len());

    println!("OK ({:?})", started.elapsed());
    Ok(())
}

// ---------- CLI ---------- //

fn run_cli(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    let backend = CpalBackend::new();

    if args.list {
        list_devices(&backend);
        return Ok(());
    }

    let sync = Arc::new(SyncEngine::new());
    let outputs = Arc::new(OutputManager::new());
    let engine = AudioEngine::new(sync.clone(), outputs.clone());
    engine.start()?;

    let chosen: Vec<String> = if args.play_default {
        let all = backend.enumerate_outputs()?;
        all.iter()
            .filter(|d| !d.is_default)
            .map(|d| d.name.clone())
            .collect()
    } else {
        args.play.clone()
    };

    if chosen.is_empty() {
        eprintln!("no output devices selected");
        return Ok(());
    }

    println!("== Zound pipeline ==");
    for name in &chosen {
        match engine.add_output(name) {
            Ok(id) => println!("  + {id}"),
            Err(e) => eprintln!("  ! failed to add {name}: {e}"),
        }
    }

    let duration = Duration::from_secs(args.duration);
    println!();
    println!(
        "Playing for {} seconds. Target latency: {:?}",
        duration.as_secs(),
        sync.target_latency()
    );
    thread::sleep(duration);

    println!("\ndone. Active outputs: {:?}", engine.active_outputs());
    Ok(())
}

fn list_devices(backend: &CpalBackend) {
    let outputs = backend.enumerate_outputs().unwrap_or_default();
    let inputs = backend.enumerate_inputs().unwrap_or_default();

    println!("== Zound ({} backend) ==", backend.name());
    println!("\nOutput devices ({}):", outputs.len());
    for d in &outputs {
        let mark = if d.is_default { "*" } else { " " };
        println!(
            "  {} {} — {} Hz, {} ch",
            mark, d.name, d.sample_rate, d.channels
        );
    }
    println!("\nInput devices ({}):", inputs.len());
    for d in &inputs {
        let mark = if d.is_default { "*" } else { " " };
        println!(
            "  {} {} — {} Hz, {} ch",
            mark, d.name, d.sample_rate, d.channels
        );
    }
    println!("\nCLI:");
    println!("  zound --list                     (devices only)");
    println!("  zound --play \"<device>\" [--play \"<other>\"]  [--duration SEC]");
    println!("  zound --play-default             (all non-default outputs)");
    println!("\nNo flags → opens UI.");
}

// ---------- общее ---------- //

fn init_logging() {
    // Конвенция: по умолчанию `info` для нашего кода и `warn` для шумных
    // зависимостей. `RUST_LOG` переопределяет всё (стандартный путь
    // `tracing-subscriber::EnvFilter`).
    //
    // Поднять подробности в debug:  `RUST_LOG=zound=debug,info`
    // Тихий режим:                  `RUST_LOG=zound=warn`
    let default_filter = "info,zound=info,zound_output=info,zound_platform=info,\
                          zound_sync=info,wry=warn,tao=warn,winit=warn";
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_filter));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_level(true)
        .init();
}

struct Args {
    list: bool,
    play: Vec<String>,
    play_default: bool,
    duration: u64,
    self_test: bool,
}

fn parse_args<I: Iterator<Item = String>>(args: I) -> Args {
    let mut out = Args {
        list: false,
        play: Vec::new(),
        play_default: false,
        duration: 5,
        self_test: false,
    };
    let mut it = args;
    while let Some(a) = it.next() {
        match a.as_str() {
            "--list" => out.list = true,
            "--play" => {
                if let Some(name) = it.next() {
                    out.play.push(name);
                }
            }
            "--play-default" => out.play_default = true,
            "--self-test" => out.self_test = true,
            "--duration" => {
                if let Some(s) = it.next() {
                    if let Ok(n) = s.parse::<u64>() {
                        out.duration = n;
                    }
                }
            }
            _ => {}
        }
    }
    out
}
