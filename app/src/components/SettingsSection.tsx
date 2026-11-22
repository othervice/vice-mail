import { useId, type ReactNode } from "react";

interface SettingsSectionProps {
  title: string;
  children: ReactNode;
}

export function SettingsSection({ title, children }: SettingsSectionProps) {
  const headingId = useId();
  return (
    <section aria-labelledby={headingId} className="flex max-w-(--ui-content-max-width) flex-col gap-6">
      <h2 id={headingId} className="text-16 font-semibold text-primary">
        {title}
      </h2>
      {children}
    </section>
  );
}
