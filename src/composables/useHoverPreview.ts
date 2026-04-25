import { onBeforeUnmount } from 'vue';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

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
  onDebug?: (message: string) => void;
}

export function useHoverPreview(opts: HoverPreviewOptions = {}) {
  const hideDelayMs = opts.hideDelayMs ?? 200;

  let showTimer: number | null = null;
  let hideTimer: number | null = null;
  let nextPreviewToken = 0;
  let activePreviewToken: number | null = null;
  let previewHovered = false;
  let unlistenEnter: UnlistenFn | null = null;
  let unlistenLeave: UnlistenFn | null = null;

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

  function clearHideTimer() {
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

  function scheduleHide(reason: string) {
    const token = activePreviewToken;
    clearHideTimer();
    opts.onDebug?.(reason);
    hideTimer = window.setTimeout(() => {
      hideTimer = null;
      if (previewHovered) return;
      if (token !== activePreviewToken) return;
      void hidePreview(token);
    }, hideDelayMs);
  }

  async function hidePreview(token: number | null = null): Promise<void> {
    try {
      opts.onDebug?.('hide-preview:start');
      await clipboardApi.hidePreview(token);
      if (token === null || activePreviewToken === token) {
        activePreviewToken = null;
      }
      opts.onDebug?.('hide-preview:done');
    } catch (error) {
      opts.onDebug?.(`hide-preview:error ${String(error)}`);
      opts.onError?.(error);
    }
  }

  async function showPreview(target: HoverPreviewTarget, token: number): Promise<void> {
    if (!isActivePreviewToken(token)) return;

    try {
      opts.onDebug?.(`show-preview:start kind=${target.kind} id=${target.id}`);
      if (target.kind === 'image') {
        await clipboardApi.showImagePreview(target.id, token);
      } else {
        await clipboardApi.showTextPreview(target.id, token);
      }
      if (!isActivePreviewToken(token)) {
        await hidePreview(token);
      }
      opts.onDebug?.(`show-preview:done kind=${target.kind} id=${target.id}`);
    } catch (error) {
      opts.onDebug?.(`show-preview:error kind=${target.kind} id=${target.id} error=${String(error)}`);
      opts.onError?.(error);
    }
  }

  function onItemChange(item: ClipboardItem | null) {
    clearTimers();

    const target = resolveHoverPreviewTarget(item);
    if (!target) {
      scheduleHide(`schedule-hide item=${item?.id ?? 'none'}`);
      return;
    }

    previewHovered = false;
    const token = startPreviewToken();
    opts.onDebug?.(`schedule-show kind=${target.kind} id=${target.id} delay=${resolveDelayMs()}`);
    showTimer = window.setTimeout(() => {
      void showPreview(target, token);
      showTimer = null;
    }, resolveDelayMs());
  }

  function onLeave() {
    if (showTimer !== null) {
      clearTimeout(showTimer);
      showTimer = null;
    }
    scheduleHide('schedule-hide leave');
  }

  function hideNow() {
    clearTimers();
    previewHovered = false;
    const token = consumeActivePreviewToken();
    void hidePreview(token);
  }

  void (async () => {
    try {
      unlistenEnter = await listen('clipboard-preview-mouse-enter', () => {
        previewHovered = true;
        opts.onDebug?.('preview:mouse-enter');
        clearHideTimer();
      });
      unlistenLeave = await listen('clipboard-preview-mouse-leave', () => {
        previewHovered = false;
        scheduleHide('preview:mouse-leave');
      });
    } catch (error) {
      opts.onDebug?.(`preview-hover-listen:error ${String(error)}`);
    }
  })();

  onBeforeUnmount(() => {
    clearTimers();
    previewHovered = false;
    const token = consumeActivePreviewToken();
    void hidePreview(token);
    unlistenEnter?.();
    unlistenLeave?.();
    unlistenEnter = null;
    unlistenLeave = null;
  });

  return {
    onItemChange,
    onLeave,
    hideNow,
  };
}
