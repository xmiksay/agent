import { ref, watch } from "vue";

// Dark/Light switching for the Instrument system. The palette lives in CSS as
// `--c-*` channel triples (src/style.css): `:root` = dark, `.theme-light` = light.
// Here we just flip that class and remember the choice.

export type ThemeName = "dark" | "light";

const THEME_KEY = "agent-theme";

// `?theme=light` wins over the stored choice — handy for sharing a comparison.
const urlTheme = typeof location !== "undefined" ? new URLSearchParams(location.search).get("theme") : null;
const stored = typeof localStorage !== "undefined" ? localStorage.getItem(THEME_KEY) : null;
const initial: ThemeName =
  urlTheme === "light" || urlTheme === "dark" ? urlTheme : stored === "light" ? "light" : "dark";

export const theme = ref<ThemeName>(initial);

function apply() {
  document.documentElement.classList.toggle("theme-light", theme.value === "light");
}

watch(theme, () => {
  try {
    localStorage.setItem(THEME_KEY, theme.value);
  } catch {
    /* private mode */
  }
  apply();
});

/** Call once before mount so the class lands before first paint. */
export function initTheme() {
  apply();
}
