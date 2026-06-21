<script setup lang="ts">
import { computed, ref, toRef, watch } from "vue";
import StatusPill from "../components/StatusPill.vue";
import ProviderBadge from "../components/ProviderBadge.vue";
import TurnTimeline from "../components/TurnTimeline.vue";
import TriggerView from "../components/TriggerView.vue";
import MarkdownView from "../components/MarkdownView.vue";
import DiffView from "../components/DiffView.vue";
import Accordion from "../components/Accordion.vue";
import TaskEditPanel from "../components/TaskEditPanel.vue";
import TaskOutlinePanel from "../components/TaskOutlinePanel.vue";
import AuthApprovalForm from "../components/AuthApprovalForm.vue";
import AnimatedNumber from "../components/AnimatedNumber.vue";
import { useTaskDetail } from "../composables/useTaskDetail";

const props = defineProps<{ id: string }>();

const {
  store, models, queues, queueLabel, modelLabel, modelUnbound, busy, pendingApprovals, eventText, hasEvents, turns, taskNotifications, wsConnected, tokensSpent,
  isLive, isRunning, isPending, canRetry, canContinue, canKill, canChat,
  onApprovalResolved, diffText, diffError, diffLoading, loadDiff,
  editing, editBranch, editTitle, editDescription, editTaskState,
  triggerHasTitle, triggerHasDescription, editModelId, editQueueId, editPriority, savingEdit, startEdit, saveEdit,
  confirmRun, retry, resume, pause, refreshToken, remove,
  message, sending, sendMessage, redefineGoal, stopAgent,
} = useTaskDetail(toRef(props, "id"));

// Accordion open state (view-only).
const showApprovals = ref(false);
const showDiff = ref(false);
// Description starts collapsed; opened on first load only when the task is
// pending (the operator is most likely reviewing what's about to run).
const showDescription = ref(false);
const showOutline = ref(false);
const showOutput = ref(true);
const showRaw = ref(false);

let didInitDescription = false;
watch(
  () => store.detail?.task_state,
  (taskState) => {
    if (didInitDescription || !taskState) return;
    didInitDescription = true;
    showDescription.value = taskState === "pending";
  },
  { immediate: true },
);

// trigger_data is a serialized TriggerReason; every variant carries `url`.
const originUrl = computed<string | null>(() => {
  const d = store.detail?.trigger_data;
  if (!d || typeof d !== "object") return null;
  const u = (d as Record<string, unknown>).url;
  return typeof u === "string" && u.length > 0 ? u : null;
});

watch(showDiff, (v) => {
  if (v && diffText.value === null) loadDiff();
});
// Surface a pending approval the moment it arrives.
watch(pendingApprovals, (p) => {
  if (p.length > 0) showApprovals.value = true;
});
</script>

