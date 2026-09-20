import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createApp, nextTick, ref, type App } from 'vue';
import { createI18n } from 'vue-i18n';
import { createDefaultState } from '@/lib/paperTodo';
import PaperTodoLauncherPage from './PaperTodoLauncherPage.vue';

const native = vi.hoisted(() => ({
  drag: vi.fn(async () => false),
  expand: vi.fn(async () => undefined),
  finish: vi.fn(async () => undefined),
}));
vi.mock('@/lib/paperTodo', async (original) => ({
  ...await original<typeof import('@/lib/paperTodo')>(),
  dragPaperLauncher: native.drag,
  setPaperLauncherExpanded: native.expand,
  finishPaperLauncherTransition: native.finish,
}));
const state = createDefaultState();
const settings = ref(state.settings);
vi.mock('@/composables/usePaperTodo', () => ({
  usePaperTodo: () => ({
    settings,
    papers: ref([]),
    error: ref(null),
    initialize: vi.fn(async () => undefined),
    refreshFromDisk: vi.fn(async () => undefined),
  }),
}));

let app: App;
let root: HTMLDivElement;
let capsule: HTMLButtonElement;
async function flush() {
  for (let i = 0; i < 30; i += 1) await nextTick();
}
function pointer(type: string, x = 10, y = 10, target: Element = capsule) {
  const event = new MouseEvent(type, { bubbles: true, cancelable: true, clientX: x, clientY: y, button: 0 });
  Object.defineProperty(event, 'pointerId', { value: 1 });
  target.dispatchEvent(event);
}
async function expand() {
  pointer('pointerdown');
  pointer('pointerup');
  await flush();
  expect(capsule.getAttribute('aria-expanded')).toBe('true');
}

beforeEach(async () => {
  vi.clearAllMocks();
  settings.value = { ...state.settings, autoCollapseLauncher: true };
  vi.stubGlobal('matchMedia', () => ({ matches: false }));
  vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => { callback(0); return 1; });
  HTMLElement.prototype.setPointerCapture = vi.fn();
  root = document.createElement('div');
  document.body.append(root);
  app = createApp(PaperTodoLauncherPage);
  app.use(createI18n({ legacy: false, locale: 'en', missingWarn: false, fallbackWarn: false }));
  app.mount(root);
  await flush();
  capsule = root.querySelector('.launcher-drag-handle')!;
  native.expand.mockClear();
});
afterEach(() => {
  app.unmount();
  root.remove();
  vi.unstubAllGlobals();
});

describe('launcher pointer interactions', () => {
  it('clicks without entering the native drag loop', async () => {
    await expand();
    expect(native.drag).not.toHaveBeenCalled();
  });

  it('never expands after a drag even if native IPC reports no movement', async () => {
    pointer('pointerdown');
    pointer('pointermove', 30, 20);
    pointer('pointerup', 30, 20);
    await flush();
    expect(native.drag).toHaveBeenCalledWith(10, 10);
    expect(native.expand).not.toHaveBeenCalled();
    expect(capsule.getAttribute('aria-expanded')).toBe('false');
  });

  it('recognizes a fast drag from its release position', async () => {
    pointer('pointerdown');
    pointer('pointerup', 30, 10);
    await flush();
    expect(native.drag).toHaveBeenCalledOnce();
    expect(native.expand).not.toHaveBeenCalled();
  });

  it('does not click after pointer cancellation', async () => {
    pointer('pointerdown');
    pointer('pointercancel');
    pointer('pointerup');
    await flush();
    expect(native.expand).not.toHaveBeenCalled();
  });

  it('collapses on focus loss even within the resize settle period', async () => {
    await expand();
    window.dispatchEvent(new Event('blur'));
    await flush();
    expect(capsule.getAttribute('aria-expanded')).toBe('false');
  });

  it('keeps action buttons open but collapses on transparent blank space', async () => {
    await expand();
    pointer('pointerdown', 0, 0, root.querySelector('.launcher-create-button')!);
    await flush();
    expect(capsule.getAttribute('aria-expanded')).toBe('true');
    pointer('pointerdown', 0, 0, document.body);
    await flush();
    expect(capsule.getAttribute('aria-expanded')).toBe('false');
  });

  it('respects the auto-collapse setting', async () => {
    settings.value.autoCollapseLauncher = false;
    await expand();
    window.dispatchEvent(new Event('blur'));
    pointer('pointerdown', 0, 0, document.body);
    await flush();
    expect(capsule.getAttribute('aria-expanded')).toBe('true');
  });
});
