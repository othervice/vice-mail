import { createContext, use } from "react";

/**
 * Decision 0031: whether the list lays its rows out narrow (more lines per row), for rows the list
 * only draws (bundles), which get their content from a source and not from the list.
 */
export const ListLayoutContext = createContext(false);

export function useNarrowList(): boolean {
  return use(ListLayoutContext);
}