<template>
  <section v-if="store.detail" class="space-y-4">
    <!-- Unbound model: prominent danger banner — this task runs every command
         with no operator approval. -->
    <div
      v-if="modelUnbound"
      class="rounded-md border border-signal-danger bg-signal-danger/15 px-4 py-3 text-sm font-bold text-signal-danger"
    >
      ⚠ This task runs an UNBOUND model: all commands run without approval.
    </div>

    <!-- Header: identity, status, link to origin, and the primary controls. -->
    <header class="card space-y-3 p-5">
      <div class="flex flex-wrap items-center gap-3">
        <ProviderBadge :provider="store.detail.provider" />
        <h1 class="font-display text-xl font-bold">{{ store.detail.project_path }}</h1>
        <StatusPill :status="store.detail.task_state" />
        <StatusPill :status="store.detail.agent_state" />
        <span
          v-if="isLive"
          class="inline-flex items-center gap-1.5 text-xs"
          :class="wsConnected ? 'text-signal-live' : 'text-faint'"
          :title="wsConnected ? 'Live stream connected' : 'Reconnecting…'"
        >
          <span class="led" :class="wsConnected ? 'led-pulse text-signal-live' : 'text-faint'" />
          {{ wsConnected ? "live" : "offline" }}
        </span>
        <a
          v-if="originUrl"
          :href="originUrl"
          target="_blank"
          rel="noopener"
          class="text-sm text-accent hover:underline"
          :title="originUrl"
        >
          {{ store.detail.trigger_type }} ↗
        </a>
        <span v-else class="text-sm text-muted">{{ store.detail.trigger_type }}</span>
      </div>

      <div class="flex flex-wrap items-center gap-2">
        <button v-if="isPending" :disabled="!!busy" class="btn btn-primary btn-sm" @click="confirmRun">
          {{ busy === "confirm" ? "Starting…" : "Confirm & run" }}
        </button>
        <button v-if="canRetry" :disabled="!!busy" class="btn btn-ghost btn-sm" @click="retry">
          {{ busy === "retry" ? "Retrying…" : "Retry" }}
        </button>
        <button
          v-if="canContinue"
          :disabled="!!busy"
          class="btn btn-primary btn-sm"
          title="Resume the claude session that produced this task"
          @click="resume"
        >
          {{ busy === "continue" ? "Resuming…" : "Resume" }}
        </button>
        <button
          v-if="wsConnected"
          class="btn btn-primary btn-sm"
          title="Graceful stop: the agent finishes the current turn, then wraps up"
          @click="stopAgent"
        >
          Stop
        </button>
        <button
          v-if="canKill"
          :disabled="!!busy"
          class="btn btn-ghost btn-sm"
          title="Hard pause: SIGKILL claude. Session id is preserved so you can Resume later."
          @click="pause"
        >
          {{ busy === "kill" ? "Pausing…" : "Pause" }}
        </button>
        <button
          v-if="store.detail.work_dir"
          :disabled="!!busy"
          class="btn btn-ghost btn-sm"
          title="Rewrite the agent's agent.env with a fresh provider token — the escape hatch when a long turn outlives the token's TTL"
          @click="refreshToken"
        >
          {{ busy === "token" ? "Refreshing…" : "Refresh token" }}
        </button>
        <button
          :disabled="!!busy"
          class="btn btn-danger btn-sm ml-auto"
          :title="isLive ? 'Force-kill claude and delete' : 'Delete'"
          @click="remove"
        >
          {{ busy === "delete" ? "Deleting…" : isLive ? "Kill & delete" : "Delete" }}
        </button>
      </div>

      <dl class="grid grid-cols-2 gap-x-4 gap-y-2 border-t border-line pt-3 text-sm md:grid-cols-4">
        <div>
          <dt class="label mb-0.5">Branch</dt>
          <dd class="truncate font-mono text-xs text-muted">{{ store.detail.branch ?? store.detail.default_branch }}</dd>
        </div>
        <div>
          <dt class="label mb-0.5">Model</dt>
          <dd class="truncate text-xs text-muted">{{ modelLabel }}</dd>
        </div>
        <div>
          <dt class="label mb-0.5">Queue</dt>
          <dd class="flex items-center gap-1.5 text-xs text-muted">
            <span class="truncate">{{ queueLabel ?? "—" }}</span>
            <span
              class="shrink-0 rounded bg-panel px-1.5 py-0.5 font-mono text-[10px] text-faint"
              title="In-queue priority (higher = sooner)"
            >
              p{{ store.detail.priority }}
            </span>
          </dd>
        </div>
        <div>
          <dt class="label mb-0.5">Created</dt>
          <dd class="text-xs text-muted">{{ new Date(store.detail.created_at).toLocaleString() }}</dd>
        </div>
        <div>
          <dt class="label mb-0.5">Finished</dt>
          <dd class="text-xs text-muted">
            {{ store.detail.finished_at ? new Date(store.detail.finished_at).toLocaleString() : "—" }}
          </dd>
        </div>
        <div v-if="store.detail.pid !== null">
          <dt class="label mb-0.5">PID</dt>
          <dd class="font-mono text-xs text-muted">{{ store.detail.pid }}</dd>
        </div>
        <div v-if="store.detail.session_id" class="col-span-2 md:col-span-4">
          <dt class="label mb-0.5">Claude session</dt>
          <dd class="break-all font-mono text-xs text-muted">{{ store.detail.session_id }}</dd>
        </div>
        <div v-if="store.detail.work_dir" class="col-span-2 md:col-span-4">
          <dt class="label mb-0.5">Worktree</dt>
          <dd class="break-all font-mono text-xs text-muted">{{ store.detail.work_dir }}</dd>
        </div>
      </dl>
    </header>

    <!-- Edit task: state on any task; the run inputs (branch/title/description)
         only while pending — before the task is tied to a run. -->
    <TaskEditPanel
      v-model:open="editing"
      v-model:branch="editBranch"
      v-model:title="editTitle"
      v-model:description="editDescription"
      v-model:task-state="editTaskState"
      v-model:model-id="editModelId"
      v-model:queue-id="editQueueId"
      v-model:priority="editPriority"
      :is-pending="isPending"
      :trigger-has-title="triggerHasTitle"
      :trigger-has-description="triggerHasDescription"
      :saving-edit="savingEdit"
      :queue-options="queues.options"
      :model-options="models.options"
      @start="startEdit"
      @save="saveEdit"
      @cancel="editing = false"
    />

    <!-- Ask for permission — present only when something is pending, auto-open. -->
    <Accordion
      v-if="pendingApprovals.length"
      v-model:open="showApprovals"
      title="Ask for permission"
      :subtitle="`${pendingApprovals.length} pending`"
    >
      <ul class="space-y-3 pt-3">
        <li
          v-for="r in pendingApprovals"
          :key="r.id"
          class="space-y-2 rounded-md border border-accent/40 bg-accent/5 p-3"
        >
          <pre class="whitespace-pre-wrap rounded bg-canvas/70 p-2 font-mono text-xs text-accent">{{ r.requested_op }}</pre>
          <p class="text-sm text-muted">{{ r.prompt_to_operator }}</p>
          <AuthApprovalForm :item="r" compact @resolved="onApprovalResolved" />
        </li>
      </ul>
    </Accordion>

    <!-- Chat with the agent — live over the socket when warm, queued otherwise. -->
    <section v-if="canChat" class="card space-y-2 p-4">
      <div class="flex items-center gap-2">
        <h2 class="text-sm font-semibold">Chat</h2>
        <span class="text-xs text-faint">
          {{ wsConnected
            ? "delivered live to the agent"
            : "queued — delivered when the session resumes" }}
        </span>
      </div>
      <textarea
        v-model="message"
        rows="3"
        :disabled="sending"
        class="textarea font-mono"
        placeholder="Message the agent…  (e.g. Also update the README.)"
        @keydown.enter.exact.prevent="sendMessage"
      ></textarea>
      <div class="flex items-center gap-2">
        <span class="font-mono text-[11px] text-faint" title="Output tokens spent this run (thinking included)">
          <AnimatedNumber :value="tokensSpent" /> tokens spent
        </span>
        <button
          v-if="isRunning && wsConnected"
          :disabled="!message.trim()"
          class="btn btn-ghost btn-sm"
          title="Redirect the agent's goal (sent immediately)"
          @click="redefineGoal"
        >
          Redefine goal
        </button>
        <button :disabled="sending || !message.trim()" class="btn btn-primary btn-sm ml-auto" @click="sendMessage">
          {{ sending ? "Sending…" : "Send" }}
        </button>
      </div>
    </section>

    <!-- Branch diff against the default branch. -->
    <Accordion
      v-model:open="showDiff"
      title="Branch diff"
      :subtitle="`vs origin/${store.detail.default_branch}`"
    >
      <template #actions>
        <button class="text-xs text-muted hover:text-ink disabled:opacity-60" :disabled="diffLoading" @click="loadDiff">
          {{ diffLoading ? "Loading…" : "Refresh" }}
        </button>
      </template>
      <div class="pt-3">
        <p v-if="diffError" class="text-xs text-signal-danger">{{ diffError }}</p>
        <p v-else-if="diffText === ''" class="text-sm text-muted">
          No changes against origin/{{ store.detail.default_branch }} yet.
        </p>
        <DiffView v-else-if="diffText !== null" :source="diffText" />
        <p v-else class="text-sm text-muted">Loading…</p>
      </div>
    </Accordion>

    <!-- Task description — what triggered the run. -->
    <Accordion v-model:open="showDescription" title="Task description">
      <div class="space-y-3 pt-3">
        <TriggerView :data="store.detail.trigger_data" />
        <details>
          <summary class="cursor-pointer text-[11px] text-faint">raw payload</summary>
          <pre class="mt-2 whitespace-pre-wrap font-mono text-xs text-muted">{{
            JSON.stringify(store.detail.trigger_data, null, 2)
          }}</pre>
        </details>
      </div>
    </Accordion>

    <!-- Result summary (only once the run produced one). -->
    <section v-if="store.detail.result" class="card space-y-3 p-4">
      <h2 class="text-sm font-semibold">Result</h2>
      <dl class="grid grid-cols-3 gap-3 text-sm">
        <div>
          <dt class="label mb-0.5">Cost</dt>
          <dd class="font-mono text-muted">${{ store.detail.result.cost_usd.toFixed(4) }}</dd>
        </div>
        <div>
          <dt class="label mb-0.5">Turns</dt>
          <dd class="font-mono text-muted">{{ store.detail.result.num_turns }}</dd>
        </div>
        <div>
          <dt class="label mb-0.5">Tokens in / out</dt>
          <dd class="font-mono text-muted">{{ store.detail.result.input_tokens }} / {{ store.detail.result.output_tokens }}</dd>
        </div>
      </dl>
      <div
        class="rounded-md border p-3"
        :class="store.detail.result.is_error ? 'border-signal-danger/40 bg-signal-danger/5' : 'border-line bg-panel-2/60'"
      >
        <pre
          v-if="store.detail.result.is_error"
          class="whitespace-pre-wrap font-mono text-xs text-signal-danger"
        >{{ store.detail.result.result_text }}</pre>
        <MarkdownView v-else :source="store.detail.result.result_text" />
      </div>
    </section>

    <TaskOutlinePanel
      v-if="taskNotifications.length"
      v-model:open="showOutline"
      :notifications="taskNotifications"
    />

    <!-- Output — agent turns (input + output), newest first. Each turn expands
         to its raw events; the "raw json" toggle drops to the flat stream. -->
    <Accordion
      v-model:open="showOutput"
      title="Output"
      :subtitle="`${turns.length} turn${turns.length === 1 ? '' : 's'}`"
    >
      <template #actions>
        <button v-if="hasEvents" class="text-[11px] text-muted hover:text-ink" @click="showRaw = !showRaw">
          {{ showRaw ? "turns" : "raw json" }}
        </button>
      </template>
      <div class="pt-3">
        <p v-if="!hasEvents" class="text-sm text-muted">
          No events yet{{ isLive ? " — waiting for the agent…" : "." }}
        </p>
        <TurnTimeline
          v-else-if="!showRaw"
          :turns="turns"
          :pending="pendingApprovals"
          @resolved="onApprovalResolved"
        />
        <pre
          v-else
          class="max-h-[32rem] overflow-auto whitespace-pre-wrap rounded-md border border-line bg-canvas p-2 font-mono text-xs text-muted"
        >{{ eventText }}</pre>
      </div>
    </Accordion>
  </section>
  <p v-else class="text-faint">Loading…</p>
</template>
