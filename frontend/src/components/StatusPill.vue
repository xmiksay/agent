<script setup lang="ts">
const props = defineProps<{ status: string }>();

const tone = (s: string) => {
  switch (s) {
    case "pending":
      return "bg-yellow-100 text-yellow-800";
    case "awaiting_auth":
      return "bg-purple-100 text-purple-800";
    case "running":
    case "active":
      return "bg-blue-100 text-blue-800";
    case "completed":
    case "approved":
      return "bg-green-100 text-green-800";
    case "failed":
    case "denied":
      return "bg-red-100 text-red-800";
    case "releasing":
      return "bg-orange-100 text-orange-800";
    default:
      return "bg-gray-100 text-gray-800";
  }
};
</script>

<template>
  <span
    class="inline-flex items-center rounded-full px-2.5 py-0.5 text-xs font-medium"
    :class="tone(props.status)"
  >
    {{ props.status }}
  </span>
</template>
