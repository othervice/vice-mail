import { useQueryClient } from "@tanstack/react-query";
import { ImagePlus, X } from "lucide-react";
import { lazy, Suspense, useEffect, useEffectEvent, useId, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import { cx } from "../../components/cx";
import { ErrorNotice } from "../../components/ErrorNotice";
import { TextField } from "../../components/TextField";
import {
  composeIpc,
  snippetsIpc,
  toCoreError,
  type CoreError,
  type SanitizeReport,
  type SignatureId,
  type SignatureImageInfo,
  type SignatureInfo,
} from "../../services/ipc";
import type { RichEditorHandle } from "../composer/RichEditor";
import { formatSize } from "../composer/addresses";
import { withoutImage } from "./placeholders";
import { hasNotes, SnippetNotes } from "./SnippetNotes";
import { signatureImagesQueryKey, useDeleteSignature, useSaveSignature, useSignatureImages, useSignatures } from "./queries";
import { SETTING_ANCHOR_CLASS, SETTING_BLOCK, settingAnchor } from "../../components/settingAnchor";

// The editor (TipTap) loads with the first signature that is edited, not with the settings.
const RichEditor = lazy(() => import("../composer/RichEditor").then((module) => ({ default: module.RichEditor })));

/** `seq` keeps the editor (and what it holds) when a new signature gets its id on the first save. */
type Editing = { kind: "new"; seq: number } | { kind: "edit"; id: SignatureId; seq: number };

let nextSeq = 1;

/** Test hook of the editor body (the composer's is `composer-rich-body`). */
const BODY_TEST_ID = "signature-body";

/**
 * Settings → Writing → Signatures: the pool shared by every sender address (decision 0023).
 * HTML with links and up to eight small images; which one an address uses is chosen per
 * identity further down.
 */
export function SignatureSettings() {
  const { t } = useTranslation();
  const headingId = useId();
  const signatures = useSignatures();
  const remove = useDeleteSignature();
  const [editing, setEditing] = useState<Editing | null>(null);
  const [deleting, setDeleting] = useState<SignatureInfo | null>(null);
  const list = signatures.data ?? [];
  const edited = editing?.kind === "edit" ? (list.find((s) => s.id === editing.id) ?? null) : null;

  return (
    <section
      aria-labelledby={headingId}
      className="flex flex-col gap-3 rounded-card border border-border-subtle bg-surface-raised shadow-card p-4"
      data-testid="signature-settings"
      {...SETTING_BLOCK}
    >
      {/* Stage 22 (decision 0073): found by the settings search. */}
      <div {...settingAnchor(t("snippets.signatures.title"))} className={cx("flex flex-col gap-3", SETTING_ANCHOR_CLASS)}>
        <h3 id={headingId} className="text-14 font-semibold text-primary">
          {t("snippets.signatures.title")}
        </h3>
        <p className="text-13 text-secondary">{t("snippets.signatures.hint")}</p>
      </div>
      {signatures.isError && (
        <ErrorNotice context={t("snippets.signatures.loadFailed")} error={signatures.error} onRetry={() => void signatures.refetch()} />
      )}
      {signatures.data && list.length === 0 && editing?.kind !== "new" && (
        <p className="text-13 text-secondary" data-testid="signatures-empty">
          {t("snippets.signatures.empty")}
        </p>
      )}
      {list.length > 0 && (
        <ul aria-label={t("snippets.signatures.list")} className="flex flex-col divide-y divide-border rounded-control border border-border" data-testid="signature-list">
          {list.map((signature) => (
            <li key={signature.id} className="flex items-center gap-3 px-3 py-2" data-testid="signature-item">
              <div className="flex min-w-0 flex-1 flex-col">
                <span className="truncate text-13 font-medium text-primary">{signature.name}</span>
                <span className="truncate text-12 text-secondary">{signature.text.split("\n")[0] || t("snippets.signatures.noText")}</span>
              </div>
              <Button
                variant="quiet"
                onClick={() => {
                  setEditing({ kind: "edit", id: signature.id, seq: nextSeq++ });
                }}
                aria-label={t("snippets.signatures.editNamed", { name: signature.name })}
                data-testid="signature-edit"
              >
                {t("snippets.edit")}
              </Button>
              <Button
                variant="quiet"
                onClick={() => {
                  setDeleting(signature);
                }}
                aria-label={t("snippets.signatures.deleteNamed", { name: signature.name })}
                data-testid="signature-delete"
              >
                {t("snippets.delete")}
              </Button>
            </li>
          ))}
        </ul>
      )}
      {remove.isError && <ErrorNotice context={t("snippets.signatures.deleteFailed")} error={remove.error} data-testid="signature-delete-error" />}
      {editing && (editing.kind === "new" || edited) ? (
        <SignatureEditor
          key={editing.seq}
          signature={edited}
          onClose={() => {
            setEditing(null);
          }}
          onCreated={(id) => {
            setEditing({ kind: "edit", id, seq: editing.seq });
          }}
        />
      ) : (
        signatures.data && (
          <div>
            <Button
              variant="secondary"
              onClick={() => {
                setEditing({ kind: "new", seq: nextSeq++ });
              }}
              data-testid="signature-new"
            >
              {t("snippets.signatures.new")}
            </Button>
          </div>
        )
      )}
      {deleting && (
        <ConfirmDialog
          title={t("snippets.signatures.deleteTitle", { name: deleting.name })}
          body={t("snippets.signatures.deleteBody")}
          confirmLabel={t("snippets.delete")}
          cancelLabel={t("snippets.keep")}
          onConfirm={() => {
            const target = deleting;
            setDeleting(null);
            if (editing?.kind === "edit" && editing.id === target.id) setEditing(null);
            remove.mutate(target.id);
          }}
          onCancel={() => {
            setDeleting(null);
          }}
          data-testid="signature-confirm-delete"
        />
      )}
    </section>
  );
}

interface SignatureEditorProps {
  /** Null for a new signature (it exists once saved). */
  signature: SignatureInfo | null;
  onClose: () => void;
  /** A new signature was saved for the first time (images need its id). */
  onCreated: (id: SignatureId) => void;
}

type EditorError = { context: "save" | "image"; error: CoreError };

function SignatureEditor({ signature, onClose, onCreated }: SignatureEditorProps) {
  const { t, i18n } = useTranslation();
  const queryClient = useQueryClient();
  const save = useSaveSignature();
  const [savedId, setSavedId] = useState<SignatureId | null>(signature?.id ?? null);
  const [name, setName] = useState(signature?.name ?? "");
  const [nameError, setNameError] = useState(false);
  const [html, setHtml] = useState(signature?.html ?? "");
  const [images, setImages] = useState<readonly SignatureImageInfo[]>(signature?.images ?? []);
  const [editorKey, setEditorKey] = useState(0);
  const [notes, setNotes] = useState<SanitizeReport | null>(null);
  const [status, setStatus] = useState<"idle" | "saved">("idle");
  const [error, setError] = useState<EditorError | null>(null);
  const [busy, setBusy] = useState(false);
  const [dropping, setDropping] = useState(false);
  const editor = useRef<RichEditorHandle | null>(null);
  const latestHtml = useRef(html);
  const imagesRef = useRef(images);
  const imageData = useSignatureImages(savedId, images.length > 0);
  const imageSources = useMemo(
    () => Object.fromEntries((imageData.data ?? []).map((image) => [image.contentId, image.dataUrl])),
    [imageData.data],
  );

  const replaceHtml = (next: string) => {
    latestHtml.current = next;
    setHtml(next);
    setEditorKey((n) => n + 1);
  };
  const keepImages = (next: readonly SignatureImageInfo[]) => {
    imagesRef.current = next;
    setImages(next);
  };

  /** Saves name and HTML; answers the signature's id, or null when it did not save. */
  const persist = async (): Promise<SignatureId | null> => {
    if (name.trim() === "") {
      setNameError(true);
      return null;
    }
    setError(null);
    try {
      const result = await save.mutateAsync({ id: savedId, name: name.trim(), html: latestHtml.current });
      const id = result.signature.id;
      keepImages(result.signature.images);
      setNotes(hasNotes(result.notes) ? result.notes : null);
      // Something was removed: show what was kept.
      if (hasNotes(result.notes)) replaceHtml(result.signature.html);
      setStatus("saved");
      if (savedId === null) {
        setSavedId(id);
        onCreated(id);
      }
      return id;
    } catch (e) {
      setError({ context: "save", error: toCoreError(e) });
      return null;
    }
  };

  const run = async (work: () => Promise<unknown>) => {
    setBusy(true);
    try {
      await work();
    } finally {
      setBusy(false);
    }
  };

  /** Adds images (from the core's dialog or a drop) and puts them at the cursor as `cid:`. */
  const addImages = (source: (id: SignatureId) => Promise<SignatureInfo>) =>
    run(async () => {
      // Images belong to a stored signature: a new one is saved first.
      const id = savedId ?? (await persist());
      if (!id) return;
      setError(null);
      try {
        const before = new Set(imagesRef.current.map((image) => image.contentId));
        const info = await source(id);
        const added = info.images.filter((image) => !before.has(image.contentId));
        keepImages(info.images);
        if (added.length === 0) return;
        await queryClient.invalidateQueries({ queryKey: signatureImagesQueryKey(id) });
        const handle = editor.current;
        if (handle) handle.insertImages(added.map((image) => ({ contentId: image.contentId, alt: "" })));
        else replaceHtml(latestHtml.current + added.map((image) => `<p><img src="cid:${image.contentId}" alt=""></p>`).join(""));
        setStatus("idle");
      } catch (e) {
        setError({ context: "image", error: toCoreError(e) });
      }
    });
  const addImage = () => addImages((id) => snippetsIpc.pickSignatureImages(id));

  // Image files dropped on the window while this editor is open go into the signature (paths
  // from the shell's own drop event; the page never reads a file).
  const onDrop = useEffectEvent((paths: string[]) => {
    void addImages((id) => snippetsIpc.addDroppedSignatureImages(id, paths));
  });
  useEffect(() => {
    let active = true;
    let stop: (() => void) | undefined;
    void composeIpc
      .onFileDrop((event) => {
        if (event.type === "enter") setDropping(true);
        else if (event.type === "leave") setDropping(false);
        else if (event.type === "drop") {
          setDropping(false);
          if (event.paths.length > 0) onDrop(event.paths);
        }
      })
      .then((unsubscribe) => {
        if (active) stop = unsubscribe;
        else unsubscribe();
      });
    return () => {
      active = false;
      stop?.();
    };
  }, []);

  const removeImage = (image: SignatureImageInfo) =>
    run(async () => {
      if (!savedId) return;
      setError(null);
      try {
        const info = await snippetsIpc.removeSignatureImage(savedId, image.contentId);
        keepImages(info.images);
        const next = withoutImage(latestHtml.current, image.contentId);
        if (next !== latestHtml.current) replaceHtml(next);
        setStatus("idle");
      } catch (e) {
        setError({ context: "image", error: toCoreError(e) });
      }
    });

  return (
    <div
      className={cx(
        "flex flex-col gap-3 rounded-control border border-border-strong p-3",
        dropping && "outline-2 -outline-offset-2 outline-focus-ring",
      )}
      data-testid="signature-editor"
    >
      {dropping && (
        <p className="rounded-control bg-accent-soft px-3 py-2 text-13 text-primary" data-testid="signature-drop-hint">
          {t("snippets.signatures.dropHere")}
        </p>
      )}
      <TextField
        label={t("snippets.signatures.name")}
        value={name}
        onChange={(value) => {
          setName(value);
          setNameError(false);
          setStatus("idle");
        }}
        error={nameError ? t("snippets.nameRequired") : null}
        data-testid="signature-name"
      />
      <div className="flex flex-col gap-1">
        <span className="text-13 font-medium text-primary">{t("snippets.signatures.body")}</span>
        <div className="flex min-h-(--ui-picker-list-height) flex-col rounded-control border border-border-strong bg-surface">
          <Suspense
            fallback={
              <p role="status" className="px-3 py-2 text-13 text-secondary">
                {t("snippets.loadingEditor")}
              </p>
            }
          >
            <RichEditor
              key={editorKey}
              initialHtml={html}
              onChange={(next) => {
                if (next === latestHtml.current) return;
                latestHtml.current = next;
                setStatus("idle");
              }}
              onBlur={() => undefined}
              label={t("snippets.signatures.body")}
              disabled={busy}
              imageSources={imageSources}
              handleRef={editor}
              testId={BODY_TEST_ID}
            />
          </Suspense>
        </div>
      </div>
      <div className="flex flex-col gap-2">
        <div className="flex flex-wrap items-center gap-3">
          <Button
            variant="secondary"
            disabled={busy}
            onClick={() => {
              void addImage();
            }}
            data-testid="signature-add-image"
          >
            <ImagePlus aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
            {t("snippets.signatures.addImage")}
          </Button>
          <p className="text-12 text-secondary">{t("snippets.signatures.imageLimits")}</p>
          <p className="text-12 text-tertiary">{t("snippets.signatures.dropHint")}</p>
        </div>
        {images.length > 0 && (
          <ul aria-label={t("snippets.signatures.images", { count: images.length })} className="flex flex-wrap gap-2" data-testid="signature-images">
            {images.map((image) => (
              <li
                key={image.contentId}
                className="flex max-w-(--ui-attachment-width) items-center gap-2 rounded-control border border-border bg-canvas py-1 ps-2 pe-1 text-13"
                data-testid="signature-image"
              >
                <span className="min-w-0 truncate text-primary" title={image.fileName}>
                  {image.fileName}
                </span>
                <span className="shrink-0 text-12 text-secondary tabular-nums">{formatSize(image.size, i18n.language)}</span>
                <button
                  type="button"
                  disabled={busy}
                  aria-label={t("snippets.signatures.removeImage", { name: image.fileName })}
                  onClick={() => {
                    void removeImage(image);
                  }}
                  className="flex size-6 shrink-0 items-center justify-center rounded-control text-secondary hover:bg-hover hover:text-primary"
                  data-testid="signature-image-remove"
                >
                  <X aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>
      <SnippetNotes notes={notes} data-testid="signature-notes" />
      {error && (
        <ErrorNotice
          context={t(error.context === "save" ? "snippets.signatures.saveFailed" : "snippets.signatures.imageFailed")}
          error={error.error}
          data-testid="signature-error"
        />
      )}
      <div className="flex items-center gap-3">
        <Button
          disabled={busy}
          onClick={() => {
            void run(persist);
          }}
          data-testid="signature-save"
        >
          {t("snippets.save")}
        </Button>
        <Button variant="secondary" onClick={onClose} data-testid="signature-close">
          {t("snippets.close")}
        </Button>
        {status === "saved" && (
          <p role="status" className="text-12 text-secondary" data-testid="signature-saved">
            {t("snippets.saved")}
          </p>
        )}
      </div>
    </div>
  );
}
