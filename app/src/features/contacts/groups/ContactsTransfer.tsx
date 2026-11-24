import { useMutation } from "@tanstack/react-query";
import { Download, Upload } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "../../../components/Button";
import { ErrorNotice } from "../../../components/ErrorNotice";
import { Notice } from "../../../components/Notice";
import { contactListIpc, type ContactExport, type VcardVersionChoice } from "../../../services/ipc";
import { OverflowMenu, type OverflowItem } from "../../actions/OverflowMenu";
import { refreshContacts } from "../queries";
import { useQueryClient } from "@tanstack/react-query";

interface ContactsTransferProps {
  /** The contact the page shows (export one), if any. */
  contact: { email: string; name: string } | null;
  /** The group the page shows (export one), if any. */
  group: { id: string; name: string } | null;
}

/**
 * Stage 19 (decision 0056): "Import vCard…" and "Export…" in the head of the contacts page. The
 * core shows the system's dialogs and reads or writes the file; the page shows what happened.
 */
export function ContactsTransfer({ contact, group }: ContactsTransferProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const importing = useMutation({
    mutationFn: contactListIpc.importVcards,
    onSuccess: () => {
      refreshContacts(queryClient);
    },
  });
  const exporting = useMutation({
    mutationFn: ({ selection, version }: { selection: ContactExport; version: VcardVersionChoice }) =>
      contactListIpc.exportVcards(selection, version),
  });
  const exportItems: OverflowItem[] = [
    {
      id: "all-v3",
      label: t("contactGroups.transfer.exportAll"),
      icon: Download,
      onSelect: () => {
        exporting.mutate({ selection: { kind: "all", peopleOnly: true }, version: "v3" });
      },
      "data-testid": "contacts-export-all",
    },
    {
      id: "all-v4",
      label: t("contactGroups.transfer.exportAllV4"),
      icon: Download,
      onSelect: () => {
        exporting.mutate({ selection: { kind: "all", peopleOnly: true }, version: "v4" });
      },
      "data-testid": "contacts-export-all-v4",
    },
    ...(contact
      ? [
          {
            id: "contact",
            label: t("contactGroups.transfer.exportContact", { name: contact.name }),
            icon: Download,
            onSelect: () => {
              exporting.mutate({ selection: { kind: "contact", email: contact.email }, version: "v3" });
            },
            "data-testid": "contacts-export-contact",
          },
        ]
      : []),
    ...(group
      ? [
          {
            id: "group",
            label: t("contactGroups.transfer.exportGroup", { name: group.name }),
            icon: Download,
            onSelect: () => {
              exporting.mutate({ selection: { kind: "list", id: group.id }, version: "v3" });
            },
            "data-testid": "contacts-export-group",
          },
        ]
      : []),
  ];
  const report = importing.data?.kind === "imported" ? importing.data.report : null;
  const saved = exporting.data?.kind === "saved" ? exporting.data : null;
  const dismiss = () => {
    importing.reset();
    exporting.reset();
  };
  const shown = report !== null || saved !== null || importing.isError || exporting.isError;
  return (
    <div className="relative flex items-center gap-1" data-testid="contacts-transfer">
      <Button
        variant="quiet"
        disabled={importing.isPending}
        onClick={() => {
          dismiss();
          importing.mutate();
        }}
        data-testid="contacts-import"
      >
        <Upload aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
        {t("contactGroups.transfer.import")}
      </Button>
      <OverflowMenu label={t("contactGroups.transfer.export")} items={exportItems} data-testid="contacts-export-menu" />
      {shown && (
        <div className="absolute end-0 top-full z-20 mt-2 flex w-(--ui-notice-width) max-w-(--ui-notice-width) flex-col gap-2 rounded-card bg-surface-raised shadow-floating">
          {report && (
            <Notice tone="success" role="status" title={t("contactGroups.transfer.imported", { count: report.cards })} data-testid="contacts-import-report">
              <p>
                {t("contactGroups.transfer.importDetail", {
                  created: report.created,
                  updated: report.updated,
                  lists: report.lists,
                  pictures: report.pictures,
                })}
              </p>
              {report.problems.length > 0 && <p>{t("contactGroups.transfer.problems", { count: report.problems.length })}</p>}
              <div>
                <Button variant="quiet" onClick={dismiss}>
                  {t("contactGroups.transfer.dismiss")}
                </Button>
              </div>
            </Notice>
          )}
          {saved && (
            <Notice tone="success" role="status" title={t("contactGroups.transfer.exported", { count: saved.cards })} data-testid="contacts-export-saved">
              <p className="break-all select-text">{saved.path}</p>
              <div>
                <Button variant="quiet" onClick={dismiss}>
                  {t("contactGroups.transfer.dismiss")}
                </Button>
              </div>
            </Notice>
          )}
          {importing.isError && <ErrorNotice context={t("contactGroups.transfer.importFailed")} error={importing.error} />}
          {exporting.isError && <ErrorNotice context={t("contactGroups.transfer.exportFailed")} error={exporting.error} />}
        </div>
      )}
    </div>
  );
}
