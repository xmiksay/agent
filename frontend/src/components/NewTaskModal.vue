<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useProjectsStore } from "../stores/projects";
import { useTasksStore } from "../stores/tasks";
import type { ProjectListItem, TriggerKind, TriggerReason } from "../types/api";

const props = defineProps<{ open: boolean }>();
const emit = defineEmits<{ (e: "close"): void; (e: "created", id: string): void }>();

const projectsStore = useProjectsStore();
const tasksStore = useTasksStore();

const projectId = ref<string>("");
const triggerKind = ref<TriggerKind>("issue");
const submitting = ref(false);
const errorMsg = ref<string | null>(null);

const fields = ref({
  iid: 1,
  title: "",
  description: "",
  url: "",
  source_branch: "",
  target_branch: "",
  review_body: "",
  comment: "",
});

function resetFields() {
  fields.value = {
    iid: 1,
    title: "",
    description: "",
    url: "",
    source_branch: "",
    target_branch: "",
    review_body: "",
    comment: "",
  };
  errorMsg.value = null;
}

onMounted(async () => {
  if (!projectsStore.list.length) await projectsStore.refresh();
  if (!projectId.value && projectsStore.list.length) {
    projectId.value = projectsStore.list[0].id;
  }
});

const selectedProject = computed<ProjectListItem | undefined>(() =>
  projectsStore.list.find((p) => p.id === projectId.value),
);

const branchPlaceholder = computed(() => selectedProject.value?.default_branch ?? "");

const triggerKinds: { value: TriggerKind; label: string; hint: string }[] = [
  { value: "issue", label: "Issue", hint: "Implement an issue assigned to the bot." },
  { value: "review_mr", label: "Review MR", hint: "Bot reviews someone else's MR." },
  { value: "fix_review", label: "Fix review", hint: "Bot addresses review feedback on its MR." },
  { value: "mr_comment", label: "MR comment", hint: "Bot replies to a comment on an MR." },
  { value: "issue_comment", label: "Issue comment", hint: "Bot replies to a comment on an issue." },
];

function buildTrigger(): TriggerReason {
  const f = fields.value;
  switch (triggerKind.value) {
    case "issue":
      return {
        type: "issue",
        iid: Number(f.iid),
        title: f.title,
        description: f.description,
        url: f.url,
      };
    case "review_mr":
      return {
        type: "review_mr",
        iid: Number(f.iid),
        title: f.title,
        source_branch: f.source_branch,
        target_branch: f.target_branch || branchPlaceholder.value,
        url: f.url,
      };
    case "fix_review":
      return {
        type: "fix_review",
        iid: Number(f.iid),
        title: f.title,
        source_branch: f.source_branch,
        url: f.url,
        review_body: f.review_body,
      };
    case "mr_comment":
      return {
        type: "mr_comment",
        mr_iid: Number(f.iid),
        comment: f.comment,
        source_branch: f.source_branch,
        url: f.url,
      };
    case "issue_comment":
      return {
        type: "issue_comment",
        issue_iid: Number(f.iid),
        comment: f.comment,
        url: f.url,
      };
  }
}

async function submit() {
  if (!projectId.value) {
    errorMsg.value = "Pick a project";
    return;
  }
  submitting.value = true;
  errorMsg.value = null;
  try {
    const id = await tasksStore.create({
      project_id: projectId.value,
      trigger: buildTrigger(),
    });
    emit("created", id);
    resetFields();
  } catch (e) {
    errorMsg.value = e instanceof Error ? e.message : String(e);
  } finally {
    submitting.value = false;
  }
}

function close() {
  if (submitting.value) return;
  resetFields();
  emit("close");
}
</script>

