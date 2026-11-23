import { useTranslation } from "react-i18next";
import { TextField } from "../../components/TextField";
import { MIN_SYNC_PASSPHRASE_CHARS } from "../../services/ipc";

export interface PassphraseDraft {
  passphrase: string;
  repeat: string;
}

export const EMPTY_PASSPHRASE: PassphraseDraft = { passphrase: "", repeat: "" };

/** Characters as the core counts them (Unicode scalar values, not UTF-16 units). */
function length(text: string): number {
  return [...text].length;
}

export type PassphraseProblem = "tooShort" | "mismatch";

export function passphraseProblem(draft: PassphraseDraft): PassphraseProblem | null {
  if (length(draft.passphrase) < MIN_SYNC_PASSPHRASE_CHARS) return "tooShort";
  if (draft.passphrase !== draft.repeat) return "mismatch";
  return null;
}

interface PassphraseFieldsProps {
  draft: PassphraseDraft;
  onChange: (draft: PassphraseDraft) => void;
  showErrors: boolean;
  /** "new" for changing or resetting, where an old passphrase exists. */
  variant?: "first" | "new";
  /** Prefix of the fields' test ids. */
  "data-testid": string;
}

/** A passphrase typed twice, with the strength hint and the minimum length from the core. */
export function PassphraseFields({ draft, onChange, showErrors, variant = "first", ...rest }: PassphraseFieldsProps) {
  const testIdPrefix = rest["data-testid"];
  const { t } = useTranslation();
  const problem = showErrors ? passphraseProblem(draft) : null;
  return (
    <div className="flex flex-col gap-3">
      <TextField
        type="password"
        autoComplete="new-password"
        label={variant === "first" ? t("settingsSync.passphrase.label") : t("settingsSync.passphrase.newLabel")}
        description={t("settingsSync.passphrase.strength", { min: MIN_SYNC_PASSPHRASE_CHARS })}
        value={draft.passphrase}
        onChange={(passphrase) => {
          onChange({ ...draft, passphrase });
        }}
        error={problem === "tooShort" ? t("settingsSync.passphrase.tooShort", { min: MIN_SYNC_PASSPHRASE_CHARS }) : null}
        data-testid={`${testIdPrefix}-passphrase`}
      />
      <TextField
        type="password"
        autoComplete="new-password"
        label={t("settingsSync.passphrase.repeat")}
        value={draft.repeat}
        onChange={(repeat) => {
          onChange({ ...draft, repeat });
        }}
        error={problem === "mismatch" ? t("settingsSync.passphrase.mismatch") : null}
        data-testid={`${testIdPrefix}-repeat`}
      />
    </div>
  );
}
