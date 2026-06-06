<script setup lang="ts">
// The Agent sigil — an instrument bezel (hexagon) around an "A" glyph, a pulsing
// signal core, and a slowly rotating dashed orbit. Pure inline SVG + CSS so it
// scales crisply and themes from `currentColor` (set text-accent on the parent).
withDefaults(defineProps<{ size?: number }>(), { size: 28 });
</script>

<template>
  <svg
    :width="size"
    :height="size"
    viewBox="0 0 32 32"
    fill="none"
    class="agent-logo text-accent"
    aria-label="Agent"
    role="img"
  >
    <defs>
      <linearGradient id="agentStroke" x1="0" y1="0" x2="32" y2="32">
        <stop offset="0" stop-color="#ffd27a" />
        <stop offset="1" stop-color="#ffb02e" />
      </linearGradient>
      <radialGradient id="agentCore" cx="0.5" cy="0.5" r="0.5">
        <stop offset="0" stop-color="#fff3da" />
        <stop offset="1" stop-color="#ffb02e" />
      </radialGradient>
    </defs>

    <!-- rotating orbit -->
    <circle class="orbit" cx="16" cy="16" r="14.2" stroke="#ffb02e" stroke-opacity="0.35"
      stroke-width="0.75" stroke-dasharray="2 3.5" />

    <!-- hexagon bezel -->
    <polygon points="16,2.4 27.8,9.2 27.8,22.8 16,29.6 4.2,22.8 4.2,9.2"
      stroke="url(#agentStroke)" stroke-width="1.4" stroke-linejoin="round" />

    <!-- A glyph -->
    <path d="M16 9 L10.5 23 M16 9 L21.5 23 M12.6 18.4 H19.4"
      stroke="url(#agentStroke)" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" />

    <!-- pulsing signal core at the apex -->
    <circle class="core" cx="16" cy="9" r="2.1" fill="url(#agentCore)" />
  </svg>
</template>

<style scoped>
.agent-logo .orbit {
  transform-origin: 16px 16px;
  animation: agent-spin 9s linear infinite;
}
.agent-logo .core {
  transform-origin: 16px 9px;
  filter: drop-shadow(0 0 4px #ffb02e);
  animation: agent-pulse 1.8s ease-in-out infinite;
}
@keyframes agent-spin {
  to { transform: rotate(360deg); }
}
@keyframes agent-pulse {
  0%, 100% { opacity: 1; transform: scale(1); }
  50% { opacity: 0.55; transform: scale(0.82); }
}
@media (prefers-reduced-motion: reduce) {
  .agent-logo .orbit, .agent-logo .core { animation: none; }
}
</style>
