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
  file: "text-gray-100 font-semibold bg-gray-800",
  meta: "text-gray-400",
  hunk: "text-cyan-300 bg-gray-800",
  add: "text-emerald-300 bg-emerald-900/30",
  del: "text-red-300 bg-red-900/30",
  ctx: "text-gray-300",
  "untracked-head": "text-amber-300 font-semibold mt-2",
  untracked: "text-amber-200",
};
</script>

<template>
  <pre
    class="text-xs font-mono bg-gray-900 text-gray-200 p-2 rounded max-h-[32rem] overflow-auto leading-snug"
  ><span
      v-for="(line, i) in lines"
      :key="i"
      :class="cls[line.kind]"
      class="block px-1"
    >{{ line.text || " " }}</span></pre>
</template>
