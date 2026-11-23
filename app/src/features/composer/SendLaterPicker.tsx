import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { cx } from "../../components/cx";
import { ErrorNotice } from "../../components/ErrorNotice";
import { MAX_SCHEDULE_AHEAD_MS, type CoreError } from "../../services/ipc";

export interface SendLaterPickerProps {
  /** Start value (Unix ms), e.g. the current time of a send being moved; default: next full hour. */
  initial?: number | null;
  confirmLabel: string;
  busy?: boolean;
  /** Shown with `ErrorNotice` inside the picker (e.g. `invalid_argument`, `message_too_large`). */
  error?: CoreError | null;
  onChoose: (sendAt: number) => void;
  onCancel: () => void;
  /** Default "send-later-picker". */
  "data-testid"?: string;
}

export type SendLaterPresetId = "inOneHour" | "tomorrowMorning" | "nextMonday";

/** The hour of the morning presets (local time). */
const MORNING_HOUR = 8;

function atMorning(day: Date): number {
  const at = new Date(day);
  at.setHours(MORNING_HOUR, 0, 0, 0);
  return at.getTime();
}

/**
 * The suggested times, in the user's time zone: in one hour (to the minute), tomorrow at 08:00
 * and next Monday at 08:00 (left out when that is tomorrow).
 */
export function sendLaterPresets(now: Date): { id: SendLaterPresetId; at: number }[] {
  const inOneHour = new Date(now.getTime() + 3_600_000);
  if (inOneHour.getSeconds() > 0 || inOneHour.getMilliseconds() > 0) {
    inOneHour.setSeconds(0, 0);
    inOneHour.setMinutes(inOneHour.getMinutes() + 1);
  }
  const tomorrow = new Date(now);
  tomorrow.setDate(tomorrow.getDate() + 1);
  const monday = new Date(now);
  // 1 = Monday; always a Monday after today (a week ahead on a Monday).
  monday.setDate(monday.getDate() + (((8 - monday.getDay()) % 7) || 7));
  const presets: { id: SendLaterPresetId; at: number }[] = [
    { id: "inOneHour", at: inOneHour.getTime() },
    { id: "tomorrowMorning", at: atMorning(tomorrow) },
  ];
  const nextMonday = atMorning(monday);
  if (nextMonday !== atMorning(tomorrow)) presets.push({ id: "nextMonday", at: nextMonday });
  return presets;
}

/** What the core would refuse (`invalid_argument`): a time not in the future, or more than 366 days ahead. */
export function validateSendAt(sendAt: number, now: number): "past" | "tooFar" | null {
  if (!Number.isFinite(sendAt) || sendAt <= now) return "past";
  if (sendAt - now > MAX_SCHEDULE_AHEAD_MS) return "tooFar";
  return null;
}

const pad = (n: number) => String(n).padStart(2, "0");

