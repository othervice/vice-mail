import { QueryClient } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { ACTIONS, actionState, TOOLBAR_ACTIONS, type ActionContext, type ActionHost } from "../src/features/actions/registry";
import {
  addAddresses,
  formatSize,
  htmlToText,
  invalidAddressIn,
  looksLikeAddress,
  parseAddress,
  parseAddressList,
  splitAddressList,
  textToHtml,
} from "../src/features/composer/addresses";
import { updateOf } from "../src/features/composer/Composer";
import { draftsQueryKey, handleComposeEvent, outgoingQueryKey } from "../src/features/composer/queries";
import { normaliseLink } from "../src/features/composer/RichEditor";
import { de } from "../src/i18n/de";
import { en } from "../src/i18n/en";
import { useComposerStore } from "../src/stores/composer";
import { makeDraft, makeOutgoing } from "./support/compose";
import { makeAccount, makeMailbox, makeMessage } from "./support/core";

describe("addresses", () => {
  it("parses names, quoted names with commas, angle brackets and mailto", () => {
    expect(parseAddress("Grace Hopper <grace@example.org>")).toEqual({ name: "Grace Hopper", email: "grace@example.org" });
    expect(parseAddress('"Lovelace, Ada" <ada@example.org>')).toEqual({ name: "Lovelace, Ada", email: "ada@example.org" });
    expect(parseAddress("<lin@example.net>")).toEqual({ name: null, email: "lin@example.net" });
    expect(parseAddress("mailto:x@example.org")).toEqual({ name: null, email: "x@example.org" });
    expect(parseAddress("   ")).toBeNull();
  });

  it("splits lists at commas, semicolons and lines, not inside quotes or brackets", () => {
    expect(splitAddressList('"Lovelace, Ada" <ada@example.org>; grace@example.org,\nlin@example.net')).toEqual([
      '"Lovelace, Ada" <ada@example.org>',
      "grace@example.org",
      "lin@example.net",
    ]);
    expect(parseAddressList("a@example.org, , b@example.org")).toHaveLength(2);
  });

  it("keeps each mailbox once, whatever the case", () => {
    const list = addAddresses([{ name: null, email: "Grace@Example.org" }], [{ name: "Grace", email: "grace@example.org" }]);
    expect(list).toHaveLength(1);
  });

  it("catches what can never be an address, nothing more", () => {
    for (const ok of ["grace@example.org", "bob@localhost", "müller@bücher.de", "a+tag@example.org"]) expect(looksLikeAddress(ok)).toBe(true);
    for (const bad of ["grace", "grace@", "@example.org", "a b@example.org", "a@@example.org", "a@example..org", "a@example."])
      expect(looksLikeAddress(bad)).toBe(false);
  });

  it("finds the address the core named in invalid_address", () => {
    const to = [{ name: null, email: "ok@example.org" }, { name: null, email: "kaputt@x" }];
    expect(invalidAddressIn("not a valid address: kaputt@x", [to])).toBe("kaputt@x");
    expect(invalidAddressIn("no recipient", [to])).toBeNull();
    expect(invalidAddressIn(null, [to])).toBeNull();
  });

  it("converts between the editor's HTML and plain text", () => {
    expect(htmlToText("<p>Hallo <strong>Grace</strong></p><ul><li>eins</li><li>zwei</li></ul><blockquote><p>alt</p></blockquote>")).toBe(
      "Hallo Grace\n- eins\n- zwei\n> alt",
    );
    expect(htmlToText("<ol><li>a</li><li>b</li></ol>")).toBe("1. a\n2. b");
    expect(htmlToText("<p>a<br>b</p>")).toBe("a\nb");
    expect(textToHtml("Hallo <Grace> & co\nZeile 2\n\nAbsatz")).toBe("<p>Hallo &lt;Grace&gt; &amp; co<br>Zeile 2</p><p>Absatz</p>");
    expect(textToHtml("  ")).toBe("");
  });

  it("formats sizes for people", () => {
    expect(formatSize(16 * 1024 * 1024, "en")).toBe("16.8 MB");
    expect(formatSize(500, "en")).toBe("500 bytes");
  });
});

describe("links in the rich text editor", () => {
  it("accepts web and mail links only", () => {
    expect(normaliseLink("example.org/page")).toBe("https://example.org/page");
    expect(normaliseLink("https://example.org")).toBe("https://example.org");
    expect(normaliseLink("grace@example.org")).toBe("mailto:grace@example.org");
    expect(normaliseLink("javascript:alert(1)")).toBeNull();
    expect(normaliseLink("file:///etc/passwd")).toBeNull();
    expect(normaliseLink("  ")).toBeNull();
  });
});

