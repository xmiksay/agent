import { computed, onMounted, ref, watch, type Ref } from "vue";
import { useRouter } from "vue-router";
import { useTasksStore } from "../stores/tasks";
import { useStreamStore } from "../stores/stream";
import { authApi } from "../api/auth";
import { tasksApi } from "../api/tasks";
import type { AuthRequest } from "../types/api";

/**
 * Everything behind the task detail view: REST detail/result, the live event
 * stream (history seed + WebSocket), operator chat/stop/redefine, the lifecycle
 * actions, and the lazy branch diff. Kept out of the SFC so the view stays
 * presentational and under the file-size cap.
 */
export function useTaskDetail(idRef: Ref<string>) {
  const store = useTasksStore();
  const stream = useStreamStore();
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

  // Pending approvals for this task, sliced from the shared live set.
  const pendingApprovals = computed(() =>
    [...stream.approvals.values()].filter((a) => a.task_id === idRef.value),
  );

  // A task is "live" while pending/running OR its agent is warm (idle between
  // turns). `wsConnected` = socket up AND this task live, so chat goes straight
  // to the agent rather than being queued.
  const isLive = computed(
    () =>
      ["pending", "running"].includes(store.detail?.status ?? "") ||
      store.detail?.live === true,
  );
  const isRunning = computed(() => store.detail?.status === "running");
  const isPending = computed(() => store.detail?.status === "pending");
  const canRetry = computed(() =>
    ["failed", "completed", "killed"].includes(store.detail?.status ?? ""),
  );
  const canContinue = computed(() => !!store.detail?.session_id && canRetry.value);
  const canKill = computed(() => isRunning.value);
  const canChat = computed(() => isRunning.value || !!store.detail?.session_id);
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
    await Promise.all([loadHistory(), reloadPending()]);
  }

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
  // A pushed status change for this task → reflect it; reload on terminal states
  // to pick up the result row.
  watch(
    () => stream.statusByTask.get(idRef.value),
    (s) => {
      if (!s || !store.detail) return;
      store.detail.status = s;
      if (["completed", "failed", "killed"].includes(s)) store.load(idRef.value);
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

  // --- Edit task (pending only) ----------------------------------------------
  const editing = ref(false);
  const editBranch = ref("");
  const editDefaultBranch = ref("");
  const savingEdit = ref(false);

  function startEdit() {
    editBranch.value = store.detail?.branch ?? "";
    editDefaultBranch.value = store.detail?.default_branch ?? "";
    editing.value = true;
  }

  async function saveEdit() {
    savingEdit.value = true;
    try {
      await store.update(idRef.value, {
        branch: editBranch.value.trim() || undefined,
        default_branch: editDefaultBranch.value.trim() || undefined,
      });
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

  async function remove() {
    const msg = isRunning.value
      ? "Task is running. Force kill claude and delete?"
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
    busy,
    pendingApprovals,
    eventText,
    eventCount,
    hasEvents,
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
    editDefaultBranch,
    savingEdit,
    startEdit,
    saveEdit,
    confirmRun,
    retry,
    resume,
    pause,
    remove,
    message,
    sending,
    sendMessage,
    redefineGoal,
    stopAgent,
  };
}
