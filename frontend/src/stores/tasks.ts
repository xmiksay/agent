import { defineStore } from "pinia";
import { ref } from "vue";
import { tasksApi } from "../api/tasks";
import type { Task, TaskDetail, TaskOutput } from "../types/api";

export const useTasksStore = defineStore("tasks", () => {
  const tasks = ref<Task[]>([]);
  const detail = ref<TaskDetail | null>(null);
  const output = ref<TaskOutput | null>(null);
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

  async function loadOutput(id: string) {
    try {
      output.value = await tasksApi.output(id);
    } catch {
      output.value = null;
    }
  }

  async function confirm(id: string) {
    await tasksApi.confirm(id);
    await load(id);
  }

  async function retry(id: string): Promise<string> {
    const { task_id } = await tasksApi.retry(id);
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

  async function remove(id: string) {
    await tasksApi.remove(id);
    tasks.value = tasks.value.filter((t) => t.id !== id);
    if (detail.value?.id === id) detail.value = null;
  }

  return {
    tasks,
    detail,
    output,
    loading,
    refresh,
    load,
    loadOutput,
    confirm,
    retry,
    continue_,
    kill,
    remove,
  };
});
