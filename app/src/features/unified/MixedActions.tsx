import type { QueryKey } from "@tanstack/react-query";
import { useLayoutEffect, useMemo, useState, type ReactNode, type RefObject } from "react";
import type { Account, Mailbox, MessageId, MessageListItem } from "../../services/ipc";
import { QuickActions } from "../actions/ActionButtons";
import { FolderActionOverlays, FolderMissingPrompt, useFolderActions, type FolderActions } from "../actions/FolderActions";
import { tagsOf } from "../actions/keywords";
import { folderMenuScope, MessageMenu, type MenuScope } from "../actions/MessageMenu";
import { QUICK_ACTIONS, type ActionContext, type ActionHost } from "../actions/registry";
import type { ShortcutPlatform } from "../actions/shortcuts";
import type { MessageListHandle, RowMenuRequest } from "../messages/MessageList";
import { useOtherAccountFolders } from "../organize/transfer";

/** A folder of the mixed list: the account and folder its rows lie in. */
const scopeKey = (item: Pick<MessageListItem, "accountId" | "mailboxId">) => `${item.accountId}:${String(item.mailboxId)}`;

interface ScopeControllerProps {
  id: string;
  account: Account;
  mailbox: Mailbox;
  mailboxes: readonly Mailbox[];
  list: RefObject<MessageListHandle | null>;
  conversations: boolean;
  listKey: QueryKey;
  loaded: readonly MessageListItem[];
  register: (id: string, actions: FolderActions | null) => void;
  onHidden: (id: string, ids: ReadonlySet<MessageId>) => void;
}

/**
 * The actions of one folder of a mixed list: its action controller (as in the folder's own view),
 * with the question for a missing folder, the pickers and dialogs it opens. The list reaches it
 * through `register` when a row's quick action or the context menu runs.
 */
function ScopeController({ id, account, mailbox, mailboxes, list, conversations, listKey, loaded, register, onHidden }: ScopeControllerProps) {
  const actions = useFolderActions({ account, mailbox, list, conversations, listKey });
  const hiddenIds = actions.controller.hiddenIds;
  // The latest controller after every render: its hosts close over its current state.
  useLayoutEffect(() => {
    register(id, actions);
  });
  useLayoutEffect(
    () => () => {
      register(id, null);
    },
    [id, register],
  );
  useLayoutEffect(() => {
    onHidden(id, hiddenIds);
  }, [id, hiddenIds, onHidden]);
  const focusList = () => {
    list.current?.focus();
  };
  return (
    <>
      <FolderMissingPrompt actions={actions} mailboxes={mailboxes} focusList={focusList} />
      <FolderActionOverlays actions={actions} mailboxes={mailboxes} loaded={loaded} focusList={focusList} />
    </>
  );
}

interface MixedActionsOptions {
  accounts: readonly Account[];
  list: RefObject<MessageListHandle | null>;
  /** Decision 0039: the rows are conversations. */
  conversations: boolean;
  /** The messages the list has loaded. */
  items: readonly MessageListItem[];
  /** The mixed list's query, reloaded after actions that move rows out of it. */
  listKey: QueryKey;
  platform: ShortcutPlatform;
}

/**
 * Stage 21 (decision 0065): quick actions and the context menu in lists that mix folders and
 * accounts (the unified inbox, search results). Each folder among the rows gets the action
 * controller of its own view, so a row acts with its own account and folder: the same actions,
 * read-only reasons, undo and outbox states as in that folder. Folders stay once seen while the
 * list lives (their pickers and questions outlast their last row).
 */
