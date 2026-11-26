import { useQuery } from "@tanstack/react-query";
import { ChevronRight } from "lucide-react";
import { useDeferredValue, useId, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { ErrorNotice } from "../../components/ErrorNotice";
import { SettingsSection } from "../../components/SettingsSection";
import { cx } from "../../components/cx";
import { toCoreError } from "../../services/ipc";
import { CopyableAddress } from "./CopyableAddress";
import { groupLicenses, licenseTextTitle, loadLicenseData, type LicenseGroup, type LicensePackage } from "./licenses";

const licensesQueryKey = ["openSourceLicenses"] as const;

/**
 * Settings → About, "Open-source licenses" (stage 15): every library the app ships with, grouped
 * by licence, searchable, each one opening to its copyright lines and licence texts. Collapsed
 * until asked for; the list itself is a chunk of its own that loads then.
 */
export function OpenSourceLicenses() {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const panelId = useId();
  return (
    <SettingsSection sub title={t("settings.about.licenses.title")}>
      <p className="text-13 text-secondary">{t("settings.about.licenses.intro")}</p>
      <div>
        <button
          type="button"
          aria-expanded={open}
          {...(open ? { "aria-controls": panelId } : {})}
          onClick={() => {
            setOpen((value) => !value);
          }}
          data-testid="about-licenses-toggle"
          className="inline-flex h-8 shrink-0 items-center justify-center gap-2 rounded-control border border-border bg-button px-3 text-13 font-medium text-primary shadow-row transition-colors hover:bg-hover"
        >
          <ChevronRight
            aria-hidden
            className={cx("size-(--ui-icon-small) shrink-0 text-tertiary", open && "rotate-90")}
            strokeWidth={1.75}
          />
          {open ? t("settings.about.licenses.hide") : t("settings.about.licenses.show")}
        </button>
      </div>
      {open && <LicenseList id={panelId} />}
    </SettingsSection>
  );
}

function LicenseList({ id }: { id: string }) {
  const { t } = useTranslation();
  const searchId = useId();
  const [search, setSearch] = useState("");
  const query = useDeferredValue(search);
  const licenses = useQuery({
    queryKey: licensesQueryKey,
    queryFn: async () => {
      try {
        return await loadLicenseData();
      } catch (error) {
        throw toCoreError(error);
      }
    },
    staleTime: Infinity,
    gcTime: Infinity,
  });
  const data = licenses.data;
  const groups = useMemo(() => (data ? groupLicenses(data.packages, query) : []), [data, query]);
  const shown = groups.reduce((sum, group) => sum + group.packages.length, 0);

  return (
    <div id={id} className="flex flex-col gap-4" data-testid="about-licenses">
      {licenses.isPending && (
        <p role="status" className="text-13 text-secondary">
          {t("settings.about.licenses.loading")}
        </p>
      )}
      {licenses.isError && (
        <ErrorNotice
          context={t("settings.about.licenses.loadFailed")}
          error={licenses.error}
          onRetry={() => {
            void licenses.refetch();
          }}
        />
      )}
      {data && (
        <>
          <div className="flex flex-col gap-1">
            <label htmlFor={searchId} className="text-13 font-medium text-primary">
              {t("settings.about.licenses.search")}
            </label>
            <input
              id={searchId}
              type="search"
              value={search}
              onChange={(event) => {
                setSearch(event.target.value);
              }}
              autoComplete="off"
              spellCheck={false}
              data-testid="about-licenses-search"
              className="h-8 w-full rounded-control border border-border-strong bg-surface-raised px-3 text-14 text-primary"
            />
          </div>
          <p role="status" className="text-12 text-secondary" data-testid="about-licenses-count">
            {query.trim() === ""
              ? t("settings.about.licenses.count", { count: data.packages.length })
              : shown === 0
                ? t("settings.about.licenses.noMatch", { query: query.trim() })
                : t("settings.about.licenses.filtered", { count: shown, total: data.packages.length })}
          </p>
          {groups.map((group) => (
            <LicenseGroupList key={group.license} group={group} texts={data.texts} />
          ))}
        </>
      )}
    </div>
  );
}

function LicenseGroupList({ group, texts }: { group: LicenseGroup; texts: Record<string, string> }) {
  const { t } = useTranslation();
  const headingId = useId();
  return (
    <section aria-labelledby={headingId} className="flex flex-col gap-2">
      <h3 id={headingId} className="text-14 font-semibold text-primary">
        {t("settings.about.licenses.group", { license: group.license, count: group.packages.length })}
      </h3>
      <ul
        aria-labelledby={headingId}
        className="flex flex-col divide-y divide-border-subtle overflow-hidden rounded-card border border-border-subtle bg-surface-raised shadow-card"
      >
        {group.packages.map((pkg) => (
          <LicenseEntry key={`${pkg.ecosystem}:${pkg.name}@${pkg.version}`} pkg={pkg} texts={texts} />
        ))}
      </ul>
    </section>
  );
}

function LicenseEntry({ pkg, texts }: { pkg: LicensePackage; texts: Record<string, string> }) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const detailsId = useId();
  const ecosystem = t(`settings.about.licenses.ecosystem.${pkg.ecosystem}`);
  return (
    <li>
      <button
        type="button"
        aria-expanded={open}
        {...(open ? { "aria-controls": detailsId } : {})}
        onClick={() => {
          setOpen((value) => !value);
        }}
        data-testid={`about-license-${pkg.ecosystem}-${pkg.name}-${pkg.version}`}
        className="flex w-full items-center gap-3 px-4 py-2 text-left transition-colors hover:bg-hover focus-visible:-outline-offset-2"
      >
        <ChevronRight
          aria-hidden
          className={cx("size-(--ui-icon-small) shrink-0 text-tertiary", open && "rotate-90")}
          strokeWidth={1.75}
        />
        <span className="min-w-0 flex-1 truncate text-14 text-primary">{pkg.name}</span>
        <span className="shrink-0 font-mono text-12 text-secondary">{pkg.version}</span>
        <span className="shrink-0 rounded-full bg-badge px-2 text-12 font-medium text-secondary">{ecosystem}</span>
      </button>
      {open && (
        <div id={detailsId} className="flex flex-col gap-4 px-4 pt-1 pb-4 ps-[calc(var(--ui-space-4)+var(--ui-icon-small)+var(--ui-space-3))]" data-testid="about-license-details">
          <dl className="grid grid-cols-[auto_1fr] items-baseline gap-x-6 gap-y-2 text-13">
            <dt className="text-secondary">{t("settings.about.licenses.usedUnder")}</dt>
            <dd className="min-w-0 text-primary select-text">{pkg.license}</dd>
            {pkg.declared !== pkg.license && (
              <>
                <dt className="text-secondary">{t("settings.about.licenses.declared")}</dt>
                <dd className="min-w-0 text-primary select-text">{pkg.declared}</dd>
              </>
            )}
            {pkg.platforms && (
              <>
                <dt className="text-secondary">{t("settings.about.licenses.platforms")}</dt>
                <dd className="min-w-0 text-primary">{pkg.platforms.map((p) => t(`platforms.${p}`)).join(", ")}</dd>
              </>
            )}
            {pkg.repository && (
              <>
                <dt className="text-secondary">{t("settings.about.licenses.source")}</dt>
                <dd className="min-w-0 text-primary">
                  <CopyableAddress url={pkg.repository} copyLabel={t("settings.about.licenses.copySource", { name: pkg.name })} />
                </dd>
              </>
            )}
          </dl>
          {pkg.notices.length > 0 && (
            <div className="flex flex-col gap-1">
              <h4 className="text-13 font-medium text-primary">{t("settings.about.licenses.copyright")}</h4>
              <ul className="flex flex-col gap-1 text-13 text-secondary select-text">
                {pkg.notices.map((notice) => (
                  <li key={notice}>{notice}</li>
                ))}
              </ul>
            </div>
          )}
          {pkg.texts.map((textId) => {
            const title = licenseTextTitle(textId);
            return (
              <div key={textId} className="flex flex-col gap-1">
                <h4 className="text-13 font-medium text-primary">
                  {title.kind === "notice"
                    ? t("settings.about.licenses.notice")
                    : t("settings.about.licenses.text", { license: title.name })}
                </h4>
                <pre className="rounded-control border border-border-subtle bg-surface p-3 font-mono text-12 break-words whitespace-pre-wrap text-primary select-text">
                  {texts[textId]}
                </pre>
              </div>
            );
          })}
        </div>
      )}
    </li>
  );
}
