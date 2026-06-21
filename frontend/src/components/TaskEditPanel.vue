<script setup lang="ts">
// "Edit task" accordion: state on any task; the run inputs (branch/title/
// description/queue/priority) only while pending — before the task is tied to a
// run. The edit fields are two-way bound back to the composable via defineModel.
import Accordion from "./Accordion.vue";
import type { TaskState } from "../types/api";

type Option = { value: string; label: string };

defineProps<{
  isPending: boolean;
  triggerHasTitle: boolean;
  triggerHasDescription: boolean;
  savingEdit: boolean;
  queueOptions: Option[];
  modelOptions: Option[];
}>();

const emit = defineEmits<{ start: []; save: []; cancel: [] }>();

const open = defineModel<boolean>("open", { required: true });
const branch = defineModel<string>("branch", { required: true });
const title = defineModel<string>("title", { required: true });
const description = defineModel<string>("description", { required: true });
const taskState = defineModel<TaskState>("taskState", { required: true });
const modelId = defineModel<string | null>("modelId", { required: true });
const queueId = defineModel<string | null>("queueId", { required: true });
const priority = defineModel<number>("priority", { required: true });
</script>

<template>
  <Accordion v-model:open="open" title="Edit task" @update:open="(v) => v && emit('start')">
    <div class="space-y-3 pt-3">
      <div>
        <label class="label">State</label>
        <select v-model="taskState" :disabled="savingEdit" class="input">
          <option value="pending">pending</option>
          <option value="working_on">working_on</option>
          <option value="completed">completed</option>
          <option value="failed">failed</option>
        </select>
      </div>
      <template v-if="isPending">
        <div>
          <label class="label">Branch</label>
          <input
            v-model="branch"
            :disabled="savingEdit"
            class="input font-mono"
            placeholder="feature-branch"
          />
          <p class="mt-1 text-xs text-muted">The branch can't equal the default branch.</p>
        </div>
        <div v-if="triggerHasTitle">
          <label class="label">Title</label>
          <input v-model="title" :disabled="savingEdit" class="input" />
        </div>
        <div v-if="triggerHasDescription">
          <label class="label">Description</label>
          <textarea v-model="description" rows="6" :disabled="savingEdit" class="textarea font-mono"></textarea>
        </div>
        <div class="grid grid-cols-2 gap-3">
          <div>
            <label class="label">Queue</label>
            <select v-model="queueId" :disabled="savingEdit" class="select">
              <option :value="null">— not queued —</option>
              <option v-for="q in queueOptions" :key="q.value" :value="q.value">
                {{ q.label }}
              </option>
            </select>
          </div>
          <div>
            <label class="label">Priority</label>
            <input
              v-model.number="priority"
              type="number"
              :disabled="savingEdit"
              class="input font-mono"
            />
            <p class="mt-1 text-xs text-muted">Higher runs sooner.</p>
          </div>
        </div>
      </template>
      <p v-else class="text-xs text-muted">
        Branch, title and description can only be edited while the task is pending.
      </p>
      <div>
        <label class="label">Model</label>
        <select v-model="modelId" :disabled="savingEdit" class="select">
          <option :value="null">— use default —</option>
          <option v-for="m in modelOptions" :key="m.value" :value="m.value">
            {{ m.label }}
          </option>
        </select>
        <p class="mt-1 text-xs text-muted">Applies on the next run/resume.</p>
      </div>
      <div class="flex justify-end gap-2">
        <button class="btn btn-ghost btn-sm" :disabled="savingEdit" @click="emit('cancel')">Cancel</button>
        <button class="btn btn-primary btn-sm" :disabled="savingEdit" @click="emit('save')">
          {{ savingEdit ? "Saving…" : "Save" }}
        </button>
      </div>
    </div>
  </Accordion>
</template>
