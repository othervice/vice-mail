import { ArrowLeft, Check, Download, Monitor, Moon, RefreshCw, Sparkles, Sun, type LucideIcon } from "lucide-react";
import { RadioGroup } from "radix-ui";
import { useEffect, useId, useRef, useState, type ReactNode, type Ref } from "react";
import { useTranslation } from "react-i18next";
import { Wordmark } from "../../components/Brand";
import { Button } from "../../components/Button";
import { cx } from "../../components/cx";
import { ErrorNotice } from "../../components/ErrorNotice";
import { Notice } from "../../components/Notice";
import { Switch } from "../../components/Switch";
import { PRODUCT_NAME } from "../../config/product";
import type { Account, CoreError, FirstStartEnd, ThemePreference } from "../../services/ipc";
import { useAccounts } from "../accounts/queries";
import { useWindowDragRegion } from "../app-info/useWindowDragRegion";
import { DEFAULT_PREFERENCES, usePreferences, useSetPreferences } from "../preferences/queries";
import { AccentPicker } from "../settings/AccentPicker";
import { AddAccount } from "../settings/accounts/AddAccount";
import { useSettingsSyncStatus } from "../settings-sync/queries";
import { SettingsSyncSettings } from "../settings-sync/SettingsSyncSettings";
import { useUpdateInfo, useUpdateStep } from "../updates/queries";
import { Promises } from "./Promises";
import { useFinishFirstStart } from "./queries";

const STEPS = ["welcome", "account", "appearance", "extras", "done"] as const;
type StepId = (typeof STEPS)[number];

interface FirstStartProps {
  eventsError: CoreError | null;
  /** The core has stored the end (completed or skipped): the frame takes over. */
  onEnd: () => void;
}

/**
 * Stage 15 (decision 0044, docs/DESIGN.md "Erster Start"): the guided first start of a new
 * installation. Five short steps on one card: welcome with the promises, the account form,
 * theme and accent (applied at once), the optional extras (settings sync, AI, the update
 * question of decision 0037) and a last word on read-only accounts. "Skip setup" ends it on
 * any step; either way the core remembers it and it never shows again.
 *
 * Every step keeps its state while the user goes back and forth (visited steps stay mounted,
 * hidden), the focus moves to the heading of each step, and Escape does nothing here: no key
 * throws away what was typed.
 */
