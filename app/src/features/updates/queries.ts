import { useMutation, useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";
import { updateIpc, type CoreEvent, type UpdateInfo } from "../../services/ipc";

/** Stage 15 (decision 0037): the update setting, the one-time question and the state. */
export const updateInfoQueryKey = ["updateInfo"] as const;

export function useUpdateInfo() {
  return useQuery({ queryKey: updateInfoQueryKey, queryFn: updateIpc.getInfo });
}

type UpdateStep = "setAutoCheck" | "checkNow" | "download" | "install";

/** Every update command answers the whole info; it replaces the cached one. */
export function useUpdateStep(step: UpdateStep) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (enabled?: boolean): Promise<UpdateInfo> => {
      switch (step) {
        case "setAutoCheck":
          return updateIpc.setAutoCheck(enabled === true);
        case "checkNow":
          return updateIpc.checkNow();
        case "download":
          return updateIpc.download();
        case "install":
          return updateIpc.install();
      }
    },
    onSuccess: (info) => {
      queryClient.setQueryData(updateInfoQueryKey, info);
    },
  });
}

/**
 * `updateStatusChanged` carries the new state; the rest of the info (last check) is fetched again
 * once the step is over. Download progress only updates the state. A new account may bring the
 * one-time question.
 */
export function handleUpdateEvent(queryClient: QueryClient, event: CoreEvent): void {
  if (event.type === "updateStatusChanged") {
    queryClient.setQueryData<UpdateInfo>(updateInfoQueryKey, (info) => (info ? { ...info, status: event.status } : info));
    if (event.status.state !== "downloading" && event.status.state !== "checking") {
      void queryClient.invalidateQueries({ queryKey: updateInfoQueryKey });
    }
  } else if (event.type === "accountsChanged") {
    void queryClient.invalidateQueries({ queryKey: updateInfoQueryKey });
  }
}
