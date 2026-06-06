// Subset of the live app's API types, at the same import path (`../types/api`)
// so redesigned components/views drop back into agent/frontend unchanged.
export type ProviderKind = "gitlab" | "github";
export type AuthStatus = "pending" | "approved" | "denied";
export type BranchStatus = "active" | "idle" | "releasing";

export interface BranchEntry {
  id: string;
  branch_name: string;
  status: BranchStatus;
  issue_iid: number | null;
  pr_iid: number | null;
  last_used_at: string;
}

export interface Task {
  id: string;
  status: string;
  trigger_type: string;
  trigger_data: unknown;
  project_path: string;
  default_branch: string;
  created_at: string;
  started_at: string | null;
  finished_at: string | null;
  provider: ProviderKind;
  branch: string | null;
  session_id: string | null;
  pid: number | null;
}

export interface TaskResult {
  cost_usd: number;
  input_tokens: number;
  output_tokens: number;
  num_turns: number;
  is_error: boolean;
  result_text: string;
}

export interface TaskDetail extends Task {
  result: TaskResult | null;
  work_dir: string | null;
  live: boolean;
}

export interface ProjectVar {
  key: string;
  value: string;
  /** Masked in the UI; stored write-only on the server. */
  secret: boolean;
}

export interface ProjectConfig {
  id: string;
  provider: ProviderKind;
  project_slug: string;
  full_name: string;
  remote_url: string;
  default_branch: string;
  allowed_operations: string[];
  notes: string;
}
export interface ProjectListItem extends ProjectConfig {
  branch_count: number;
}

export interface GitServiceView {
  id: string;
  kind: ProviderKind;
  slug: string;
  display_name: string;
  base_url: string;
  bot_username: string;
  webhook_path: string;
}

export interface AuthQuestionOption {
  label: string;
  description?: string;
}
export interface AuthQuestion {
  question: string;
  header?: string;
  multiSelect?: boolean;
  options: AuthQuestionOption[];
}
export interface AuthRequestMetadata {
  questions?: AuthQuestion[];
}
export interface AuthRequest {
  id: string;
  task_id: string;
  requested_op: string;
  prompt_to_operator: string;
  status: AuthStatus;
  operator_reply: string | null;
  created_at: string;
  resolved_at: string | null;
  metadata?: AuthRequestMetadata | null;
}

export type TriggerKind =
  | "issue"
  | "review_mr"
  | "fix_review"
  | "mr_comment"
  | "issue_comment";

export type TriggerReason =
  | { type: "issue"; iid: number; title: string; description: string; url: string }
  | { type: "review_mr"; iid: number; title: string; source_branch: string; target_branch: string; url: string }
  | { type: "fix_review"; iid: number; title: string; source_branch: string; url: string; review_body: string }
  | { type: "mr_comment"; mr_iid: number; comment: string; source_branch: string; url: string }
  | { type: "issue_comment"; issue_iid: number; comment: string; url: string };

export interface StatsRow {
  key: string;
  label: string;
  task_count: number;
  total_secs: number;
}
export interface StatsResponse {
  from: string;
  to: string;
  group_by: string;
  total_tasks: number;
  total_secs: number;
  rows: StatsRow[];
}
