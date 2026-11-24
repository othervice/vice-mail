import { createContext, use } from "react";

/**
 * Decision 0031: whether the list lays its rows out narrow (more lines per row), for rows the list
 * only draws (bundles), which get their content from a source and not from the list.
 */
export const ListLayoutContext = createContext(false);

export function useNarrowList(): boolean {
  return use(ListLayoutContext);
}

/**
 * Stage 17 (decision 0049): whether the list is a tree of conversations (`role="tree"`). Drawn rows
 * leave out in words what the tree item says itself (a bundle's "collapsed" is `aria-expanded`).
 */
export const ListTreeContext = createContext(false);

export function useTreeList(): boolean {
  return use(ListTreeContext);
}
