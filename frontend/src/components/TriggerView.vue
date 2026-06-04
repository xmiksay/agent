<script setup lang="ts">
// Render the trigger_data blob (a serialized Rust TriggerReason enum) as a
// readable card instead of raw JSON. Falls back to a JSON dump if the shape
// doesn't match anything we know.
//
// Variants (see src/jobs/types.rs):
//   { type: "issue", iid, title, description, url }
//   { type: "review_mr", iid, title, source_branch, target_branch, url }
//   { type: "fix_review", iid, title, source_branch, url }
//   { type: "mr_comment", mr_iid, comment, source_branch, url }
//   { type: "issue_comment", issue_iid, comment, url }

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
    <h3 class="font-medium">{{ headline }}</h3>

    <dl v-if="meta.length" class="grid grid-cols-[max-content_1fr] gap-x-4 gap-y-1 text-xs">
      <template v-for="m in meta" :key="m.label">
        <dt class="text-gray-500">{{ m.label }}</dt>
        <dd :class="m.mono ? 'font-mono' : ''">
          <a v-if="m.link" :href="m.link" target="_blank" rel="noopener" class="text-blue-700 hover:underline">
            {{ m.value }}
          </a>
          <template v-else>{{ m.value }}</template>
        </dd>
      </template>
    </dl>

    <div v-if="body" class="bg-gray-50 rounded p-3">
      <div class="text-[10px] uppercase tracking-wide text-gray-500 mb-1">
        {{ variant === "issue" ? "Description" : "Comment" }}
      </div>
      <MarkdownView :source="body" />
    </div>
  </div>
  <pre v-else class="text-xs whitespace-pre-wrap font-mono">{{ JSON.stringify(props.data, null, 2) }}</pre>
</template>
