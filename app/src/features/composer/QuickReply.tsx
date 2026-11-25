import { Maximize2, Send, Trash2 } from "lucide-react";
import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { cx } from "../../components/cx";
import { ErrorNotice } from "../../components/ErrorNotice";
import { Tooltip } from "../../components/Tooltip";
import {
  composeIpc,
  reminderIpc,
  toCoreError,
  type Account,
  type CoreError,
  type Draft,
  type DraftUpdate,
  type MessageId,
  type QuoteLanguage,
} from "../../services/ipc";
import { openComposer, useComposerStore } from "../../stores/composer";
import { useUiStore } from "../../stores/ui";
import { useAppInfo } from "../app-info/queries";
import { formatShortcut, shortcutPlatform } from "../actions/shortcuts";
import { AttachmentReminder } from "./AttachmentReminder";

/** Local save this long after the last key, like the composer (contract: about 3 s). */
export const QUICK_REPLY_SAVE_DELAY_MS = 3000;

type ErrorContext = "start" | "save" | "send" | "expand" | "discard";

function quoteLanguage(language: string): QuoteLanguage {
  return language.toLowerCase().startsWith("de") ? "de" : "en";
}

/** The fields of a draft the quick reply writes: plain text, the quote as the user chose. */
function quickUpdate(draft: Draft, text: string, includeQuote: boolean): DraftUpdate {
  return {
    identityId: draft.identityId,
    to: draft.to,
    cc: draft.cc,
    bcc: draft.bcc,
    subject: draft.subject,
    format: "text",
    bodyHtml: draft.bodyHtml,
    bodyText: text,
    includeQuote,
    quotePosition: draft.quotePosition,
    includeSignature: draft.includeSignature,
    signatureId: draft.signatureId,
  };
}

interface QuickReplyProps {
  account: Account;
  messageId: MessageId;
  /** Who the reply goes to, for the field's name ("Quick reply to Grace Hopper"). */
  sender: string;
  /** Escape in the field: the focus goes back to the message. */
  onLeave: () => void;
}

/**
 * Stage 16: the inline quick reply under the open message. A plain-text field backed by a real
 * reply draft (created with the first key, never before), saved like the composer's and sent
 * through the same outbox with the usual undo delay. "Expand" hands the same draft to the full
 * composer; the quote follows the writing settings (position) and can be left out here.
 */
