import { ArrowDownWideNarrow, ArrowUpNarrowWide } from "lucide-react";
import { useTranslation } from "react-i18next";
import { ChoiceMenu, type ChoiceGroup } from "../../components/ChoiceMenu";
import type { ListPresentation, SortKey } from "../../services/ipc";
import { isDefaultOrder, sortKeysFor, useListOrder, useSetListOrder } from "./order";
import { useListTable, useSetListTable } from "./table";
import { useColumnChooser } from "./TableHeader";

interface ListViewMenuProps {
  viewKey: string;
  /** Sent and Drafts: rows name the recipients, the recipient is a sort key. */
  showRecipients: boolean;
  /** Rows of several accounts mix (the account column). */
  mixedAccounts: boolean;
}

const PRESENTATIONS: readonly ListPresentation[] = ["rows", "table"];

/**
 * Stage 19 (decision 0061): the view menu in the header of a folder, an inbox or a unified view:
 * what the list sorts by and in which direction (stored for this view), whether the wide list is
 * drawn as rows or as a table, and in a table which columns it shows. The button names the order.
 */
export function ListViewMenu({ viewKey, showRecipients, mixedAccounts }: ListViewMenuProps) {
  const { t } = useTranslation();
  const order = useListOrder(viewKey);
  const setOrder = useSetListOrder();
  const table = useListTable();
  const saveTable = useSetListTable();
  const columns = useColumnChooser({ showRecipients, mixedAccounts });
  const keys = sortKeysFor(showRecipients, order.key);
  const choose = (key: SortKey) => {
    if (key === order.key) return;
    setOrder.mutate({ view: viewKey, order: { key, ascending: key === "sender" || key === "recipient" || key === "subject" } });
  };
  const direction = (ascending: boolean) => {
    if (ascending !== order.ascending) setOrder.mutate({ view: viewKey, order: { ...order, ascending } });
  };
  const directionKind = order.key === "date" ? "date" : order.key === "size" ? "size" : "text";
  const groups: ChoiceGroup[] = [
    {
      id: "key",
      label: t("views.sort.keyGroup"),
      kind: "radio",
      items: keys.map((key) => ({
        id: key,
        label: t(`views.sort.keys.${key}`),
        checked: order.key === key,
        onSelect: () => {
          choose(key);
        },
        "data-testid": `sort-key-${key}`,
      })),
    },
    {
      id: "direction",
      label: t("views.sort.directionGroup"),
      kind: "radio",
      items: [false, true].map((ascending) => ({
        id: ascending ? "ascending" : "descending",
        label: t(`views.sort.direction.${directionKind}.${ascending ? "ascending" : "descending"}`),
        checked: order.ascending === ascending,
        onSelect: () => {
          direction(ascending);
        },
        "data-testid": `sort-${ascending ? "ascending" : "descending"}`,
      })),
    },
  ];
  if (table.data) {
    const settings = table.data;
    groups.push({
      id: "presentation",
      label: t("views.presentation.group"),
      kind: "radio",
      items: PRESENTATIONS.map((presentation) => ({
        id: presentation,
        label: t(`views.presentation.${presentation}`),
        checked: settings.presentation === presentation,
        onSelect: () => {
          if (settings.presentation !== presentation) saveTable.mutate({ ...settings, presentation });
        },
        "data-testid": `presentation-${presentation}`,
      })),
    });
  }
  if (columns) groups.push(columns);
  const name = t(`views.sort.keys.${order.key}`);
  const way = t(`views.sort.direction.${directionKind}.${order.ascending ? "ascending" : "descending"}`);
  return (
    <ChoiceMenu
      label={t("views.sort.menu")}
      icon={order.ascending ? ArrowUpNarrowWide : ArrowDownWideNarrow}
      name={t("views.sort.current", { key: name, direction: way })}
      text={name}
      active={!isDefaultOrder(order)}
      groups={groups}
      data-testid="list-view-menu"
    />
  );
}
