import { Lock } from "lucide-react";
import { Fragment, useId, useLayoutEffect, useRef, useState, type KeyboardEvent, type RefObject } from "react";
import { useTranslation } from "react-i18next";
import { cx } from "../../components/cx";
import { Tooltip } from "../../components/Tooltip";
import { useNoticeStore } from "../../stores/notices";
import { useBoundShortcuts } from "../../stores/shortcuts";
import { useUiStore } from "../../stores/ui";
import { OverflowMenu, type OverflowItem } from "./OverflowMenu";
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
  /** The first shortcut as the platform writes it ("⌘P", "E"), for menus. */
  keys: string | undefined;
}

function useEntries({ ids, ctx, platform }: Omit<ActionSurfaceProps, "host">): Entry[] {
  const { t } = useTranslation();
  const shortcutsOf = useBoundShortcuts(platform);
  return ids.flatMap((id) => {
    const definition = ACTIONS[id];
    const state = actionState(definition, ctx);
    if (state.kind === "unavailable") return [];
    const label = t(`actions.labels.${definition.label(ctx)}`);
    const first = shortcutsOf(id)[0];
    const readOnly = state.kind === "readOnly";
    const hint = readOnly
      ? t("actions.readOnly.tooltip", { label })
      : first
        ? t("actions.withShortcut", { label, shortcut: formatShortcut(first, platform) })
        : label;
    return [{ definition, readOnly, label, hint, shortcut: first && ariaShortcut(first, platform), keys: first && formatShortcut(first, platform) }];
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
 * The toolbar's own buttons, in groups with a thin line between them: respond, file away, mark.
 * At most seven show at once (the actions of a folder exclude each other within a group: Delete
 * or Delete permanently, Reply or Edit draft). Every other action is always in "More actions".
 */
export const PRIMARY_GROUPS: readonly (readonly ActionId[])[] = [
  ["reply", "replyAll", "forward", "editDraft"],
  ["archive", "delete", "deletePermanently", "notSpam"],
  ["toggleRead", "toggleFlag"],
];

function groupOf(id: ActionId): number {
  return PRIMARY_GROUPS.findIndex((group) => group.includes(id));
}

/** When even the primary buttons do not fit (a narrow pane, large text), these stay longest. */
const NARROW_PRIORITY: readonly ActionId[] = [
  "archive",
  "delete",
  "deletePermanently",
  "notSpam",
  "reply",
  "editDraft",
  "toggleRead",
  "replyAll",
  "forward",
  "toggleFlag",
];

function rank(id: ActionId): number {
  const index = NARROW_PRIORITY.indexOf(id);
  return index === -1 ? NARROW_PRIORITY.length : index;
}

/** The primary entries the toolbar shows itself; `count` limits them in a narrow pane. */
export function visibleEntries<T extends { definition: { id: ActionId } }>(entries: readonly T[], count: number | null = null): T[] {
  const primary = entries.filter((entry) => groupOf(entry.definition.id) !== -1);
  if (count === null || count >= primary.length) return primary;
  const keep = new Set(
    [...primary]
      .sort((a, b) => rank(a.definition.id) - rank(b.definition.id))
      .slice(0, Math.max(0, count))
      .map((entry) => entry.definition.id),
  );
  return primary.filter((entry) => keep.has(entry.definition.id));
}

/**
 * How many buttons fit into `available` pixels: null when all `total` do (or nothing could be
 * measured), else how many fit next to the "More" button (0 when not even that has room). Buttons
 * are all the same size; their gap comes from the toolbar.
 */
export function fittingCount(available: number, button: number, gap: number, total: number): number | null {
  if (button <= 0) return null;
  if (available <= 0) return 0;
  if (total * button + Math.max(0, total - 1) * gap <= available) return null;
  return Math.max(0, Math.floor((available - button) / (button + gap)));
}

/** What the read-only note keeps: lock and "Allow changes" always, its label up to 40 % of the room. */
function noteRoom(note: HTMLElement, room: number): number {
  const inner = note.firstElementChild;
  const innerGap = inner ? Number.parseFloat(getComputedStyle(inner).columnGap) || 0 : 0;
  const lock = note.querySelector("svg")?.getBoundingClientRect().width ?? 0;
  const allow = note.querySelector("button")?.getBoundingClientRect().width ?? 0;
  return Math.max(lock + allow + 2 * innerGap, Math.min(note.scrollWidth, room * 0.4));
}

/**
 * Measures the room of the toolbar's wrapper and answers how many actions fit (null: all). Without
 * layout (no ResizeObserver, or nothing measured yet) every action shows.
 */
function useFitting(
  wrapper: RefObject<HTMLDivElement | null>,
  toolbar: RefObject<HTMLDivElement | null>,
  note: RefObject<HTMLDivElement | null>,
  total: number,
  readOnly: boolean,
): number | null {
  const [fit, setFit] = useState<number | null>(null);
  useLayoutEffect(() => {
    const element = wrapper.current;
    if (!element || typeof ResizeObserver === "undefined") return undefined;
    const measure = () => {
      const bar = toolbar.current;
      const sample = bar?.querySelector<HTMLElement>("button");
      if (!bar || !sample) return;
      // No layout (jsdom, a hidden window): nothing is measured, every primary action shows.
      if (element.clientWidth === 0) {
        setFit(null);
        return;
      }
      const style = getComputedStyle(bar);
      const gap = Number.parseFloat(style.columnGap) || 0;
      const outerGap = Number.parseFloat(getComputedStyle(element).columnGap) || 0;
      // The read-only note gives way first: its label truncates (noteRoom).
      const noteWidth = note.current ? noteRoom(note.current, element.clientWidth) + outerGap : 0;
      // The lines between the groups and before "More" take room too.
      const line = bar.querySelector<HTMLElement>("[data-toolbar-separator]");
      const lineStyle = line ? getComputedStyle(line) : null;
      const lineWidth = line && lineStyle ? line.getBoundingClientRect().width + (Number.parseFloat(lineStyle.marginLeft) || 0) + (Number.parseFloat(lineStyle.marginRight) || 0) + gap : 0;
      const room = element.clientWidth - noteWidth - PRIMARY_GROUPS.length * lineWidth;
      const next = fittingCount(room, sample.getBoundingClientRect().width, gap, total + 1);
      setFit((current) => (current === next ? current : next));
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    // The read-only note can appear or change without the wrapper changing its size.
    if (note.current) observer.observe(note.current);
    return () => {
      observer.disconnect();
    };
  }, [wrapper, toolbar, note, total, readOnly]);
  return fit;
}

/**
 * The actions as a toolbar (reading pane, selection bar): one tab stop, arrow keys move between
 * the buttons (WAI-ARIA toolbar). Read-only accounts see the actions disabled, each saying why,
 * and a note with the way to allow changes. The toolbar shows the primary groups (respond, file
 * away, mark: at most seven buttons); everything else is in the "More actions" menu at its end,
 * which is part of the arrow-key order. In a narrow pane primary buttons move there too. Every
 * action stays reachable by its shortcut and in the command palette. The buttons of the menu stay
 * in the document (hidden), so the toolbar's set of actions is the same at every width.
 */
/** The thin line between two groups of the toolbar (decorative). */
function Separator() {
  return <span aria-hidden data-toolbar-separator className="mx-1 h-4 w-px shrink-0 bg-border" />;
}

export function ActionToolbar({ label, ...props }: ActionSurfaceProps & { label: string }) {
  const { t } = useTranslation();
  const entries = useEntries(props);
  const run = useRun(props.ctx, props.host);
  const [active, setActive] = useState(0);
  const noteId = useId();
  const wrapper = useRef<HTMLDivElement>(null);
  const toolbar = useRef<HTMLDivElement>(null);
  const note = useRef<HTMLDivElement>(null);
  const readOnly = entries.some((entry) => entry.readOnly);
  const primaryCount = visibleEntries(entries).length;
  const fit = useFitting(wrapper, toolbar, note, primaryCount, readOnly);
  const shown = visibleEntries(entries, fit);
  const shownIds = new Set(shown.map((entry) => entry.definition.id));
  const overflow = entries.filter((entry) => !shownIds.has(entry.definition.id));
  // The focusable stops in order: the shown buttons, then "More".
  const stops = shown.length + (overflow.length > 0 ? 1 : 0);
  const current = Math.min(active, Math.max(0, stops - 1));
  const menuItems: OverflowItem[] = overflow.map((entry) => ({
    id: entry.definition.id,
    label: entry.label,
    icon: entry.definition.icon(props.ctx),
    ...(entry.keys !== undefined && !entry.readOnly && { detail: entry.keys }),
    ...(entry.shortcut !== undefined && { keyshortcuts: entry.shortcut }),
    inert: entry.readOnly,
    onSelect: () => {
      run(entry);
    },
    "data-testid": `action-menu-${entry.definition.id}`,
  }));

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const step = { ArrowRight: 1, ArrowLeft: -1 }[event.key];
    const jump = { Home: 0, End: stops - 1 }[event.key];
    if (step === undefined && jump === undefined) return;
    event.preventDefault();
    const next = jump ?? (current + (step ?? 0) + stops) % stops;
    setActive(next);
    const buttons = [...event.currentTarget.querySelectorAll<HTMLButtonElement>("button")].filter((button) => !button.closest("[hidden]"));
    buttons[next]?.focus();
  };

  if (entries.length === 0) return null;
  return (
    <div ref={wrapper} className="flex min-w-0 flex-1 items-center gap-3">
      <div
        ref={toolbar}
        role="toolbar"
        aria-label={label}
        aria-describedby={readOnly ? noteId : undefined}
        onKeyDown={onKeyDown}
        className="flex shrink-0 items-center gap-1"
      >
        <div className="contents" data-testid="action-toolbar">
          {entries.map((entry) => {
            const Icon = entry.definition.icon(props.ctx);
            const index = shown.indexOf(entry);
            const hidden = index === -1;
            const previous = index > 0 ? shown[index - 1] : undefined;
            const newGroup = previous !== undefined && groupOf(previous.definition.id) !== groupOf(entry.definition.id);
            return (
              <Fragment key={entry.definition.id}>
                {newGroup && <Separator />}
                <Tooltip label={entry.hint} disabled={hidden}>
                  <button
                    type="button"
                    hidden={hidden}
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
              </Fragment>
            );
          })}
        </div>
        {overflow.length > 0 && shown.length > 0 && <Separator />}
        {overflow.length > 0 && (
          <OverflowMenu
            label={t("actions.more")}
            tabIndex={current === shown.length ? 0 : -1}
            onFocus={() => {
              setActive(shown.length);
            }}
            items={menuItems}
            data-testid="action-more"
          />
        )}
      </div>
      {readOnly && (
        <div ref={note} className="flex min-w-0">
          <ReadOnlyNote ctx={props.ctx} id={noteId} />
        </div>
      )}
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
    // Stage 8 (axe "nested-interactive"): an option of the list may not contain controls, so
    // these are mouse targets, not buttons. Keyboard and screen reader users reach the same
    // actions through the shortcuts, the toolbar of the reading pane and the command palette.
    return (
      <span
        key={entry.definition.id}
        title={entry.hint}
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
      </span>
    );
  });
}