export function useMixedActions({ accounts, list, conversations, items, listKey, platform }: MixedActionsOptions) {
  // Filled by the folders' controllers after they render; read when an action runs, never to
  // render (so a mutable map that stays the same object, not state).
  const [registry] = useState(() => new Map<string, FolderActions>());
  const [hiddenBy, setHiddenBy] = useState<ReadonlyMap<string, ReadonlySet<MessageId>>>(new Map());
  const [menu, setMenu] = useState<RowMenuRequest | null>(null);
  const [scopes, setScopes] = useState<readonly { id: string; accountId: string; mailboxId: number }[]>([]);
  // Every account's folders (no account has the empty id).
  const folders = useOtherAccountFolders("");
  const accountById = useMemo(() => new Map(accounts.map((account) => [account.id, account])), [accounts]);
  const mailboxesOf = (accountId: string) => folders.find((entry) => entry.account.id === accountId)?.mailboxes ?? [];
  const mailboxOf = (item: Pick<MessageListItem, "accountId" | "mailboxId">) => mailboxesOf(item.accountId).find((m) => m.id === item.mailboxId);

  const seen = new Set(scopes.map((scope) => scope.id));
  const added = new Map<string, { id: string; accountId: string; mailboxId: number }>();
  for (const item of items) {
    const id = scopeKey(item);
    if (!seen.has(id) && !added.has(id)) added.set(id, { id, accountId: item.accountId, mailboxId: item.mailboxId });
  }
  if (added.size > 0) setScopes([...scopes, ...added.values()]);

  const register = useMemo(
    () => (id: string, actions: FolderActions | null) => {
      if (actions) registry.set(id, actions);
      else registry.delete(id);
    },
    [registry],
  );
  const onHidden = useMemo(
    () => (id: string, ids: ReadonlySet<MessageId>) => {
      setHiddenBy((current) => {
        if (current.get(id) === ids || (ids.size === 0 && !current.get(id)?.size)) return current;
        const next = new Map(current);
        next.set(id, ids);
        return next;
      });
    },
    [],
  );
  const hiddenIds = useMemo(() => new Set([...hiddenBy.values()].flatMap((ids) => [...ids])), [hiddenBy]);

  /** Runs `act` with the folder's actions as they are now (at the click, not at the render). */
  const withActions = (id: string, act: (actions: FolderActions) => void) => {
    const actions = registry.get(id);
    if (actions) act(actions);
  };
  /** The host of a folder's controller for `childIds`, looked up when an action runs. */
  const hostOf = (id: string, childIds: ReadonlySet<MessageId>): ActionHost => {
    const host = (act: (host: ActionHost) => void) => {
      withActions(id, (actions) => {
        act(actions.controller.hostFor(childIds));
      });
    };
    return {
      apply: (action, rows) => {
        host((h) => {
          h.apply(action, rows);
        });
      },
      confirmDeletePermanently: (rows) => {
        host((h) => {
          h.confirmDeletePermanently(rows);
        });
      },
      openMove: (rows) => {
        host((h) => {
          h.openMove(rows);
        });
      },
      openTags: (rows) => {
        host((h) => {
          h.openTags(rows);
        });
      },
      undo: () => {
        host((h) => {
          h.undo();
        });
      },
      toggleSelect: () => {
        list.current?.toggleCursor();
      },
      selectAll: () => {
        list.current?.selectAll();
      },
      setPinned: (rows, pinned) => {
        host((h) => {
          h.setPinned(rows, pinned);
        });
      },
      openSnooze: (rows) => {
        host((h) => {
          h.openSnooze(rows);
        });
      },
      unsnooze: (rows) => {
        host((h) => {
          h.unsnooze(rows);
        });
      },
      openFollowUp: (rows) => {
        host((h) => {
          h.openFollowUp(rows);
        });
      },
      clearFollowUp: (rows) => {
        host((h) => {
          h.clearFollowUp(rows);
        });
      },
      setTask: (rows, mark) => {
        host((h) => {
          h.setTask(rows, mark);
        });
      },
      setReadingList: (rows, on) => {
        host((h) => {
          h.setReadingList(rows, on);
        });
      },
      blockSenders: (rows) => {
        host((h) => {
          h.blockSenders(rows);
        });
      },
    };
  };

  const ctxOf = (rows: readonly MessageListItem[]): ActionContext | null => {
    const first = rows[0];
    const account = first ? accountById.get(first.accountId) : undefined;
    const mailbox = first ? mailboxOf(first) : undefined;
    return account && mailbox ? { account, mailbox, items: rows, canUndo: false } : null;
  };

  /** Quick actions of a row, with its own account and folder (`child`: it acts alone). */
  const renderQuickActions = (item: MessageListItem, _selected: boolean, child: boolean): ReactNode => {
    const ctx = ctxOf([item]);
    if (!ctx) return null;
    return <QuickActions ids={QUICK_ACTIONS} ctx={ctx} host={hostOf(scopeKey(item), child ? new Set([item.id]) : new Set())} platform={platform} />;
  };

  /** The menu's scopes: the request's messages by account and folder. */
  const menuScopes = (request: RowMenuRequest): MenuScope[] => {
    const groups = new Map<string, MessageListItem[]>();
    for (const item of request.items) groups.set(scopeKey(item), [...(groups.get(scopeKey(item)) ?? []), item]);
    return [...groups.entries()].flatMap(([id, rows]) => {
      const ctx = ctxOf(rows);
      if (!ctx) return [];
      const childIds = new Set(rows.filter((row) => request.childIds.has(row.id)).map((row) => row.id));
      const scope = (act: (scope: MenuScope) => void) => {
        withActions(id, (actions) => {
          act(folderMenuScope(actions, ctx, childIds));
        });
      };
      return [
        {
          ctx,
          host: hostOf(id, childIds),
          moveTo: (target) => {
            scope((s) => {
              s.moveTo(target);
            });
          },
          copyTo: (target) => {
            scope((s) => {
              s.copyTo(target);
            });
          },
          setTag: (tag, on) => {
            scope((s) => {
              s.setTag(tag, on);
            });
          },
          snooze: (choice) => {
            scope((s) => {
              s.snooze(choice);
            });
          },
          followUp: (choice) => {
            scope((s) => {
              s.followUp(choice);
            });
          },
        },
      ];
    });
  };

  const element = (
    <>
      {scopes.map((scope) => {
        const account = accountById.get(scope.accountId);
        const mailbox = mailboxOf(scope);
        if (!account || !mailbox) return null;
        return (
          <ScopeController
            key={scope.id}
            id={scope.id}
            account={account}
            mailbox={mailbox}
            mailboxes={mailboxesOf(scope.accountId)}
            list={list}
            conversations={conversations}
            listKey={listKey}
            loaded={items}
            register={register}
            onHidden={onHidden}
          />
        );
      })}
      {menu && (
        <MessageMenu
          request={menu}
          scopes={menuScopes(menu)}
          platform={platform}
          knownTags={tagsOf([...items, ...menu.items])}
          onClose={() => {
            setMenu(null);
          }}
        />
      )}
    </>
  );

  return { hiddenIds, renderQuickActions, onRowMenu: setMenu, element };
}
