//! Output Manager + Device Manager + AudioEngine.
//!
//! Output Manager хранит per-device состояние (громкость, mute, balance).
//! AudioEngine — реальный pipeline, связывающий capture и все активные
//! output-устройства через ringbuf-ы и ресемплеры.

pub mod engine;
pub mod guard;
pub mod profile;

pub use engine::AudioEngine;
pub use guard::{is_capture_source, ERR_FEEDBACK_LOOP};
pub use profile::{DevicePreset, SessionProfile};

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use parking_lot::RwLock;
use zound_core::{DeviceId, Result};

/// Линейная громкость в диапазоне [0.0, 1.0]. 1.0 = unity gain.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Volume(pub f32);

impl Volume {
    pub const MUTE: Self = Self(0.0);
    pub const UNITY: Self = Self(1.0);

    pub fn clamp(self) -> Self {
        Self(self.0.clamp(0.0, 1.0))
    }
}

impl Default for Volume {
    fn default() -> Self {
        Self::UNITY
    }
}

#[derive(Debug, Clone)]
pub struct OutputState {
    pub volume: Volume,
    pub muted: bool,
    /// Баланс -1.0 (только левый) … 0.0 (центр) … 1.0 (только правый).
    pub balance: f32,
}

impl Default for OutputState {
    fn default() -> Self {
        Self {
            volume: Volume::UNITY,
            muted: false,
            balance: 0.0,
        }
    }
}

/// Глобальные master-контролы. Шарятся между UI и audio-thread через
/// atomics — чтение из worker-loop без локов.
#[derive(Clone)]
pub struct MasterControls {
    gain_bits: Arc<AtomicU32>,
    muted: Arc<AtomicBool>,
}

impl MasterControls {
    pub fn new() -> Self {
        Self {
            gain_bits: Arc::new(AtomicU32::new(1.0_f32.to_bits())),
            muted: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Линейный gain в [0.0; 1.0]. Не SoX-style dB — общий простой
    /// множитель, применяемый к каждому output поверх per-device volume.
    pub fn gain(&self) -> f32 {
        f32::from_bits(self.gain_bits.load(Ordering::Relaxed))
    }

    pub fn set_gain(&self, v: f32) {
        self.gain_bits
            .store(v.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    pub fn muted(&self) -> bool {
        self.muted.load(Ordering::Relaxed)
    }

    pub fn set_muted(&self, m: bool) {
        self.muted.store(m, Ordering::Relaxed);
    }

    /// Эффективный множитель: 0.0 если muted, иначе gain.
    /// Удобно вызывать в audio-thread per-chunk, один atomic-load пары.
    pub fn effective_gain(&self) -> f32 {
        if self.muted() {
            0.0
        } else {
            self.gain()
        }
    }
}

impl Default for MasterControls {
    fn default() -> Self {
        Self::new()
    }
}

/// Управляет набором активных output-устройств + глобальные master-
/// контролы (громкость и mute).
pub struct OutputManager {
    outputs: RwLock<HashMap<DeviceId, OutputState>>,
    master: MasterControls,
}

impl OutputManager {
    pub fn new() -> Self {
        Self {
            outputs: RwLock::new(HashMap::new()),
            master: MasterControls::new(),
        }
    }

    pub fn master(&self) -> &MasterControls {
        &self.master
    }

    /// Добавить устройство в активный набор.
    pub fn add(&self, id: DeviceId) {
        self.outputs.write().insert(id, OutputState::default());
    }

    pub fn remove(&self, id: &DeviceId) {
        self.outputs.write().remove(id);
    }

    pub fn set_volume(&self, id: &DeviceId, volume: Volume) -> Result<()> {
        let mut outputs = self.outputs.write();
        match outputs.get_mut(id) {
            Some(state) => {
                state.volume = volume.clamp();
                Ok(())
            }
            None => Err(zound_core::Error::DeviceNotFound(id.to_string())),
        }
    }

    pub fn set_muted(&self, id: &DeviceId, muted: bool) -> Result<()> {
        let mut outputs = self.outputs.write();
        match outputs.get_mut(id) {
            Some(state) => {
                state.muted = muted;
                Ok(())
            }
            None => Err(zound_core::Error::DeviceNotFound(id.to_string())),
        }
    }

    pub fn state(&self, id: &DeviceId) -> Option<OutputState> {
        self.outputs.read().get(id).cloned()
    }

    pub fn active(&self) -> Vec<DeviceId> {
        self.outputs.read().keys().cloned().collect()
    }

    pub fn count(&self) -> usize {
        self.outputs.read().len()
    }
}

impl Default for OutputManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_clamped_to_unit_interval() {
        let m = OutputManager::new();
        let id = DeviceId::from("x");
        m.add(id.clone());
        m.set_volume(&id, Volume(5.0)).unwrap();
        assert_eq!(m.state(&id).unwrap().volume, Volume::UNITY);
        m.set_volume(&id, Volume(-1.0)).unwrap();
        assert_eq!(m.state(&id).unwrap().volume, Volume::MUTE);
    }

    #[test]
    fn operations_on_missing_device_error() {
        let m = OutputManager::new();
        let id = DeviceId::from("missing");
        assert!(m.set_volume(&id, Volume::UNITY).is_err());
        assert!(m.set_muted(&id, true).is_err());
    }

    #[test]
    fn master_default_is_unity_unmuted() {
        let m = MasterControls::new();
        assert_eq!(m.gain(), 1.0);
        assert!(!m.muted());
        assert_eq!(m.effective_gain(), 1.0);
    }

    #[test]
    fn master_mute_overrides_gain() {
        let m = MasterControls::new();
        m.set_gain(0.5);
        assert_eq!(m.effective_gain(), 0.5);
        m.set_muted(true);
        assert_eq!(m.effective_gain(), 0.0);
        m.set_muted(false);
        assert_eq!(m.effective_gain(), 0.5);
    }

    #[test]
    fn master_gain_clamped() {
        let m = MasterControls::new();
        m.set_gain(2.0);
        assert_eq!(m.gain(), 1.0);
        m.set_gain(-0.5);
        assert_eq!(m.gain(), 0.0);
    }
}
