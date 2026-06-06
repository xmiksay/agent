import type { Config } from "tailwindcss";

// "Instrument" theme — the portable artifact. To adopt in the live app, copy this
// `extend` block into agent/frontend/tailwind.config.ts (and the token blocks from
// src/style.css). Colors resolve from CSS variables (RGB channel triples) so the
// same utilities drive both the dark default and the `.theme-light` variant, and
// individual tokens can be tweaked live — see src/theme.ts.
const v = (name: string) => `rgb(var(${name}) / <alpha-value>)`;

export default {
  content: ["./index.html", "./src/**/*.{vue,ts,tsx}"],
  theme: {
    extend: {
      colors: {
        canvas: v("--c-canvas"),
        panel: v("--c-panel"),
        "panel-2": v("--c-panel-2"),
        "panel-3": v("--c-panel-3"),
        line: v("--c-line"),
        "line-2": v("--c-line-2"),
        ink: v("--c-ink"),
        muted: v("--c-muted"),
        faint: v("--c-faint"),
        accent: {
          DEFAULT: v("--c-accent"),
          deep: v("--c-accent-deep"),
          soft: v("--c-accent-soft"),
          ink: v("--c-accent-ink"),
        },
        signal: {
          live: v("--c-live"),
          ok: v("--c-ok"),
          danger: v("--c-danger"),
          auth: v("--c-auth"),
          release: v("--c-release"),
        },
      },
      fontFamily: {
        display: ['"Bricolage Grotesque"', "system-ui", "sans-serif"],
        sans: ['"Archivo"', "system-ui", "sans-serif"],
        mono: ['"IBM Plex Mono"', "ui-monospace", "SFMono-Regular", "monospace"],
      },
      borderRadius: {
        sm: "3px",
        DEFAULT: "5px",
        md: "6px",
        lg: "8px",
        xl: "12px",
      },
      letterSpacing: { label: "0.14em" },
    },
  },
} satisfies Config;
