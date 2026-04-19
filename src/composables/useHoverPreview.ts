import { onBeforeUnmount, ref } from 'vue';

import type { ClipboardItem } from '@/lib/clipboardTypes';

export interface HoverPreviewOptions {
  delayMs?: number;
  hideDelayMs?: number;
  minScale?: number;
  maxScale?: number;
}

export function useHoverPreview(opts: HoverPreviewOptions = {}) {
  const delayMs = opts.delayMs ?? 500;
  const hideDelayMs = opts.hideDelayMs ?? 150;
  const minScale = opts.minScale ?? 0.5;
  const maxScale = opts.maxScale ?? 5;

  const activeItem = ref<ClipboardItem | null>(null);
  const scale = ref(1);

  let showTimer: number | null = null;
  let hideTimer: number | null = null;

  function clearTimers() {
    if (showTimer !== null) {
      clearTimeout(showTimer);
      showTimer = null;
    }
    if (hideTimer !== null) {
      clearTimeout(hideTimer);
      hideTimer = null;
    }
  }

  function onEnter(item: ClipboardItem) {
    clearTimers();
    scale.value = 1;
    showTimer = window.setTimeout(() => {
      activeItem.value = item;
      showTimer = null;
    }, delayMs);
  }

  function onLeave() {
    clearTimers();
    hideTimer = window.setTimeout(() => {
      activeItem.value = null;
      hideTimer = null;
    }, hideDelayMs);
  }

  function onWheelZoom(e: WheelEvent) {
    if (!e.ctrlKey || !activeItem.value || activeItem.value.kind !== 'image') return;
    e.preventDefault();
    const delta = e.deltaY < 0 ? 0.1 : -0.1;
    scale.value = Math.max(minScale, Math.min(maxScale, scale.value + delta));
  }

  onBeforeUnmount(clearTimers);

  return {
    activeItem,
    scale,
    onEnter,
    onLeave,
    onWheelZoom,
  };
}