export function FirstStart({ eventsError, onEnd }: FirstStartProps) {
  const { t } = useTranslation();
  const dragRegion = useWindowDragRegion();
  const [step, setStep] = useState<StepId>("welcome");
  const [visited, setVisited] = useState<ReadonlySet<StepId>>(() => new Set<StepId>(["welcome"]));
  const [account, setAccount] = useState<Account | null>(null);
  const [syncOpen, setSyncOpen] = useState(false);
  const body = useRef<HTMLDivElement>(null);
  const updates = useUpdateInfo();
  const answerUpdates = useUpdateStep("setAutoCheck");
  const finish = useFinishFirstStart();
  const index = STEPS.indexOf(step);
  const busy = finish.isPending || answerUpdates.isPending;

  // A new step starts at its top, with the focus on its heading (also the first one).
  useEffect(() => {
    if (!body.current) return;
    body.current.scrollTop = 0;
    body.current.querySelector<HTMLElement>(`[data-step-heading="${step}"]`)?.focus();
  }, [step]);

  const go = (next: StepId) => {
    setSyncOpen(false);
    setVisited((previous) => (previous.has(next) ? previous : new Set([...previous, next])));
    setStep(next);
  };

  /**
   * Decision 0044: the update question of decision 0037 is asked on the extras step. Once that
   * step was shown, leaving it forward or skipping the rest answers it with what the switch
   * shows (off unless turned on), so the separate question after the first account never comes.
   */
  const answerUpdateQuestion = async (): Promise<boolean> => {
    const info = updates.data;
    if (!info?.supported || !visited.has("extras")) return true;
    try {
      await answerUpdates.mutateAsync(info.autoCheck);
      return true;
    } catch {
      // The mutation keeps the error; the footer shows it and the user stays on the step.
      return false;
    }
  };

  const end = (how: FirstStartEnd) => {
    finish.mutate(how, { onSuccess: onEnd });
  };

  const skip = async () => {
    if (await answerUpdateQuestion()) end("skipped");
  };

  const next = async () => {
    if (step === "extras" && !(await answerUpdateQuestion())) return;
    const following = STEPS[index + 1];
    if (following) go(following);
  };

  const back = () => {
    if (syncOpen) {
      setSyncOpen(false);
      return;
    }
    const previous = STEPS[index - 1];
    if (previous) go(previous);
  };

  let primary: ReactNode;
  switch (step) {
    case "welcome":
      primary = (
        <Button onClick={() => void next()} data-testid="first-start-next">
          {t("firstStart.welcome.start")}
        </Button>
      );
      break;
    case "account":
      primary = account ? (
        <Button onClick={() => void next()} data-testid="first-start-next">
          {t("firstStart.next")}
        </Button>
      ) : (
        <Button variant="secondary" onClick={() => void next()} data-testid="first-start-later">
          {t("firstStart.account.later")}
        </Button>
      );
      break;
    case "appearance":
    case "extras":
      primary = (
        <Button onClick={() => void next()} disabled={busy} data-testid="first-start-next">
          {t("firstStart.next")}
        </Button>
      );
      break;
    case "done":
      primary = (
        <Button
          onClick={() => {
            end("completed");
          }}
          disabled={busy}
          data-testid="first-start-finish"
        >
          {account ? t("firstStart.done.openInbox") : t("firstStart.done.start", { product: PRODUCT_NAME })}
        </Button>
      );
      break;
  }

  return (
    <div className="flex h-full flex-col bg-canvas text-primary" data-testid="first-start" data-step={step}>
      {/* On macOS the band holds the window buttons and moves the window, like the view headers. */}
      <header
        data-tauri-drag-region={dragRegion ? "deep" : undefined}
        className="flex h-(--ui-header-height) shrink-0 items-center justify-end px-4"
      >
        {step !== "done" && (
          <Button variant="quiet" onClick={() => void skip()} disabled={busy} data-testid="first-start-skip">
            {t("firstStart.skip")}
          </Button>
        )}
      </header>
      <main className="flex min-h-0 flex-1 flex-col items-center justify-center px-6 pb-6">
        <div className="flex min-h-0 w-full max-w-(--ui-content-max-width) flex-1 flex-col overflow-hidden rounded-panel border border-border-subtle bg-surface-raised shadow-panel max-h-(--ui-first-start-height)">
          <StepList current={index} />
          <div ref={body} className="flex min-h-0 flex-1 flex-col overflow-y-auto px-8 py-6" data-testid="first-start-body">
            {eventsError && (
              <div className="pb-6">
                <ErrorNotice context={t("errors.context.events")} error={eventsError} />
              </div>
            )}
            {visited.has("welcome") && (
              <Step
                id="welcome"
                current={step}
                title={t("firstStart.welcome.title")}
                centered
                top={<Wordmark aria-label={PRODUCT_NAME} className="h-(--ui-wordmark-height) w-auto max-w-full text-brand-ink" />}
              >
                <div className="flex flex-col gap-2 text-14 text-secondary">
                  <p>{t("firstStart.welcome.body", { product: PRODUCT_NAME })}</p>
                  <p>{t("firstStart.welcome.time")}</p>
                </div>
                <Promises aria-label={t("firstStart.welcome.promises")} />
              </Step>
            )}
            {visited.has("account") && (
              <Step
                id="account"
                current={step}
                title={t("firstStart.account.title")}
                intro={t("firstStart.account.body")}
              >
                {account ? <AccountAdded account={account} /> : <AddAccount embedded={{ onAdded: setAccount }} />}
              </Step>
            )}
            {visited.has("appearance") && (
              <Step
                id="appearance"
                current={step}
                title={t("firstStart.appearance.title")}
                intro={t("firstStart.appearance.body")}
              >
                <ThemePicker />
                <AccentPicker />
              </Step>
            )}
            {visited.has("extras") && (
              <Step
                id="extras"
                current={step}
                title={t("firstStart.extras.title")}
                intro={t("firstStart.extras.body")}
              >
                <Extras
                  syncOpen={syncOpen}
                  onSync={setSyncOpen}
                  autoCheck={updates.data?.supported ? updates.data.autoCheck : null}
                  onAutoCheck={(enabled) => {
                    answerUpdates.mutate(enabled);
                  }}
                  autoCheckBusy={answerUpdates.isPending}
                />
              </Step>
            )}
            {visited.has("done") && (
              <Step
                id="done"
                current={step}
                title={t("firstStart.done.title")}
                centered
                top={
                  <span className="flex size-12 items-center justify-center rounded-full bg-accent-soft text-accent">
                    <Check aria-hidden className="size-(--ui-icon)" strokeWidth={2} />
                  </span>
                }
              >
                <div className="w-full text-start">
                  {account ? (
                    <Notice title={t("firstStart.done.readOnlyTitle", { email: account.email })} data-testid="first-start-read-only">
                      <p>{t("firstStart.done.readOnlyBody")}</p>
                    </Notice>
                  ) : (
                    <Notice title={t("firstStart.done.noAccountTitle")} data-testid="first-start-no-account">
                      <p>{t("firstStart.done.noAccountBody")}</p>
                    </Notice>
                  )}
                </div>
                <p className="text-13 text-secondary">{t("firstStart.done.later")}</p>
              </Step>
            )}
          </div>
          <footer className="flex shrink-0 flex-col gap-3 border-t border-border px-8 py-4">
            {answerUpdates.isError && (
              <ErrorNotice context={t("errors.context.updateSetting")} error={answerUpdates.error} data-testid="first-start-error" />
            )}
            {finish.isError && <ErrorNotice context={t("firstStart.finishFailed")} error={finish.error} data-testid="first-start-error" />}
            <div className="flex items-center justify-between gap-3">
              <div>
                {index > 0 && (
                  <Button variant="secondary" onClick={back} disabled={busy} data-testid="first-start-back">
                    {syncOpen && <ArrowLeft aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />}
                    {syncOpen ? t("firstStart.extras.sync.back") : t("firstStart.back")}
                  </Button>
                )}
              </div>
              {primary}
            </div>
          </footer>
        </div>
      </main>
    </div>
  );
}

