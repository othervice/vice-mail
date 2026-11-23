import { Inbox, Layers } from "lucide-react";
import { useTranslation } from "react-i18next";
import { cx } from "../../components/cx";
import type { UnifiedView } from "../../services/ipc";
import { useUiStore } from "../../stores/ui";
import { useAccountGroups } from "./queries";

function same(a: UnifiedView | null, b: UnifiedView): boolean {
  if (!a || a.kind !== b.kind) return false;
  return a.kind === "all" || (b.kind === "group" && a.groupId === b.groupId);
}

/**
 * Top of the unfolded sidebar (stage 7): the unified inbox and one entry per account group,
 * above the folder trees of the accounts. Shown with more than one account or with groups.
 */
export function UnifiedTree({ accountCount }: { accountCount: number }) {
  const { t } = useTranslation();
  const view = useUiStore((state) => state.view);
  const unified = useUiStore((state) => state.unifiedInbox);
  const showUnifiedInbox = useUiStore((state) => state.showUnifiedInbox);
  const groups = useAccountGroups().data ?? [];
  if (accountCount < 2 && groups.length === 0) return null;
  const inInbox = view.kind === "mail" && view.mailView === "inbox";
  const entries: { view: UnifiedView; name: string; key: string }[] = [
    { view: { kind: "all" }, name: t("unified.title"), key: "all" },
    ...groups.map((group) => ({ view: { kind: "group", groupId: group.id } as const, name: group.name, key: group.id })),
  ];
  return (
    <section aria-label={t("unified.sidebar")} className="flex flex-col gap-1" data-testid="unified-tree">
      <h2 className="flex h-8 items-center px-2 text-12 font-semibold text-secondary">{t("unified.sidebar")}</h2>
      <ul className="flex flex-col">
        {entries.map((entry) => {
          const current = inInbox && same(unified, entry.view);
          const Icon = entry.view.kind === "all" ? Inbox : Layers;
          return (
            <li key={entry.key}>
              <button
                type="button"
                aria-current={current ? "page" : undefined}
                onClick={() => {
                  showUnifiedInbox(entry.view);
                }}
                className={cx(
                  "flex h-8 w-full min-w-0 items-center gap-2 rounded-control px-2 text-13 transition-colors",
                  current ? "bg-selected font-medium text-primary" : "text-secondary hover:bg-hover hover:text-primary",
                )}
                data-testid={`unified-${entry.key}`}
              >
                <Icon aria-hidden className="size-(--ui-icon-small) shrink-0" strokeWidth={1.75} />
                <span className="truncate">{entry.name}</span>
              </button>
            </li>
          );
        })}
      </ul>
    </section>
  );
}
