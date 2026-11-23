import { useQueryClient } from "@tanstack/react-query";
import { CircleCheck, CircleExclamationPoint, Info, X } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { cx } from "../../components/cx";
import { useNoticeStore, type Notice } from "../../stores/notices";
import { boundShortcuts } from "../../stores/shortcuts";
import { useUiStore } from "../../stores/ui";
import { useAppInfo } from "../app-info/queries";
import { comfortNoticeText } from "../comfort/notices";
import { ariaShortcut, formatShortcut, shortcutPlatform } from "./shortcuts";
import { undoAction } from "./undo";

/** How long a notice stays (docs/DESIGN.md: "a few seconds"); hovering or focusing holds it. */
export const NOTICE_MS = 6000;
const ERROR_NOTICE_MS = 10_000;

function isError(notice: Notice): boolean {
  const { content } = notice;
  if (content.kind === "comfort") return content.notice.type === "failed" || content.notice.type === "ruleSkipped";
  return content.kind === "error" || content.kind === "operationFailed";
}

function NoticeItem({ notice }: { notice: Notice }) {
  const { t, i18n } = useTranslation();
  const queryClient = useQueryClient();
  const dismiss = useNoticeStore((state) => state.dismiss);
  const showAccountsPage = useUiStore((state) => state.showAccountsPage);
  const showActivity = useUiStore((state) => state.showActivity);
  const platform = shortcutPlatform(useAppInfo().data?.platform);
  const [held, setHeld] = useState(0);
  const { content } = notice;
  const error = isError(notice);

  useEffect(() => {
    if (held > 0) return undefined;
    const timer = window.setTimeout(
      () => {
        dismiss(notice.id);
      },
      error ? ERROR_NOTICE_MS : NOTICE_MS,
    );
    return () => {
      window.clearTimeout(timer);
    };
  }, [held, dismiss, notice.id, error]);

  let text: string;
  let detail: string | null = null;
  let action: { label: string; hint?: string; keys?: string; run: () => void; testId: string } | null = null;
  switch (content.kind) {
    case "done": {
      text = t(`actions.done.${content.action}`, { count: content.count, detail: content.detail ?? "" });
      const undo = content.undo;
      const shortcut = boundShortcuts("undo", platform)[0];
      if (undo) {
        action = {
          label: t("actions.undo"),
          ...(shortcut && { hint: formatShortcut(shortcut, platform), keys: ariaShortcut(shortcut, platform) }),
          run: () => {
            void undoAction(queryClient, undo);
          },
          testId: "notice-undo",
        };
      }
      break;
    }
    case "undone":
      text = t("actions.undone");
      break;
    case "notUndoable":
      text = t("errors.codes.not_undoable.title");
      detail = t("errors.codes.not_undoable.hint");
      break;
    case "readOnly": {
      const { accountId } = content;
      text = t("errors.codes.read_only.title");
      detail = t("actions.readOnly.explanation");
      action = {
        label: t("actions.readOnly.open"),
        run: () => {
          dismiss(notice.id);
          showAccountsPage({ kind: "account", accountId });
        },
        testId: "notice-allow-changes",
      };
      break;
    }
    case "outdated":
      text = t("actions.outdated");
      break;
    case "error":
      text = t(`actions.failed.${content.context}`);
      detail =
        // Stage 16: the one refusal of a transfer the user can act on (decision 0033).
        content.context === "transfer" && content.error.code === "invalid_argument"
          ? t("organize.transfer.noMessageId")
          : t(`errors.codes.${content.error.code}.title`);
      break;
    case "transfer":
      text =
        content.mode === "move"
          ? t("organize.transfer.movingStarted", { count: content.count, folder: content.folder, account: content.account })
          : t("organize.transfer.started", { count: content.count, folder: content.folder, account: content.account });
      action = {
        label: t("actions.showActivity"),
        run: () => {
          dismiss(notice.id);
          showActivity();
        },
        testId: "notice-show-activity",
      };
      break;
    case "comfort": {
      // Stage 9: pins, snoozes, tasks, skipped local rules.
      const comfort = comfortNoticeText(t, i18n.language, content.notice);
      text = comfort.text;
      detail = comfort.detail;
      const undo = comfort.undo;
      if (undo) {
        action = {
          label: t("actions.undo"),
          run: () => {
            dismiss(notice.id);
            undo(queryClient);
          },
          testId: "notice-undo",
        };
      }
      break;
    }
    case "operationFailed": {
      const name = t(`activity.actions.${content.action}`);
      text = content.conflict
        ? t("actions.operationConflict", { count: content.count, action: name })
        : t("actions.operationFailed", { count: content.count, action: name });
      if (content.conflict) detail = t(`activity.conflict.${content.conflict}`);
      else if (content.error) detail = t(`errors.codes.${content.error.code}.title`);
      action = {
        label: t("actions.showActivity"),
        run: () => {
          dismiss(notice.id);
          showActivity();
        },
        testId: "notice-show-activity",
      };
      break;
    }
  }

  const Icon = error ? CircleExclamationPoint : content.kind === "done" || content.kind === "undone" ? CircleCheck : Info;
  return (
    <div
      role={error ? "alert" : undefined}
      onMouseEnter={() => {
        setHeld((n) => n + 1);
      }}
      onMouseLeave={() => {
        setHeld((n) => Math.max(0, n - 1));
      }}
      onFocus={() => {
        setHeld((n) => n + 1);
      }}
      onBlur={() => {
        setHeld((n) => Math.max(0, n - 1));
      }}
      data-testid="notice"
      data-notice-kind={content.kind}
      className={cx(
        "pointer-events-auto flex w-full max-w-(--ui-notice-width) items-start gap-3 rounded-card border bg-surface-raised px-4 py-3 shadow-floating",
        error ? "border-danger" : "border-border",
      )}
    >
      <Icon
        aria-hidden
        className={cx("size-(--ui-icon-small) shrink-0", error ? "text-danger" : content.kind === "done" ? "text-success" : "text-accent")}
        strokeWidth={1.75}
      />
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <p className="text-13 font-medium text-primary">{text}</p>
        {detail && <p className="text-12 text-secondary">{detail}</p>}
      </div>
      {action && (
        <button
          type="button"
          onClick={action.run}
          aria-keyshortcuts={action.keys}
          className="flex shrink-0 items-center gap-2 rounded-control px-2 py-1 text-13 font-medium text-accent hover:bg-hover"
          data-testid={action.testId}
        >
          {action.label}
          {action.hint && (
            <kbd aria-hidden className="rounded-control border border-border px-1 font-sans text-12 text-secondary">
              {action.hint}
            </kbd>
          )}
        </button>
      )}
      <button
        type="button"
        aria-label={t("actions.dismiss")}
        onClick={() => {
          dismiss(notice.id);
        }}
        className="flex size-6 shrink-0 items-center justify-center rounded-control text-secondary hover:bg-hover hover:text-primary"
      >
        <X aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
      </button>
    </div>
  );
}

/**
 * Notices at the bottom of the window: the undo notice after each action, and what went wrong.
 * The polite ones live in a status region that is always there, so screen readers announce
 * every new one; failures are alerts.
 */
export function Notices() {
  const { t } = useTranslation();
  const notices = useNoticeStore((state) => state.notices);
  const calm = notices.filter((notice) => !isError(notice));
  const errors = notices.filter(isError);
  return (
    <div
      aria-label={t("actions.notices")}
      role="group"
      className="pointer-events-none fixed inset-x-0 bottom-6 z-30 flex flex-col items-center gap-2 px-4"
      data-testid="notices"
    >
      <div role="status" className="flex w-full flex-col items-center gap-2">
        {calm.map((notice) => (
          <NoticeItem key={notice.id} notice={notice} />
        ))}
      </div>
      {errors.map((notice) => (
        <NoticeItem key={notice.id} notice={notice} />
      ))}
    </div>
  );
}
