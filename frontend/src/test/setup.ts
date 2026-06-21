// jsdom under vitest ships no Storage implementation, but the session store
// reads localStorage the moment it's constructed. Polyfill a minimal in-memory
// one and reset it before every test so state never leaks between cases.
import { beforeEach } from "vitest";

class MemoryStorage implements Storage {
  private store = new Map<string, string>();
  get length(): number {
    return this.store.size;
  }
  clear(): void {
    this.store.clear();
  }
  getItem(key: string): string | null {
    return this.store.has(key) ? this.store.get(key)! : null;
  }
  key(index: number): string | null {
    return [...this.store.keys()][index] ?? null;
  }
  removeItem(key: string): void {
    this.store.delete(key);
  }
  setItem(key: string, value: string): void {
    this.store.set(key, String(value));
  }
}

// Always install our own — don't merely fill an `undefined` gap. Newer Node
// exposes a *native* `localStorage` global that throws unless started with
// `--localstorage-file`, and jsdom doesn't shadow it; a conditional polyfill
// would leave that throwing accessor in place. Force a deterministic in-memory
// store so tests never touch real persistence.
Object.defineProperty(globalThis, "localStorage", {
  value: new MemoryStorage(),
  writable: true,
  configurable: true,
});

beforeEach(() => globalThis.localStorage.clear());
