import { Check } from "lucide-react";
import { RadioGroup } from "radix-ui";
import { useId } from "react";
import { useTranslation } from "react-i18next";
import { ErrorNotice } from "../../components/ErrorNotice";
import { ACCENT_THEMES, DEFAULT_ACCENT, useAccentStore } from "../preferences/accent";
import { cx } from "../../components/cx";
import { SETTING_ANCHOR_CLASS, settingAnchor } from "../../components/settingAnchor";

/**
 * Stage 16: the accent theme (Settings → Appearance), a UI preference of the core synced between
 * devices (decision 0036). One radio per theme: a swatch drawn in that
 * theme's own accent (its `data-accent` scopes the tokens to the swatch, in the current light or
 * dark mode) and always its name, so the colour is never the only cue. Arrow keys move between
 * themes, Space picks one, like every choice group of the settings.
 */
export function AccentPicker() {
  const { t } = useTranslation();
  const labelId = useId();
  const descriptionId = useId();
  const accent = useAccentStore((state) => state.accent);
  const saveError = useAccentStore((state) => state.saveError);
  const setAccent = useAccentStore((state) => state.setAccent);
  return (
    // Stage 22 (decision 0073): found by the settings search by its label.
    <div {...settingAnchor(t("themes.label"))} className={cx("flex flex-col gap-2", SETTING_ANCHOR_CLASS)}>
      <div className="flex flex-col gap-1">
        <span id={labelId} className="text-14 font-medium text-primary">
          {t("themes.label")}
        </span>
        <span id={descriptionId} className="text-13 text-secondary">
          {t("themes.description")}
        </span>
      </div>
      <RadioGroup.Root
        value={accent}
        onValueChange={(next) => {
          const theme = ACCENT_THEMES.find((value) => value === next);
          if (theme) setAccent(theme);
        }}
        aria-labelledby={labelId}
        aria-describedby={descriptionId}
        className="grid grid-cols-[repeat(auto-fill,minmax(10rem,1fr))] gap-2"
        data-testid="accent-picker"
      >
        {ACCENT_THEMES.map((theme) => (
          <RadioGroup.Item
            key={theme}
            value={theme}
            data-testid={`accent-${theme}`}
            className="flex h-10 items-center gap-3 rounded-card border border-border bg-surface px-3 text-start text-13 font-medium text-primary transition-colors hover:bg-hover data-[state=checked]:border-accent data-[state=checked]:bg-accent-soft"
          >
            <span
              aria-hidden
              data-accent={theme}
              className="flex size-5 shrink-0 items-center justify-center rounded-full bg-accent-fill text-on-accent ring-1 ring-accent"
            >
              {theme === accent && <Check aria-hidden className="size-3" strokeWidth={2.5} />}
            </span>
            <span className="truncate">
              {theme === DEFAULT_ACCENT ? t("themes.withDefault", { name: t(`themes.names.${theme}`) }) : t(`themes.names.${theme}`)}
            </span>
          </RadioGroup.Item>
        ))}
      </RadioGroup.Root>
      <p className="text-12 text-tertiary" data-testid="accent-device-note">
        {t("themes.deviceOnly")}
      </p>
      {saveError && <ErrorNotice context={t("themes.notStored")} error={saveError} data-testid="accent-not-stored" />}
    </div>
  );
}
