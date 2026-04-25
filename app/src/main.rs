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
use std::sync::Arc;
use std::thread;
use std::time::Duration;

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

    let state = AppState {
        engine,
        sync,
        i18n: i18n_inst,
    };

    tauri::Builder::default()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            commands::list_outputs,
            commands::list_all_devices,
            commands::start_engine,
            commands::stop_engine,
            commands::engine_status,
            commands::add_output,
            commands::remove_output,
            commands::set_output_volume,
            commands::set_output_muted,
            commands::set_output_balance,
            commands::set_output_latency,
            commands::target_latency_ms,
            commands::sync_status,
            commands::play_test_signal,
            commands::stop_test_signal,
            commands::load_dictionary,
            commands::format_message,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Zound");
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
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}

struct Args {
    list: bool,
    play: Vec<String>,
    play_default: bool,
    duration: u64,
}

fn parse_args<I: Iterator<Item = String>>(args: I) -> Args {
    let mut out = Args {
        list: false,
        play: Vec::new(),
        play_default: false,
        duration: 5,
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