describe("compose actions in the action directory", () => {
  const host = {} as ActionHost;
  const ctx = (overrides: Partial<ActionContext> = {}): ActionContext => ({
    account: makeAccount({ writesEnabled: false }),
    mailbox: makeMailbox({ id: 1, path: "INBOX", role: "inbox" }),
    items: [makeMessage({ id: 77 })],
    canUndo: false,
    ...overrides,
  });

  it("reply, reply all and forward act on one message, also on a read-only account", () => {
    for (const id of ["reply", "replyAll", "forward"] as const) {
      expect(actionState(ACTIONS[id], ctx()).kind).toBe("enabled");
      expect(actionState(ACTIONS[id], ctx({ items: [makeMessage(), makeMessage()] })).kind).toBe("unavailable");
      expect(actionState(ACTIONS[id], ctx({ items: [makeMessage({ draft: true })] })).kind).toBe("unavailable");
      expect(TOOLBAR_ACTIONS).toContain(id);
    }
    ACTIONS.replyAll.run(ctx(), host);
    expect(useComposerStore.getState().session?.source).toEqual({ kind: "reply_all", messageId: 77 });
    ACTIONS.forward.run(ctx(), host);
    expect(useComposerStore.getState().session?.source).toEqual({ kind: "forward", messageId: 77 });
  });

  it("a draft opens through the core, a new message needs no selection", () => {
    expect(actionState(ACTIONS.editDraft, ctx()).kind).toBe("unavailable");
    expect(actionState(ACTIONS.editDraft, ctx({ items: [makeMessage({ id: 5, draft: true })] })).kind).toBe("enabled");
    ACTIONS.editDraft.run(ctx({ items: [makeMessage({ id: 5, draft: true })] }), host);
    expect(useComposerStore.getState().session?.source).toEqual({ kind: "message", messageId: 5 });
    const account = makeAccount({ id: "acc-2" });
    expect(actionState(ACTIONS.compose, ctx({ account, items: [] })).kind).toBe("enabled");
    ACTIONS.compose.run(ctx({ account, items: [] }), host);
    expect(useComposerStore.getState().session?.source).toEqual({ kind: "new", accountId: "acc-2" });
  });

  it("has its labels in both languages", () => {
    for (const id of ["reply", "replyAll", "forward", "editDraft", "compose"] as const) {
      expect(en.actions.labels[id]).toBeTruthy();
      expect(de.actions.labels[id]).toBeTruthy();
    }
  });
});

describe("compose events", () => {
  it("draftsChanged closes a composer whose draft went elsewhere and refreshes the drafts", async () => {
    const queryClient = new QueryClient();
    queryClient.setQueryData(draftsQueryKey, []);
    useComposerStore.getState().open({ kind: "draft", draftId: "d1" });
    handleComposeEvent(queryClient, { type: "draftsChanged", accountId: "a", changed: ["d2"], removed: [] });
    expect(useComposerStore.getState().session).not.toBeNull();
    handleComposeEvent(queryClient, { type: "draftsChanged", accountId: "a", changed: [], removed: ["d1"] });
    expect(useComposerStore.getState().session).toBeNull();
    await vi.waitFor(() => {
      expect(queryClient.getQueryState(draftsQueryKey)?.isInvalidated).toBe(true);
    });
  });

  it("outgoingChanged updates the outbox list and only the sends this window shows", () => {
    const queryClient = new QueryClient();
    const mine = makeOutgoing({ operationId: 1, accountId: "a" });
    queryClient.setQueryData(outgoingQueryKey("a"), [mine]);
    useComposerStore.getState().track(mine);
    const other = makeOutgoing({ operationId: 2, accountId: "a", state: "sending" });
    handleComposeEvent(queryClient, { type: "outgoingChanged", accountId: "a", mails: [{ ...mine, state: "sent" }, other] });
    expect(queryClient.getQueryData<{ operationId: number; state: string }[]>(outgoingQueryKey("a"))?.map((m) => [m.operationId, m.state])).toEqual([
      [2, "sending"],
      [1, "sent"],
    ]);
    expect(useComposerStore.getState().sends.map((s) => [s.mail.operationId, s.mail.state])).toEqual([[1, "sent"]]);
    // A failure of a send started elsewhere (another window, before a restart) is shown too.
    handleComposeEvent(queryClient, { type: "outgoingChanged", accountId: "a", mails: [{ ...other, state: "failed" }] });
    expect(useComposerStore.getState().sends.map((s) => s.mail.operationId)).toEqual([1, 2]);
  });

  it("sendNeedsDecision always asks", () => {
    const queryClient = new QueryClient();
    const mail = makeOutgoing({ operationId: 9, state: "unclear" });
    handleComposeEvent(queryClient, { type: "sendNeedsDecision", accountId: mail.accountId, mail });
    expect(useComposerStore.getState().sends.map((s) => s.mail.state)).toEqual(["unclear"]);
  });
});

describe("draft fields", () => {
  it("the update carries every editable field and nothing of the core's own", () => {
    const update = updateOf(makeDraft({ subject: "S", includeQuote: false, quotePosition: "above" }));
    expect(Object.keys(update).sort()).toEqual(
      ["bcc", "bodyHtml", "bodyText", "cc", "format", "identityId", "includeQuote", "includeSignature", "quotePosition", "subject", "to"].sort(),
    );
    expect(update).toMatchObject({ subject: "S", includeQuote: false, quotePosition: "above" });
  });
});
