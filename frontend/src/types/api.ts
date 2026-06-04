export type ProviderKind = "gitlab" | "github";
export type BranchStatus = "active" | "idle" | "releasing";
export type AuthStatus = "pending" | "approved" | "denied";

export interface Task {
  id: string;
  status: string;
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

export interface TaskOutput {
  task_id: string;
  command: string;
  exit_code: number | null;
  stdout: string;
  stderr: string;
  captured_at: string;
  finished: boolean;
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
}

export interface NewGitService {
  kind: ProviderKind;
  slug: string;
  display_name: string;
  base_url: string;
  token: string;
  webhook_secret: string;
  bot_username: string;
}

export interface UpdateGitService {
  display_name?: string;
  base_url?: string;
  token?: string;
  webhook_secret?: string;
  bot_username?: string;
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
