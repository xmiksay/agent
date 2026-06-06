<script setup lang="ts">
import { computed } from "vue";

const props = defineProps<{ source: string }>();

type Kind = "file" | "meta" | "hunk" | "add" | "del" | "ctx" | "untracked-head" | "untracked";

const lines = computed<{ kind: Kind; text: string }[]>(() => {
  const out: { kind: Kind; text: string }[] = [];
  let inUntracked = false;
  for (const raw of (props.source ?? "").split("\n")) {
    if (raw === "Untracked files:") {
      inUntracked = true;
      out.push({ kind: "untracked-head", text: raw });
      continue;
    }
    if (inUntracked) {
      out.push({ kind: "untracked", text: raw });
      continue;
    }
    if (raw.startsWith("diff --git ")) out.push({ kind: "file", text: raw });
    else if (
      raw.startsWith("index ") ||
      raw.startsWith("--- ") ||
      raw.startsWith("+++ ") ||
      raw.startsWith("new file mode") ||
      raw.startsWith("deleted file mode") ||
      raw.startsWith("similarity index") ||
      raw.startsWith("rename ") ||
      raw.startsWith("Binary files")
    )
      out.push({ kind: "meta", text: raw });
    else if (raw.startsWith("@@")) out.push({ kind: "hunk", text: raw });
    else if (raw.startsWith("+")) out.push({ kind: "add", text: raw });
    else if (raw.startsWith("-")) out.push({ kind: "del", text: raw });
    else out.push({ kind: "ctx", text: raw });
  }
  return out;
});

const cls: Record<Kind, string> = {
  file: "text-ink font-semibold bg-panel-2",
  meta: "text-faint",
  hunk: "text-signal-live bg-panel-2",
  add: "text-signal-ok bg-signal-ok/10",
  del: "text-signal-danger bg-signal-danger/10",
  ctx: "text-muted",
  "untracked-head": "text-accent font-semibold mt-2",
  untracked: "text-accent/80",
};
</script>

<template>
  <pre
    class="max-h-[32rem] overflow-auto rounded-md border border-line bg-canvas p-2 font-mono text-xs leading-snug text-muted"
  ><span
      v-for="(line, i) in lines"
      :key="i"
      :class="cls[line.kind]"
      class="block px-1"
    >{{ line.text || " " }}</span></pre>
</template>
