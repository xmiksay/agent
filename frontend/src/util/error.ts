// Pull a human-readable message out of an unknown thrown value.
// API errors surface as `{ data: string }`; everything else falls back to
// `message` or the stringified value.

export function extractErrorMessage(e: unknown): string {
  if (typeof e === "object" && e !== null) {
    const err = e as { data?: unknown; message?: string };
    if (typeof err.data === "string") return err.data;
    if (err.message) return err.message;
  }
  return String(e);
}
