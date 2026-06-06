<script setup lang="ts">
import { ref } from "vue";
import StatusPill from "../components/StatusPill.vue";
import ProviderBadge from "../components/ProviderBadge.vue";
import TriggerView from "../components/TriggerView.vue";
import DiffView from "../components/DiffView.vue";
import ClaudeStream from "../components/ClaudeStream.vue";
import Accordion from "../components/Accordion.vue";
import AuthApprovalForm from "../components/AuthApprovalForm.vue";
import { authRequests, diffSample, streamSample, taskDetail } from "../fixtures";

// Workbench: TaskDetail renders a single fixture. The live view drives the same
// markup from useTaskDetail() (store + WebSocket).
const detail = taskDetail;
const pending = authRequests.filter((a) => a.task_id === detail.id && a.status === "pending");
const showApprovals = ref(true);
const showDiff = ref(true);
const showDescription = ref(false);
const showOutput = ref(true);
const message = ref("");
</script>

<template>
  <section class="space-y-4">
    <!-- Header -->
    <header class="card space-y-3 p-5">
      <div class="flex flex-wrap items-center gap-3">
        <ProviderBadge :provider="detail.provider" />
        <h1 class="font-display text-xl font-bold">{{ detail.project_path }}</h1>
        <StatusPill :status="detail.status" />
        <span v-if="detail.live" class="inline-flex items-center gap-1.5 text-xs text-signal-live">
          <span class="led led-pulse text-signal-live" /> live
        </span>
        <span class="ml-auto font-mono text-xs text-faint">{{ detail.trigger_type }} ↗</span>
      </div>

      <div class="flex flex-wrap items-center gap-2">
        <button class="btn btn-primary btn-sm">Stop</button>
        <button class="btn btn-ghost btn-sm">Resume</button>
        <button class="btn btn-ghost btn-sm">Pause</button>
        <button class="btn btn-danger btn-sm ml-auto">Kill &amp; delete</button>
      </div>

      <dl class="grid grid-cols-2 gap-x-4 gap-y-2 border-t border-line pt-3 text-sm md:grid-cols-4">
        <div><dt class="label mb-0.5">Branch</dt><dd class="truncate font-mono text-xs text-muted">{{ detail.branch }}</dd></div>
        <div><dt class="label mb-0.5">Created</dt><dd class="text-xs text-muted">{{ new Date(detail.created_at).toLocaleString() }}</dd></div>
        <div><dt class="label mb-0.5">PID</dt><dd class="font-mono text-xs text-muted">{{ detail.pid ?? "—" }}</dd></div>
        <div><dt class="label mb-0.5">Session</dt><dd class="font-mono text-xs text-muted">{{ detail.session_id }}</dd></div>
        <div class="col-span-2 md:col-span-4"><dt class="label mb-0.5">Worktree</dt><dd class="break-all font-mono text-xs text-muted">{{ detail.work_dir }}</dd></div>
      </dl>
    </header>

    <!-- Pending approvals -->
    <Accordion v-if="pending.length" v-model:open="showApprovals" title="Ask for permission" :subtitle="`${pending.length} pending`">
      <ul class="space-y-3 pt-3">
        <li v-for="r in pending" :key="r.id" class="space-y-2 rounded-md border border-accent/40 bg-accent/5 p-3">
          <pre class="rounded bg-canvas/70 p-2 font-mono text-xs text-accent whitespace-pre-wrap">{{ r.requested_op }}</pre>
          <p class="text-sm text-muted">{{ r.prompt_to_operator }}</p>
          <AuthApprovalForm :item="r" compact />
        </li>
      </ul>
    </Accordion>

    <!-- Chat -->
    <section class="card space-y-2 p-4">
      <div class="flex items-center gap-2">
        <h2 class="text-sm font-semibold">Chat</h2>
        <span class="text-xs text-faint">delivered live to the agent</span>
      </div>
      <textarea v-model="message" rows="2" class="textarea font-mono" placeholder="Message the agent…  (e.g. Also update the README.)" />
      <div class="flex justify-end gap-2">
        <button class="btn btn-ghost btn-sm">Redefine goal</button>
        <button class="btn btn-primary btn-sm" :disabled="!message.trim()">Send</button>
      </div>
    </section>

    <!-- Diff -->
    <Accordion v-model:open="showDiff" title="Branch diff" :subtitle="`vs origin/${detail.default_branch}`">
      <div class="pt-3"><DiffView :source="diffSample" /></div>
    </Accordion>

    <!-- Description -->
    <Accordion v-model:open="showDescription" title="Task description">
      <div class="pt-3"><TriggerView :data="detail.trigger_data" /></div>
    </Accordion>

    <!-- Result -->
    <section v-if="detail.result" class="card space-y-3 p-4">
      <h2 class="text-sm font-semibold">Result</h2>
      <dl class="grid grid-cols-3 gap-3 text-sm">
        <div><dt class="label mb-0.5">Cost</dt><dd class="font-mono">${{ detail.result.cost_usd.toFixed(4) }}</dd></div>
        <div><dt class="label mb-0.5">Turns</dt><dd class="font-mono">{{ detail.result.num_turns }}</dd></div>
        <div><dt class="label mb-0.5">Tokens in / out</dt><dd class="font-mono">{{ detail.result.input_tokens }} / {{ detail.result.output_tokens }}</dd></div>
      </dl>
      <div class="rounded-md border border-line bg-panel-2/60 p-3 text-sm text-muted">{{ detail.result.result_text }}</div>
    </section>

    <!-- Output -->
    <Accordion v-model:open="showOutput" title="Output" subtitle="live agent events">
      <div class="pt-3"><ClaudeStream :text="streamSample" :pending="pending" /></div>
    </Accordion>
  </section>
</template>
