import { Ban, Copy, FolderInput } from "lucide-react";
import { useLayoutEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { cx } from "../../components/cx";
import { useDragStore, type DropHover } from "../../stores/drag";
import { shortcutPlatform } from "../actions/shortcuts";
import { useAppInfo } from "../app-info/queries";

/** The hint's text: what a drop here does, or why it does not take the rows. */
function useHintText(): (hover: DropHover) => string {
  const { t } = useTranslation();
  const platform = shortcutPlatform(useAppInfo().data?.platform);
  return ({ verdict, folder }) => {
    if (verdict.kind === "accept") return t(verdict.mode === "copy" ? "drop.hint.copy" : "drop.hint.move", { count: verdict.count, folder });
    const account = verdict.account.displayName || verdict.account.email;
    switch (verdict.reason) {
      case "targetReadOnly":
        return t("drop.hint.targetReadOnly", { account });
      case "sourceReadOnly":
        return t("drop.hint.sourceReadOnly", { account, key: t(platform === "macos" ? "drop.optionKey" : "drop.ctrlKey") });
      case "noRoleFolder":
        return t("drop.hint.noRoleFolder", { account, role: t(`roles.${verdict.role}`) });
    }
  };
}

/**
 * Stage 22 (decision 0081): beside the folder the dragged rows are over, what dropping does there
 * ("Move 3 messages to Projects", "Copy …" while Option or Ctrl is held) or why that folder does not
 * take them (a read-only account, a missing role folder). Visible next to the folder and announced
 * through a status region that is always there; the cursor shows the same as copy, move or "no".
 */
export function DropHint() {
  const hover = useDragStore((state) => state.hover);
  const dragging = useDragStore((state) => state.session !== null);
  const text = useHintText();
  const bubble = useRef<HTMLDivElement>(null);
  const shown = dragging ? hover : null;
  const message = shown ? text(shown) : "";

  // Beside the folder (CSSOM, not a style attribute: CSP style-src 'self'); below it where the
  // window ends first.
  useLayoutEffect(() => {
    const node = bubble.current;
    if (!node || !shown) return;
    const width = node.offsetWidth;
    const room = window.innerWidth - shown.rect.right - 8;
    const below = room < width;
    node.style.left = `${String(below ? Math.max(8, shown.rect.left) : shown.rect.right + 8)}px`;
    node.style.top = `${String(below ? shown.rect.bottom + 4 : shown.rect.top)}px`;
  }, [shown]);

  const refused = shown?.verdict.kind === "refuse";
  const Icon = refused ? Ban : shown?.verdict.kind === "accept" && shown.verdict.mode === "copy" ? Copy : FolderInput;
  return (
    <>
      <div role="status" className="sr-only" data-testid="drop-hint-status">
        {message}
      </div>
      {shown && (
        <div
          ref={bubble}
          aria-hidden
          data-testid="drop-hint"
          data-drop-hint={refused ? "refused" : "accept"}
          className={cx(
            "pointer-events-none fixed z-40 flex max-w-(--ui-notice-width) items-start gap-2 rounded-control border bg-surface-raised px-2 py-1 text-12 font-medium text-primary shadow-floating",
            refused ? "border-danger" : "border-border",
          )}
        >
          <Icon aria-hidden className={cx("mt-px size-(--ui-icon-small) shrink-0", refused ? "text-danger" : "text-accent")} strokeWidth={1.75} />
          <span>{message}</span>
        </div>
      )}
    </>
  );
}
