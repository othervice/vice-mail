import { useQueryClient } from "@tanstack/react-query";
import type { TFunction } from "i18next";
import { ArrowDown, ArrowUp, Flag, Paperclip } from "lucide-react";
import { useEffect, useId, useRef, type Ref } from "react";
import { useTranslation } from "react-i18next";
import type { ChoiceGroup } from "../../components/ChoiceMenu";
import { cx } from "../../components/cx";
import { ResizeHandle } from "../../components/ResizeHandle";
import { VIEW_LIMITS, type ListOrder, type ListTableSettings, type SortKey, type TableColumn, type TableColumnId } from "../../services/ipc";
import { orderFor } from "./order";
import {
  clampColumnWidth,
  columnSortKey,
  defaultWidth,
  drawnColumns,
  gridTemplate,
  isResizable,
  listTableQueryKey,
  useListTable,
  useSetListTable,
  withColumn,
} from "./table";

/** How long a column keeps still before its new width is stored. */
const SAVE_WIDTH_MS = 400;

/** What the list needs to draw itself as a table (decision 0061). */
export interface ListTableConfig {
  /** The drawn columns, in order, with their widths. */
  columns: readonly TableColumn[];
  /** CSS `grid-template-columns` of the header and every row. */
  template: string;
  order: ListOrder;
  showRecipients: boolean;
  /** A click on a column's header: sort by it, or turn its direction around. */
  onSort: (key: SortKey) => void;
  /** A column's handle moved (CSS px at 100 % text size). */
  onResize: (id: TableColumnId, width: number) => void;
}

interface TableConfigOptions {
  order: ListOrder;
  setOrder: (order: ListOrder) => void;
  showRecipients: boolean;
  /** Rows of several accounts mix (the account column makes sense). */
  mixedAccounts: boolean;
}

/**
 * The table of a view while the presentation is "table" (undefined for rows). Sizes go to the
 * query cache at once and to the core once the handle keeps still.
 */
export function useListTableConfig({ order, setOrder, showRecipients, mixedAccounts }: TableConfigOptions): ListTableConfig | undefined {
  const settings = useListTable().data;
  const save = useSetListTable();
  const queryClient = useQueryClient();
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(
    () => () => {
      if (timer.current) clearTimeout(timer.current);
    },
    [],
  );
  if (!settings || settings.presentation !== "table") return undefined;
  const columns = drawnColumns(settings, { mixedAccounts, showRecipients });
  return {
    columns,
    template: gridTemplate(columns),
    order,
    showRecipients,
    onSort: (key) => {
      setOrder(orderFor(order, key, true));
    },
    onResize: (id, width) => {
      const current = queryClient.getQueryData<ListTableSettings>(listTableQueryKey) ?? settings;
      const next = withColumn(current, id, { width: clampColumnWidth(width) });
      queryClient.setQueryData(listTableQueryKey, next);
      if (timer.current) clearTimeout(timer.current);
      timer.current = setTimeout(() => {
        timer.current = null;
        save.mutate(queryClient.getQueryData<ListTableSettings>(listTableQueryKey) ?? next);
      }, SAVE_WIDTH_MS);
    },
  };
}

/**
 * The columns the view menu shows or hides while the list is a table (the subject always shows;
 * the account only where accounts mix, the recipient not in Sent and Drafts, where the first
 * column names the recipients).
 */
export function useColumnChooser({ showRecipients, mixedAccounts }: { showRecipients: boolean; mixedAccounts: boolean }): ChoiceGroup | null {
  const { t } = useTranslation();
  const settings = useListTable().data;
  const save = useSetListTable();
  if (!settings || settings.presentation !== "table") return null;
  const choosable = settings.columns.filter(
    (column) => column.id !== "subject" && (column.id !== "account" || mixedAccounts) && (column.id !== "recipient" || !showRecipients),
  );
  return {
    id: "columns",
    label: t("views.table.columnsGroup"),
    kind: "checkbox",
    items: choosable.map((column) => ({
      id: column.id,
      label: columnName(t, column.id, showRecipients),
      checked: column.visible,
      onSelect: () => {
        save.mutate(withColumn(settings, column.id, { visible: !column.visible }));
      },
      "data-testid": `column-toggle-${column.id}`,
    })),
  };
}

const COLUMN_NAMES = {
  flag: "views.table.columns.flag",
  attachment: "views.table.columns.attachment",
  correspondent: "views.table.columns.correspondent",
  recipient: "views.table.columns.recipient",
  subject: "views.table.columns.subject",
  account: "views.table.columns.account",
  date: "views.table.columns.date",
  size: "views.table.columns.size",
} as const satisfies Record<TableColumnId, string>;

