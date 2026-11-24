import { cx } from "../../components/cx";
import type { AccountColor } from "../../services/ipc";
import { useContactPicture } from "../contacts/pictures";
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
  /**
   * `mini`: 16 px, one initial, for the sender chips of bundles and the composer's recipients;
   * `large`: 96 px, for the contact's page (stage 17).
   */
  size?: "regular" | "mini" | "large";
  /**
   * Stage 17 (decision 0050): show the picture the user chose for the contact with this address,
   * if there is one (default). The account's own addresses never have one.
   */
  picture?: boolean;
}

const SIZES = {
  regular: "size-(--ui-avatar) text-12",
  mini: "size-(--ui-avatar-mini) text-9",
  large: "size-(--ui-avatar-large) text-28",
} as const;

/**
 * The contact's picture where the user chose one (stage 17), else initials on a soft tint chosen
 * per sender (decision 0031), with a ring in the account colour. Never a picture from the network:
 * the core's helper wrote every picture shown. Decorative: the name it stands for is always written
 * next to it.
 */
export function Avatar({ name, email, accountColor, ring = true, size = "regular", picture = true }: AvatarProps) {
  const letters = initials(name, email);
  const url = useContactPicture(picture ? email : null);
  return (
    <span
      aria-hidden
      data-testid="avatar"
      data-picture={url ? "" : undefined}
      className={cx(
        "flex shrink-0 items-center justify-center overflow-hidden rounded-full font-semibold select-none",
        !url && senderTint(email, name),
        SIZES[size],
        ring && size === "regular" && cx("ring-2", accountRing(accountColor)),
      )}
    >
      {url ? (
        <img src={url} alt="" draggable={false} className="size-full object-cover" data-testid="avatar-picture" />
      ) : size === "mini" ? (
        Array.from(letters)[0]
      ) : (
        letters
      )}
    </span>
  );
}
