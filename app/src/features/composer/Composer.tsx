import {
  Archive,
  CalendarClock,
  ChevronDown,
  ChevronUp,
  Clock,
  FileText,
  ImagePlus,
  Minus,
  MoreHorizontal,
  Paperclip,
  Trash2,
  Upload,
  X,
} from "lucide-react";
import { lazy, Suspense, useEffect, useId, useLayoutEffect, useRef, useState, type KeyboardEvent, type ReactNode, type RefObject } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import { cx } from "../../components/cx";
import { ErrorNotice } from "../../components/ErrorNotice";
import { folderError, useWindowFileDrop } from "../../components/fileDrop";
import { Notice } from "../../components/Notice";
import { Tooltip } from "../../components/Tooltip";
import {
  composeIpc,
  composerIpc,
  parityIpc,
  reminderIpc,
  snippetsIpc,
  toCoreError,
  type AccountColor,
  type Address,
  type ComposeIdentity,
  type CoreError,
  type Draft,
  type DraftAttachment,
  type DraftSendOptions,
  type DraftSizeCheck,
  type DraftUpdate,
  type InlineImagesAdded,
  type QuoteLanguage,
  type TemplateFillProblem,
  type TemplateId,
  uiIpc,
} from "../../services/ipc";
// Stage 8: the composer in a window of its own.
import { OpenComposerWindowButton, useComposerInWindow } from "../windows/composerPlacement";
import { takeReturnFocus, useComposerStore, type ComposePrefill, type ComposerSource } from "../../stores/composer";
import { useComposerBridge } from "../../stores/composerBridge";
import { boundShortcuts } from "../../stores/shortcuts";
import { useSignatures, useTemplates } from "../snippets/queries";
import { matchesShortcut } from "../actions/shortcuts";
import type { PasteResult, RichEditorHandle } from "./RichEditor";
import { SendLaterPicker, sendLaterPresets } from "./SendLaterPicker";
import { AttachmentReminder } from "./AttachmentReminder";
import { FollowUpOption } from "./FollowUpOption";
import { useUiStore } from "../../stores/ui";
import { useAppInfo } from "../app-info/queries";
import { formatShortcut, shortcutPlatform } from "../actions/shortcuts";
import { formatSize, htmlToText, invalidAddressIn, textToHtml } from "./addresses";
import { ComposerAttachments } from "./ComposerAttachments";
import { MailtoNotices } from "../extras/MailtoNotices";
import { useComposeIdentities } from "./queries";
import { RecipientField } from "./RecipientField";
// Stage 19 (decision 0056): a group with many members suggests Bcc.
import { MANY_RECIPIENTS } from "../../services/ipc";
import { addAddresses, sameAddress } from "./addresses";
import { SignatureHtml } from "./SignaturePreview";
import { WritingHelp } from "../ai/WritingHelp";
import { ariaShortcut } from "../actions/shortcuts";
// Stage 23 (decision 0086): the composer's menus; the account's colour in the "From" chip.
import { ComposerMenu, type MenuEntry, type MenuSection } from "./ComposerMenu";
import { accountFill } from "../accounts/accountColor";
import { useAccounts } from "../accounts/queries";

// The editor (TipTap, ProseMirror) loads with the first rich text body, not with the app.
const RichEditor = lazy(() => import("./RichEditor").then((module) => ({ default: module.RichEditor })));

const FORMATS: readonly DraftUpdate["format"][] = ["html", "text"];
const QUOTE_POSITIONS: readonly DraftUpdate["quotePosition"][] = ["below", "above"];

/** Local save this long after the last change (contract: about 3 s), and on leaving a field. */
export const DRAFT_SAVE_DELAY_MS = 3000;

/** The fields of a draft the user edits; attachments have their own commands. */
export function updateOf(draft: Draft): DraftUpdate {
  return {
    identityId: draft.identityId,
    to: draft.to,
    cc: draft.cc,
    bcc: draft.bcc,
    subject: draft.subject,
    format: draft.format,
    bodyHtml: draft.bodyHtml,
    bodyText: draft.bodyText,
    includeQuote: draft.includeQuote,
    quotePosition: draft.quotePosition,
    includeSignature: draft.includeSignature,
    signatureId: draft.signatureId,
  };
}

/** Files that go into the text when dropped on a rich text draft (the core checks the bytes). */
export function isImagePath(path: string): boolean {
  return /\.(png|jpe?g|gif|webp)$/i.test(path);
}

/** Puts `text` into a plain text body at the textarea's cursor (at the end without one). */
export function insertAtCursor(body: string, text: string, field: HTMLTextAreaElement | null): string {
  if (!field || document.activeElement !== field) return body === "" ? text : `${body}\n${text}`;
  const start = field.selectionStart;
  const end = field.selectionEnd;
  return body.slice(0, start) + text + body.slice(end);
}

/** The quote language follows the app's language ("On … wrote:" / "Am … schrieb …:"). */
function quoteLanguage(language: string): QuoteLanguage {
  return language.toLowerCase().startsWith("de") ? "de" : "en";
}

/**
 * Stage 9: a `mailto:` link fills more than the recipients the core's new draft takes. The rest
 * goes in with the first save, before the form shows the draft.
 */
export async function prefillDraft(draft: Draft, prefill: ComposePrefill): Promise<Draft> {
  const { cc, bcc, subject, body } = prefill;
  if (cc.length === 0 && bcc.length === 0 && !subject && !body) return draft;
  const text = body ? (draft.format === "text" ? { bodyText: body } : { bodyHtml: textToHtml(body) }) : {};
  try {
    return await composeIpc.saveDraft(draft.id, { ...updateOf(draft), cc: [...cc], bcc: [...bcc], subject: subject ?? draft.subject, ...text });
  } catch (e) {
    // "Try again" starts over with a new draft; this half-filled one must not stay behind.
    await composeIpc.deleteDraft(draft.id).catch(() => undefined);
    throw e;
  }
}

function loadDraft(source: ComposerSource, language: QuoteLanguage): Promise<Draft> {
  switch (source.kind) {
    case "new": {
      const { prefill } = source;
      const created = composeIpc.createDraft({ kind: "new", accountId: source.accountId, identityId: null, to: [...(source.to ?? [])] });
      return prefill ? created.then((draft) => prefillDraft(draft, prefill)) : created;
    }
    case "reply":
    case "reply_all":
    case "forward":
    case "forward_as_attachment":
      return composeIpc.createDraft({ kind: source.kind, messageId: source.messageId, language });
    case "draft":
      return source.draft ? Promise.resolve(source.draft) : composeIpc.getDraft(source.draftId);
    case "message":
      return composeIpc.openDraftFromMessage(source.messageId);
    case "edit_as_new":
      return composeIpc.createDraft({ kind: "edit_as_new", messageId: source.messageId });
  }
}

/**
 * Loads in flight, by opening: React's StrictMode runs the effect twice in development, and a
 * second `createDraft` would leave an orphaned draft behind.
 */
const loading = new Map<string, Promise<Draft>>();

function loadOnce(key: string, start: () => Promise<Draft>): Promise<Draft> {
  const running = loading.get(key);
  if (running) return running;
  const promise = start().finally(() => {
    loading.delete(key);
  });
  loading.set(key, promise);
  return promise;
}

/** The open composer, if any: one at a time, a card over the reading area (docs/DESIGN.md). */
export function ComposerHost() {
  const session = useComposerStore((state) => state.session);
  const open = session !== null;
  // Closed (sent, saved, discarded): the focus goes back to where it was, e.g. the message list.
  useEffect(() => {
    if (open) return;
    const lost = document.activeElement === null || document.activeElement === document.body;
    const target = takeReturnFocus();
    if (lost) target?.focus();
  }, [open]);
  if (!session) return null;
  return <Composer key={session.seq} seq={session.seq} source={session.source} />;
}

