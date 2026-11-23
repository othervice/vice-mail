import { ArrowUp, Sparkles } from "lucide-react";
import { useId, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { AI_LIMITS, type Account, type AiAvailability, type AiSettingsInfo, type MessageId } from "../../services/ipc";
import { currentActionTarget, useActionTargetStore } from "../../stores/actionTarget";
import { useAiPanelStore } from "../../stores/aiPanel";
import { useUiStore } from "../../stores/ui";
import { cloudBlocked, panelAvailable } from "./AiPanel";
import { useAiAvailability, useAiSettings } from "./queries";

/**
 * The message the AI panel is about (the selection, else the row with the cursor): its thread
 * functions run for this message only. Null while none or several are chosen.
 */
function useBoundId(): MessageId | null {
  return useActionTargetStore((state) => {
    const ctx = currentActionTarget(state.targets)?.ctx;
    return ctx?.items.length === 1 ? (ctx.items[0]?.id ?? null) : null;
  });
}

/** Where the AI panel lives: the main window's mail views. */
function useInMail(): boolean {
  return useUiStore((state) => state.view.kind === "mail" || state.view.kind === "mailbox");
}

interface Usable {
  availability: AiAvailability;
  settings: AiSettingsInfo;
  summary: boolean;
  reply: boolean;
}

/**
 * What the AI may do for this account now: the function is on, and its profile is local or the
 * account has the cloud permission (decision 0024, point 5). Unknown settings: nothing.
 */
function useUsable(account: Account): Usable | null {
  const availability = useAiAvailability().data;
  const available = panelAvailable(availability);
  const settings = useAiSettings(available).data;
  if (!availability || !available || !settings) return null;
  return {
    availability,
    settings,
    summary: availability.summary && !cloudBlocked(settings, "summary", account.id),
    reply: availability.replySuggestion && !cloudBlocked(settings, "reply_suggestion", account.id),
  };
}

const CHIP =
  "inline-flex h-8 shrink-0 items-center gap-1 rounded-full border border-border bg-surface-raised px-3 text-12 font-medium text-secondary transition-colors hover:bg-hover hover:text-primary";

const ASK_CHIP =
  "inline-flex h-8 shrink-0 items-center rounded-full border border-border bg-surface-raised px-4 text-13 font-medium text-accent shadow-card transition-colors hover:bg-hover";

interface QuickRepliesProps {
  account: Account;
  messageId: MessageId;
}

/**
 * AI chips under an open message (decision 0024, point 8): "Suggest a reply" asks the panel for a
 * reply suggestion, which shows as an editable card there with "Open draft"; nothing is sent and
 * no draft exists before the user opens one. Only where the panel works on this very message,
 * while the function is usable and the account permitted; with AI off there is nothing.
 */
export function QuickReplies({ account, messageId }: QuickRepliesProps) {
  const { t } = useTranslation();
  const usable = useUsable(account);
  const bound = useBoundId();
  const inMail = useInMail();
  const request = useAiPanelStore((state) => state.request);
  if (!usable || !inMail || bound !== messageId || (!usable.reply && !usable.summary)) return null;
  return (
    <div role="group" aria-label={t("reading.ai.label")} className="flex flex-wrap items-center gap-2" data-testid="quick-replies">
      <Sparkles aria-hidden className="size-(--ui-icon-small) shrink-0 text-accent" strokeWidth={1.75} />
      {usable.reply && (
        <button
          type="button"
          className={CHIP}
          onClick={() => {
            request("suggestReply");
          }}
          data-testid="quick-reply-suggest"
        >
          {t("reading.ai.suggestReply")}
        </button>
      )}
      {usable.summary && (
        <button
          type="button"
          className={CHIP}
          onClick={() => {
            request("summarize");
          }}
          data-testid="quick-reply-summarize"
        >
          {t("reading.ai.summarize")}
        </button>
      )}
    </div>
  );
}

interface AskBarProps {
  account: Account;
}

/**
 * "Ask anything": a question field floating at the bottom of the reading pane while the AI panel
 * is folded. It sends nothing itself: Enter hands the question to the panel, which unfolds and
 * asks it through its own flow (consent step before the first request to a cloud provider, the
 * transfer shown, cancel). The chips start the panel's existing functions the same way.
 */
export function AskBar({ account }: AskBarProps) {
  const { t } = useTranslation();
  const inputId = useId();
  const usable = useUsable(account);
  const inMail = useInMail();
  const panelOpen = useAiPanelStore((state) => state.open);
  const request = useAiPanelStore((state) => state.request);
  const bound = useBoundId();
  const [question, setQuestion] = useState("");
  const [focused, setFocused] = useState(false);
  if (!usable || !inMail || panelOpen || !usable.availability.search) return null;
  const text = question.trim();
  const tooLong = [...text].length > AI_LIMITS.questionChars;

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (!text || tooLong) return;
    request("ask", text);
    setQuestion("");
  };
  const suggestions: { key: string; label: string; run: () => void }[] = [];
  if (usable.summary && bound !== null) {
    suggestions.push({ key: "summarize", label: t("reading.ask.summarize"), run: () => { request("summarize"); } });
  }
  if (usable.reply && bound !== null) {
    suggestions.push({ key: "reply", label: t("reading.ask.reply"), run: () => { request("suggestReply"); } });
  }
  suggestions.push({ key: "waiting", label: t("reading.ask.waiting"), run: () => { request("ask", t("reading.ask.waitingQuestion")); } });

  return (
    <div
      className="pointer-events-none absolute inset-x-4 bottom-4 flex justify-end"
      data-testid="ask-bar"
      onFocus={() => {
        setFocused(true);
      }}
      onBlur={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget)) setFocused(false);
      }}
    >
      <div className="pointer-events-auto flex w-full max-w-(--ui-reading-ask-width) flex-col-reverse items-end gap-2">
        <form
          noValidate
          onSubmit={submit}
          className="flex h-10 w-full items-center gap-2 rounded-full border border-border bg-surface-raised ps-3 pe-1 shadow-floating focus-within:outline-2 focus-within:outline-offset-2 focus-within:outline-focus-ring"
        >
          <Sparkles aria-hidden className="size-(--ui-icon-small) shrink-0 text-accent" strokeWidth={1.75} />
          <label htmlFor={inputId} className="sr-only">
            {t("reading.ask.label")}
          </label>
          <input
            id={inputId}
            type="text"
            value={question}
            onChange={(event) => {
              setQuestion(event.target.value);
            }}
            placeholder={t("reading.ask.placeholder")}
            aria-invalid={tooLong || undefined}
            autoComplete="off"
            className="h-full min-w-0 flex-1 bg-transparent text-13 text-primary outline-none placeholder:text-tertiary"
            data-testid="ask-bar-input"
          />
          <button
            type="submit"
            aria-label={t("reading.ask.submit")}
            disabled={!text || tooLong}
            className="flex size-8 shrink-0 items-center justify-center rounded-full bg-accent-fill text-on-accent transition-colors disabled:bg-hover disabled:text-tertiary"
            data-testid="ask-bar-submit"
          >
            <ArrowUp aria-hidden className="size-(--ui-icon-small)" strokeWidth={2} />
          </button>
        </form>
        {(focused || text !== "") && (
          <div role="group" aria-label={t("reading.ask.suggestions")} className="flex flex-col items-end gap-2" data-testid="ask-bar-suggestions">
            {suggestions.map((entry) => (
              <button
                key={entry.key}
                type="button"
                onClick={entry.run}
                className={ASK_CHIP}
              >
                {entry.label}
              </button>
            ))}
          </div>
        )}
        {tooLong && (
          <p role="status" className="rounded-control bg-surface-raised px-2 text-12 text-danger shadow-card">
            {t("ai.ask.tooLong", { max: AI_LIMITS.questionChars })}
          </p>
        )}
      </div>
    </div>
  );
}
