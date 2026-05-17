// Алиасы устройств — display-only. Системное имя WASAPI/CoreAudio/
// PipeWire не меняется, алиас живёт в session.json под endpoint_id и
// рендерится поверх системного имени в списке устройств и в миксере.

import { state } from "./state.js";
import { invoke } from "./ipc.js";

/// Отображаемое имя устройства. Принимает объект с полями `endpointId` и
/// `name` — подходит и для `DeviceDto`, и для элемента `state.active`.
export function displayName(d) {
  if (!d) return "";
  const key = d.endpointId;
  if (key && state.aliases.has(key)) {
    return state.aliases.get(key);
  }
  return d.name;
}

/// Есть ли у устройства пользовательское имя. Нужно UI, чтобы решить,
/// показывать ли пункт «Сбросить имя» в контекстном меню.
export function hasAlias(endpointId) {
  return !!endpointId && state.aliases.has(endpointId);
}

export async function loadAliases() {
  try {
    const map = await invoke("list_device_aliases");
    state.aliases.clear();
    for (const [k, v] of Object.entries(map || {})) {
      if (typeof v === "string" && v.length > 0) state.aliases.set(k, v);
    }
  } catch (e) {
    console.warn("list_device_aliases failed", e);
  }
}

export async function setAlias(endpointId, alias) {
  const trimmed = (alias ?? "").trim();
  await invoke("set_device_alias", {
    endpointId,
    alias: trimmed.length > 0 ? trimmed : null,
  });
  if (trimmed.length > 0) state.aliases.set(endpointId, trimmed);
  else state.aliases.delete(endpointId);
}

export async function clearAlias(endpointId) {
  await invoke("set_device_alias", { endpointId, alias: null });
  state.aliases.delete(endpointId);
}
