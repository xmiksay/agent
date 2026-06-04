<script setup lang="ts">
// One row's worth of approve/deny controls + reply input.
//
// Two modes:
//   1. AskUserQuestion (metadata.questions present) — render each question as
//      buttons (single-select) or checkboxes (multi-select). Picks build the
//      reply string ("Q: ...\n  → answer"). An optional free-text override
//      lets the operator type something instead.
//   2. Anything else — plain "approve / deny" with an optional reply textarea.

import { computed, ref } from "vue";
import { authApi } from "../api/auth";
import type { AuthQuestion, AuthRequest } from "../types/api";

const props = defineProps<{ item: AuthRequest; compact?: boolean }>();
const emit = defineEmits<{ resolved: [AuthRequest] }>();

const reply = ref("");
const busy = ref<"approve" | "deny" | null>(null);
const error = ref<string | null>(null);

const questions = computed<AuthQuestion[] | null>(() => {
  const q = props.item.metadata?.questions;
  return Array.isArray(q) && q.length > 0 ? q : null;
});

// Per-question selections: single-select → string|null, multi-select → string[].
const singlePicks = ref<Record<number, string | null>>({});
const multiPicks = ref<Record<number, string[]>>({});
const customReply = ref("");
const useCustom = ref(false);

function pickSingle(qi: number, label: string) {
  singlePicks.value[qi] = singlePicks.value[qi] === label ? null : label;
}

function toggleMulti(qi: number, label: string) {
  const cur = multiPicks.value[qi] ?? [];
  multiPicks.value[qi] = cur.includes(label)
    ? cur.filter((l) => l !== label)
    : [...cur, label];
}

function isPickedSingle(qi: number, label: string): boolean {
  return singlePicks.value[qi] === label;
}

function isPickedMulti(qi: number, label: string): boolean {
  return (multiPicks.value[qi] ?? []).includes(label);
}

function buildReplyFromPicks(): string {
  const qs = questions.value;
  if (!qs) return "";
  return qs
    .map((q, qi) => {
      let answer: string;
      if (q.multiSelect) {
        const picks = multiPicks.value[qi] ?? [];
        answer = picks.length === 0 ? "(no selection)" : picks.join(", ");
      } else {
        answer = singlePicks.value[qi] ?? "(no selection)";
      }
      return `Q: ${q.question}\n→ ${answer}`;
    })
    .join("\n\n");
}

const allAnswered = computed(() => {
  const qs = questions.value;
  if (!qs) return true;
  return qs.every((q, qi) =>
    q.multiSelect
      ? (multiPicks.value[qi] ?? []).length > 0
      : !!singlePicks.value[qi],
  );
});

const canApprove = computed(() => {
  if (questions.value) {
    if (useCustom.value) return customReply.value.trim().length > 0;
    return allAnswered.value;
  }
  return true;
});

async function resolve(decision: "approve" | "deny") {
  busy.value = decision;
  error.value = null;
  try {
    let body: string | undefined;
    if (questions.value && decision === "approve") {
      body = useCustom.value ? customReply.value : buildReplyFromPicks();
    } else {
      body = reply.value || undefined;
    }
    const updated = await authApi.resolve(props.item.id, decision, body);
    emit("resolved", updated);
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  } finally {
    busy.value = null;
  }
}
</script>

<template>
  <div class="space-y-3">
    <!-- AskUserQuestion: structured controls -->
    <template v-if="questions">
      <div
        v-for="(q, qi) in questions"
        :key="qi"
        class="space-y-1.5 border-l-2 border-amber-300 pl-3"
      >
        <div class="text-xs font-medium text-gray-800">
          {{ q.question }}
          <span v-if="q.multiSelect" class="text-[10px] uppercase text-gray-400 ml-1">
            multi
          </span>
        </div>
        <div class="flex flex-wrap gap-1.5">
          <template v-if="q.multiSelect">
            <button
              v-for="opt in q.options"
              :key="opt.label"
              type="button"
              :disabled="!!busy || useCustom"
              class="px-2 py-1 text-xs rounded border transition"
              :class="
                isPickedMulti(qi, opt.label)
                  ? 'bg-amber-600 border-amber-700 text-white'
                  : 'bg-white border-gray-300 text-gray-800 hover:bg-amber-50'
              "
              :title="opt.description"
              @click="toggleMulti(qi, opt.label)"
            >
              <span class="font-medium">{{ opt.label }}</span>
              <span v-if="opt.description" class="text-[10px] opacity-75 ml-1">
                — {{ opt.description }}
              </span>
            </button>
          </template>
          <template v-else>
            <button
              v-for="opt in q.options"
              :key="opt.label"
              type="button"
              :disabled="!!busy || useCustom"
              class="px-2 py-1 text-xs rounded border transition"
              :class="
                isPickedSingle(qi, opt.label)
                  ? 'bg-amber-600 border-amber-700 text-white'
                  : 'bg-white border-gray-300 text-gray-800 hover:bg-amber-50'
              "
              :title="opt.description"
              @click="pickSingle(qi, opt.label)"
            >
              <span class="font-medium">{{ opt.label }}</span>
              <span v-if="opt.description" class="text-[10px] opacity-75 ml-1">
                — {{ opt.description }}
              </span>
            </button>
          </template>
        </div>
      </div>

      <div class="space-y-1">
        <label class="flex items-center gap-1.5 text-[11px] text-gray-600">
          <input v-model="useCustom" type="checkbox" class="rounded" />
          Custom reply instead
        </label>
        <textarea
          v-if="useCustom"
          v-model="customReply"
          :rows="props.compact ? 2 : 3"
          class="w-full border rounded p-2 text-xs font-mono"
          placeholder="Type a free-form answer — sent to claude as the tool result."
        />
      </div>
    </template>

    <!-- Non-question: plain reply -->
    <template v-else>
      <div>
        <label class="block text-[10px] uppercase tracking-wide text-gray-500 mb-1">
          Optional reply
        </label>
        <textarea
          v-model="reply"
          :rows="props.compact ? 1 : 2"
          class="w-full border rounded p-2 text-xs font-mono"
          placeholder="Optional note attached to your decision."
        />
      </div>
    </template>

    <div class="flex gap-2">
      <button
        :disabled="!!busy || !canApprove"
        class="rounded bg-emerald-600 text-white px-3 py-1 text-xs hover:bg-emerald-700 disabled:opacity-60"
        @click="resolve('approve')"
      >
        {{ busy === "approve" ? "Approving…" : "Approve" }}
      </button>
      <button
        :disabled="!!busy"
        class="rounded border border-red-300 text-red-700 px-3 py-1 text-xs hover:bg-red-50 disabled:opacity-60"
        @click="resolve('deny')"
      >
        {{ busy === "deny" ? "Denying…" : "Deny" }}
      </button>
    </div>
    <p v-if="error" class="text-xs text-red-700">{{ error }}</p>
  </div>
</template>
