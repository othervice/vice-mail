import { Plus, RotateCcw, X } from "lucide-react";
import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { cx } from "../../components/cx";
import { Notice } from "../../components/Notice";
import { SettingsSection } from "../../components/SettingsSection";
import { Tooltip } from "../../components/Tooltip";
import { useBoundShortcuts, useShortcutStore } from "../../stores/shortcuts";
import { ariaShortcut, formatShortcut, shortcutPlatform, type Shortcut, type ShortcutPlatform } from "../actions/shortcuts";
import { useAppInfo } from "../app-info/queries";
import { COMMAND_IDS, isActionId, shortcutFromEvent, type CommandId, type ShortcutProblem } from "./bindings";
import { useCommandLabel } from "./labels";

type Status = { tone: "ok" | "problem"; text: string } | null;

const MOD_NAMES: Record<ShortcutPlatform, { mod: string; other: string }> = {
  macos: { mod: "⌘", other: "Control" },
  windows: { mod: "Ctrl", other: "Windows" },
};

interface RowProps {
  id: CommandId;
  platform: ShortcutPlatform;
  recording: boolean;
  onRecord: (id: CommandId | null) => void;
  onStatus: (status: Status) => void;
}

/** One command: its shortcuts as removable chips, "Add" (records the next key press), "Reset". */
function CommandRow({ id, platform, recording, onRecord, onStatus }: RowProps) {
  const { t } = useTranslation();
  const label = useCommandLabel()(id);
  const shortcuts = useBoundShortcuts(platform)(id);
  const changed = useShortcutStore((state) => state.overrides[platform][id] !== undefined);
  const addShortcut = useShortcutStore((state) => state.addShortcut);
  const removeShortcut = useShortcutStore((state) => state.removeShortcut);
  const resetCommand = useShortcutStore((state) => state.resetCommand);
  const labelOf = useCommandLabel();
  const names = MOD_NAMES[platform];
  const addRef = useRef<HTMLButtonElement>(null);
  const wasRecording = useRef(false);

  // The recorder disappears after a key was taken or Escape: the focus goes back to "Add"
  // (unless the user moved it elsewhere, which also ends the recording).
  useEffect(() => {
    const active = document.activeElement;
    if (wasRecording.current && !recording && (!active || active === document.body)) addRef.current?.focus();
    wasRecording.current = recording;
  }, [recording]);

  const problemText = (problem: ShortcutProblem, shortcut: Shortcut) => {
    const shown = formatShortcut(shortcut, platform);
    if (problem.kind === "conflict") {
      return t("keyboard.settings.problems.conflict", { shortcut: shown, other: labelOf(problem.other) });
    }
    return t(`keyboard.settings.problems.${problem.kind}`, { shortcut: shown, label });
  };

  const onRecordKey = (event: KeyboardEvent<HTMLButtonElement>) => {
    // Every key belongs to the recorder now; nothing else may act on it.
    event.preventDefault();
    event.stopPropagation();
    if (event.key === "Escape" && !event.shiftKey && !event.metaKey && !event.ctrlKey && !event.altKey) {
      onRecord(null);
      onStatus(null);
      return;
    }
    const recorded = shortcutFromEvent(event.nativeEvent, platform);
    if (recorded.kind === "pending") return;
    if (recorded.kind !== "shortcut") {
      onStatus({ tone: "problem", text: t(`keyboard.settings.problems.${recorded.kind}`, names) });
      return;
    }
    const problem = addShortcut(platform, id, recorded.shortcut);
    if (problem) {
      onStatus({ tone: "problem", text: problemText(problem, recorded.shortcut) });
      return;
    }
    onRecord(null);
    onStatus({ tone: "ok", text: t("keyboard.settings.added", { shortcut: formatShortcut(recorded.shortcut, platform), label }) });
  };

  return (
    <li className="flex flex-wrap items-center gap-x-3 gap-y-1 border-b border-border py-2" data-testid={`shortcut-row-${id}`}>
      <span className="min-w-0 flex-1 text-14 text-primary">
        {label}
        {changed && <span className="ms-2 text-12 text-secondary">{t("keyboard.settings.changed")}</span>}
      </span>
      <ul aria-label={t("keyboard.settings.shortcuts")} className="flex flex-wrap items-center gap-1">
        {shortcuts.map((shortcut) => {
          const shown = formatShortcut(shortcut, platform);
          return (
            <li key={ariaShortcut(shortcut, platform)} className="flex items-center rounded-control border border-border bg-canvas ps-2">
              <kbd className="font-sans text-12 text-primary">{shown}</kbd>
              <Tooltip label={t("keyboard.settings.remove", { shortcut: shown, label })}>
                <button
                  type="button"
                  aria-label={t("keyboard.settings.remove", { shortcut: shown, label })}
                  onClick={() => {
                    removeShortcut(platform, id, shortcut);
                    onStatus({ tone: "ok", text: t("keyboard.settings.removed", { shortcut: shown, label }) });
                  }}
                  className="flex size-6 items-center justify-center rounded-control text-secondary hover:bg-hover hover:text-primary"
                >
                  <X aria-hidden className="size-3" strokeWidth={2} />
                </button>
              </Tooltip>
            </li>
          );
        })}
      </ul>
      {recording ? (
        <button
          type="button"
          // The recorder takes the focus at once, so the next key press is recorded.
          autoFocus
          aria-label={t("keyboard.settings.recording", { label })}
          onKeyDown={onRecordKey}
          onBlur={() => {
            onRecord(null);
          }}
          className="h-7 rounded-control border border-dashed border-accent px-2 text-12 text-accent"
          data-testid="shortcut-recorder"
        >
          {t("keyboard.settings.recordingShort")}
        </button>
      ) : (
        <Tooltip label={t("keyboard.settings.add", { label })}>
          <button
            ref={addRef}
            type="button"
            aria-label={t("keyboard.settings.add", { label })}
            onClick={() => {
              onStatus(null);
              onRecord(id);
            }}
            className="flex h-7 items-center gap-1 rounded-control px-2 text-12 font-medium text-accent hover:bg-hover"
          >
            <Plus aria-hidden className="size-3.5" strokeWidth={2} />
            {t("keyboard.settings.addShort")}
          </button>
        </Tooltip>
      )}
      {changed && (
        <Tooltip label={t("keyboard.settings.reset", { label })}>
          <button
            type="button"
            aria-label={t("keyboard.settings.reset", { label })}
            onClick={() => {
              resetCommand(platform, id);
              onStatus({ tone: "ok", text: t("keyboard.settings.resetDone", { label }) });
            }}
            className="flex h-7 items-center gap-1 rounded-control px-2 text-12 font-medium text-secondary hover:bg-hover hover:text-primary"
          >
            <RotateCcw aria-hidden className="size-3.5" strokeWidth={2} />
            {t("keyboard.settings.resetShort")}
          </button>
        </Tooltip>
      )}
    </li>
  );
}

