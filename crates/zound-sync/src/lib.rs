//! Sync Engine — выравнивание воспроизведения между несколькими устройствами.
//!
//! Центральная идея: для каждого устройства известна его latency `L_i`.
//! Общая целевая latency `L = max(L_i) + margin`. Каждому устройству
//! добавляется искусственная задержка `L - L_i`, чтобы сэмпл с тем же
//! исходным timestamp-ом доходил до всех одновременно.
//!
//! На MVP — ручная калибровка: слайдер latency на устройство. Авто-
//! калибровка и компенсация дрейфа (PLL-подобная коррекция ресемплинга)
//! — после MVP. Детали в `.claude/skills/zound-audio-sync/SKILL.md`.

pub mod calibration;
pub mod drift;

pub use calibration::{
    cross_correlation_peak, generate_chirp, lag_to_micros, CalibrationOutcome,
    DEFAULT_CHIRP_DURATION_MS,
};
pub use drift::{DriftCorrector, DEADBAND_MICROS, MAX_CORRECTION};

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;
use zound_core::{DeviceId, Result};

/// Минимально возможный буфер безопасности. 20 мс — запас на джиттер
/// callback-ов.
pub const MIN_SAFETY_MARGIN: Duration = Duration::from_millis(20);

/// Целевая общая latency по умолчанию. 200 мс хватает под BT SBC.
pub const DEFAULT_TARGET_LATENCY: Duration = Duration::from_millis(200);

/// Порог, выше которого считаем устройства рассинхронизованными.
/// Используется UI для индикатора drift.
pub const DRIFT_THRESHOLD_MS: u64 = 50;

/// Настройки одного устройства, учитываемые при синхронизации.
#[derive(Debug, Clone)]
pub struct DeviceSyncParams {
    /// Измеренная/заданная собственная latency устройства.
    pub intrinsic_latency: Duration,
    /// Искусственная задержка, которую Sync Engine добавляет, чтобы
    /// выровнять устройство до общей цели. Вычисляется, не задаётся.
    pub compensation_delay: Duration,
    /// Текущий sample rate устройства (может отличаться от внутреннего).
    pub sample_rate: u32,
    /// Timestamp последнего push-а в ringbuf этого устройства (UNIX epoch
    /// micros). 0 = ещё не пушили (ignored aggregator-ом). Worker сторит
    /// это после каждого `producer.push_slice`.
    pub last_push_micros: Arc<AtomicU64>,
}

/// Снимок drift-состояния для UI.
#[derive(Debug, Clone, Copy)]
pub struct DriftSnapshot {
    pub drift_ms: u32,
    pub active_count: u32,
    pub in_sync: bool,
}

pub struct SyncEngine {
    inner: RwLock<Inner>,
}

struct Inner {
    target_latency: Duration,
    safety_margin: Duration,
    devices: HashMap<DeviceId, DeviceSyncParams>,
}

