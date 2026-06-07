import { createApp } from "vue";
import { createPinia } from "pinia";
import App from "./App.vue";
import { router } from "./router";
import { initTheme } from "./theme";
import { registerServiceWorker } from "./pwa";
import "./style.css";

// Apply the persisted dark/light class before mount so it lands before first paint.
initTheme();

const app = createApp(App);
app.use(createPinia());
app.use(router);
app.mount("#app");

// Make the app installable to a phone home screen and resilient offline.
registerServiceWorker();
