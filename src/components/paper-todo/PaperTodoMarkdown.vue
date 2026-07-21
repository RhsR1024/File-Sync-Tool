<script setup lang="ts">
import MarkdownIt from 'markdown-it';
import { onBeforeUnmount, ref, watch } from 'vue';

import { resolvePaperAssets } from '@/lib/paperTodo';

const props = defineProps<{ content: string }>();

const rendered = ref('');
let renderToken = 0;
let renderTimer: ReturnType<typeof setTimeout> | null = null;

const markdown = new MarkdownIt({
  html: false,
  linkify: true,
  breaks: true,
  typographer: false,
});

const defaultLinkOpen = markdown.renderer.rules.link_open
  ?? ((tokens, index, options, _env, self) => self.renderToken(tokens, index, options));

markdown.renderer.rules.link_open = (tokens, index, options, env, self) => {
  tokens[index].attrSet('target', '_blank');
  tokens[index].attrSet('rel', 'noopener noreferrer');
  return defaultLinkOpen(tokens, index, options, env, self);
};

markdown.renderer.rules.image = (tokens, index, _options, env) => {
  const token = tokens[index];
  const source = token.attrGet('src') ?? '';
  const [rawAlt, rawSize = ''] = (token.content || 'image').split('|', 2);
  const alt = markdown.utils.escapeHtml(rawAlt || 'image');
  if (/^https?:/i.test(source)) {
    return `<span class="paper-image-blocked">[${alt}]</span>`;
  }
  if (!source.startsWith('i:')) return '';
  const id = source.slice(2);
  const url = (env.assetUrls as Record<string, string> | undefined)?.[id];
  if (!url) return `<span class="paper-image-missing">[${alt}]</span>`;
  const percent = rawSize.match(/\b(\d{1,3})%\b/)?.[1];
  const dimensions = rawSize.match(/\b(\d{1,4})x(\d{1,4})\b/);
  const widthStyle = percent
    ? `width:${Math.min(100, Math.max(10, Number(percent)))}%`
    : dimensions
      ? `width:${Math.min(4096, Number(dimensions[1]))}px;max-height:${Math.min(4096, Number(dimensions[2]))}px`
      : '';
  return `<img src="${markdown.utils.escapeHtml(url)}" alt="${alt}" style="${widthStyle}" loading="lazy" decoding="async">`;
};

async function renderContent(content: string): Promise<void> {
  const token = ++renderToken;
  const ids = [...content.matchAll(/\bi:([a-fA-F0-9]{16,64})\b/g)].map((match) => match[1]);
  const assetUrls = await resolvePaperAssets([...new Set(ids)]);
  if (token !== renderToken) return;
  rendered.value = markdown.render(content, { assetUrls });
}

watch(
  () => props.content,
  (content) => {
    if (renderTimer) clearTimeout(renderTimer);
    renderTimer = setTimeout(() => void renderContent(content), 100);
  },
  { immediate: true },
);

onBeforeUnmount(() => {
  if (renderTimer) clearTimeout(renderTimer);
  renderToken += 1;
});
</script>

<template>
  <article class="paper-markdown" v-html="rendered"></article>
</template>

<style scoped>
.paper-markdown :deep(h1),
.paper-markdown :deep(h2),
.paper-markdown :deep(h3),
.paper-markdown :deep(h4),
.paper-markdown :deep(h5),
.paper-markdown :deep(h6) {
  margin: 0.8em 0 0.35em;
  font-weight: 700;
  line-height: 1.25;
}
.paper-markdown :deep(h1) { font-size: 1.55em; }
.paper-markdown :deep(h2) { font-size: 1.35em; }
.paper-markdown :deep(h3) { font-size: 1.18em; }
.paper-markdown :deep(p),
.paper-markdown :deep(ul),
.paper-markdown :deep(ol),
.paper-markdown :deep(blockquote),
.paper-markdown :deep(pre) { margin: 0.55em 0; }
.paper-markdown :deep(ul),
.paper-markdown :deep(ol) { padding-left: 1.35em; }
.paper-markdown :deep(ul) { list-style: disc; }
.paper-markdown :deep(ol) { list-style: decimal; }
.paper-markdown :deep(blockquote) {
  border-left: 3px solid rgb(100 116 139 / 0.45);
  padding-left: 0.8em;
  color: rgb(71 85 105);
}
.paper-markdown :deep(code) {
  border-radius: 4px;
  background: rgb(15 23 42 / 0.08);
  padding: 0.1em 0.3em;
  font-family: ui-monospace, SFMono-Regular, Consolas, monospace;
  font-size: 0.9em;
}
.paper-markdown :deep(pre) {
  overflow-x: auto;
  border-radius: 6px;
  background: rgb(15 23 42 / 0.9);
  padding: 0.75em;
  color: rgb(241 245 249);
}
.paper-markdown :deep(pre code) { background: transparent; padding: 0; }
.paper-markdown :deep(a) { color: rgb(2 132 199); text-decoration: underline; }
.paper-markdown :deep(hr) { margin: 0.9em 0; border-color: rgb(100 116 139 / 0.25); }
.paper-markdown :deep(img) {
  display: block;
  max-width: 100%;
  height: auto;
  margin: 0.75em auto;
  border-radius: 6px;
}
.paper-markdown :deep(.paper-image-blocked),
.paper-markdown :deep(.paper-image-missing) {
  display: block;
  margin: 0.5em 0;
  color: rgb(100 116 139);
  font-size: 0.85em;
}
</style>
