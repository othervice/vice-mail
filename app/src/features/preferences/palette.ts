import { Monitor, Moon, Sun, type LucideIcon } from "lucide-react";
import { useTranslation } from "react-i18next";
import { toCoreError, type ThemePreference } from "../../services/ipc";
import { useNoticeStore } from "../../stores/notices";
import type { PaletteEntry } from "../palette/entries";
import { DEFAULT_PREFERENCES, usePreferences, useSetPreferences } from "./queries";

const THEMES: readonly ThemePreference[] = ["light", "dark", "system"];
const THEME_ICONS: Record<ThemePreference, LucideIcon> = { light: Sun, dark: Moon, system: Monitor };

/**
 * Stage 18 (decision 0053): light, dark or as the system, from the command palette, without
 * opening the settings. Saved through the core like the switch in Settings → Appearance and
 * applied at once (optimistic, rolled back if the core refuses). The palette is closed by then,
 * so a refusal is a notice.
 */
export function useThemePaletteEntries(): PaletteEntry[] {
  const { t } = useTranslation();
  const preferences = usePreferences();
  const save = useSetPreferences();
  const push = useNoticeStore((state) => state.push);
  const current = preferences.data ?? DEFAULT_PREFERENCES;
  return THEMES.map((theme) => ({
    key: `layout:theme:${theme}`,
    group: "layout",
    label: t(`settings.appearance.theme.command.${theme}`),
    icon: THEME_ICONS[theme],
    keywords: [t("settings.appearance.title"), t("settings.appearance.theme.label"), t(`settings.appearance.theme.${theme}`)],
    ...(current.theme === theme && { detail: t("palette.current") }),
    run: () => {
      // The promise settles after the palette is gone (a callback of `mutate` would not).
      save.mutateAsync({ ...current, theme }).catch((error: unknown) => {
        push({ kind: "error", context: "appearance", error: toCoreError(error) });
      });
    },
  }));
}
