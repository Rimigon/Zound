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

use std::collections::HashMap;
use std::time::Duration;

use parking_lot::RwLock;
use zound_core::{DeviceId, Result};

/// Минимально возможный буфер безопасности. 20 мс — запас на джиттер
/// callback-ов.
pub const MIN_SAFETY_MARGIN: Duration = Duration::from_millis(20);

/// Целевая общая latency по умолчанию. 200 мс хватает под BT SBC.
pub const DEFAULT_TARGET_LATENCY: Duration = Duration::from_millis(200);

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
    /// compensation_delay для всех устройств.
    pub fn add_device(&self, id: DeviceId, intrinsic_latency: Duration, sample_rate: u32) {
        let mut inner = self.inner.write();
        inner.devices.insert(
            id,
            DeviceSyncParams {
                intrinsic_latency,
                compensation_delay: Duration::ZERO,
                sample_rate,
            },
        );
        Self::recalculate(&mut inner);
    }

    pub fn remove_device(&self, id: &DeviceId) {
        let mut inner = self.inner.write();
        inner.devices.remove(id);
        Self::recalculate(&mut inner);
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
}
