import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { resolveLanguage } from "../../i18n";
import {
  popupIpc,
  toCoreError,
  type MessageId,
  type NewMailCard,
  type NewMailPopup,
  type PopupAppearance,
} from "../../services/ipc";
import { applyTheme, DARK_SCHEME_QUERY, resolveTheme } from "../preferences/theme";
import { cardTitle, NewMailPopupView, type CardState } from "./NewMailPopupView";

/** The popup hides itself after this long without the pointer on it (docs/DESIGN.md). */
export const HIDE_AFTER_MS = 10_000;
/** A deleted card offers "Undo" this long, then closes. */
export const DELETED_FOR_MS = 5_000;

/** The newer of two states of the popup (events and command answers can cross). */
export function newer(current: NewMailPopup | null, next: NewMailPopup): NewMailPopup {
  return current && current.revision > next.revision ? current : next;
}

/** Theme, language and accent of the app's windows, applied to this document. */
function useAppearance(appearance: PopupAppearance | undefined): void {
  const { i18n } = useTranslation();
  const theme = appearance?.theme;
  const language = appearance?.language;
  const accent = appearance?.accent;
  useEffect(() => {
    if (!theme) return undefined;
    const media = window.matchMedia(DARK_SCHEME_QUERY);
    const apply = () => {
      applyTheme(resolveTheme(theme, media.matches));
    };
    apply();
    if (theme !== "system") return undefined;
    media.addEventListener("change", apply);
    return () => {
      media.removeEventListener("change", apply);
    };
  }, [theme]);
  useEffect(() => {
    if (!language) return;
    const resolved = resolveLanguage(language, navigator.languages);
    document.documentElement.lang = resolved;
    if (i18n.language !== resolved) void i18n.changeLanguage(resolved);
  }, [language, i18n]);
  useEffect(() => {
    // tokens.css switches the accent family on this attribute (like `applyAccent`).
    if (accent) document.documentElement.dataset["accent"] = accent;
  }, [accent]);
}

/** What the live region says for cards that were not there before. */
function useAnnouncement(popup: NewMailPopup | null): string {
  const { t } = useTranslation();
  const seen = useRef<Set<MessageId>>(new Set());
  const [text, setText] = useState("");
  useEffect(() => {
    if (!popup) return;
    const fresh = popup.cards.filter((card) => !card.deleted && !seen.current.has(card.messageId));
    for (const card of popup.cards) seen.current.add(card.messageId);
    if (fresh.length === 0) return;
    const [first] = fresh;
    if (fresh.length > 1 || !first) setText(t("popup.announceMany", { count: fresh.length + popup.more }));
    else if (!first.content) setText(t("popup.announceHidden"));
    else
      setText(
        t("popup.announce", {
          sender: cardTitle(first, t),
          subject: first.content.subject || t("popup.noSubject"),
        }),
      );
  }, [popup, t]);
  return text;
}

/**
 * Stage 18 (decision 0051): the page of the new-mail popup's window. It asks the core for the
 * cards, follows `newMailPopupChanged`, forwards the buttons, hides itself after
 * [`HIDE_AFTER_MS`] (not while the pointer is on it), closes deleted cards after
 * [`DELETED_FOR_MS`], and tells the shell its height so the window fits the cards at the bottom
 * right of the screen. It calls only the popup's own commands (`popupIpc`).
 */
export function PopupApp() {
  const [popup, setPopup] = useState<NewMailPopup | null>(null);
  const [states, setStates] = useState<ReadonlyMap<MessageId, CardState>>(new Map());
  const [hovered, setHovered] = useState(false);
  const [now, setNow] = useState(() => new Date());
  const root = useRef<HTMLDivElement>(null);
  const take = useCallback((next: NewMailPopup) => {
    setPopup((current) => newer(current, next));
    setNow(new Date());
  }, []);

  // Listen first, then ask: a change in between arrives one way or the other.
  useEffect(() => {
    let stop: (() => void) | null = null;
    let gone = false;
    void popupIpc
      .onChanged(take)
      .then((unsubscribe) => {
        if (gone) unsubscribe();
        else stop = unsubscribe;
        return popupIpc.get();
      })
      .then(take)
      .catch(() => undefined);
    return () => {
      gone = true;
      stop?.();
    };
  }, [take]);

  useAppearance(popup?.appearance);
  const announcement = useAnnouncement(popup);

  // The window takes the height of the cards (the shell keeps it at the bottom right).
  useLayoutEffect(() => {
    const element = root.current;
    if (!element || !popup || popup.cards.length === 0) return undefined;
    let last = 0;
    const report = () => {
      const height = Math.ceil(element.getBoundingClientRect().height);
      if (height > 0 && height !== last) {
        last = height;
        void popupIpc.fit(height).catch(() => undefined);
      }
    };
    report();
    if (typeof ResizeObserver === "undefined") return undefined;
    const observer = new ResizeObserver(report);
    observer.observe(element);
    return () => {
      observer.disconnect();
    };
  }, [popup]);

  // Hides itself after a while, never while the pointer is on it. Every change starts over.
  const revision = popup?.revision;
  const shown = (popup?.cards.length ?? 0) > 0;
  useEffect(() => {
    if (!shown || hovered) return undefined;
    const timer = window.setTimeout(() => {
      void popupIpc.dismiss(null).then(take).catch(() => undefined);
    }, HIDE_AFTER_MS);
    return () => {
      window.clearTimeout(timer);
    };
  }, [shown, hovered, revision, take]);

  // A deleted card offers "Undo" for a few seconds, then goes.
  const deleted = popup?.cards.filter((card) => card.deleted).map((card) => card.messageId).join(",") ?? "";
  useEffect(() => {
    if (!deleted || hovered) return undefined;
    const ids = deleted.split(",").map(Number);
    const timer = window.setTimeout(() => {
      for (const id of ids) void popupIpc.dismiss(id).then(take).catch(() => undefined);
    }, DELETED_FOR_MS);
    return () => {
      window.clearTimeout(timer);
    };
  }, [deleted, hovered, take]);

  const run = (card: NewMailCard, action: (id: MessageId) => Promise<NewMailPopup>) => {
    const id = card.messageId;
    setStates((current) => new Map(current).set(id, { busy: true, error: null }));
    action(id)
      .then((next) => {
        take(next);
        setStates((current) => {
          const copy = new Map(current);
          copy.delete(id);
          return copy;
        });
      })
      .catch((error: unknown) => {
        setStates((current) => new Map(current).set(id, { busy: false, error: toCoreError(error) }));
      });
  };

  if (!popup || popup.cards.length === 0) return null;
  return (
    <div
      ref={root}
      data-testid="popup-root"
      onPointerEnter={() => {
        setHovered(true);
      }}
      onPointerLeave={() => {
        setHovered(false);
      }}
    >
      <NewMailPopupView
        popup={popup}
        states={states}
        announcement={announcement}
        now={now}
        onRead={(card) => {
          run(card, popupIpc.read);
        }}
        onDelete={(card) => {
          run(card, popupIpc.delete);
        }}
        onUndo={(card) => {
          run(card, popupIpc.undo);
        }}
        onClose={(card) => {
          run(card, popupIpc.dismiss);
        }}
        onOpenInbox={() => {
          void popupIpc.openInbox().then(take).catch(() => undefined);
        }}
      />
    </div>
  );
}
