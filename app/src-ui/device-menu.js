// ПКМ-меню на строке устройства: переименование / сброс. Вынесено в
// отдельный модуль, чтобы devices.js и mixer.js (где меню тоже
// открывается) не зависели друг от друга через циклический import.

import { state } from "./state.js";
import { t } from "./i18n.js";
import { setStatus } from "./status.js";
import { openContextMenu } from "./context-menu.js";
import { openRenamePrompt } from "./rename-prompt.js";
import { setAlias, clearAlias, hasAlias } from "./aliases.js";

/// Перерисовка после изменения алиаса. Импортируется лениво (динамически),
/// чтобы не тянуть mixer/devices при загрузке модуля и снова не словить
/// цикл.
async function rerenderAfterAliasChange() {
  const [{ renderDevices }, { renderActives }, { refreshDefaultSourceWarning }] =
    await Promise.all([
      import("./devices.js"),
      import("./mixer.js"),
      import("./sync.js"),
    ]);
  renderDevices();
  renderActives();
  // Warning у источника захвата теперь показывает алиас в скобках.
  refreshDefaultSourceWarning();
}

export function openDeviceContextMenu(x, y, d) {
  if (!d || !d.endpointId) return;
  openContextMenu(x, y, [
    {
      label: t("device-rename"),
      onClick: async () => {
        const next = await openRenamePrompt({
          initialValue: state.aliases.get(d.endpointId) ?? "",
          systemName: d.name,
        });
        if (next === null) return;
        try {
          await setAlias(d.endpointId, next);
          setStatus(
            next.trim().length > 0
              ? t("device-rename-saved")
              : t("device-rename-cleared"),
            "ok",
          );
          await rerenderAfterAliasChange();
        } catch (e) {
          setStatus(String(e?.message ?? e), "err");
        }
      },
    },
    {
      label: t("device-rename-reset"),
      hidden: !hasAlias(d.endpointId),
      onClick: async () => {
        try {
          await clearAlias(d.endpointId);
          setStatus(t("device-rename-cleared"), "ok");
          await rerenderAfterAliasChange();
        } catch (e) {
          setStatus(String(e?.message ?? e), "err");
        }
      },
    },
  ]);
}
