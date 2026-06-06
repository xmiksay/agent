<script setup lang="ts">
// Render the trigger_data blob (a serialized Rust TriggerReason enum) as a
// readable card instead of raw JSON. Falls back to a JSON dump if the shape
// doesn't match anything we know. Logic is unchanged from the live component.
import { computed } from "vue";
import MarkdownView from "./MarkdownView.vue";

const props = defineProps<{ data: unknown }>();

const t = computed(() => {
  const v = props.data;
  if (!v || typeof v !== "object") return null;
  return v as Record<string, unknown>;
});

const variant = computed(() => (t.value?.type as string | undefined) ?? null);

const headline = computed(() => {
  const o = t.value;
  if (!o) return "";
  switch (variant.value) {
    case "issue":
      return `Issue #${o.iid}: ${o.title ?? ""}`;
    case "review_mr":
      return `Review MR !${o.iid}: ${o.title ?? ""}`;
    case "fix_review":
      return `Fix review on MR !${o.iid}: ${o.title ?? ""}`;
    case "mr_comment":
      return `Comment on MR !${o.mr_iid}`;
    case "issue_comment":
      return `Comment on issue #${o.issue_iid}`;
    default:
      return variant.value ?? "trigger";
  }
});

const body = computed<string | null>(() => {
  const o = t.value;
  if (!o) return null;
  const v = (o.description ?? o.comment) as unknown;
  return typeof v === "string" && v.length > 0 ? v : null;
});

interface MetaItem {
  label: string;
  value: string;
  mono?: boolean;
  link?: string;
}

const meta = computed<MetaItem[]>(() => {
  const o = t.value;
  if (!o) return [];
  const items: MetaItem[] = [];
  if (typeof o.source_branch === "string") {
    items.push({ label: "Source branch", value: o.source_branch, mono: true });
  }
  if (typeof o.target_branch === "string") {
    items.push({ label: "Target branch", value: o.target_branch, mono: true });
  }
  if (typeof o.url === "string" && o.url.length > 0) {
    items.push({ label: "URL", value: o.url, link: o.url });
  }
  return items;
});
</script>

<template>
  <div v-if="t" class="space-y-3">
    <h3 class="font-display text-base font-semibold text-ink">{{ headline }}</h3>

    <dl v-if="meta.length" class="grid grid-cols-[max-content_1fr] gap-x-4 gap-y-1 text-xs">
      <template v-for="m in meta" :key="m.label">
        <dt class="uppercase tracking-label text-faint">{{ m.label }}</dt>
        <dd :class="m.mono ? 'font-mono text-muted' : 'text-muted'">
          <a v-if="m.link" :href="m.link" target="_blank" rel="noopener" class="text-accent hover:underline">
            {{ m.value }}
          </a>
          <template v-else>{{ m.value }}</template>
        </dd>
      </template>
    </dl>

    <div v-if="body" class="rounded-md border border-line bg-panel-2/60 p-3">
      <div class="mb-1 text-[10px] uppercase tracking-label text-faint">
        {{ variant === "issue" ? "Description" : "Comment" }}
      </div>
      <MarkdownView :source="body" />
    </div>
  </div>
  <pre v-else class="whitespace-pre-wrap font-mono text-xs text-muted">{{ JSON.stringify(props.data, null, 2) }}</pre>
</template>
