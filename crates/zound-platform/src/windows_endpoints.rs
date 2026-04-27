//! WASAPI endpoint id и watcher смены default-устройства.
//!
//! `cpal` отдаёт только friendly-name. Для стабильного матчинга в session
//! profile нужен endpoint id (`{0.0.0.00000000}.{guid-...}`) — он
//! переживает переименование и одинаково именованные устройства различает.
//!
//! Дополнительно поднимаем простой watcher: фоновый поток поллит
//! `IMMDeviceEnumerator::GetDefaultAudioEndpoint(eRender, eConsole)`, и
//! при смене endpoint-а вызывает callback. Полноценный
//! `IMMNotificationClient` через COM-implement — больше боли, чем пользы
//! (vtable, AddRef/Release, регистрация); поллинг раз в 500 мс
//! функционально эквивалентен и не имеет COM-edge-cases.
//!
//! Все вызовы COM сериализуются через ленивый `CoInitializeEx` —
//! безопасно и в одиночку, и совместно с cpal-овским WASAPI backend
//! (флаг MULTITHREADED совместим с cpal).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use windows::core::{Interface, GUID, PCWSTR};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Media::Audio::{
    eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ,
};

/// Идемпотентный CoInitializeEx. `RPC_E_CHANGED_MODE` означает «уже
/// инициализировано в другом режиме» — это нормально, мы продолжаем.
fn ensure_com_initialized() {
    static INIT: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    INIT.get_or_init(|| unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    });
}

/// Вернуть `(friendly_name → endpoint_id)` для всех активных render-устройств.
/// Пустой map при ошибке COM или отсутствии устройств.
pub fn endpoint_id_map() -> HashMap<String, String> {
    ensure_com_initialized();
    let mut out = HashMap::new();
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            match CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) {
                Ok(e) => e,
                Err(e) => {
                    tracing::warn!(?e, "MMDeviceEnumerator CoCreateInstance");
                    return out;
                }
            };
        let coll = match enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(?e, "EnumAudioEndpoints");
                return out;
            }
        };
        let count = coll.GetCount().unwrap_or(0);
        for i in 0..count {
            let dev = match coll.Item(i) {
                Ok(d) => d,
                Err(_) => continue,
            };
            let id_pwstr = match dev.GetId() {
                Ok(p) => p,
                Err(_) => continue,
            };
            let id = pwstr_to_string(id_pwstr.as_ptr());
            let store = match dev.OpenPropertyStore(STGM_READ) {
                Ok(s) => s,
                Err(_) => {
                    out.entry(id.clone()).or_insert(id.clone());
                    continue;
                }
            };
            let propvar = match store.GetValue(&PKEY_Device_FriendlyName as *const _) {
                Ok(p) => p,
                Err(_) => continue,
            };
            let name = propvariant_to_string(&propvar).unwrap_or_default();
            if !name.is_empty() {
                out.insert(name, id);
            }
        }
    }
    out
}

/// Endpoint id текущего системного default render-устройства, либо None.
pub fn default_render_endpoint_id() -> Option<String> {
    ensure_com_initialized();
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
        let dev = enumerator.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
        let id_pwstr = dev.GetId().ok()?;
        Some(pwstr_to_string(id_pwstr.as_ptr()))
    }
}

unsafe fn pwstr_to_string(p: *const u16) -> String {
    if p.is_null() {
        return String::new();
    }
    let mut len = 0;
    while *p.add(len) != 0 {
        len += 1;
    }
    let slice = std::slice::from_raw_parts(p, len);
    String::from_utf16_lossy(slice)
}

/// Достать строку из PROPVARIANT (`VT_LPWSTR`). Возвращает None для
/// неподдерживаемых типов.
unsafe fn propvariant_to_string(pv: &PROPVARIANT) -> Option<String> {
    // PROPVARIANT — union; используем стабильный helper from windows crate.
    let s = pv.to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

// Глушитель неиспользуемых символов на debug-сборке.
#[allow(dead_code)]
const _PKEY_GUID_PROBE: GUID = PKEY_Device_FriendlyName.fmtid;
#[allow(dead_code)]
fn _interface_probe<I: Interface>(_: &I) {}
#[allow(dead_code)]
fn _pcwstr_probe(_: PCWSTR) {}

/// Watcher смены default render-устройства. При вызове запускает
/// фоновый поток, который раз в `poll_interval` поллит default endpoint
/// id и при изменении вызывает `on_change(new_endpoint_id)`.
pub struct DefaultDeviceWatcher {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl DefaultDeviceWatcher {
    pub fn spawn<F>(poll_interval: Duration, mut on_change: F) -> Self
    where
        F: FnMut(Option<String>) + Send + 'static,
    {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = stop.clone();
        let join = thread::Builder::new()
            .name("zound-default-watcher".into())
            .spawn(move || {
                let mut last = default_render_endpoint_id();
                while !stop_thread.load(Ordering::Relaxed) {
                    thread::sleep(poll_interval);
                    if stop_thread.load(Ordering::Relaxed) {
                        break;
                    }
                    let cur = default_render_endpoint_id();
                    if cur != last {
                        tracing::info!(
                            old = ?last, new = ?cur,
                            "default render endpoint changed"
                        );
                        on_change(cur.clone());
                        last = cur;
                    }
                }
            })
            .ok();
        Self { stop, join }
    }
}

impl Drop for DefaultDeviceWatcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}
