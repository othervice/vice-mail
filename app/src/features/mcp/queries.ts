import { useInfiniteQuery, useMutation, useQuery, useQueryClient, type InfiniteData, type QueryClient } from "@tanstack/react-query";
import {
  mcpIpc,
  type AccountId,
  type McpAccessEntry,
  type McpAccessPage,
  type McpAccountAccess,
  type McpSettingsInfo,
  type McpStatus,
} from "../../services/ipc";

export const mcpSettingsQueryKey = ["mcp", "settings"] as const;
export const mcpSetupQueryKey = ["mcp", "setup"] as const;
export const mcpAccessQueryKey = ["mcp", "access"] as const;

/** Rows per page of the access view. */
export const MCP_ACCESS_PAGE_SIZE = 100;

/** Master switch, shares, clients and status of the page "AI over MCP". */
export function useMcpSettings() {
  return useQuery({ queryKey: mcpSettingsQueryKey, queryFn: mcpIpc.getSettings });
}

/** The configurations with the placeholder `<token>` (setup help without a new client). */
export function useMcpSetup() {
  return useQuery({ queryKey: mcpSetupQueryKey, queryFn: mcpIpc.getSetup });
}

/** Every command of the page answers the whole settings; they replace the cached ones. */
function useSettingsMutation<T>(run: (value: T) => Promise<McpSettingsInfo>) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: run,
    onSuccess: (settings) => {
      queryClient.setQueryData(mcpSettingsQueryKey, settings);
    },
  });
}

/** `false` is the emergency stop. */
export function useSetMcpEnabled() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (enabled: boolean) => mcpIpc.setEnabled(enabled),
    onSuccess: async (settings) => {
      queryClient.setQueryData(mcpSettingsQueryKey, settings);
      // The setup help shows the endpoint, which exists only while MCP is on.
      await queryClient.invalidateQueries({ queryKey: mcpSetupQueryKey });
    },
  });
}

export function useSetMcpAccountAccess() {
  return useSettingsMutation(({ accountId, access }: { accountId: AccountId; access: McpAccountAccess }) =>
    mcpIpc.setAccountAccess(accountId, access),
  );
}

export function useRevokeMcpClient() {
  return useSettingsMutation((clientId: string) => mcpIpc.revokeClient(clientId));
}

/**
 * The answer holds the token: it lives only in this mutation's state until the user closes it.
 * `gcTime: 0` drops it from the mutation cache at once after `reset()` or unmounting, instead
 * of keeping it for the default five minutes.
 */
export function useCreateMcpClient() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (name: string) => mcpIpc.createClient(name),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: mcpSettingsQueryKey });
    },
    gcTime: 0,
  });
}

export function useTestMcpConnection() {
  return useMutation({ mutationFn: () => mcpIpc.testConnection() });
}

/** The access log, newest first, page by page; new entries arrive with `mcpAccess`. */
export function useMcpAccess() {
  return useInfiniteQuery({
    queryKey: mcpAccessQueryKey,
    queryFn: ({ pageParam }) => mcpIpc.listAccess(pageParam, MCP_ACCESS_PAGE_SIZE),
    initialPageParam: null as number | null,
    getNextPageParam: (page) => page.nextCursor,
  });
}

/** `mcpStatusChanged`: the switch and the endpoint, without asking the core again. */
export function applyMcpStatus(queryClient: QueryClient, status: McpStatus): void {
  queryClient.setQueryData<McpSettingsInfo>(mcpSettingsQueryKey, (settings) =>
    settings ? { ...settings, enabled: status.enabled, status } : settings,
  );
  void queryClient.invalidateQueries({ queryKey: mcpSetupQueryKey });
}

/**
 * `mcpAccess`: new log entries (oldest first) go on top of the first page. Entries already
 * there are skipped, so an event that overlaps a fresh load adds nothing twice. Clients'
 * "last used" changes with every access, so the client list reloads when it is on screen.
 */
export function applyMcpAccess(queryClient: QueryClient, entries: readonly McpAccessEntry[]): void {
  queryClient.setQueryData<InfiniteData<McpAccessPage, number | null>>(mcpAccessQueryKey, (data) => {
    if (!data) return data;
    const [first, ...rest] = data.pages;
    if (!first) return data;
    const known = new Set(data.pages.flatMap((page) => page.entries.map((entry) => entry.id)));
    const fresh = entries.filter((entry) => !known.has(entry.id)).sort((a, b) => b.id - a.id);
    if (fresh.length === 0) return data;
    return { ...data, pages: [{ ...first, entries: [...fresh, ...first.entries] }, ...rest] };
  });
  void queryClient.invalidateQueries({ queryKey: mcpSettingsQueryKey });
}
