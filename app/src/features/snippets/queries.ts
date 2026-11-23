import { useMutation, useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";
import { refreshQuery } from "../../app/refreshQuery";
import {
  snippetsIpc,
  type IdentityId,
  type SignatureId,
  type SignatureInfo,
  type SignatureInput,
  type TemplateId,
  type TemplateInfo,
  type TemplateInput,
} from "../../services/ipc";

// Stage 10: the signature pool, the defaults per identity and the templates (decision 0023).

export const signaturesQueryKey = ["signatures"] as const;
export const templatesQueryKey = ["templates"] as const;
export const signatureDefaultsQueryKey = (identityId: IdentityId) => ["signatureDefaults", identityId] as const;
export const signatureImagesQueryKey = (signatureId: SignatureId) => ["signatureImages", signatureId] as const;

/** Every signature of the pool (shared by all identities); `signaturesChanged` refreshes it. */
export function useSignatures() {
  return useQuery({ queryKey: signaturesQueryKey, queryFn: snippetsIpc.listSignatures });
}

/** Every template; `templatesChanged` refreshes it. */
export function useTemplates(enabled = true) {
  return useQuery({ queryKey: templatesQueryKey, queryFn: snippetsIpc.listTemplates, enabled });
}

export function useSignatureDefaults(identityId: IdentityId | null) {
  return useQuery({
    queryKey: signatureDefaultsQueryKey(identityId ?? -1),
    queryFn: () => snippetsIpc.getIdentitySignatureDefaults(identityId ?? -1),
    enabled: identityId !== null,
  });
}

/** `data:` URLs of a signature's images, for showing them in place of `cid:` (never saved). */
export function useSignatureImages(signatureId: SignatureId | null, enabled = true) {
  return useQuery({
    queryKey: signatureImagesQueryKey(signatureId ?? ""),
    queryFn: () => snippetsIpc.signatureImageData(signatureId ?? ""),
    enabled: enabled && signatureId !== null,
  });
}

function putSignature(queryClient: QueryClient, signature: SignatureInfo): void {
  queryClient.setQueryData<SignatureInfo[]>(signaturesQueryKey, (list) => {
    if (!list) return [signature];
    return list.some((s) => s.id === signature.id) ? list.map((s) => (s.id === signature.id ? signature : s)) : [...list, signature];
  });
}

function putTemplate(queryClient: QueryClient, template: TemplateInfo): void {
  queryClient.setQueryData<TemplateInfo[]>(templatesQueryKey, (list) => {
    if (!list) return [template];
    return list.some((t) => t.id === template.id) ? list.map((t) => (t.id === template.id ? template : t)) : [...list, template];
  });
}

export function useSaveSignature() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: SignatureInput) => snippetsIpc.saveSignature(input),
    onSuccess: (saved) => {
      putSignature(queryClient, saved.signature);
      void queryClient.invalidateQueries({ queryKey: signatureImagesQueryKey(saved.signature.id) });
    },
  });
}

export function useDeleteSignature() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (signatureId: SignatureId) => snippetsIpc.deleteSignature(signatureId),
    onSuccess: (_done, signatureId) => {
      queryClient.setQueryData<SignatureInfo[]>(signaturesQueryKey, (list) => list?.filter((s) => s.id !== signatureId));
    },
  });
}

export function useSetSignatureDefaults() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ identityId, newSignature, replySignature }: { identityId: IdentityId; newSignature: SignatureId | null; replySignature: SignatureId | null }) =>
      snippetsIpc.setIdentitySignatureDefaults(identityId, newSignature, replySignature),
    onSuccess: (saved) => {
      queryClient.setQueryData(signatureDefaultsQueryKey(saved.identityId), saved);
      // The composer reads the defaults from the compose identities.
      void queryClient.invalidateQueries({ queryKey: ["composeIdentities"] });
    },
  });
}

export function useSaveTemplate() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: TemplateInput) => snippetsIpc.saveTemplate(input),
    onSuccess: (saved) => {
      putTemplate(queryClient, saved.template);
    },
  });
}

export function useDeleteTemplate() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (templateId: TemplateId) => snippetsIpc.deleteTemplate(templateId),
    onSuccess: (_done, templateId) => {
      queryClient.setQueryData<TemplateInfo[]>(templatesQueryKey, (list) => list?.filter((t) => t.id !== templateId));
    },
  });
}

/** `signaturesChanged` / `templatesChanged` (also after the settings sync applied a change). */
export function handleSnippetsEvent(queryClient: QueryClient, type: "signaturesChanged" | "templatesChanged"): void {
  if (type === "signaturesChanged") {
    refreshQuery(queryClient, signaturesQueryKey);
    void queryClient.invalidateQueries({ queryKey: ["signatureImages"] });
  } else {
    refreshQuery(queryClient, templatesQueryKey);
  }
}
