import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

const BACKEND = process.env.AGENT_BACKEND ?? "http://127.0.0.1:3000";

export default defineConfig({
  plugins: [vue()],
  server: {
    port: 5173,
    proxy: {
      "/api": { target: BACKEND, changeOrigin: true },
      "/webhook": { target: BACKEND, changeOrigin: true },
      "/internal": { target: BACKEND, changeOrigin: true },
      "/health": { target: BACKEND, changeOrigin: true },
    },
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
  },
});
