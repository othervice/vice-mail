import { FileInput } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { PaletteEntry } from "../palette/entries";
import { openEmlFile } from "./queries";

/** Stage 9 (decision 0027): "Open .eml file…" in the command palette. */
export function useEmlPaletteEntries(): PaletteEntry[] {
  const { t } = useTranslation();
  return [
    {
      key: "eml:open",
      group: "commands",
      label: t("eml.open"),
      icon: FileInput,
      keywords: ["eml", t("eml.keyword")],
      run: () => {
        void openEmlFile();
      },
    },
  ];
}