/** Where the user is: five labelled segments, the current one marked for assistive technology. */
function StepList({ current }: { current: number }) {
  const { t } = useTranslation();
  return (
    <nav aria-label={t("firstStart.steps.label")} className="shrink-0 border-b border-border px-8 pt-5 pb-4">
      <ol className="grid grid-cols-5 gap-2" data-testid="first-start-steps">
        {STEPS.map((id, index) => {
          const done = index < current;
          const here = index === current;
          return (
            <li key={id} aria-current={here ? "step" : undefined} className="flex min-w-0 flex-col gap-2">
              <span aria-hidden className={cx("h-1 rounded-full", done || here ? "bg-accent-fill" : "bg-track")} />
              <span className={cx("truncate text-12", here ? "font-semibold text-primary" : done ? "text-secondary" : "text-tertiary")}>
                {t(`firstStart.steps.${id}`)}
                {done && <span className="sr-only">{t("firstStart.steps.completed")}</span>}
              </span>
            </li>
          );
        })}
      </ol>
    </nav>
  );
}

interface StepProps {
  id: StepId;
  current: StepId;
  title: string;
  intro?: string;
  /** Above the heading (the wordmark on the welcome step). */
  top?: ReactNode;
  centered?: boolean;
  children: ReactNode;
}

/** One step: hidden while another one shows, labelled by its heading, which takes the focus. */
function Step({ id, current, title, intro, top, centered = false, children }: StepProps) {
  const headingId = useId();
  return (
    <section
      aria-labelledby={headingId}
      hidden={id !== current}
      // Centred steps (welcome, done) also sit in the middle of the card's height when they are
      // shorter than it; auto margins never push a taller step out of its scroll area.
      className={cx("flex flex-col gap-6", centered && "my-auto items-center text-center")}
      data-testid={`first-start-${id}`}
    >
      {top}
      <div className="flex flex-col gap-2">
        {/* Not a control: the focus lands here so a screen reader reads the step from its top. */}
        <h1 id={headingId} data-step-heading={id} tabIndex={-1} className="text-20 font-semibold text-primary outline-none">
          {title}
        </h1>
        {intro && <p className="text-14 text-secondary">{intro}</p>}
      </div>
      {children}
    </section>
  );
}

