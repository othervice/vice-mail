/*
 * Stage 22 (decision 0073): what the settings search can jump to. Every option of the settings
 * (a switch, a choice, a field, a block or sub-section with its heading) marks its row with its
 * title as shown (`data-setting-title`); the search finds the row by that title in the section it
 * opened, scrolls it into view, moves the focus to it and marks it for a moment
 * (`data-setting-found`). The texts come from the same i18n keys the row renders
 * (`features/settings/search/settingsIndex.ts`); `tests/settings-search.test.tsx` checks that
 * every row in every section is in the index and that the index says what the row shows.
 */

/**
 * The row's look while it is the search's target: the soft accent behind it, drawn 8 px around
 * it (the negative margin keeps the layout), faded in and out like every colour (none under
 * "reduce motion").
 */
export const SETTING_ANCHOR_CLASS = "-m-2 rounded-card p-2 transition-colors data-[setting-found]:bg-accent-soft";

/** The same for a fieldset's legend, which keeps its place in the fieldset (no negative margin). */
export const SETTING_LEGEND_CLASS = "rounded-control transition-colors data-[setting-found]:bg-accent-soft";

/**
 * The attributes of a row of the settings that the search can find. `key`, the i18n key of the
 * title, tells rows apart that show the same title in one section (the search's index names it).
 */
export function settingAnchor(title: string, key?: string): { "data-setting-title": string; "data-setting-key"?: string } {
  return key === undefined ? { "data-setting-title": title } : { "data-setting-title": title, "data-setting-key": key };
}

/**
 * A block of settings under a heading the search can find (its anchor): the controls in it that
 * have no title of their own (a row per account, a list of the user's signatures) are found
 * through the block's heading.
 */
export const SETTING_BLOCK = { "data-setting-block": "" } as const;
