import { describe, it, expect, beforeEach, vi } from "vitest";
import { defineComponent, nextTick, ref } from "vue";
import { mount, flushPromises } from "@vue/test-utils";
import { setActivePinia, createPinia } from "pinia";
import type { AiModel, TaskDetail } from "../types/api";

// useTaskDetail's onMounted(setup) fans out to REST calls and uses the router;
// stub both so the composable mounts without a backend, then drive its computeds
// by setting store state directly.
const push = vi.fn();
vi.mock("vue-router", () => ({ useRouter: () => ({ push }) }));
vi.mock("../api/tasks", () => ({
  tasksApi: {
    get: vi.fn().mockResolvedValue(null),
    events: vi.fn().mockResolvedValue({ events: [] }),
    diff: vi.fn().mockResolvedValue({ diff: "" }),
  },
}));
vi.mock("../api/auth", () => ({ authApi: { list: vi.fn().mockResolvedValue([]) } }));
vi.mock("../api/models", () => ({ modelsApi: { list: vi.fn().mockResolvedValue([]) } }));
vi.mock("../api/queues", () => ({ queuesApi: { list: vi.fn().mockResolvedValue([]) } }));

import { useTaskDetail } from "./useTaskDetail";

function taskDetail(over: Partial<TaskDetail> = {}): TaskDetail {
  return {
    id: "t1",
    task_state: "pending",
    agent_state: "cold",
    trigger_type: "issue",
    trigger_data: null,
    project_path: "p",
    git_url: "https://x/p.git",
    default_branch: "master",
    created_at: "2026-01-01T00:00:00Z",
    started_at: null,
    finished_at: null,
    provider: "github",
    branch: null,
    project_id: null,
    service_id: null,
    session_id: null,
    pid: null,
    model_id: null,
    queue_id: null,
    priority: 0,
    result: null,
    work_dir: null,
    ...over,
  };
}

// Mount the composable inside a throwaway component so onMounted runs, then
// return its API for assertions.
async function mountDetail(id = "t1") {
  type Api = ReturnType<typeof useTaskDetail>;
  let api!: Api;
  const Comp = defineComponent({
    setup() {
      api = useTaskDetail(ref(id));
      return () => null;
    },
  });
  mount(Comp);
  await flushPromises();
  return api;
}

describe("useTaskDetail lifecycle computeds", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    push.mockClear();
  });

  it("isLive / isRunning track agent_state", async () => {
    const d = await mountDetail();
    d.store.detail = taskDetail({ agent_state: "running" });
    await nextTick();
    expect(d.isLive.value).toBe(true);
    expect(d.isRunning.value).toBe(true);

    d.store.detail = taskDetail({ agent_state: "warm" });
    await nextTick();
    expect(d.isLive.value).toBe(true);
    expect(d.isRunning.value).toBe(false);

    d.store.detail = taskDetail({ agent_state: "cold" });
    await nextTick();
    expect(d.isLive.value).toBe(false);
  });

  it("isPending tracks the operator task_state", async () => {
    const d = await mountDetail();
    d.store.detail = taskDetail({ task_state: "pending" });
    await nextTick();
    expect(d.isPending.value).toBe(true);

    d.store.detail = taskDetail({ task_state: "working_on" });
    await nextTick();
    expect(d.isPending.value).toBe(false);
  });

  it("canRetry only once the lifecycle is terminal", async () => {
    const d = await mountDetail();
    for (const [state, expected] of [
      ["pending", false],
      ["working_on", false],
      ["completed", true],
      ["failed", true],
    ] as const) {
      d.store.detail = taskDetail({ task_state: state });
      await nextTick();
      expect(d.canRetry.value).toBe(expected);
    }
  });

  it("canContinue needs a session id and a non-live agent", async () => {
    const d = await mountDetail();
    // Has a session but still running → cannot resume.
    d.store.detail = taskDetail({ session_id: "s1", agent_state: "running" });
    await nextTick();
    expect(d.canContinue.value).toBe(false);

    // Cold with a session → resumable.
    d.store.detail = taskDetail({ session_id: "s1", agent_state: "cold" });
    await nextTick();
    expect(d.canContinue.value).toBe(true);

    // No session → never resumable.
    d.store.detail = taskDetail({ session_id: null, agent_state: "failed" });
    await nextTick();
    expect(d.canContinue.value).toBe(false);
  });

  it("canChat when live or when a session exists to resume into", async () => {
    const d = await mountDetail();
    d.store.detail = taskDetail({ agent_state: "cold", session_id: null });
    await nextTick();
    expect(d.canChat.value).toBe(false);

    d.store.detail = taskDetail({ agent_state: "cold", session_id: "s1" });
    await nextTick();
    expect(d.canChat.value).toBe(true);

    d.store.detail = taskDetail({ agent_state: "running", session_id: null });
    await nextTick();
    expect(d.canChat.value).toBe(true);
    expect(d.canKill.value).toBe(true);
  });
});

