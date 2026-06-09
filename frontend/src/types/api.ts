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
  service_id: string | null;
  session_id: string | null;
  pid: number | null;
  // The chosen catalog model for this task; null = use the global default.
  model_id: string | null;
}

/** A lifecycle action applicable to many tasks at once. `run` confirms a
 *  pending task or resumes a paused one (chosen per task on the server). */
export type BulkAction = "run" | "pause" | "resume" | "delete";

/** Per-id outcome of a bulk action: a bad row reports here instead of failing
 *  the whole batch. */
export interface BulkActionResponse {
  succeeded: string[];
  failed: { id: string; error: string }[];
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
  service_id: string | null;
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

// 'pat' (GitHub/GitLab PATs and GitLab Group/Project Access Tokens); 'app' is
// groundwork for GitHub App (#9). GitLab has no 'app' flow.
export type AuthKind = "pat" | "app";

// Which issue signal triggers the agent: the bot being an assignee, a watched
// label, or either. Label mode is how a GitHub App identity (which can't be an
// assignee) gets triggered.
export type TriggerMode = "assignee" | "label" | "both";

export type GitLabTokenScope = "group" | "project";

// Non-secret metadata about a GitLab bot token minted via the provisioning
// flow. Present only on GitLab services whose token was provisioned (not pasted).
export interface GitLabTokenMeta {
  scope: GitLabTokenScope;
  namespace: string;
  token_id: number;
  expires_at: string | null;
}

export interface ServiceView {
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
  // The app_credentials bundle is write-only and never returned.
  auth_kind: AuthKind;
  // True once a GitHub App install has been recorded (installation_id present).
  app_installed: boolean;
  // Set once a GitLab bot token has been minted via the provisioning flow.
  gitlab_token: GitLabTokenMeta | null;
  trigger_mode: TriggerMode;
  // The label name watched when trigger_mode includes labels; "" otherwise.
  trigger_label: string;
  // Per-trigger-type model mapping: trigger_type -> model id (uuid). {} when
  // nothing mapped.
  models: Record<string, string>;
  // Present only on a create/update response when the webhook secret was just
  // auto-generated (left blank). Revealed once; never returned by list/get.
  generated_webhook_secret?: string | null;
}

export interface ProvisionGitLabToken {
  scope: GitLabTokenScope;
  namespace: string;
  name?: string;
  expires_at?: string;
}

export interface GitHubAppSyncResult {
  installation_id: string;
  webhook_registered: boolean;
  webhook_url: string | null;
  message: string;
}

// Provider-specific app secret bundle stored under `app_credentials` when
// auth_kind === "app". GitHub: { app_id, private_key, installation_id }.
// GitLab has no "app" flow — its bot identity is a Group/Project Access Token
// carried via auth_kind === "pat".
export type AppCredentials = Record<string, string>;

export interface NewService {
  kind: ProviderKind;
  slug: string;
  display_name: string;
  base_url: string;
  token: string;
  webhook_secret: string;
  bot_username: string;
  autofire: boolean;
  auth_kind?: AuthKind;
  app_credentials?: AppCredentials;
  trigger_mode?: TriggerMode;
  trigger_label?: string;
  // Per-trigger-type model mapping. Sending it replaces the whole mapping;
  // {} clears it.
  models?: Record<string, string>;
}

export interface UpdateService {
  display_name?: string;
  base_url?: string;
  token?: string;
  webhook_secret?: string;
  bot_username?: string;
  autofire?: boolean;
  auth_kind?: AuthKind;
  app_credentials?: AppCredentials;
  trigger_mode?: TriggerMode;
  trigger_label?: string;
  // Per-trigger-type model mapping. Omit to leave unchanged; sending it replaces
  // the whole mapping; {} clears it.
  models?: Record<string, string>;
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

export interface RegisterWebhookResponse {
  status: "registered" | "skipped";
  message: string;
  webhook_url: string | null;
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

/** Editable task fields. `branch`/`title`/`description` are the run inputs —
 *  pending-only; `task_state` (the operator lifecycle) can be set on any task.
 *  Only provided keys change. */
export interface TaskEdits {
  branch?: string;
  task_state?: TaskState;
  title?: string;
  description?: string;
  // null clears the per-task model override (back to the global default).
  model_id?: string | null;
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

// A model provider — a managed DB entity carrying an optional API key and a
// `kind` drawn from the system-defined backend keys.
export interface ModelProviderView {
  id: string;
  kind: string;
  name: string;
  // Indicates an API key is stored; the key itself is write-only.
  has_api_key: boolean;
  created_at: string;
  updated_at: string;
}

export interface NewModelProvider {
  kind: string;
  name: string;
  api_key?: string;
}

export interface UpdateModelProvider {
  kind?: string;
  name?: string;
  // Nullable patch: null clears the key, a string sets it, omit to leave it.
  api_key?: string | null;
}

export interface ModelProvidersListResponse {
  providers: ModelProviderView[];
  // System-defined backend keys a provider's `kind` may be.
  kinds: string[];
}

// Extended-thinking effort level; null disables it.
export type ModelEffort = "low" | "medium" | "high";

// A catalog entry for an AI model the agent can run.
export interface AiModel {
  id: string;
  provider_id: string;
  model_id: string;
  alias: string;
  input_price: number;
  output_price: number;
  cache_write_price: number;
  cache_read_price: number;
  thinking: boolean;
  effort: ModelEffort | null;
  is_default: boolean;
  // DANGEROUS: a task on this model runs every tool call — including arbitrary
  // shell — with no permission gating or operator approval
  // (--dangerously-skip-permissions). Only for fully trusted, sandboxed setups.
  unbound: boolean;
  created_at: string;
  updated_at: string;
}

export interface NewModel {
  provider_id: string;
  model_id: string;
  alias: string;
  input_price: number;
  output_price: number;
  cache_write_price: number;
  cache_read_price: number;
  thinking: boolean;
  effort?: ModelEffort | null;
  is_default: boolean;
  // See AiModel.unbound. Defaults false server-side.
  unbound?: boolean;
}

export interface UpdateModel {
  provider_id?: string;
  model_id?: string;
  alias?: string;
  input_price?: number;
  output_price?: number;
  cache_write_price?: number;
  cache_read_price?: number;
  thinking?: boolean;
  // null clears the effort; omit to leave unchanged.
  effort?: ModelEffort | null;
  is_default?: boolean;
  // See AiModel.unbound.
  unbound?: boolean;
}
