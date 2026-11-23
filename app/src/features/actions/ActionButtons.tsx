import { Lock } from "lucide-react";
import { useId, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { cx } from "../../components/cx";
import { Tooltip } from "../../components/Tooltip";
import { useNoticeStore } from "../../stores/notices";
import { useUiStore } from "../../stores/ui";
import { ACTIONS, actionState, type ActionContext, type ActionDefinition, type ActionHost, type ActionId } from "./registry";
import { ariaShortcut, formatShortcut, type ShortcutPlatform } from "./shortcuts";

interface ActionSurfaceProps {
  ids: readonly ActionId[];
  ctx: ActionContext;
  host: ActionHost;
  platform: ShortcutPlatform;
}

interface Entry {
  definition: ActionDefinition;
  readOnly: boolean;
  label: string;
  /** Tooltip: the label with its shortcut, or why it is disabled. */
  hint: string;
  shortcut: string | undefined;
}

function useEntries({ ids, ctx, platform }: Omit<ActionSurfaceProps, "host">): Entry[] {
  const { t } = useTranslation();
  return ids.flatMap((id) => {
    const definition = ACTIONS[id];
    const state = actionState(definition, ctx);
    if (state.kind === "unavailable") return [];
    const label = t(`actions.labels.${definition.label(ctx)}`);
    const first = definition.shortcuts[platform][0];
    const readOnly = state.kind === "readOnly";
    const hint = readOnly
      ? t("actions.readOnly.tooltip", { label })
      : first
        ? t("actions.withShortcut", { label, shortcut: formatShortcut(first, platform) })
        : label;
    return [{ definition, readOnly, label, hint, shortcut: first && ariaShortcut(first, platform) }];
  });
}

/** Runs an action, or explains why it cannot (the same notice the shortcut shows). */
function useRun(ctx: ActionContext, host: ActionHost) {
  const push = useNoticeStore((state) => state.push);
  return (entry: Entry) => {
    if (entry.readOnly) push({ kind: "readOnly", accountId: ctx.account.id });
    else entry.definition.run(ctx, host);
  };
}

/** "Read-only" next to disabled actions, with the way to the switch (Settings, Accounts). */
export function ReadOnlyNote({ ctx, id }: { ctx: ActionContext; id: string }) {
  const { t } = useTranslation();
  const showAccountsPage = useUiStore((state) => state.showAccountsPage);
  return (
    <div className="flex min-w-0 items-center gap-2 text-12 text-secondary" data-testid="read-only-actions">
      <Lock aria-hidden className="size-(--ui-icon-small) shrink-0" strokeWidth={1.75} />
      <span id={id} className="truncate" title={t("actions.readOnly.explanation")}>
        <span aria-hidden>{t("actions.readOnly.label")}</span>
        <span className="sr-only">{t("actions.readOnly.explanation")}</span>
      </span>
      <button
        type="button"
        onClick={() => {
          showAccountsPage({ kind: "account", accountId: ctx.account.id });
        }}
        className="shrink-0 rounded-control px-1 font-medium text-accent hover:bg-hover"
        data-testid="allow-changes"
      >
        {t("actions.readOnly.open")}
      </button>
    </div>
  );
}

/**
 * The actions as a toolbar (reading pane, selection bar): one tab stop, arrow keys move between
 * the buttons (WAI-ARIA toolbar). Read-only accounts see the actions disabled, each saying why,
 * and a note with the way to allow changes.
 */
export function ActionToolbar({ label, ...props }: ActionSurfaceProps & { label: string }) {
  const entries = useEntries(props);
  const run = useRun(props.ctx, props.host);
  const [active, setActive] = useState(0);
  const noteId = useId();
  const readOnly = entries.some((entry) => entry.readOnly);
  const current = Math.min(active, Math.max(0, entries.length - 1));

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const step = { ArrowRight: 1, ArrowLeft: -1 }[event.key];
    const jump = { Home: 0, End: entries.length - 1 }[event.key];
    if (step === undefined && jump === undefined) return;
    event.preventDefault();
    const next = jump ?? (current + (step ?? 0) + entries.length) % entries.length;
    setActive(next);
    const buttons = event.currentTarget.querySelectorAll<HTMLButtonElement>("button");
    buttons[next]?.focus();
  };

  if (entries.length === 0) return null;
  return (
    <div className="flex min-w-0 items-center gap-3">
      <div
        role="toolbar"
        aria-label={label}
        aria-describedby={readOnly ? noteId : undefined}
        onKeyDown={onKeyDown}
        className="flex shrink-0 items-center gap-1"
        data-testid="action-toolbar"
      >
        {entries.map((entry, index) => {
          const Icon = entry.definition.icon(props.ctx);
          return (
            <Tooltip key={entry.definition.id} label={entry.hint}>
              <button
                type="button"
                tabIndex={index === current ? 0 : -1}
                aria-label={entry.label}
                aria-disabled={entry.readOnly || undefined}
                aria-keyshortcuts={entry.readOnly ? undefined : entry.shortcut}
                onFocus={() => {
                  setActive(index);
                }}
                onClick={() => {
                  run(entry);
                }}
                data-testid={`action-${entry.definition.id}`}
                className={cx(
                  "flex size-8 items-center justify-center rounded-control text-secondary transition-colors",
                  entry.readOnly ? "cursor-default opacity-50" : "hover:bg-hover hover:text-primary",
                )}
              >
                <Icon aria-hidden className="size-(--ui-icon)" strokeWidth={1.75} />
              </button>
            </Tooltip>
          );
        })}
      </div>
      {readOnly && <ReadOnlyNote ctx={props.ctx} id={noteId} />}
    </div>
  );
}

/**
 * Quick actions of one row (docs/DESIGN.md): mouse shortcuts on hover. Not focusable and hidden
 * from screen readers (the row is an `option`, which must not contain controls); the keyboard has
 * the same actions as shortcuts and in the toolbar. Native `title` tooltips: dozens of rows are
 * on screen at once.
 */
export function QuickActions(props: ActionSurfaceProps) {
  const entries = useEntries(props);
  const run = useRun(props.ctx, props.host);
  return entries.map((entry) => {
    const Icon = entry.definition.icon(props.ctx);
    return (
      <button
        key={entry.definition.id}
        type="button"
        tabIndex={-1}
        title={entry.hint}
        aria-label={entry.label}
        aria-disabled={entry.readOnly || undefined}
        // Neither focus nor selection change: the action is for this row, wherever the cursor is.
        onMouseDown={(event) => {
          event.preventDefault();
          event.stopPropagation();
        }}
        onClick={(event) => {
          event.stopPropagation();
          run(entry);
        }}
        onDoubleClick={(event) => {
          event.stopPropagation();
        }}
        data-testid={`quick-${entry.definition.id}`}
        className={cx(
          "flex size-8 items-center justify-center rounded-control text-secondary",
          entry.readOnly ? "cursor-default opacity-50" : "hover:bg-surface-raised hover:text-primary",
        )}
      >
        <Icon aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
      </button>
    );
  });
}
