// The trigger types a service can map a model to, with operator-facing labels.
export const TRIGGER_TYPES: { value: string; label: string }[] = [
  { value: "issue", label: "Issue" },
  { value: "review_mr", label: "MR review" },
  { value: "fix_review", label: "Fix review" },
  { value: "mr_comment", label: "MR comment" },
  { value: "issue_comment", label: "Issue comment" },
];