impl SyncEngine {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(Inner {
                target_latency: DEFAULT_TARGET_LATENCY,
                safety_margin: MIN_SAFETY_MARGIN,
                devices: HashMap::new(),
            }),
        }
    }

    /// Добавить устройство с известной latency. Пересчитывает
    /// compensation_delay для всех устройств. Возвращает Arc на
    /// `last_push_micros` — worker должен хранить clone и обновлять
    /// его после каждого push-а в ringbuf.
    pub fn add_device(
        &self,
        id: DeviceId,
        intrinsic_latency: Duration,
        sample_rate: u32,
    ) -> Arc<AtomicU64> {
        let last_push_micros = Arc::new(AtomicU64::new(0));
        let mut inner = self.inner.write();
        inner.devices.insert(
            id,
            DeviceSyncParams {
                intrinsic_latency,
                compensation_delay: Duration::ZERO,
                sample_rate,
                last_push_micros: last_push_micros.clone(),
            },
        );
        Self::recalculate(&mut inner);
        last_push_micros
    }

    pub fn remove_device(&self, id: &DeviceId) {
        let mut inner = self.inner.write();
        inner.devices.remove(id);
        Self::recalculate(&mut inner);
    }

    /// Поставить одинаковую intrinsic latency всем устройствам сразу.
    /// Используется UI-режимом «связанные задержки»: пользователь крутит
    /// один слайдер, все остальные подтягиваются. Возвращает количество
    /// затронутых устройств.
    pub fn set_all_latencies(&self, latency: Duration) -> usize {
        let mut inner = self.inner.write();
        let n = inner.devices.len();
        for params in inner.devices.values_mut() {
            params.intrinsic_latency = latency;
        }
        Self::recalculate(&mut inner);
        n
    }

    /// Ручная корректировка intrinsic latency (слайдер в UI).
    pub fn set_device_latency(&self, id: &DeviceId, latency: Duration) -> Result<()> {
        let mut inner = self.inner.write();
        match inner.devices.get_mut(id) {
            Some(params) => {
                params.intrinsic_latency = latency;
                Self::recalculate(&mut inner);
                Ok(())
            }
            None => Err(zound_core::Error::DeviceNotFound(id.to_string())),
        }
    }

    pub fn target_latency(&self) -> Duration {
        self.inner.read().target_latency
    }

    pub fn device_params(&self, id: &DeviceId) -> Option<DeviceSyncParams> {
        self.inner.read().devices.get(id).cloned()
    }

    pub fn device_count(&self) -> usize {
        self.inner.read().devices.len()
    }

    /// Per-device смещение в micros относительно среднего по «живым»
    /// устройствам. Положительное = устройство опережает агрегат
    /// (нужно замедлить). Используется адаптивным `DriftCorrector` в
    /// audio-thread.
    ///
    /// При <2 живых устройств возвращает пустой Vec (корректировка не
    /// нужна).
    pub fn per_device_errors(&self) -> Vec<(DeviceId, i64)> {
        let inner = self.inner.read();
        let mut samples: Vec<(DeviceId, u64)> = Vec::new();
        for (id, p) in inner.devices.iter() {
            let t = p.last_push_micros.load(Ordering::Relaxed);
            if t != 0 {
                samples.push((id.clone(), t));
            }
        }
        if samples.len() < 2 {
            return Vec::new();
        }
        let mean: i128 =
            samples.iter().map(|(_, t)| *t as i128).sum::<i128>() / samples.len() as i128;
        samples
            .into_iter()
            .map(|(id, t)| (id, t as i128 - mean))
            .map(|(id, e)| (id, e.clamp(i64::MIN as i128, i64::MAX as i128) as i64))
            .collect()
    }

    /// Снимок drift между устройствами. drift = max - min последних
    /// timestamp'ов push-а; нули (устройство ещё не пушило) игнорируются.
    /// При <2 «живых» устройств drift = 0, in_sync = true.
    pub fn drift_snapshot(&self) -> DriftSnapshot {
        let inner = self.inner.read();
        let mut min_t = u64::MAX;
        let mut max_t = 0u64;
        let mut active = 0u32;
        for params in inner.devices.values() {
            let t = params.last_push_micros.load(Ordering::Relaxed);
            if t == 0 {
                continue;
            }
            active += 1;
            if t < min_t {
                min_t = t;
            }
            if t > max_t {
                max_t = t;
            }
        }
        if active < 2 {
            return DriftSnapshot {
                drift_ms: 0,
                active_count: active,
                in_sync: true,
            };
        }
        let drift_micros = max_t.saturating_sub(min_t);
        let drift_ms = (drift_micros / 1000) as u32;
        DriftSnapshot {
            drift_ms,
            active_count: active,
            in_sync: (drift_ms as u64) <= DRIFT_THRESHOLD_MS,
        }
    }

    /// Пересчёт целевой latency и компенсаций. Вызывается под write-lock.
    fn recalculate(inner: &mut Inner) {
        let max_intrinsic = inner
            .devices
            .values()
            .map(|p| p.intrinsic_latency)
            .max()
            .unwrap_or(Duration::ZERO);
        inner.target_latency = max_intrinsic + inner.safety_margin;

        for params in inner.devices.values_mut() {
            params.compensation_delay = inner
                .target_latency
                .saturating_sub(params.intrinsic_latency);
        }
    }
}

impl Default for SyncEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn compensation_equals_margin_for_slowest_device() {
        let eng = SyncEngine::new();
        eng.add_device(DeviceId::from("fast"), ms(50), 48_000);
        eng.add_device(DeviceId::from("slow"), ms(150), 48_000);

        let slow = eng.device_params(&DeviceId::from("slow")).unwrap();
        let fast = eng.device_params(&DeviceId::from("fast")).unwrap();

        // Целевая = max(intrinsic) + margin, поэтому даже у самого
        // медленного остаётся margin-запас.
        assert_eq!(slow.compensation_delay, MIN_SAFETY_MARGIN);
        assert_eq!(fast.compensation_delay, ms(100) + MIN_SAFETY_MARGIN);
        assert_eq!(eng.target_latency(), ms(150) + MIN_SAFETY_MARGIN);
    }

    #[test]
    fn removing_slowest_recalculates_target() {
        let eng = SyncEngine::new();
        eng.add_device(DeviceId::from("fast"), ms(30), 48_000);
        eng.add_device(DeviceId::from("slow"), ms(200), 48_000);
        assert_eq!(eng.target_latency(), ms(200) + MIN_SAFETY_MARGIN);

        eng.remove_device(&DeviceId::from("slow"));
        assert_eq!(eng.target_latency(), ms(30) + MIN_SAFETY_MARGIN);
    }

    #[test]
    fn set_latency_unknown_device_errors() {
        let eng = SyncEngine::new();
        let err = eng.set_device_latency(&DeviceId::from("ghost"), ms(100));
        assert!(err.is_err());
    }

    #[test]
    fn set_all_latencies_applies_to_every_device() {
        let eng = SyncEngine::new();
        eng.add_device(DeviceId::from("a"), ms(20), 48_000);
        eng.add_device(DeviceId::from("b"), ms(150), 48_000);
        eng.add_device(DeviceId::from("c"), ms(80), 48_000);

        let n = eng.set_all_latencies(ms(100));
        assert_eq!(n, 3);

        // У всех intrinsic = 100, значит compensation = margin (одна и та же).
        for id in ["a", "b", "c"] {
            let p = eng.device_params(&DeviceId::from(id)).unwrap();
            assert_eq!(p.intrinsic_latency, ms(100));
            assert_eq!(p.compensation_delay, MIN_SAFETY_MARGIN);
        }
        assert_eq!(eng.target_latency(), ms(100) + MIN_SAFETY_MARGIN);
    }

    #[test]
    fn set_all_latencies_on_empty_returns_zero() {
        let eng = SyncEngine::new();
        assert_eq!(eng.set_all_latencies(ms(100)), 0);
    }
}