/** A column's name; the correspondent names the recipients in Sent and Drafts. */
export function columnName(t: TFunction, id: TableColumnId, showRecipients: boolean): string {
  return t(id === "correspondent" && showRecipients ? COLUMN_NAMES.recipient : COLUMN_NAMES[id]);
}

interface TableHeaderProps {
  config: ListTableConfig;
  /** The list it belongs to (its name says which). */
  listLabel: string;
  ref?: Ref<HTMLDivElement>;
}

/**
 * The table's column headers above the list (decision 0061). A table of one header row
 * (`columnheader` with `aria-sort` on the sorted one), separate from the list, which stays a
 * listbox or tree whose options name everything a row shows. The sortable headers are buttons;
 * the handle on a column's end is a window splitter (← and → by 16 px, Home and End, a double
 * click back to the default), like the list's border. The view menu shows and hides columns.
 * The columns line up with the rows through the same `--table-columns` and the same scrollbar
 * gutter.
 */
export function TableHeader({ config, listLabel, ref }: TableHeaderProps) {
  const { t } = useTranslation();
  const prefix = useId();
  const scale = (px: number) => `${String(px)} px`;
  return (
    <div
      className="flex shrink-0 items-center overflow-y-hidden border-b border-border bg-surface-list px-2 [scrollbar-gutter:stable]"
      data-testid="table-header"
    >
      <div role="table" aria-label={t("views.table.label", { list: listLabel })} className="min-w-0 flex-1">
        <div role="row" className="flex h-8 items-center gap-3 px-3">
          {/* The selection box's column. */}
          <span aria-hidden className="size-(--ui-icon-small) shrink-0" />
          <div ref={ref} className="grid min-w-0 flex-1 grid-cols-(--table-columns) items-center">
            {config.columns.map((column) => {
              const key = columnSortKey(column.id, config.showRecipients);
              const sorted = key !== null && key === config.order.key;
              const name = columnName(t, column.id, config.showRecipients);
              const id = `${prefix}-${column.id}`;
              const Arrow = config.order.ascending ? ArrowUp : ArrowDown;
              const mark = column.id === "flag" ? Flag : column.id === "attachment" ? Paperclip : null;
              return (
                <div
                  key={column.id}
                  id={id}
                  role="columnheader"
                  // Its name is the column's, not the handle's inside it.
                  aria-label={name}
                  aria-sort={sorted ? (config.order.ascending ? "ascending" : "descending") : undefined}
                  className={cx("relative flex h-8 min-w-0 items-center", mark ? "justify-center" : "pe-3")}
                  data-testid={`column-${column.id}`}
                >
                  {mark ? (
                    <span title={name} className="flex items-center">
                      <span className="sr-only">{name}</span>
                      {(() => {
                        const Mark = mark;
                        return <Mark aria-hidden className="size-(--ui-icon-small) text-tertiary" strokeWidth={1.75} />;
                      })()}
                    </span>
                  ) : key !== null ? (
                    <button
                      type="button"
                      onClick={() => {
                        config.onSort(key);
                      }}
                      className={cx(
                        "-ms-1 flex h-6 min-w-0 items-center gap-1 rounded-control px-1 text-12 font-semibold transition-colors hover:bg-hover hover:text-primary",
                        sorted ? "text-primary" : "text-tertiary",
                        column.id === "size" && "ms-auto",
                      )}
                      data-testid={`sort-by-${column.id}`}
                    >
                      <span className="truncate">{name}</span>
                      {sorted && <Arrow aria-hidden className="size-3 shrink-0" strokeWidth={2.25} />}
                    </button>
                  ) : (
                    <span className="truncate text-12 font-semibold text-tertiary">{name}</span>
                  )}
                  {isResizable(column.id) && (
                    <ResizeHandle
                      orientation="vertical"
                      value={column.width}
                      limits={{ min: VIEW_LIMITS.minColumnWidth, max: VIEW_LIMITS.maxColumnWidth, default: defaultWidth(column.id), step: 16 }}
                      onChange={(width) => {
                        config.onResize(column.id, width);
                      }}
                      label={t("views.table.resize", { column: name })}
                      valueText={scale(column.width)}
                      controls={id}
                      className="absolute inset-y-1 -end-1"
                      data-testid={`column-resize-${column.id}`}
                    />
                  )}
                </div>
              );
            })}
          </div>
        </div>
      </div>
    </div>
  );
}
