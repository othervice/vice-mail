import { Check, ChevronDown } from "lucide-react";
import { useId, useRef, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { cx } from "../../components/cx";
import type { Account, AccountColor, UnifiedView } from "../../services/ipc";
import { useUiStore } from "../../stores/ui";
import { accountFill } from "../accounts/accountColor";
import { useAccountGroups } from "./queries";

/** One choice of the switcher: all accounts, a group, or one account. */
interface Choice {
  key: string;
  label: string;
  color: AccountColor | null;
  current: boolean;
  choose: () => void;
}

interface InboxScopeSwitcherProps {
  accounts: readonly Account[];
  /** The account the views show while no unified inbox is chosen. */
  account: Account | null;
}

function sameView(a: UnifiedView | null, b: UnifiedView): boolean {
  if (!a || a.kind !== b.kind) return false;
  return a.kind === "all" || (b.kind === "group" && a.groupId === b.groupId);
}

/**
 * Under the Inbox title (docs/DESIGN.md, "Kopf der Liste"): whose inbox the list shows. All
 * accounts, a group, or one account, each account with its colour next to its name. A button
 * that unfolds the choices right below it (no popover: the CSP rules out Radix Popover); arrow
 * keys move between them, Escape folds them away again.
 */
export function InboxScopeSwitcher({ accounts, account }: InboxScopeSwitcherProps) {
  const { t } = useTranslation();
  const listId = useId();
  const labelId = useId();
  const valueId = useId();
  const labelledBy = [labelId, valueId].join(" ");
  const [open, setOpen] = useState(false);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const listRef = useRef<HTMLUListElement>(null);
  const unified = useUiStore((state) => state.unifiedInbox);
  const selectAccount = useUiStore((state) => state.selectAccount);
  const showUnifiedInbox = useUiStore((state) => state.showUnifiedInbox);
  const groups = useAccountGroups().data ?? [];
  const accountLabel = (a: Account) => (a.displayName ? t("nav.account", { name: a.displayName, email: a.email }) : a.email);

  const choices: Choice[] = [
    {
      key: "all",
      label: t("unified.all"),
      color: null,
      current: sameView(unified, { kind: "all" }),
      choose: () => {
        showUnifiedInbox({ kind: "all" });
      },
    },
    ...groups.map((group) => ({
      key: `group-${group.id}`,
      label: group.name,
      color: null,
      current: sameView(unified, { kind: "group", groupId: group.id }),
      choose: () => {
        showUnifiedInbox({ kind: "group", groupId: group.id });
      },
    })),
    ...accounts.map((a) => ({
      key: `account-${a.id}`,
      label: accountLabel(a),
      color: a.color,
      current: unified === null && a.id === account?.id,
      choose: () => {
        selectAccount(a.id);
      },
    })),
  ];
  const current = choices.find((choice) => choice.current) ?? choices[0];

  const close = () => {
    setOpen(false);
    buttonRef.current?.focus();
  };
  const focusChoice = (index: number) => {
    const buttons = listRef.current?.querySelectorAll<HTMLButtonElement>("button");
    if (!buttons || buttons.length === 0) return;
    buttons[(index + buttons.length) % buttons.length]?.focus();
  };
  const onListKey = (event: KeyboardEvent<HTMLUListElement>) => {
    const buttons = [...(listRef.current?.querySelectorAll<HTMLButtonElement>("button") ?? [])];
    const at = buttons.findIndex((button) => button === document.activeElement);
    const moves: Record<string, number> = { ArrowDown: at + 1, ArrowUp: at - 1, Home: 0, End: buttons.length - 1 };
    if (event.key === "Escape") {
      event.preventDefault();
      close();
      return;
    }
    const target = moves[event.key];
    if (target === undefined) return;
    event.preventDefault();
    focusChoice(target);
  };

  return (
    <div className="relative flex min-w-0 items-center">
      <button
        ref={buttonRef}
        type="button"
        aria-expanded={open}
        aria-controls={open ? listId : undefined}
        aria-labelledby={labelledBy}
        onClick={() => {
          setOpen(!open);
          if (!open) window.requestAnimationFrame(() => focusChoice(Math.max(0, choices.findIndex((c) => c.current))));
        }}
        onKeyDown={(event) => {
          if (event.key === "Escape" && open) {
            event.preventDefault();
            close();
          }
        }}
        className="flex h-6 min-w-0 items-center gap-2 rounded-control pe-1 text-12 text-secondary hover:bg-hover hover:text-primary"
        data-testid="inbox-scope"
      >
        {current?.color !== null && current?.color !== undefined && (
          <span aria-hidden className={cx("size-(--ui-dot) shrink-0 rounded-full", accountFill(current.color))} />
        )}
        <span id={labelId} className="sr-only">
          {t("unified.switcher")}
        </span>
        <span id={valueId} className="truncate">
          {current?.label}
        </span>
        <ChevronDown aria-hidden className="size-3 shrink-0" strokeWidth={2} />
      </button>
      {open && (
        <ul
          ref={listRef}
          id={listId}
          aria-label={t("unified.switcher")}
          onKeyDown={onListKey}
          className="absolute top-full left-0 z-20 mt-1 flex min-w-(--ui-list-sender-width) flex-col rounded-control border border-border bg-surface-raised p-1 shadow-floating"
          data-testid="inbox-scope-choices"
        >
          {choices.map((choice) => (
            <li key={choice.key}>
              <button
                type="button"
                aria-current={choice.current ? "true" : undefined}
                onClick={() => {
                  choice.choose();
                  close();
                }}
                className="flex h-8 w-full min-w-0 items-center gap-2 rounded-control px-2 text-start text-13 text-primary hover:bg-hover"
                data-testid={`inbox-scope-${choice.key}`}
              >
                <span aria-hidden className="flex size-(--ui-icon-small) shrink-0 items-center justify-center">
                  {choice.current && <Check className="size-3" strokeWidth={2.5} />}
                </span>
                {choice.color !== null && (
                  <span aria-hidden className={cx("size-(--ui-dot) shrink-0 rounded-full", accountFill(choice.color))} />
                )}
                <span className="truncate">{choice.label}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
