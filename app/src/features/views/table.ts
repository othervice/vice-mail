import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  toCoreError,
  VIEW_LIMITS,
  viewIpc,
  type ListPresentation,
  type ListTableSettings,
  type SortKey,
  type TableColumn,
  type TableColumnId,
} from "../../services/ipc";
import { useNoticeStore } from "../../stores/notices";

/**
 * Stage 19 (decision 0061): the wide list as a table, its columns and their widths, stored on this
 * device like the window's layout sizes. Widths are CSS pixels at 100 % text size.
 */
export const listTableQueryKey = ["views", "table"] as const;

/** Every column in the table's order, with its defaults, like `TableColumnId::ALL` in the core. */
export const TABLE_COLUMNS: readonly TableColumn[] = [
  { id: "flag", visible: true, width: 32 },
  { id: "attachment", visible: true, width: 32 },
  { id: "correspondent", visible: true, width: 200 },
  { id: "recipient", visible: false, width: 200 },
  { id: "subject", visible: true, width: 160 },
  { id: "account", visible: true, width: 120 },
  { id: "date", visible: true, width: 136 },
  { id: "size", visible: true, width: 88 },
];

export const DEFAULT_LIST_TABLE: ListTableSettings = { presentation: "rows", columns: [...TABLE_COLUMNS] };

/** The mark columns are as wide as their symbol; the subject takes the rest. */
export function isResizable(id: TableColumnId): boolean {
  return id !== "flag" && id !== "attachment" && id !== "subject";
}

export function clampColumnWidth(width: number): number {
  return Math.round(Math.min(VIEW_LIMITS.maxColumnWidth, Math.max(VIEW_LIMITS.minColumnWidth, width)));
}

export function defaultWidth(id: TableColumnId): number {
  return TABLE_COLUMNS.find((column) => column.id === id)?.width ?? 120;
}

/**
 * What a column sorts by; `null` for the marks and the account. The correspondent sorts by who
 * it names: the sender, in Sent and Drafts the recipient.
 */
export function columnSortKey(id: TableColumnId, showRecipients: boolean): SortKey | null {
  switch (id) {
    case "correspondent":
      return showRecipients ? "recipient" : "sender";
    case "recipient":
      return "recipient";
    case "subject":
      return "subject";
    case "date":
      return "date";
    case "size":
      return "size";
    case "flag":
    case "attachment":
    case "account":
      return null;
  }
}

/**
 * The columns a list draws: the shown ones, the account only where rows of several accounts mix,
 * the recipient not twice in Sent and Drafts (the correspondent names them there).
 */
export function drawnColumns(settings: ListTableSettings, options: { mixedAccounts: boolean; showRecipients: boolean }): TableColumn[] {
  return settings.columns.filter((column) => {
    if (!column.visible) return false;
    if (column.id === "account") return options.mixedAccounts;
    if (column.id === "recipient") return !options.showRecipients;
    return true;
  });
}

/** CSS `grid-template-columns` of the drawn columns: fixed widths in rem, the subject flexible. */
export function gridTemplate(columns: readonly TableColumn[]): string {
  return columns
    .map((column) => (column.id === "subject" ? `minmax(${String(column.width / 16)}rem, 1fr)` : `${String(column.width / 16)}rem`))
    .join(" ");
}

export function useListTable() {
  return useQuery({ queryKey: listTableQueryKey, queryFn: () => viewIpc.getTable() });
}

/** The presentation of the wide list (rows until the setting is loaded). */
export function useListPresentation(): ListPresentation {
  return useListTable().data?.presentation ?? "rows";
}

/** Stores the table settings: shown at once, rolled back if the core refuses. */
export function useSetListTable() {
  const queryClient = useQueryClient();
  const push = useNoticeStore((state) => state.push);
  return useMutation({
    mutationFn: (settings: ListTableSettings) => viewIpc.setTable(settings),
    onMutate: async (settings) => {
      await queryClient.cancelQueries({ queryKey: listTableQueryKey });
      const before = queryClient.getQueryData<ListTableSettings>(listTableQueryKey);
      queryClient.setQueryData(listTableQueryKey, settings);
      return { before };
    },
    onSuccess: (settings) => {
      queryClient.setQueryData(listTableQueryKey, settings);
    },
    onError: (error, _settings, context) => {
      queryClient.setQueryData(listTableQueryKey, context?.before);
      push({ kind: "error", context: "action", error: toCoreError(error) });
    },
  });
}

/** `settings` with one column changed. */
export function withColumn(settings: ListTableSettings, id: TableColumnId, patch: Partial<Omit<TableColumn, "id">>): ListTableSettings {
  return { ...settings, columns: settings.columns.map((column) => (column.id === id ? { ...column, ...patch } : column)) };
}
