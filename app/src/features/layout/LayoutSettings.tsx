import { useTranslation } from "react-i18next";
import { ChoiceGroup } from "../../components/ChoiceGroup";
import { ErrorNotice } from "../../components/ErrorNotice";
import { DENSITIES, READING_PANE_POSITIONS, TEXT_SIZES, useLayoutStore, type TextSize } from "../../stores/layout";
import { useWindowLayoutLoad } from "../keyboard/useUiPreferencesSync";
import { useListTable, useSetListTable } from "../views/table";

/** Stage 19: how the wide list is drawn. */
const PRESENTATIONS = ["rows", "table"] as const;

/** Settings → Appearance (stage 8): reading pane position and density of this window. */
export function LayoutSettings() {
  const { t } = useTranslation();
  const readingPane = useLayoutStore((state) => state.readingPane);
  const density = useLayoutStore((state) => state.density);
  const textSize = useLayoutStore((state) => state.textSize);
  const saveError = useLayoutStore((state) => state.saveError);
  const load = useWindowLayoutLoad();
  const setLayout = useLayoutStore((state) => state.setLayout);
  const table = useListTable();
  const saveTable = useSetListTable();
  return (
    <>
      <ChoiceGroup
        label={t("layout.readingPane.label")}
        description={t("layout.readingPane.description")}
        value={readingPane}
        options={READING_PANE_POSITIONS.map((value) => ({ value, label: t(`layout.readingPane.${value}`) }))}
        onChange={(next) => {
          setLayout({ readingPane: next });
        }}
      />
      <ChoiceGroup
        label={t("layout.density.label")}
        description={t("layout.density.description")}
        value={density}
        options={DENSITIES.map((value) => ({ value, label: t(`layout.density.${value}`) }))}
        onChange={(next) => {
          setLayout({ density: next });
        }}
      />
      {/* Stage 19 (decision 0061): the wide list as rows or as a table. */}
      {table.data && (
        <ChoiceGroup
          label={t("views.presentation.label")}
          description={t("views.presentation.description")}
          value={table.data.presentation}
          options={PRESENTATIONS.map((value) => ({ value, label: t(`views.presentation.${value}`) }))}
          onChange={(presentation) => {
            if (table.data) saveTable.mutate({ ...table.data, presentation });
          }}
        />
      )}
      <ChoiceGroup
        label={t("layout.textSize.label")}
        description={t("layout.textSize.description")}
        value={String(textSize)}
        options={TEXT_SIZES.map((value) => ({ value: String(value), label: t("layout.textSize.option", { percent: value }) }))}
        onChange={(next) => {
          const size = TEXT_SIZES.find((value) => String(value) === next);
          if (size) setLayout({ textSize: size satisfies TextSize });
        }}
      />
      {load.isError && (
        <ErrorNotice context={t("layout.loadFailed")} error={load.error} onRetry={() => void load.refetch()} data-testid="layout-load-error" />
      )}
      {saveError && <ErrorNotice context={t("layout.saveFailed")} error={saveError} data-testid="layout-save-error" />}
    </>
  );
}
