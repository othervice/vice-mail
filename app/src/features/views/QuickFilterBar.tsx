import type { QueryKey } from "@tanstack/react-query";
import { Flag, ListFilter, Mail, Paperclip, Search, Tag, UserRound, X, type LucideIcon } from "lucide-react";
import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { ChoiceMenu, type ChoiceGroup } from "../../components/ChoiceMenu";
import { cx } from "../../components/cx";
import { useRovingFocus } from "../../components/useRovingFocus";
import type { InboxScope, QuickFilter } from "../../services/ipc";
import { isUserTag } from "../actions/keywords";
import { isFiltering, useQuickFilterStore, type QuickFilterState } from "./quickFilter";
import { useQuickFilterFocus } from "./focus";
import { useViewCount, useViewTags } from "./source";

type Switch = "unread" | "flagged" | "attachment" | "contacts";

/** The tag menu's place in the toolbar and its group. */
const TAG_KEY = "tag";
/** The menu's item for "no tag filter". */
const NO_TAG = "none";

const SWITCHES: readonly { key: Switch; icon: LucideIcon }[] = [
  { key: "unread", icon: Mail },
  { key: "flagged", icon: Flag },
  { key: "attachment", icon: Paperclip },
  { key: "contacts", icon: UserRound },
];

interface QuickFilterBarProps {
  viewKey: string;
  scope: InboxScope;
  /** The view's lists' key, so the count reloads with them. */
  keyBase: QueryKey;
  /** The filter as the list asks for it (text debounced). */
  query: QuickFilter;
  filter: QuickFilterState;
}

/**
 * Stage 19 (decision 0062): the quick filter above the list. Switches for unread, flagged, with
 * attachment and from contacts, a tag menu, and a text field that takes search language (sender,
 * subject and text; operators such as `from:` too). Everything combines; "Reset" clears it, and
 * while anything is set the bar says how many messages match. The switches and the tag menu are a
 * toolbar (one tab stop, ← and →); the text field and Reset are tab stops of their own. Escape in
 * the text field clears the text, a second Escape the whole filter.
 */
