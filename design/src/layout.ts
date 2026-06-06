import { ref, watch } from "vue";

// Top/Side chrome choice — a design tweak so the two shell options compare live.
export type Layout = "top" | "side";

const LAYOUT_KEY = "agent-layout";

// `?layout=side` overrides the stored choice — handy for sharing a comparison.
const urlLayout = typeof location !== "undefined" ? new URLSearchParams(location.search).get("layout") : null;
const stored = typeof localStorage !== "undefined" ? localStorage.getItem(LAYOUT_KEY) : null;
const initial: Layout =
  urlLayout === "top" || urlLayout === "side" ? urlLayout : stored === "side" ? "side" : "top";

export const layout = ref<Layout>(initial);

watch(layout, (v) => {
  try {
    localStorage.setItem(LAYOUT_KEY, v);
  } catch {
    /* private mode */
  }
});
