import { ref, watch } from "vue";

// Persist the task-list filters so the operator returns to the same view across
// reloads. Same shape/idiom as layout.ts and theme.ts, but four fields so it
// rides as one JSON blob under a single key.
const KEY = "agent-task-filters";

export interface TaskFilters {
  taskState: string;
  agentState: string;
  serviceId: string;
  projectId: string;
}

const EMPTY: TaskFilters = { taskState: "", agentState: "", serviceId: "", projectId: "" };

function read(): TaskFilters {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return { ...EMPTY };
    const v = JSON.parse(raw) as Partial<TaskFilters>;
    return {
      taskState: typeof v.taskState === "string" ? v.taskState : "",
      agentState: typeof v.agentState === "string" ? v.agentState : "",
      serviceId: typeof v.serviceId === "string" ? v.serviceId : "",
      projectId: typeof v.projectId === "string" ? v.projectId : "",
    };
  } catch {
    return { ...EMPTY };
  }
}

export function useTaskFilters() {
  const initial = read();
  const taskState = ref(initial.taskState);
  const agentState = ref(initial.agentState);
  const serviceId = ref(initial.serviceId);
  const projectId = ref(initial.projectId);

  watch([taskState, agentState, serviceId, projectId], () => {
    try {
      localStorage.setItem(
        KEY,
        JSON.stringify({
          taskState: taskState.value,
          agentState: agentState.value,
          serviceId: serviceId.value,
          projectId: projectId.value,
        }),
      );
    } catch {
      // Private mode / storage disabled — filters just won't persist.
    }
  });

  return { taskState, agentState, serviceId, projectId };
}
