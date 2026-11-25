import { MailClock } from "lucide-react";
import { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { followUpPresets, type FollowUpPreset } from "../comfort/FollowUpPicker";
import { useFollowUpTimes } from "../comfort/queries";
import { formatSnoozeTime, parseLocalDateTime, snoozeTimeProblem, toLocalDateTimeValue, type SnoozeTimeProblem } from "../comfort/snooze";

interface FollowUpOptionProps {
  /** The draft's reminder (Unix ms), null for none. */
  value: number | null;
  disabled: boolean;
  onChange: (at: number | null) => void;
  /** For tests: the clock the custom time is checked against. */
  now?: () => number;
}

type Choice = "none" | FollowUpPreset | "custom" | "chosen";

const NONE: Choice = "none";
const CHOSEN: Choice = "chosen";
const CUSTOM: Choice = "custom";

/**
 * Stage 20 (decision 0059): "Remind me if no reply by …" in the composer's send options:
 * tomorrow, in three days, next week or a time of one's own. Recorded once the server accepted the
 * message; a reply in the conversation ends it.
 */
export function FollowUpOption({ value, disabled, onChange, now = Date.now }: FollowUpOptionProps) {
  const { t, i18n } = useTranslation();
  const selectId = useId();
  const inputId = useId();
  const problemId = useId();
  const times = useFollowUpTimes();
  const [customOpen, setCustomOpen] = useState(false);
  const [custom, setCustom] = useState("");
  const [problem, setProblem] = useState<SnoozeTimeProblem | null>(null);
  const presets = times.data ? followUpPresets(times.data) : [];
  const preset = value === null ? undefined : presets.find((entry) => entry.at === value);
  const selected: Choice = customOpen ? "custom" : value === null ? "none" : (preset?.kind ?? "chosen");

  const choose = (choice: Choice) => {
    setProblem(null);
    if (choice === "custom") {
      setCustomOpen(true);
      setCustom(value === null ? "" : toLocalDateTimeValue(value));
      return;
    }
    setCustomOpen(false);
    if (choice === "none") onChange(null);
    const at = presets.find((entry) => entry.kind === choice)?.at;
    if (at !== undefined) onChange(at);
  };
  const applyCustom = () => {
    const at = parseLocalDateTime(custom);
    const found = snoozeTimeProblem(at, now());
    setProblem(found);
    if (found === null && at !== null) {
      setCustomOpen(false);
      onChange(at);
    }
  };

  return (
    <div className="flex flex-wrap items-center gap-2" data-testid="composer-follow-up">
      <MailClock aria-hidden className="size-(--ui-icon-small) shrink-0" strokeWidth={1.75} />
      <label htmlFor={selectId}>{t("composer.followUp.label")}</label>
      <select
        id={selectId}
        value={selected}
        disabled={disabled}
        onChange={(event) => {
          choose(event.target.value as Choice);
        }}
        className="h-8 rounded-control border border-border-strong bg-surface-raised px-2 text-13 text-primary"
        data-testid="composer-follow-up-choice"
      >
        <option value={NONE}>{t("composer.followUp.none")}</option>
        {presets.map((entry) => (
          <option key={entry.kind} value={entry.kind}>
            {t("composer.followUp.preset", { name: t(`comfort.followUp.${entry.kind}`), time: formatSnoozeTime(entry.at, now(), i18n.language) })}
          </option>
        ))}
        {selected === "chosen" && value !== null && <option value={CHOSEN}>{formatSnoozeTime(value, now(), i18n.language)}</option>}
        <option value={CUSTOM}>{t("composer.followUp.custom")}</option>
      </select>
      {customOpen && (
        <span className="flex items-center gap-2">
          <label htmlFor={inputId} className="sr-only">
            {t("composer.followUp.customLabel")}
          </label>
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
            data-testid="composer-follow-up-custom"
            className="h-8 rounded-control border border-border bg-surface px-2 text-13 text-primary"
          />
          <Button variant="secondary" onClick={applyCustom} data-testid="composer-follow-up-set">
            {t("composer.followUp.set")}
          </Button>
        </span>
      )}
      {problem && (
        <span id={problemId} role="alert" className="text-12 text-danger" data-testid="composer-follow-up-problem">
          {t(`comfort.snooze.problems.${problem}`)}
        </span>
      )}
    </div>
  );
}
