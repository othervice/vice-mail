import { cx } from "../../components/cx";
import type { AccountColor } from "../../services/ipc";
import { accountRing, initials } from "./accountColor";

interface AvatarProps {
  name: string | null | undefined;
  email: string;
  /** The account the message or entry belongs to: its colour forms the ring. */
  accountColor: AccountColor;
}

/**
 * Initials on a neutral surface with a ring in the account colour. Never a picture from the
 * network. Decorative: the name it stands for is always written next to it.
 */
export function Avatar({ name, email, accountColor }: AvatarProps) {
  return (
    <span
      aria-hidden
      className={cx(
        "flex size-(--ui-avatar) shrink-0 items-center justify-center rounded-full bg-rail text-12 font-semibold text-secondary ring-2",
        accountRing(accountColor),
      )}
    >
      {initials(name, email)}
    </span>
  );
}
