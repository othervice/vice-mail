import { Pencil, Trash2 } from "lucide-react";
import { useEffect, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import { TextField } from "../../components/TextField";
import { ipc, toCoreError, type Account, type Mailbox } from "../../services/ipc";
import { useFolderEditStore, type FolderEditRequest } from "../../stores/folderEdit";
import { useNoticeStore } from "../../stores/notices";
import type { OverflowItem } from "../actions/OverflowMenu";
import { FloatingPanel } from "../actions/Pickers";
import { useMailboxes } from "./queries";

/**
 * Audit 24.11.2026: whether the account may rename and delete its folders here: writes on, and
 * not JMAP (the JMAP adapter cannot do either yet; the core refuses it too).
 */
export function folderEditBlock(account: Account): "readOnly" | "jmap" | null {
  if (account.protocol === "jmap") return "jmap";
  if (!account.writesEnabled) return "readOnly";
  return null;
}

/** Only folders of the user's own (no role) are renamed or deleted. */
export function isEditableFolder(mailbox: Mailbox): boolean {
  return mailbox.role === "custom" && mailbox.path.toUpperCase() !== "INBOX";
}

/**
 * "Rename folder…" and "Delete folder…" for a folder's menu. On an account that only reads, or a
 * JMAP account, they stay in the menu and say why they do nothing.
 */
export function useFolderEditItems(account: Account | undefined, mailbox: Mailbox, returnFocus?: () => void): OverflowItem[] {
  const { t } = useTranslation();
  const ask = useFolderEditStore((state) => state.ask);
  const push = useNoticeStore((state) => state.push);
  if (!account || !isEditableFolder(mailbox)) return [];
  const block = folderEditBlock(account);
  const run = (kind: FolderEditRequest["kind"]) => () => {
    if (block === "readOnly") push({ kind: "readOnly", accountId: account.id });
    else if (block === "jmap") push({ kind: "error", context: "folder", error: { code: "unsupported_server", message: t("folderEdit.jmap"), detail: null } });
    else ask({ kind, account, mailbox, ...(returnFocus && { returnFocus }) });
  };
  return [
    { id: "folder-rename", label: t("folderEdit.rename"), icon: Pencil, ...(block && { inert: true }), onSelect: run("rename"), "data-testid": "folder-rename" },
    { id: "folder-delete", label: t("folderEdit.delete"), icon: Trash2, ...(block && { inert: true }), onSelect: run("delete"), "data-testid": "folder-delete" },
  ];
}

/** The folder's own name: the last level of its path. */
function leafName(mailbox: Mailbox): string {
  return mailbox.name;
}

function RenamePanel({ request, onClose }: { request: FolderEditRequest; onClose: () => void }) {
  const { t } = useTranslation();
  const push = useNoticeStore((state) => state.push);
  const { account, mailbox } = request;
  const [name, setName] = useState(leafName(mailbox));
  const [problem, setProblem] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const delimiter = mailbox.delimiter ?? "";
  // The name is what one changes: it takes the focus, selected.
  useEffect(() => {
    const field = document.querySelector<HTMLInputElement>('[data-testid="folder-rename-name"]');
    field?.focus();
    field?.select();
  }, []);
  const submit = (event: FormEvent) => {
    event.preventDefault();
    const trimmed = name.trim();
    if (!trimmed) {
      setProblem(t("folderEdit.nameRequired"));
      return;
    }
    if (delimiter && trimmed.includes(delimiter)) {
      setProblem(t("folderEdit.nameInvalid", { delimiter }));
      return;
    }
    if (trimmed === leafName(mailbox)) {
      onClose();
      return;
    }
    setBusy(true);
    ipc.renameMailbox(account.id, mailbox.id, trimmed).then(
      () => {
        push({ kind: "done", action: "rename_mailbox", count: 1, detail: leafName(mailbox), undo: null });
        onClose();
      },
      (error: unknown) => {
        setBusy(false);
        push({ kind: "error", context: "folder", error: toCoreError(error) });
      },
    );
  };
  return (
    <FloatingPanel
      title={t("folderEdit.renameTitle", { name: leafName(mailbox) })}
      onClose={onClose}
      restoreFocus={request.returnFocus ?? (() => undefined)}
      data-testid="folder-rename-panel"
    >
      <form noValidate onSubmit={submit} className="flex flex-col gap-3">
        <TextField
          label={t("folderEdit.newName")}
          value={name}
          onChange={(next) => {
            setName(next);
            setProblem(null);
          }}
          error={problem}
          data-testid="folder-rename-name"
        />
        <p className="text-12 text-secondary">{t("folderEdit.renameHint")}</p>
        <div className="flex justify-end gap-2">
          <Button variant="quiet" onClick={onClose}>
            {t("folderEdit.cancel")}
          </Button>
          <Button type="submit" variant="primary" disabled={busy} data-testid="folder-rename-confirm">
            {t("folderEdit.renameButton")}
          </Button>
        </div>
      </form>
    </FloatingPanel>
  );
}

function DeleteDialog({ request, onClose }: { request: FolderEditRequest; onClose: () => void }) {
  const { t } = useTranslation();
  const push = useNoticeStore((state) => state.push);
  const { account, mailbox } = request;
  const mailboxes = useMailboxes(account.id).data ?? [];
  // The count as stored here; the list is the latest the core gave.
  const current = mailboxes.find((m) => m.id === mailbox.id) ?? mailbox;
  const subfolders = mailboxes.some((m) => m.parentId === mailbox.id);
  const name = leafName(mailbox);
  return (
    <ConfirmDialog
      title={t("folderEdit.deleteTitle", { name })}
      body={
        subfolders
          ? t("folderEdit.hasSubfolders", { name })
          : current.total > 0
            ? t("folderEdit.deleteBody", { count: current.total })
            : t("folderEdit.deleteBodyEmpty")
      }
      confirmLabel={t("folderEdit.deleteConfirm")}
      cancelLabel={t("folderEdit.cancel")}
      onConfirm={() => {
        onClose();
        if (subfolders) return;
        ipc.deleteMailbox(account.id, mailbox.id).then(
          () => {
            push({ kind: "done", action: "delete_mailbox", count: 1, detail: name, undo: null });
          },
          (error: unknown) => {
            push({ kind: "error", context: "folder", error: toCoreError(error) });
          },
        );
      }}
      onCancel={onClose}
      {...(request.returnFocus && { focusAfterConfirm: request.returnFocus })}
      data-testid="folder-delete-confirm"
    />
  );
}

/** Shows the open rename or delete question (from a folder's menu or the palette). */
export function FolderEditHost() {
  const request = useFolderEditStore((state) => state.request);
  const close = useFolderEditStore((state) => state.close);
  if (!request) return null;
  const key = `${request.kind}-${String(request.mailbox.id)}`;
  return request.kind === "rename" ? <RenamePanel key={key} request={request} onClose={close} /> : <DeleteDialog key={key} request={request} onClose={close} />;
}
