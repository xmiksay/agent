import { describe, it, expect } from "vitest";
import { ref } from "vue";
import {
  useClaudeStream,
  stringifyToolBody,
  toolInputSummary,
  clamp,
  bashCommand,
  extractTaskNotifications,
  questionList,
  type Block,
  type ToolUseBlock,
} from "./useClaudeStream";
import type { AuthRequest } from "../types/api";

// Build a stream-json stdout string from individual event objects.
const lines = (...events: unknown[]) => events.map((e) => JSON.stringify(e)).join("\n");

function authRequest(over: Partial<AuthRequest> = {}): AuthRequest {
  return {
    id: "auth-1",
    task_id: "t1",
    requested_op: "ls -la",
    prompt_to_operator: "Run ls -la?",
    status: "pending",
    operator_reply: null,
    created_at: "2026-01-01T00:00:00Z",
    resolved_at: null,
    ...over,
  };
}

// useClaudeStream returns blocks newest-first; oldest-first is easier to assert.
function blocksOldestFirst(text: string, pending: AuthRequest[] = []): Block[] {
  const { blocks } = useClaudeStream(ref(text), ref(pending));
  return [...blocks.value].reverse();
}

describe("parseLines (via useClaudeStream)", () => {
  it("parses an init event into an init block", () => {
    const text = lines({
      type: "system",
      subtype: "init",
      cwd: "/repo",
      session_id: "sess-1",
      tools: ["Bash", "Read", "Edit"],
    });
    const [block] = blocksOldestFirst(text);
    expect(block).toEqual({
      kind: "init",
      cwd: "/repo",
      sessionId: "sess-1",
      toolCount: 3,
    });
  });

  it("parses assistant text and tool_use content blocks", () => {
    const text = lines({
      type: "assistant",
      message: {
        content: [
          { type: "text", text: "Working on it" },
          { type: "tool_use", name: "Bash", input: { command: "ls" }, id: "tu-1" },
        ],
      },
    });
    const blocks = blocksOldestFirst(text);
    expect(blocks[0]).toEqual({ kind: "text", role: "assistant", body: "Working on it" });
    expect(blocks[1]).toMatchObject({
      kind: "tool_use",
      name: "Bash",
      id: "tu-1",
      awaitingApproval: null,
    });
  });

  it("parses a tool_result user event", () => {
    const text = lines({
      type: "user",
      message: {
        content: [{ type: "tool_result", tool_use_id: "tu-1", content: "done", is_error: false }],
      },
    });
    const [block] = blocksOldestFirst(text);
    expect(block).toEqual({ kind: "tool_result", id: "tu-1", body: "done", isError: false });
  });

  it("skips result, thinking_tokens, and task_notification events", () => {
    const text = lines(
      { type: "result", is_error: false, result: "all good" },
      { type: "thinking_tokens", count: 5 },
      { type: "system", subtype: "task_notification", status: "done", summary: "x" },
    );
    expect(blocksOldestFirst(text)).toEqual([]);
  });

  it("captures an error block, preferring message over error", () => {
    const text = lines({ type: "error", message: "boom", error: "ignored" });
    expect(blocksOldestFirst(text)[0]).toEqual({ kind: "error", message: "boom" });
  });

  it("parses a rate_limit_event into a rate_limit block", () => {
    const text = lines({
      type: "rate_limit_event",
      rate_limit_info: { status: "throttled", rateLimitType: "tokens", resetsAt: 123 },
    });
    expect(blocksOldestFirst(text)[0]).toMatchObject({
      kind: "rate_limit",
      status: "throttled",
      rateLimitType: "tokens",
      resetsAt: 123,
    });
  });

  it("records an unparseable line as an unknown block", () => {
    const text = "{not json";
    expect(blocksOldestFirst(text)[0]).toMatchObject({ kind: "unknown", summary: "(unparseable line)" });
  });

  it("ignores blank lines", () => {
    const text = "\n   \n";
    expect(blocksOldestFirst(text)).toEqual([]);
  });
});

describe("dedupeUserEchoes (via useClaudeStream)", () => {
  it("collapses consecutive identical user text echoes", () => {
    const userMsg = { type: "user", message: { content: [{ type: "text", text: "fix the bug" }] } };
    const blocks = blocksOldestFirst(lines(userMsg, userMsg));
    const userTexts = blocks.filter((b) => b.kind === "text");
    expect(userTexts).toHaveLength(1);
  });

  it("keeps a user echo that is interrupted by another block", () => {
    const userMsg = { type: "user", message: { content: [{ type: "text", text: "go" }] } };
    const assistant = { type: "assistant", message: { content: [{ type: "text", text: "ok" }] } };
    const blocks = blocksOldestFirst(lines(userMsg, assistant, userMsg));
    expect(blocks.filter((b) => b.kind === "text" && b.role === "user")).toHaveLength(2);
  });
});

