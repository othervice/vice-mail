import { ArrowDown, ArrowUp, GripVertical, X, type LucideIcon } from "lucide-react";
import { useEffect, useId, useLayoutEffect, useRef, useState, type KeyboardEvent, type PointerEvent as ReactPointerEvent } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { cx } from "../../components/cx";
import { ErrorNotice } from "../../components/ErrorNotice";
import { Tooltip } from "../../components/Tooltip";
import { shortcutPlatform } from "../actions/shortcuts";
import { useAppInfo } from "../app-info/queries";
import { trapTab } from "../keyboard/ShortcutOverview";
import { MAIL_VIEW_ICONS } from "../mail/views";
import { isDefaultColumnViews, MORE_ENTRY, moveColumnView, useColumnViews, useColumnViewsStore, type ColumnEntryId, type ColumnViewChoice } from "./columnViews";

/** Stage 17: "Edit views…", mounted once in the frame; open from the column or the palette. */
export function EditViewsHost() {
  const editing = useColumnViewsStore((state) => state.editing);
  return editing ? <EditViewsDialog /> : null;
}

/** A view being dragged by its handle: the order shown meanwhile and the one it started from. */
interface Drag {
  id: ColumnEntryId;
  order: readonly ColumnViewChoice[];
  start: readonly ColumnViewChoice[];
  stop: () => void;
}

const sameOrder = (a: readonly ColumnViewChoice[], b: readonly ColumnViewChoice[]) => a.every((view, index) => view.id === b[index]?.id);

function MoveButton({ icon: Icon, label, disabled, onClick, ...rest }: { icon: LucideIcon; label: string; disabled: boolean; onClick: () => void; "data-testid": string }) {
  return (
    <Tooltip label={label} disabled={disabled}>
      <button
        type="button"
        aria-label={label}
        disabled={disabled}
        // For the mouse; with the keyboard, Alt+Up and Alt+Down on the view do the same.
        tabIndex={-1}
        onClick={onClick}
        data-testid={rest["data-testid"]}
        className="flex size-8 shrink-0 items-center justify-center rounded-control text-secondary transition-colors hover:bg-hover hover:text-primary disabled:cursor-default disabled:opacity-40 disabled:hover:bg-transparent disabled:hover:text-secondary"
      >
        <Icon aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
      </button>
    </Tooltip>
  );
}

/**
 * Which views the column and the rail show, and in which order (stage 17). Every change applies at
 * once, so the column and the rail behind the dialog are its preview; a dragged view moves there
 * while it is dragged and is saved when it is dropped. Keyboard: Tab goes from view to view, Space
 * shows or hides one, Alt+Up and Alt+Down move it (the focus stays on it), Escape closes.
 */
