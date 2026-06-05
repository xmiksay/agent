import { computed, onMounted, onUnmounted, reactive, ref, watch, type Ref } from "vue";
import { useRouter } from "vue-router";
import { useTasksStore } from "../stores/tasks";
import { authApi } from "../api/auth";
import { tasksApi } from "../api/tasks";
import { openTaskStream, type TaskStreamController } from "../api/ws";
import type { AuthRequest, StreamEnvelope } from "../types/api";

/**
 * Everything behind the task detail view: REST detail/result, the live event
 * stream (history seed + WebSocket), operator chat/stop/redefine, the lifecycle
 * actions, and the lazy branch diff. Kept out of the SFC so the view stays
 * presentational and under the file-size cap.
 */
export function useTaskDetail(idRef: Ref<string>) {
  const store = useTasksStore();
  const router = useRouter();
  const busy = ref<string | null>(null);
  const pendingApprovals = ref<AuthRequest[]>([]);

  // --- Live event stream -----------------------------------------------------
  // seq → raw agent event. The persisted /events history seeds 0..N-1; live WS
  // frames continue from N. Dedupe is automatic since seq is the map key.
  const events = reactive(new Map<number, unknown>());
  const wsConnected = ref(false);
  let stream: TaskStreamController | null = null;

  // ClaudeStream consumes a newline-delimited JSON string and reverses it
  // (newest on top), so re-serialize the ordered events into that shape.
  const eventText = computed(() =>
    [...events.keys()]
      .sort((a, b) => a - b)
      .map((k) => JSON.stringify(events.get(k)))
      .join("\n"),
  );
  const eventCount = computed(() => events.size);
  const hasEvents = computed(() => events.size > 0);

  const isLive = computed(() =>
    ["pending", "running"].includes(store.detail?.status ?? ""),
  );
  const isRunning = computed(() => store.detail?.status === "running");
  const isPending = computed(() => store.detail?.status === "pending");
  const canRetry = computed(() =>
    ["failed", "completed", "killed"].includes(store.detail?.status ?? ""),
  );
  const canContinue = computed(() => !!store.detail?.session_id && canRetry.value);
  const canKill = computed(() => isRunning.value);
  const canChat = computed(() => isRunning.value || !!store.detail?.session_id);

  async function reloadPending() {
    try {
      pendingApprovals.value = await authApi.list({ task_id: idRef.value, status: "pending" });
    } catch {
      /* ignore — section will just be empty */
    }
  }

  async function loadHistory() {
    try {
      const { events: hist } = await tasksApi.events(idRef.value);
      hist.forEach((e, i) => events.set(i, e));
    } catch {
      /* history endpoint may 404 for a brand-new task */
    }
  }

  function applyEnvelope(env: StreamEnvelope) {
    if (env.kind === "event") {
      events.set(env.seq, env.payload);
    } else if (env.kind === "auth_request") {
      const r = env.payload as AuthRequest;
      const rest = pendingApprovals.value.filter((p) => p.id !== r.id);
      pendingApprovals.value = r.status === "pending" ? [...rest, r] : rest;
    } else if (env.kind === "status") {
      const s = (env.payload as { status?: string }).status;
      if (s && store.detail) store.detail.status = s;
    }
  }

  function openStream() {
    if (stream) return;
    stream = openTaskStream(idRef.value, {
      onEnvelope: applyEnvelope,
      onOpen: async () => {
        wsConnected.value = true;
        // Refetch history on (re)connect to fill any gap from a dropped socket.
        await loadHistory();
      },
      onClose: async () => {
        wsConnected.value = false;
        // The session may have ended — reconcile status + final history.
        await store.load(idRef.value);
        await loadHistory();
      },
      shouldReconnect: () => isLive.value,
    });
  }

  function closeStream() {
    stream?.close();
    stream = null;
    wsConnected.value = false;
  }

  async function setup() {
    events.clear();
    await store.load(idRef.value);
    await Promise.all([loadHistory(), reloadPending()]);
    if (isLive.value) openStream();
  }

  onMounted(setup);
  onUnmounted(closeStream);
  watch(idRef, async () => {
    closeStream();
    await setup();
  });

  function onApprovalResolved(resolved: AuthRequest) {
    pendingApprovals.value = pendingApprovals.value.filter((p) => p.id !== resolved.id);
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
    await withBusy("confirm", async () => {
      await store.confirm(idRef.value);
      openStream();
    });
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
      if (isLive.value) openStream();
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
      if (isRunning.value && wsConnected.value) {
        // Live: straight to the agent's stdin, no delay.
        stream?.send({ kind: "chat", text });
      } else {
        // Paused/resumable: queue it; delivered on the next resume.
        await tasksApi.pushMessage(idRef.value, text);
        await store.load(idRef.value);
        if (isLive.value) openStream();
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
    stream?.send({ kind: "redefine", text });
    message.value = "";
  }

  function stopAgent() {
    if (!confirm("Stop the agent? It finishes the current turn, then wraps up.")) return;
    stream?.send({ kind: "stop" });
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
