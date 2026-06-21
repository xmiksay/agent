import { defineConfig } from "vitest/config";
import vue from "@vitejs/plugin-vue";

// Unit tests for the composables/stores. jsdom gives the DOM globals the Vue
// reactivity + Pinia stores expect (window, document, localStorage); the vue
// plugin lets `.vue` imports resolve even though current tests only hit `.ts`.
export default defineConfig({
  plugins: [vue()],
  test: {
    environment: "jsdom",
    setupFiles: ["src/test/setup.ts"],
    include: ["src/**/*.{test,spec}.ts"],
  },
});
