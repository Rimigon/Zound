// Сессионный профиль — backend-side (session.json в app data dir).
// localStorage используется только как legacy-fallback при первом
// запуске после миграции.

import { state, KEYS, INTERVALS } from "./state.js";
import { invoke } from "./ipc.js";
import { t, tFmt } from "./i18n.js";
import { setStatus } from "./status.js";
import { refreshTargetLatency } from "./sync.js";

let persistTimer = null;

/// Собрать `Vec<DevicePreset>` для save_session_profile. Поля идут в
/// camelCase, потому что `DevicePreset` сериализуется
/// `serde(rename_all = "camelCase")`.
function buildProfile() {
  return {
    devices: state.active.map((a) => {
      const eq = state.eq.get(a.id) || { low: 0, mid: 0, high: 0 };
      return {
        name: a.name,
        endpointId: a.endpointId ?? null,
        volume: a.volume,
        muted: !!a.muted,
        balance: a.balance ?? 0,
        latencyMs: a.latencyMs,
        eqLowDb: eq.low,
        eqMidDb: eq.mid,
        eqHighDb: eq.high,
      };
    }),
    masterGain: state.master.gain,
    masterMuted: state.master.muted,
  };
}

export function persistSession() {
  if (state.suspendPersist) return;
  if (persistTimer) clearTimeout(persistTimer);
  persistTimer = setTimeout(async () => {
    persistTimer = null;
    const profile = buildProfile();
    try {
      await invoke("save_session_profile", profile);
    } catch (e) {
      console.warn("session save failed", e);
    }
  }, INTERVALS.profilePersistDebounceMs);
}

async function loadSession() {
  // Backend-профайл (новый путь).
  try {
    const profile = await invoke("load_session_profile");
    if (profile && Array.isArray(profile.devices)) {
      state.master.gain = profile.masterGain ?? 1.0;
      state.master.muted = !!profile.masterMuted;
      try {
        await invoke("set_master_gain", { gain: state.master.gain });
      } catch (_) {}
      try {
        await invoke("set_master_muted", { muted: state.master.muted });
      } catch (_) {}
      return profile.devices.filter((d) => d && typeof d.name === "string");
    }
  } catch (e) {
    console.warn("load_session_profile failed", e);
  }
  // Legacy localStorage fallback — мигрируем и удаляем.
  try {
    const raw = localStorage.getItem(KEYS.legacySession);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    localStorage.removeItem(KEYS.legacySession);
    if (!Array.isArray(parsed)) return [];
    return parsed.filter((d) => d && typeof d.name === "string");
  } catch (_) {
    localStorage.removeItem(KEYS.legacySession);
    return [];
  }
}

export async function restoreSession(renderDevices, renderActives) {
  const saved = await loadSession();
  if (saved.length === 0) return;
  state.suspendPersist = true;
  const failures = [];
  for (const item of saved) {
    try {
      // P1.1: при добавлении передаём оба — name и endpointId. Backend
      // сам решит, по чему матчить (id приоритетнее).
      const res = await invoke("add_output", {
        deviceName: item.name,
        endpointId: item.endpointId ?? null,
      });
      const id = res.id;
      const devInfo = state.devices.find((d) => d.name === item.name);
      const channels = devInfo ? devInfo.channels : 2;
      const newActive = {
        id,
        name: item.name,
        endpointId: res.endpointId ?? item.endpointId ?? null,
        volume: item.volume ?? 1.0,
        latencyMs: item.latencyMs ?? 20,
        balance: item.balance ?? 0,
        muted: !!item.muted,
        channels,
      };
      state.active.push(newActive);
      // EQ из профиля → state.eq + push в backend.
      const eq = {
        low: item.eqLowDb ?? 0,
        mid: item.eqMidDb ?? 0,
        high: item.eqHighDb ?? 0,
      };
      state.eq.set(id, eq);
      try {
        invoke("set_output_volume", { id, volume: newActive.volume });
      } catch (_) {}
      try {
        await invoke("set_output_latency", {
          id,
          latencyMs: newActive.latencyMs,
        });
      } catch (_) {}
      if (channels === 2) {
        invoke("set_output_balance", { id, balance: newActive.balance });
      }
      if (newActive.muted) {
        invoke("set_output_muted", { id, muted: true });
      }
      if (eq.low || eq.mid || eq.high) {
        invoke("set_output_eq", {
          id,
          lowDb: eq.low,
          midDb: eq.mid,
          highDb: eq.high,
        });
      }
    } catch (_) {
      failures.push(item.name);
    }
  }
  state.suspendPersist = false;
  renderDevices();
  renderActives();
  refreshTargetLatency();

  if (failures.length === 1) {
    setStatus(tFmt("status-restore-failed-one", { name: failures[0] }), "warn");
  } else if (failures.length >= 2) {
    setStatus(
      tFmt("status-restore-partial", { count: failures.length }),
      "warn",
    );
  } else if (saved.length > 0) {
    setStatus(t("status-restore-ok"), "ok");
  }
  persistSession();
}