describe("useTaskDetail derived labels", () => {
  beforeEach(() => setActivePinia(createPinia()));

  it("modelLabel resolves alias, raw id, then default", async () => {
    const d = await mountDetail();
    const model = { id: "m1", alias: "Opus", unbound: false } as AiModel;
    d.models.list = [model];

    d.store.detail = taskDetail({ model_id: "m1" });
    await nextTick();
    expect(d.modelLabel.value).toBe("Opus");

    d.store.detail = taskDetail({ model_id: "unknown" });
    await nextTick();
    expect(d.modelLabel.value).toBe("unknown");

    d.store.detail = taskDetail({ model_id: null });
    await nextTick();
    expect(d.modelLabel.value).toBe("default");
  });

  it("modelUnbound reflects the resolved model's unbound flag", async () => {
    const d = await mountDetail();
    d.models.list = [{ id: "m1", alias: "Danger", unbound: true } as AiModel];
    d.store.detail = taskDetail({ model_id: "m1" });
    await nextTick();
    expect(d.modelUnbound.value).toBe(true);
  });

  it("triggerHasTitle / triggerHasDescription read the trigger payload", async () => {
    const d = await mountDetail();
    d.store.detail = taskDetail({ trigger_data: { title: "Bug", description: "broken" } });
    await nextTick();
    expect(d.triggerHasTitle.value).toBe(true);
    expect(d.triggerHasDescription.value).toBe(true);

    d.store.detail = taskDetail({ trigger_data: { comment: "no title here" } });
    await nextTick();
    expect(d.triggerHasTitle.value).toBe(false);
  });
});

describe("useTaskDetail state transitions", () => {
  beforeEach(() => setActivePinia(createPinia()));

  it("a pushed status frame updates both axes of the loaded detail", async () => {
    const d = await mountDetail("t1");
    d.store.detail = taskDetail({ task_state: "pending", agent_state: "cold" });
    await nextTick();

    // Simulate a live status frame routed through the stream store.
    d.store.detail.task_state; // touch to ensure detail is set
    const { useStreamStore } = await import("../stores/stream");
    const stream = useStreamStore();
    stream.apply({
      task_id: "t1",
      agent: "claude",
      seq: 0,
      kind: "status",
      payload: { task_state: "working_on", agent_state: "running" },
    });
    await nextTick();

    expect(d.store.detail?.task_state).toBe("working_on");
    expect(d.store.detail?.agent_state).toBe("running");
  });

  it("startEdit seeds the edit form from the current detail", async () => {
    const d = await mountDetail();
    d.store.detail = taskDetail({
      branch: "feature/x",
      model_id: "m1",
      priority: 5,
      trigger_data: { title: "T", description: "D" },
    });
    await nextTick();

    d.startEdit();
    expect(d.editing.value).toBe(true);
    expect(d.editBranch.value).toBe("feature/x");
    expect(d.editTitle.value).toBe("T");
    expect(d.editDescription.value).toBe("D");
    expect(d.editModelId.value).toBe("m1");
    expect(d.editPriority.value).toBe(5);
  });
});
