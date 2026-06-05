<script setup lang="ts">
import { computed, ref, toRef, watch } from "vue";
import StatusPill from "../components/StatusPill.vue";
import ProviderBadge from "../components/ProviderBadge.vue";
import ClaudeStream from "../components/ClaudeStream.vue";
import TriggerView from "../components/TriggerView.vue";
import MarkdownView from "../components/MarkdownView.vue";
import DiffView from "../components/DiffView.vue";
import Accordion from "../components/Accordion.vue";
import AuthApprovalForm from "../components/AuthApprovalForm.vue";
import { useTaskDetail } from "../composables/useTaskDetail";

const props = defineProps<{ id: string }>();

const {
  store, busy, pendingApprovals, eventText, eventCount, hasEvents, wsConnected,
  isLive, isRunning, isPending, canRetry, canContinue, canKill, canChat,
  onApprovalResolved, diffText, diffError, diffLoading, loadDiff,
  editing, editBranch, editDefaultBranch, savingEdit, startEdit, saveEdit,
  confirmRun, retry, resume, pause, remove,
  message, sending, sendMessage, redefineGoal, stopAgent,
} = useTaskDetail(toRef(props, "id"));

// Accordion open state (view-only).
const showApprovals = ref(false);
const showDiff = ref(false);
const showDescription = ref(true);
const showOutput = ref(true);
const showRaw = ref(false);

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
  <section v-if="store.detail" class="space-y-3">
    <!-- Header: identity, status, link to origin, and the primary controls. -->
    <header class="bg-white rounded shadow-sm p-4 space-y-3">
      <div class="flex items-center gap-3 flex-wrap">
        <ProviderBadge :provider="store.detail.provider" />
        <h1 class="text-lg font-semibold">{{ store.detail.project_path }}</h1>
        <StatusPill :status="store.detail.status" />
        <span
          v-if="isLive"
          class="inline-flex items-center gap-1 text-xs"
          :class="wsConnected ? 'text-emerald-600' : 'text-gray-400'"
          :title="wsConnected ? 'Live stream connected' : 'Reconnecting…'"
        >
          <span class="w-1.5 h-1.5 rounded-full" :class="wsConnected ? 'bg-emerald-500' : 'bg-gray-300'" />
          {{ wsConnected ? "live" : "offline" }}
        </span>
        <a
          v-if="originUrl"
          :href="originUrl"
          target="_blank"
          rel="noopener"
          class="text-sm text-blue-700 hover:underline"
          :title="originUrl"
        >
          {{ store.detail.trigger_type }} ↗
        </a>
        <span v-else class="text-sm text-gray-500">{{ store.detail.trigger_type }}</span>
      </div>

      <div class="flex flex-wrap items-center gap-2">
        <button
          v-if="isPending"
          :disabled="!!busy"
          class="rounded bg-blue-600 text-white px-3 py-1.5 text-sm hover:bg-blue-700 disabled:opacity-60"
          @click="confirmRun"
        >
          {{ busy === "confirm" ? "Starting…" : "Confirm & run" }}
        </button>
        <button
          v-if="canRetry"
          :disabled="!!busy"
          class="rounded bg-blue-600 text-white px-3 py-1.5 text-sm hover:bg-blue-700 disabled:opacity-60"
          @click="retry"
        >
          {{ busy === "retry" ? "Retrying…" : "Retry" }}
        </button>
        <button
          v-if="canContinue"
          :disabled="!!busy"
          class="rounded bg-emerald-600 text-white px-3 py-1.5 text-sm hover:bg-emerald-700 disabled:opacity-60"
          title="Resume the claude session that produced this task"
          @click="resume"
        >
          {{ busy === "continue" ? "Resuming…" : "Resume" }}
        </button>
        <button
          v-if="isRunning"
          class="rounded bg-amber-600 text-white px-3 py-1.5 text-sm hover:bg-amber-700"
          title="Graceful stop: the agent finishes the current turn, then wraps up"
          @click="stopAgent"
        >
          Stop
        </button>
        <button
          v-if="canKill"
          :disabled="!!busy"
          class="rounded border border-amber-300 text-amber-700 px-3 py-1.5 text-sm hover:bg-amber-50 disabled:opacity-60"
          title="Hard pause: SIGKILL claude. Session id is preserved so you can Resume later."
          @click="pause"
        >
          {{ busy === "kill" ? "Pausing…" : "Pause" }}
        </button>
        <button
          :disabled="!!busy"
          class="rounded border border-red-300 text-red-700 px-3 py-1.5 text-sm hover:bg-red-50 disabled:opacity-60 ml-auto"
          :title="isRunning ? 'Force-kill claude and delete' : 'Delete'"
          @click="remove"
        >
          {{ busy === "delete" ? "Deleting…" : isRunning ? "Kill & delete" : "Delete" }}
        </button>
      </div>

      <dl class="grid grid-cols-2 md:grid-cols-4 gap-x-4 gap-y-2 text-sm border-t border-gray-100 pt-3">
        <div>
          <dt class="text-[11px] uppercase text-gray-400">Branch</dt>
          <dd class="font-mono text-xs truncate">{{ store.detail.branch ?? store.detail.default_branch }}</dd>
        </div>
        <div>
          <dt class="text-[11px] uppercase text-gray-400">Created</dt>
          <dd class="text-xs">{{ new Date(store.detail.created_at).toLocaleString() }}</dd>
        </div>
        <div>
          <dt class="text-[11px] uppercase text-gray-400">Finished</dt>
          <dd class="text-xs">
            {{ store.detail.finished_at ? new Date(store.detail.finished_at).toLocaleString() : "—" }}
          </dd>
        </div>
        <div v-if="store.detail.pid !== null">
          <dt class="text-[11px] uppercase text-gray-400">PID</dt>
          <dd class="font-mono text-xs">{{ store.detail.pid }}</dd>
        </div>
        <div v-if="store.detail.session_id" class="col-span-2 md:col-span-4">
          <dt class="text-[11px] uppercase text-gray-400">Claude session</dt>
          <dd class="font-mono text-xs break-all">{{ store.detail.session_id }}</dd>
        </div>
        <div v-if="store.detail.work_dir" class="col-span-2 md:col-span-4">
          <dt class="text-[11px] uppercase text-gray-400">Worktree</dt>
          <dd class="font-mono text-xs break-all">{{ store.detail.work_dir }}</dd>
        </div>
      </dl>
    </header>

    <!-- Edit (pending only). -->
    <Accordion v-if="isPending" v-model:open="editing" title="Edit task" @update:open="(v) => v && startEdit()">
      <div class="space-y-2 pt-3">
        <label class="block text-xs text-gray-500">
          Branch
          <input
            v-model="editBranch"
            :disabled="savingEdit"
            class="mt-1 w-full text-sm font-mono border border-gray-300 rounded p-2 disabled:opacity-60"
            placeholder="feature-branch"
          />
        </label>
        <label class="block text-xs text-gray-500">
          Default branch (diff / MR base)
          <input
            v-model="editDefaultBranch"
            :disabled="savingEdit"
            class="mt-1 w-full text-sm font-mono border border-gray-300 rounded p-2 disabled:opacity-60"
          />
        </label>
        <p class="text-xs text-gray-500">The branch can't equal the default branch.</p>
        <div class="flex justify-end gap-2">
          <button
            class="rounded border border-gray-300 px-3 py-1.5 text-sm hover:bg-gray-50 disabled:opacity-60"
            :disabled="savingEdit"
            @click="editing = false"
          >
            Cancel
          </button>
          <button
            class="rounded bg-blue-600 text-white px-3 py-1.5 text-sm hover:bg-blue-700 disabled:opacity-60"
            :disabled="savingEdit"
            @click="saveEdit"
          >
            {{ savingEdit ? "Saving…" : "Save" }}
          </button>
        </div>
      </div>
    </Accordion>

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
          class="rounded border border-amber-200 bg-amber-50 p-3 space-y-2"
        >
          <pre class="bg-white/70 rounded text-xs p-2 whitespace-pre-wrap font-mono">{{ r.requested_op }}</pre>
          <p class="text-sm text-amber-900">{{ r.prompt_to_operator }}</p>
          <AuthApprovalForm :item="r" compact @resolved="onApprovalResolved" />
        </li>
      </ul>
    </Accordion>

    <!-- Chat with the agent — live over the socket when running, queued otherwise. -->
    <section v-if="canChat" class="bg-white p-4 rounded shadow-sm space-y-2">
      <div class="flex items-center gap-2">
        <h2 class="font-medium text-sm">Chat</h2>
        <span class="text-xs text-gray-500">
          {{ isRunning && wsConnected
            ? "delivered live to the running agent"
            : "queued — delivered on the next resume" }}
        </span>
      </div>
      <textarea
        v-model="message"
        rows="3"
        :disabled="sending"
        class="w-full text-sm font-mono border border-gray-300 rounded p-2 disabled:opacity-60"
        placeholder="Message the agent…  (e.g. Also update the README.)"
        @keydown.enter.exact.prevent="sendMessage"
      ></textarea>
      <div class="flex items-center gap-2">
        <button
          v-if="isRunning && wsConnected"
          :disabled="!message.trim()"
          class="rounded border border-gray-300 px-3 py-1.5 text-sm hover:bg-gray-50 disabled:opacity-60"
          title="Redirect the agent's goal (sent immediately)"
          @click="redefineGoal"
        >
          Redefine goal
        </button>
        <button
          :disabled="sending || !message.trim()"
          class="rounded bg-blue-600 text-white px-4 py-1.5 text-sm hover:bg-blue-700 disabled:opacity-60 ml-auto"
          @click="sendMessage"
        >
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
        <button
          class="text-xs text-gray-500 hover:text-gray-800 disabled:opacity-60"
          :disabled="diffLoading"
          @click="loadDiff"
        >
          {{ diffLoading ? "Loading…" : "Refresh" }}
        </button>
      </template>
      <div class="pt-3">
        <p v-if="diffError" class="text-xs text-red-700">{{ diffError }}</p>
        <p v-else-if="diffText === ''" class="text-sm text-gray-500">
          No changes against origin/{{ store.detail.default_branch }} yet.
        </p>
        <DiffView v-else-if="diffText !== null" :source="diffText" />
        <p v-else class="text-sm text-gray-500">Loading…</p>
      </div>
    </Accordion>

    <!-- Task description — what triggered the run. -->
    <Accordion v-model:open="showDescription" title="Task description">
      <div class="space-y-3 pt-3">
        <TriggerView :data="store.detail.trigger_data" />
        <details>
          <summary class="cursor-pointer text-[11px] text-gray-500">raw payload</summary>
          <pre class="text-xs whitespace-pre-wrap mt-2">{{
            JSON.stringify(store.detail.trigger_data, null, 2)
          }}</pre>
        </details>
      </div>
    </Accordion>

    <!-- Result summary (only once the run produced one). -->
    <section v-if="store.detail.result" class="bg-white p-4 rounded shadow-sm space-y-2">
      <h2 class="font-medium text-sm">Result</h2>
      <dl class="grid grid-cols-3 gap-3 text-sm">
        <div>
          <dt class="text-xs text-gray-500">Cost</dt>
          <dd>${{ store.detail.result.cost_usd.toFixed(4) }}</dd>
        </div>
        <div>
          <dt class="text-xs text-gray-500">Turns</dt>
          <dd>{{ store.detail.result.num_turns }}</dd>
        </div>
        <div>
          <dt class="text-xs text-gray-500">Tokens</dt>
          <dd>{{ store.detail.result.input_tokens }} / {{ store.detail.result.output_tokens }}</dd>
        </div>
      </dl>
      <div class="p-3 rounded" :class="store.detail.result.is_error ? 'bg-red-50' : 'bg-gray-50'">
        <pre
          v-if="store.detail.result.is_error"
          class="text-xs whitespace-pre-wrap font-mono text-red-900"
        >{{ store.detail.result.result_text }}</pre>
        <MarkdownView v-else :source="store.detail.result.result_text" />
      </div>
    </section>

    <!-- Output — live agent events, newest first. -->
    <Accordion
      v-model:open="showOutput"
      title="Output"
      :subtitle="`${eventCount} event${eventCount === 1 ? '' : 's'}`"
    >
      <template #actions>
        <button
          v-if="hasEvents"
          class="text-[11px] text-gray-500 hover:text-gray-800"
          @click="showRaw = !showRaw"
        >
          {{ showRaw ? "formatted" : "raw json" }}
        </button>
      </template>
      <div class="pt-3">
        <p v-if="!hasEvents" class="text-sm text-gray-500">
          No events yet{{ isLive ? " — waiting for the agent…" : "." }}
        </p>
        <ClaudeStream
          v-else-if="!showRaw"
          :text="eventText"
          :pending="pendingApprovals"
          @resolved="onApprovalResolved"
        />
        <pre
          v-else
          class="text-xs whitespace-pre-wrap font-mono bg-gray-50 p-2 rounded max-h-[32rem] overflow-auto"
        >{{ eventText }}</pre>
      </div>
    </Accordion>
  </section>
  <p v-else class="text-gray-500">Loading…</p>
</template>