function Composer({ seq, source }: { seq: number; source: ComposerSource }) {
  const { t, i18n } = useTranslation();
  const close = useComposerStore((state) => state.close);
  const setOpenDraft = useComposerStore((state) => state.setOpenDraft);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [error, setError] = useState<CoreError | null>(null);
  const [attempt, setAttempt] = useState(0);
  const language = quoteLanguage(i18n.language);

  useEffect(() => {
    let active = true;
    loadOnce(`${String(seq)}:${String(attempt)}`, () => loadDraft(source, language)).then(
      (loaded) => {
        if (!active) return;
        setDraft(loaded);
        setOpenDraft(loaded.id);
      },
      (e: unknown) => {
        if (active) setError(toCoreError(e));
      },
    );
    return () => {
      active = false;
    };
    // The source is fixed for this composer (a new opening is a new composer); `attempt` retries.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [attempt]);

  if (draft) return <ComposerForm seq={seq} initial={draft} prefill={source.kind === "new" ? source.prefill : undefined} />;
  const title = t(`composer.title.${loadingTitle(source.kind)}`);
  return (
    <ComposerFrame title={title} heading={<ComposerTitle title={title} />} onClose={close}>
      {error ? (
        <div className="px-6 py-4">
          <ErrorNotice
            context={t("composer.errors.open")}
            error={error}
            onRetry={() => {
              setError(null);
              setAttempt((n) => n + 1);
            }}
            data-testid="composer-open-error"
          />
        </div>
      ) : (
        <p role="status" className="px-6 py-4 text-13 text-secondary" data-testid="composer-loading">
          {t("composer.loading")}
        </p>
      )}
    </ComposerFrame>
  );
}

function loadingTitle(kind: ComposerSource["kind"]): "new" | "reply" | "reply_all" | "forward" | "draft" {
  if (kind === "draft" || kind === "message") return "draft";
  if (kind === "forward_as_attachment") return "forward";
  if (kind === "edit_as_new") return "new";
  return kind;
}

/** The title while the draft loads: the kind of message, where the subject will stand. */
function ComposerTitle({ title }: { title: string }) {
  return <p className="min-w-0 flex-1 truncate text-20 font-bold text-primary">{title}</p>;
}

/** What can take the focus inside the card (for Tab at its ends). */
const FOCUSABLE =
  'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [contenteditable="true"], [tabindex]:not([tabindex="-1"])';

/** Shown: not inside something hidden (the folded quote, the minimised form) or inert. */
function shown(element: HTMLElement): boolean {
  return element.closest("[hidden], .hidden, [inert]") === null;
}

function focusables(root: HTMLElement): HTMLElement[] {
  return [...root.querySelectorAll<HTMLElement>(FOCUSABLE)].filter(shown);
}

/**
 * Stage 23 (decision 0086): where the card stands in the main window. Over the reading area when
 * the reading pane is beside the list (its start is where the list ends, its top where the area
 * below the view's header starts), else at the end of the window. Measured on the DOM (the list's
 * width follows its handle, the layout and the view change while the composer is open) and given
 * to the card as CSS variables (CSSOM, no style attribute: CSP); the stylesheet keeps the card
 * inside the window and wide enough.
 */
function useCardPlacement(card: RefObject<HTMLElement | null>, enabled: boolean): void {
  useLayoutEffect(() => {
    const node = card.current;
    const host = node?.parentElement;
    if (!enabled || !node || !host) return undefined;
    let frame = 0;
    let watched: readonly HTMLElement[] = [];
    const resize = new ResizeObserver(() => {
      schedule();
    });
    const place = () => {
      frame = 0;
      const next = placeCard(node, host);
      // Watch what decides the place: the window's content, the reading area and the list.
      if (next.length !== watched.length || next.some((element, index) => element !== watched[index])) {
        resize.disconnect();
        for (const element of next) resize.observe(element);
        watched = next;
      }
    };
    const schedule = () => {
      if (frame === 0) frame = window.requestAnimationFrame(place);
    };
    // A view or a layout replaced: the reading area may be another element now. The card's own
    // changes (typing) and the rows of the list (scrolling) do not matter.
    const mutations = new MutationObserver((records) => {
      const list = watched[2];
      if (records.some((record) => !node.contains(record.target) && !list?.contains(record.target))) schedule();
    });
    mutations.observe(host, { childList: true, subtree: true, attributes: true, attributeFilter: ["data-reading-pane"] });
    place();
    return () => {
      window.cancelAnimationFrame(frame);
      resize.disconnect();
      mutations.disconnect();
    };
  }, [card, enabled]);
}

/** A bar at the top of the reading area is lower than this (px): a toolbar's row, not the pane. */
const BAR_MAX_HEIGHT = 80;

/** Measures the reading area in `host` and puts the card there; returns what was measured. */
function placeCard(node: HTMLElement, host: HTMLElement): HTMLElement[] {
  const box = host.getBoundingClientRect();
  const area = [...host.querySelectorAll<HTMLElement>("[data-reading-pane]")].find((element) => !node.contains(element)) ?? null;
  let top = 0;
  let start: number | null = null;
  let list: HTMLElement | null = null;
  if (area) {
    const areaTop = area.getBoundingClientRect().top;
    top = areaTop - box.top;
    if (area.dataset["readingPane"] === "right") {
      const handle = area.querySelector('[role="separator"][aria-controls]');
      const id = handle?.getAttribute("aria-controls");
      list = id ? document.getElementById(id) : null;
      if (list) {
        const rect = list.getBoundingClientRect();
        start = document.documentElement.dir === "rtl" ? box.right - rect.left : rect.right - box.left;
      }
      // The reading pane's toolbar at the top of the area stays visible and usable: the card
      // starts below its bar.
      for (const toolbar of area.querySelectorAll<HTMLElement>('[role="toolbar"]')) {
        if (node.contains(toolbar) || list?.contains(toolbar)) continue;
        let bar: HTMLElement = toolbar;
        while (bar.parentElement && bar.parentElement !== area && bar.parentElement.getBoundingClientRect().height < BAR_MAX_HEIGHT) bar = bar.parentElement;
        const rect = bar.getBoundingClientRect();
        if (rect.top - areaTop < BAR_MAX_HEIGHT && rect.height > 0) top = Math.max(top, rect.bottom - box.top);
      }
    }
  }
  // Written only when it changed: an unchanged card needs no new style.
  const set = (name: string, value: string | null) => {
    if (value === null) {
      if (node.style.getPropertyValue(name) !== "") node.style.removeProperty(name);
    } else if (node.style.getPropertyValue(name) !== value) node.style.setProperty(name, value);
  };
  set("--composer-top", `${String(Math.max(0, Math.round(top)))}px`);
  set("--composer-start", start === null ? null : `${String(Math.max(0, Math.round(start)))}px`);
  const placement = start === null ? "end" : "area";
  if (node.dataset["placement"] !== placement) node.dataset["placement"] = placement;
  // In this order: the window's content, the reading area, the list (read by the observer).
  return area ? (list ? [host, area, list] : [host, area]) : [host];
}

interface ComposerFrameProps {
  /** The card's name (the subject, else the kind of message); also the minimised bar's text. */
  title: string;
  /** The top line's main part: the subject field (or the title while loading). */
  heading: ReactNode;
  onClose: () => void;
  status?: ReactNode;
  actions?: ReactNode;
  children: ReactNode;
  onKeyDown?: (event: KeyboardEvent<HTMLElement>) => void;
  dropping?: boolean;
  /** Minimised to a slim bar (main window only); the form keeps its state meanwhile. */
  minimised?: boolean;
  onMinimisedChange?: (minimised: boolean) => void;
}

/**
 * Stage 23 (decision 0086): the composer as a floating card over the reading area of the main
 * window (14 px corners, a hairline, the panel shadow, 20 px from the area's top and bottom and
 * 24 px from its sides), the list beside it stays visible and usable. A non-modal dialog: Tab
 * cycles inside it, Escape closes it (the draft is kept), the focus goes back to where it was.
 * "Minimise" docks it as a slim bar at the bottom of the window; the draft, the editor and the
 * focus inside wait there. In a window of its own (stage 8) the same card fills the window.
 */
function ComposerFrame({ title, heading, onClose, status, actions, children, onKeyDown, dropping = false, minimised = false, onMinimisedChange }: ComposerFrameProps) {
  const { t } = useTranslation();
  const titleId = useId();
  const platform = shortcutPlatform(useAppInfo().data?.platform);
  const inWindow = useComposerInWindow();
  const card = useRef<HTMLElement>(null);
  const restoreButton = useRef<HTMLButtonElement>(null);
  /** Where the focus was in the card when it was minimised; it goes back there. */
  const lastFocus = useRef<HTMLElement | null>(null);
  const minimisable = !inWindow && onMinimisedChange !== undefined;
  const shownMinimised = minimisable && minimised;
  useCardPlacement(card, !inWindow);

  const minimise = () => {
    onMinimisedChange?.(true);
  };
  const restore = () => {
    onMinimisedChange?.(false);
  };
  // Minimised: the focus on the bar's button. Restored: back where it was, else the first field.
  const wasMinimised = useRef(shownMinimised);
  useLayoutEffect(() => {
    if (wasMinimised.current === shownMinimised) return;
    wasMinimised.current = shownMinimised;
    if (shownMinimised) {
      restoreButton.current?.focus();
      return;
    }
    const target = lastFocus.current;
    lastFocus.current = null;
    if (target?.isConnected && card.current?.contains(target) && shown(target)) target.focus();
    else if (card.current) focusables(card.current)[0]?.focus();
  }, [shownMinimised]);

  return (
    <section
      ref={card}
      role={inWindow ? undefined : "dialog"}
      aria-labelledby={titleId}
      onFocus={(event) => {
        // The last place the user worked in (not the buttons of the top line or the bar): the
        // focus goes back there when the card comes back from its bar.
        if (event.target instanceof HTMLElement && !event.target.closest("[data-composer-chrome]")) lastFocus.current = event.target;
      }}
      onKeyDown={(event) => {
        // Keys of a dialog this composer opened arrive through the portal; they are the dialog's.
        if (!(event.target instanceof Node) || !event.currentTarget.contains(event.target)) return;
        onKeyDown?.(event);
        if (!event.defaultPrevented && event.key === "Escape") {
          event.preventDefault();
          onClose();
        }
        // Tab at the card's ends goes round inside it (the list stays usable with the pointer).
        if (!event.defaultPrevented && event.key === "Tab" && !inWindow && !shownMinimised && card.current) {
          const all = focusables(card.current);
          const first = all[0];
          const last = all.at(-1);
          if (event.shiftKey && document.activeElement === first) {
            event.preventDefault();
            last?.focus();
          } else if (!event.shiftKey && document.activeElement === last) {
            event.preventDefault();
            first?.focus();
          }
        }
        // The list's single-key shortcuts (E, #, …) must not act on mail behind the composer. The
        // command palette opens from here too (stage 10: templates are inserted through it).
        const palette = boundShortcuts("palette", platform).some((s) => matchesShortcut(event, s, platform));
        if (!palette) event.stopPropagation();
      }}
      className={cx(
        "z-20 flex flex-col bg-surface-raised",
        inWindow
          ? "absolute inset-0"
          : shownMinimised
            ? "absolute end-6 bottom-0 w-(--ui-composer-bar-width) max-w-[calc(100%-var(--ui-space-12))] rounded-t-panel border border-b-0 border-border-subtle shadow-panel"
            : cx(
                "absolute end-6 bottom-5 top-[calc(var(--composer-top,0px)+var(--ui-space-5))] rounded-panel border border-border-subtle shadow-panel",
                // Over the reading area (its start where the list ends), never narrower than the
                // reading pane's minimum; centred beyond the widest comfortable card.
                "data-[placement=area]:start-[max(var(--ui-space-6),min(calc(var(--composer-start,0px)+var(--ui-space-6)),calc(100%-var(--ui-composer-min-width)-var(--ui-space-6))))] data-[placement=area]:mx-auto data-[placement=area]:max-w-(--ui-composer-max-width)",
                // No reading pane beside the list: at the end of the window.
                "w-[min(calc(100%-var(--ui-space-12)),var(--ui-composer-end-width))] data-[placement=area]:w-auto",
              ),
        dropping && "outline-2 -outline-offset-2 outline-focus-ring",
      )}
      data-minimised={shownMinimised || undefined}
      data-app-shortcuts=""
      data-testid="composer"
    >
      <h2 id={titleId} className="sr-only">
        {title}
      </h2>
      {shownMinimised ? (
        <div className="flex h-12 shrink-0 items-center gap-1 ps-2 pe-2" data-composer-chrome>
          <button
            ref={restoreButton}
            type="button"
            aria-label={t("composer.expand", { title })}
            onClick={restore}
            className="flex h-8 min-w-0 flex-1 items-center gap-2 rounded-control px-2 text-start text-13 font-semibold text-primary hover:bg-hover"
            data-testid="composer-restore"
          >
            <span className="min-w-0 flex-1 truncate">{title}</span>
            <ChevronUp aria-hidden className="size-(--ui-icon-small) shrink-0 text-secondary" strokeWidth={1.75} />
          </button>
          <CloseButton onClose={onClose} />
        </div>
      ) : (
        <div className="flex shrink-0 items-center gap-2 border-b border-border-subtle pt-4 pb-2 ps-6 pe-4" data-testid="composer-head">
          {heading}
          {status}
          <div className="flex shrink-0 items-center gap-2" data-composer-chrome>
            {actions}
            {minimisable && (
              <Tooltip label={t("composer.minimise")}>
                <button
                  type="button"
                  aria-label={t("composer.minimise")}
                  onClick={minimise}
                  className={HEAD_BUTTON}
                  data-testid="composer-minimise"
                >
                  <Minus aria-hidden className="size-(--ui-icon)" strokeWidth={1.75} />
                </button>
              </Tooltip>
            )}
            <CloseButton onClose={onClose} />
          </div>
        </div>
      )}
      {/* Minimised, the form stays (draft, editor, its undo history), out of sight. */}
      <div className={shownMinimised ? "hidden" : "flex min-h-0 flex-1 flex-col"}>{children}</div>
    </section>
  );
}

const HEAD_BUTTON = "flex size-8 shrink-0 items-center justify-center rounded-control text-tertiary hover:bg-hover hover:text-primary disabled:opacity-50";

function CloseButton({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation();
  return (
    <Tooltip label={t("composer.close")}>
      <button type="button" aria-label={t("composer.close")} onClick={onClose} className={HEAD_BUTTON} data-testid="composer-close">
        <X aria-hidden className="size-(--ui-icon)" strokeWidth={1.75} />
      </button>
    </Tooltip>
  );
}

type SaveState = "idle" | "saving" | "saved" | "error";

type ActionErrorContext = "attach" | "discard" | "unfreeze" | "images" | "template" | "size" | "window" | "sendOptions";

const ACTION_ERROR_TEXT = {
  attach: "composer.errors.attach",
  discard: "composer.errors.discard",
  unfreeze: "composer.errors.unfreeze",
  images: "writing.images.failed",
  template: "writing.templates.failed",
  size: "writing.size.checkFailed",
  window: "windows.composer.openFailed",
  sendOptions: "composer.sendOptions.failed",
} as const satisfies Record<ActionErrorContext, string>;

/** Stage 16: Cmd/Ctrl+Shift+Enter sends and archives the original (fixed, like Cmd/Ctrl+Enter). */
export const SEND_AND_ARCHIVE_SHORTCUT = { key: "Enter", mod: true, shift: true } as const;

/** The value of the signature choice: the draft kind's default, one of the pool, or none. */
const SIGNATURE_DEFAULT = "default";
const SIGNATURE_NONE = "none";

function ComposerForm({ seq, initial, prefill }: { seq: number; initial: Draft; prefill?: ComposePrefill | undefined }) {
  const { t, i18n } = useTranslation();
  const language = quoteLanguage(i18n.language);
  const close = useComposerStore((state) => state.close);
  const track = useComposerStore((state) => state.track);
  const report = useComposerStore((state) => state.report);
  const showAccountsPage = useUiStore((state) => state.showAccountsPage);
  const platform = shortcutPlatform(useAppInfo().data?.platform);
  const identities = useComposeIdentities(initial.accountId);
  const [draft, setDraft] = useState(initial);
  const [fields, setFields] = useState<DraftUpdate>(() => updateOf(initial));
  const [editorKey, setEditorKey] = useState(0);
  const [showCc, setShowCc] = useState(initial.cc.length > 0);
  const [showBcc, setShowBcc] = useState(initial.bcc.length > 0);
  // Stage 19: the members of the group just added to To or Cc, while the hint about Bcc shows.
  const [groupHint, setGroupHint] = useState<{ field: "to" | "cc"; members: Address[] } | null>(null);
  const [saveState, setSaveState] = useState<SaveState>("idle");
  const [saveError, setSaveError] = useState<CoreError | null>(null);
  const [sendError, setSendError] = useState<CoreError | null>(null);
  const [actionError, setActionError] = useState<{ context: ActionErrorContext; error: CoreError } | null>(null);
  const [sending, setSending] = useState(false);
  const [attaching, setAttaching] = useState(false);
  const [confirmDiscard, setConfirmDiscard] = useState(false);
  // Stage 10: inline images (shown from `data:` URLs, saved as `cid:`), the size check, notes after
  // pasting or inserting a template, the template menu and "send later".
  const [imageSources, setImageSources] = useState<Record<string, string>>({});
  const [addingImages, setAddingImages] = useState(false);
  const [sizeCheck, setSizeCheck] = useState<DraftSizeCheck | null>(null);
  const [pasteNote, setPasteNote] = useState<PasteResult | null>(null);
  const [templateOpen, setTemplateOpen] = useState(false);
  const [templateNote, setTemplateNote] = useState<{
    name: string;
    problems: readonly TemplateFillProblem[];
    recipientsAdded: number;
  } | null>(null);
  // Stage 16: "send and archive" and "request read receipt" of this draft (`setDraftSendOptions`).
  const [sendOptions, setSendOptions] = useState<DraftSendOptions>({
    archiveOriginal: initial.archiveOriginal,
    requestReadReceipt: initial.requestReadReceipt,
    followUpAt: initial.followUpAt,
  });
  const optionsQueue = useRef<Promise<unknown>>(Promise.resolve());
  const [laterOpen, setLaterOpen] = useState(false);
  const [scheduling, setScheduling] = useState(false);
  const [laterError, setLaterError] = useState<CoreError | null>(null);
  // Stage 20 (decision 0058): the text speaks of an attachment but none is there; `proceed`
  // sends (or schedules) as the user first asked, without asking again.
  const [attachmentAsk, setAttachmentAsk] = useState<{ word: string; proceed: () => void } | null>(null);
  const editorHandle = useRef<RichEditorHandle | null>(null);
  const textBody = useRef<HTMLTextAreaElement | null>(null);
  const requestedImages = useRef(new Set<string>());
  const mounted = useRef(true);
  const signatures = useSignatures();
  const templates = useTemplates(templateOpen);
  const setInsertTemplate = useComposerBridge((state) => state.setInsertTemplate);
  const accounts = useAccounts();
  const templateMenuId = useId();
  const textBodyId = useId();
  const quoteId = useId();
  // Stage 23 (decision 0086): the card minimised to a bar; the quote folded (shown on request);
  // where the editor draws its compact formatting bar; the send button's arrow (focus target).
  const [minimised, setMinimised] = useState(false);
  const [quoteShown, setQuoteShown] = useState(false);
  const [formatBarSlot, setFormatBarSlot] = useState<HTMLDivElement | null>(null);
  const sendArrow = useRef<HTMLDivElement>(null);

  // Latest fields for saves that run after a render (debounce, unmount); `dirty` since the last save.
  const latest = useRef(fields);
  const dirty = useRef(false);
  const timer = useRef<number | undefined>(undefined);
  const queue = useRef<Promise<unknown>>(Promise.resolve());
  const finished = useRef(false);
  const draftId = initial.id;
  const frozen = draft.state === "sending";

  /** Saves what changed since the last save; saves run one after the other. Rejects on failure. */
  const flush = (): Promise<void> => {
    window.clearTimeout(timer.current);
    const run = queue.current.then(async () => {
      if (!dirty.current) return;
      dirty.current = false;
      const update = latest.current;
      setSaveState("saving");
      try {
        const saved = await composeIpc.saveDraft(draftId, update);
        setDraft(saved);
        setSaveState("saved");
        setSaveError(null);
      } catch (e) {
        dirty.current = true;
        const err = toCoreError(e);
        setSaveState("error");
        setSaveError(err);
        throw err;
      }
    });
    // A failed save must not block the next one.
    queue.current = run.catch(() => undefined);
    return run;
  };

  const change = (patch: Partial<DraftUpdate>) => {
    const next = { ...latest.current, ...patch };
    latest.current = next;
    dirty.current = true;
    setFields(next);
    setSendError(null);
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => {
      void flush().catch(() => undefined);
    }, DRAFT_SAVE_DELAY_MS);
  };
  const saveNow = () => {
    if (dirty.current) void flush().catch(() => undefined);
  };
  // Stage 19 (decision 0056): a group chosen in a recipient field becomes its members there.
  const addGroup = (field: "to" | "cc" | "bcc", members: Address[]) => {
    change({ [field]: addAddresses(latest.current[field], members) });
    if (field !== "bcc") setGroupHint({ field, members });
  };

  // Replaced by another composer: save and hand the draft to the server copy. Not when the
  // session simply ended (closed, sent, discarded, removed elsewhere) or React only simulates an
  // unmount (StrictMode in development keeps the same session).
  useEffect(
    () => () => {
      window.clearTimeout(timer.current);
      const current = useComposerStore.getState().session;
      if (finished.current || !current || current.seq === seq) return;
      void flush()
        .then(() => composeIpc.closeDraft(draftId))
        .catch((e: unknown) => {
          report("close", toCoreError(e));
        });
    },
    // Runs once on unmount; `flush` reads refs only.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [],
  );

  // Stage 23 (decision 0082): files dropped on the window while the composer is open become
  // attachments, handed over as bytes (the page never sees a path).
  const fileDrop = useWindowFileDrop(({ files, folders }) => {
    setMinimised(false);
    if (folders.length > 0) setActionError({ context: "attach", error: folderError(folders) });
    if (files.length === 0) return;
    // Rich text: images go into the text (stage 10), everything else is attached.
    const inline = latest.current.format === "html" ? files.filter((file) => isImagePath(file.name)) : [];
    const attached = files.filter((file) => !inline.includes(file));
    void (async () => {
      if (inline.length > 0) {
        setAddingImages(true);
        try {
          applyAdded(await composerIpc.addInlineImageFiles(draftId, inline));
        } catch (e) {
          setActionError({ context: "images", error: toCoreError(e) });
        } finally {
          setAddingImages(false);
        }
      }
      if (attached.length > 0) {
        setAttaching(true);
        try {
          const saved = await composeIpc.attachFiles(draftId, attached);
          setDraft((d) => ({ ...d, attachments: saved.attachments, server: saved.server, state: saved.state }));
          checkSize();
        } catch (e) {
          setActionError({ context: "attach", error: toCoreError(e) });
        } finally {
          setAttaching(false);
        }
      }
    })();
  });
  const dropping = fileDrop.over;
  // Files dragged over the window bring a minimised card back, so the drop shows where it goes.
  if (dropping && minimised) setMinimised(false);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  // Inline images of the draft (also of a reopened one) are shown from `data:` URLs.
  const inlineImages = draft.attachments.filter((a) => a.contentId !== null);
  const inlineKey = inlineImages.map((a) => a.id).join(" ");
  useEffect(() => {
    for (const image of inlineImages) {
      const contentId = image.contentId;
      if (contentId === null || requestedImages.current.has(image.id)) continue;
      requestedImages.current.add(image.id);
      composerIpc.inlineImageData(draftId, image.id).then(
        (url) => {
          if (mounted.current) setImageSources((current) => ({ ...current, [contentId]: url }));
        },
        (e: unknown) => {
          requestedImages.current.delete(image.id);
          if (mounted.current) setActionError({ context: "images", error: toCoreError(e) });
        },
      );
    }
    // Keyed by the ids of the inline images; the list itself is new on every render.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [draftId, inlineKey]);

  // A draft that starts with files (a forward, "forward as attachment") is checked against the
  // server's size limit at once.
  useEffect(() => {
    if (initial.attachments.length > 0) checkSize();
    // Once per composer.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  /** Asks the core for the size against the SMTP server's SIZE limit; offline is no error. */
  function checkSize(): void {
    composerIpc.checkDraftSize(draftId).then(
      (check) => {
        if (mounted.current) setSizeCheck(check);
      },
      (e: unknown) => {
        if (mounted.current) setActionError({ context: "size", error: toCoreError(e) });
      },
    );
  }

  /** New inline images: into the text at the cursor as `cid:` images, then the size check. */
  function applyAdded(result: InlineImagesAdded): void {
    setDraft((d) => ({ ...d, attachments: result.draft.attachments, server: result.draft.server }));
    const images = result.added.flatMap((a) => (a.contentId ? [{ contentId: a.contentId, alt: "" }] : []));
    editorHandle.current?.insertImages(images);
    if (images.length > 0) checkSize();
  }

  const closeComposer = async () => {
    finished.current = true;
    window.clearTimeout(timer.current);
    try {
      await flush();
      await composeIpc.closeDraft(draftId);
    } catch (e) {
      // The draft stays local; the next save or the start of the app retries the server copy.
      report("close", toCoreError(e));
    }
    close();
  };

  // Stage 8: this draft in a window of its own. The panel here closes without handing the draft
  // to the server copy; the composer of the new window does that when it closes.
  const [openingWindow, setOpeningWindow] = useState(false);
  const openInWindow = async () => {
    finished.current = true;
    window.clearTimeout(timer.current);
    setOpeningWindow(true);
    try {
      await flush();
      await uiIpc.openComposerWindow(draftId);
      close();
    } catch (e) {
      finished.current = false;
      setActionError({ context: "window", error: toCoreError(e) });
    } finally {
      if (mounted.current) setOpeningWindow(false);
    }
  };

  const discard = async () => {
    setConfirmDiscard(false);
    finished.current = true;
    window.clearTimeout(timer.current);
    try {
      await composeIpc.deleteDraft(draftId);
      close();
    } catch (e) {
      finished.current = false;
      setActionError({ context: "discard", error: toCoreError(e) });
    }
  };

  /**
   * Stage 16: the send options go to the core at once (they are not part of the draft's fields);
   * a refusal puts the switch back.
   */
  const applySendOptions = (next: DraftSendOptions): Promise<void> => {
    const previous = sendOptions;
    setSendOptions(next);
    const run = optionsQueue.current.then(async () => {
      try {
        const saved = await parityIpc.setDraftSendOptions(draftId, next);
        setDraft((d) => ({
          ...d,
          archiveOriginal: saved.archiveOriginal,
          requestReadReceipt: saved.requestReadReceipt,
          followUpAt: saved.followUpAt,
        }));
      } catch (e) {
        setSendOptions(previous);
        setActionError({ context: "sendOptions", error: toCoreError(e) });
        throw e;
      }
    });
    optionsQueue.current = run.catch(() => undefined);
    return run;
  };

  /**
   * Sends after the undo delay; `archive` first turns on "archive the original" (Send and archive).
   * Stage 20: asks first when the text speaks of an attachment and none is there (`checked`: the
   * user answered "Send anyway").
   */
  const send = async (archive = false, checked = false) => {
    if (sending || frozen) return;
    setSending(true);
    setSendError(null);
    try {
      dirty.current = true; // the final save, also when nothing changed since the last one
      await flush();
      await optionsQueue.current;
      if (!checked) {
        const word = await reminderIpc.checkAttachmentReminder(draftId);
        if (word !== null) {
          setSending(false);
          setAttachmentAsk({
            word,
            proceed: () => {
              void send(archive, true);
            },
          });
          return;
        }
      }
      if (archive && !sendOptions.archiveOriginal) {
        const next = { ...sendOptions, archiveOriginal: true };
        await parityIpc.setDraftSendOptions(draftId, next);
        setSendOptions(next);
      }
      const mail = await composeIpc.sendDraft(draftId, crypto.randomUUID());
      finished.current = true;
      track(mail);
      close();
    } catch (e) {
      setSendError(toCoreError(e));
      setSending(false);
    }
  };

  const unfreeze = async () => {
    try {
      const released = await composeIpc.cancelSend(draftId);
      setDraft(released);
    } catch (e) {
      setActionError({ context: "unfreeze", error: toCoreError(e) });
    }
  };

  const pick = async () => {
    setAttaching(true);
    try {
      await flush().catch(() => undefined);
      const saved = await composeIpc.pickAttachments(draftId);
      setDraft((d) => ({ ...d, attachments: saved.attachments, server: saved.server }));
      checkSize();
    } catch (e) {
      setActionError({ context: "attach", error: toCoreError(e) });
    } finally {
      setAttaching(false);
    }
  };

  const pickImages = async () => {
    setAddingImages(true);
    try {
      await flush().catch(() => undefined);
      applyAdded(await composerIpc.pickInlineImages(draftId));
    } catch (e) {
      setActionError({ context: "images", error: toCoreError(e) });
    } finally {
      setAddingImages(false);
    }
  };

  /** Fills the template for this draft (first To recipient) and puts it where the cursor is. */
  const insertTemplate = async (templateId: TemplateId) => {
    setTemplateOpen(false);
    if (frozen) return;
    setMinimised(false);
    try {
      await flush().catch(() => undefined);
      const filled = await snippetsIpc.fillTemplate(templateId, draftId, language);
      const name = (templates.data ?? []).find((tpl) => tpl.id === templateId)?.name ?? "";
      if (latest.current.subject.trim() === "" && filled.subject.trim() !== "") change({ subject: filled.subject });
      // Stage 16: the template's recipients, merged by the core after the draft's own.
      if (filled.recipientsAdded > 0) {
        change({ to: [...filled.to], cc: [...filled.cc], bcc: [...filled.bcc] });
        if (filled.cc.length > 0) setShowCc(true);
        if (filled.bcc.length > 0) setShowBcc(true);
      }
      if (latest.current.format === "html") {
        if (editorHandle.current) editorHandle.current.insertHtml(filled.html);
        else {
          change({ bodyHtml: latest.current.bodyHtml + filled.html });
          setEditorKey((n) => n + 1);
        }
      } else {
        change({ bodyText: insertAtCursor(latest.current.bodyText, filled.text, textBody.current) });
      }
      setTemplateNote({ name, problems: filled.problems, recipientsAdded: filled.recipientsAdded });
    } catch (e) {
      setActionError({ context: "template", error: toCoreError(e) });
    }
  };
  // The command palette inserts templates through the open composer.
  const insertRef = useRef(insertTemplate);
  useEffect(() => {
    insertRef.current = insertTemplate;
  });
  useEffect(() => {
    setInsertTemplate((templateId) => {
      void insertRef.current(templateId);
    });
    return () => {
      setInsertTemplate(null);
    };
  }, [setInsertTemplate]);

  /**
   * "Send later": frozen like a send, sent at `sendAt` only if the app runs and is online then.
   * Stage 20: the attachment reminder is asked now, while the user is here; the send itself later
   * never asks.
   */
  const scheduleAt = async (sendAt: number, checked = false) => {
    if (scheduling || frozen) return;
    setScheduling(true);
    setLaterError(null);
    try {
      dirty.current = true;
      await flush();
      if (!checked) {
        const word = await reminderIpc.checkAttachmentReminder(draftId);
        if (word !== null) {
          setScheduling(false);
          setAttachmentAsk({
            word,
            proceed: () => {
              void scheduleAt(sendAt, true);
            },
          });
          return;
        }
      }
      const mail = await composerIpc.scheduleSend(draftId, crypto.randomUUID(), { sendAt });
      finished.current = true;
      track(mail);
      close();
    } catch (e) {
      setLaterError(toCoreError(e));
      setScheduling(false);
    }
  };

  const removeAttachment = async (attachment: DraftAttachment) => {
    try {
      const saved = await composeIpc.removeDraftAttachment(draftId, attachment.id);
      setDraft((d) => ({ ...d, attachments: saved.attachments, server: saved.server }));
      checkSize();
    } catch (e) {
      setActionError({ context: "attach", error: toCoreError(e) });
    }
  };

  const switchFormat = (format: DraftUpdate["format"]) => {
    if (format === fields.format) return;
    if (format === "text") change({ format, bodyText: htmlToText(latest.current.bodyHtml) });
    else change({ format, bodyHtml: textToHtml(latest.current.bodyText) });
    setEditorKey((n) => n + 1);
  };

  const canArchive = draft.originalMessageId !== null;
  const onKeyDown = (event: KeyboardEvent<HTMLElement>) => {
    if (event.key === "Enter" && (platform === "macos" ? event.metaKey : event.ctrlKey) && !event.altKey) {
      // Stage 16: with Shift, "Send and archive" (where there is an original to archive).
      if (event.shiftKey && !canArchive) return;
      event.preventDefault();
      void send(event.shiftKey);
    }
  };

  const identityList = identities.data ?? [];
  const identity =
    identityList.find((i) => i.id === fields.identityId) ?? identityList.find((i) => i.isDefault) ?? identityList[0] ?? null;
  const identityText = (i: ComposeIdentity) => (i.name ? t("composer.identityChip", { name: i.name, email: i.email }) : i.email);
  const account = (accounts.data ?? []).find((a) => a.id === draft.accountId);
  const identitySections: MenuSection[] = [
    {
      id: "identities",
      entries: identityList.map(
        (i): MenuEntry => ({
          kind: "radio",
          id: String(i.id),
          label: identityText(i),
          checked: i.id === identity?.id,
          onSelect: () => {
            change({ identityId: i.id });
            saveNow();
          },
          "data-testid": `composer-identity-${String(i.id)}`,
        }),
      ),
    },
  ];
  const kind = draft.kind;
  // Stage 10: the signature pool; the draft takes its kind's default unless the user chose one.
  const pool = signatures.data ?? [];
  const defaultSignatureId = (kind === "new" ? identity?.signatureNew : identity?.signatureReply) ?? null;
  const defaultSignature = pool.find((s) => s.id === defaultSignatureId) ?? null;
  const chosenSignature = fields.signatureId ? (pool.find((s) => s.id === fields.signatureId) ?? null) : defaultSignature;
  // Without the pool (not loaded), the stage 5 text of the identity's default for new mail.
  const fallbackText = pool.length === 0 && !fields.signatureId && kind === "new" && identity?.signature?.trim() ? identity.signature : null;
  const signature = chosenSignature ? chosenSignature.text.trim() || null : fallbackText;
  const signatureChoice = !fields.includeSignature ? SIGNATURE_NONE : (fields.signatureId ?? SIGNATURE_DEFAULT);
  const chooseSignature = (value: string) => {
    if (value === SIGNATURE_NONE) change({ includeSignature: false });
    else if (value === SIGNATURE_DEFAULT) change({ includeSignature: true, signatureId: null });
    else change({ includeSignature: true, signatureId: value });
    saveNow();
  };
  // The separator the core adds when it puts the message together (plain text, not translated).
  const signatureText = signature ? `-- \n${signature}` : "";
  // Rich text: the signature with its formatting, right below the text inside the writing area;
  // plain text (and the identity's text signature): as text under the text field.
  const richSignature = fields.format === "html" && chosenSignature !== null && signature !== null && fields.includeSignature;
  const signaturePreview = richSignature ? (
    <div aria-label={t("composer.signature.preview")} className="px-2 pt-1 pb-3 font-mail text-13 text-secondary" data-testid="composer-signature">
      <p className="whitespace-pre">{"-- "}</p>
      <SignatureHtml html={chosenSignature.html} />
    </div>
  ) : undefined;
  const hasQuote = draft.quoteHtml !== null || draft.quoteText !== null;
  const refused = sendError?.code === "invalid_address" ? invalidAddressIn(sendError.detail, [fields.to, fields.cc, fields.bcc]) : null;
  const sendShortcut = formatShortcut({ key: "Enter", mod: true }, platform);
  const archiveShortcut = formatShortcut(SEND_AND_ARCHIVE_SHORTCUT, platform);
  const title = fields.subject.trim() || t(`composer.title.${kind}`);
  const html = fields.format === "html";

  const saveText =
    saveState === "saving"
      ? t("composer.saveState.saving")
      : saveState === "error"
        ? t("composer.saveState.error")
        : saveState === "saved"
          ? t(`composer.saveState.server.${draft.server}`)
          : "";
  const status = (
    <p
      aria-live="polite"
      title={saveText || undefined}
      className="max-w-(--ui-composer-status-width) min-w-0 shrink truncate text-11 text-tertiary"
      data-testid="composer-save-state"
      data-state={saveState}
    >
      {saveText}
    </p>
  );

  const recipients = (list: readonly Address[]) => [...list];
  // In rich text with the answer above the original, the original follows the signature inside
  // the writing area (one scrolling document); otherwise it stands as its own block. Stage 23
  // (decision 0086): folded behind "··· Show quote"; it is sent either way.
  const quoteInWriting = html && fields.quotePosition === "above";
  const quoteBlock =
    hasQuote && fields.includeQuote ? (
      <div className={cx("mb-3 flex flex-col items-start gap-2", quoteInWriting ? "px-2" : "px-6")} data-testid="composer-quote">
        <button
          type="button"
          aria-expanded={quoteShown}
          aria-controls={quoteId}
          onClick={() => {
            setQuoteShown((shown) => !shown);
          }}
          className="flex h-6 items-center gap-1 rounded-control bg-badge px-2 text-12 text-secondary hover:bg-hover hover:text-primary"
          data-testid="composer-quote-toggle"
        >
          <MoreHorizontal aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
          {t(kind === "forward" ? (quoteShown ? "composer.quote.hideForward" : "composer.quote.showForward") : quoteShown ? "composer.quote.hide" : "composer.quote.show")}
        </button>
        <div id={quoteId} hidden={!quoteShown} className="w-full rounded-card border border-border-subtle bg-canvas p-3" data-testid="composer-quote-text">
          {draft.quoteText !== null ? (
            <pre className="max-h-(--ui-picker-list-height) overflow-y-auto font-mail text-13 whitespace-pre-wrap text-secondary">{draft.quoteText}</pre>
          ) : (
            <p className="text-13 text-secondary">{t("composer.quote.formattedOnly")}</p>
          )}
        </div>
      </div>
    ) : null;
  const writingFooter =
    signaturePreview !== undefined || (quoteInWriting && quoteBlock !== null) ? (
      <>
        {signaturePreview}
        {quoteInWriting && quoteBlock}
      </>
    ) : undefined;

  // ---- The "…" menu: every option that has no place of its own (stage 23, decision 0086).
  const optionSections: MenuSection[] = [];
  if (pool.length > 0) {
    optionSections.push({
      id: "signature",
      label: t("writing.signature.label"),
      entries: [
        {
          kind: "radio",
          id: SIGNATURE_DEFAULT,
          label: defaultSignature ? t("writing.signature.defaultNamed", { name: defaultSignature.name }) : t("writing.signature.defaultNone"),
          checked: signatureChoice === SIGNATURE_DEFAULT,
          onSelect: () => {
            chooseSignature(SIGNATURE_DEFAULT);
          },
          "data-testid": "composer-signature-default",
        },
        ...pool.map(
          (s): MenuEntry => ({
            kind: "radio",
            id: s.id,
            label: s.name,
            checked: signatureChoice === s.id,
            onSelect: () => {
              chooseSignature(s.id);
            },
            "data-testid": `composer-signature-${s.id}`,
          }),
        ),
        ...(fields.signatureId && !pool.some((s) => s.id === fields.signatureId)
          ? [
              {
                kind: "radio",
                id: fields.signatureId,
                label: t("writing.signature.deleted"),
                checked: signatureChoice === fields.signatureId,
                onSelect: () => undefined,
                "data-testid": "composer-signature-deleted",
              } satisfies MenuEntry,
            ]
          : []),
        {
          kind: "radio",
          id: SIGNATURE_NONE,
          label: t("writing.signature.none"),
          checked: signatureChoice === SIGNATURE_NONE,
          onSelect: () => {
            chooseSignature(SIGNATURE_NONE);
          },
          "data-testid": "composer-signature-none",
        },
      ],
    });
  } else if (signature) {
    optionSections.push({
      id: "signature",
      entries: [
        {
          kind: "checkbox",
          id: "include-signature",
          label: t("composer.signature.include"),
          checked: fields.includeSignature,
          onSelect: () => {
            change({ includeSignature: !latest.current.includeSignature });
          },
          "data-testid": "composer-include-signature",
        },
      ],
    });
  }
  if (hasQuote) {
    optionSections.push({
      id: "quote",
      entries: [
        {
          kind: "checkbox",
          id: "include-quote",
          label: t(kind === "forward" ? "composer.quote.includeForward" : "composer.quote.include"),
          checked: fields.includeQuote,
          onSelect: () => {
            change({ includeQuote: !latest.current.includeQuote });
          },
          "data-testid": "composer-include-quote",
        },
      ],
    });
    if (fields.includeQuote) {
      optionSections.push({
        id: "quote-position",
        label: t("composer.quote.position"),
        entries: QUOTE_POSITIONS.map(
          (position): MenuEntry => ({
            kind: "radio",
            id: position,
            label: t(`composer.quote.${position}`),
            checked: fields.quotePosition === position,
            onSelect: () => {
              change({ quotePosition: position });
            },
            "data-testid": `composer-quote-${position}`,
          }),
        ),
      });
    }
  }
  optionSections.push({
    id: "send",
    label: t("composer.sendOptions.whenSending"),
    entries: [
      ...(canArchive
        ? [
            {
              kind: "checkbox",
              id: "archive-original",
              label: t("composer.sendOptions.archiveOriginal"),
              checked: sendOptions.archiveOriginal,
              onSelect: () => {
                void applySendOptions({ ...sendOptions, archiveOriginal: !sendOptions.archiveOriginal }).catch(() => undefined);
              },
              "data-testid": "composer-archive-original",
            } satisfies MenuEntry,
          ]
        : []),
      {
        kind: "checkbox",
        id: "read-receipt",
        label: t("composer.sendOptions.readReceipt"),
        hint: t("composer.sendOptions.readReceiptHint"),
        checked: sendOptions.requestReadReceipt,
        onSelect: () => {
          void applySendOptions({ ...sendOptions, requestReadReceipt: !sendOptions.requestReadReceipt }).catch(() => undefined);
        },
        "data-testid": "composer-read-receipt",
      },
    ],
  });
  optionSections.push({
    id: "format",
    label: t("composer.format.label"),
    entries: FORMATS.map(
      (format): MenuEntry => ({
        kind: "radio",
        id: format,
        label: t(`composer.format.${format}`),
        checked: fields.format === format,
        onSelect: () => {
          switchFormat(format);
        },
        "data-testid": `composer-format-${format}`,
      }),
    ),
  });

  // ---- The arrow of the send button: "Send later" with its suggestions, then "Send and archive".
  const sendSections = (openedAt: number): MenuSection[] => {
    const format = new Intl.DateTimeFormat(i18n.language, { weekday: "short", day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" });
    const sections: MenuSection[] = [
      {
        id: "later",
        label: t("composer.sendOptions.later"),
        entries: [
          ...sendLaterPresets(new Date(openedAt)).map(
            (preset): MenuEntry => ({
              kind: "action",
              id: preset.id,
              label: t(`sendLater.preset.${preset.id}`, { time: format.format(preset.at) }),
              icon: Clock,
              onSelect: () => {
                setLaterOpen(false);
                void scheduleAt(preset.at);
              },
              "data-testid": `send-later-preset-${preset.id}`,
            }),
          ),
          {
            kind: "action",
            id: "pick",
            label: t("composer.sendOptions.pick"),
            icon: CalendarClock,
            onSelect: () => {
              setLaterError(null);
              setLaterOpen(true);
            },
            "data-testid": "composer-send-later-pick",
          },
        ],
      },
    ];
    if (canArchive) {
      sections.push({
        id: "archive",
        entries: [
          {
            kind: "action",
            id: "sendAndArchive",
            label: t("composer.sendOptions.sendAndArchive"),
            icon: Archive,
            detail: archiveShortcut,
            keyshortcuts: ariaShortcut(SEND_AND_ARCHIVE_SHORTCUT, platform),
            onSelect: () => {
              void send(true);
            },
            "data-testid": "composer-send-and-archive",
          },
        ],
      });
    }
    return sections;
  };

  const busySending = sending || frozen || scheduling;
  return (
    <ComposerFrame
      title={title}
      heading={
        <input
          value={fields.subject}
          disabled={frozen}
          aria-label={t("composer.subject")}
          placeholder={t("composer.subject")}
          onChange={(e) => {
            change({ subject: e.target.value });
          }}
          onBlur={saveNow}
          className="h-8 min-w-0 flex-1 rounded-control bg-transparent text-20 font-bold text-primary placeholder:text-tertiary"
          data-testid="composer-subject"
        />
      }
      onClose={() => {
        void closeComposer();
      }}
      status={status}
      actions={<OpenComposerWindowButton disabled={frozen} busy={openingWindow} onOpen={() => void openInWindow()} />}
      onKeyDown={onKeyDown}
      dropping={dropping}
      minimised={minimised}
      onMinimisedChange={setMinimised}
    >
      <div className="flex min-h-0 flex-1 flex-col overflow-y-auto" data-testid="composer-body">
        {/* Stage 9: hints about the mailto: link this draft came from (blind copies first). */}
        {prefill && <MailtoNotices notices={prefill.notices} inReplyTo={prefill.inReplyTo !== null} />}
        {/* Audit 24.11.2026: another program's draft the app could not take over completely. */}
        {draft.originalKept && (
          <div className="px-6 pt-3">
            <Notice tone="info" title={t("composer.originalKept.title")} data-testid="composer-original-kept">
              <p>{t("composer.originalKept.body")}</p>
            </Notice>
          </div>
        )}
        {frozen && (
          <div className="px-6 py-3">
            <Notice tone="info" title={t("composer.frozen.title")} data-testid="composer-frozen">
              <p>{t("composer.frozen.body")}</p>
              <div>
                <Button
                  variant="secondary"
                  onClick={() => {
                    void unfreeze();
                  }}
                  data-testid="composer-unfreeze"
                >
                  {t("composer.frozen.edit")}
                </Button>
              </div>
            </Notice>
          </div>
        )}
        <RecipientField
          look="card"
          label={t("composer.recipients.to")}
          addresses={fields.to}
          onChange={(to) => {
            change({ to: recipients(to) });
          }}
          onBlur={saveNow}
          refused={refused}
          autoFocus={kind === "new" || kind === "forward"}
          disabled={frozen}
          onGroup={(members) => {
            addGroup("to", members);
          }}
          trailing={
            <>
              {!showCc && (
                <button
                  type="button"
                  onClick={() => {
                    setShowCc(true);
                  }}
                  aria-label={t("composer.recipients.showCc")}
                  className={FIELD_LINK}
                  data-testid="composer-show-cc"
                >
                  {t("composer.recipients.cc")}
                </button>
              )}
              {!showBcc && (
                <button
                  type="button"
                  onClick={() => {
                    setShowBcc(true);
                  }}
                  aria-label={t("composer.recipients.showBcc")}
                  className={FIELD_LINK}
                  data-testid="composer-show-bcc"
                >
                  {t("composer.recipients.bcc")}
                </button>
              )}
            </>
          }
          data-testid="composer-to"
        />
        {showCc && (
          <RecipientField
            look="card"
            label={t("composer.recipients.cc")}
            addresses={fields.cc}
            onChange={(cc) => {
              change({ cc: recipients(cc) });
            }}
            onBlur={saveNow}
            refused={refused}
            autoFocus={initial.cc.length === 0}
            disabled={frozen}
            onGroup={(members) => {
              addGroup("cc", members);
            }}
            data-testid="composer-cc"
          />
        )}
        {showBcc && (
          <RecipientField
            look="card"
            label={t("composer.recipients.bcc")}
            addresses={fields.bcc}
            onChange={(bcc) => {
              change({ bcc: recipients(bcc) });
            }}
            onBlur={saveNow}
            refused={refused}
            autoFocus={initial.bcc.length === 0}
            disabled={frozen}
            onGroup={(members) => {
              addGroup("bcc", members);
            }}
            data-testid="composer-bcc"
          />
        )}
        {groupHint && fields.to.length + fields.cc.length >= MANY_RECIPIENTS && (
          <div className="border-b border-border-subtle px-6 py-2" data-testid="composer-bcc-hint">
            <Notice tone="info" title={t("composer.recipients.manyTitle", { count: fields.to.length + fields.cc.length })}>
              <p>{t("composer.recipients.manyBody")}</p>
              <div className="flex gap-2">
                <Button
                  variant="secondary"
                  disabled={frozen}
                  onClick={() => {
                    const { field, members } = groupHint;
                    const moved = (list: readonly Address[]) => list.filter((a) => !members.some((m) => sameAddress(a, m)));
                    change(field === "to" ? { to: moved(fields.to), bcc: addAddresses(fields.bcc, members) } : { cc: moved(fields.cc), bcc: addAddresses(fields.bcc, members) });
                    setShowBcc(true);
                    setGroupHint(null);
                  }}
                  data-testid="composer-move-to-bcc"
                >
                  {t("composer.recipients.moveToBcc")}
                </Button>
                <Button
                  variant="quiet"
                  onClick={() => {
                    setGroupHint(null);
                  }}
                  data-testid="composer-keep-recipients"
                >
                  {t("composer.recipients.keepRecipients")}
                </Button>
              </div>
            </Notice>
          </div>
        )}
        {/* Stage 23 (decision 0086): "From" as a chip with the account's colour and a menu. */}
        {identity && (
          <div className="flex items-center gap-2 border-b border-border-subtle px-6 py-1" data-testid="composer-from">
            <span className="flex h-8 w-12 shrink-0 items-center text-12 text-tertiary">
              {t("composer.from")}
            </span>
            {identityList.length > 1 ? (
              <ComposerMenu
                label={t("composer.from")}
                name={t("composer.identityMenu", { identity: identityText(identity) })}
                tooltip={false}
                disabled={frozen}
                sections={identitySections}
                className={(open) => cx(CHIP_BUTTON, open && OPEN)}
                data-testid="composer-identity"
              >
                <IdentityChip identity={identityText(identity)} color={account?.color} />
                <ChevronDown aria-hidden className="size-(--ui-icon-small) shrink-0 text-secondary" strokeWidth={1.75} />
              </ComposerMenu>
            ) : (
              <p className="flex h-6 min-w-0 items-center gap-2 rounded-full bg-badge px-3 text-13 text-primary" data-testid="composer-identity-single">
                <IdentityChip identity={identityText(identity)} color={account?.color} />
              </p>
            )}
          </div>
        )}
        {/* Stage 8: without the identities there is no From line; say why, at its place. */}
        {identities.isError && (
          <div className="border-b border-border-subtle px-6 py-2">
            <ErrorNotice
              context={t("presentation.composer.identitiesFailed")}
              error={identities.error}
              onRetry={() => {
                void identities.refetch();
              }}
              data-testid="composer-identity-error"
            />
          </div>
        )}
        {templateNote && (
          <div className="px-6 pt-3">
            <Notice
              tone={templateNote.problems.length > 0 ? "warning" : "info"}
              title={t("writing.templates.inserted", { name: templateNote.name })}
              data-testid="composer-template-note"
            >
              {templateNote.recipientsAdded > 0 && (
                <p data-testid="composer-template-recipients">
                  {t("writing.templates.recipientsAdded", { count: templateNote.recipientsAdded })}
                </p>
              )}
              {templateNote.problems.map((problem) => (
                <p key={problem.kind === "unknown" ? `u:${problem.name}` : `m:${problem.placeholder}`}>
                  {problem.kind === "unknown"
                    ? t("writing.templates.unknown", { name: `{{${problem.name}}}` })
                    : t("writing.templates.missing", { placeholder: `{{${problem.placeholder}}}` })}
                </p>
              ))}
              <div>
                <Button
                  variant="quiet"
                  onClick={() => {
                    setTemplateNote(null);
                  }}
                  data-testid="composer-template-note-dismiss"
                >
                  {t("writing.paste.dismiss")}
                </Button>
              </div>
            </Notice>
          </div>
        )}
        {pasteNote && (
          <PasteNote
            result={pasteNote}
            onDismiss={() => {
              setPasteNote(null);
            }}
          />
        )}
        {/* `quotePosition` is where the answer goes (core: `QuotePosition`): "below" puts the quote first. */}
        {fields.quotePosition === "below" && quoteBlock && <div className="pt-3">{quoteBlock}</div>}
        {html ? (
          <Suspense
            fallback={
              <p role="status" className="px-6 py-3 text-13 text-secondary">
                {t("composer.loading")}
              </p>
            }
          >
            <RichEditor
              key={editorKey}
              initialHtml={fields.bodyHtml}
              onChange={(bodyHtml) => {
                change({ bodyHtml });
              }}
              onBlur={saveNow}
              label={t("composer.body")}
              disabled={frozen}
              autoFocus={editorKey === 0 && (kind === "reply" || kind === "reply_all")}
              imageSources={imageSources}
              onInsertImage={() => {
                void pickImages();
              }}
              handleRef={editorHandle}
              onPasteResult={(result) => {
                const notable = result.kind !== "cleaned" || result.imagesRemoved > 0 || result.formattingSimplified;
                setPasteNote(notable ? result : null);
              }}
              footer={writingFooter}
              formatBarSlot={formatBarSlot}
              onPlainText={() => {
                switchFormat("text");
              }}
            />
          </Suspense>
        ) : (
          <>
            <label htmlFor={textBodyId} className="sr-only">
              {t("composer.body")}
            </label>
            <textarea
              id={textBodyId}
              ref={textBody}
              value={fields.bodyText}
              disabled={frozen}
              autoFocus={kind === "reply" || kind === "reply_all"}
              onChange={(e) => {
                change({ bodyText: e.target.value });
              }}
              onBlur={saveNow}
              className="mx-4 mb-2 min-h-(--ui-picker-list-height) flex-1 resize-none rounded-card bg-transparent px-2 py-3 font-mail text-14 text-primary focus-visible:-outline-offset-2"
              data-testid="composer-text-body"
            />
          </>
        )}
        {/* Stage 13: AI writing help on the selection (only while it is set up). */}
        <WritingHelp disabled={frozen} />
        {signature && fields.includeSignature && !richSignature && (
          <pre aria-label={t("composer.signature.preview")} className="mx-6 mb-3 font-mail text-13 whitespace-pre-wrap text-secondary" data-testid="composer-signature">
            {signatureText}
          </pre>
        )}
        {fields.quotePosition === "above" && !quoteInWriting && quoteBlock}
        {/* Stage 8: the signature choice is missing while the signatures cannot be loaded. */}
        {signatures.isError && (
          <div className="px-6 pb-3">
            <ErrorNotice
              context={t("presentation.composer.signaturesFailed")}
              error={signatures.error}
              onRetry={() => {
                void signatures.refetch();
              }}
              data-testid="composer-signatures-error"
            />
          </div>
        )}
      </div>
      <div className="flex shrink-0 flex-col gap-3 px-6 pt-3 pb-4" data-testid="composer-foot">
        {addingImages && (
          <p role="status" className="text-12 text-secondary" data-testid="composer-images-adding">
            {t("writing.images.adding")}
          </p>
        )}
        {sendError && (
          <ErrorNotice
            context={t("composer.errors.send")}
            error={sendError}
            actions={
              sendError.code === "read_only" ? (
                <Button
                  variant="secondary"
                  onClick={() => {
                    showAccountsPage({ kind: "account", accountId: draft.accountId });
                  }}
                  data-testid="composer-allow-changes"
                >
                  {t("actions.readOnly.open")}
                </Button>
              ) : undefined
            }
            data-testid="composer-send-error"
          />
        )}
        {saveError && !sendError && (
          <ErrorNotice
            context={t("composer.errors.save")}
            error={saveError}
            onRetry={() => {
              dirty.current = true;
              void flush().catch(() => undefined);
            }}
            data-testid="composer-save-error"
          />
        )}
        {actionError && <ErrorNotice context={t(ACTION_ERROR_TEXT[actionError.context])} error={actionError.error} data-testid="composer-action-error" />}
        {sizeCheck?.exceedsServerLimit && sizeCheck.serverLimit !== null && (
          <Notice
            tone="warning"
            title={t("writing.size.exceedsTitle", {
              size: formatSize(sizeCheck.size, i18n.language),
              limit: formatSize(sizeCheck.serverLimit, i18n.language),
            })}
            data-testid="composer-size-warning"
          >
            <p>{t("writing.size.exceedsHint")}</p>
          </Notice>
        )}
        {sizeCheck && !sizeCheck.exceedsServerLimit && sizeCheck.large && (
          <p className="text-12 text-secondary" data-testid="composer-size-large">
            {t("writing.size.large")}
          </p>
        )}
        {/* A time chosen in the send button's menu that the core refused (in the picker it shows there). */}
        {laterError && !laterOpen && <ErrorNotice context={t("sendLater.failed")} error={laterError} data-testid="send-later-error" />}
        {laterOpen && (
          <SendLaterPicker
            confirmLabel={t("writing.sendLater.confirm")}
            busy={scheduling}
            error={laterError}
            onChoose={(sendAt) => {
              void scheduleAt(sendAt);
            }}
            onCancel={() => {
              setLaterOpen(false);
              setLaterError(null);
              sendArrow.current?.querySelector("button")?.focus();
            }}
            data-testid="composer-send-later-picker"
          />
        )}
        {/* Stage 23 (decisions 0082, 0086): the hint about dropping only while files are dragged. */}
        {dropping && (
          <div
            className="flex shrink-0 flex-wrap items-center gap-2 rounded-card border border-dashed border-accent bg-accent-soft px-3 py-2 text-13 text-primary"
            data-testid="composer-drop-hint"
          >
            <Upload aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
            {t("composer.attachments.dropHere")}
            {html && <span className="text-secondary">{t("writing.images.dropHint")}</span>}
          </div>
        )}
        <ComposerAttachments
          attachments={draft.attachments.filter((a) => a.contentId === null)}
          busy={attaching}
          disabled={frozen}
          onRemove={(attachment) => {
            void removeAttachment(attachment);
          }}
        />
        {/* The compact formatting bar of the editor (rich text only) is drawn in here. */}
        {html && <div ref={setFormatBarSlot} className="flex min-w-0 empty:hidden" />}
        <div className="relative flex flex-wrap items-center gap-1" data-testid="composer-actions">
          <div className="me-2 flex rounded-control bg-accent-fill text-on-accent">
            <Tooltip label={t("composer.sendWithShortcut", { shortcut: sendShortcut })}>
              <button
                type="button"
                onClick={() => {
                  void send();
                }}
                disabled={busySending}
                aria-label={t("composer.sendWithShortcut", { shortcut: sendShortcut })}
                aria-keyshortcuts={ariaShortcut({ key: "Enter", mod: true }, platform)}
                className="flex h-8 items-center rounded-s-control px-4 text-13 font-semibold hover:bg-accent-fill/90 disabled:opacity-50"
                data-testid="composer-send"
              >
                {sending ? t("composer.sendingButton") : t("composer.sendButton")}
              </button>
            </Tooltip>
            <div ref={sendArrow} className="flex border-s border-on-accent/30">
              <ComposerMenu
                label={t("composer.sendOptions.menu")}
                sections={sendSections}
                note={t("sendLater.hint")}
                disabled={busySending}
                className={(open) => cx(SEND_ARROW, open && SEND_ARROW_OPEN)}
                data-testid="composer-send-later"
              >
                <ChevronDown aria-hidden className="size-(--ui-icon-small)" strokeWidth={2} />
              </ComposerMenu>
            </div>
          </div>
          <Tooltip label={t("composer.attachments.add")}>
            <button
              type="button"
              aria-label={t("composer.attachments.add")}
              aria-busy={attaching || undefined}
              disabled={frozen || attaching}
              onClick={() => {
                void pick();
              }}
              className={ROW_BUTTON}
              data-testid="composer-attach"
            >
              <Paperclip aria-hidden className="size-(--ui-icon)" strokeWidth={1.75} />
            </button>
          </Tooltip>
          {html && (
            <Tooltip label={t("writing.images.insert")}>
              <button
                type="button"
                aria-label={t("writing.images.insert")}
                aria-busy={addingImages || undefined}
                disabled={frozen || addingImages}
                onClick={() => {
                  void pickImages();
                }}
                className={ROW_BUTTON}
                data-testid="composer-insert-image"
              >
                <ImagePlus aria-hidden className="size-(--ui-icon)" strokeWidth={1.75} />
              </button>
            </Tooltip>
          )}
          <div className="relative flex">
            <Tooltip label={t("writing.templates.insert")} disabled={templateOpen}>
              <button
                type="button"
                aria-label={t("writing.templates.insert")}
                aria-expanded={templateOpen}
                aria-controls={templateOpen ? templateMenuId : undefined}
                disabled={frozen}
                onClick={() => {
                  setTemplateOpen((open) => !open);
                }}
                className={cx(ROW_BUTTON, templateOpen && "bg-hover text-primary")}
                data-testid="composer-template"
              >
                <FileText aria-hidden className="size-(--ui-icon)" strokeWidth={1.75} />
              </button>
            </Tooltip>
            {templateOpen && (
              <TemplateMenu
                id={templateMenuId}
                templates={templates}
                onChoose={(templateId) => {
                  void insertTemplate(templateId);
                }}
                onClose={() => {
                  setTemplateOpen(false);
                }}
              />
            )}
          </div>
          {/* Stage 20 (decision 0059): "Remind me if no reply by …". */}
          <FollowUpOption
            value={sendOptions.followUpAt ?? null}
            disabled={frozen}
            onChange={(followUpAt) => {
              void applySendOptions({ ...sendOptions, followUpAt }).catch(() => undefined);
            }}
            className={(open) => cx(ROW_BUTTON, ACTIVE_ROW_BUTTON, open && OPEN)}
          />
          <ComposerMenu
            label={t("composer.sendOptions.options")}
            sections={optionSections}
            disabled={frozen}
            className={(open) => cx(ROW_BUTTON, open && OPEN)}
            data-testid="composer-options"
          >
            <MoreHorizontal aria-hidden className="size-(--ui-icon)" strokeWidth={1.75} />
          </ComposerMenu>
          <span className="flex-1" />
          <Tooltip label={t("composer.discard.label")}>
            <button
              type="button"
              aria-label={t("composer.discard.label")}
              disabled={frozen}
              onClick={() => {
                setConfirmDiscard(true);
              }}
              className={cx(ROW_BUTTON, "text-tertiary")}
              data-testid="composer-discard"
            >
              <Trash2 aria-hidden className="size-(--ui-icon)" strokeWidth={1.75} />
            </button>
          </Tooltip>
        </div>
      </div>
      {attachmentAsk && (
        <AttachmentReminder
          word={attachmentAsk.word}
          onSendAnyway={() => {
            const { proceed } = attachmentAsk;
            setAttachmentAsk(null);
            proceed();
          }}
          onAddAttachment={() => {
            setAttachmentAsk(null);
            void pick();
          }}
          onCancel={() => {
            setAttachmentAsk(null);
          }}
        />
      )}
      {confirmDiscard && (
        <ConfirmDialog
          title={t("composer.discard.title")}
          body={t("composer.discard.body")}
          confirmLabel={t("composer.discard.confirm")}
          cancelLabel={t("composer.discard.cancel")}
          onConfirm={() => {
            void discard();
          }}
          onCancel={() => {
            setConfirmDiscard(false);
          }}
          data-testid="composer-confirm-discard"
        />
      )}
    </ComposerFrame>
  );
}

/** Buttons of the send row: 32 px icons, quiet until hovered; a menu button while its menu shows. */
const ROW_BUTTON = "flex size-8 items-center justify-center rounded-control text-secondary transition-colors hover:bg-hover hover:text-primary disabled:opacity-50";
const OPEN = "bg-hover text-primary";
/** The reminder's icon: in the accent while a reminder is set (with a dot for its place). */
const ACTIVE_ROW_BUTTON = "relative data-active:text-accent";
/** The arrow of the send button ("Send later", "Send and archive"). */
const SEND_ARROW = "flex size-8 items-center justify-center rounded-e-control hover:bg-accent-fill/90 disabled:opacity-50";
const SEND_ARROW_OPEN = "bg-accent-fill/90";
/** The "From" chip with its menu. */
const CHIP_BUTTON = "flex h-6 max-w-full min-w-0 items-center gap-2 rounded-full bg-badge ps-3 pe-2 text-13 text-primary hover:bg-hover";

/** "Cc" and "Bcc" at the end of the To row: small text links that show their rows. */
const FIELD_LINK = "flex h-8 items-center rounded-control px-1 text-12 text-accent hover:underline";

/** The sending identity in its chip: a square in the account's colour, then "name · address". */
function IdentityChip({ identity, color }: { identity: string; color: AccountColor | undefined }) {
  return (
    <>
      {color !== undefined && <span aria-hidden className={cx("size-(--ui-dot) shrink-0 rounded-(--ui-composer-swatch-radius)", accountFill(color))} data-testid="composer-identity-color" />}
      <span className="min-w-0 truncate">{identity}</span>
    </>
  );
}

interface TemplateMenuProps {
  id: string;
  templates: ReturnType<typeof useTemplates>;
  onChoose: (templateId: TemplateId) => void;
  onClose: () => void;
}

/**
 * The composer's template list. Stage 23 (decision 0086): a panel above the template icon of the
 * send row (inline, no popover library: CSP); Escape or a press outside close it.
 */
function TemplateMenu({ id, templates, onChoose, onClose }: TemplateMenuProps) {
  const { t } = useTranslation();
  const showSettings = useUiStore((state) => state.showSettings);
  const list = templates.data ?? [];
  const panel = useRef<HTMLDivElement>(null);
  const close = useRef(onClose);
  useEffect(() => {
    close.current = onClose;
  });
  useEffect(() => {
    const onPointerDown = (event: PointerEvent) => {
      const target = event.target instanceof Node ? event.target : null;
      // The template button toggles the panel itself.
      if (target && (panel.current?.contains(target) || panel.current?.parentElement?.contains(target))) return;
      close.current();
    };
    document.addEventListener("pointerdown", onPointerDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
    };
  }, []);
  return (
    <div
      ref={panel}
      id={id}
      role="group"
      aria-label={t("writing.templates.menu")}
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          // Closes the list, not the composer.
          event.preventDefault();
          onClose();
        }
      }}
      className="absolute start-0 bottom-full z-30 mb-2 flex max-h-(--ui-picker-list-height) w-(--ui-picker-width) max-w-[calc(100vw-var(--ui-space-4))] flex-col gap-1 overflow-y-auto rounded-card border border-border bg-surface-raised p-2 shadow-floating"
      data-testid="composer-template-menu"
    >
      {templates.isError ? (
        <ErrorNotice context={t("writing.templates.loadFailed")} error={templates.error} onRetry={() => void templates.refetch()} />
      ) : templates.isPending ? (
        <p role="status" className="px-2 py-1 text-13 text-secondary">
          {t("writing.templates.loading")}
        </p>
      ) : list.length === 0 ? (
        <p className="px-2 py-1 text-13 text-secondary">{t("writing.templates.empty")}</p>
      ) : (
        <ul className="flex flex-col">
          {list.map((template, index) => (
            <li key={template.id}>
              <button
                type="button"
                autoFocus={index === 0}
                onClick={() => {
                  onChoose(template.id);
                }}
                className="flex w-full flex-col items-start rounded-control px-2 py-1 text-start hover:bg-hover"
                data-testid="composer-template-option"
              >
                <span className="text-13 font-medium text-primary">{template.name}</span>
                {template.subject && <span className="truncate text-12 text-secondary">{template.subject}</span>}
              </button>
            </li>
          ))}
        </ul>
      )}
      <div>
        <Button
          variant="quiet"
          onClick={() => {
            showSettings("compose");
          }}
          data-testid="composer-template-manage"
        >
          {t("writing.templates.manage")}
        </Button>
      </div>
    </div>
  );
}

/** What pasting did, when worth saying: images removed, formatting simplified, plain text, failure. */
function PasteNote({ result, onDismiss }: { result: PasteResult; onDismiss: () => void }) {
  const { t } = useTranslation();
  if (result.kind === "failed") {
    return (
      <div className="px-4 pt-2">
        <ErrorNotice context={t("writing.paste.failed")} error={result.error} data-testid="composer-paste-error" />
      </div>
    );
  }
  const lines =
    result.kind === "plain"
      ? [t("writing.paste.plain")]
      : [
          ...(result.imagesRemoved > 0 ? [t("writing.paste.imagesRemoved")] : []),
          ...(result.formattingSimplified ? [t("writing.paste.simplified")] : []),
        ];
  return (
    <div className="px-4 pt-2">
      <Notice tone="info" title={lines[0] ?? ""} data-testid="composer-paste-note">
        {lines.slice(1).map((line) => (
          <p key={line}>{line}</p>
        ))}
        <div>
          <Button variant="quiet" onClick={onDismiss} data-testid="composer-paste-note-dismiss">
            {t("writing.paste.dismiss")}
          </Button>
        </div>
      </Notice>
    </div>
  );
}