/** After the form added the account: what happens now, the focus on it (the form is gone). */
function AccountAdded({ account }: { account: Account }) {
  const { t } = useTranslation();
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    ref.current?.focus();
  }, []);
  return (
    <div ref={ref} tabIndex={-1} className="outline-none" data-testid="first-start-account-added">
      <Notice tone="success" role="status" title={t("firstStart.account.addedTitle", { email: account.email })}>
        <p>{t("firstStart.account.addedBody")}</p>
      </Notice>
    </div>
  );
}

const THEME_ICONS: Record<ThemePreference, LucideIcon> = { system: Monitor, light: Sun, dark: Moon };
const THEMES: readonly ThemePreference[] = ["system", "light", "dark"];

/**
 * The theme as three tiles like the accent tiles below it (icon and name), saved through the
 * core and applied at once (optimistic, rolled back if the core refuses).
 */
function ThemePicker() {
  const { t } = useTranslation();
  const labelId = useId();
  const descriptionId = useId();
  const preferences = usePreferences();
  const save = useSetPreferences();
  const current = preferences.data ?? DEFAULT_PREFERENCES;
  return (
    <div className="flex flex-col gap-2">
      <div className="flex flex-col gap-1">
        <span id={labelId} className="text-14 font-medium text-primary">
          {t("settings.appearance.theme.label")}
        </span>
        <span id={descriptionId} className="text-13 text-secondary">
          {t("settings.appearance.theme.description")}
        </span>
      </div>
      <RadioGroup.Root
        value={current.theme}
        onValueChange={(next) => {
          const theme = THEMES.find((value) => value === next);
          if (theme) save.mutate({ ...current, theme });
        }}
        aria-labelledby={labelId}
        aria-describedby={descriptionId}
        className="grid grid-cols-3 gap-2"
        data-testid="first-start-theme"
      >
        {THEMES.map((theme) => {
          const Icon = THEME_ICONS[theme];
          return (
            <RadioGroup.Item
              key={theme}
              value={theme}
              data-testid={`first-start-theme-${theme}`}
              className="flex h-10 items-center gap-3 rounded-card border border-border bg-surface px-3 text-start text-13 font-medium text-primary transition-colors hover:bg-hover data-[state=checked]:border-accent data-[state=checked]:bg-accent-soft"
            >
              <Icon aria-hidden className="size-(--ui-icon-small) shrink-0 text-accent" strokeWidth={1.75} />
              <span className="truncate">{t(`settings.appearance.theme.${theme}`)}</span>
            </RadioGroup.Item>
          );
        })}
      </RadioGroup.Root>
      {save.isError && <ErrorNotice context={t("errors.context.savePreferences")} error={save.error} />}
    </div>
  );
}

interface ExtrasProps {
  syncOpen: boolean;
  onSync: (open: boolean) => void;
  /** The update switch; null where this build does not update itself (nothing to ask). */
  autoCheck: boolean | null;
  onAutoCheck: (enabled: boolean) => void;
  autoCheckBusy: boolean;
}

