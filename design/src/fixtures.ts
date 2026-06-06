// Mock data layer for the workbench. Stands in for the live Pinia stores / API
// so every view renders a realistic state without a backend. Views read these
// directly; in agent/frontend the same shapes come from the stores.
import type {
  AuthRequest,
  BranchEntry,
  GitServiceView,
  ProjectListItem,
  ProjectVar,
  StatsResponse,
  Task,
  TaskDetail,
} from "./types/api";

const iso = (s: string) => new Date(s).toISOString();

export const tasks: Task[] = [
  {
    id: "4f9c21a0",
    status: "running",
    trigger_type: "issue",
    trigger_data: { type: "issue", iid: 42, title: "Fix the post-login redirect", url: "https://github.com/acme/agent/issues/42" },
    project_path: "acme/agent",
    default_branch: "main",
    created_at: iso("2026-06-06T09:12:00"),
    started_at: iso("2026-06-06T09:12:04"),
    finished_at: null,
    provider: "github",
    branch: "42-fix-login-button",
    session_id: "sess_8a1f",
    pid: 48211,
  },
  {
    id: "a13b0833",
    status: "awaiting_auth",
    trigger_type: "review_mr",
    trigger_data: { type: "review_mr", iid: 17, title: "RS485 bridge", source_branch: "feat/rs485-bridge", target_branch: "main", url: "https://gitlab.com/acme/lighting/-/merge_requests/17" },
    project_path: "acme/lighting",
    default_branch: "main",
    created_at: iso("2026-06-06T08:40:00"),
    started_at: iso("2026-06-06T08:40:09"),
    finished_at: null,
    provider: "gitlab",
    branch: "feat/rs485-bridge",
    session_id: "sess_b22c",
    pid: 47788,
  },
  {
    id: "77de1042",
    status: "completed",
    trigger_type: "issue_comment",
    trigger_data: { type: "issue_comment", issue_iid: 8, comment: "please also add a test", url: "https://github.com/acme/cloud/issues/8" },
    project_path: "acme/cloud",
    default_branch: "main",
    created_at: iso("2026-06-06T07:55:00"),
    started_at: iso("2026-06-06T07:55:06"),
    finished_at: iso("2026-06-06T08:03:41"),
    provider: "github",
    branch: "fix-upload-race",
    session_id: "sess_19ab",
    pid: null,
  },
  {
    id: "2c5f9310",
    status: "failed",
    trigger_type: "issue",
    trigger_data: { type: "issue", iid: 8, title: "BLE timeout on reconnect", url: "https://gitlab.com/acme/manguard/-/issues/8" },
    project_path: "acme/manguard",
    default_branch: "main",
    created_at: iso("2026-06-05T18:20:00"),
    started_at: iso("2026-06-05T18:20:05"),
    finished_at: iso("2026-06-05T18:31:12"),
    provider: "gitlab",
    branch: "8-ble-timeout",
    session_id: "sess_77f0",
    pid: null,
  },
  {
    id: "0b91ccde",
    status: "pending",
    trigger_type: "issue",
    trigger_data: { type: "issue", iid: 51, title: "Dark mode for settings", url: "https://github.com/acme/agent/issues/51" },
    project_path: "acme/agent",
    default_branch: "main",
    created_at: iso("2026-06-06T09:30:00"),
    started_at: null,
    finished_at: null,
    provider: "github",
    branch: "51-dark-mode-settings",
    session_id: null,
    pid: null,
  },
];

export const streamSample = [
  JSON.stringify({ type: "system", subtype: "init", session_id: "sess_8a1f", cwd: "/work/acme/agent", tools: ["Read", "Edit", "Bash", "Grep"] }),
  JSON.stringify({ type: "assistant", message: { content: [{ type: "text", text: "Reading `src/auth/login.rs` to find where the post-login redirect is built." }] } }),
  JSON.stringify({ type: "assistant", message: { content: [{ type: "tool_use", name: "Edit", id: "tu_1", input: { file_path: "src/auth/login.rs", old_string: "redirect_to(\"/\")", new_string: "redirect_to(dest)" } }] } }),
  JSON.stringify({ type: "user", message: { content: [{ type: "tool_result", tool_use_id: "tu_1", content: "Applied edit: +2 -1" }] } }),
  JSON.stringify({ type: "assistant", message: { content: [{ type: "tool_use", name: "Bash", id: "tu_2", input: { command: "cargo check" } }] } }),
  JSON.stringify({ type: "user", message: { content: [{ type: "tool_result", tool_use_id: "tu_2", content: "Finished `dev` profile in 4.21s\n0 errors" }] } }),
  JSON.stringify({ type: "assistant", message: { content: [{ type: "text", text: "Fixed the redirect to honour `?next=` and verified the build. Ready to push." }] } }),
].join("\n");

