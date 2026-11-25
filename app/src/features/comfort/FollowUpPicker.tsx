import { CalendarClock, MailClock } from "lucide-react";
import { useEffect, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { ErrorNotice } from "../../components/ErrorNotice";
import type { FollowUpChoice, FollowUpTimes } from "../../services/ipc";
import { FloatingPanel } from "../actions/Pickers";
import { useFollowUpTimes } from "./queries";
import { formatSnoozeTime, parseLocalDateTime, snoozeTimeProblem, toLocalDateTimeValue, type SnoozeTimeProblem } from "./snooze";

export type FollowUpPreset = Exclude<FollowUpChoice["kind"], "at">;

/** The menu's presets in order, with their times. */
export function followUpPresets(times: FollowUpTimes): { kind: FollowUpPreset; at: number }[] {
  return [
    { kind: "tomorrow", at: times.tomorrow },
    { kind: "inThreeDays", at: times.inThreeDays },
    { kind: "nextWeek", at: times.nextWeek },
  ];
}

interface FollowUpPickerProps {
  count: number;
  onChoose: (choice: FollowUpChoice) => void;
  onClose: () => void;
  restoreFocus: () => void;
  /** For tests: the clock the custom time is checked against. */
  now?: () => number;
}

/**
 * Stage 20 (decision 0059): "Remind me if no reply by …": tomorrow, in three days, next week, or a
 * time of one's own. If no reply comes in the conversation by then, the message comes back above
 * the inbox; a reply ends the reminder. Local, like a snooze.
 */
export function FollowUpPicker({ count, onChoose, onClose, restoreFocus, now = Date.now }: FollowUpPickerProps) {
  const { t, i18n } = useTranslation();
  const times = useFollowUpTimes();
  const inputId = useId();
  const problemId = useId();
  const first = useRef<HTMLButtonElement>(null);
  const [custom, setCustom] = useState("");
  const [problem, setProblem] = useState<SnoozeTimeProblem | null>(null);
  const presets = times.data ? followUpPresets(times.data) : [];
  const ready = presets.length > 0;

  useEffect(() => {
    if (ready) first.current?.focus();
  }, [ready]);

  const chooseCustom = () => {
    const at = parseLocalDateTime(custom);
    const found = snoozeTimeProblem(at, now());
    setProblem(found);
    if (found === null && at !== null) onChoose({ kind: "at", at });
  };

  return (
    <FloatingPanel title={t("comfort.followUp.title", { count })} onClose={onClose} restoreFocus={restoreFocus} data-testid="follow-up-picker">
      <p className="text-13 text-secondary">{t("comfort.followUp.explanation")}</p>
      {times.isError && (
        <ErrorNotice
          context={t("comfort.followUp.timesFailed")}
          error={times.error}
          onRetry={() => {
            void times.refetch();
          }}
        />
      )}
      {ready && (
        <ul className="flex flex-col gap-1" aria-label={t("comfort.followUp.presets")}>
          {presets.map((preset, index) => (
            <li key={preset.kind}>
              <button
                ref={index === 0 ? first : undefined}
                type="button"
                onClick={() => {
                  onChoose({ kind: preset.kind });
                }}
                data-testid={`follow-up-${preset.kind}`}
                className="flex w-full items-center gap-3 rounded-control px-2 py-2 text-start text-14 text-primary hover:bg-hover focus-visible:outline-2 focus-visible:outline-focus-ring"
              >
                <MailClock aria-hidden className="size-(--ui-icon-small) shrink-0 text-secondary" strokeWidth={1.75} />
                <span className="flex-1">{t(`comfort.followUp.${preset.kind}`)}</span>
                <span className="text-13 text-secondary tabular-nums">{formatSnoozeTime(preset.at, now(), i18n.language)}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
      <form
        noValidate
        className="flex flex-col gap-2 border-t border-border pt-3"
        onSubmit={(event) => {
          event.preventDefault();
          chooseCustom();
        }}
      >
        <label htmlFor={inputId} className="flex items-center gap-2 text-13 font-medium text-primary">
          <CalendarClock aria-hidden className="size-(--ui-icon-small) text-secondary" strokeWidth={1.75} />
          {t("comfort.followUp.custom")}
        </label>
        <div className="flex items-center gap-2">
          <input
            id={inputId}
            type="datetime-local"
            value={custom}
            min={toLocalDateTimeValue(now() + 60_000)}
            aria-invalid={problem !== null}
            aria-describedby={problem ? problemId : undefined}
            onChange={(event) => {
              setCustom(event.target.value);
              setProblem(null);
            }}
            data-testid="follow-up-custom"
            className="min-w-0 flex-1 rounded-control border border-border bg-surface px-2 py-1 text-14 text-primary focus-visible:outline-2 focus-visible:outline-focus-ring"
          />
          <Button type="submit" data-testid="follow-up-custom-submit">
            {t("comfort.followUp.submit")}
          </Button>
        </div>
        {problem && (
          <p id={problemId} role="alert" className="text-12 text-danger" data-testid="follow-up-problem">
            {t(`comfort.snooze.problems.${problem}`)}
          </p>
        )}
      </form>
    </FloatingPanel>
  );
}
