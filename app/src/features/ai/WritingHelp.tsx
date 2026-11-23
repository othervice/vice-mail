import type { Editor } from "@tiptap/react";
import { WandSparkles } from "lucide-react";
import { useId, useRef, useState, type MouseEvent } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { cx } from "../../components/cx";
import { ErrorNotice } from "../../components/ErrorNotice";
import { AI_LIMITS, aiIpc, type AiTargetLanguage, type AiTone, type AiWritingResult, type AiWritingTask } from "../../services/ipc";
import { AiProgress, TransferSummary } from "./AiTransfer";
import { AiLinkCheck, AiText } from "./AiText";
import { ConsentStep } from "./ConsentStep";
import { useAiAvailability } from "./queries";
import { useAiRun } from "./useAiRun";

export const TONES: readonly AiTone[] = ["formal", "friendly", "neutral", "direct"];
const BASIC_TASKS = ["improve", "shorten", "rephrase"] as const;
export const TARGET_LANGUAGES: readonly AiTargetLanguage[] = ["en", "de", "fr", "es", "it", "nl", "pt", "pl"];

/** Where the passage came from, to put the suggestion back exactly there. */
export type Selection =
  | { kind: "text"; field: HTMLTextAreaElement; start: number; end: number; passage: string }
  | { kind: "rich"; editor: Editor; from: number; to: number; passage: string };

/** The rich editor's DOM node carries its editor (set by tiptap). */
type EditorElement = HTMLElement & { editor?: Editor };

/** The composer's body around `anchor`: the rich editor or the plain-text field. */
export function readSelection(anchor: HTMLElement | null): Selection | null {
  const composer = anchor?.closest('[data-testid="composer"]');
  if (!composer) return null;
  const field = composer.querySelector<HTMLTextAreaElement>('textarea[data-testid="composer-text-body"]');
  if (field) {
    const start = field.selectionStart;
    const end = field.selectionEnd;
    return { kind: "text", field, start, end, passage: field.value.slice(start, end) };
  }
  const rich = composer.querySelector<EditorElement>('[data-testid="composer-rich-body"]');
  const editor = rich?.editor;
  if (!editor || editor.isDestroyed) return null;
  const { from, to } = editor.state.selection;
  return { kind: "rich", editor, from, to, passage: editor.state.doc.textBetween(from, to, "\n") };
}

/** Whether the selected text is still what was sent, so applying replaces the right words. */
function unchanged(selection: Selection): boolean {
  if (selection.kind === "text") {
    return selection.field.isConnected && selection.field.value.slice(selection.start, selection.end) === selection.passage;
  }
  const { editor, from, to, passage } = selection;
  if (editor.isDestroyed || to > editor.state.doc.content.size) return false;
  return editor.state.doc.textBetween(from, to, "\n") === passage;
}

/**
 * Puts the suggestion in place of the selection, as plain text: in the rich editor as text nodes
 * and line breaks (never parsed as HTML), in the text field through its own input event so the
 * composer saves it like typing.
 */
export function applySuggestion(selection: Selection, text: string): void {
  if (selection.kind === "text") {
    const { field, start, end } = selection;
    const next = field.value.slice(0, start) + text + field.value.slice(end);
    // The prototype's setter, so React's value tracker sees a change and fires `onChange`.
    const descriptor = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value");
    descriptor?.set?.call(field, next);
    field.dispatchEvent(new Event("input", { bubbles: true }));
    field.focus();
    field.setSelectionRange(start, start + text.length);
    return;
  }
  const content = text
    .split("\n")
    .flatMap((line, index) => [...(index > 0 ? [{ type: "hardBreak" }] : []), ...(line ? [{ type: "text", text: line }] : [])]);
  selection.editor.chain().focus().insertContentAt({ from: selection.from, to: selection.to }, content).run();
}

type Problem = "empty" | "tooLong" | "changed" | "noEditor";