/** Unix ms as the value of a `datetime-local` field (local time, minutes). */
export function toLocalInput(ms: number): string {
  const d = new Date(ms);
  return `${String(d.getFullYear())}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/** The value of a `datetime-local` field as Unix ms (local time); NaN when empty or malformed. */
export function fromLocalInput(value: string): number {
  const match = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})/.exec(value);
  if (!match) return Number.NaN;
  const [, y, mo, d, h, mi] = match.map(Number) as [number, number, number, number, number, number];
  return new Date(y, mo - 1, d, h, mi, 0, 0).getTime();
}

/** The clock at the moment of a click (not during render). */
function currentTime(): number {
  return Date.now();
}

function nextFullHour(now: number): number {
  const d = new Date(now);
  d.setMinutes(0, 0, 0);
  d.setHours(d.getHours() + 1);
  return d.getTime();
}

/**
 * "Send later": suggestions and a date and time of the user's choice, inline (no dialog: the CSP
 * allows no Radix Dialog or Popover). It says plainly that the message goes out only if the app
 * runs and is online at that time; there is no sending through a server. Escape cancels.
 */
export function SendLaterPicker({ initial, confirmLabel, busy = false, error = null, onChoose, onCancel, ...rest }: SendLaterPickerProps) {
  const { t, i18n } = useTranslation();
  const labelId = useId();
  const hintId = useId();
  const inputId = useId();
  const problemId = useId();
  const first = useRef<HTMLButtonElement>(null);
  const [now] = useState(() => Date.now());
  const [value, setValue] = useState(() => toLocalInput(initial ?? nextFullHour(now)));
  const [problem, setProblem] = useState<"empty" | "past" | "tooFar" | null>(null);
  const presets = sendLaterPresets(new Date(now));
  const format = new Intl.DateTimeFormat(i18n.language, {
    weekday: "short",
    day: "numeric",
    month: "short",
    hour: "2-digit",
    minute: "2-digit",
  });

  useEffect(() => {
    first.current?.focus();
  }, []);

  const choose = (sendAt: number) => {
    const invalid = validateSendAt(sendAt, currentTime());
    if (invalid) {
      setProblem(invalid);
      return;
    }
    setProblem(null);
    onChoose(sendAt);
  };
  const confirm = () => {
    const sendAt = fromLocalInput(value);
    if (Number.isNaN(sendAt)) {
      setProblem("empty");
      return;
    }
    choose(sendAt);
  };
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Escape") {
      // The picker's, not the composer's or the notice's: only the picker closes.
      event.preventDefault();
      event.stopPropagation();
      onCancel();
    } else if (event.key === "Enter" && event.target instanceof HTMLInputElement) {
      event.preventDefault();
      event.stopPropagation();
      confirm();
    }
  };

  return (
    <div
      role="group"
      aria-labelledby={labelId}
      aria-describedby={hintId}
      onKeyDown={onKeyDown}
      className="flex flex-col gap-3 rounded-card border border-border bg-surface p-3"
      data-testid={rest["data-testid"] ?? "send-later-picker"}
    >
      <div className="flex flex-col gap-1">
        <p id={labelId} className="text-13 font-semibold text-primary">
          {t("sendLater.label")}
        </p>
        <p id={hintId} className="text-12 text-secondary" data-testid="send-later-hint">
          {t("sendLater.hint")}
        </p>
      </div>
      <div role="group" aria-label={t("sendLater.presets")} className="flex flex-wrap gap-2">
        {presets.map((preset, index) => (
          <button
            key={preset.id}
            ref={index === 0 ? first : undefined}
            type="button"
            disabled={busy}
            onClick={() => {
              choose(preset.at);
            }}
            className="rounded-control border border-border-strong bg-surface px-2 py-1 text-13 text-primary hover:bg-hover disabled:opacity-50"
            data-testid={`send-later-preset-${preset.id}`}
          >
            {t(`sendLater.preset.${preset.id}`, { time: format.format(preset.at) })}
          </button>
        ))}
      </div>
      <div className="flex flex-wrap items-center gap-2">
        <label htmlFor={inputId} className="text-13 text-secondary">
          {t("sendLater.custom")}
        </label>
        <input
          id={inputId}
          type="datetime-local"
          value={value}
          min={toLocalInput(now)}
          max={toLocalInput(now + MAX_SCHEDULE_AHEAD_MS)}
          disabled={busy}
          onChange={(e) => {
            setValue(e.target.value);
            setProblem(null);
          }}
          aria-invalid={problem ? true : undefined}
          aria-describedby={problem ? problemId : undefined}
          className={cx(
            "h-8 rounded-control border bg-surface px-2 text-13 text-primary",
            problem ? "border-danger" : "border-border-strong",
          )}
          data-testid="send-later-input"
        />
      </div>
      {problem && (
        <p id={problemId} className="text-12 text-danger" data-testid="send-later-problem" data-problem={problem}>
          {t(`sendLater.invalid.${problem}`)}
        </p>
      )}
      {error && <ErrorNotice context={t("sendLater.failed")} error={error} data-testid="send-later-error" />}
      <div className="flex flex-wrap gap-2">
        <Button onClick={confirm} disabled={busy} data-testid="send-later-confirm">
          {confirmLabel}
        </Button>
        <Button variant="secondary" onClick={onCancel} data-testid="send-later-cancel">
          {t("sendLater.cancel")}
        </Button>
      </div>
    </div>
  );
}
