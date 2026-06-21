import type { TriggerConfig } from "../types/api";

// The trigger types a service can map a model to, with operator-facing labels.
export const TRIGGER_TYPES: { value: string; label: string }[] = [
  { value: "issue", label: "Issue" },
  { value: "review_mr", label: "MR review" },
  { value: "fix_review", label: "Fix review" },
  { value: "mr_comment", label: "MR comment" },
  { value: "issue_comment", label: "Issue comment" },
];

// Build a full 5-entry gating map for the editor, overlaying any persisted rows
// onto the default (enabled, assignee, no label). The editor mutates this in
// place and submits all five entries, so every payload is a wholesale replace.
export function seedTriggers(
  saved: Record<string, TriggerConfig> = {},
): Record<string, TriggerConfig> {
  const out: Record<string, TriggerConfig> = {};
  for (const t of TRIGGER_TYPES) {
    const row = saved[t.value];
    out[t.value] = {
      enabled: row?.enabled ?? true,
      mode: row?.mode ?? "assignee",
      label: row?.label ?? "",
    };
  }
  return out;
}