export const diffSample = `diff --git a/src/auth/login.rs b/src/auth/login.rs
index 1a2b3c4..5d6e7f8 100644
--- a/src/auth/login.rs
+++ b/src/auth/login.rs
@@ -41,7 +41,9 @@ async fn login(
-    redirect_to("/")
+    let dest = next.unwrap_or("/dashboard");
+    redirect_to(dest)
     Ok(response)
Untracked files:
  tests/login_redirect.rs`;

export const taskDetail: TaskDetail = {
  ...tasks[0],
  work_dir: "/work/acme/agent/42-fix-login-button",
  live: true,
  result: {
    cost_usd: 0.0412,
    input_tokens: 184320,
    output_tokens: 37210,
    num_turns: 3,
    is_error: false,
    result_text: "Fixed the post-login redirect to honour the `next` query param and added a regression test. Build is green.",
  },
};

export const projects: ProjectListItem[] = [
  { id: "p1", provider: "github", project_slug: "agent", full_name: "acme/agent", remote_url: "git@github.com:acme/agent.git", default_branch: "main", allowed_operations: ["Bash(npm run *)", "Bash(cargo *)", "Read", "Edit"], notes: "", branch_count: 6 },
  { id: "p2", provider: "gitlab", project_slug: "lighting", full_name: "acme/lighting", remote_url: "git@gitlab.com:acme/lighting.git", default_branch: "main", allowed_operations: ["Read", "Edit"], notes: "", branch_count: 3 },
  { id: "p3", provider: "github", project_slug: "cloud", full_name: "acme/cloud", remote_url: "git@github.com:acme/cloud.git", default_branch: "main", allowed_operations: ["Bash(npm *)", "Read", "Edit"], notes: "", branch_count: 9 },
];

export const branchesByProject: Record<string, BranchEntry[]> = {
  p1: [
    { id: "b1", branch_name: "42-fix-login-button", status: "active", issue_iid: 42, pr_iid: null, last_used_at: iso("2026-06-06T09:12:00") },
    { id: "b2", branch_name: "51-dark-mode-settings", status: "idle", issue_iid: 51, pr_iid: null, last_used_at: iso("2026-06-06T09:30:00") },
    { id: "b3", branch_name: "fix-upload-race", status: "releasing", issue_iid: null, pr_iid: 23, last_used_at: iso("2026-06-05T20:00:00") },
  ],
  p2: [
    { id: "b4", branch_name: "feat/rs485-bridge", status: "active", issue_iid: null, pr_iid: 17, last_used_at: iso("2026-06-06T08:40:00") },
  ],
  p3: [],
};

export const variablesByProject: Record<string, ProjectVar[]> = {
  p1: [
    { key: "NODE_VERSION", value: "20", secret: false },
    { key: "DEPLOY_ENV", value: "staging", secret: false },
    { key: "GH_TOKEN", value: "ghp_xxxxxxxxxxxxxxxx", secret: true },
  ],
  p2: [
    { key: "TARGET", value: "armv7-unknown-linux-gnueabihf", secret: false },
    { key: "VAULT_TOKEN", value: "hvs.xxxxxxxxxxxx", secret: true },
  ],
  p3: [],
};

export const services: GitServiceView[] = [
  { id: "s1", kind: "github", slug: "github-main", display_name: "GitHub", base_url: "https://api.github.com", bot_username: "acme-agent", webhook_path: "/webhook/github/github-main" },
  { id: "s2", kind: "gitlab", slug: "gitlab-self", display_name: "GitLab (self-hosted)", base_url: "https://gitlab.acme.dev", bot_username: "agent-bot", webhook_path: "/webhook/gitlab/gitlab-self" },
];

export const authRequests: AuthRequest[] = [
  {
    id: "ar1",
    task_id: "a13b0833",
    requested_op: "git push -u origin HEAD",
    prompt_to_operator: "The agent wants to push the branch feat/rs485-bridge to origin. Approve?",
    status: "pending",
    operator_reply: null,
    created_at: iso("2026-06-06T08:52:00"),
    resolved_at: null,
  },
  {
    id: "ar2",
    task_id: "4f9c21a0",
    requested_op: "AskUserQuestion",
    prompt_to_operator: "Claude is asking the operator a question.",
    status: "pending",
    operator_reply: null,
    created_at: iso("2026-06-06T09:20:00"),
    resolved_at: null,
    metadata: {
      questions: [
        {
          question: "Redirect target after login?",
          header: "Redirect",
          options: [
            { label: "/dashboard", description: "Last visited dashboard (recommended)" },
            { label: "/", description: "Marketing home" },
          ],
        },
      ],
    },
  },
];

export const stats: StatsResponse = {
  from: iso("2026-05-07"),
  to: iso("2026-06-06"),
  group_by: "project",
  total_tasks: 128,
  total_secs: 218_400,
  rows: [
    { key: "p1", label: "acme/agent", task_count: 64, total_secs: 121_000 },
    { key: "p3", label: "acme/cloud", task_count: 38, total_secs: 61_200 },
    { key: "p2", label: "acme/lighting", task_count: 26, total_secs: 36_200 },
  ],
};
