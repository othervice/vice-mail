import { Tabs } from "radix-ui";
import type { ComponentType } from "react";
import { useTranslation } from "react-i18next";
import { ViewHeader } from "../../components/ViewHeader";
import { SETTINGS_SECTION_IDS, useUiStore, type SettingsSectionId } from "../../stores/ui";
import { useWindowDragRegion } from "../app-info/useWindowDragRegion";
import { InboxSettings } from "../comfort/InboxSettings";
import { KeyboardSettings } from "../keyboard/KeyboardSettings";
import { ComposeSettings } from "../composer/ComposeSettings";
import { ContactsSettings } from "../contacts/ContactsSettings";
import { NotificationSettings } from "../comfort/NotificationSettings";
import { AiSettings } from "../ai/AiSettings";
import { McpSettings } from "../mcp/McpSettings";
import { SettingsSyncSettings } from "../settings-sync/SettingsSyncSettings";
import { AboutSettings } from "./AboutSettings";
import { AccountsSettings } from "./AccountsSettings";
import { AppearanceSettings } from "./AppearanceSettings";
import { GeneralSettings } from "./GeneralSettings";
import { ReadingSettings } from "./ReadingSettings";

const SECTIONS: Record<SettingsSectionId, ComponentType> = {
  general: GeneralSettings,
  accounts: AccountsSettings,
  reading: ReadingSettings,
  compose: ComposeSettings,
  contacts: ContactsSettings,
  inbox: InboxSettings,
  notifications: NotificationSettings,
  appearance: AppearanceSettings,
  keyboard: KeyboardSettings,
  sync: SettingsSyncSettings,
  ai: AiSettings,
  mcp: McpSettings,
  about: AboutSettings,
};

interface SettingsViewProps {
  section: SettingsSectionId;
}

/** Settings are a view of their own next to the rail, not a dialog (docs/DESIGN.md). */
export function SettingsView({ section }: SettingsViewProps) {
  const { t } = useTranslation();
  const showSettings = useUiStore((state) => state.showSettings);
  const dragRegion = useWindowDragRegion();

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <ViewHeader title={t("settings.title")} dragRegion={dragRegion} />
      <Tabs.Root
        value={section}
        onValueChange={(next) => {
          const id = SETTINGS_SECTION_IDS.find((candidate) => candidate === next);
          if (id) showSettings(id);
        }}
        orientation="vertical"
        className="flex min-h-0 flex-1"
      >
        <Tabs.List
          aria-label={t("settings.sections")}
          className="flex w-(--ui-settings-nav-width) shrink-0 flex-col gap-1 border-r border-border px-3 py-4"
        >
          {SETTINGS_SECTION_IDS.map((id) => (
            <Tabs.Trigger
              key={id}
              value={id}
              data-testid={`settings-tab-${id}`}
              className="relative flex h-8 items-center rounded-control px-3 text-left text-14 text-secondary transition-colors hover:bg-hover hover:text-primary data-[state=active]:bg-selected data-[state=active]:font-medium data-[state=active]:text-primary data-[state=active]:before:absolute data-[state=active]:before:inset-y-2 data-[state=active]:before:left-0 data-[state=active]:before:w-1 data-[state=active]:before:rounded-control data-[state=active]:before:bg-accent"
            >
              {t(`settings.${id}.title`)}
            </Tabs.Trigger>
          ))}
        </Tabs.List>
        {SETTINGS_SECTION_IDS.map((id) => {
          const Section = SECTIONS[id];
          return (
            <Tabs.Content key={id} value={id} className="min-w-0 flex-1 overflow-y-auto px-8 py-6">
              <Section />
            </Tabs.Content>
          );
        })}
      </Tabs.Root>
    </div>
  );
}
