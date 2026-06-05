import { defineStore } from "pinia";
import { ref } from "vue";
import { tasksApi } from "../api/tasks";
import type { NewTaskBody, Task, TaskDetail, TaskEdits } from "../types/api";

export const useTasksStore = defineStore("tasks", () => {
  const tasks = ref<Task[]>([]);
  const detail = ref<TaskDetail | null>(null);
  const loading = ref(false);

  async function refresh(status?: string) {
    loading.value = true;
    try {
      tasks.value = await tasksApi.list(status);
    } finally {
      loading.value = false;
    }
  }

  async function load(id: string) {
    detail.value = await tasksApi.get(id);
  }

  async function confirm(id: string) {
    await tasksApi.confirm(id);
    await load(id);
  }

  async function retry(id: string): Promise<string> {
    const { task_id } = await tasksApi.retry(id);
    return task_id;
  }

  async function create(body: NewTaskBody): Promise<string> {
    const { task_id } = await tasksApi.create(body);
    return task_id;
  }

  async function continue_(id: string): Promise<string> {
    const { task_id } = await tasksApi.continue_(id);
    return task_id;
  }

  async function kill(id: string) {
    await tasksApi.kill(id);
    await load(id);
  }

  async function update(id: string, edits: TaskEdits) {
    await tasksApi.update(id, edits);
    await load(id);
  }

  async function remove(id: string) {
    await tasksApi.remove(id);
    tasks.value = tasks.value.filter((t) => t.id !== id);
    if (detail.value?.id === id) detail.value = null;
  }

  return {
    tasks,
    detail,
    loading,
    refresh,
    load,
    confirm,
    retry,
    create,
    continue_,
    kill,
    update,
    remove,
  };
});
