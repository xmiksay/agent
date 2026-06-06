<script setup lang="ts">
// A number that tweens toward its target instead of snapping — used for the
// live token-spend counter so the operator sees the digits climb as the agent
// burns tokens. Pure rAF tween, no deps.
import { onUnmounted, ref, watch } from "vue";

const props = withDefaults(defineProps<{ value: number; duration?: number }>(), {
  duration: 600,
});

const display = ref(props.value);
let frame: number | null = null;

function stop() {
  if (frame !== null) {
    cancelAnimationFrame(frame);
    frame = null;
  }
}

function animateTo(target: number) {
  stop();
  const from = display.value;
  const delta = target - from;
  if (delta === 0) return;
  const start = performance.now();
  const step = (now: number) => {
    const t = Math.min(1, (now - start) / props.duration);
    // easeOutCubic — fast then settles, reads as "spending".
    const eased = 1 - Math.pow(1 - t, 3);
    display.value = Math.round(from + delta * eased);
    frame = t < 1 ? requestAnimationFrame(step) : null;
  };
  frame = requestAnimationFrame(step);
}

watch(() => props.value, animateTo);
onUnmounted(stop);
</script>

<template>
  <span>{{ display.toLocaleString() }}</span>
</template>
