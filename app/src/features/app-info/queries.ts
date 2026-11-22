import { useQuery } from "@tanstack/react-query";
import { ipc } from "../../services/ipc";

export const appInfoQueryKey = ["appInfo"] as const;

export function useAppInfo() {
  return useQuery({ queryKey: appInfoQueryKey, queryFn: ipc.getAppInfo });
}
