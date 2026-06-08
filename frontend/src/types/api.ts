export type ProviderKind = "gitlab" | "github";
export type BranchStatus = "active" | "idle" | "releasing";
export type AuthStatus = "pending" | "approved" | "denied";

/** Persisted operator lifecycle of a task. */
export type TaskState = "pending" | "working_on" | "completed" | "failed";
/** Derived runtime disposition — the live hub overlaid on the durable column. */
export type AgentState = "cold" | "warm" | "pending" | "running" | "failed";

export interface Task {
  id: string;
  task_state: TaskState;
  agent_state: AgentState;
  trigger_type: string;
  trigger_data: unknown;
  project_path: string;
  git_url: string;
  default_branch: string;
  created_at: string;
  started_at: string | null;
  finished_at: string | null;
  provider: ProviderKind;
  branch: string | null;
  project_id: string | null;
  git_service_id: string | null;
  session_id: string | null;
  pid: number | null;
}

/** One frame on the task live stream (see src/ws/mod.rs `Envelope`). */
export type EnvelopeKind = "event" | "auth_request" | "status";

export interface StreamEnvelope {
  task_id: string;
  agent: string;
  /** Monotonic per-task sequence. `event` frames dedupe against the persisted
   *  /events history by this seq. */
  seq: number;
  kind: EnvelopeKind;
  payload: unknown;
}

/** One persisted live-stream frame from GET /api/tasks/{id}/events — the same
 *  shape as a `StreamEnvelope`, minus the task/agent routing fields. */
export interface PersistedEvent {
  seq: number;
  kind: EnvelopeKind;
  payload: unknown;
}

export interface TaskResult {
  id: string;
  task_id: string;
  cost_usd: number;
  input_tokens: number;
  output_tokens: number;
  num_turns: number;
  is_error: boolean;
  result_text: string;
  session_id: string;
}

export interface TaskDetail extends Task {
  result: TaskResult | null;
  work_dir: string | null;
}

export interface ProjectConfig {
  id: string;
  provider: ProviderKind;
  git_service_id: string | null;
  project_slug: string;
  full_name: string;
  remote_url: string;
  default_branch: string;
  my_username: string;
  allowed_operations: string[];
  /** Raw .env-style text injected as env vars when the agent starts. */
  env_file: string;
  notes: string;
  created_at: string;
  updated_at: string;
}

export interface GitServiceView {
  id: string;
  kind: ProviderKind;
  slug: string;
  display_name: string;
  base_url: string;
  bot_username: string;
  created_at: string;
  updated_at: string;
  webhook_path: string;
  autofire: boolean;
}

export interface NewGitService {
  kind: ProviderKind;
  slug: string;
  display_name: string;
  base_url: string;
  token: string;
  webhook_secret: string;
  bot_username: string;
  autofire: boolean;
}

export interface UpdateGitService {
  display_name?: string;
  base_url?: string;
  token?: string;
  webhook_secret?: string;
  bot_username?: string;
  autofire?: boolean;
}

export interface BranchEntry {
  id: string;
  project_id: string;
  branch_name: string;
  branch_slug: string;
  issue_iid: number | null;
  pr_iid: number | null;
  status: BranchStatus;
  checked_out_at: string;
  last_used_at: string;
}

export interface ProjectListItem extends ProjectConfig {
  branch_count: number;
}

export interface ProjectDetailResponse extends ProjectConfig {
  branches: BranchEntry[];
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

export type TriggerReason =
  | { type: "issue"; iid: number; title: string; description: string; url: string }
  | {
      type: "review_mr";
      iid: number;
      title: string;
      source_branch: string;
      target_branch: string;
      url: string;
    }
  | {
      type: "fix_review";
      iid: number;
      title: string;
      source_branch: string;
      url: string;
      review_body: string;
    }
  | {
      type: "mr_comment";
      mr_iid: number;
      comment: string;
      source_branch: string;
      url: string;
    }
  | { type: "issue_comment"; issue_iid: number; comment: string; url: string };

export type TriggerKind = TriggerReason["type"];

export interface NewTaskBody {
  project_id: string;
  trigger: TriggerReason;
}

/** Editable task fields. `branch`/`default_branch` are pending-only; `task_state`
 *  (the operator lifecycle) can be set on any task. Only provided keys change. */
export interface TaskEdits {
  branch?: string;
  default_branch?: string;
  task_state?: TaskState;
}

export type StatsGroupBy = "project" | "service" | "branch" | "trigger_type";

export interface StatsRow {
  key: string;
  label: string;
  task_count: number;
  total_secs: number;
}

export interface StatsResponse {
  from: string;
  to: string;
  group_by: StatsGroupBy;
  total_tasks: number;
  total_secs: number;
  rows: StatsRow[];
}

export interface StatsQuery {
  from?: string;
  to?: string;
  group_by?: StatsGroupBy;
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
