<script setup lang="ts">
import { computed } from "vue";
import {
  enableNotifications,
  notificationPermission,
  notificationsSupported,
} from "../notifications";

// Only worth showing while the operator hasn't decided yet — once granted the
// app notifies on its own; once denied the browser won't let us re-prompt.
const show = computed(
  () => notificationsSupported && notificationPermission.value === "default",
);
</script>

<template>
  <button
    v-if="show"
    class="font-mono text-xs text-faint hover:text-accent"
    title="Get a notification when a command needs approval"
    @click="enableNotifications"
  >
    enable notifications
  </button>
</template>
