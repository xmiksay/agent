import { ref, watch } from "vue";

// Top/Side chrome choice, remembered in localStorage so it survives reloads.
export type Layout = "top" | "side";

const LAYOUT_KEY = "agent-layout";

function readInitial(): Layout {
  // `?layout=side` is a one-time override; it's written through below so the
  // choice persists even after the param is dropped from the URL.
  const url =
    typeof location !== "undefined" ? new URLSearchParams(location.search).get("layout") : null;
  if (url === "top" || url === "side") return url;
  const stored = typeof localStorage !== "undefined" ? localStorage.getItem(LAYOUT_KEY) : null;
  return stored === "side" || stored === "top" ? stored : "top";
}

export const layout = ref<Layout>(readInitial());

function persist() {
  try {
    localStorage.setItem(LAYOUT_KEY, layout.value);
  } catch {
    /* private mode */
  }
}

// Write through the resolved initial so the choice is stored from first paint.
persist();
watch(layout, persist);
