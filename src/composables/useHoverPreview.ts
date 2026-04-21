import { onBeforeUnmount } from 'vue';

import type { ClipboardItem } from '@/lib/clipboardTypes';
import {
  resolveHoverPreviewTarget,
  type HoverPreviewTarget,
} from '@/lib/clipboardPreviewHelpers';
import { clipboardApi } from '@/lib/tauri';

export interface HoverPreviewOptions {
  delayMs?: number | (() => number);
  hideDelayMs?: number;
  onError?: (error: unknown) => void;
}

export function useHoverPreview(opts: HoverPreviewOptions = {}) {
  const hideDelayMs = opts.hideDelayMs ?? 150;

  let showTimer: number | null = null;
  let hideTimer: number | null = null;

  function resolveDelayMs(): number {
    const delayMs =
      typeof opts.delayMs === 'function'
        ? opts.delayMs()
        : opts.delayMs;
    return Math.max(0, delayMs ?? 500);
  }

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

  async function hidePreview(): Promise<void> {
    try {
      await clipboardApi.hidePreview();
    } catch (error) {
      opts.onError?.(error);
    }
  }

  async function showPreview(target: HoverPreviewTarget): Promise<void> {
    try {
      if (target.kind === 'image') {
        await clipboardApi.showImagePreview(target.id);
      } else {
        await clipboardApi.showTextPreview(target.id);
      }
    } catch (error) {
      opts.onError?.(error);
    }
  }

  function onItemChange(item: ClipboardItem | null) {
    clearTimers();

    const target = resolveHoverPreviewTarget(item);
    if (!target) {
      hideTimer = window.setTimeout(() => {
        void hidePreview();
        hideTimer = null;
      }, hideDelayMs);
      return;
    }

    void hidePreview();
    showTimer = window.setTimeout(() => {
      void showPreview(target);
      showTimer = null;
    }, resolveDelayMs());
  }

  function onLeave() {
    clearTimers();
    hideTimer = window.setTimeout(() => {
      void hidePreview();
      hideTimer = null;
    }, hideDelayMs);
  }

  function hideNow() {
    clearTimers();
    void hidePreview();
  }

  onBeforeUnmount(() => {
    clearTimers();
    void hidePreview();
  });

  return {
    onItemChange,
    onLeave,
    hideNow,
  };
}
