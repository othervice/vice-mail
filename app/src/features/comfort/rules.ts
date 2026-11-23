import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  comfortIpc,
  type AccountId,
  type MailRule,
  type RuleAction,
  type RuleCondition,
  type RuleId,
  type RuleInput,
  type TextMatchOp,
} from "../../services/ipc";

/** The core's limits (contract of stage 9); checked here too, so the form can say what is wrong. */
export const MAX_RULES = 200;
export const MAX_RULE_PARTS = 20;
export const MAX_RULE_TEXT = 200;

export type ConditionField = RuleCondition["field"];
export type ActionType = RuleAction["type"];

export const CONDITION_FIELDS: readonly ConditionField[] = ["from", "to", "subject", "list", "account"];
export const ACTION_TYPES: readonly ActionType[] = ["move", "markRead", "setCategory", "pin", "notify"];

/** Which comparisons make sense per field: a domain only for addresses, a list by its id. */
export function opsFor(field: Exclude<ConditionField, "account">): readonly TextMatchOp[] {
  if (field === "from" || field === "to") return ["contains", "is", "domain"];
  return ["contains", "is"];
}

export function newCondition(field: ConditionField, accountId: AccountId | null): RuleCondition {
  if (field === "account") return { field, accountId: accountId ?? "" };
  return { field, op: "contains", value: "" };
}

export function newAction(type: ActionType, accountId: AccountId | null): RuleAction {
  switch (type) {
    case "move":
      return { type, accountId: accountId ?? "", path: "" };
    case "setCategory":
      return { type, category: "notification" };
    case "notify":
      return { type, enabled: false };
    default:
      return { type };
  }
}

export function emptyRule(): RuleInput {
  return { name: "", enabled: true, matchAll: true, conditions: [], actions: [], stop: false };
}

export function ruleInputOf(rule: MailRule): RuleInput {
  const { name, enabled, matchAll, conditions, actions, stop } = rule;
  return { name, enabled, matchAll, conditions, actions, stop };
}

export type RuleProblem =
  | "name"
  | "tooLong"
  | "noAction"
  | "tooManyParts"
  | "emptyValue"
  | "noAccount"
  | "noFolder";

/** What keeps the core from taking a rule, in the order the form shows it; null if nothing. */
export function ruleProblem(input: RuleInput): RuleProblem | null {
  if (!input.name.trim()) return "name";
  const texts = [
    input.name,
    ...input.conditions.flatMap((c) => (c.field === "account" ? [] : [c.value])),
    ...input.actions.flatMap((a) => (a.type === "move" ? [a.path] : [])),
  ];
  if (texts.some((text) => text.length > MAX_RULE_TEXT)) return "tooLong";
  if (input.actions.length === 0) return "noAction";
  if (input.conditions.length > MAX_RULE_PARTS || input.actions.length > MAX_RULE_PARTS) return "tooManyParts";
  if (input.conditions.some((c) => c.field !== "account" && !c.value.trim())) return "emptyValue";
  if (input.conditions.some((c) => c.field === "account" && !c.accountId)) return "noAccount";
  if (input.actions.some((a) => a.type === "move" && !a.accountId)) return "noAccount";
  if (input.actions.some((a) => a.type === "move" && !a.path)) return "noFolder";
  return null;
}

/** The rule ids with one rule moved up (-1) or down (+1); unchanged at either end. */
export function moved(ids: readonly RuleId[], index: number, direction: -1 | 1): RuleId[] {
  const target = index + direction;
  const next = [...ids];
  const a = next[index];
  const b = next[target];
  if (a === undefined || b === undefined) return next;
  next[index] = b;
  next[target] = a;
  return next;
}

export const rulesQueryKey = ["comfort", "rules"] as const;

export function useRules() {
  return useQuery({ queryKey: rulesQueryKey, queryFn: () => comfortIpc.listRules() });
}

/** Create (no id) or replace a rule; the list reloads afterwards. */
export function useSaveRule() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ id, input }: { id: RuleId | null; input: RuleInput }) =>
      id === null ? comfortIpc.createRule(input) : comfortIpc.updateRule(id, input),
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: rulesQueryKey });
    },
  });
}

export function useDeleteRule() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: RuleId) => comfortIpc.deleteRule(id),
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: rulesQueryKey });
    },
  });
}

export function useReorderRules() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (ids: RuleId[]) => comfortIpc.reorderRules(ids),
    onSuccess: (rules) => {
      queryClient.setQueryData(rulesQueryKey, rules);
    },
  });
}
