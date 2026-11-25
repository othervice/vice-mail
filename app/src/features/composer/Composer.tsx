import { Archive, FileText, Send, Trash2, Upload, X } from "lucide-react";
import { lazy, Suspense, useEffect, useId, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import { cx } from "../../components/cx";
import { ErrorNotice } from "../../components/ErrorNotice";
import { Notice } from "../../components/Notice";
import { Tooltip } from "../../components/Tooltip";
import {
  composeIpc,
  composerIpc,
  parityIpc,
  reminderIpc,
  snippetsIpc,
  toCoreError,
  type Address,
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
import { SendLaterPicker } from "./SendLaterPicker";
import { AttachmentReminder } from "./AttachmentReminder";
import { FollowUpOption } from "./FollowUpOption";
import { useUiStore } from "../../stores/ui";
import { useAppInfo } from "../app-info/queries";
import { formatShortcut, shortcutPlatform } from "../actions/shortcuts";
import { formatSize, htmlToText, invalidAddressIn, textToHtml } from "./addresses";
import { ComposerAttachments } from "./ComposerAttachments";
import { MailtoNotices } from "../extras/MailtoNotices";
import { useComposeIdentities } from "./queries";
import { FIELD_ROW_FOCUS, RecipientField } from "./RecipientField";
// Stage 19 (decision 0056): a group with many members suggests Bcc.
import { MANY_RECIPIENTS } from "../../services/ipc";
import { addAddresses, sameAddress } from "./addresses";
import { SignatureHtml } from "./SignaturePreview";
import { WritingHelp } from "../ai/WritingHelp";
// Stage 16: "Send and archive" in the menu next to the send button.
import { OverflowMenu, type OverflowItem } from "../actions/OverflowMenu";
import { ariaShortcut } from "../actions/shortcuts";

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

/** The open composer, if any: one at a time, above the reading pane (docs/DESIGN.md). */
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
  return (
    <ComposerFrame title={t(`composer.title.${loadingTitle(source.kind)}`)} onClose={close}>
      {error ? (
        <div className="p-4">
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
        <p role="status" className="p-4 text-13 text-secondary" data-testid="composer-loading">
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

interface ComposerFrameProps {
  title: string;
  onClose: () => void;
  status?: ReactNode;
  actions?: ReactNode;
  children: ReactNode;
  onKeyDown?: (event: KeyboardEvent<HTMLElement>) => void;
  dropping?: boolean;
  /** A second line of the drop hint (rich text: images go into the text). */
  dropDetail?: string | undefined;
}

/** The panel: over the reading pane, at the right of the window. */
function ComposerFrame({ title, onClose, status, actions, children, onKeyDown, dropping = false, dropDetail }: ComposerFrameProps) {
  const { t } = useTranslation();
  const titleId = useId();
  const platform = shortcutPlatform(useAppInfo().data?.platform);
  const inWindow = useComposerInWindow();
  return (
    <section
      aria-labelledby={titleId}
      onKeyDown={(event) => {
        // Keys of a dialog this composer opened arrive through the portal; they are the dialog's.
        if (!(event.target instanceof Node) || !event.currentTarget.contains(event.target)) return;
        onKeyDown?.(event);
        if (!event.defaultPrevented && event.key === "Escape") {
          event.preventDefault();
          onClose();
        }
        // The list's single-key shortcuts (E, #, …) must not act on mail behind the composer. The
        // command palette opens from here too (stage 10: templates are inserted through it).
        const palette = boundShortcuts("palette", platform).some((s) => matchesShortcut(event, s, platform));
        if (!palette) event.stopPropagation();
      }}
      className={cx(
        inWindow
          ? "absolute inset-0 z-20 flex flex-col bg-surface"
          : "absolute inset-y-0 end-0 z-20 flex w-full max-w-(--ui-reading-max-width) flex-col border-s border-border bg-surface shadow-panel",
        dropping && "outline-2 -outline-offset-2 outline-focus-ring",
      )}
      data-testid="composer"
    >
      <header className="flex h-(--ui-header-height) shrink-0 items-center gap-3 border-b border-border-subtle px-4">
        <h2 id={titleId} className="min-w-0 flex-1 truncate text-16 font-semibold text-primary">
          {title}
        </h2>
        {status}
        {actions}
        <Tooltip label={t("composer.close")}>
          <button
            type="button"
            aria-label={t("composer.close")}
            onClick={onClose}
            className="flex size-8 items-center justify-center rounded-control text-secondary hover:bg-hover hover:text-primary"
            data-testid="composer-close"
          >
            <X aria-hidden className="size-(--ui-icon)" strokeWidth={1.75} />
          </button>
        </Tooltip>
      </header>
      {dropping && (
        <div className="flex shrink-0 items-center gap-2 bg-accent-soft px-4 py-2 text-13 text-primary" data-testid="composer-drop-hint">
          <Upload aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
          {t("composer.attachments.dropHere")}
          {dropDetail && <span className="text-secondary">{dropDetail}</span>}
        </div>
      )}
      {children}
    </section>
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
  const [dropping, setDropping] = useState(false);
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
  const templateMenuId = useId();
  const signatureId = useId();
  const identityId = useId();
  const subjectId = useId();
  const textBodyId = useId();
  const quoteId = useId();

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

  // Files dropped on the window while the composer is open become attachments (paths from the shell).
  useEffect(() => {
    let active = true;
    let stop: (() => void) | undefined;
    void composeIpc
      .onFileDrop((event) => {
        if (event.type === "enter") setDropping(true);
        else if (event.type === "leave") setDropping(false);
        else if (event.type === "drop") {
          setDropping(false);
          if (event.paths.length === 0) return;
          // Rich text: images go into the text (stage 10), everything else is attached.
          const inline = latest.current.format === "html" ? event.paths.filter(isImagePath) : [];
          const files = event.paths.filter((path) => !inline.includes(path));
          void (async () => {
            if (inline.length > 0) {
              setAddingImages(true);
              try {
                applyAdded(await composerIpc.addDroppedInlineImages(draftId, inline));
              } catch (e) {
                setActionError({ context: "images", error: toCoreError(e) });
              } finally {
                setAddingImages(false);
              }
            }
            if (files.length > 0) {
              setAttaching(true);
              try {
                const saved = await composeIpc.addDroppedAttachments(draftId, files);
                setDraft((d) => ({ ...d, attachments: saved.attachments, server: saved.server, state: saved.state }));
                checkSize();
              } catch (e) {
                setActionError({ context: "attach", error: toCoreError(e) });
              } finally {
                setAttaching(false);
              }
            }
          })();
        }
      })
      .then((unsubscribe) => {
        if (active) stop = unsubscribe;
        else unsubscribe();
      });
    return () => {
      active = false;
      stop?.();
    };
    // `applyAdded` and `checkSize` read refs and stable setters only.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [draftId]);

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
  };
  // The separator the core adds when it puts the message together (plain text, not translated).
  const signatureText = signature ? `-- \n${signature}` : "";
  // Rich text: the signature with its formatting, right below the text inside the writing area;
  // plain text (and the identity's text signature): as text under the text field.
  const richSignature = fields.format === "html" && chosenSignature !== null && signature !== null && fields.includeSignature;
  const signaturePreview = richSignature ? (
    <div aria-label={t("composer.signature.preview")} className="px-2 pt-1 pb-3 text-13 text-secondary" data-testid="composer-signature">
      <p className="whitespace-pre">{"-- "}</p>
      <SignatureHtml html={chosenSignature.html} />
    </div>
  ) : undefined;
  const hasQuote = draft.quoteHtml !== null || draft.quoteText !== null;
  const refused = sendError?.code === "invalid_address" ? invalidAddressIn(sendError.detail, [fields.to, fields.cc, fields.bcc]) : null;
  const sendShortcut = formatShortcut({ key: "Enter", mod: true }, platform);
  const archiveShortcut = formatShortcut(SEND_AND_ARCHIVE_SHORTCUT, platform);
  // Stage 16: the menu next to the send button.
  const sendMenu: OverflowItem[] = [
    {
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
  ];
  const title = fields.subject.trim() || t(`composer.title.${kind}`);

  const status = (
    <p aria-live="polite" className="shrink-0 text-12 text-tertiary" data-testid="composer-save-state" data-state={saveState}>
      {saveState === "saving"
        ? t("composer.saveState.saving")
        : saveState === "error"
          ? t("composer.saveState.error")
          : saveState === "saved"
            ? t(`composer.saveState.server.${draft.server}`)
            : ""}
    </p>
  );
  const discardButton = (
    <Tooltip label={t("composer.discard.label")}>
      <button
        type="button"
        aria-label={t("composer.discard.label")}
        disabled={frozen}
        onClick={() => {
          setConfirmDiscard(true);
        }}
        className="flex size-8 items-center justify-center rounded-control text-secondary hover:bg-hover hover:text-primary disabled:opacity-50"
        data-testid="composer-discard"
      >
        <Trash2 aria-hidden className="size-(--ui-icon)" strokeWidth={1.75} />
      </button>
    </Tooltip>
  );

  const recipients = (list: readonly Address[]) => [...list];
  // In rich text with the answer above the original, the original follows the signature inside
  // the writing area (one scrolling document); otherwise it stands as its own block.
  const quoteInWriting = fields.format === "html" && fields.quotePosition === "above";
  const quoteBlock =
    hasQuote && fields.includeQuote ? (
      <div className={cx("mb-3 rounded-card border border-border-subtle bg-canvas p-3", quoteInWriting ? "mx-2" : "mx-4")} data-testid="composer-quote">
        {draft.quoteText !== null ? (
          <pre className="max-h-(--ui-picker-list-height) overflow-y-auto font-sans text-13 whitespace-pre-wrap text-secondary">
            {draft.quoteText}
          </pre>
        ) : (
          <p className="text-13 text-secondary">{t("composer.quote.formattedOnly")}</p>
        )}
      </div>
    ) : null;
  const writingFooter =
    signaturePreview !== undefined || (quoteInWriting && quoteBlock !== null) ? (
      <>
        {signaturePreview}
        {quoteInWriting && quoteBlock}
      </>
    ) : undefined;

  return (
    <ComposerFrame
      title={title}
      onClose={() => {
        void closeComposer();
      }}
      status={status}
      actions={
        <>
          <OpenComposerWindowButton disabled={frozen} busy={openingWindow} onOpen={() => void openInWindow()} />
          {discardButton}
        </>
      }
      onKeyDown={onKeyDown}
      dropping={dropping}
      dropDetail={fields.format === "html" ? t("writing.images.dropHint") : undefined}
    >
      <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">
        {/* Stage 9: hints about the mailto: link this draft came from (blind copies first). */}
        {prefill && <MailtoNotices notices={prefill.notices} inReplyTo={prefill.inReplyTo !== null} />}
        {/* Audit 24.11.2026: another program's draft the app could not take over completely. */}
        {draft.originalKept && (
          <div className="px-4 pt-3">
            <Notice tone="info" title={t("composer.originalKept.title")} data-testid="composer-original-kept">
              <p>{t("composer.originalKept.body")}</p>
            </Notice>
          </div>
        )}
        {frozen && (
          <div className="p-4">
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
        {identityList.length > 1 && (
          <div className="flex items-center gap-2 border-b border-border-subtle px-4 py-1">
            <label htmlFor={identityId} className="w-12 shrink-0 text-13 text-tertiary">
              {t("composer.from")}
            </label>
            <select
              id={identityId}
              value={identity?.id ?? ""}
              disabled={frozen}
              onChange={(e) => {
                change({ identityId: Number(e.target.value) });
              }}
              onBlur={saveNow}
              className="h-8 min-w-0 flex-1 rounded-control bg-transparent text-13 text-primary"
              data-testid="composer-identity"
            >
              {identityList.map((i) => (
                <option key={i.id} value={i.id}>
                  {i.name ? t("composer.identity", { name: i.name, email: i.email }) : i.email}
                </option>
              ))}
            </select>
          </div>
        )}
        {/* Stage 8: without the identities there is no From line; say why, at its place. */}
        {identities.isError && (
          <div className="border-b border-border-subtle px-4 py-2">
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
        {identityList.length === 1 && identity && (
          <p className="flex items-center gap-2 border-b border-border-subtle px-4 py-2 text-13" data-testid="composer-identity-single">
            <span className="w-12 shrink-0 text-tertiary">{t("composer.from")}</span>
            <span className="truncate text-primary">
              {identity.name ? t("composer.identity", { name: identity.name, email: identity.email }) : identity.email}
            </span>
          </p>
        )}
        <RecipientField
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
                <Button
                  variant="quiet"
                  onClick={() => {
                    setShowCc(true);
                  }}
                  aria-label={t("composer.recipients.showCc")}
                  data-testid="composer-show-cc"
                >
                  {t("composer.recipients.cc")}
                </Button>
              )}
              {!showBcc && (
                <Button
                  variant="quiet"
                  onClick={() => {
                    setShowBcc(true);
                  }}
                  aria-label={t("composer.recipients.showBcc")}
                  data-testid="composer-show-bcc"
                >
                  {t("composer.recipients.bcc")}
                </Button>
              )}
            </>
          }
          data-testid="composer-to"
        />
        {showCc && (
          <RecipientField
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
          <div className="border-b border-border-subtle px-4 py-2" data-testid="composer-bcc-hint">
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
        <div className={cx("flex items-center gap-2 border-b border-border-subtle px-4 py-1", FIELD_ROW_FOCUS)}>
          <label htmlFor={subjectId} className="w-12 shrink-0 text-13 text-tertiary">
            {t("composer.subject")}
          </label>
          <input
            id={subjectId}
            value={fields.subject}
            disabled={frozen}
            onChange={(e) => {
              change({ subject: e.target.value });
            }}
            onBlur={saveNow}
            className="h-8 min-w-0 flex-1 bg-transparent text-14 text-primary outline-none"
            data-testid="composer-subject"
          />
        </div>
        <div
          role="group"
          aria-label={t("composer.format.label")}
          className="flex shrink-0 items-center gap-1 px-3 pt-2"
          data-testid="composer-format"
        >
          <span className="inline-flex gap-1 rounded-card bg-hover p-1">
            {FORMATS.map((format) => (
              <button
                key={format}
                type="button"
                aria-pressed={fields.format === format}
                disabled={frozen}
                onClick={() => {
                  switchFormat(format);
                }}
                className={cx(
                  "h-6 rounded-control px-2 text-12 font-medium transition-colors",
                  fields.format === format ? "bg-surface-raised text-primary shadow-row" : "text-secondary hover:text-primary",
                )}
                data-testid={`composer-format-${format}`}
              >
                {t(`composer.format.${format}`)}
              </button>
            ))}
          </span>
          <span className="flex-1" />
          <Button
            variant="quiet"
            disabled={frozen}
            aria-expanded={templateOpen}
            aria-controls={templateOpen ? templateMenuId : undefined}
            onClick={() => {
              setTemplateOpen((open) => !open);
            }}
            data-testid="composer-template"
          >
            <FileText aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
            {t("writing.templates.button")}
          </Button>
        </div>
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
        {templateNote && (
          <div className="px-4 pt-2">
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
        {pasteNote && <PasteNote result={pasteNote} onDismiss={() => {
          setPasteNote(null);
        }} />}
        {/* `quotePosition` is where the answer goes (core: `QuotePosition`): "below" puts the quote first. */}
        {fields.quotePosition === "below" && quoteBlock}
        {fields.format === "html" ? (
          <Suspense
            fallback={
              <p role="status" className="px-4 py-3 text-13 text-secondary">
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
                const notable =
                  result.kind !== "cleaned" || result.imagesRemoved > 0 || result.formattingSimplified;
                setPasteNote(notable ? result : null);
              }}
              footer={writingFooter}
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
              className="mx-2 mb-2 min-h-(--ui-picker-list-height) flex-1 resize-none rounded-card bg-transparent px-2 py-3 font-sans text-14 text-primary focus-visible:-outline-offset-2"
              data-testid="composer-text-body"
            />
          </>
        )}
        {/* Stage 13: AI writing help on the selection (only while it is set up). */}
        <WritingHelp disabled={frozen} />
        {signature && fields.includeSignature && !richSignature && (
          <pre
            aria-label={t("composer.signature.preview")}
            className="mx-4 mb-3 font-sans text-13 whitespace-pre-wrap text-secondary"
            data-testid="composer-signature"
          >
            {signatureText}
          </pre>
        )}
        {fields.quotePosition === "above" && !quoteInWriting && quoteBlock}
        {/* Stage 8: the signature choice is missing while the signatures cannot be loaded. */}
        {signatures.isError && (
          <div className="px-4 pb-3">
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
        {(signature || hasQuote || pool.length > 0) && (
          <div className="flex flex-wrap items-center gap-x-4 gap-y-2 px-4 pb-3 text-13 text-secondary">
            {pool.length > 0 && (
              <span className="flex items-center gap-2">
                <label htmlFor={signatureId}>{t("writing.signature.label")}</label>
                <select
                  id={signatureId}
                  value={signatureChoice}
                  disabled={frozen}
                  onChange={(e) => {
                    chooseSignature(e.target.value);
                  }}
                  onBlur={saveNow}
                  className="h-8 rounded-control bg-transparent text-13 text-primary"
                  data-testid="composer-signature-choice"
                >
                  <option value={SIGNATURE_DEFAULT}>
                    {defaultSignature
                      ? t("writing.signature.defaultNamed", { name: defaultSignature.name })
                      : t("writing.signature.defaultNone")}
                  </option>
                  {pool.map((s) => (
                    <option key={s.id} value={s.id}>
                      {s.name}
                    </option>
                  ))}
                  {fields.signatureId && !pool.some((s) => s.id === fields.signatureId) && (
                    <option value={fields.signatureId}>{t("writing.signature.deleted")}</option>
                  )}
                  <option value={SIGNATURE_NONE}>{t("writing.signature.none")}</option>
                </select>
              </span>
            )}
            {signature && pool.length === 0 && (
              <label className="flex items-center gap-2">
                <input
                  type="checkbox"
                  checked={fields.includeSignature}
                  disabled={frozen}
                  onChange={(e) => {
                    change({ includeSignature: e.target.checked });
                  }}
                  data-testid="composer-include-signature"
                />
                {t("composer.signature.include")}
              </label>
            )}
            {hasQuote && (
              <>
                <label className="flex items-center gap-2">
                  <input
                    type="checkbox"
                    checked={fields.includeQuote}
                    disabled={frozen}
                    onChange={(e) => {
                      change({ includeQuote: e.target.checked });
                    }}
                    data-testid="composer-include-quote"
                  />
                  {t(kind === "forward" ? "composer.quote.includeForward" : "composer.quote.include")}
                </label>
                {fields.includeQuote && (
                  <span className="flex items-center gap-2">
                    <label htmlFor={quoteId}>{t("composer.quote.position")}</label>
                    <select
                      id={quoteId}
                      value={fields.quotePosition}
                      disabled={frozen}
                      onChange={(e) => {
                        const position = QUOTE_POSITIONS.find((p) => p === e.target.value);
                        if (position) change({ quotePosition: position });
                      }}
                      className="h-8 rounded-control bg-transparent text-13 text-primary"
                      data-testid="composer-quote-position"
                    >
                      {QUOTE_POSITIONS.map((position) => (
                        <option key={position} value={position}>
                          {t(`composer.quote.${position}`)}
                        </option>
                      ))}
                    </select>
                  </span>
                )}
              </>
            )}
          </div>
        )}
        {/* Stage 16: per-draft send options. */}
        <div className="flex flex-wrap items-center gap-x-4 gap-y-2 px-4 pb-3 text-13 text-secondary" data-testid="composer-send-options">
          {canArchive && (
            <label className="flex items-center gap-2">
              <input
                type="checkbox"
                checked={sendOptions.archiveOriginal}
                disabled={frozen}
                onChange={(e) => {
                  void applySendOptions({ ...sendOptions, archiveOriginal: e.target.checked }).catch(() => undefined);
                }}
                data-testid="composer-archive-original"
              />
              {t("composer.sendOptions.archiveOriginal")}
            </label>
          )}
          <label className="flex items-center gap-2" title={t("composer.sendOptions.readReceiptHint")}>
            <input
              type="checkbox"
              checked={sendOptions.requestReadReceipt}
              disabled={frozen}
              onChange={(e) => {
                void applySendOptions({ ...sendOptions, requestReadReceipt: e.target.checked }).catch(() => undefined);
              }}
              data-testid="composer-read-receipt"
            />
            {t("composer.sendOptions.readReceipt")}
          </label>
          {/* Stage 20 (decision 0059): "Remind me if no reply by …". */}
          <FollowUpOption
            value={sendOptions.followUpAt ?? null}
            disabled={frozen}
            onChange={(followUpAt) => {
              void applySendOptions({ ...sendOptions, followUpAt }).catch(() => undefined);
            }}
          />
        </div>
      </div>
      {addingImages && (
        <p role="status" className="shrink-0 border-t border-border px-4 py-2 text-12 text-secondary" data-testid="composer-images-adding">
          {t("writing.images.adding")}
        </p>
      )}
      <ComposerAttachments
        attachments={draft.attachments.filter((a) => a.contentId === null)}
        busy={attaching}
        disabled={frozen}
        onPick={() => {
          void pick();
        }}
        onRemove={(attachment) => {
          void removeAttachment(attachment);
        }}
      />
      <div className="flex shrink-0 flex-col gap-2 border-t border-border-subtle px-4 py-3">
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
        {actionError && (
          <ErrorNotice context={t(ACTION_ERROR_TEXT[actionError.context])} error={actionError.error} data-testid="composer-action-error" />
        )}
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
            }}
            data-testid="composer-send-later-picker"
          />
        )}
        <div className="flex items-center gap-3">
          <Button
            onClick={() => {
              void send();
            }}
            disabled={sending || frozen}
            aria-label={t("composer.sendWithShortcut", { shortcut: sendShortcut })}
            data-testid="composer-send"
          >
            <Send aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
            {sending ? t("composer.sendingButton") : t("composer.sendButton")}
          </Button>
          <kbd aria-hidden className="rounded-control border border-border-subtle bg-canvas px-1 font-sans text-12 text-tertiary">
            {sendShortcut}
          </kbd>
          {canArchive && !sending && !frozen && (
            <OverflowMenu label={t("composer.sendOptions.menu")} align="start" items={sendMenu} data-testid="composer-send-menu" />
          )}
          <Button
            variant="secondary"
            disabled={sending || frozen || scheduling}
            aria-expanded={laterOpen}
            onClick={() => {
              setLaterOpen((open) => !open);
              setLaterError(null);
            }}
            data-testid="composer-send-later"
          >
            {t("writing.sendLater.button")}
          </Button>
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

interface TemplateMenuProps {
  id: string;
  templates: ReturnType<typeof useTemplates>;
  onChoose: (templateId: TemplateId) => void;
  onClose: () => void;
}

/** The composer's template list: a plain panel under the format bar (no popover: CSP). */
function TemplateMenu({ id, templates, onChoose, onClose }: TemplateMenuProps) {
  const { t } = useTranslation();
  const showSettings = useUiStore((state) => state.showSettings);
  const list = templates.data ?? [];
  return (
    <div
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
      className="mx-4 mt-2 flex flex-col gap-1 rounded-card border border-border bg-surface-raised p-2 shadow-floating"
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
