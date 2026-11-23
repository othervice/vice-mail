import { CircleCheck, Info, TriangleExclamationPoint, type LucideIcon } from "lucide-react";
import type { ReactNode } from "react";
import { cx } from "./cx";

interface NoticeProps {
  tone?: "info" | "warning" | "success";
  title: string;
  children?: ReactNode;
  /** `status` for results the user just triggered (announced politely). */
  role?: "status";
  "data-testid"?: string;
}

const TONES: Record<NonNullable<NoticeProps["tone"]>, { icon: LucideIcon; className: string }> = {
  info: { icon: Info, className: "text-accent" },
  warning: { icon: TriangleExclamationPoint, className: "text-warning" },
  success: { icon: CircleCheck, className: "text-success" },
};

/** A calm, inline hint: what it is about in one line, the details below. Not for errors. */
export function Notice({ tone = "info", title, children, role, ...rest }: NoticeProps) {
  const { icon: Icon, className } = TONES[tone];
  return (
    <div
      role={role}
      data-testid={rest["data-testid"]}
      className="flex gap-3 rounded-card border border-border-subtle bg-canvas p-4"
    >
      <Icon aria-hidden className={cx("size-(--ui-icon) shrink-0", className)} strokeWidth={1.75} />
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <p className="font-semibold text-primary">{title}</p>
        {children && <div className="flex flex-col gap-2 text-secondary">{children}</div>}
      </div>
    </div>
  );
}
