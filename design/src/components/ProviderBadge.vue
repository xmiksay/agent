<script setup lang="ts">
import type { ProviderKind } from "../types/api";

defineProps<{ provider: ProviderKind }>();
</script>

<template>
  <span
    class="tag uppercase"
    :class="
      provider === 'gitlab'
        ? 'border-signal-release/35 text-signal-release'
        : 'border-line-2 text-ink'
    "
  >
    {{ provider }}
  </span>
</template>
