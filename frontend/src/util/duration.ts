// Compact duration formatter — "1h 23m", "12m 5s", "8s", "—".
// Used both for per-task spent time and for aggregated totals on the stats page.

export function formatSecs(secs: number): string {
  if (!Number.isFinite(secs) || secs <= 0) return "—";
  const total = Math.floor(secs);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  if (h > 0) return `${h}h ${m}m`;
  if (m > 0) return `${m}m ${s}s`;
  return `${s}s`;
}

/**
 * Spent time for a task as a count of seconds.
 *
 * - Finished tasks: finished_at − started_at.
 * - Running tasks: now − started_at (call with the same `now` for whole-table
 *   stability; pass a reactive ref for a ticking value).
 * - Not yet started: 0.
 */
export function taskSpentSecs(
  t: { started_at: string | null; finished_at: string | null; status: string },
  now: Date,
): number {
  if (!t.started_at) return 0;
  const start = new Date(t.started_at).getTime();
  const end = t.finished_at
    ? new Date(t.finished_at).getTime()
    : t.status === "running"
      ? now.getTime()
      : 0;
  if (!end) return 0;
  return Math.max(0, Math.floor((end - start) / 1000));
}
