<script setup lang="ts">
const props = defineProps<{ status: string }>();

// Map a task/auth status to its signal color. The class drives both the LED
// glow (via currentColor) and the label tint, so one map themes the whole pill.
const tone = (s: string): string => {
  switch (s) {
    case "pending":
      return "text-accent";
    case "awaiting_auth":
      return "text-signal-auth";
    case "running":
    case "active":
      return "text-signal-live";
    case "completed":
    case "approved":
      return "text-signal-ok";
    case "failed":
    case "denied":
      return "text-signal-danger";
    case "releasing":
      return "text-signal-release";
    default:
      return "text-muted";
  }
};

const isLive = (s: string) => s === "running" || s === "active";
</script>

<template>
  <span class="pill" :class="tone(props.status)">
    <span class="led" :class="[tone(props.status), { 'led-pulse': isLive(props.status) }]" />
    {{ props.status }}
  </span>
</template>