export function QuickFilterBar({ viewKey, scope, keyBase, query, filter }: QuickFilterBarProps) {
  const { t, i18n } = useTranslation();
  const set = useQuickFilterStore((state) => state.set);
  const reset = useQuickFilterStore((state) => state.reset);
  const filtering = isFiltering(filter);
  const count = useViewCount(scope, keyBase, query, filtering);
  const [tagsOpen, setTagsOpen] = useState(false);
  const tags = useViewTags(scope, keyBase, tagsOpen || filter.tag !== null);
  const field = useRef<HTMLInputElement>(null);
  const keys = [...SWITCHES.map((s) => s.key), TAG_KEY];
  const roving = useRovingFocus({ keys, activeKey: null, orientation: "horizontal" });
  const numbers = new Intl.NumberFormat(i18n.language);

  // Cmd/Ctrl+Shift+K (the command "Quick filter") puts the focus into the text field.
  const focusRequest = useQuickFilterFocus((state) => state.request);
  useEffect(() => {
    if (focusRequest > 0) field.current?.focus();
  }, [focusRequest]);

  const knownTags = (tags.data ?? []).filter(isUserTag);
  if (filter.tag !== null && !knownTags.some((tag) => tag.toLowerCase() === filter.tag?.toLowerCase())) knownTags.unshift(filter.tag);

  const tagProps = roving.itemProps(TAG_KEY);
  const tagGroup: ChoiceGroup = {
    id: TAG_KEY,
    label: tags.isPending ? t("views.filter.tagsLoading") : knownTags.length === 0 ? t("views.filter.noTags") : t("views.filter.tagsGroup"),
    kind: "radio",
    items: [
      {
        id: NO_TAG,
        label: t("views.filter.anyTag"),
        checked: filter.tag === null,
        onSelect: () => {
          set(viewKey, { tag: null });
        },
        "data-testid": "quick-filter-tag-none",
      },
      ...knownTags.map((tag) => ({
        id: `${TAG_KEY}-${tag}`,
        label: tag,
        checked: filter.tag?.toLowerCase() === tag.toLowerCase(),
        onSelect: () => {
          set(viewKey, { tag });
        },
        "data-testid": `quick-filter-tag-${tag}`,
      })),
    ],
  };

  const onFieldKey = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key !== "Escape") return;
    if (filter.text !== "") {
      event.preventDefault();
      event.stopPropagation();
      set(viewKey, { text: "" });
    } else if (filtering) {
      event.preventDefault();
      event.stopPropagation();
      reset(viewKey);
    }
  };

  return (
    <section
      aria-label={t("views.filter.label")}
      className="flex shrink-0 flex-wrap items-center gap-x-2 gap-y-1.5 border-b border-border bg-surface-list px-4 py-1.5"
      data-testid="quick-filter"
    >
      <div role="toolbar" aria-label={t("views.filter.switches")} className="flex flex-wrap items-center gap-1.5" {...roving.groupProps}>
        {SWITCHES.map(({ key, icon: Icon }) => (
          <button
            key={key}
            type="button"
            aria-pressed={filter[key]}
            onClick={() => {
              set(viewKey, { [key]: !filter[key] });
            }}
            {...roving.itemProps(key)}
            data-testid={`quick-filter-${key}`}
            className={cx(
              "flex h-7 items-center gap-1.5 rounded-full border px-2.5 text-13 transition-colors",
              filter[key] ? "border-accent bg-accent-soft font-medium text-accent" : "border-border text-secondary hover:bg-hover hover:text-primary",
            )}
          >
            <Icon aria-hidden className={cx("size-(--ui-icon-small) shrink-0", key === "flagged" && filter[key] && "fill-current")} strokeWidth={1.75} />
            <span>{t(`views.filter.${key}`)}</span>
          </button>
        ))}
        <ChoiceMenu
          label={t("views.filter.tag")}
          icon={Tag}
          text={filter.tag === null ? t("views.filter.tag") : t("views.filter.tagChosen", { tag: filter.tag })}
          active={filter.tag !== null}
          chip
          align="start"
          {...tagProps}
          onFocus={() => {
            tagProps.onFocus();
            setTagsOpen(true);
          }}
          groups={[tagGroup]}
          data-testid="quick-filter-tag"
        />
      </div>
      <label className="flex h-7 min-w-(--ui-list-sender-width) flex-1 items-center gap-1.5 rounded-control border border-border bg-surface px-2 focus-within:outline-2 focus-within:outline-focus-ring">
        <Search aria-hidden className="size-(--ui-icon-small) shrink-0 text-tertiary" strokeWidth={1.75} />
        <input
          ref={field}
          type="text"
          aria-label={t("views.filter.textLabel")}
          value={filter.text}
          placeholder={t("views.filter.textPlaceholder")}
          onChange={(event) => {
            set(viewKey, { text: event.target.value });
          }}
          onKeyDown={onFieldKey}
          autoComplete="off"
          autoCapitalize="none"
          autoCorrect="off"
          spellCheck={false}
          className="min-w-0 flex-1 bg-transparent text-13 text-primary outline-none placeholder:text-tertiary"
          data-testid="quick-filter-text"
        />
      </label>
      {filtering && (
        <>
          <p role="status" className="flex items-center gap-1 text-12 text-secondary tabular-nums" data-testid="quick-filter-count">
            <ListFilter aria-hidden className="size-(--ui-icon-small) shrink-0" strokeWidth={1.75} />
            {count.isError
              ? t("views.filter.countFailed")
              : count.data === undefined
                ? t("views.filter.counting")
                : t("views.filter.count", { count: count.data, formatted: numbers.format(count.data) })}
          </p>
          <button
            type="button"
            onClick={() => {
              reset(viewKey);
              field.current?.focus();
            }}
            className="flex h-7 items-center gap-1 rounded-control px-2 text-13 font-medium text-accent transition-colors hover:bg-hover"
            data-testid="quick-filter-reset"
          >
            <X aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
            {t("views.filter.reset")}
          </button>
        </>
      )}
    </section>
  );
}
