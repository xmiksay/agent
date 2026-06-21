import { computed, onMounted, ref, watch, type Ref } from "vue";
import { useRouter } from "vue-router";
import { useTasksStore } from "../stores/tasks";
import { useStreamStore } from "../stores/stream";
import { useModelsStore } from "../stores/models";
import { authApi } from "../api/auth";
import { tasksApi } from "../api/tasks";
import { extractTaskNotifications } from "./useClaudeStream";
import type { AuthRequest, TaskEdits, TaskState } from "../types/api";

/**
 * Everything behind the task detail view: REST detail/result, the live event
 * stream (history seed + WebSocket), operator chat/stop/redefine, the lifecycle
 * actions, and the lazy branch diff. Kept out of the SFC so the view stays
 * presentational and under the file-size cap.
 */
export function useTaskDetail(idRef: Ref<string>) {
  const store = useTasksStore();
  const stream = useStreamStore();
  const models = useModelsStore();
  const router = useRouter();
  const busy = ref<string | null>(null);

  // --- Live event stream (via the single app-wide socket) --------------------
  // The global stream store holds every task's events keyed by task_id; we read
  // this task's slice. History from /events seeds seq 0..N-1; live frames
  // continue from N (dedupe is automatic since seq is the map key). ClaudeStream
  // wants a newline-delimited JSON string (it reverses to newest-first).
  const eventText = computed(() => {
    const m = stream.eventsFor(idRef.value);
    if (!m) return "";
    return [...m.keys()]
      .sort((a, b) => a - b)
      .map((k) => JSON.stringify(m.get(k)))
      .join("\n");
  });
  const eventCount = computed(() => stream.eventsFor(idRef.value)?.size ?? 0);
  const hasEvents = computed(() => eventCount.value > 0);

  // Background-task completions, lifted out of the timeline into the Outline.
  const taskNotifications = computed(() => {
    const m = stream.eventsFor(idRef.value);
    if (!m) return [];
    const ordered = [...m.keys()].sort((a, b) => a - b).map((k) => m.get(k));
    return extractTaskNotifications(ordered);
  });

  // Cumulative output tokens spent this run, summed from each event's usage —
  // mirrors the backend's per-chunk accounting (src/jobs/stream.rs). Thinking
  // tokens are hidden from the timeline but still counted here, so the operator
  // sees the real spend behind the chat box.
  const tokensSpent = computed(() => {
    const m = stream.eventsFor(idRef.value);
    if (!m) return 0;
    let total = 0;
    for (const ev of m.values()) {
      const e = ev as Record<string, any>;
      const out =
        e?.usage?.output_tokens ?? e?.message?.usage?.output_tokens ?? null;
      if (typeof out === "number") total += out;
    }
    return total;
  });

  // Pending approvals for this task, sliced from the shared live set.
  const pendingApprovals = computed(() =>
    [...stream.approvals.values()].filter((a) => a.task_id === idRef.value),
  );

  // A task is "live" while its agent is attached — running, or warm (idle between
  // turns). `wsConnected` = socket up AND this task live, so chat goes straight
  // to the agent rather than being queued.
  const isLive = computed(() =>
    ["running", "warm"].includes(store.detail?.agent_state ?? ""),
  );
  const isRunning = computed(() => store.detail?.agent_state === "running");
  const isPending = computed(() => store.detail?.task_state === "pending");
  // Retry is offered once the operator lifecycle is terminal.
  const canRetry = computed(() =>
    ["completed", "failed"].includes(store.detail?.task_state ?? ""),
  );
  // Resume reattaches a prior claude session — only when no agent is live.
  const canContinue = computed(
    () =>
      !!store.detail?.session_id &&
      ["cold", "failed"].includes(store.detail?.agent_state ?? ""),
  );
  const canKill = computed(() => isLive.value);
  const canChat = computed(() => isLive.value || !!store.detail?.session_id);
  const wsConnected = computed(() => stream.connected && isLive.value);

  async function reloadPending() {
    try {
      stream.seedApprovals(await authApi.list({ task_id: idRef.value, status: "pending" }));
    } catch {
      /* ignore — section will just be empty */
    }
  }

  async function loadHistory() {
    try {
      const { events: hist } = await tasksApi.events(idRef.value);
      stream.seedEvents(idRef.value, hist);
    } catch {
      /* history endpoint may 404 for a brand-new task */
    }
  }

  async function setup() {
    stream.start();
    await store.load(idRef.value);
    await Promise.all([loadHistory(), reloadPending(), models.refresh()]);
  }

  // Resolve the task's model to a display label: its alias, else the raw id,
  // else "default" when no per-task override is set.
  const modelLabel = computed(() => {
    const id = store.detail?.model_id;
    if (!id) return "default";
    return models.list.find((m) => m.id === id)?.alias ?? id;
  });

  // True when this task's resolved model runs with no permission gating — drives
  // the prominent danger banner in the task header.
  const modelUnbound = computed(() => {
    const id = store.detail?.model_id;
    if (!id) return false;
    return models.list.find((m) => m.id === id)?.unbound ?? false;
  });

  onMounted(setup);
  watch(idRef, setup);

  // Reconnect → refetch history (fill any gap) + reconcile task state.
  watch(
    () => stream.connected,
    (up) => {
      if (up) {
        loadHistory();
        store.load(idRef.value);
      }
    },
  );
  // A pushed status change for this task → reflect both axes; reload when the
  // operator lifecycle goes terminal to pick up the result row.
  watch(
    () => stream.statusByTask.get(idRef.value),
    (s) => {
      if (!s || !store.detail) return;
      store.detail.task_state = s.task_state;
      store.detail.agent_state = s.agent_state;
      if (["completed", "failed"].includes(s.task_state)) store.load(idRef.value);
    },
  );

  function onApprovalResolved(resolved: AuthRequest) {
    stream.dropApproval(resolved.id);
  }

  // --- Branch diff (lazy) ----------------------------------------------------
  const diffText = ref<string | null>(null);
  const diffError = ref<string | null>(null);
  const diffLoading = ref(false);

  async function loadDiff() {
    diffLoading.value = true;
    diffError.value = null;
    try {
      diffText.value = (await tasksApi.diff(idRef.value)).diff;
    } catch (e) {
      diffError.value = e instanceof Error ? e.message : String(e);
    } finally {
      diffLoading.value = false;
    }
  }

  // --- Edit task -------------------------------------------------------------
  // `task_state` is editable on any task; the run inputs (branch, and the
  // trigger's title/description that drive the prompt) only while pending —
  // before the task is related to a run.
  const editing = ref(false);
  const editBranch = ref("");
  const editTitle = ref("");
  const editDescription = ref("");
  const editTaskState = ref<TaskState>("pending");
  // null = use the global/service default; a model id pins this task to it.
  const editModelId = ref<string | null>(null);
  const savingEdit = ref(false);

  // trigger_data is a serialized TriggerReason; only some variants carry a
  // title/description, so the form shows those inputs conditionally.
  const triggerData = computed(() => {
    const d = store.detail?.trigger_data;
    return d && typeof d === "object" ? (d as Record<string, unknown>) : null;
  });
  const triggerHasTitle = computed(() => typeof triggerData.value?.title === "string");
  const triggerHasDescription = computed(
    () => typeof triggerData.value?.description === "string",
  );

  function startEdit() {
    editBranch.value = store.detail?.branch ?? "";
    editTitle.value = (triggerData.value?.title as string) ?? "";
    editDescription.value = (triggerData.value?.description as string) ?? "";
    editTaskState.value = store.detail?.task_state ?? "pending";
    editModelId.value = store.detail?.model_id ?? null;
    editing.value = true;
  }

  async function saveEdit() {
    savingEdit.value = true;
    try {
      const edits: TaskEdits = { task_state: editTaskState.value };
      if (isPending.value) {
        edits.branch = editBranch.value.trim() || undefined;
        if (triggerHasTitle.value) edits.title = editTitle.value.trim() || undefined;
        if (triggerHasDescription.value) edits.description = editDescription.value;
      }
      // Model is editable in any state — null clears the override, a string pins
      // it; takes effect on the next run/resume (#51).
      edits.model_id = editModelId.value;
      await store.update(idRef.value, edits);
      editing.value = false;
    } catch (e) {
      alert(e instanceof Error ? e.message : String(e));
    } finally {
      savingEdit.value = false;
    }
  }

  // --- Lifecycle actions -----------------------------------------------------
  async function withBusy(label: string, fn: () => Promise<void>) {
    busy.value = label;
    try {
      await fn();
    } catch (e) {
      alert(e instanceof Error ? e.message : String(e));
    } finally {
      busy.value = null;
    }
  }

  async function confirmRun() {
    await withBusy("confirm", () => store.confirm(idRef.value));
  }

  async function retry() {
    await withBusy("retry", async () => {
      const id = await store.retry(idRef.value);
      router.push({ name: "task-detail", params: { id } });
    });
  }

  async function resume() {
    await withBusy("continue", async () => {
      await store.continue_(idRef.value);
      await store.load(idRef.value);
    });
  }

  async function pause() {
    if (!confirm("Pause this task? Claude is stopped and the session id is kept so you can Resume later.")) return;
    await withBusy("kill", () => store.kill(idRef.value));
  }

  // Rewrite this task's agent.env with a fresh token — the mid-turn escape hatch
  // when a long turn outlives the App token's ~1h TTL (#52). Server re-resolves
  // from the service credentials.
  async function refreshToken() {
    await withBusy("token", async () => {
      await tasksApi.refreshToken(idRef.value);
    });
  }

  async function remove() {
    const msg = isLive.value
      ? "An agent is attached. Force kill claude and delete?"
      : "Delete this task and its result?";
    if (!confirm(msg)) return;
    await withBusy("delete", async () => {
      await store.remove(idRef.value);
      router.push({ name: "tasks" });
    });
  }

  // --- Chat / Stop / Redefine ------------------------------------------------
  const message = ref("");
  const sending = ref(false);

  async function sendMessage() {
    const text = message.value.trim();
    if (!text) return;
    sending.value = true;
    try {
      if (wsConnected.value) {
        // A warm agent is attached (running or idle between turns) — straight to
        // its stdin, no delay.
        stream.send({ kind: "chat", task_id: idRef.value, text });
      } else {
        // No live agent: queue it; delivered when the session resumes.
        await tasksApi.pushMessage(idRef.value, text);
        await store.load(idRef.value);
      }
      message.value = "";
    } catch (e) {
      alert(e instanceof Error ? e.message : String(e));
    } finally {
      sending.value = false;
    }
  }

  function redefineGoal() {
    const text = message.value.trim();
    if (!text || !wsConnected.value) return;
    stream.send({ kind: "redefine", task_id: idRef.value, text });
    message.value = "";
  }

  function stopAgent() {
    if (!confirm("Stop the agent? It finishes the current turn, then wraps up.")) return;
    stream.send({ kind: "stop", task_id: idRef.value });
  }

  return {
    store,
    models,
    modelLabel,
    modelUnbound,
    busy,
    pendingApprovals,
    eventText,
    eventCount,
    hasEvents,
    taskNotifications,
    tokensSpent,
    wsConnected,
    isLive,
    isRunning,
    isPending,
    canRetry,
    canContinue,
    canKill,
    canChat,
    onApprovalResolved,
    diffText,
    diffError,
    diffLoading,
    loadDiff,
    editing,
    editBranch,
    editTitle,
    editDescription,
    editTaskState,
    triggerHasTitle,
    triggerHasDescription,
    editModelId,
    savingEdit,
    startEdit,
    saveEdit,
    confirmRun,
    retry,
    resume,
    pause,
    refreshToken,
    remove,
    message,
    sending,
    sendMessage,
    redefineGoal,
    stopAgent,
  };
}
