import { readonly, ref } from 'vue';

export type ToastTone = 'info' | 'success' | 'error' | 'warning';

export interface ToastAction {
  label: string;
  onClick: () => void;
}

export interface Toast {
  id: string;
  message: string;
  tone: ToastTone;
  ttlMs: number;
  action?: ToastAction;
}

export interface PushToastOptions {
  ttlMs?: number;
  action?: ToastAction;
}

const DEFAULT_TTL_MS = 3000;

const toasts = ref<Toast[]>([]);
const timers = new Map<string, ReturnType<typeof setTimeout>>();

let idCounter = 0;
function makeId(): string {
  // Simple monotonically-increasing id is enough — the queue lives in-process
  // and the keys never need to be reconciled with anything else.
  idCounter += 1;
  return `toast-${Date.now().toString(36)}-${idCounter.toString(36)}`;
}

function clearTimer(id: string) {
  const handle = timers.get(id);
  if (handle !== undefined) {
    clearTimeout(handle);
    timers.delete(id);
  }
}

function scheduleAutoDismiss(toast: Toast) {
  if (toast.ttlMs <= 0) return;
  const handle = setTimeout(() => {
    timers.delete(toast.id);
    dismissToast(toast.id);
  }, toast.ttlMs);
  timers.set(toast.id, handle);
}

export function pushToast(
  message: string,
  tone: ToastTone = 'info',
  opts: PushToastOptions = {},
): string {
  const ttlMs = typeof opts.ttlMs === 'number' ? opts.ttlMs : DEFAULT_TTL_MS;
  const toast: Toast = {
    id: makeId(),
    message,
    tone,
    ttlMs,
    action: opts.action,
  };
  toasts.value.push(toast);
  scheduleAutoDismiss(toast);
  return toast.id;
}

export function dismissToast(id: string): void {
  clearTimer(id);
  const next = toasts.value.filter((toast) => toast.id !== id);
  if (next.length !== toasts.value.length) {
    toasts.value = next;
  }
}

export function clearToasts(): void {
  for (const id of [...timers.keys()]) {
    clearTimer(id);
  }
  toasts.value = [];
}

export function useToast() {
  return {
    toasts: readonly(toasts),
    pushToast,
    dismissToast,
    clearToasts,
  };
}