function EditViewsDialog() {
  const { t } = useTranslation();
  const titleId = useId();
  const introId = useId();
  const hintId = useId();
  const views = useColumnViews();
  const choose = useColumnViewsStore((state) => state.choose);
  const reset = useColumnViewsStore((state) => state.reset);
  const setPreview = useColumnViewsStore((state) => state.setPreview);
  const close = useColumnViewsStore((state) => state.closeEditor);
  const saveError = useColumnViewsStore((state) => state.saveError);
  const platform = shortcutPlatform(useAppInfo().data?.platform);
  const [announcement, setAnnouncement] = useState("");
  const [dragging, setDragging] = useState<ColumnEntryId | null>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const listRef = useRef<HTMLUListElement>(null);
  const doneRef = useRef<HTMLDivElement>(null);
  const drag = useRef<Drag | null>(null);
  // The view whose checkbox keeps the focus while it moves to another place.
  const keepFocus = useRef<ColumnEntryId | null>(null);
  // Stage 23 (decision 0085): the line of "More" is an entry of the list too.
  const name = (id: ColumnEntryId) => (id === MORE_ENTRY ? t("nav.more") : t(`nav.${id}`));

  // The focus starts on the first view and goes back where it was (else to the column's "Views",
  // else to the main area); a click beside the dialog closes it, like Escape.
  useEffect(() => {
    const opener = document.activeElement instanceof HTMLElement && document.activeElement !== document.body ? document.activeElement : null;
    panelRef.current?.querySelector<HTMLElement>('input[type="checkbox"]')?.focus();
    const onPointerDown = (event: PointerEvent) => {
      if (event.target instanceof Node && !panelRef.current?.contains(event.target)) close();
    };
    // Escape closes it also when the focus was lost to the page (a re-render, the window losing
    // and regaining focus): the panel's own handler only sees keys pressed inside it.
    const onDocumentKeyDown = (event: globalThis.KeyboardEvent) => {
      if (event.key !== "Escape" || (event.target instanceof Node && panelRef.current?.contains(event.target))) return;
      event.preventDefault();
      event.stopPropagation();
      close();
    };
    document.addEventListener("pointerdown", onPointerDown, true);
    document.addEventListener("keydown", onDocumentKeyDown, true);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown, true);
      document.removeEventListener("keydown", onDocumentKeyDown, true);
      drag.current?.stop();
      const active = document.activeElement;
      if (active && active !== document.body) return;
      const target =
        (opener?.isConnected ? opener : null) ??
        document.querySelector<HTMLElement>('[data-testid="sidebar-section-views"]') ??
        document.querySelector<HTMLElement>("main");
      target?.focus({ preventScroll: true });
    };
  }, [close]);

  useLayoutEffect(() => {
    const id = keepFocus.current;
    if (id === null) return;
    keepFocus.current = null;
    const box = listRef.current?.querySelector<HTMLElement>(`[data-view-row="${id}"] input`);
    if (box && document.activeElement !== box) box.focus();
  }, [views]);

  // The place among the views of "All accounts" or of "More", and which of the two.
  const announceMove = (order: readonly ColumnViewChoice[], id: ColumnEntryId) => {
    const line = order.findIndex((view) => view.id === MORE_ENTRY);
    const index = order.findIndex((view) => view.id === id);
    const under = line >= 0 && index > line;
    const group = order.filter((view, at) => view.id !== MORE_ENTRY && (line < 0 || (at > line) === under));
    const position = group.findIndex((view) => view.id === id) + 1;
    setAnnouncement(t(under ? "columnViews.movedMore" : "columnViews.moved", { name: name(id), position, count: group.length }));
  };

  const move = (from: number, to: number) => {
    const moved = views[from];
    if (!moved || to < 0 || to >= views.length || from === to) return;
    const next = moveColumnView(views, from, to);
    keepFocus.current = moved.id;
    choose(next);
    announceMove(next, moved.id);
  };

  const toggle = (id: ColumnEntryId, shown: boolean) => {
    choose(views.map((view) => (view.id === id ? { ...view, shown } : view)));
  };

  const onCheckboxKeyDown = (index: number) => (event: KeyboardEvent<HTMLInputElement>) => {
    if (!event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
    if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return;
    event.preventDefault();
    move(index, index + (event.key === "ArrowUp" ? -1 : 1));
  };

  // Dragging by the handle: the row under the pointer (above the first: the first, below the last:
  // the last) is where the view goes. Window listeners, not pointer capture: the row moves in the
  // DOM while it is dragged, which would end a capture.
  const startDrag = (id: ColumnEntryId) => (event: ReactPointerEvent<HTMLElement>) => {
    if (event.button !== 0 || drag.current) return;
    event.preventDefault();
    const onMove = (pointer: PointerEvent) => {
      const state = drag.current;
      if (!state) return;
      const rows = [...(listRef.current?.querySelectorAll<HTMLElement>("[data-view-row]") ?? [])];
      const under = rows.findIndex((row) => pointer.clientY < row.getBoundingClientRect().bottom);
      const to = under < 0 ? rows.length - 1 : under;
      const from = state.order.findIndex((view) => view.id === state.id);
      if (to < 0 || to === from) return;
      state.order = moveColumnView(state.order, from, to);
      setPreview(state.order);
    };
    const end = (commit: boolean) => {
      const state = drag.current;
      if (!state) return;
      state.stop();
      setDragging(null);
      if (commit && !sameOrder(state.order, state.start)) {
        keepFocus.current = state.id;
        choose(state.order);
        announceMove(state.order, state.id);
      } else {
        setPreview(null);
      }
    };
    const onUp = () => {
      end(true);
    };
    const onCancel = () => {
      end(false);
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
    window.addEventListener("pointercancel", onCancel);
    drag.current = {
      id,
      order: views,
      start: views,
      stop: () => {
        window.removeEventListener("pointermove", onMove);
        window.removeEventListener("pointerup", onUp);
        window.removeEventListener("pointercancel", onCancel);
        drag.current = null;
      },
    };
    setDragging(id);
    setPreview(views);
  };

  const keys = platform === "macos" ? { up: "⌥↑", down: "⌥↓" } : { up: "Alt+↑", down: "Alt+↓" };
  const isDefault = isDefaultColumnViews(views);
  const resetToDefault = () => {
    reset();
    setAnnouncement(t("columnViews.resetDone"));
    // The button is disabled now: the focus goes on to "Done".
    doneRef.current?.querySelector<HTMLElement>("button:last-child")?.focus();
  };

  return (
    <>
      {/* The same dimmed backdrop as the palette; the column and the rail show through it. */}
      <div aria-hidden className="fixed inset-0 z-40 bg-scrim" data-testid="edit-views-backdrop" />
      <div
        ref={panelRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={introId}
        // A click on the panel's background keeps the focus inside, so Escape still reaches it.
        tabIndex={-1}
        onKeyDown={(event) => {
          // A modal dialog: no key reaches the shortcuts of the view behind it.
          event.stopPropagation();
          if (event.key === "Escape") {
            event.preventDefault();
            close();
            return;
          }
          trapTab(event);
        }}
        data-testid="edit-views"
        className="fixed top-(--ui-palette-top) left-1/2 z-50 flex max-h-[calc(100%-2*var(--ui-palette-top))] w-(--ui-dialog-width) max-w-[calc(100%-2rem)] -translate-x-1/2 flex-col gap-4 rounded-panel border border-border bg-surface-raised p-6 shadow-panel"
      >
        <div className="flex items-start justify-between gap-2">
          <div className="flex min-w-0 flex-col gap-1">
            <h2 id={titleId} className="text-16 font-semibold text-primary">
              {t("columnViews.title")}
            </h2>
            <p id={introId} className="text-13 text-secondary">
              {t("columnViews.intro")}
            </p>
          </div>
          <button
            type="button"
            aria-label={t("columnViews.close")}
            onClick={close}
            data-testid="edit-views-close"
            className="-me-2 -mt-1 flex size-8 shrink-0 items-center justify-center rounded-control text-secondary hover:bg-hover hover:text-primary"
          >
            <X aria-hidden className="size-(--ui-icon)" strokeWidth={1.75} />
          </button>
        </div>
        {saveError && <ErrorNotice context={t("columnViews.saveFailed")} error={saveError} data-testid="edit-views-error" />}
        <ul ref={listRef} aria-label={t("columnViews.list")} className="-mx-2 flex min-h-0 flex-col gap-px overflow-y-auto" data-testid="edit-views-list">
          {views.map((view, index) => {
            const label = name(view.id);
            const dragged = dragging === view.id;
            const moveButtons = (
              <>
                <MoveButton
                  icon={ArrowUp}
                  label={t("columnViews.moveUp", { name: label })}
                  disabled={index === 0}
                  onClick={() => {
                    move(index, index - 1);
                  }}
                  data-testid={`edit-views-up-${view.id}`}
                />
                <MoveButton
                  icon={ArrowDown}
                  label={t("columnViews.moveDown", { name: label })}
                  disabled={index === views.length - 1}
                  onClick={() => {
                    move(index, index + 1);
                  }}
                  data-testid={`edit-views-down-${view.id}`}
                />
              </>
            );
            if (view.id === MORE_ENTRY) {
              // Stage 23 (decision 0085): the line of "More": the views below it show under "More"
              // in the column. For the mouse it moves like a view; with the keyboard the views move
              // across it.
              return (
                <li
                  key={view.id}
                  data-view-row={view.id}
                  data-dragging={dragged || undefined}
                  data-testid="edit-views-row-more"
                  className={cx(
                    "flex h-10 shrink-0 items-center gap-1 rounded-tile ps-1 pe-1 transition-colors",
                    dragged ? "bg-selected shadow-floating" : dragging === null && "hover:bg-hover",
                  )}
                >
                  <span
                    aria-hidden
                    onPointerDown={startDrag(view.id)}
                    data-testid="edit-views-handle-more"
                    className={cx(
                      "flex size-8 shrink-0 touch-none items-center justify-center rounded-control text-tertiary",
                      dragging === null ? "cursor-grab hover:text-secondary" : "cursor-grabbing",
                    )}
                  >
                    <GripVertical className="size-(--ui-icon-small)" strokeWidth={1.75} />
                  </span>
                  <span className="flex min-w-0 flex-1 items-center gap-2 text-11 font-semibold text-tertiary">
                    <span className="truncate">{t("columnViews.moreLine")}</span>
                    <span aria-hidden className="h-px flex-1 bg-border-subtle" />
                  </span>
                  {moveButtons}
                </li>
              );
            }
            const Icon = MAIL_VIEW_ICONS[view.id];
            return (
              <li
                key={view.id}
                data-view-row={view.id}
                data-dragging={dragged || undefined}
                data-testid={`edit-views-row-${view.id}`}
                className={cx(
                  "flex h-10 shrink-0 items-center gap-1 rounded-tile ps-1 pe-1 transition-colors",
                  dragged ? "bg-selected shadow-floating" : dragging === null && "hover:bg-hover",
                )}
              >
                {/* For the mouse only: the keyboard moves a view with Alt+arrow keys. */}
                <span
                  aria-hidden
                  onPointerDown={startDrag(view.id)}
                  data-testid={`edit-views-handle-${view.id}`}
                  className={cx(
                    "flex size-8 shrink-0 touch-none items-center justify-center rounded-control text-tertiary",
                    dragging === null ? "cursor-grab hover:text-secondary" : "cursor-grabbing",
                  )}
                >
                  <GripVertical className="size-(--ui-icon-small)" strokeWidth={1.75} />
                </span>
                <label className="flex h-full min-w-0 flex-1 cursor-pointer items-center gap-2">
                  <input
                    type="checkbox"
                    checked={view.shown}
                    onChange={(event) => {
                      toggle(view.id, event.target.checked);
                    }}
                    onKeyDown={onCheckboxKeyDown(index)}
                    aria-describedby={hintId}
                    aria-keyshortcuts="Alt+ArrowUp Alt+ArrowDown"
                    data-testid={`edit-views-check-${view.id}`}
                    className="size-4 shrink-0 accent-accent"
                  />
                  <Icon aria-hidden className={cx("size-(--ui-icon-small) shrink-0", view.shown ? "text-secondary" : "text-tertiary")} strokeWidth={1.75} />
                  <span className={cx("truncate text-13", view.shown ? "text-primary" : "text-secondary")}>{label}</span>
                </label>
                {moveButtons}
              </li>
            );
          })}
        </ul>
        <p id={hintId} className="text-12 text-tertiary">
          {t("columnViews.hint", keys)}
        </p>
        <div ref={doneRef} className="flex items-center justify-between gap-2">
          <Button
            variant="secondary"
            disabled={isDefault}
            onClick={resetToDefault}
            data-testid="edit-views-reset"
          >
            {t("columnViews.reset")}
          </Button>
          <Button onClick={close} data-testid="edit-views-done">
            {t("columnViews.done")}
          </Button>
        </div>
        <span aria-live="polite" className="sr-only" data-testid="edit-views-announcement">
          {announcement}
        </span>
      </div>
    </>
  );
}
