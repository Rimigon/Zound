//! Кроссплатформенные интеграционные тесты — публичный API zound-output
//! без обращений к реальному аудио-железу. Запускаются на всех трёх OS
//! из CI (см. `.github/workflows/ci.yml`).

use std::sync::Arc;
use std::time::Duration;

use zound_core::DeviceId;
use zound_output::{
    AudioEngine, DevicePreset, MasterControls, OutputManager, SessionProfile, Volume,
};
use zound_sync::SyncEngine;

#[test]
fn audio_engine_spawn_drop_cycle() {
    // Spawn / drop 5 раз подряд — проверяем, что audio-thread корректно
    // завершается (без deadlock в Drop).
    for _ in 0..5 {
        let sync = Arc::new(SyncEngine::new());
        let outputs = Arc::new(OutputManager::new());
        let engine = AudioEngine::new(sync, outputs);
        assert!(engine.active_outputs().is_empty());
        drop(engine);
    }
}

#[test]
fn output_manager_master_independent_devices() {
    // Несколько устройств не пересекаются по volume / mute.
    let m = OutputManager::new();
    let a = DeviceId::from("device-a");
    let b = DeviceId::from("device-b");
    m.add(a.clone());
    m.add(b.clone());
    m.set_volume(&a, Volume(0.25)).unwrap();
    m.set_volume(&b, Volume(0.75)).unwrap();
    m.set_muted(&a, true).unwrap();
    assert_eq!(m.state(&a).unwrap().volume.0, 0.25);
    assert_eq!(m.state(&b).unwrap().volume.0, 0.75);
    assert!(m.state(&a).unwrap().muted);
    assert!(!m.state(&b).unwrap().muted);
    assert_eq!(m.count(), 2);
}

#[test]
fn master_state_thread_safe_clone() {
    // MasterControls должен быть Send/Sync — проверяем clone и
    // что чтение из второго потока даёт согласованное значение.
    let m = MasterControls::new();
    m.set_gain(0.42);
    let m2 = m.clone();
    let h = std::thread::spawn(move || m2.gain());
    let observed = h.join().unwrap();
    assert!((observed - 0.42).abs() < 1e-6);
}

#[test]
fn session_profile_in_temp_dir() {
    // Полный roundtrip через temp_dir(). Имя директории включает PID,
    // чтобы параллельные тестовые запуски не топтали друг друга.
    let dir = std::env::temp_dir().join(format!(
        "zound-cross-test-{}-{}",
        std::process::id(),
        line!()
    ));
    let path = dir.join("session.json");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    // Файла нет → load_from вернёт Ok(None).
    assert!(SessionProfile::load_from(&path).unwrap().is_none());

    // Сохраняем и читаем.
    let p = SessionProfile {
        master_gain: 0.9,
        master_muted: false,
        devices: vec![DevicePreset {
            name: "X-Y/Z".into(),
            endpoint_id: Some("test-endpoint:1".into()),
            volume: 0.6,
            muted: true,
            balance: 0.1,
            latency_ms: 33,
            eq_low_db: 0.0,
            eq_mid_db: 4.0,
            eq_high_db: -3.0,
        }],
        ..SessionProfile::default()
    };
    p.save_to(&path).unwrap();
    let read = SessionProfile::load_from(&path).unwrap().unwrap();
    assert_eq!(p, read);

    // Перезапись поверх — atomic save (tmp+rename) должен работать.
    let mut p2 = p.clone();
    p2.master_gain = 0.1;
    p2.save_to(&path).unwrap();
    let read2 = SessionProfile::load_from(&path).unwrap().unwrap();
    assert_eq!(p2.master_gain, read2.master_gain);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn session_profile_with_unicode_device_name() {
    // На Windows пути могут не любить юникод, на macOS нужно NFD-форму
    // — наш save/load через std::fs должен это переваривать.
    let dir = std::env::temp_dir().join(format!("zound-utf-test-{}", std::process::id()));
    let path = dir.join("session.json");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let p = SessionProfile {
        devices: vec![
            DevicePreset::new("Наушники (EDIFIER W820NB Plus) 🎧"),
            DevicePreset::new("内蔵スピーカー"),
        ],
        ..SessionProfile::default()
    };
    p.save_to(&path).unwrap();
    let read = SessionProfile::load_from(&path).unwrap().unwrap();
    assert_eq!(p.devices.len(), read.devices.len());
    assert_eq!(p.devices[0].name, read.devices[0].name);
    assert_eq!(p.devices[1].name, read.devices[1].name);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn sync_engine_recalculates_after_set_all() {
    // Проверяем, что set_all_latencies приводит target к ожидаемому
    // значению независимо от количества устройств.
    let sync = SyncEngine::new();
    for i in 0..5 {
        sync.add_device(
            DeviceId::from(format!("d{i}")),
            Duration::from_millis(i * 30),
            48_000,
        );
    }
    let n = sync.set_all_latencies(Duration::from_millis(100));
    assert_eq!(n, 5);
    // target = max(intrinsic) + margin = 100 + 20 = 120 ms.
    assert_eq!(sync.target_latency().as_millis(), 120);
}

#[test]
fn audio_engine_handles_remove_of_unknown_device() {
    // remove_output на неактивном устройстве не должен паниковать /
    // зависать.
    let sync = Arc::new(SyncEngine::new());
    let outputs = Arc::new(OutputManager::new());
    let engine = AudioEngine::new(sync, outputs);
    engine.remove_output(&DeviceId::from("ghost"));
    drop(engine);
}

#[test]
fn audio_engine_set_volume_unknown_is_silent_warning() {
    // P1.6 — set_volume стал fire-and-forget. Ошибки (device not found)
    // ловятся в audio-thread и пишутся в `tracing::warn!`, не возвращаются
    // наружу. UI-сторона остаётся отзывчивой на каждое движение слайдера.
    let sync = Arc::new(SyncEngine::new());
    let outputs = Arc::new(OutputManager::new());
    let engine = AudioEngine::new(sync, outputs);
    engine.set_volume(&DeviceId::from("nope"), 0.5);
    // Engine остаётся жив.
    assert!(engine.health().is_alive());
    drop(engine);
}
