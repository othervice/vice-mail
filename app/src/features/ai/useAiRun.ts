import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import {
  aiIpc,
  toCoreError,
  type AiFunction,
  type AiProfileInfo,
  type AiSettingsInfo,
  type AiTransferInput,
  type CoreError,
} from "../../services/ipc";
import { useAiRequestStore } from "../../stores/aiRequests";
import { aiAvailabilityQueryKey, aiSettingsQueryKey } from "./queries";

/** The profile a function uses now, or null (off). */
export function profileFor(settings: AiSettingsInfo | undefined, aiFunction: AiFunction): AiProfileInfo | null {
  const id = settings?.functions.find((choice) => choice.function === aiFunction)?.profileId ?? null;
  return settings?.profiles.find((profile) => profile.id === id) ?? null;
}

export type AiRunState<T> =
  | { kind: "idle" }
  /**
   * A cloud profile without consent: nothing was sent; the explanation waits for the user.
   * `input` lets the step show the exact size (decision 0028: the core's dry run).
   */
  | { kind: "consent"; profile: AiProfileInfo | null; input: AiTransferInput | null }
  | { kind: "running"; requestId: string }
  | { kind: "done"; requestId: string; result: T }
  /** The user cancelled (the core answered `ai_cancelled`). */
  | { kind: "cancelled" }
  | { kind: "error"; error: CoreError };

export interface AiRun<T> {
  state: AiRunState<T>;
  /**
   * Starts the request, or first asks for consent when its profile needs it. `input` is what the
   * request gets, for the consent step's exact size.
   */
  run: (start: (requestId: string) => Promise<T>, input?: AiTransferInput) => void;
  /** After the user agreed in the consent step: records it and starts what waited. */
  agree: () => Promise<void>;
  /** Consent error (the agreement could not be saved). */
  agreeError: CoreError | null;
  cancel: () => void;
  reset: () => void;
}

/**
 * One AI request of a view: a fresh request id, the consent step before the first request to a
 * cloud provider (asked in advance from the settings and also when the core answers
 * `ai_consent_required`), cancelling, and cancelling on unmount, so no request runs that the
 * user cannot see. The progress (`aiRequest` events) is in `useAiRequestStore` under the id.
 */
export function useAiRun<T>(aiFunction: AiFunction): AiRun<T> {
  const queryClient = useQueryClient();
  const forget = useAiRequestStore((store) => store.forget);
  const [state, setState] = useState<AiRunState<T>>({ kind: "idle" });
  const [agreeError, setAgreeError] = useState<CoreError | null>(null);
  const pending = useRef<((requestId: string) => Promise<T>) | null>(null);
  /** What the request waiting for consent gets (for the dry run). */
  const pendingInput = useRef<AiTransferInput | null>(null);
  /** The request still running (cancelled on unmount). */
  const current = useRef<string | null>(null);
  /** The request whose progress is in the store (forgotten on unmount or the next run). */
  const shown = useRef<string | null>(null);
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      const id = current.current;
      if (id) void aiIpc.cancelRequest(id).catch(() => undefined);
      if (shown.current) forget(shown.current);
    };
  }, [forget]);

  const settingsNow = async (): Promise<AiSettingsInfo | undefined> => {
    try {
      return await queryClient.fetchQuery({ queryKey: aiSettingsQueryKey, queryFn: aiIpc.getSettings });
    } catch {
      return undefined;
    }
  };

  const start = async (begin: (requestId: string) => Promise<T>) => {
    if (shown.current) forget(shown.current);
    const requestId = crypto.randomUUID();
    current.current = requestId;
    shown.current = requestId;
    setState({ kind: "running", requestId });
    try {
      const result = await begin(requestId);
      if (!mounted.current || current.current !== requestId) return;
      current.current = null;
      setState({ kind: "done", requestId, result });
    } catch (e) {
      if (!mounted.current || current.current !== requestId) return;
      const error = toCoreError(e);
      current.current = null;
      forget(requestId);
      if (error.code === "ai_cancelled") {
        setState({ kind: "cancelled" });
      } else if (error.code === "ai_consent_required") {
        // The settings changed under us (another host): ask, with what the core knows now.
        await queryClient.invalidateQueries({ queryKey: aiSettingsQueryKey });
        pending.current = begin;
        setState({ kind: "consent", profile: profileFor(await settingsNow(), aiFunction), input: pendingInput.current });
      } else {
        if (error.code === "ai_off") void queryClient.invalidateQueries({ queryKey: aiAvailabilityQueryKey });
        setState({ kind: "error", error });
      }
    }
  };

  return {
    state,
    agreeError,
    run: (begin, input) => {
      setAgreeError(null);
      pendingInput.current = input ?? null;
      void (async () => {
        const profile = profileFor(await settingsNow(), aiFunction);
        if (!mounted.current) return;
        if (profile?.needsConsent) {
          pending.current = begin;
          setState({ kind: "consent", profile, input: input ?? null });
          return;
        }
        await start(begin);
      })();
    },
    agree: async () => {
      const begin = pending.current;
      const profile = state.kind === "consent" ? state.profile : null;
      if (!begin || !profile) return;
      try {
        const settings = await aiIpc.setConsent(profile.id, true);
        queryClient.setQueryData(aiSettingsQueryKey, settings);
      } catch (e) {
        setAgreeError(toCoreError(e));
        return;
      }
      pending.current = null;
      setAgreeError(null);
      await start(begin);
    },
    cancel: () => {
      if (state.kind === "consent") {
        pending.current = null;
        setState({ kind: "idle" });
        return;
      }
      const id = current.current;
      if (id) void aiIpc.cancelRequest(id).catch(() => undefined);
    },
    reset: () => {
      if (current.current) void aiIpc.cancelRequest(current.current).catch(() => undefined);
      if (shown.current) forget(shown.current);
      current.current = null;
      shown.current = null;
      pending.current = null;
      setAgreeError(null);
      setState({ kind: "idle" });
    },
  };
}
