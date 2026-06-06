import { ref, watch } from "vue";

// Dark/Light switching for the Instrument system. The palette lives in CSS as
// `--c-*` channel triples (src/style.css): `:root` = dark, `.theme-light` = light.
// Here we just flip that class and remember the choice in localStorage.

export type ThemeName = "dark" | "light";

const THEME_KEY = "agent-theme";

function readInitial(): ThemeName {
  // `?theme=light` is a one-time override; it's written through below so the
  // choice persists even after the param is dropped from the URL.
  const url =
    typeof location !== "undefined" ? new URLSearchParams(location.search).get("theme") : null;
  if (url === "light" || url === "dark") return url;
  const stored = typeof localStorage !== "undefined" ? localStorage.getItem(THEME_KEY) : null;
  return stored === "light" || stored === "dark" ? stored : "dark";
}

export const theme = ref<ThemeName>(readInitial());

function persist() {
  try {
    localStorage.setItem(THEME_KEY, theme.value);
  } catch {
    /* private mode */
  }
}

function apply() {
  document.documentElement.classList.toggle("theme-light", theme.value === "light");
}

watch(theme, () => {
  persist();
  apply();
});

/** Call once before mount so the class lands before first paint and the resolved
 *  choice is written through (so it survives a reload even before any toggle). */
export function initTheme() {
  persist();
  apply();
}
