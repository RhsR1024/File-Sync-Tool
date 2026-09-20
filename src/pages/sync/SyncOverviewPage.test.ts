import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { createApp, nextTick, type App } from 'vue';
import { createI18n } from 'vue-i18n';
import SyncOverviewPage from './SyncOverviewPage.vue';
import { appStore } from '@/lib/store';
import { executeScan } from '@/lib/scheduler';

vi.mock('@/lib/scheduler', () => ({ executeScan: vi.fn(), startScheduler: vi.fn(), stopScheduler: vi.fn() }));
vi.mock('@/lib/tauri', async original => ({
  ...await original<typeof import('@/lib/tauri')>(),
  getConfig: vi.fn(async () => ({ sync_retention_days: 5 })),
}));
vi.mock('@/lib/taskStateStore', () => ({ taskStateStore: {
  groups: [], selectedTaskGroupId: null, selectedGroupDetail: null, taskLogsByGroup: {}, isHydrated: true,
} }));
vi.mock('@/components/Empty.vue', () => ({ default: { render: () => null } }));
vi.mock('@/components/LoadingSkeleton.vue', () => ({ default: { render: () => null } }));
vi.mock('@/components/ManualCopyModal.vue', () => ({ default: { render: () => null } }));
vi.mock('@/components/tasks/TaskGroupsTable.vue', () => ({ default: { render: () => null } }));
vi.mock('@/components/tasks/TaskGroupDetailPanel.vue', () => ({ default: { render: () => null } }));
vi.mock('@/components/AppConfirmDialog.vue', () => ({ default: { render: () => null } }));

let app: App;
let root: HTMLDivElement;
beforeEach(async () => {
  vi.clearAllMocks();
  Object.assign(appStore, {
    isRunning: true, scanInProgress: false, scanStartedAt: null,
    scanWaitingForQueue: false, lastScanError: '', nowTick: 1_000_000, nextRunAt: 1_060_000,
  });
  root = document.createElement('div');
  document.body.append(root);
  app = createApp(SyncOverviewPage);
  app.use(createI18n({ legacy: false, locale: 'en', missingWarn: false, fallbackWarn: false, messages: { en: { console: {
    scanCountdown: 'In {seconds}s', scanElapsed: 'Elapsed: {seconds}s',
    scanWaiting: 'Waiting for next scan', scanning: 'Scanning / processing',
  } } } }));
  app.mount(root);
  await nextTick();
});
afterEach(() => { app.unmount(); root.remove(); });

it('allows an immediate scan with the scheduler enabled and updates the countdown', async () => {
  const button = root.querySelector<HTMLButtonElement>('[aria-label="console.scanNow"]')!;
  expect(button.disabled).toBe(false);
  expect(root.textContent).toContain('Waiting for next scan');
  expect(root.textContent).toContain('In 60s');
  appStore.nowTick += 1_000;
  await nextTick();
  expect(root.textContent).toContain('In 59s');
  button.click();
  expect(executeScan).toHaveBeenCalledTimes(1);
});

it('shows elapsed time and disables repeated clicks only while a scan is pending', async () => {
  appStore.scanInProgress = true;
  appStore.scanStartedAt = appStore.nowTick;
  appStore.nextRunAt = null;
  await nextTick();
  const button = root.querySelector<HTMLButtonElement>('[aria-label="console.scanNow"]')!;
  expect(button.disabled).toBe(true);
  expect(root.textContent).toContain('Scanning / processing');
  appStore.nowTick += 2_000;
  await nextTick();
  expect(root.textContent).toContain('Elapsed: 2s');
  appStore.scanInProgress = false;
  await nextTick();
  expect(button.disabled).toBe(false);
});
