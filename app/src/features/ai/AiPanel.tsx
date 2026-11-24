import { useQuery } from "@tanstack/react-query";
import { MessagesSquare, PanelRightClose, Sparkles } from "lucide-react";
import { useEffect, useId, useRef, useState, type FormEvent, type KeyboardEvent, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { ChoiceGroup } from "../../components/ChoiceGroup";
import { ErrorNotice } from "../../components/ErrorNotice";
import { IconButton } from "../../components/IconButton";
import { Notice } from "../../components/Notice";
import {
  AI_LIMITS,
  aiIpc,
  searchIpc,
  toCoreError,
  type Account,
  type AccountId,
  type AiAnswer,
  type AiAvailability,
  type AiClaim,
  type AiFunction,
  type AiReplySuggestion,
  type AiSettingsInfo,
  type AiSummary,
  type CoreError,
  type MessageId,
  type MessageListItem,
  type RenderToken,
} from "../../services/ipc";
import { currentActionTarget, useActionTargetStore } from "../../stores/actionTarget";
import { useAiPanelStore } from "../../stores/aiPanel";
import { openComposer } from "../../stores/composer";
import { useSearchStore } from "../../stores/search";
import { useUiStore } from "../../stores/ui";
import { useAccounts } from "../accounts/queries";
import { threadQueryKey } from "../threads/queries";
import { AiLinkCheck, AiLinkList, AiText } from "./AiText";
import { AiProgress, isLocal, TransferSummary } from "./AiTransfer";
import { ConsentStep } from "./ConsentStep";
import { OpenReplyHintList } from "./OpenReplyHints";
import { useAiAvailability, useAiSettings } from "./queries";
import { openSource, sourceLabel, useSourceItems } from "./sources";
import { profileFor, useAiRun, type AiRun } from "./useAiRun";

/** Some function of the panel is usable now (docs/DESIGN.md: without AI the panel is not there). */
export function panelAvailable(availability: AiAvailability | undefined): boolean {
  return Boolean(availability?.enabled && (availability.summary || availability.replySuggestion || availability.search));
}

/**
 * Whether the UI must not send this account's mail for a function: its profile is a cloud
 * provider and the account has no cloud permission. The core refuses too (`ai_not_permitted`);
 * the UI does not even ask.
 */
export function cloudBlocked(settings: AiSettingsInfo | undefined, aiFunction: AiFunction, accountId: AccountId): boolean {
  const profile = profileFor(settings, aiFunction);
  if (!profile || isLocal(profile.locality)) return false;
  return !settings?.accounts.some((entry) => entry.accountId === accountId && entry.cloudAllowed);
}

/** The header button that folds the panel in and out; absent while no AI function is usable. */
export function AiPanelToggle() {
  const { t } = useTranslation();
  const availability = useAiAvailability();
  const open = useAiPanelStore((state) => state.open);
  const toggle = useAiPanelStore((state) => state.toggle);
  if (!panelAvailable(availability.data)) return null;
  return <IconButton icon={Sparkles} label={open ? t("ai.panel.hide") : t("ai.panel.show")} onClick={toggle} />;
}

/** The single message the mail view acts on (the selection, else the row with the cursor). */
function useBoundMessage(): { item: MessageListItem; account: Account } | null {
  const ctx = useActionTargetStore((state) => currentActionTarget(state.targets)?.ctx ?? null);
  const [last, setLast] = useState<{ item: MessageListItem; account: Account } | null>(null);
  // While a picker is open the view offers no context: the panel stays on its thread.
  if (ctx === null) return last;
  const item = ctx.items.length === 1 ? ctx.items[0] : undefined;
  const current = item ? { item, account: ctx.account } : null;
  if (current?.item.id !== last?.item.id || current?.account.id !== last?.account.id) setLast(current);
  return current;
}

function errorOf(state: { kind: string; error?: CoreError }): CoreError | null {
  return state.kind === "error" && state.error ? state.error : null;
}

/** Labels for the messages of the bound thread ("Grace Hopper, 20 Nov 2026"), to name sources. */
function useSourceLabels(account: Account, messageId: MessageId, enabled: boolean): Map<MessageId, string> {
  const { t, i18n } = useTranslation();
  const thread = useQuery({
    queryKey: threadQueryKey(account.id, messageId, null),
    queryFn: () => searchIpc.getThread(messageId, null),
    enabled,
  });
  const labels = new Map<MessageId, string>();
  for (const entry of thread.data?.messages ?? []) labels.set(entry.message.id, sourceLabel(t, i18n.language, entry.message));
  return labels;
}

/** Numbers the sources of a result in the order they first appear ("Mail 1", "Mail 2"). */
export function numberSources(claims: readonly AiClaim[]): Map<MessageId, number> {
  const numbers = new Map<MessageId, number>();
  for (const claim of claims) for (const id of claim.sources) if (!numbers.has(id)) numbers.set(id, numbers.size + 1);
  return numbers;
}

/**
 * The mails a statement rests on, as buttons that show the mail in the thread, or (decision 0027)
 * in a window of its own when the thread pane does not have it.
 */
function Sources({
  sources,
  labels,
  numbers,
  inList = true,
}: {
  sources: readonly MessageId[];
  labels: Map<MessageId, string>;
  numbers: Map<MessageId, number>;
  /** False for a summary: selecting another row of the list would bind the panel to it. */
  inList?: boolean;
}) {
  const { t } = useTranslation();
  return (
    <span className="mt-1 flex flex-wrap gap-1" aria-label={t("ai.sources.title")} role="group">
      {sources.map((id) => {
        const label = labels.get(id) ?? t("ai.sources.numbered", { n: numbers.get(id) ?? 0 });
        return (
          <button
            key={id}
            type="button"
            onClick={() => {
              void openSource(id, { inList });
            }}
            aria-label={t("ai.sources.show", { label })}
            className="rounded-control border border-border px-2 text-12 text-secondary hover:bg-hover hover:text-primary"
            data-testid="ai-source"
            data-message-id={id}
          >
            {label}
          </button>
        );
      })}
    </span>
  );
}

/**
 * AI text is untrusted: text nodes only, never HTML, never an image; links the core found open
 * only through the link check (decision 0028).
 */
function ClaimList({
  title,
  claims,
  labels,
  numbers,
  linkToken,
  ...rest
}: {
  title: string;
  claims: readonly (AiClaim & { due?: string })[];
  labels: Map<MessageId, string>;
  numbers: Map<MessageId, number>;
  linkToken: RenderToken | null;
  "data-testid": string;
}) {
  const { t } = useTranslation();
  if (claims.length === 0) return null;
  return (
    <section aria-label={title} className="flex flex-col gap-2" data-testid={rest["data-testid"]}>
      <h4 className="text-13 font-semibold text-primary">{title}</h4>
      <ul className="flex flex-col gap-2">
        {claims.map((claim, index) => (
          <li key={index} className="flex flex-col text-13 text-primary" data-testid="ai-claim">
            <span className="break-words whitespace-pre-wrap">
              <AiText segments={claim.segments} linkToken={linkToken} />
              {claim.due !== undefined && ` ${t("ai.summary.due", { due: claim.due })}`}
            </span>
            <Sources sources={claim.sources} labels={labels} numbers={numbers} inList={false} />
          </li>
        ))}
      </ul>
    </section>
  );
}

function Card({ title, children, ...rest }: { title: string; children: ReactNode; "data-testid": string }) {
  return (
    <article aria-label={title} className="flex flex-col gap-3 rounded-card border border-border bg-surface-raised p-3" data-testid={rest["data-testid"]}>
      <h3 className="text-14 font-semibold text-primary">{title}</h3>
      {children}
    </article>
  );
}

function SummaryCard({ summary, account, messageId }: { summary: AiSummary; account: Account; messageId: MessageId }) {
  const { t } = useTranslation();
  const labels = useSourceLabels(account, messageId, true);
  const numbers = numberSources([...summary.keyPoints, ...summary.openQuestions, ...summary.deadlines]);
  const empty = summary.keyPoints.length === 0 && summary.openQuestions.length === 0 && summary.deadlines.length === 0;
  return (
    <Card title={t("ai.summary.title")} data-testid="ai-summary">
      {empty && <p className="text-13 text-secondary">{t("ai.summary.empty")}</p>}
      <ClaimList title={t("ai.summary.keyPoints")} claims={summary.keyPoints} labels={labels} numbers={numbers} linkToken={summary.linkToken} data-testid="ai-key-points" />
      <ClaimList title={t("ai.summary.openQuestions")} claims={summary.openQuestions} labels={labels} numbers={numbers} linkToken={summary.linkToken} data-testid="ai-open-questions" />
      <ClaimList title={t("ai.summary.deadlines")} claims={summary.deadlines} labels={labels} numbers={numbers} linkToken={summary.linkToken} data-testid="ai-deadlines" />
      {summary.dropped > 0 && (
        <p className="text-12 text-secondary" data-testid="ai-dropped">
          {t("ai.summary.dropped", { count: summary.dropped })}
        </p>
      )}
      <AiLinkCheck linkToken={summary.linkToken} />
      <TransferSummary transfers={[summary.transfer]} />
    </Card>
  );
}

function ReplyCard({ suggestion }: { suggestion: AiReplySuggestion }) {
  const { t } = useTranslation();
  const id = useId();
  const [text, setText] = useState(suggestion.text);
  const [opening, setOpening] = useState(false);
  const [error, setError] = useState<CoreError | null>(null);
  const open = async () => {
    setOpening(true);
    setError(null);
    try {
      // A normal reply draft with this text; the AI never creates one itself.
      const draft = await aiIpc.openReplyDraft(suggestion.inReplyTo, text);
      openComposer({ kind: "draft", draftId: draft.id, draft });
    } catch (e) {
      setError(toCoreError(e));
    } finally {
      setOpening(false);
    }
  };
  return (
    <Card title={t("ai.reply.title")} data-testid="ai-reply">
      <label htmlFor={id} className="text-12 text-secondary">
        {t("ai.reply.editable")}
      </label>
      <textarea
        id={id}
        value={text}
        onChange={(event) => {
          setText(event.target.value);
        }}
        rows={8}
        className="w-full resize-y rounded-control border border-border-strong bg-surface px-3 py-2 font-sans text-13 text-primary select-text"
        data-testid="ai-reply-text"
      />
      <div>
        <Button disabled={opening || text.trim() === ""} onClick={() => void open()} data-testid="ai-open-draft">
          {t("ai.reply.openDraft")}
        </Button>
      </div>
      {error && <ErrorNotice context={t("ai.reply.openFailed")} error={error} />}
      <AiLinkList segments={suggestion.segments} linkToken={suggestion.linkToken} />
      <AiLinkCheck linkToken={suggestion.linkToken} />
      <TransferSummary transfers={[suggestion.transfer]} />
    </Card>
  );
}

/** Consent, progress, error or nothing, for one run. Results are drawn by the caller. */
function RunState<T>({ run, aiFunction, context }: { run: AiRun<T>; aiFunction: AiFunction; context: string }) {
  const { t } = useTranslation();
  const showSettings = useUiStore((state) => state.showSettings);
  const { state } = run;
  if (state.kind === "consent") {
    return (
      <ConsentStep
        aiFunction={aiFunction}
        profile={state.profile}
        input={state.input}
        error={run.agreeError}
        onAgree={() => void run.agree()}
        onCancel={run.cancel}
      />
    );
  }
  if (state.kind === "running") return <AiProgress requestId={state.requestId} onCancel={run.cancel} />;
  if (state.kind === "cancelled") {
    return (
      <p role="status" className="text-13 text-secondary" data-testid="ai-cancelled">
        {t("ai.progress.cancelled")}
      </p>
    );
  }
  const error = errorOf(state);
  if (!error) return null;
  const toSettings = error.code === "ai_off" || error.code === "ai_not_permitted" || error.code === "ai_unauthorized";
  return (
    <ErrorNotice
      context={context}
      error={error}
      data-testid="ai-error"
      {...(toSettings && {
        actions: (
          <Button
            variant="secondary"
            onClick={() => {
              showSettings("ai");
            }}
          >
            {t("ai.openSettings")}
          </Button>
        ),
      })}
    />
  );
}

/** Not allowed for this account: said before anything happens, with the way to change it. */
function NotPermitted({ id }: { id: string }) {
  const { t } = useTranslation();
  const showSettings = useUiStore((state) => state.showSettings);
  return (
    <div id={id}>
      <Notice title={t("ai.panel.notPermittedTitle")} data-testid="ai-not-permitted">
        <p>{t("ai.panel.notPermittedBody")}</p>
        <div>
          <Button
            variant="secondary"
            onClick={() => {
              showSettings("ai");
            }}
          >
            {t("ai.openSettings")}
          </Button>
        </div>
      </Notice>
    </div>
  );
}

interface ThreadToolsProps {
  item: MessageListItem;
  account: Account;
  availability: AiAvailability;
  settings: AiSettingsInfo | undefined;
  /** A consent step of this thread waits for the user (the panel folds its question box away). */
  onConsentChange: (pending: boolean) => void;
}

function ThreadTools({ item, account, availability, settings, onConsentChange }: ThreadToolsProps) {
  const { t } = useTranslation();
  const blockedId = useId();
  const summary = useAiRun<AiSummary>("summary");
  const reply = useAiRun<AiReplySuggestion>("reply_suggestion");
  const consentPending = summary.state.kind === "consent" || reply.state.kind === "consent";
  useEffect(() => {
    onConsentChange(consentPending);
  }, [consentPending, onConsentChange]);
  useEffect(
    () => () => {
      onConsentChange(false);
    },
    [onConsentChange],
  );
  const summaryBlocked = cloudBlocked(settings, "summary", account.id);
  const replyBlocked = cloudBlocked(settings, "reply_suggestion", account.id);
  const blocked = (availability.summary && summaryBlocked) || (availability.replySuggestion && replyBlocked);
  const busy = (run: { state: { kind: string } }) => run.state.kind === "running" || run.state.kind === "consent";
  const startSummary = () => {
    summary.run((requestId) => aiIpc.summarizeThread(requestId, item.id), { function: "summary", messageId: item.id });
  };
  const startReply = () => {
    reply.run((requestId) => aiIpc.suggestReply(requestId, item.id), { function: "reply_suggestion", messageId: item.id });
  };
  // Stage 8 palette: "Summarize" and "Suggest a reply" run here, where their progress shows.
  const command = useAiPanelStore((state) => state.command);
  const clearCommand = useAiPanelStore((state) => state.clearCommand);
  const settingsKnown = settings !== undefined;
  useEffect(() => {
    // Without the settings the cloud permission is unknown: the command waits for them.
    if (!command || command.kind === "ask" || !settingsKnown) return;
    clearCommand();
    if (command.kind === "summarize" && availability.summary && !summaryBlocked) startSummary();
    if (command.kind === "suggestReply" && availability.replySuggestion && !replyBlocked) startReply();
    // Only a new command (or the settings arriving) starts something.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [command, settingsKnown]);
  return (
    <section aria-label={t("ai.panel.thread", { subject: item.subject || t("list.noSubject") })} className="flex flex-col gap-3" data-testid="ai-thread">
      <p className="truncate text-13 text-secondary" data-testid="ai-bound">
        {t("ai.panel.about", { subject: item.subject || t("list.noSubject") })}
      </p>
      {blocked && settings && <NotPermitted id={blockedId} />}
      <div className="flex flex-wrap gap-2">
        {availability.summary && (
          <Button
            variant="secondary"
            disabled={!settings || summaryBlocked || busy(summary)}
            {...(summaryBlocked && { "aria-describedby": blockedId })}
            onClick={startSummary}
            data-testid="ai-summarize"
          >
            {t("ai.panel.summarize")}
          </Button>
        )}
        {availability.replySuggestion && (
          <Button
            variant="secondary"
            disabled={!settings || replyBlocked || busy(reply)}
            {...(replyBlocked && { "aria-describedby": blockedId })}
            onClick={startReply}
            data-testid="ai-suggest-reply"
          >
            {t("ai.panel.suggestReply")}
          </Button>
        )}
      </div>
      <RunState run={summary} aiFunction="summary" context={t("ai.summary.failed")} />
      {summary.state.kind === "done" && <SummaryCard summary={summary.state.result} account={account} messageId={item.id} />}
      <RunState run={reply} aiFunction="reply_suggestion" context={t("ai.reply.failed")} />
      {reply.state.kind === "done" && <ReplyCard key={reply.state.requestId} suggestion={reply.state.result} />}
    </section>
  );
}

type AskScope = "account" | "all";

function AnswerCard({ answer, leftOut }: { answer: AiAnswer; leftOut: number }) {
  const { t, i18n } = useTranslation();
  const openSearch = useSearchStore((state) => state.openSearch);
  const setQuery = useSearchStore((state) => state.setQuery);
  const numbers = numberSources(answer.statements);
  // Decision 0027: the sources come from anywhere; the core names them by id.
  const items = useSourceItems([...numbers.keys()]);
  const labels = new Map<MessageId, string>();
  for (const [id, item] of items) labels.set(id, sourceLabel(t, i18n.language, item));
  return (
    <Card title={t("ai.ask.answerTitle")} data-testid="ai-answer">
      {answer.found ? (
        <ul className="flex flex-col gap-2">
          {answer.statements.map((claim, index) => (
            <li key={index} className="flex flex-col text-13 text-primary" data-testid="ai-claim">
              <span className="break-words whitespace-pre-wrap">
                <AiText segments={claim.segments} linkToken={answer.linkToken} />
              </span>
              <Sources sources={claim.sources} labels={labels} numbers={numbers} />
            </li>
          ))}
        </ul>
      ) : (
        <p className="text-13 text-primary" data-testid="ai-nothing-found">
          {t("ai.ask.nothingFound")}
        </p>
      )}
      <p className="text-12 text-secondary">{t("ai.ask.candidates", { count: answer.candidates })}</p>
      {answer.excluded + leftOut > 0 && (
        <p className="text-12 text-secondary" data-testid="ai-excluded">
          {t("ai.ask.excluded", { count: answer.excluded + leftOut })}
        </p>
      )}
      {answer.dropped > 0 && <p className="text-12 text-secondary">{t("ai.summary.dropped", { count: answer.dropped })}</p>}
      <AiLinkCheck linkToken={answer.linkToken} />
      {answer.query && (
        <div>
          <Button
            variant="quiet"
            onClick={() => {
              openSearch();
              setQuery(answer.query);
            }}
            data-testid="ai-show-search"
          >
            {t("ai.ask.showSearch")}
          </Button>
        </div>
      )}
      <TransferSummary transfers={answer.transfers} />
    </Card>
  );
}

/** The question the ask bar handed over with the command that unfolded the panel, else "". */
function handedOverQuestion(): string {
  const command = useAiPanelStore.getState().command;
  return command?.kind === "ask" ? (command.question ?? "") : "";
}

/** "Ask about your mail": the question becomes a search; the answer only cites what it found. */
function AskBox({ account, settings }: { account: Account | null; settings: AiSettingsInfo | undefined }) {
  const { t } = useTranslation();
  const questionId = useId();
  const blockedId = useId();
  const accounts = useAccounts().data ?? [];
  const unified = useUiStore((state) => state.unifiedInbox !== null);
  const [scope, setScope] = useState<AskScope>(unified || !account ? "all" : "account");
  const [question, setQuestion] = useState(handedOverQuestion);
  const [leftOut, setLeftOut] = useState(0);
  const ask = useAiRun<AiAnswer>("search");
  const profile = profileFor(settings, "search");
  const cloud = profile !== null && !isLocal(profile.locality);
  const permitted = new Set(settings?.accounts.filter((entry) => entry.cloudAllowed).map((entry) => entry.accountId) ?? []);
  const inScope = scope === "account" && account ? [account.id] : accounts.map((a) => a.id);
  const allowed = cloud ? inScope.filter((id) => permitted.has(id)) : inScope;
  const blocked = cloud && allowed.length === 0;
  const text = question.trim();
  const tooLong = [...text].length > AI_LIMITS.questionChars;
  const field = useRef<HTMLTextAreaElement>(null);
  const command = useAiPanelStore((state) => state.command);
  const clearCommand = useAiPanelStore((state) => state.clearCommand);
  useEffect(() => {
    if (command?.kind !== "ask") return;
    clearCommand();
    field.current?.focus();
  }, [command, clearCommand]);

  const askNow = (value: string) => {
    if (!value || [...value].length > AI_LIMITS.questionChars || blocked || !settings) return;
    // A cloud provider only gets candidates of accounts with cloud permission: the UI asks for
    // those alone and says how many it left out.
    const accountIds = cloud ? allowed : scope === "account" && account ? [account.id] : null;
    setLeftOut(cloud ? inScope.length - allowed.length : 0);
    // `run` shows the consent step first where the profile needs it: nothing is sent before.
    ask.run((requestId) => aiIpc.ask(requestId, value, accountIds), { function: "search", question: value });
  };
  const submit = (event?: FormEvent) => {
    event?.preventDefault();
    askNow(text);
  };
  // The reading pane's ask bar hands a question over: it unfolds the panel (this box starts with
  // the question in its field) and the question is asked here, once the settings are known.
  const [handedOver] = useState(handedOverQuestion);
  const askedHandOver = useRef(false);
  const settingsKnown = settings !== undefined;
  useEffect(() => {
    if (askedHandOver.current || handedOver === "" || !settingsKnown) return;
    askedHandOver.current = true;
    askNow(handedOver.trim());
    // Only the hand-over (once the settings are there) asks.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [handedOver, settingsKnown]);
  const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
      event.preventDefault();
      submit();
    }
  };
  return (
    <section aria-label={t("ai.ask.title")} className="flex flex-col gap-3 border-t border-border pt-3" data-testid="ai-ask">
      <RunState run={ask} aiFunction="search" context={t("ai.ask.failed")} />
      {ask.state.kind === "done" && <AnswerCard answer={ask.state.result} leftOut={leftOut} />}
      <form noValidate onSubmit={submit} className="flex flex-col gap-2">
        {account && accounts.length > 1 && (
          <ChoiceGroup
            label={t("ai.ask.scope")}
            value={scope}
            options={[
              { value: "account", label: t("ai.ask.scopeAccount") },
              { value: "all", label: t("ai.ask.scopeAll") },
            ]}
            onChange={setScope}
          />
        )}
        <label htmlFor={questionId} className="text-13 font-medium text-primary">
          {t("ai.ask.label")}
        </label>
        <textarea
          id={questionId}
          ref={field}
          value={question}
          rows={3}
          onChange={(event) => {
            setQuestion(event.target.value);
          }}
          onKeyDown={onKeyDown}
          aria-invalid={tooLong ? true : undefined}
          aria-describedby={blocked ? blockedId : undefined}
          placeholder={t("ai.ask.placeholder")}
          className="w-full resize-y rounded-control border border-border-strong bg-surface-raised px-3 py-2 font-sans text-13 text-primary select-text"
          data-testid="ai-question"
        />
        {tooLong && <p className="text-12 text-danger">{t("ai.ask.tooLong", { max: AI_LIMITS.questionChars })}</p>}
        {blocked && (
          <p id={blockedId} className="text-12 text-secondary" data-testid="ai-ask-not-permitted">
            {t("ai.ask.notPermitted")}
          </p>
        )}
        <div>
          <Button type="submit" disabled={!settings || !text || tooLong || blocked || ask.state.kind === "running"} data-testid="ai-ask-submit">
            {t("ai.ask.submit")}
          </Button>
        </div>
      </form>
    </section>
  );
}

/**
 * The AI panel (docs/DESIGN.md): right of the mail view, 380 px, collapsible, about the selected
 * thread. Results are cards; the question box is at the bottom. Not there while no AI function
 * is usable, so with AI off nothing in it can start a request.
 */
export function AiPanel() {
  const { t } = useTranslation();
  const view = useUiStore((state) => state.view);
  const availability = useAiAvailability();
  const settings = useAiSettings(panelAvailable(availability.data));
  const open = useAiPanelStore((state) => state.open);
  const setOpen = useAiPanelStore((state) => state.setOpen);
  const bound = useBoundMessage();
  const inMail = view.kind === "mail" || view.kind === "mailbox";
  // While a summary or reply waits for consent, the step gets the panel's height: the question
  // box is folded away (kept mounted, so a question typed meanwhile stays).
  const [threadConsent, setThreadConsent] = useState(false);
  // A thread command without a thread (the selection went away meanwhile) must not wait for the next one.
  const command = useAiPanelStore((state) => state.command);
  const clearCommand = useAiPanelStore((state) => state.clearCommand);
  const stale = command !== null && command.kind !== "ask" && !bound;
  useEffect(() => {
    if (stale) clearCommand();
  }, [stale, clearCommand]);
  if (!inMail || !open || !panelAvailable(availability.data) || !availability.data) return null;
  const data = availability.data;
  return (
    <aside
      aria-label={t("ai.panel.title")}
      className="flex w-(--ui-ai-panel-width) max-w-full shrink-0 flex-col border-s border-border bg-surface max-[1100px]:absolute max-[1100px]:inset-y-0 max-[1100px]:end-0 max-[1100px]:z-10 max-[1100px]:shadow-panel"
      data-testid="ai-panel"
    >
      <header className="flex h-12 shrink-0 items-center justify-between gap-2 border-b border-border px-4">
        <h2 className="flex items-center gap-2 text-14 font-semibold text-primary">
          <Sparkles aria-hidden className="size-(--ui-icon) text-accent" strokeWidth={1.75} />
          {t("ai.panel.title")}
        </h2>
        <IconButton
          icon={PanelRightClose}
          label={t("ai.panel.hide")}
          onClick={() => {
            setOpen(false);
          }}
        />
      </header>
      <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto p-4">
        <p className="text-12 text-secondary">{t("ai.panel.untrusted")}</p>
        {/* Stage 8: without its settings no request can start; say so instead of grey buttons. */}
        {settings.isError && (
          <ErrorNotice
            context={t("presentation.ai.settingsFailed")}
            error={settings.error}
            onRetry={() => {
              void settings.refetch();
            }}
            data-testid="ai-panel-settings-error"
          />
        )}
        {bound ? (
          <ThreadTools
            key={bound.item.id}
            item={bound.item}
            account={bound.account}
            availability={data}
            settings={settings.data}
            onConsentChange={setThreadConsent}
          />
        ) : (
          <div className="flex flex-col items-center gap-2 py-6 text-center" data-testid="ai-no-thread">
            <MessagesSquare aria-hidden className="size-8 text-tertiary" strokeWidth={1.5} />
            <p className="text-13 text-secondary">{t("ai.panel.chooseThread")}</p>
          </div>
        )}
        {!bound && <OpenReplyHintList />}
      </div>
      {data.search && (
        <div hidden={threadConsent} className="max-h-1/2 shrink-0 overflow-y-auto px-4 pb-4" data-testid="ai-ask-area">
          <AskBox account={bound?.account ?? null} settings={settings.data} />
        </div>
      )}
    </aside>
  );
}
