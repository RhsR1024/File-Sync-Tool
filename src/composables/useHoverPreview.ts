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
  let nextPreviewToken = 0;
  let activePreviewToken: number | null = null;

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

  function startPreviewToken(): number {
    nextPreviewToken += 1;
    activePreviewToken = nextPreviewToken;
    return activePreviewToken;
  }

  function consumeActivePreviewToken(): number | null {
    const token = activePreviewToken;
    activePreviewToken = null;
    return token;
  }

  function isActivePreviewToken(token: number): boolean {
    return activePreviewToken === token;
  }

  async function hidePreview(token: number | null = null): Promise<void> {
    try {
      await clipboardApi.hidePreview(token);
    } catch (error) {
      opts.onError?.(error);
    }
  }

  async function showPreview(target: HoverPreviewTarget, token: number): Promise<void> {
    if (!isActivePreviewToken(token)) return;

    try {
      if (target.kind === 'image') {
        await clipboardApi.showImagePreview(target.id, token);
      } else {
        await clipboardApi.showTextPreview(target.id, token);
      }
      if (!isActivePreviewToken(token)) {
        await hidePreview(token);
      }
    } catch (error) {
      opts.onError?.(error);
    }
  }

  function onItemChange(item: ClipboardItem | null) {
    clearTimers();

    const target = resolveHoverPreviewTarget(item);
    if (!target) {
      const token = consumeActivePreviewToken();
      hideTimer = window.setTimeout(() => {
        void hidePreview(token);
        hideTimer = null;
      }, hideDelayMs);
      return;
    }

    const token = startPreviewToken();
    showTimer = window.setTimeout(() => {
      void showPreview(target, token);
      showTimer = null;
    }, resolveDelayMs());
  }

  function onLeave() {
    clearTimers();
    const token = consumeActivePreviewToken();
    hideTimer = window.setTimeout(() => {
      void hidePreview(token);
      hideTimer = null;
    }, hideDelayMs);
  }

  function hideNow() {
    clearTimers();
    const token = consumeActivePreviewToken();
    void hidePreview(token);
  }

  onBeforeUnmount(() => {
    clearTimers();
    const token = consumeActivePreviewToken();
    void hidePreview(token);
  });

  return {
    onItemChange,
    onLeave,
    hideNow,
  };
}
