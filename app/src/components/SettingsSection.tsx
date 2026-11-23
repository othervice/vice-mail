import { useId, type ReactNode } from "react";
import { cx } from "./cx";

interface SettingsSectionProps {
  title: string;
  children: ReactNode;
  /** A further section on the same page (Inbox: unread first, sorting, blocked, rules): 16 px. */
  sub?: boolean;
}

export function SettingsSection({ title, children, sub = false }: SettingsSectionProps) {
  const headingId = useId();
  return (
    // Design pass 23.11.2026: the page's section as its title (20 px), further sections of the
    // same page as 16 px headings with a hairline above; the settings below them.
    <section
      aria-labelledby={headingId}
      className={cx("flex max-w-(--ui-content-max-width) flex-col", sub ? "gap-4 border-t border-border-subtle pt-6" : "gap-6")}
    >
      <h2 id={headingId} className={cx("font-semibold text-primary", sub ? "text-16" : "text-20")}>
        {title}
      </h2>
      {children}
    </section>
  );
}
