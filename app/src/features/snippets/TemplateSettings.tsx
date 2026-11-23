import { lazy, Suspense, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import { ErrorNotice } from "../../components/ErrorNotice";
import { Notice } from "../../components/Notice";
import { TextField } from "../../components/TextField";
import { toCoreError, type Address, type CoreError, type SanitizeReport, type TemplateId, type TemplateInfo } from "../../services/ipc";
import type { RichEditorHandle } from "../composer/RichEditor";
// Stage 16: templates with recipients, entered like the composer's.
import { invalidAddressIn } from "../composer/addresses";
import { RecipientField } from "../composer/RecipientField";
import { escapeHtml, placeholderToken, TEMPLATE_PLACEHOLDERS } from "./placeholders";
import { hasNotes, SnippetNotes } from "./SnippetNotes";
import { useDeleteTemplate, useSaveTemplate, useTemplates } from "./queries";

// The editor (TipTap) loads with the first template that is edited, not with the settings.
const RichEditor = lazy(() => import("../composer/RichEditor").then((module) => ({ default: module.RichEditor })));

/** `seq` keeps the editor (and what it holds) when a new template gets its id on the first save. */
type Editing = { kind: "new"; seq: number } | { kind: "edit"; id: TemplateId; seq: number };

let nextSeq = 1;

/** Stage 16: the recipient fields of a template, in the order the core merges them. */
const RECIPIENT_KINDS = ["to", "cc", "bcc"] as const;
type RecipientKind = (typeof RECIPIENT_KINDS)[number];

/** Test hook of the editor body (the composer's is `composer-rich-body`). */
const BODY_TEST_ID = "template-body";

/**
 * Settings → Writing → Templates: subject and text with simple placeholders (the recipient's
 * first name and the like). They are inserted in the composer and from the command palette.
 */
export function TemplateSettings() {
  const { t } = useTranslation();
  const headingId = useId();
  const templates = useTemplates();
  const remove = useDeleteTemplate();
  const [editing, setEditing] = useState<Editing | null>(null);
  const [deleting, setDeleting] = useState<TemplateInfo | null>(null);
  const list = templates.data ?? [];
  const edited = editing?.kind === "edit" ? (list.find((tpl) => tpl.id === editing.id) ?? null) : null;

  return (
    <section aria-labelledby={headingId} className="flex flex-col gap-3 rounded-card border border-border-subtle bg-surface-raised shadow-card p-4" data-testid="template-settings">
      <h3 id={headingId} className="text-14 font-semibold text-primary">
        {t("snippets.templates.title")}
      </h3>
      <p className="text-13 text-secondary">{t("snippets.templates.hint")}</p>
      {templates.isError && (
        <ErrorNotice context={t("snippets.templates.loadFailed")} error={templates.error} onRetry={() => void templates.refetch()} />
      )}
      {templates.data && list.length === 0 && editing?.kind !== "new" && (
        <p className="text-13 text-secondary" data-testid="templates-empty">
          {t("snippets.templates.empty")}
        </p>
      )}
      {list.length > 0 && (
        <ul aria-label={t("snippets.templates.list")} className="flex flex-col divide-y divide-border rounded-control border border-border" data-testid="template-list">
          {list.map((template) => (
            <li key={template.id} className="flex items-center gap-3 px-3 py-2" data-testid="template-item">
              <div className="flex min-w-0 flex-1 flex-col">
                <span className="truncate text-13 font-medium text-primary">{template.name}</span>
                <span className="truncate text-12 text-secondary">{template.subject.trim() || t("composer.noSubject")}</span>
                {template.unknownPlaceholders.length > 0 && (
                  <span className="text-12 text-warning" data-testid="template-item-unknown">
                    {t("snippets.templates.unknownShort", { names: template.unknownPlaceholders.map(placeholderToken).join(", ") })}
                  </span>
                )}
              </div>
              <Button
                variant="quiet"
                onClick={() => {
                  setEditing({ kind: "edit", id: template.id, seq: nextSeq++ });
                }}
                aria-label={t("snippets.templates.editNamed", { name: template.name })}
                data-testid="template-edit"
              >
                {t("snippets.edit")}
              </Button>
              <Button
                variant="quiet"
                onClick={() => {
                  setDeleting(template);
                }}
                aria-label={t("snippets.templates.deleteNamed", { name: template.name })}
                data-testid="template-delete"
              >
                {t("snippets.delete")}
              </Button>
            </li>
          ))}
        </ul>
      )}
      {remove.isError && <ErrorNotice context={t("snippets.templates.deleteFailed")} error={remove.error} data-testid="template-delete-error" />}
      {editing && (editing.kind === "new" || edited) ? (
        <TemplateEditor
          key={editing.seq}
          template={edited}
          onClose={() => {
            setEditing(null);
          }}
          onCreated={(id) => {
            setEditing({ kind: "edit", id, seq: editing.seq });
          }}
        />
      ) : (
        templates.data && (
          <div>
            <Button
              variant="secondary"
              onClick={() => {
                setEditing({ kind: "new", seq: nextSeq++ });
              }}
              data-testid="template-new"
            >
              {t("snippets.templates.new")}
            </Button>
          </div>
        )
      )}
      {deleting && (
        <ConfirmDialog
          title={t("snippets.templates.deleteTitle", { name: deleting.name })}
          body={t("snippets.templates.deleteBody")}
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
          data-testid="template-confirm-delete"
        />
      )}
    </section>
  );
}

interface TemplateEditorProps {
  template: TemplateInfo | null;
  onClose: () => void;
  onCreated: (id: TemplateId) => void;
}

function TemplateEditor({ template, onClose, onCreated }: TemplateEditorProps) {
  const { t } = useTranslation();
  const save = useSaveTemplate();
  const placeholdersId = useId();
  const [savedId, setSavedId] = useState<TemplateId | null>(template?.id ?? null);
  const [name, setName] = useState(template?.name ?? "");
  const [nameError, setNameError] = useState(false);
  const [subject, setSubject] = useState(template?.subject ?? "");
  const [html, setHtml] = useState(template?.html ?? "");
  // Stage 16: To, Cc and Bcc the template adds to a message (refs: a blur commits just before Save).
  const [recipients, setRecipients] = useState<Record<RecipientKind, Address[]>>({
    to: template?.to ?? [],
    cc: template?.cc ?? [],
    bcc: template?.bcc ?? [],
  });
  const latestRecipients = useRef(recipients);
  const recipientsId = useId();
  const recipientsHintId = useId();
  const [editorKey, setEditorKey] = useState(0);
  const [unknown, setUnknown] = useState<readonly string[]>(template?.unknownPlaceholders ?? []);
  const [notes, setNotes] = useState<SanitizeReport | null>(null);
  const [status, setStatus] = useState<"idle" | "saved">("idle");
  const [error, setError] = useState<CoreError | null>(null);
  const editor = useRef<RichEditorHandle | null>(null);
  const latestHtml = useRef(html);

  const persist = async () => {
    if (name.trim() === "") {
      setNameError(true);
      return;
    }
    setError(null);
    try {
      // Empty lists are left out ("none"), so a template without recipients saves as before.
      const { to, cc, bcc } = latestRecipients.current;
      const result = await save.mutateAsync({
        id: savedId,
        name: name.trim(),
        subject,
        html: latestHtml.current,
        ...(to.length > 0 && { to }),
        ...(cc.length > 0 && { cc }),
        ...(bcc.length > 0 && { bcc }),
      });
      setUnknown(result.template.unknownPlaceholders);
      // The core trims, checks and removes doubles: show what it kept.
      const saved = { to: result.template.to, cc: result.template.cc, bcc: result.template.bcc };
      latestRecipients.current = saved;
      setRecipients(saved);
      setNotes(hasNotes(result.notes) ? result.notes : null);
      if (hasNotes(result.notes)) {
        latestHtml.current = result.template.html;
        setHtml(result.template.html);
        setEditorKey((n) => n + 1);
      }
      setStatus("saved");
      if (savedId === null) {
        setSavedId(result.template.id);
        onCreated(result.template.id);
      }
    } catch (e) {
      setError(toCoreError(e));
    }
  };

  const changeRecipients = (kind: RecipientKind, addresses: Address[]) => {
    const next = { ...latestRecipients.current, [kind]: addresses };
    latestRecipients.current = next;
    setRecipients(next);
    setStatus("idle");
    setError(null);
  };
  const refused =
    error?.code === "invalid_address" ? invalidAddressIn(error.detail, [recipients.to, recipients.cc, recipients.bcc]) : null;

  const insertPlaceholder = (placeholder: string) => {
    const token = escapeHtml(placeholderToken(placeholder));
    const handle = editor.current;
    if (handle) {
      handle.insertHtml(token);
    } else {
      // The editor is still loading: add it at the end.
      latestHtml.current = `${latestHtml.current}<p>${token}</p>`;
      setHtml(latestHtml.current);
      setEditorKey((n) => n + 1);
    }
    setStatus("idle");
  };

  return (
    <div className="flex flex-col gap-3 rounded-control border border-border-strong p-3" data-testid="template-editor">
      <TextField
        label={t("snippets.templates.name")}
        value={name}
        onChange={(value) => {
          setName(value);
          setNameError(false);
          setStatus("idle");
        }}
        error={nameError ? t("snippets.nameRequired") : null}
        data-testid="template-name"
      />
      <TextField
        label={t("snippets.templates.subject")}
        value={subject}
        onChange={(value) => {
          setSubject(value);
          setStatus("idle");
        }}
        description={t("snippets.templates.subjectHint", { example: placeholderToken("first_name") })}
        data-testid="template-subject"
      />
      <div role="group" aria-labelledby={recipientsId} aria-describedby={recipientsHintId} className="flex flex-col gap-1">
        <span id={recipientsId} className="text-13 font-medium text-primary">
          {t("snippets.templates.recipients")}
        </span>
        <div className="flex flex-col rounded-control border border-border-strong bg-surface [&>*:last-child]:border-b-0" data-testid="template-recipients">
          {RECIPIENT_KINDS.map((kind) => (
            <RecipientField
              key={kind}
              label={t(`composer.recipients.${kind}`)}
              addresses={recipients[kind]}
              onChange={(addresses) => {
                changeRecipients(kind, addresses);
              }}
              onBlur={() => undefined}
              refused={refused}
              data-testid={`template-${kind}`}
            />
          ))}
        </div>
        <p id={recipientsHintId} className="text-12 text-secondary">
          {t("snippets.templates.recipientsHint")}
        </p>
      </div>
      <div className="flex flex-col gap-1">
        <span className="text-13 font-medium text-primary">{t("snippets.templates.body")}</span>
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
              label={t("snippets.templates.body")}
              handleRef={editor}
              testId={BODY_TEST_ID}
            />
          </Suspense>
        </div>
      </div>
      <div className="flex flex-col gap-2">
        <p id={placeholdersId} className="text-13 font-medium text-primary">
          {t("snippets.templates.placeholders")}
        </p>
        <div role="group" aria-labelledby={placeholdersId} className="flex flex-wrap gap-1" data-testid="template-placeholders">
          {TEMPLATE_PLACEHOLDERS.map((placeholder) => (
            <button
              key={placeholder}
              type="button"
              // Keep the cursor in the editor: the button must not take the focus on mouse down.
              onMouseDown={(e) => {
                e.preventDefault();
              }}
              onClick={() => {
                insertPlaceholder(placeholder);
              }}
              aria-label={t("snippets.templates.insertPlaceholder", { name: t(`snippets.placeholders.${placeholder}`) })}
              title={t(`snippets.placeholders.${placeholder}`)}
              className="rounded-control border border-border px-2 py-1 font-mono text-12 text-primary hover:bg-hover"
              data-testid={`template-placeholder-${placeholder}`}
            >
              {placeholderToken(placeholder)}
            </button>
          ))}
        </div>
        <p className="text-12 text-secondary">
          {t("snippets.templates.fallbackHint", {
            example: placeholderToken(`first_name|${t("snippets.templates.fallbackWord")}`),
            word: t("snippets.templates.fallbackWord"),
          })}
        </p>
      </div>
      {unknown.length > 0 && (
        <Notice tone="warning" role="status" title={t("snippets.templates.unknownTitle")} data-testid="template-unknown">
          <p>{t("snippets.templates.unknownBody", { names: unknown.map(placeholderToken).join(", ") })}</p>
        </Notice>
      )}
      <SnippetNotes notes={notes} data-testid="template-notes" />
      {error && <ErrorNotice context={t("snippets.templates.saveFailed")} error={error} data-testid="template-error" />}
      <div className="flex items-center gap-3">
        <Button
          disabled={save.isPending}
          onClick={() => {
            void persist();
          }}
          data-testid="template-save"
        >
          {t("snippets.save")}
        </Button>
        <Button variant="secondary" onClick={onClose} data-testid="template-close">
          {t("snippets.close")}
        </Button>
        {status === "saved" && (
          <p role="status" className="text-12 text-secondary" data-testid="template-saved">
            {t("snippets.saved")}
          </p>
        )}
      </div>
    </div>
  );
}
