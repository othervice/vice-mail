import { beforeEach, describe, expect, it } from "vitest";
import { cloudBlocked, numberSources, panelAvailable } from "../src/features/ai/AiPanel";
import { profileFor } from "../src/features/ai/useAiRun";
import type { AiSettingsInfo } from "../src/services/ipc";
import { useAiRequestStore } from "../src/stores/aiRequests";
import { AI_PRESETS, CLOUD_PROFILE_ID, makeAiProfile, makeCloudProfile, makeTransfer } from "./support/ai";

const ACCOUNT = "11111111-1111-4111-8111-111111111111";

function settings(overrides: Partial<AiSettingsInfo> = {}): AiSettingsInfo {
  return {
    enabled: true,
    profiles: [makeAiProfile(), makeCloudProfile()],
    functions: [
      { function: "summary", profileId: CLOUD_PROFILE_ID },
      { function: "reply_suggestion", profileId: makeAiProfile().id },
      { function: "writing_help", profileId: null },
      { function: "search", profileId: CLOUD_PROFILE_ID },
    ],
    accounts: [{ accountId: ACCOUNT, cloudAllowed: false }],
    presets: AI_PRESETS,
    openReplies: { enabled: true, days: 3 },
    ...overrides,
  };
}

describe("AI rules in the UI", () => {
  it("finds the profile of a function, or none when it is off", () => {
    expect(profileFor(settings(), "summary")?.id).toBe(CLOUD_PROFILE_ID);
    expect(profileFor(settings(), "writing_help")).toBeNull();
    expect(profileFor(undefined, "summary")).toBeNull();
  });

  it("blocks only cloud profiles for accounts without permission", () => {
    expect(cloudBlocked(settings(), "summary", ACCOUNT)).toBe(true);
    expect(cloudBlocked(settings(), "reply_suggestion", ACCOUNT)).toBe(false);
    expect(cloudBlocked(settings({ accounts: [{ accountId: ACCOUNT, cloudAllowed: true }] }), "summary", ACCOUNT)).toBe(false);
    // An account the settings do not know has no permission.
    expect(cloudBlocked(settings(), "summary", "22222222-2222-4222-8222-222222222222")).toBe(true);
  });

  it("shows the panel only while AI is on and a panel function is usable", () => {
    expect(panelAvailable(undefined)).toBe(false);
    expect(panelAvailable({ enabled: false, summary: true, replySuggestion: true, writingHelp: true, search: true })).toBe(false);
    expect(panelAvailable({ enabled: true, summary: false, replySuggestion: false, writingHelp: true, search: false })).toBe(false);
    expect(panelAvailable({ enabled: true, summary: false, replySuggestion: false, writingHelp: false, search: true })).toBe(true);
  });

  it("numbers sources in the order they first appear", () => {
    const numbers = numberSources([
      { text: "a", segments: [], sources: [30, 10] },
      { text: "b", segments: [], sources: [10, 20] },
    ]);
    expect([...numbers.entries()]).toEqual([
      [30, 1],
      [10, 2],
      [20, 3],
    ]);
  });
});

describe("AI request progress", () => {
  beforeEach(() => {
    useAiRequestStore.setState(useAiRequestStore.getInitialState(), true);
  });

  it("follows the phases of a request: each sending adds a transfer", () => {
    const store = useAiRequestStore.getState();
    store.apply("r1", { phase: "sending", transfer: makeTransfer({ mails: 0 }) });
    store.apply("r1", { phase: "receiving", chars: 256 });
    store.apply("r1", { phase: "sending", transfer: makeTransfer({ mails: 4 }) });
    let progress = useAiRequestStore.getState().requests["r1"];
    expect(progress?.transfers.map((t) => t.mails)).toEqual([0, 4]);
    expect(progress?.chars).toBe(0);
    store.apply("r1", { phase: "finished", outcome: "ok" });
    progress = useAiRequestStore.getState().requests["r1"];
    expect(progress?.outcome).toBe("ok");
    store.forget("r1");
    expect(useAiRequestStore.getState().requests["r1"]).toBeUndefined();
  });

  it("drops finished requests of other windows first once many are tracked", () => {
    const store = useAiRequestStore.getState();
    for (let i = 0; i < 60; i += 1) {
      store.apply(`r${String(i)}`, { phase: "sending", transfer: makeTransfer() });
      if (i < 30) store.apply(`r${String(i)}`, { phase: "finished", outcome: "ok" });
    }
    const ids = Object.keys(useAiRequestStore.getState().requests);
    expect(ids).toHaveLength(50);
    // Every running one is still there.
    for (let i = 30; i < 60; i += 1) expect(ids).toContain(`r${String(i)}`);
  });
});