/** Settings → Keyboard (stage 8): every command of the action directory with its shortcuts. */
export function KeyboardSettings() {
  const { t } = useTranslation();
  const platform = shortcutPlatform(useAppInfo().data?.platform);
  const [recording, setRecording] = useState<CommandId | null>(null);
  const [status, setStatus] = useState<Status>(null);
  const saveFailed = useShortcutStore((state) => state.saveFailed);
  const resetAll = useShortcutStore((state) => state.resetAll);
  const anyChanged = useShortcutStore((state) => Object.keys(state.overrides[platform]).length > 0);
  const descriptionId = useId();
  const groups = [
    { id: "actions" as const, commands: COMMAND_IDS.filter(isActionId) },
    { id: "commands" as const, commands: COMMAND_IDS.filter((id) => !isActionId(id)) },
  ];

  return (
    <SettingsSection title={t("keyboard.settings.title")}>
      <p id={descriptionId} className="text-13 text-secondary">
        {t("keyboard.settings.description")}
      </p>
      <div className="flex flex-wrap items-center gap-3">
        <Button
          variant="secondary"
          disabled={!anyChanged}
          onClick={() => {
            resetAll(platform);
            setStatus({ tone: "ok", text: t("keyboard.settings.resetAllDone") });
          }}
        >
          {t("keyboard.settings.resetAll")}
        </Button>
      </div>
      <p
        role="status"
        className={cx("min-h-5 text-13", status?.tone === "problem" ? "text-danger" : "text-secondary")}
        data-testid="shortcut-status"
      >
        {status?.text}
      </p>
      {saveFailed && <Notice tone="warning" title={t("keyboard.settings.saveFailed")} />}
      {groups.map((group) => (
        <section key={group.id} aria-labelledby={`${descriptionId}-${group.id}`} className="flex flex-col">
          <h3 id={`${descriptionId}-${group.id}`} className="text-14 font-semibold text-primary">
            {t(`keyboard.settings.groups.${group.id}`)}
          </h3>
          <ul className="flex flex-col">
            {group.commands.map((id) => (
              <CommandRow
                key={id}
                id={id}
                platform={platform}
                recording={recording === id}
                onRecord={setRecording}
                onStatus={setStatus}
              />
            ))}
          </ul>
        </section>
      ))}
    </SettingsSection>
  );
}
