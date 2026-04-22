export const CLIPBOARD_PANEL_DRAG_SKIP_SELECTOR = 'button, input, a, [data-no-drag]';
export const CLIPBOARD_PANEL_USE_NATIVE_DRAG_REGION = false;

type DragTargetLike = EventTarget | {
  closest?: (selector: string) => unknown;
} | null | undefined;

export function shouldStartClipboardPanelDrag(event: {
  button: number;
  target: DragTargetLike;
}): boolean {
  if (event.button !== 0) {
    return false;
  }

  const maybeElementTarget = event.target as { closest?: (selector: string) => unknown } | null;
  return !maybeElementTarget?.closest?.(CLIPBOARD_PANEL_DRAG_SKIP_SELECTOR);
}
