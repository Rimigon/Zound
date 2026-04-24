//! Output Manager + Device Manager + AudioEngine.
//!
//! Output Manager хранит per-device состояние (громкость, mute, balance).
//! AudioEngine — реальный pipeline, связывающий capture и все активные
//! output-устройства через ringbuf-ы и ресемплеры.

pub mod engine;

pub use engine::AudioEngine;

use std::collections::HashMap;

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

/// Управляет набором активных output-устройств.
pub struct OutputManager {
    outputs: RwLock<HashMap<DeviceId, OutputState>>,
}

impl OutputManager {
    pub fn new() -> Self {
        Self {
            outputs: RwLock::new(HashMap::new()),
        }
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
}