describe("approval pairing (via useClaudeStream)", () => {
  const toolUse = (id: string) => ({
    type: "assistant",
    message: { content: [{ type: "tool_use", name: "Bash", input: { command: "ls" }, id }] },
  });
  const toolResult = (id: string) => ({
    type: "user",
    message: { content: [{ type: "tool_result", tool_use_id: id, content: "ok" }] },
  });

  it("attaches a pending approval to an unresolved tool_use", () => {
    const auth = authRequest({ id: "a1" });
    const blocks = blocksOldestFirst(lines(toolUse("tu-1")), [auth]);
    const tu = blocks.find((b): b is ToolUseBlock => b.kind === "tool_use")!;
    expect(tu.awaitingApproval).toEqual(auth);
  });

  it("does not attach an approval to an already-resolved tool_use", () => {
    const auth = authRequest({ id: "a1" });
    const blocks = blocksOldestFirst(lines(toolUse("tu-1"), toolResult("tu-1")), [auth]);
    const tu = blocks.find((b): b is ToolUseBlock => b.kind === "tool_use")!;
    expect(tu.awaitingApproval).toBeNull();
  });

  it("pairs approvals to tool_uses in creation order, oldest first", () => {
    const older = authRequest({ id: "old", created_at: "2026-01-01T00:00:00Z" });
    const newer = authRequest({ id: "new", created_at: "2026-01-01T00:05:00Z" });
    // Pass newest-first to prove the sort, not input order, drives pairing.
    const blocks = blocksOldestFirst(lines(toolUse("tu-1"), toolUse("tu-2")), [newer, older]);
    const tus = blocks.filter((b): b is ToolUseBlock => b.kind === "tool_use");
    expect(tus[0].awaitingApproval?.id).toBe("old");
    expect(tus[1].awaitingApproval?.id).toBe("new");
  });

  it("renders a leftover approval as a standalone pending row", () => {
    const auth = authRequest({ id: "a1", prompt_to_operator: "approve me" });
    const blocks = blocksOldestFirst("", [auth]);
    const tu = blocks.find((b): b is ToolUseBlock => b.kind === "tool_use")!;
    expect(tu).toMatchObject({ name: "pending approval", id: "a1", awaitingApproval: auth });
    expect(tu.input).toEqual({ prompt: "approve me" });
  });

  it("orders blocks newest-first", () => {
    const text = lines(
      { type: "assistant", message: { content: [{ type: "text", text: "first" }] } },
      { type: "assistant", message: { content: [{ type: "text", text: "second" }] } },
    );
    const { blocks } = useClaudeStream(ref(text), ref([]));
    expect(blocks.value.map((b) => (b.kind === "text" ? b.body : null))).toEqual(["second", "first"]);
  });
});

describe("stringifyToolBody", () => {
  it("returns a string as-is", () => {
    expect(stringifyToolBody("hello")).toBe("hello");
  });
  it("joins an array of text parts", () => {
    expect(stringifyToolBody([{ type: "text", text: "a" }, "b"])).toBe("a\nb");
  });
  it("returns empty string for null", () => {
    expect(stringifyToolBody(null)).toBe("");
  });
  it("pretty-prints an object", () => {
    expect(stringifyToolBody({ a: 1 })).toBe('{\n  "a": 1\n}');
  });
});

describe("toolInputSummary", () => {
  it("prefers command", () => {
    expect(toolInputSummary({ command: "ls -la" })).toBe("ls -la");
  });
  it("labels a file edit vs read/write", () => {
    expect(toolInputSummary({ file_path: "a.ts", old_string: "x" })).toBe("a.ts (edit)");
    expect(toolInputSummary({ file_path: "a.ts" })).toBe("a.ts (read/write)");
  });
  it("falls back to JSON for an unrecognised object", () => {
    expect(toolInputSummary({ foo: 1 })).toBe('{"foo":1}');
  });
  it("returns empty string for null", () => {
    expect(toolInputSummary(null)).toBe("");
  });
});

describe("clamp", () => {
  it("leaves short strings untouched", () => {
    expect(clamp("abc", 5)).toBe("abc");
  });
  it("truncates with an ellipsis", () => {
    expect(clamp("abcdef", 3)).toBe("abc…");
  });
});

describe("bashCommand", () => {
  const block = (over: Partial<ToolUseBlock>): ToolUseBlock => ({
    kind: "tool_use",
    name: "Bash",
    input: { command: "echo hi" },
    id: "x",
    awaitingApproval: null,
    ...over,
  });
  it("extracts the command from a Bash tool_use", () => {
    expect(bashCommand(block({}))).toBe("echo hi");
  });
  it("returns null for non-Bash tools", () => {
    expect(bashCommand(block({ name: "Read" }))).toBeNull();
  });
});

describe("extractTaskNotifications", () => {
  it("pulls task_notification system events with normalised fields", () => {
    const out = extractTaskNotifications([
      { type: "system", subtype: "task_notification", status: "ok", summary: "done", task_id: "bg-1", output_file: "/x" },
      { type: "assistant" },
      null,
    ]);
    expect(out).toEqual([
      { status: "ok", summary: "done", bgTaskId: "bg-1", toolUseId: undefined, outputFile: "/x" },
    ]);
  });
  it("omits an empty output_file", () => {
    const [n] = extractTaskNotifications([
      { type: "system", subtype: "task_notification", status: "ok", summary: "s", output_file: "" },
    ]);
    expect(n.outputFile).toBeUndefined();
  });
});

describe("questionList", () => {
  it("returns the questions array when present", () => {
    const qs = [{ question: "Pick one", options: [{ label: "a" }] }];
    expect(questionList({ questions: qs })).toEqual(qs);
  });
  it("returns null when there are no questions", () => {
    expect(questionList({ questions: [] })).toBeNull();
    expect(questionList(null)).toBeNull();
  });
});
