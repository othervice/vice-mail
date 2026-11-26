import { useInfiniteQuery } from "@tanstack/react-query";
import { Send } from "lucide-react";
import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { searchIpc, type Account, type ListCursor, type UnifiedView } from "../../services/ipc";
import type { ShortcutPlatform } from "../actions/shortcuts";
import { useMarkListSource } from "../comfort/listSources";
import { mixedMarksQueryKey, type MarkListKind } from "../comfort/queries";
import type { MessageListSource } from "../messages/listSource";
import { NO_SELECTION, type ListSelection } from "../messages/MessageList";
import { MESSAGE_PAGE_SIZE } from "../messages/queries";
import { buildRows } from "../messages/rows";
import { withThreadItems } from "../threads/queries";
import { decorateConversations, useThreadInfo } from "../threads/source";
import { useConversationsOn } from "../threads/preference";
import { markScopeOf } from "./scope";
import { MixedPanes } from "./UnifiedInbox";

/** Stage 23 (decision 0087): Sent of "All accounts" (or of a group), under `["unified", …]`. */
export const unifiedSentQueryKey = (view: UnifiedView, conversations: boolean) => ["unified", "sent", view, conversations ? "threads" : "messages"] as const;

/** What actions in Sent of "All accounts" reload. */
const UNIFIED_SENT_LIST_KEY = ["unified", "sent"] as const;

interface ScopedViewProps {
  /** The unified scope: every visible account, or a group's. */
  view: UnifiedView;
  accounts: readonly Account[];
  /** The view's name (the list's name for screen readers). */
  name: string;
  platform: ShortcutPlatform;
}

/**
 * Stage 23 (decision 0087): Later, Flagged and the other views of marked messages over the
 * accounts of "All accounts" (or of a group): one list mixed by account, each row with its
 * account's stripe and acting with its own account (as the unified inbox).
 */
export function ScopedMarkView({ kind, ...props }: ScopedViewProps & { kind: MarkListKind }) {
  const { t } = useTranslation();
  const [selection, setSelection] = useState<ListSelection>(NO_SELECTION);
  const source = useMarkListSource(kind, markScopeOf(props.view), [...mixedMarksQueryKey, props.view]);
  if (!source) return null;
  return (
    <MixedPanes
      accounts={props.accounts}
      source={source}
      listKey={mixedMarksQueryKey}
      name={props.name}
      platform={props.platform}
      conversations={false}
      selection={selection}
      onSelection={setSelection}
      errorContext={t("unified.viewFailed")}
      data-testid="all-accounts-view"
    />
  );
}

/**
 * Stage 23 (decision 0087): the Sent folders of the accounts of "All accounts" (or of a group),
 * newest first, as conversations when the conversation view is on (decision 0039).
 */
export function ScopedSentView(props: ScopedViewProps) {
  const conversations = useConversationsOn();
  // A new list when the conversation view changes: the selection belongs to the other rows.
  return <ScopedSentPanes key={conversations ? "threads" : "messages"} conversations={conversations} {...props} />;
}

function ScopedSentPanes({ conversations, ...props }: ScopedViewProps & { conversations: boolean }) {
  const { t } = useTranslation();
  const [selection, setSelection] = useState<ListSelection>(NO_SELECTION);
  const messages = useInfiniteQuery({
    queryKey: unifiedSentQueryKey(props.view, false),
    queryFn: ({ pageParam }) => searchIpc.listUnifiedSent(props.view, pageParam, MESSAGE_PAGE_SIZE),
    initialPageParam: null as ListCursor | null,
    getNextPageParam: (lastPage) => lastPage.nextCursor,
    enabled: !conversations,
  });
  const threads = useInfiniteQuery({
    queryKey: unifiedSentQueryKey(props.view, true),
    queryFn: ({ pageParam }) => searchIpc.listUnifiedSentThreads(props.view, pageParam, MESSAGE_PAGE_SIZE),
    initialPageParam: null as ListCursor | null,
    getNextPageParam: (lastPage) => lastPage.nextCursor,
    enabled: conversations,
    select: withThreadItems,
  });
  const info = useThreadInfo(useMemo(() => threads.data?.pages.flatMap((page) => page.threads), [threads.data]));
  const empty = { icon: Send, title: t("unified.sentEmpty.title"), body: t("unified.sentEmpty.body") };
  const source: MessageListSource = conversations
    ? { query: threads, buildRows, decorate: decorateConversations(info), conversations: true, empty }
    : { query: messages, buildRows, empty };
  return (
    <MixedPanes
      accounts={props.accounts}
      source={source}
      listKey={UNIFIED_SENT_LIST_KEY}
      name={props.name}
      platform={props.platform}
      conversations={conversations}
      showRecipients
      selection={selection}
      onSelection={setSelection}
      errorContext={t("unified.viewFailed")}
      data-testid="all-accounts-sent"
    />
  );
}
