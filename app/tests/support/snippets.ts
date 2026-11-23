// Stage 10: the fake core's signature pool, identity defaults and templates, with the command
// names and argument shapes of the stage 10 IPC contract (decision 0023).
// At most this many images per signature: the core's constant (decision 0025).
import { MAX_SIGNATURE_IMAGES } from "../../src/services/ipc";
import type {
  Draft,
  FilledTemplateInfo,
  IdentitySignatureDefaults,
  IpcError,
  SanitizeReport,
  SavedSignature,
  SavedTemplate,
  SignatureImageData,
  SignatureImageInfo,
  SignatureInfo,
  SignatureInput,
  TemplateFillProblem,
  TemplateInfo,
  TemplateInput,
} from "../../src/services/ipc";

export function snippetsError(code: IpcError["code"], detail: string | null = null): IpcError {
  return { code, message: `English fallback for ${code}`, detail };
}

/** The placeholders the core knows (decision 0023). */
const KNOWN = ["first_name", "last_name", "name", "email", "date", "my_name", "my_first_name", "my_email"];


export function makeSignature(overrides: Partial<SignatureInfo> = {}): SignatureInfo {
  const html = overrides.html ?? "<p>Ada Lovelace</p>";
  return {
    id: "sig-1",
    name: "Work",
    html,
    text: htmlText(html),
    images: [],
    updatedAt: 1_700_000_000_000,
    ...overrides,
  };
}

export function makeTemplate(overrides: Partial<TemplateInfo> = {}): TemplateInfo {
  const subject = overrides.subject ?? "Thanks";
  const html = overrides.html ?? "<p>Hello {{first_name}}</p>";
  return {
    id: "tpl-1",
    name: "Thank you",
    subject,
    html,
    to: [],
    cc: [],
    bcc: [],
    ...placeholdersOf(`${subject} ${html}`),
    updatedAt: 1_700_000_000_000,
    ...overrides,
  };
}

function htmlText(html: string): string {
  return html
    .replace(/<\/(p|div|h[1-6]|li)>/g, "\n")
    .replace(/<br\s*\/?>/g, "\n")
    .replace(/<[^>]+>/g, "")
    .trim();
}

function placeholdersOf(text: string): { placeholders: string[]; unknownPlaceholders: string[] } {
  const names = [...text.matchAll(/\{\{\s*([a-z_]+)(?:\|[^}]*)?\s*\}\}/gi)].map((m) => (m[1] ?? "").toLowerCase());
  const unique = [...new Set(names)];
  return { placeholders: unique.filter((n) => KNOWN.includes(n)), unknownPlaceholders: unique.filter((n) => !KNOWN.includes(n)) };
}

