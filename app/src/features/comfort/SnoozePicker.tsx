import { CalendarClock, Clock } from "lucide-react";
import { useEffect, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { ErrorNotice } from "../../components/ErrorNotice";
import type { SnoozeChoice } from "../../services/ipc";
import { FloatingPanel } from "../actions/Pickers";
import { useSnoozeTimes } from "./queries";
import {
  formatSnoozeTime,
  parseLocalDateTime,
  snoozePresets,
  snoozeTimeProblem,
  toLocalDateTimeValue,
  type SnoozeTimeProblem,
} from "./snooze";

interface SnoozePickerProps {
  count: number;
  onChoose: (choice: SnoozeChoice) => void;
  onClose: () => void;
  restoreFocus: () => void;
  /** For tests: the clock the custom time is checked against. */
  now?: () => number;
}

/**
 * The snooze menu (docs/stages/09): later today, tomorrow, the weekend, next week, or a time of
 * one's own. The times come from the core (system time zone, daylight saving), asked anew each time
 * the menu opens. The message leaves the inbox and comes back on top at that time; nothing moves
 * on the server.
 */
export function SnoozePicker({ count, onChoose, onClose, restoreFocus, now = Date.now }: SnoozePickerProps) {
  const { t, i18n } = useTranslation();
  const times = useSnoozeTimes();
  const inputId = useId();
  const problemId = useId();
  const first = useRef<HTMLButtonElement>(null);
  const [custom, setCustom] = useState("");
  const [problem, setProblem] = useState<SnoozeTimeProblem | null>(null);
  const presets = times.data ? snoozePresets(times.data) : [];
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
    <FloatingPanel
      title={t("comfort.snooze.title", { count })}
      onClose={onClose}
      restoreFocus={restoreFocus}
      data-testid="snooze-picker"
    >
      {times.isError && (
        <ErrorNotice
          context={t("comfort.snooze.timesFailed")}
          error={times.error}
          onRetry={() => {
            void times.refetch();
          }}
        />
      )}
      {ready && (
        <ul className="flex flex-col gap-1" aria-label={t("comfort.snooze.presets")}>
          {presets.map((preset, index) => (
            <li key={preset.kind}>
              <button
                ref={index === 0 ? first : undefined}
                type="button"
                onClick={() => {
                  onChoose({ kind: preset.kind });
                }}
                data-testid={`snooze-${preset.kind}`}
                className="flex w-full items-center gap-3 rounded-control px-2 py-1.5 text-start text-14 text-primary hover:bg-hover focus-visible:outline-2 focus-visible:outline-focus-ring"
              >
                <Clock aria-hidden className="size-(--ui-icon-small) shrink-0 text-secondary" strokeWidth={1.75} />
                <span className="flex-1">{t(`comfort.snooze.${preset.kind}`)}</span>
                <span className="text-13 text-secondary tabular-nums">{formatSnoozeTime(preset.at, now(), i18n.language)}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
      <form
        // The app's own check below, with a message screen readers hear, instead of the browser's bubble.
        noValidate
        className="flex flex-col gap-2 border-t border-border pt-3"
        onSubmit={(event) => {
          event.preventDefault();
          chooseCustom();
        }}
      >
        <label htmlFor={inputId} className="flex items-center gap-2 text-13 font-medium text-primary">
          <CalendarClock aria-hidden className="size-(--ui-icon-small) text-secondary" strokeWidth={1.75} />
          {t("comfort.snooze.custom")}
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
            data-testid="snooze-custom"
            className="min-w-0 flex-1 rounded-control border border-border bg-surface px-2 py-1 text-14 text-primary focus-visible:outline-2 focus-visible:outline-focus-ring"
          />
          <Button type="submit" data-testid="snooze-custom-submit">
            {t("comfort.snooze.submit")}
          </Button>
        </div>
        {problem && (
          <p id={problemId} role="alert" className="text-12 text-danger" data-testid="snooze-problem">
            {t(`comfort.snooze.problems.${problem}`)}
          </p>
        )}
      </form>
    </FloatingPanel>
  );
}
