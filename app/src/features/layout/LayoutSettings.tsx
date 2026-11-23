import { useTranslation } from "react-i18next";
import { ChoiceGroup } from "../../components/ChoiceGroup";
import { Notice } from "../../components/Notice";
import { DENSITIES, READING_PANE_POSITIONS, TEXT_SIZES, useLayoutStore, type TextSize } from "../../stores/layout";

/** Settings → Appearance (stage 8): reading pane position and density of this window. */
export function LayoutSettings() {
  const { t } = useTranslation();
  const readingPane = useLayoutStore((state) => state.readingPane);
  const density = useLayoutStore((state) => state.density);
  const textSize = useLayoutStore((state) => state.textSize);
  const saveFailed = useLayoutStore((state) => state.saveFailed);
  const setLayout = useLayoutStore((state) => state.setLayout);
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
      {saveFailed && <Notice tone="warning" title={t("layout.saveFailed")} />}
    </>
  );
}
