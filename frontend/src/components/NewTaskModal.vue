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
    class="fixed inset-0 z-50 flex items-center justify-center bg-canvas/70 backdrop-blur-sm"
    @click.self="close"
  >
    <div class="card max-h-[90vh] w-[640px] overflow-auto border-line-2 shadow-[0_30px_80px_-20px_rgba(0,0,0,0.85)]">
      <div class="flex items-center justify-between border-b border-line px-5 py-4">
        <h2 class="font-display text-lg font-bold">New task</h2>
        <button class="btn btn-subtle btn-sm" :disabled="submitting" @click="close">✕</button>
      </div>

      <div class="space-y-4 px-5 py-4">
        <div>
          <label class="label">Project</label>
          <select v-model="projectId" class="select">
            <option v-for="p in projectsStore.list" :key="p.id" :value="p.id">
              {{ p.full_name }} ({{ p.provider }})
            </option>
          </select>
        </div>

        <div>
          <label class="label">Trigger</label>
          <select v-model="triggerKind" class="select">
            <option v-for="k in triggerKinds" :key="k.value" :value="k.value">
              {{ k.label }} — {{ k.hint }}
            </option>
          </select>
        </div>

        <div class="grid grid-cols-2 gap-3">
          <div>
            <label class="label">
              {{ triggerKind === "issue_comment" ? "Issue iid" : triggerKind === "mr_comment" ? "MR iid" : "iid" }}
            </label>
            <input v-model.number="fields.iid" type="number" min="1" class="input" />
          </div>
          <div>
            <label class="label">URL</label>
            <input v-model="fields.url" type="url" class="input" />
          </div>
        </div>

        <div v-if="triggerKind === 'issue' || triggerKind === 'review_mr' || triggerKind === 'fix_review'">
          <label class="label">Title</label>
          <input v-model="fields.title" type="text" class="input" />
        </div>

        <div v-if="triggerKind === 'issue'">
          <label class="label">Description</label>
          <textarea v-model="fields.description" rows="5" class="textarea font-mono" />
        </div>

        <div
          v-if="
            triggerKind === 'review_mr' ||
            triggerKind === 'fix_review' ||
            triggerKind === 'mr_comment'
          "
        >
          <label class="label">Source branch</label>
          <input v-model="fields.source_branch" type="text" class="input font-mono" />
        </div>

        <div v-if="triggerKind === 'review_mr'">
          <label class="label">
            Target branch <span class="normal-case text-faint">(default: {{ branchPlaceholder }})</span>
          </label>
          <input
            v-model="fields.target_branch"
            type="text"
            :placeholder="branchPlaceholder"
            class="input font-mono"
          />
        </div>

        <div v-if="triggerKind === 'fix_review'">
          <label class="label">Review body</label>
          <textarea v-model="fields.review_body" rows="4" class="textarea font-mono" />
        </div>

        <div v-if="triggerKind === 'mr_comment' || triggerKind === 'issue_comment'">
          <label class="label">Comment</label>
          <textarea v-model="fields.comment" rows="4" class="textarea font-mono" />
        </div>

        <p v-if="errorMsg" class="text-sm text-signal-danger">{{ errorMsg }}</p>
      </div>

      <div class="flex justify-end gap-2 border-t border-line px-5 py-3">
        <button class="btn btn-subtle" :disabled="submitting" @click="close">Cancel</button>
        <button class="btn btn-primary" :disabled="submitting || !projectId" @click="submit">
          {{ submitting ? "Creating…" : "Create as pending" }}
        </button>
      </div>
    </div>
  </div>
</template>
