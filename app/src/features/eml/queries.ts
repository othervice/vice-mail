import { keepPreviousData, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { emlIpc, toCoreError, type BodyFormat, type EmlFileId } from "../../services/ipc";
import { useNoticeStore } from "../../stores/notices";

/** Like the reading pane's views: the core keeps the link targets of about 32 renderings. */
const EML_VIEW_GC_MS = 30_000;

export const emlViewQueryKey = (fileId: EmlFileId, format: BodyFormat) => ["eml", fileId, format] as const;

/**
 * Stage 9 (decision 0027): what the window of an opened `.eml` file shows. The core answers from
 * the bytes it read once; a file the system handed over that is no mail answers its typed error.
 * Switching the format keeps the previous rendering on screen until the new one is there.
 */
export function useEmlView(fileId: EmlFileId, format: BodyFormat) {
  return useQuery({
    queryKey: emlViewQueryKey(fileId, format),
    queryFn: () => emlIpc.getMessage(fileId, format),
    gcTime: EML_VIEW_GC_MS,
    placeholderData: keepPreviousData,
    retry: false,
  });
}

/** External content of this rendering, loaded once by the core (there is no sender list without an account). */
export function useLoadEmlRemoteContent(fileId: EmlFileId, format: BodyFormat) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => emlIpc.loadRemoteContent(fileId),
    onSuccess: (view) => {
      queryClient.setQueryData(emlViewQueryKey(fileId, format), view);
    },
  });
}

/**
 * "Open .eml file…": the core's open dialog; the file opens in a window of its own. A refusal
 * (too large, not a mail, unreadable) is a notice with the reason; cancelling says nothing.
 */
export function openEmlFile(): Promise<void> {
  return emlIpc.openFile().then(
    () => undefined,
    (error: unknown) => {
      useNoticeStore.getState().push({ kind: "error", context: "emlFile", error: toCoreError(error) });
    },
  );
}
