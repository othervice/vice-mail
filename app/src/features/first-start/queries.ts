import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { firstStartIpc, type FirstStart, type FirstStartEnd } from "../../services/ipc";

/** Stage 15 (decision 0044): whether the main window shows the guided first start. */
export const firstStartQueryKey = ["firstStart"] as const;

export function useFirstStart() {
  return useQuery({ queryKey: firstStartQueryKey, queryFn: firstStartIpc.get });
}

/** "Skip setup" or the last step. The answer replaces the cached state. */
export function useFinishFirstStart() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (end: FirstStartEnd): Promise<FirstStart> => firstStartIpc.finish(end),
    onSuccess: (state) => {
      queryClient.setQueryData(firstStartQueryKey, state);
    },
  });
}
