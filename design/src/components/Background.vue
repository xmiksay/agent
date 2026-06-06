<script setup lang="ts">
// Decorative only — a slow orbital system behind the app. Pure CSS, fixed and
// pointer-events-none, masked toward the lower-centre so dense views stay legible.
</script>

<template>
  <div class="space" aria-hidden="true">
    <div class="system">
      <div class="sun" />
      <div class="orbit o1"><span class="planet p1" /></div>
      <div class="orbit o2"><span class="planet p2" /></div>
      <div class="orbit o3"><span class="planet p3" /></div>
      <div class="orbit o4"><span class="planet p4" /></div>
      <div class="orbit o5"><span class="planet p5" /></div>
    </div>
  </div>
</template>

<style scoped>
.space {
  position: fixed;
  inset: 0;
  z-index: -10;
  overflow: hidden;
  pointer-events: none;
  opacity: 1;
  /* Concentrate the system in the lower-centre and fade it out under the chrome. */
  -webkit-mask: radial-gradient(130% 115% at 50% 80%, #000 46%, transparent 92%);
  mask: radial-gradient(130% 115% at 50% 80%, #000 46%, transparent 92%);
}
.system {
  position: absolute;
  left: 50%;
  top: 72%;
}

.sun {
  position: absolute;
  left: -34px;
  top: -34px;
  width: 68px;
  height: 68px;
  border-radius: 50%;
  background: radial-gradient(circle at 50% 42%, #fff1d4 0%, #ffb02e 58%, transparent 72%);
  filter: blur(1px);
  animation: sun-pulse 7s ease-in-out infinite;
}

.orbit {
  position: absolute;
  left: 50%;
  top: 50%;
  border: 1px solid rgb(var(--c-ink) / 0.12);
  border-radius: 50%;
  transform-origin: center;
}
.o1 { width: 240px;  height: 240px;  margin: -120px 0 0 -120px; animation: spin 24s linear infinite; }
.o2 { width: 400px;  height: 400px;  margin: -200px 0 0 -200px; animation: spin 38s linear infinite reverse; }
.o3 { width: 580px;  height: 580px;  margin: -290px 0 0 -290px; animation: spin 58s linear infinite; }
.o4 { width: 780px;  height: 780px;  margin: -390px 0 0 -390px; animation: spin 86s linear infinite reverse; }
.o5 { width: 1040px; height: 1040px; margin: -520px 0 0 -520px; animation: spin 124s linear infinite; }

.planet {
  position: absolute;
  left: 50%;
  top: 0;
  transform: translate(-50%, -50%);
  border-radius: 50%;
}
.p1 { width: 13px; height: 13px; background: radial-gradient(circle at 35% 30%, #d6f5fb, #3ec1d3 72%); box-shadow: 0 0 18px 2px #3ec1d3; }
.p2 { width: 20px; height: 20px; background: radial-gradient(circle at 35% 30%, #ffe9c4, #ffb02e 72%); box-shadow: 0 0 24px 3px #ffb02e; }
.p3 { width: 11px; height: 11px; background: radial-gradient(circle at 35% 30%, #ece3ff, #b692f6 72%); box-shadow: 0 0 18px 2px #b692f6; }
.p4 { width: 26px; height: 26px; background: radial-gradient(circle at 35% 30%, #ffe0c2, #f78c3c 72%); box-shadow: 0 0 28px 3px #f78c3c; }
/* A ringed body. */
.p5 { width: 14px; height: 14px; background: radial-gradient(circle at 35% 30%, #c2e9d0, #56d364 72%); box-shadow: 0 0 14px #56d364; }
.p5::after {
  content: "";
  position: absolute;
  left: 50%;
  top: 50%;
  width: 30px;
  height: 10px;
  border: 1px solid rgba(86, 211, 100, 0.5);
  border-radius: 50%;
  transform: translate(-50%, -50%) rotate(-18deg);
}

@keyframes spin { to { transform: rotate(360deg); } }
@keyframes sun-pulse {
  0%, 100% { opacity: 0.55; transform: scale(1); }
  50% { opacity: 0.8; transform: scale(1.06); }
}
@media (prefers-reduced-motion: reduce) {
  .orbit, .sun { animation: none; }
}
</style>