<template>
  <div
    v-if="props.open"
    class="fixed inset-0 bg-black/40 flex items-center justify-center z-50"
    @click.self="close"
  >
    <div class="bg-white rounded-lg shadow-lg w-[640px] max-h-[90vh] overflow-auto">
      <div class="px-5 py-4 border-b flex items-center justify-between">
        <h2 class="text-lg font-semibold">New task</h2>
        <button class="text-gray-500 hover:text-gray-800" :disabled="submitting" @click="close">
          ✕
        </button>
      </div>

      <div class="px-5 py-4 space-y-4">
        <div>
          <label class="block text-xs uppercase text-gray-500 mb-1">Project</label>
          <select v-model="projectId" class="w-full border rounded px-2 py-1.5">
            <option v-for="p in projectsStore.list" :key="p.id" :value="p.id">
              {{ p.full_name }} ({{ p.provider }})
            </option>
          </select>
        </div>

        <div>
          <label class="block text-xs uppercase text-gray-500 mb-1">Trigger</label>
          <select v-model="triggerKind" class="w-full border rounded px-2 py-1.5">
            <option v-for="k in triggerKinds" :key="k.value" :value="k.value">
              {{ k.label }} — {{ k.hint }}
            </option>
          </select>
        </div>

        <div class="grid grid-cols-2 gap-3">
          <div>
            <label class="block text-xs uppercase text-gray-500 mb-1">
              {{ triggerKind === "issue_comment" ? "Issue iid" : triggerKind === "mr_comment" ? "MR iid" : "iid" }}
            </label>
            <input
              v-model.number="fields.iid"
              type="number"
              min="1"
              class="w-full border rounded px-2 py-1.5"
            />
          </div>
          <div>
            <label class="block text-xs uppercase text-gray-500 mb-1">URL</label>
            <input v-model="fields.url" type="url" class="w-full border rounded px-2 py-1.5" />
          </div>
        </div>

        <div v-if="triggerKind === 'issue' || triggerKind === 'review_mr' || triggerKind === 'fix_review'">
          <label class="block text-xs uppercase text-gray-500 mb-1">Title</label>
          <input v-model="fields.title" type="text" class="w-full border rounded px-2 py-1.5" />
        </div>

        <div v-if="triggerKind === 'issue'">
          <label class="block text-xs uppercase text-gray-500 mb-1">Description</label>
          <textarea
            v-model="fields.description"
            rows="5"
            class="w-full border rounded px-2 py-1.5 font-mono text-sm"
          ></textarea>
        </div>

        <div
          v-if="
            triggerKind === 'review_mr' ||
            triggerKind === 'fix_review' ||
            triggerKind === 'mr_comment'
          "
        >
          <label class="block text-xs uppercase text-gray-500 mb-1">Source branch</label>
          <input
            v-model="fields.source_branch"
            type="text"
            class="w-full border rounded px-2 py-1.5"
          />
        </div>

        <div v-if="triggerKind === 'review_mr'">
          <label class="block text-xs uppercase text-gray-500 mb-1">
            Target branch <span class="text-gray-400">(default: {{ branchPlaceholder }})</span>
          </label>
          <input
            v-model="fields.target_branch"
            type="text"
            :placeholder="branchPlaceholder"
            class="w-full border rounded px-2 py-1.5"
          />
        </div>

        <div v-if="triggerKind === 'fix_review'">
          <label class="block text-xs uppercase text-gray-500 mb-1">Review body</label>
          <textarea
            v-model="fields.review_body"
            rows="4"
            class="w-full border rounded px-2 py-1.5 font-mono text-sm"
          ></textarea>
        </div>

        <div v-if="triggerKind === 'mr_comment' || triggerKind === 'issue_comment'">
          <label class="block text-xs uppercase text-gray-500 mb-1">Comment</label>
          <textarea
            v-model="fields.comment"
            rows="4"
            class="w-full border rounded px-2 py-1.5 font-mono text-sm"
          ></textarea>
        </div>

        <p v-if="errorMsg" class="text-sm text-red-600">{{ errorMsg }}</p>
      </div>

      <div class="px-5 py-3 border-t flex justify-end gap-2 bg-gray-50 rounded-b-lg">
        <button
          class="px-3 py-1.5 text-sm rounded border hover:bg-gray-100"
          :disabled="submitting"
          @click="close"
        >
          Cancel
        </button>
        <button
          class="px-3 py-1.5 text-sm rounded bg-blue-600 text-white hover:bg-blue-700 disabled:opacity-60"
          :disabled="submitting || !projectId"
          @click="submit"
        >
          {{ submitting ? "Creating…" : "Create as pending" }}
        </button>
      </div>
    </div>
  </div>
</template>
