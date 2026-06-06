<script setup lang="ts">
import { ref } from "vue";
import Logo from "./components/Logo.vue";
import StatusPill from "./components/StatusPill.vue";
import ProviderBadge from "./components/ProviderBadge.vue";
import TriggerView from "./components/TriggerView.vue";
import Accordion from "./components/Accordion.vue";

const statuses = [
  "pending",
  "running",
  "completed",
  "failed",
  "awaiting_auth",
  "releasing",
];

const issueTrigger = {
  type: "issue",
  iid: 42,
  title: "Fix the post-login redirect",
  description:
    "After sign-in the user lands on `/` instead of `/dashboard`.\n\n- Repro: log in from the marketing page\n- Expected: redirect to the **last visited** dashboard",
  url: "https://github.com/acme/agent/issues/42",
};
const mrTrigger = {
  type: "review_mr",
  iid: 17,
  title: "RS485 → Home Assistant bridge",
  source_branch: "feat/rs485-bridge",
  target_branch: "main",
  url: "https://gitlab.com/acme/lighting/-/merge_requests/17",
};

const accOpen = ref(true);
</script>

<template>
  <div class="min-h-screen">
    <!-- App chrome -->
    <header class="sticky top-0 z-30 border-b border-line bg-canvas/80 backdrop-blur-md">
      <div class="mx-auto flex max-w-6xl items-center gap-6 px-6 py-3">
        <div class="flex items-center gap-2.5">
          <Logo :size="28" />
          <span class="font-display text-lg font-bold tracking-tight">Agent</span>
          <span class="led led-pulse text-signal-live" />
        </div>
        <span class="ml-auto font-mono text-xs text-faint">component workbench</span>
      </div>
    </header>

    <main class="mx-auto max-w-6xl space-y-12 px-6 py-12">
      <header>
        <p class="font-mono text-xs uppercase tracking-label text-accent">Instrument · live components</p>
        <h1 class="mt-2 font-display text-4xl font-bold tracking-tight">Component workbench</h1>
        <p class="mt-2 max-w-2xl text-sm text-muted">
          Real Vue SFCs on the live app's stack. Each block below is the redesigned
          component with its actual props — copy the file into <code class="tag">agent/frontend/src/components</code> to adopt.
        </p>
      </header>

      <!-- StatusPill -->
      <section class="space-y-4 border-t border-line pt-8">
        <h2 class="text-sm font-semibold uppercase tracking-label text-faint">StatusPill.vue</h2>
        <div class="flex flex-wrap gap-2">
          <StatusPill v-for="s in statuses" :key="s" :status="s" />
        </div>
      </section>

      <!-- Buttons (class-based) -->
      <section class="space-y-4 border-t border-line pt-8">
        <h2 class="text-sm font-semibold uppercase tracking-label text-faint">Buttons · <code class="tag">.btn</code></h2>
        <div class="flex flex-wrap items-center gap-3">
          <button class="btn btn-primary">Confirm run</button>
          <button class="btn btn-ghost">View diff</button>
          <button class="btn btn-subtle">Cancel</button>
          <button class="btn btn-danger">Kill task</button>
          <button class="btn btn-ghost btn-sm">Retry</button>
          <button class="btn btn-primary" disabled>Disabled</button>
        </div>
      </section>

      <!-- Badges -->
      <section class="space-y-4 border-t border-line pt-8">
        <h2 class="text-sm font-semibold uppercase tracking-label text-faint">ProviderBadge.vue + tags</h2>
        <div class="flex flex-wrap items-center gap-2">
          <ProviderBadge provider="github" />
          <ProviderBadge provider="gitlab" />
          <span class="tag">issue #42</span>
          <span class="tag">MR !17</span>
          <span class="tag text-ink">task_4f9c21</span>
        </div>
      </section>

      <!-- Forms (class-based) -->
      <section class="space-y-4 border-t border-line pt-8">
        <h2 class="text-sm font-semibold uppercase tracking-label text-faint">Forms · <code class="tag">.input .select .textarea</code></h2>
        <div class="grid max-w-2xl gap-4 sm:grid-cols-2">
          <div>
            <label class="label">Branch</label>
            <input class="input" value="42-fix-login-button" />
          </div>
          <div>
            <label class="label">Provider</label>
            <select class="select"><option>github</option><option>gitlab</option></select>
          </div>
          <div class="sm:col-span-2">
            <label class="label">Prompt</label>
            <textarea class="textarea" rows="2" placeholder="Describe the change…" />
          </div>
        </div>
      </section>

      <!-- TriggerView -->
      <section class="space-y-4 border-t border-line pt-8">
        <h2 class="text-sm font-semibold uppercase tracking-label text-faint">TriggerView.vue</h2>
        <div class="grid gap-4 lg:grid-cols-2">
          <div class="card p-4"><TriggerView :data="issueTrigger" /></div>
          <div class="card p-4"><TriggerView :data="mrTrigger" /></div>
        </div>
      </section>

      <!-- Accordion -->
      <section class="space-y-4 border-t border-line pt-8">
        <h2 class="text-sm font-semibold uppercase tracking-label text-faint">Accordion.vue</h2>
        <Accordion v-model:open="accOpen" title="Allowed operations" subtitle="3 rules">
          <template #actions>
            <button class="btn btn-ghost btn-sm">Edit</button>
          </template>
          <div class="space-y-1.5 font-mono text-xs text-muted">
            <p>Bash(npm run *)</p>
            <p>Bash(cargo *)</p>
            <p>Read · Edit</p>
          </div>
        </Accordion>
      </section>

      <!-- Table (class-based) -->
      <section class="space-y-4 border-t border-line pt-8">
        <h2 class="text-sm font-semibold uppercase tracking-label text-faint">Data table · <code class="tag">.tbl</code></h2>
        <div class="card overflow-hidden">
          <table class="tbl">
            <thead>
              <tr><th>Status</th><th>Task</th><th>Provider</th><th>Branch</th></tr>
            </thead>
            <tbody>
              <tr>
                <td><StatusPill status="running" /></td>
                <td class="font-mono text-xs text-ink">task_4f9c21</td>
                <td><ProviderBadge provider="github" /></td>
                <td class="font-mono text-xs text-muted">42-fix-login-button</td>
              </tr>
              <tr>
                <td><StatusPill status="awaiting_auth" /></td>
                <td class="font-mono text-xs text-ink">task_a13b08</td>
                <td><ProviderBadge provider="gitlab" /></td>
                <td class="font-mono text-xs text-muted">feat/rs485-bridge</td>
              </tr>
              <tr>
                <td><StatusPill status="completed" /></td>
                <td class="font-mono text-xs text-ink">task_77de10</td>
                <td><ProviderBadge provider="github" /></td>
                <td class="font-mono text-xs text-muted">fix-upload-race</td>
              </tr>
            </tbody>
          </table>
        </div>
      </section>
    </main>
  </div>
</template>
