import { keepPreviousData, useInfiniteQuery, useQuery, type InfiniteData } from "@tanstack/react-query";
import {
  contactsIpc,
  MAX_SEARCH_PAGE,
  searchIpc,
  type ListCursor,
  type MessageListItem,
  type SearchPage,
  type SearchRequest,
  type ServerSearchResult,
} from "../../services/ipc";

/** Hits per call. The contract allows 1..`MAX_SEARCH_PAGE`. */
export const SEARCH_PAGE_SIZE = Math.min(100, MAX_SEARCH_PAGE);

/** A search without its paging fields. */
export type SearchInput = Omit<SearchRequest, "cursor" | "limit">;

export const searchQueryKey = (input: SearchInput) => ["search", input] as const;

/** A search page with the hits' rows, so the message list can show it like a folder page. */
export type SearchListPage = SearchPage & { items: MessageListItem[] };

function withItems(data: InfiniteData<SearchPage, ListCursor | null>): InfiniteData<SearchListPage, ListCursor | null> {
  return { ...data, pages: data.pages.map((page) => ({ ...page, items: page.hits.map((hit) => hit.message) })) };
}

/**
 * Local search, page by page (keyset cursor from the core). While the user types, the previous
 * results stay on screen until the new ones arrive. Null: no search.
 */
export function useSearchPages(input: SearchInput | null) {
  return useInfiniteQuery({
    queryKey: searchQueryKey(input ?? { query: "", scope: { kind: "all" }, typing: false }),
    queryFn: ({ pageParam }) => {
      if (!input) throw new Error("search without input");
      return searchIpc.search({ ...input, cursor: pageParam, limit: SEARCH_PAGE_SIZE });
    },
    initialPageParam: null as ListCursor | null,
    getNextPageParam: (lastPage) => lastPage.nextCursor,
    enabled: input !== null,
    placeholderData: keepPreviousData,
    select: withItems,
  });
}

/** Server results with the hits' rows, like a one-page list. */
export type ServerListPage = ServerSearchResult & { items: MessageListItem[] };

/**
 * "Search on the server" as a one-page list query, so the message list can show its hits. Only
 * runs once the user asked for it (`input` set); not reloaded by core events (it asks the server).
 */
export function useServerSearchPages(input: SearchInput | null) {
  return useInfiniteQuery({
    queryKey: ["serverSearch", input],
    queryFn: () => {
      if (!input) throw new Error("server search without input");
      return searchIpc.searchOnServer({ ...input, typing: false, cursor: null, limit: SEARCH_PAGE_SIZE });
    },
    initialPageParam: null as ListCursor | null,
    getNextPageParam: () => undefined,
    enabled: input !== null,
    // Every click asks the server again; its answer is not kept once the view is gone.
    gcTime: 0,
    select: (data): InfiniteData<ServerListPage, ListCursor | null> => ({
      ...data,
      pages: data.pages.map((page) => ({ ...page, items: page.hits.map((hit) => hit.message) })),
    }),
  });
}

/** Stage 22 (decision 0074): contacts shown on top while typing a search. */
export const CONTACT_SUGGESTIONS = 3;

/**
 * Up to [`CONTACT_SUGGESTIONS`] contacts whose name, address or domain starts with every word,
 * every kind (a shop's address finds its mail too). Under `contactDirectory`, so a change to
 * the contacts reloads them. Empty words: no request.
 */
export function useContactSuggestions(words: string) {
  return useQuery({
    queryKey: ["contactDirectory", "searchSuggestions", words] as const,
    queryFn: () => contactsIpc.listContacts(words, null, CONTACT_SUGGESTIONS, "all"),
    enabled: words !== "",
    placeholderData: keepPreviousData,
  });
}

