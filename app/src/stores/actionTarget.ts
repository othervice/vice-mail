import { useEffect, useId } from "react";
import { create } from "zustand";
import type { ActionContext, ActionHost, ActionId } from "../features/actions/registry";

/**
 * What the mail actions of the command palette act on (stage 8): the view that has the list
 * registers the same context and host its shortcuts use. The most recently mounted view wins
 * (the search results lie over the folder view while they are open).
 */
export interface ActionTarget {
  /** Null: the view has nothing to act on yet (still loading). */
  ctx: ActionContext | null;
  host: ActionHost;
  /** Only these actions (a view without the pickers of the folder view offers fewer); all if absent. */
  allowed?: ReadonlySet<ActionId>;
}

interface ActionTargetState {
  targets: readonly { key: string; target: ActionTarget }[];
  register: (key: string, target: ActionTarget) => void;
  unregister: (key: string) => void;
}

export const useActionTargetStore = create<ActionTargetState>()((set) => ({
  targets: [],
  register: (key, target) => {
    set((state) => {
      const index = state.targets.findIndex((entry) => entry.key === key);
      if (index < 0) return { targets: [...state.targets, { key, target }] };
      const targets = [...state.targets];
      targets[index] = { key, target };
      return { targets };
    });
  },
  unregister: (key) => {
    set((state) => ({ targets: state.targets.filter((entry) => entry.key !== key) }));
  },
}));

export function currentActionTarget(targets: ActionTargetState["targets"]): ActionTarget | null {
  return targets[targets.length - 1]?.target ?? null;
}

/** Offers the view's actions to the command palette while the view is mounted. */
export function useActionTarget(target: ActionTarget): void {
  const key = useId();
  const register = useActionTargetStore((state) => state.register);
  const unregister = useActionTargetStore((state) => state.unregister);
  const { ctx, host, allowed } = target;
  useEffect(() => {
    register(key, { ctx, host, ...(allowed && { allowed }) });
  });
  useEffect(
    () => () => {
      unregister(key);
    },
    [key, unregister],
  );
}
