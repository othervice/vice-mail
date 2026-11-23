import { CircleCheck } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { EmptyState } from "../../components/EmptyState";
import { ErrorNotice } from "../../components/ErrorNotice";
import { toCoreError, uiIpc, type CoreError, type DraftId, type OutgoingMail } from "../../services/ipc";
import { needsUser, openComposer, useComposerStore } from "../../stores/composer";
import { ComposerHost } from "../composer/Composer";
import { SendNotices } from "../composer/SendNotices";
import { ComposerInWindow } from "./composerPlacement";

/** A send of this draft the window waits for: the undo delay runs, or it needs the user's answer. */
function holdsWindow(mail: OutgoingMail, draftId: DraftId): boolean {
  if (mail.draftId !== draftId) return false;
  return (mail.state === "scheduled" && !mail.scheduled) || needsUser(mail);
}

interface ComposerWindowProps {
  draftId: DraftId;
  eventsError: CoreError | null;
}

/**
 * Stage 8 (decision 0025): the composer of one draft in a window of its own, the whole window.
 * Several of these can be open at once, each with its own draft. When the draft is closed or
 * discarded, the window closes; after "Send" it stays while the undo delay runs (the notice can
 * bring the draft back) or while the send needs an answer, then closes. If the shell does not
 * let the window close itself, it says that the work is done and how to close it.
 */
export function ComposerWindow({ draftId, eventsError }: ComposerWindowProps) {
  const { t } = useTranslation();
  const session = useComposerStore((state) => state.session);
  const sends = useComposerStore((state) => state.sends);
  const problems = useComposerStore((state) => state.problems);
  const opened = useRef(false);
  const [closeError, setCloseError] = useState<CoreError | null>(null);

  useEffect(() => {
    if (opened.current) return;
    opened.current = true;
    openComposer({ kind: "draft", draftId });
  }, [draftId]);

  // Once the composer had its session, its end (sent, closed, discarded) is the end of the window.
  const [seen, setSeen] = useState(false);
  if (session && !seen) setSeen(true);
  const done = seen && session === null;
  const waiting = sends.some((send) => holdsWindow(send.mail, draftId)) || problems.length > 0;
  useEffect(() => {
    if (!done || waiting) return;
    let active = true;
    uiIpc.closeThisWindow().catch((e: unknown) => {
      if (active) setCloseError(toCoreError(e));
    });
    return () => {
      active = false;
    };
  }, [done, waiting]);

  return (
    <ComposerInWindow.Provider value={true}>
      <div className="flex h-full bg-canvas text-primary" data-window="composer">
        <main tabIndex={-1} className="relative flex min-w-0 flex-1 flex-col bg-surface outline-none" aria-label={t("windows.composer.region")}>
          {eventsError && (
            <div className="px-6 pt-4">
              <ErrorNotice context={t("errors.context.events")} error={eventsError} />
            </div>
          )}
          {done && (
            <>
              <EmptyState
                icon={CircleCheck}
                title={t(waiting ? "windows.composer.waiting.title" : "windows.composer.done.title")}
                body={t(waiting ? "windows.composer.waiting.body" : "windows.composer.done.body")}
              />
              {closeError && !waiting && (
                <div className="px-6 pb-6">
                  <ErrorNotice context={t("windows.closeFailed")} error={closeError} data-testid="window-close-failed" />
                </div>
              )}
            </>
          )}
          <ComposerHost />
        </main>
        <SendNotices />
      </div>
    </ComposerInWindow.Provider>
  );
}
