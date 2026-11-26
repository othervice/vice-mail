import { MailClock } from "lucide-react";
import { useId, useRef, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { cx } from "../../components/cx";
import { followUpPresets } from "../comfort/FollowUpPicker";
import { useFollowUpTimes } from "../comfort/queries";
import { formatSnoozeTime, parseLocalDateTime, snoozeTimeProblem, toLocalDateTimeValue, type SnoozeTimeProblem } from "../comfort/snooze";
import { ComposerMenu, type MenuEntry, type MenuSection } from "./ComposerMenu";

interface FollowUpOptionProps {
  /** The draft's reminder (Unix ms), null for none. */
  value: number | null;
  disabled: boolean;
  onChange: (at: number | null) => void;
  /** Classes of the icon button (the composer's send row). */
  className: (open: boolean) => string;
  /** For tests: the clock the custom time is checked against. */
  now?: () => number;
}

/**
 * Stage 20 (decision 0059): "Remind me if no reply by …": tomorrow, in three days, next week or a
 * time of one's own. Recorded once the server accepted the message; a reply in the conversation
 * ends it. Stage 23 (decision 0086): an icon in the composer's send row with the choices as a
 * menu; the icon shows in the accent while a reminder is set, and its name says when. "Pick a
 * date and time…" opens a small panel above the row (Escape closes it, not the composer).
 */
export function FollowUpOption({ value, disabled, onChange, className, now = Date.now }: FollowUpOptionProps) {
  const { t, i18n } = useTranslation();
  const inputId = useId();
  const problemId = useId();
  const panelId = useId();
  const times = useFollowUpTimes();
  const wrapper = useRef<HTMLDivElement>(null);
  const [customOpen, setCustomOpen] = useState(false);
  const [custom, setCustom] = useState("");
  const [problem, setProblem] = useState<SnoozeTimeProblem | null>(null);
  const presets = times.data ? followUpPresets(times.data) : [];
  const preset = value === null ? undefined : presets.find((entry) => entry.at === value);

  const closeCustom = () => {
    setCustomOpen(false);
    setProblem(null);
    wrapper.current?.querySelector<HTMLButtonElement>('[data-testid="composer-follow-up"]')?.focus();
  };
  const applyCustom = () => {
    const at = parseLocalDateTime(custom);
    const found = snoozeTimeProblem(at, now());
    setProblem(found);
    if (found === null && at !== null) {
      onChange(at);
      closeCustom();
    }
  };
  const onPanelKey = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      closeCustom();
    } else if (event.key === "Enter" && event.target instanceof HTMLInputElement) {
      // Enter in the field sets the time (not "send": that needs Cmd/Ctrl).
      if (event.metaKey || event.ctrlKey) return;
      event.preventDefault();
      event.stopPropagation();
      applyCustom();
    }
  };

  const sections = (openedAt: number): MenuSection[] => [
    {
      id: "choice",
      entries: [
        {
          kind: "radio",
          id: "none",
          label: t("composer.followUp.none"),
          checked: value === null,
          onSelect: () => {
            setCustomOpen(false);
            onChange(null);
          },
          "data-testid": "composer-follow-up-none",
        },
        ...presets.map(
          (entry): MenuEntry => ({
            kind: "radio",
            id: entry.kind,
            label: t("composer.followUp.preset", { name: t(`comfort.followUp.${entry.kind}`), time: formatSnoozeTime(entry.at, openedAt, i18n.language) }),
            checked: preset?.kind === entry.kind,
            onSelect: () => {
              setCustomOpen(false);
              onChange(entry.at);
            },
            "data-testid": `composer-follow-up-${entry.kind}`,
          }),
        ),
        ...(value !== null && preset === undefined
          ? [
              {
                kind: "radio",
                id: "chosen",
                label: formatSnoozeTime(value, openedAt, i18n.language),
                checked: true,
                onSelect: () => undefined,
                "data-testid": "composer-follow-up-chosen",
              } satisfies MenuEntry,
            ]
          : []),
      ],
    },
    {
      id: "custom",
      entries: [
        {
          kind: "action",
          id: "custom",
          label: t("composer.followUp.custom"),
          onSelect: () => {
            setCustom(value === null ? "" : toLocalDateTimeValue(value));
            setProblem(null);
            setCustomOpen(true);
          },
          "data-testid": "composer-follow-up-other",
        },
      ],
    },
  ];

  const name = value === null ? t("composer.followUp.button") : t("composer.followUp.buttonSet", { time: formatSnoozeTime(value, now(), i18n.language) });
  return (
    <div ref={wrapper} className="relative flex">
      <ComposerMenu
        label={t("composer.followUp.button")}
        name={name}
        sections={sections}
        disabled={disabled}
        className={className}
        data-active={value !== null}
        data-testid="composer-follow-up"
      >
        <MailClock aria-hidden className="size-(--ui-icon)" strokeWidth={1.75} />
        {value !== null && <span aria-hidden className="absolute end-1 top-1 size-2 rounded-full bg-accent-fill" />}
      </ComposerMenu>
      {customOpen && (
        <div
          id={panelId}
          role="group"
          aria-labelledby={`${panelId}-label`}
          onKeyDown={onPanelKey}
          className="absolute start-0 bottom-full z-30 mb-2 flex w-max max-w-[calc(100vw-var(--ui-space-4))] flex-col gap-2 rounded-card border border-border bg-surface-raised p-3 shadow-floating"
          data-testid="composer-follow-up-panel"
        >
          <label id={`${panelId}-label`} htmlFor={inputId} className="text-13 font-semibold text-primary">
            {t("composer.followUp.customLabel")}
          </label>
          <div className="flex flex-wrap items-center gap-2">
            <input
              id={inputId}
              type="datetime-local"
              autoFocus
              value={custom}
              min={toLocalDateTimeValue(now() + 60_000)}
              aria-invalid={problem !== null}
              aria-describedby={problem ? problemId : undefined}
              onChange={(event) => {
                setCustom(event.target.value);
                setProblem(null);
              }}
              data-testid="composer-follow-up-custom"
              className={cx("h-8 rounded-control border bg-surface px-2 text-13 text-primary", problem ? "border-danger" : "border-border-strong")}
            />
            <Button onClick={applyCustom} data-testid="composer-follow-up-set">
              {t("composer.followUp.set")}
            </Button>
            <Button variant="secondary" onClick={closeCustom} data-testid="composer-follow-up-cancel">
              {t("composer.followUp.cancel")}
            </Button>
          </div>
          {problem && (
            <p id={problemId} role="alert" className="text-12 text-danger" data-testid="composer-follow-up-problem">
              {t(`comfort.snooze.problems.${problem}`)}
            </p>
          )}
        </div>
      )}
    </div>
  );
}