/**
 * Writing help in the composer (stage 13): improve, shorten, rephrase, change the tone or
 * translate the selected text. The answer is a preview; the draft changes only on "Apply".
 * Shown only while the writing help has a usable profile.
 */
export function WritingHelp({ disabled = false }: { disabled?: boolean }) {
  const { t } = useTranslation();
  const availability = useAiAvailability();
  const root = useRef<HTMLDivElement>(null);
  const toolsId = useId();
  const toneId = useId();
  const languageId = useId();
  const [open, setOpen] = useState(false);
  const [tone, setTone] = useState<AiTone>("friendly");
  const [language, setLanguage] = useState<AiTargetLanguage>("en");
  const [problem, setProblem] = useState<Problem | null>(null);
  const [target, setTarget] = useState<Selection | null>(null);
  const run = useAiRun<AiWritingResult>("writing_help");
  if (!availability.data?.writingHelp) return null;

  const start = (task: AiWritingTask) => {
    setProblem(null);
    const selection = readSelection(root.current);
    if (!selection) {
      setProblem("noEditor");
      return;
    }
    if (selection.passage.trim() === "") {
      setProblem("empty");
      return;
    }
    if ([...selection.passage].length > AI_LIMITS.passageChars) {
      setProblem("tooLong");
      return;
    }
    setTarget(selection);
    run.run((requestId) => aiIpc.writingHelp(requestId, selection.passage, task), { function: "writing_help", passage: selection.passage, task });
  };
  const discard = () => {
    run.reset();
    setTarget(null);
    setProblem(null);
  };
  const apply = (text: string) => {
    if (!target || !unchanged(target)) {
      setProblem("changed");
      return;
    }
    applySuggestion(target, text);
    run.reset();
    setTarget(null);
  };
  // Keeps the editor's selection when a tool is clicked with the mouse.
  const keepSelection = (event: MouseEvent) => {
    event.preventDefault();
  };
  const running = run.state.kind === "running" || run.state.kind === "consent";
  const toolClass = "h-7 rounded-control px-2 text-12 font-medium text-primary hover:bg-hover disabled:opacity-50";
  const state = run.state;

  return (
    <div ref={root} className="flex shrink-0 flex-col gap-2 border-t border-border px-4 py-2" data-testid="ai-writing-help">
      <div>
        <button
          type="button"
          aria-expanded={open}
          aria-controls={toolsId}
          disabled={disabled}
          onClick={() => {
            setOpen((value) => !value);
          }}
          className="inline-flex h-7 items-center gap-2 rounded-control px-2 text-12 font-medium text-secondary hover:bg-hover hover:text-primary disabled:opacity-50"
          data-testid="ai-writing-toggle"
        >
          <WandSparkles aria-hidden className="size-(--ui-icon)" strokeWidth={1.75} />
          {t("ai.writing.title")}
        </button>
      </div>
      {open && (
        <div id={toolsId} className="flex flex-col gap-2">
          <p className="text-12 text-secondary">{t("ai.writing.hint")}</p>
          <div role="toolbar" aria-label={t("ai.writing.title")} className="flex flex-wrap items-center gap-1">
            {BASIC_TASKS.map((task) => (
              <button
                key={task}
                type="button"
                disabled={disabled || running}
                onMouseDown={keepSelection}
                onClick={() => {
                  start({ type: task });
                }}
                className={cx(toolClass, "border border-border-strong")}
                data-testid={`ai-writing-${task}`}
              >
                {t(`ai.writing.tasks.${task}`)}
              </button>
            ))}
            <span className="flex items-center gap-1">
              <label htmlFor={toneId} className="sr-only">
                {t("ai.writing.tone")}
              </label>
              <select
                id={toneId}
                value={tone}
                disabled={disabled || running}
                onChange={(event) => {
                  const next = TONES.find((value) => value === event.target.value);
                  if (next) setTone(next);
                }}
                className="h-7 rounded-control border border-border-strong bg-surface-raised px-1 text-12 text-primary"
                data-testid="ai-writing-tone"
              >
                {TONES.map((value) => (
                  <option key={value} value={value}>
                    {t(`ai.writing.tones.${value}`)}
                  </option>
                ))}
              </select>
              <button
                type="button"
                disabled={disabled || running}
                onMouseDown={keepSelection}
                onClick={() => {
                  start({ type: "tone", tone });
                }}
                className={cx(toolClass, "border border-border-strong")}
                data-testid="ai-writing-tone-run"
              >
                {t("ai.writing.applyTone")}
              </button>
            </span>
            <span className="flex items-center gap-1">
              <label htmlFor={languageId} className="sr-only">
                {t("ai.writing.language")}
              </label>
              <select
                id={languageId}
                value={language}
                disabled={disabled || running}
                onChange={(event) => {
                  const next = TARGET_LANGUAGES.find((value) => value === event.target.value);
                  if (next) setLanguage(next);
                }}
                className="h-7 rounded-control border border-border-strong bg-surface-raised px-1 text-12 text-primary"
                data-testid="ai-writing-language"
              >
                {TARGET_LANGUAGES.map((value) => (
                  <option key={value} value={value}>
                    {t(`ai.writing.languages.${value}`)}
                  </option>
                ))}
              </select>
              <button
                type="button"
                disabled={disabled || running}
                onMouseDown={keepSelection}
                onClick={() => {
                  start({ type: "translate", language });
                }}
                className={cx(toolClass, "border border-border-strong")}
                data-testid="ai-writing-translate"
              >
                {t("ai.writing.translate")}
              </button>
            </span>
          </div>
          {problem && (
            <p role="alert" className="text-12 text-danger" data-testid="ai-writing-problem">
              {t(`ai.writing.problems.${problem}`, { max: AI_LIMITS.passageChars })}
            </p>
          )}
          {state.kind === "consent" && (
            <ConsentStep
              aiFunction="writing_help"
              profile={state.profile}
              input={state.input}
              error={run.agreeError}
              onAgree={() => void run.agree()}
              onCancel={discard}
            />
          )}
          {state.kind === "running" && <AiProgress requestId={state.requestId} onCancel={run.cancel} />}
          {state.kind === "cancelled" && (
            <p role="status" className="text-12 text-secondary">
              {t("ai.progress.cancelled")}
            </p>
          )}
          {state.kind === "error" && <ErrorNotice context={t("ai.writing.failed")} error={state.error} data-testid="ai-writing-error" />}
          {state.kind === "done" && target && (
            <section aria-label={t("ai.writing.preview")} className="flex flex-col gap-2 rounded-card border border-accent bg-canvas p-3" data-testid="ai-writing-preview">
              <h3 className="text-13 font-semibold text-primary">{t("ai.writing.preview")}</h3>
              <div className="grid gap-2 sm:grid-cols-2">
                <div className="flex flex-col gap-1">
                  <p className="text-12 text-secondary">{t("ai.writing.before")}</p>
                  <p className="text-13 break-words whitespace-pre-wrap text-secondary" data-testid="ai-writing-before">
                    {target.passage}
                  </p>
                </div>
                <div className="flex flex-col gap-1">
                  <p className="text-12 text-secondary">{t("ai.writing.after")}</p>
                  <p className="text-13 break-words whitespace-pre-wrap text-primary" data-testid="ai-writing-after">
                    <AiText segments={state.result.segments} linkToken={state.result.linkToken} />
                  </p>
                </div>
              </div>
              <div className="flex flex-wrap gap-2">
                <Button
                  onClick={() => {
                    apply(state.result.text);
                  }}
                  data-testid="ai-writing-apply"
                >
                  {t("ai.writing.apply")}
                </Button>
                <Button variant="secondary" onClick={discard} data-testid="ai-writing-discard">
                  {t("ai.writing.discard")}
                </Button>
              </div>
              <AiLinkCheck linkToken={state.result.linkToken} />
              <TransferSummary transfers={[state.result.transfer]} />
            </section>
          )}
        </div>
      )}
    </div>
  );
}
