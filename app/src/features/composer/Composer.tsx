import { Send, Trash2, Upload, X } from "lucide-react";
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
  toCoreError,
  type Address,
  type CoreError,
  type Draft,
  type DraftAttachment,
  type DraftUpdate,
  type QuoteLanguage,
} from "../../services/ipc";
import { takeReturnFocus, useComposerStore, type ComposePrefill, type ComposerSource } from "../../stores/composer";
import { useUiStore } from "../../stores/ui";
import { useAppInfo } from "../app-info/queries";
import { formatShortcut, shortcutPlatform } from "../actions/shortcuts";
import { htmlToText, invalidAddressIn, textToHtml } from "./addresses";
import { ComposerAttachments } from "./ComposerAttachments";
import { MailtoNotices } from "../extras/MailtoNotices";
import { useComposeIdentities } from "./queries";
import { RecipientField } from "./RecipientField";

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
  };
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
      return composeIpc.createDraft({ kind: source.kind, messageId: source.messageId, language });
    case "draft":
      return source.draft ? Promise.resolve(source.draft) : composeIpc.getDraft(source.draftId);
    case "message":
      return composeIpc.openDraftFromMessage(source.messageId);
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
    <ComposerFrame title={t(`composer.title.${source.kind === "draft" || source.kind === "message" ? "draft" : source.kind}`)} onClose={close}>
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

interface ComposerFrameProps {
  title: string;
  onClose: () => void;
  status?: ReactNode;
  actions?: ReactNode;
  children: ReactNode;
  onKeyDown?: (event: KeyboardEvent<HTMLElement>) => void;
  dropping?: boolean;
}

/** The panel: over the reading pane, at the right of the window. */
function ComposerFrame({ title, onClose, status, actions, children, onKeyDown, dropping = false }: ComposerFrameProps) {
  const { t } = useTranslation();
  const titleId = useId();
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
        // The list's single-key shortcuts (E, #, …) must not act on mail behind the composer.
        event.stopPropagation();
      }}
      className={cx(
        "absolute inset-y-0 end-0 z-20 flex w-full max-w-(--ui-reading-max-width) flex-col border-s border-border bg-surface shadow-panel",
        dropping && "outline-2 -outline-offset-2 outline-focus-ring",
      )}
      data-testid="composer"
    >
      <header className="flex h-(--ui-header-height) shrink-0 items-center gap-3 border-b border-border px-4">
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
        </div>
      )}
      {children}
    </section>
  );
}

type SaveState = "idle" | "saving" | "saved" | "error";

