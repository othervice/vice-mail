import { cx } from "../../components/cx";
import type { AccountColor } from "../../services/ipc";
import { accountRing, initials } from "./accountColor";
import { senderTint } from "./senderTint";

interface AvatarProps {
  name: string | null | undefined;
  email: string;
  /** The account the message or entry belongs to: its colour forms the ring. */
  accountColor: AccountColor;
  /**
   * Draw the ring in the account colour (default). Lists of one account leave it out: every row
   * would carry the same ring (decision 0031); lists that mix accounts and the rail keep it.
   */
  ring?: boolean;
  /** `mini`: 16 px, one initial, for the sender chips of bundles. */
  size?: "regular" | "mini";
}

/**
 * Initials on a soft tint chosen per sender (decision 0031), with a ring in the account colour.
 * Never a picture from the network. Decorative: the name it stands for is always written next to
 * it.
 */
export function Avatar({ name, email, accountColor, ring = true, size = "regular" }: AvatarProps) {
  const letters = initials(name, email);
  return (
    <span
      aria-hidden
      data-testid="avatar"
      className={cx(
        "flex shrink-0 items-center justify-center rounded-full font-semibold select-none",
        senderTint(email, name),
        size === "mini" ? "size-(--ui-avatar-mini) text-9" : "size-(--ui-avatar) text-12",
        ring && size === "regular" && cx("ring-2", accountRing(accountColor)),
      )}
    >
      {size === "mini" ? Array.from(letters)[0] : letters}
    </span>
  );
}