export function QuickReply({ account, messageId, sender, onLeave }: QuickReplyProps) {
  const { t, i18n } = useTranslation();
  const language = quoteLanguage(i18n.language);
  const platform = shortcutPlatform(useAppInfo().data?.platform);
  const track = useComposerStore((state) => state.track);
  const showAccountsPage = useUiStore((state) => state.showAccountsPage);
  const fieldId = useId();
  const hintId = useId();
  const [text, setText] = useState("");
  const [includeQuote, setIncludeQuote] = useState(true);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [busy, setBusy] = useState<"send" | "expand" | "discard" | null>(null);
  const [error, setError] = useState<{ context: ErrorContext; error: CoreError } | null>(null);
  const [focused, setFocused] = useState(false);
  // Stage 20 (decision 0058): the text speaks of an attachment; a quick reply has none.
  const [attachmentWord, setAttachmentWord] = useState<string | null>(null);

  // Refs for saves that run after a render (debounce, unmount) and for the single creation.
  const latest = useRef({ text, includeQuote });
  const created = useRef<Promise<Draft> | null>(null);
  const draftRef = useRef<Draft | null>(null);
  const dirty = useRef(false);
  const timer = useRef<number | undefined>(undefined);
  const queue = useRef<Promise<unknown>>(Promise.resolve());
  // The draft could not be created: no new attempt per key press, only when the user asks.
  const startFailed = useRef(false);

  /** The reply draft, created once with the first key. Rejects with a `CoreError`. */
  const ensureDraft = (): Promise<Draft> => {
    if (!created.current) {
      created.current = composeIpc.createDraft({ kind: "reply", messageId, language }).then(
        (next) => {
          startFailed.current = false;
          draftRef.current = next;
          setDraft(next);
          const quote = next.includeQuote && latest.current.includeQuote;
          latest.current = { ...latest.current, includeQuote: quote };
          setIncludeQuote(quote);
          return next;
        },
        (e: unknown) => {
          created.current = null;
          startFailed.current = true;
          throw toCoreError(e);
        },
      );
    }
    return created.current;
  };

  /** Saves what changed since the last save, one save after the other. Rejects on failure. */
  const flush = (): Promise<void> => {
    window.clearTimeout(timer.current);
    const run = queue.current.then(async () => {
      if (!dirty.current) return;
      const current = await ensureDraft();
      dirty.current = false;
      try {
        const saved = await composeIpc.saveDraft(current.id, quickUpdate(current, latest.current.text, latest.current.includeQuote));
        draftRef.current = saved;
        setDraft(saved);
      } catch (e) {
        dirty.current = true;
        throw toCoreError(e);
      }
    });
    queue.current = run.catch(() => undefined);
    return run;
  };

  const schedule = () => {
    dirty.current = true;
    window.clearTimeout(timer.current);
    if (startFailed.current) return;
    timer.current = window.setTimeout(() => {
      void flush().catch((e: unknown) => {
        setError({ context: "save", error: toCoreError(e) });
      });
    }, QUICK_REPLY_SAVE_DELAY_MS);
  };

  const change = (next: string) => {
    latest.current = { ...latest.current, text: next };
    setText(next);
    // No draft before the user writes something, and no new attempt per key after a failure.
    if (next.trim() === "" && !created.current) return;
    if (startFailed.current) {
      dirty.current = true;
      return;
    }
    setError(null);
    if (!created.current) {
      ensureDraft().catch((e: unknown) => {
        setError({ context: "start", error: toCoreError(e) });
      });
    }
    schedule();
  };

  const toggleQuote = (next: boolean) => {
    latest.current = { ...latest.current, includeQuote: next };
    setIncludeQuote(next);
    if (created.current) schedule();
  };

  const reset = () => {
    window.clearTimeout(timer.current);
    created.current = null;
    startFailed.current = false;
    draftRef.current = null;
    dirty.current = false;
    latest.current = { text: "", includeQuote: true };
    setText("");
    setIncludeQuote(true);
    setDraft(null);
  };

  // Leaving the message: the draft is saved and handed to the server copy, or removed when
  // nothing was written in the end.
  useEffect(
    () => () => {
      window.clearTimeout(timer.current);
      if (!created.current) return;
      const pending = created.current;
      const empty = latest.current.text.trim() === "";
      void (empty
        ? pending.then((d) => composeIpc.deleteDraft(d.id))
        : flush().then(() => pending.then((d) => composeIpc.closeDraft(d.id)))
      ).catch((e: unknown) => {
        useComposerStore.getState().report("close", toCoreError(e));
      });
    },
    // Runs once on unmount; `flush` reads refs only.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [],
  );

  /** Stage 20: asks first when the text speaks of an attachment (`checked`: "Send anyway"). */
  const send = async (checked = false) => {
    if (busy || latest.current.text.trim() === "") return;
    setBusy("send");
    setError(null);
    try {
      dirty.current = true; // the final save, also when nothing changed since the last one
      await flush();
      const current = draftRef.current ?? (await ensureDraft());
      if (!checked) {
        const word = await reminderIpc.checkAttachmentReminder(current.id);
        if (word !== null) {
          setAttachmentWord(word);
          return;
        }
      }
      const mail = await composeIpc.sendDraft(current.id, crypto.randomUUID());
      track(mail);
      reset();
    } catch (e) {
      setError({ context: "send", error: toCoreError(e) });
    } finally {
      setBusy(null);
    }
  };

  /** The same draft in the full composer; this field starts over empty. */
  const expand = async () => {
    if (busy) return;
    setBusy("expand");
    setError(null);
    try {
      dirty.current = true;
      await flush();
      const current = draftRef.current ?? (await ensureDraft());
      reset();
      openComposer({ kind: "draft", draftId: current.id, draft: current });
    } catch (e) {
      setError({ context: "expand", error: toCoreError(e) });
    } finally {
      setBusy(null);
    }
  };

  const discard = async () => {
    if (busy || !created.current) return;
    setBusy("discard");
    setError(null);
    try {
      window.clearTimeout(timer.current);
      const current = await created.current;
      await queue.current;
      await composeIpc.deleteDraft(current.id);
      reset();
    } catch (e) {
      setError({ context: "discard", error: toCoreError(e) });
    } finally {
      setBusy(null);
    }
  };

  const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      onLeave();
      return;
    }
    if (event.key === "Enter" && (platform === "macos" ? event.metaKey : event.ctrlKey) && !event.altKey && !event.shiftKey) {
      event.preventDefault();
      void send();
    }
  };

  const sendShortcut = formatShortcut({ key: "Enter", mod: true }, platform);
  const hasText = text.trim() !== "";
  const open = focused || text !== "" || draft !== null;
  const hasQuote = draft === null || draft.quoteText !== null || draft.quoteHtml !== null;

  return (
    <div
      role="group"
      aria-label={t("quickReply.label", { name: sender })}
      // Open while the focus is anywhere in it (Tab from the field reaches the buttons).
      onFocus={() => {
        setFocused(true);
      }}
      onBlur={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget)) setFocused(false);
      }}
      className="flex flex-col gap-2 rounded-card border border-border-subtle bg-surface px-3 py-2 has-[textarea:focus-visible]:outline-2 has-[textarea:focus-visible]:-outline-offset-2 has-[textarea:focus-visible]:outline-focus-ring"
      data-testid="inline-reply"
      data-print-hide
    >
      <label htmlFor={fieldId} className="sr-only">
        {t("quickReply.label", { name: sender })}
      </label>
      <textarea
        id={fieldId}
        value={text}
        rows={open ? 4 : 1}
        placeholder={t("quickReply.placeholder", { name: sender })}
        aria-describedby={hintId}
        spellCheck
        onChange={(e) => {
          change(e.target.value);
        }}
        onKeyDown={onKeyDown}
        onBlur={() => {
          if (dirty.current) {
            void flush().catch((e: unknown) => {
              setError({ context: "save", error: toCoreError(e) });
            });
          }
        }}
        className="w-full resize-none bg-transparent text-14 leading-relaxed text-primary outline-none placeholder:text-tertiary"
        data-testid="inline-reply-field"
      />
      <p id={hintId} className={cx("text-12 text-tertiary", !open && "sr-only")}>
        {t("quickReply.hint", { shortcut: sendShortcut })}
      </p>
      {error && (
        <ErrorNotice
          context={t(`quickReply.errors.${error.context}`)}
          error={error.error}
          {...(error.context === "start" && {
            onRetry: () => {
              startFailed.current = false;
              setError(null);
              ensureDraft()
                .then(() => {
                  if (dirty.current) schedule();
                })
                .catch((e: unknown) => {
                  setError({ context: "start", error: toCoreError(e) });
                });
            },
          })}
          actions={
            error.error.code === "read_only" ? (
              <Button
                variant="secondary"
                onClick={() => {
                  showAccountsPage({ kind: "account", accountId: account.id });
                }}
                data-testid="inline-reply-allow-changes"
              >
                {t("actions.readOnly.open")}
              </Button>
            ) : undefined
          }
          data-testid="inline-reply-error"
        />
      )}
      {open && (
        <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
          <Button
            disabled={!hasText || busy !== null}
            onClick={() => {
              void send();
            }}
            aria-label={t("quickReply.sendWithShortcut", { shortcut: sendShortcut })}
            data-testid="inline-reply-send"
          >
            <Send aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
            {busy === "send" ? t("quickReply.sending") : t("quickReply.send")}
          </Button>
          <Tooltip label={t("quickReply.expandHint")}>
            <Button
              variant="secondary"
              disabled={busy !== null}
              onClick={() => {
                void expand();
              }}
              aria-label={t("quickReply.expandHint")}
              data-testid="inline-reply-expand"
            >
              <Maximize2 aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
              {t("quickReply.expand")}
            </Button>
          </Tooltip>
          {hasQuote && (
            <label className="flex items-center gap-2 text-13 text-secondary">
              <input
                type="checkbox"
                checked={includeQuote}
                disabled={busy !== null}
                onChange={(e) => {
                  toggleQuote(e.target.checked);
                }}
                data-testid="inline-reply-quote"
              />
              {t("quickReply.includeQuote")}
            </label>
          )}
          {draft && (
            <Tooltip label={t("quickReply.discardHint")}>
              <button
                type="button"
                aria-label={t("quickReply.discardHint")}
                disabled={busy !== null}
                onClick={() => {
                  void discard();
                }}
                className="ms-auto flex size-8 items-center justify-center rounded-control text-secondary hover:bg-hover hover:text-primary disabled:opacity-50"
                data-testid="inline-reply-discard"
              >
                <Trash2 aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
              </button>
            </Tooltip>
          )}
        </div>
      )}
      {attachmentWord !== null && (
        <AttachmentReminder
          word={attachmentWord}
          onSendAnyway={() => {
            setAttachmentWord(null);
            void send(true);
          }}
          onAddAttachment={() => {
            // Attachments are added in the full composer, with the same draft.
            setAttachmentWord(null);
            void expand();
          }}
          onCancel={() => {
            setAttachmentWord(null);
          }}
        />
      )}
    </div>
  );
}
