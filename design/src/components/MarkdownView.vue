<script setup lang="ts">
import { computed } from "vue";
import { marked } from "marked";
import DOMPurify from "dompurify";

const props = defineProps<{ source: string }>();

// Render untrusted markdown (issue/comment bodies) as sanitized HTML.
const html = computed(() => {
  const raw = marked.parse(props.source ?? "", {
    async: false,
    gfm: true,
    breaks: true,
  }) as string;
  return DOMPurify.sanitize(raw, { ADD_ATTR: ["target"] });
});
</script>

<template>
  <div class="markdown" v-html="html" />
</template>

<style>
/* Instrument-themed prose for agent/issue/comment markdown. */
.markdown {
  font-size: 0.875rem;
  line-height: 1.5;
  color: #c8cdd8;
}
.markdown p { margin: 0.4em 0; }
.markdown h1, .markdown h2, .markdown h3, .markdown h4 {
  font-weight: 600;
  margin: 0.8em 0 0.3em;
  color: #e9ecf2;
}
.markdown h1 { font-size: 1.15rem; }
.markdown h2 { font-size: 1.05rem; }
.markdown h3 { font-size: 1rem; }
.markdown ul, .markdown ol { margin: 0.4em 0 0.4em 1.4em; }
.markdown ul { list-style: disc; }
.markdown ol { list-style: decimal; }
.markdown li { margin: 0.15em 0; }
.markdown a { color: #ffb02e; text-decoration: underline; }
.markdown code {
  background: #1a1e26;
  padding: 0.1em 0.3em;
  border-radius: 3px;
  font-size: 0.85em;
  font-family: "IBM Plex Mono", ui-monospace, SFMono-Regular, monospace;
}
.markdown pre {
  background: #0c0e12;
  color: #e9ecf2;
  border: 1px solid #262b35;
  padding: 0.7em;
  border-radius: 6px;
  overflow-x: auto;
  font-size: 0.8em;
  margin: 0.5em 0;
}
.markdown pre code { background: transparent; padding: 0; color: inherit; }
.markdown blockquote {
  border-left: 2px solid #333a46;
  margin: 0.5em 0;
  padding: 0.1em 0.8em;
  color: #9aa3b2;
}
.markdown table { border-collapse: collapse; margin: 0.5em 0; }
.markdown th, .markdown td { border: 1px solid #262b35; padding: 0.3em 0.6em; }
.markdown hr { border: 0; border-top: 1px solid #262b35; margin: 0.8em 0; }
</style>
