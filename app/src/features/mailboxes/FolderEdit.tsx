import { FolderPlus, Pencil, RefreshCw, Trash2 } from "lucide-react";
import { useEffect, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import { TextField } from "../../components/TextField";
import { ipc, toCoreError, type Account, type Mailbox } from "../../services/ipc";
import { useFolderEditStore, type FolderEditRequest } from "../../stores/folderEdit";
import { newFolderPath } from "../actions/MissingMailboxPrompt";
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
  const run = (kind: "rename" | "delete") => () => {
    if (block === "readOnly") push({ kind: "readOnly", accountId: account.id });
    else if (block === "jmap") push({ kind: "error", context: "folder", error: { code: "unsupported_server", message: t("folderEdit.jmap"), detail: null } });
    else ask({ kind, account, mailbox, ...(returnFocus && { returnFocus }) });
  };
  return [
    { id: "folder-rename", label: t("folderEdit.rename"), icon: Pencil, ...(block && { inert: true }), onSelect: run("rename"), "data-testid": "folder-rename" },
    { id: "folder-delete", label: t("folderEdit.delete"), icon: Trash2, ...(block && { inert: true }), onSelect: run("delete"), "data-testid": "folder-delete" },
  ];
}

/**
 * Stage 18: whether a folder can hold folders here: the server has a hierarchy (a delimiter).
 * Whether the server forbids it for this one (`\Noinferiors`) only the core knows; it refuses.
 */
export function canHoldFolders(mailbox: Mailbox): boolean {
  return Boolean(mailbox.delimiter);
}

/**
 * Stage 18: "New folder…" (at the top of the account, `parent` null) or "New subfolder…" for a
 * menu. On an account that only reads it stays in the menu and says why it does nothing, like
 * renaming; JMAP accounts create folders too. None for a folder that cannot hold folders.
 */
export function useFolderCreateItem(account: Account | undefined, parent: Mailbox | null, returnFocus?: () => void): OverflowItem[] {
  const { t } = useTranslation();
  const ask = useFolderEditStore((state) => state.ask);
  const push = useNoticeStore((state) => state.push);
  if (!account || (parent && !canHoldFolders(parent))) return [];
  const readOnly = !account.writesEnabled;
  return [
    {
      id: parent ? "folder-create-sub" : "folder-create",
      label: t(parent ? "folderEdit.createSub" : "folderEdit.create"),
      icon: FolderPlus,
      ...(readOnly && { inert: true }),
      onSelect: () => {
        if (readOnly) push({ kind: "readOnly", accountId: account.id });
        else ask({ kind: "create", account, parent, ...(returnFocus && { returnFocus }) });
      },
      "data-testid": parent ? "folder-create-sub" : "folder-create",
    },
  ];
}

/** Stage 18: lists the account's folders on the server again now, with a short notice. */
export function useReloadFolders(): (account: Account) => void {
  const push = useNoticeStore((state) => state.push);
  return (account) => {
    ipc.reloadFolders(account.id).then(
      () => {
        push({ kind: "foldersReloading", account: account.displayName || account.email });
      },
      (error: unknown) => {
        push({ kind: "error", context: "folder", error: toCoreError(error) });
      },
    );
  };
}

/** Stage 18: "Reload folders from server" for the account's menu. */
export function useReloadFoldersItem(account: Account): OverflowItem {
  const { t } = useTranslation();
  const reload = useReloadFolders();
  return {
    id: "folders-reload",
    label: t("folderEdit.reload"),
    icon: RefreshCw,
    onSelect: () => {
      reload(account);
    },
    "data-testid": "folders-reload",
  };
}

/** The folder's own name: the last level of its path. */
function leafName(mailbox: Mailbox): string {
  return mailbox.name;
}

type EditRequest = Extract<FolderEditRequest, { kind: "rename" | "delete" }>;
type CreateRequest = Extract<FolderEditRequest, { kind: "create" }>;

/**
 * Stage 18 (task D): "New folder…" and "New subfolder…": the name, checked here as the core
 * checks it (not empty, without the server's delimiter, not taken where it goes); created on the
 * server through the outbox. The folder shows in the tree once the server made it.
 */
function CreatePanel({ request, onClose }: { request: CreateRequest; onClose: () => void }) {
  const { t } = useTranslation();
  const push = useNoticeStore((state) => state.push);
  const { account, parent } = request;
  const mailboxes = useMailboxes(account.id).data ?? [];
  const [name, setName] = useState("");
  const [problem, setProblem] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const delimiter = parent ? (parent.delimiter ?? "") : (mailboxes.find((m) => m.role === "inbox")?.delimiter ?? mailboxes.find((m) => m.delimiter)?.delimiter ?? "");
  useEffect(() => {
    document.querySelector<HTMLInputElement>('[data-testid="folder-create-name"]')?.focus();
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
    const path = parent ? `${parent.path}${delimiter}${trimmed}` : newFolderPath(mailboxes, trimmed);
    if (path.toUpperCase() === "INBOX" || mailboxes.some((m) => m.path === path)) {
      setProblem(t("folderEdit.nameTaken", { name: trimmed }));
      return;
    }
    setBusy(true);
    ipc.createFolder(account.id, parent?.id ?? null, trimmed).then(
      () => {
        push({ kind: "done", action: "create_mailbox", count: 1, detail: trimmed, undo: null });
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
      title={parent ? t("folderEdit.createSubTitle", { name: parent.name }) : t("folderEdit.createTitle", { account: account.displayName || account.email })}
      onClose={onClose}
      restoreFocus={request.returnFocus ?? (() => undefined)}
      data-testid="folder-create-panel"
    >
      <form noValidate onSubmit={submit} className="flex flex-col gap-3">
        <TextField
          label={t("folderEdit.name")}
          value={name}
          onChange={(next) => {
            setName(next);
            setProblem(null);
          }}
          error={problem}
          data-testid="folder-create-name"
        />
        <p className="text-12 text-secondary">{t("folderEdit.createHint")}</p>
        <div className="flex justify-end gap-2">
          <Button variant="quiet" onClick={onClose}>
            {t("folderEdit.cancel")}
          </Button>
          <Button type="submit" variant="primary" disabled={busy} data-testid="folder-create-confirm">
            {t("folderEdit.createButton")}
          </Button>
        </div>
      </form>
    </FloatingPanel>
  );
}

function RenamePanel({ request, onClose }: { request: EditRequest; onClose: () => void }) {
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

function DeleteDialog({ request, onClose }: { request: EditRequest; onClose: () => void }) {
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

/** Shows the open create, rename or delete question (from a folder's or account's menu, or the palette). */
export function FolderEditHost() {
  const request = useFolderEditStore((state) => state.request);
  const close = useFolderEditStore((state) => state.close);
  if (!request) return null;
  if (request.kind === "create") {
    return <CreatePanel key={`create-${request.account.id}-${String(request.parent?.id ?? "top")}`} request={request} onClose={close} />;
  }
  const key = `${request.kind}-${String(request.mailbox.id)}`;
  return request.kind === "rename" ? <RenamePanel key={key} request={request} onClose={close} /> : <DeleteDialog key={key} request={request} onClose={close} />;
}
