import {
  AtSign,
  Bell,
  BookOpen,
  BookUser,
  Cable,
  Inbox,
  Info,
  Keyboard,
  Palette,
  PenLine,
  RefreshCw,
  Settings2,
  Sparkles,
  type LucideIcon,
} from "lucide-react";
import { Tabs } from "radix-ui";
import { useRef, type ComponentType } from "react";
import { useTranslation } from "react-i18next";
import { ViewHeader } from "../../components/ViewHeader";
import { SETTINGS_SECTION_IDS, useUiStore, type SettingsSectionId } from "../../stores/ui";
import { shortcutPlatform } from "../actions/shortcuts";
import { useAppInfo } from "../app-info/queries";
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
import { SettingsSearch, type SettingsSearchHandle } from "./search/SettingsSearch";
import { useSettingsJump, useSettingsSearchShortcut } from "./search/jump";

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

// Design pass 23.11.2026: every section has a small symbol in the list (decorative; the name says it).
const ICONS: Record<SettingsSectionId, LucideIcon> = {
  general: Settings2,
  accounts: AtSign,
  reading: BookOpen,
  compose: PenLine,
  contacts: BookUser,
  inbox: Inbox,
  notifications: Bell,
  appearance: Palette,
  keyboard: Keyboard,
  sync: RefreshCw,
  ai: Sparkles,
  mcp: Cable,
  about: Info,
};

interface SettingsViewProps {
  section: SettingsSectionId;
}

/**
 * Settings are a view of their own next to the rail, not a dialog (docs/DESIGN.md). Stage 22
 * (decision 0073): a search field at the top of the section list finds every option; Cmd/Ctrl+F
 * (the search shortcut) puts the focus into it.
 */
export function SettingsView({ section }: SettingsViewProps) {
  const { t } = useTranslation();
  const showSettings = useUiStore((state) => state.showSettings);
  const dragRegion = useWindowDragRegion();
  const search = useRef<SettingsSearchHandle>(null);
  const panels = useRef<HTMLDivElement>(null);
  const jump = useSettingsJump(panels, section);
  useSettingsSearchShortcut(shortcutPlatform(useAppInfo().data?.platform), () => {
    search.current?.focus();
  });

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
        {/* Design pass 23.11.2026: the section list on the list column's tint, like the mail list.
            Stage 22: the search field above it. */}
        <div className="flex w-(--ui-settings-nav-width) shrink-0 flex-col border-r border-border-subtle bg-surface-list">
          <div className="shrink-0 px-3 pt-4 pb-2">
            <SettingsSearch ref={search} onChoose={jump} />
          </div>
          <Tabs.List
            aria-label={t("settings.sections")}
            className="flex min-h-0 flex-1 flex-col gap-1 overflow-y-auto px-3 pt-2 pb-4"
          >
            {SETTINGS_SECTION_IDS.map((id) => {
              const Icon = ICONS[id];
              return (
                <Tabs.Trigger
                  key={id}
                  value={id}
                  data-testid={`settings-tab-${id}`}
                  className="group relative flex h-8 shrink-0 items-center gap-2 rounded-control px-3 text-left text-14 text-secondary transition-colors hover:bg-hover hover:text-primary data-[state=active]:bg-selected data-[state=active]:font-medium data-[state=active]:text-primary data-[state=active]:before:absolute data-[state=active]:before:inset-y-2 data-[state=active]:before:left-0 data-[state=active]:before:w-1 data-[state=active]:before:rounded-control data-[state=active]:before:bg-accent-fill"
                >
                  <Icon
                    aria-hidden
                    className="size-(--ui-icon-small) shrink-0 text-tertiary group-hover:text-secondary group-data-[state=active]:text-accent"
                    strokeWidth={1.75}
                  />
                  <span className="truncate">{t(`settings.${id}.title`)}</span>
                </Tabs.Trigger>
              );
            })}
          </Tabs.List>
        </div>
        <div ref={panels} className="flex min-w-0 flex-1">
          {SETTINGS_SECTION_IDS.map((id) => {
            const Section = SECTIONS[id];
            return (
              <Tabs.Content key={id} value={id} className="min-w-0 flex-1 overflow-y-auto px-10 py-8" data-settings-panel={id}>
                <Section />
              </Tabs.Content>
            );
          })}
        </div>
      </Tabs.Root>
    </div>
  );
}
