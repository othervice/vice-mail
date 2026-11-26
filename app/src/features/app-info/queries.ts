import { useQuery } from "@tanstack/react-query";
import { ipc } from "../../services/ipc";

export const appInfoQueryKey = ["appInfo"] as const;

export function useAppInfo() {
  return useQuery({ queryKey: appInfoQueryKey, queryFn: ipc.getAppInfo });
}

export const windowBackdropQueryKey = ["windowBackdrop"] as const;

/** Stage 22 (decision 0079): what Windows draws behind this window's transparent chrome. */
export function useWindowBackdrop() {
  return useQuery({ queryKey: windowBackdropQueryKey, queryFn: ipc.getWindowBackdrop, staleTime: Infinity });
}
