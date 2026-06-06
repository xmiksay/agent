import type { Task } from "../types/api";

export function formatSecs(secs: number): string {
  if (secs < 60) return `${Math.round(secs)}s`;
  const m = Math.floor(secs / 60);
  const s = Math.round(secs % 60);
  if (m < 60) return `${m}m ${s}s`;
  const h = Math.floor(m / 60);
  return `${h}h ${m % 60}m`;
}

export function taskSpentSecs(t: Task, now: Date): number {
  if (!t.started_at) return 0;
  const start = new Date(t.started_at).getTime();
  const end = t.finished_at ? new Date(t.finished_at).getTime() : now.getTime();
  return Math.max(0, (end - start) / 1000);
}