function ComposerForm({ seq, initial, prefill }: { seq: number; initial: Draft; prefill?: ComposePrefill | undefined }) {
  const { t } = useTranslation();
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
  const [saveState, setSaveState] = useState<SaveState>("idle");
  const [saveError, setSaveError] = useState<CoreError | null>(null);
  const [sendError, setSendError] = useState<CoreError | null>(null);
  const [actionError, setActionError] = useState<{ context: "attach" | "discard" | "unfreeze"; error: CoreError } | null>(null);
  const [sending, setSending] = useState(false);
  const [attaching, setAttaching] = useState(false);
  const [confirmDiscard, setConfirmDiscard] = useState(false);
  const [dropping, setDropping] = useState(false);
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
          setAttaching(true);
          composeIpc
            .addDroppedAttachments(draftId, event.paths)
            .then(
              (saved) => {
                setDraft((d) => ({ ...d, attachments: saved.attachments, server: saved.server, state: saved.state }));
              },
              (e: unknown) => {
                setActionError({ context: "attach", error: toCoreError(e) });
              },
            )
            .finally(() => {
              setAttaching(false);
            });
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
  }, [draftId]);

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

  const send = async () => {
    if (sending || frozen) return;
    setSending(true);
    setSendError(null);
    try {
      dirty.current = true; // the final save, also when nothing changed since the last one
      await flush();
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
    } catch (e) {
      setActionError({ context: "attach", error: toCoreError(e) });
    } finally {
      setAttaching(false);
    }
  };

  const removeAttachment = async (attachment: DraftAttachment) => {
    try {
      const saved = await composeIpc.removeDraftAttachment(draftId, attachment.id);
      setDraft((d) => ({ ...d, attachments: saved.attachments, server: saved.server }));
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

  const onKeyDown = (event: KeyboardEvent<HTMLElement>) => {
    if (event.key === "Enter" && (platform === "macos" ? event.metaKey : event.ctrlKey) && !event.altKey) {
      event.preventDefault();
      void send();
    }
  };

  const identityList = identities.data ?? [];
  const identity =
    identityList.find((i) => i.id === fields.identityId) ?? identityList.find((i) => i.isDefault) ?? identityList[0] ?? null;
  const signature = identity?.signature?.trim() ? identity.signature : null;
  // The separator the core adds when it puts the message together (plain text, not translated).
  const signatureText = signature ? `-- \n${signature}` : "";
  const hasQuote = draft.quoteHtml !== null || draft.quoteText !== null;
  const refused = sendError?.code === "invalid_address" ? invalidAddressIn(sendError.detail, [fields.to, fields.cc, fields.bcc]) : null;
  const sendShortcut = formatShortcut({ key: "Enter", mod: true }, platform);
  const kind = draft.kind;
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
  const quoteBlock =
    hasQuote && fields.includeQuote ? (
      <div className="mx-4 mb-3 rounded-card border border-border bg-canvas p-3" data-testid="composer-quote">
        {draft.quoteText !== null ? (
          <pre className="max-h-(--ui-picker-list-height) overflow-y-auto font-sans text-13 whitespace-pre-wrap text-secondary">
            {draft.quoteText}
          </pre>
        ) : (
          <p className="text-13 text-secondary">{t("composer.quote.formattedOnly")}</p>
        )}
      </div>
    ) : null;

  return (
    <ComposerFrame
      title={title}
      onClose={() => {
        void closeComposer();
      }}
      status={status}
      actions={discardButton}
      onKeyDown={onKeyDown}
      dropping={dropping}
    >
      <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">
        {/* Stage 9: hints about the mailto: link this draft came from (blind copies first). */}
        {prefill && <MailtoNotices notices={prefill.notices} inReplyTo={prefill.inReplyTo !== null} />}
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
          <div className="flex items-center gap-2 border-b border-border px-4 py-1">
            <label htmlFor={identityId} className="w-12 shrink-0 text-13 text-secondary">
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
        {identityList.length === 1 && identity && (
          <p className="flex items-center gap-2 border-b border-border px-4 py-2 text-13" data-testid="composer-identity-single">
            <span className="w-12 shrink-0 text-secondary">{t("composer.from")}</span>
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
            data-testid="composer-bcc"
          />
        )}
        <div className="flex items-center gap-2 border-b border-border px-4 py-1">
          <label htmlFor={subjectId} className="w-12 shrink-0 text-13 text-secondary">
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
                "rounded-control px-2 py-1 text-12 font-medium",
                fields.format === format ? "bg-selected text-primary" : "text-secondary hover:bg-hover",
              )}
              data-testid={`composer-format-${format}`}
            >
              {t(`composer.format.${format}`)}
            </button>
          ))}
        </div>
        {fields.quotePosition === "above" && quoteBlock}
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
            />
          </Suspense>
        ) : (
          <>
            <label htmlFor={textBodyId} className="sr-only">
              {t("composer.body")}
            </label>
            <textarea
              id={textBodyId}
              value={fields.bodyText}
              disabled={frozen}
              autoFocus={kind === "reply" || kind === "reply_all"}
              onChange={(e) => {
                change({ bodyText: e.target.value });
              }}
              onBlur={saveNow}
              className="min-h-(--ui-picker-list-height) flex-1 resize-none bg-transparent px-4 py-3 font-sans text-14 text-primary outline-none"
              data-testid="composer-text-body"
            />
          </>
        )}
        {signature && fields.includeSignature && (
          <pre
            aria-label={t("composer.signature.preview")}
            className="mx-4 mb-3 font-sans text-13 whitespace-pre-wrap text-secondary"
            data-testid="composer-signature"
          >
            {signatureText}
          </pre>
        )}
        {fields.quotePosition === "below" && quoteBlock}
        {(signature || hasQuote) && (
          <div className="flex flex-wrap items-center gap-x-4 gap-y-2 px-4 pb-3 text-13 text-secondary">
            {signature && (
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
      </div>
      <ComposerAttachments
        attachments={draft.attachments}
        busy={attaching}
        disabled={frozen}
        onPick={() => {
          void pick();
        }}
        onRemove={(attachment) => {
          void removeAttachment(attachment);
        }}
      />
      <div className="flex shrink-0 flex-col gap-2 border-t border-border px-4 py-3">
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
          <ErrorNotice context={t(`composer.errors.${actionError.context}`)} error={actionError.error} data-testid="composer-action-error" />
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
          <kbd aria-hidden className="rounded-control border border-border px-1 font-sans text-12 text-secondary">
            {sendShortcut}
          </kbd>
        </div>
      </div>
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