/**
 * Settings sync, AI and updates, all off. "Set up now" opens the settings sync's own page in the
 * step (the footer's first button leads back); AI is only explained; the update switch is the
 * question of decision 0037.
 */
function Extras({ syncOpen, onSync, autoCheck, onAutoCheck, autoCheckBusy }: ExtrasProps) {
  const { t } = useTranslation();
  const accounts = useAccounts();
  const sync = useSettingsSyncStatus();
  const page = useRef<HTMLDivElement>(null);
  const syncTitle = useRef<HTMLHeadingElement>(null);
  const opened = useRef(false);

  // Opening the sync page moves the focus to its heading; closing it, back to the sync row.
  useEffect(() => {
    if (syncOpen) {
      opened.current = true;
      const heading = page.current?.querySelector<HTMLElement>("h2");
      if (heading) {
        heading.tabIndex = -1;
        heading.classList.add("outline-none");
        heading.focus();
      }
    } else if (opened.current) {
      opened.current = false;
      syncTitle.current?.focus();
    }
  }, [syncOpen]);

  if (syncOpen) {
    return (
      <div ref={page} className="border-t border-border pt-6" data-testid="first-start-sync-page">
        <SettingsSyncSettings />
      </div>
    );
  }

  const hasAccount = (accounts.data?.length ?? 0) > 0;
  const syncOn = sync.data !== undefined && sync.data.state !== "off";
  return (
    <div className="flex flex-col divide-y divide-border rounded-card border border-border" data-testid="first-start-extras-list">
      <ExtraRow icon={RefreshCw} title={t("firstStart.extras.sync.title")} titleRef={syncTitle}>
        <p>{t("firstStart.extras.sync.body")}</p>
        {!hasAccount && <p className="text-tertiary">{t("firstStart.extras.sync.noAccount")}</p>}
        {hasAccount && syncOn && (
          <p className="flex items-center gap-1 font-medium text-success" data-testid="first-start-sync-on">
            <Check aria-hidden className="size-(--ui-icon-small)" strokeWidth={2} />
            {t("firstStart.extras.sync.on")}
          </p>
        )}
        {hasAccount && !syncOn && (
          <div>
            <Button
              variant="secondary"
              onClick={() => {
                onSync(true);
              }}
              data-testid="first-start-sync"
            >
              {t("firstStart.extras.sync.setUp")}
            </Button>
          </div>
        )}
      </ExtraRow>
      <ExtraRow icon={Sparkles} title={t("firstStart.extras.ai.title")}>
        <p>{t("firstStart.extras.ai.body")}</p>
      </ExtraRow>
      {autoCheck !== null && (
        <div className="flex items-start gap-3 px-4 py-4" data-testid="first-start-updates">
          <IconTile icon={Download} />
          <div className="min-w-0 flex-1 pt-1">
            <Switch
              label={t("updates.autoCheck")}
              description={t("updates.autoCheckHint")}
              checked={autoCheck}
              disabled={autoCheckBusy}
              onChange={onAutoCheck}
              data-testid="first-start-auto-check"
            />
          </div>
        </div>
      )}
    </div>
  );
}

function IconTile({ icon: Icon }: { icon: LucideIcon }) {
  return (
    <span className="flex size-8 shrink-0 items-center justify-center rounded-tile bg-accent-soft text-accent">
      <Icon aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
    </span>
  );
}

function ExtraRow({
  icon,
  title,
  titleRef,
  children,
}: {
  icon: LucideIcon;
  title: string;
  titleRef?: Ref<HTMLHeadingElement>;
  children: ReactNode;
}) {
  return (
    <div className="flex items-start gap-3 px-4 py-4">
      <IconTile icon={icon} />
      <div className="flex min-w-0 flex-1 flex-col gap-2 pt-1 text-13 text-secondary">
        <h2 ref={titleRef} tabIndex={titleRef ? -1 : undefined} className="text-14 font-semibold text-primary outline-none">
          {title}
        </h2>
        {children}
      </div>
    </div>
  );
}