/** What the allowlist does in the fake: drops non-`cid:` images and style attributes. */
function sanitize(html: string): { html: string; notes: SanitizeReport } {
  let imagesRemoved = 0;
  let out = html.replace(/<img\b[^>]*>/g, (tag) => {
    if (/src="cid:/.test(tag)) return tag;
    imagesRemoved += 1;
    return "";
  });
  const formattingSimplified = /\sstyle="(?!text-align)/.test(out) || /<font\b|<table\b/.test(out);
  out = out.replace(/\sstyle="(?!text-align)[^"]*"/g, "");
  return { html: out, notes: { imagesRemoved, formattingSimplified } };
}

export interface SnippetsFakeOptions {
  signatures?: SignatureInfo[];
  templates?: TemplateInfo[];
  /** Identity defaults by identity id (default: none for both). */
  defaults?: Record<number, Omit<IdentitySignatureDefaults, "identityId">>;
  /** What the system dialog "picks" on `pickSignatureImages` (empty: the user cancelled). */
  pickedSignatureImages?: SignatureImageInfo[];
  /** Overrides what sanitising reports on every save. */
  notes?: SanitizeReport;
  /** The user's own name and address for `{{my_*}}` in `fillTemplate`. */
  me?: { name: string; email: string };
}

export interface SnippetsFakeState {
  readonly signatures: readonly SignatureInfo[];
  readonly templates: readonly TemplateInfo[];
  defaultsOf: (identityId: number) => IdentitySignatureDefaults;
}

export interface SnippetsFakeDeps {
  /** The compose fake's drafts (`fillTemplate` reads the first To recipient). */
  drafts: () => ReadonlyMap<string, Draft>;
}

export function createSnippetsFake(options: SnippetsFakeOptions, deps: SnippetsFakeDeps) {
  let signatures: SignatureInfo[] = [...(options.signatures ?? [])];
  let templates: TemplateInfo[] = [...(options.templates ?? [])];
  const defaults = new Map<number, IdentitySignatureDefaults>(
    Object.entries(options.defaults ?? {}).map(([id, value]) => [Number(id), { identityId: Number(id), ...value }]),
  );
  let nextSignature = signatures.length + 1;
  let nextTemplate = templates.length + 1;
  let clock = 1_700_000_100_000;

  const reject = (code: IpcError["code"], detail: string | null = null) =>
    // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
    Promise.reject(snippetsError(code, detail));

  const defaultsOf = (identityId: number): IdentitySignatureDefaults =>
    defaults.get(identityId) ?? { identityId, newSignature: null, replySignature: null };

  const putSignature = (signature: SignatureInfo): SignatureInfo => {
    signatures = signatures.some((s) => s.id === signature.id)
      ? signatures.map((s) => (s.id === signature.id ? signature : s))
      : [...signatures, signature];
    return { ...signature, images: [...signature.images] };
  };

  function saveSignature(input: SignatureInput): SavedSignature | Promise<never> {
    if (input.name.trim() === "") return reject("invalid_argument", "a signature needs a name");
    const existing = input.id === null ? undefined : signatures.find((s) => s.id === input.id);
    if (input.id !== null && !existing) return reject("not_found");
    const cleaned = sanitize(input.html);
    // Images the HTML no longer shows are dropped on save.
    const images = (existing?.images ?? []).filter((image) => cleaned.html.includes(`cid:${image.contentId}`));
    const signature = putSignature({
      id: existing?.id ?? `sig-${String(nextSignature++)}`,
      name: input.name.trim(),
      html: cleaned.html,
      text: htmlText(cleaned.html),
      images,
      updatedAt: clock++,
    });
    return { signature, notes: options.notes ?? cleaned.notes };
  }

  function addImages(signatureId: string, added: readonly SignatureImageInfo[]): SignatureInfo | Promise<never> {
    const signature = signatures.find((s) => s.id === signatureId);
    if (!signature) return reject("not_found");
    const fresh = added.filter((image) => !signature.images.some((i) => i.contentId === image.contentId));
    if (signature.images.length + fresh.length > MAX_SIGNATURE_IMAGES) return reject("attachment_too_large", "at most 8 images");
    return putSignature({ ...signature, images: [...signature.images, ...fresh], updatedAt: clock++ });
  }

  function saveTemplate(input: TemplateInput): SavedTemplate | Promise<never> {
    if (input.name.trim() === "") return reject("invalid_argument", "a template needs a name");
    if (/<img\b/.test(input.html)) return reject("invalid_argument", "templates contain no images");
    const existing = input.id === null ? undefined : templates.find((t) => t.id === input.id);
    if (input.id !== null && !existing) return reject("not_found");
    const cleaned = sanitize(input.html);
    const template: TemplateInfo = {
      id: existing?.id ?? `tpl-${String(nextTemplate++)}`,
      name: input.name.trim(),
      subject: input.subject,
      html: cleaned.html,
      to: input.to ?? [],
      cc: input.cc ?? [],
      bcc: input.bcc ?? [],
      ...placeholdersOf(`${input.subject} ${cleaned.html}`),
      updatedAt: clock++,
    };
    templates = existing ? templates.map((t) => (t.id === template.id ? template : t)) : [...templates, template];
    return { template: { ...template }, notes: options.notes ?? cleaned.notes };
  }

  function fillTemplate(templateId: string, draftId: string): FilledTemplateInfo | Promise<never> {
    const template = templates.find((t) => t.id === templateId);
    if (!template) return reject("not_found");
    const draft = deps.drafts().get(draftId);
    if (!draft) return reject("not_found");
    const to = draft.to[0];
    const [first, ...rest] = (to?.name ?? "").split(/\s+/).filter(Boolean);
    const values: Record<string, string | undefined> = {
      first_name: first,
      last_name: rest.length > 0 ? rest.join(" ") : undefined,
      name: to?.name ?? undefined,
      email: to?.email,
      date: "23.11.2026",
      my_name: options.me?.name,
      my_first_name: options.me?.name.split(/\s+/)[0],
      my_email: options.me?.email,
    };
    const problems: TemplateFillProblem[] = [];
    const fill = (text: string) =>
      text.replace(/\{\{\s*([a-z_]+)(?:\|([^}]*))?\s*\}\}/gi, (whole, rawName: string, fallback: string | undefined) => {
        const name = rawName.toLowerCase();
        if (!KNOWN.includes(name)) {
          if (!problems.some((p) => p.kind === "unknown" && p.name === name)) problems.push({ kind: "unknown", name });
          return whole;
        }
        const value = values[name];
        if (value) return value;
        if (fallback !== undefined) return fallback;
        if (!problems.some((p) => p.kind === "missing" && p.placeholder === name)) problems.push({ kind: "missing", placeholder: name });
        return "";
      });
    const html = fill(template.html);
    return {
      subject: fill(template.subject),
      html,
      text: htmlText(html),
      problems,
      to: draft.to,
      cc: draft.cc,
      bcc: draft.bcc,
      recipientsAdded: 0,
    };
  }

  function handle(cmd: string, args: Record<string, unknown>): unknown {
    switch (cmd) {
      case "list_signatures":
        return signatures.map((s) => ({ ...s, images: [...s.images] }));
      case "save_signature":
        return saveSignature(args["signature"] as SignatureInput);
      case "delete_signature": {
        const id = args["signatureId"] as string;
        if (!signatures.some((s) => s.id === id)) return reject("not_found");
        signatures = signatures.filter((s) => s.id !== id);
        return null;
      }
      case "get_identity_signature_defaults":
        return defaultsOf(args["identityId"] as number);
      case "set_identity_signature_defaults": {
        const identityId = args["identityId"] as number;
        const next: IdentitySignatureDefaults = {
          identityId,
          newSignature: args["newSignature"] as string | null,
          replySignature: args["replySignature"] as string | null,
        };
        for (const id of [next.newSignature, next.replySignature]) {
          if (id !== null && !signatures.some((s) => s.id === id)) return reject("not_found");
        }
        defaults.set(identityId, next);
        return next;
      }
      case "pick_signature_images":
        return addImages(args["signatureId"] as string, options.pickedSignatureImages ?? []);
      case "add_dropped_signature_images": {
        const paths = args["paths"] as string[];
        return addImages(
          args["signatureId"] as string,
          paths.map((path, i) => ({
            contentId: `drop${String(i)}@vicemail.test`,
            fileName: path.split("/").pop() ?? path,
            mime: "image/png",
            size: 1000,
          })),
        );
      }
      case "remove_signature_image": {
        const signature = signatures.find((s) => s.id === args["signatureId"]);
        if (!signature) return reject("not_found");
        return putSignature({ ...signature, images: signature.images.filter((i) => i.contentId !== args["contentId"]) });
      }
      case "signature_image_data": {
        const signature = signatures.find((s) => s.id === args["signatureId"]);
        if (!signature) return reject("not_found");
        return signature.images.map(
          (image): SignatureImageData => ({ contentId: image.contentId, dataUrl: `data:${image.mime};base64,AAAA` }),
        );
      }
      case "list_templates":
        return templates.map((t) => ({ ...t }));
      case "save_template":
        return saveTemplate(args["template"] as TemplateInput);
      case "delete_template": {
        const id = args["templateId"] as string;
        if (!templates.some((t) => t.id === id)) return reject("not_found");
        templates = templates.filter((t) => t.id !== id);
        return null;
      }
      case "fill_template":
        return fillTemplate(args["templateId"] as string, args["draftId"] as string);
      default:
        return undefined;
    }
  }

  const state: SnippetsFakeState = {
    get signatures() {
      return signatures;
    },
    get templates() {
      return templates;
    },
    defaultsOf,
  };
  return { handle, state };
}
