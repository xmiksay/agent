import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

export default defineConfig({
  // Relative base so the build works when served from a subpath — e.g. the
  // `design` tool's live preview at `/raw/preview/`. (The live app, served from
  // the site root, can drop this.)
  base: "./",
  plugins: [vue()],
  server: { port: 5180 },
  build: { outDir: "preview", emptyOutDir: true },
});
