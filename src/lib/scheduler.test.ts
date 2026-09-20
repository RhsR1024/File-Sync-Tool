import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('./tauri', () => ({ scanNow: vi.fn(), getConfig: vi.fn() }));
vi.mock('../i18n', () => ({ i18n: { global: { t: (key: string) => key } } }));

import { scanNow, getConfig, type ScanResult } from './tauri';
import { appStore } from './store';
import { executeScan, restartSchedulerInterval, startScheduler, stopScheduler } from './scheduler';

const result: ScanResult = {
  scanned_paths: 1, found_folders: [], copied_folders: [], errors: [], deferred_for_copy_queue: false,
};
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>(done => { resolve = done; });
  return { promise, resolve };
}

describe('scan scheduling lifecycle', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2026-09-20T00:00:00Z'));
    vi.mocked(scanNow).mockReset().mockResolvedValue(result);
    vi.mocked(getConfig).mockReset().mockResolvedValue({ interval_minutes: 5 } as Awaited<ReturnType<typeof getConfig>>);
    appStore.progress = null;
    appStore.scanWaitingForQueue = false;
    appStore.lastScanError = '';
  });
  afterEach(() => {
    stopScheduler();
    vi.useRealTimers();
  });

  it('never overlaps a long scan and schedules the next scan after completion', async () => {
    const pending = deferred<ScanResult>();
    vi.mocked(scanNow).mockReturnValueOnce(pending.promise);
    await startScheduler();
    const scan = executeScan();
    expect(executeScan()).toBe(scan);
    await vi.advanceTimersByTimeAsync(14 * 24 * 60 * 60 * 1000);
    expect(scanNow).toHaveBeenCalledTimes(1);
    expect(appStore.scanInProgress).toBe(true);
    expect(appStore.nextRunAt).toBeNull();
    pending.resolve(result);
    await scan;
    expect(appStore.scanInProgress).toBe(false);
    expect(appStore.nextRunAt).toBe(Date.now() + 300_000);
    await vi.advanceTimersByTimeAsync(300_000);
    expect(scanNow).toHaveBeenCalledTimes(2);
    expect(vi.getTimerCount()).toBe(1);
  });

  it('allows scan now while waiting and replaces the scheduled deadline', async () => {
    await startScheduler();
    await executeScan();
    await vi.advanceTimersByTimeAsync(60_000);
    await executeScan();
    expect(scanNow).toHaveBeenCalledTimes(2);
    expect(appStore.nextRunAt).toBe(Date.now() + 300_000);
    expect(vi.getTimerCount()).toBe(1);
  });

  it('uses one deferred retry and does not clear another copy progress', async () => {
    vi.mocked(scanNow).mockRejectedValueOnce('Manual copy queue already in progress');
    appStore.progress = { folder: 'manual', percentage: 20, copied: 2, total: 10, speed: 1, eta: 8, elapsed: 2, source: 'manual' };
    await startScheduler();
    await executeScan();
    expect(appStore.scanWaitingForQueue).toBe(true);
    expect(appStore.progress?.folder).toBe('manual');
    expect(appStore.nextRunAt).toBe(Date.now() + 60_000);
    expect(vi.getTimerCount()).toBe(1);
    await vi.advanceTimersByTimeAsync(60_000);
    expect(scanNow).toHaveBeenCalledTimes(2);
    expect(appStore.scanWaitingForQueue).toBe(false);
  });

  it('does not resurrect a stopped scheduler when a deferred scan completes', async () => {
    const pending = deferred<ScanResult>();
    vi.mocked(scanNow).mockReturnValueOnce(pending.promise);
    await startScheduler();
    const scan = executeScan();
    stopScheduler();
    pending.resolve({ ...result, deferred_for_copy_queue: true });
    await scan;
    expect(appStore.isRunning).toBe(false);
    expect(appStore.nextRunAt).toBeNull();
    expect(vi.getTimerCount()).toBe(0);
  });

  it('invalidates an in-flight start when stopped and coalesces double start', async () => {
    const config = deferred<Awaited<ReturnType<typeof getConfig>>>();
    vi.mocked(getConfig).mockReturnValueOnce(config.promise);
    const start = startScheduler();
    await startScheduler();
    expect(getConfig).toHaveBeenCalledTimes(1);
    stopScheduler();
    config.resolve({ interval_minutes: 1 } as Awaited<ReturnType<typeof getConfig>>);
    await start;
    expect(appStore.isRunning).toBe(false);
    expect(scanNow).not.toHaveBeenCalled();
    expect(vi.getTimerCount()).toBe(0);
  });

  it('applies a new interval after the current scan, without overlapping', async () => {
    const pending = deferred<ScanResult>();
    vi.mocked(scanNow).mockReturnValueOnce(pending.promise);
    await startScheduler();
    const scan = executeScan();
    vi.mocked(getConfig).mockResolvedValue({ interval_minutes: 2 } as Awaited<ReturnType<typeof getConfig>>);
    await restartSchedulerInterval();
    expect(scanNow).toHaveBeenCalledTimes(1);
    pending.resolve(result);
    await scan;
    expect(appStore.nextRunAt).toBe(Date.now() + 120_000);
  });

  it('recovers after a failed scan and exposes errors until the next attempt', async () => {
    vi.mocked(scanNow).mockRejectedValueOnce('Share unavailable');
    await startScheduler();
    await executeScan();
    expect(appStore.lastScanError).toBe('Share unavailable');
    expect(appStore.scanInProgress).toBe(false);
    await vi.advanceTimersByTimeAsync(300_000);
    expect(appStore.lastScanError).toBe('');
    expect(scanNow).toHaveBeenCalledTimes(2);
  });

  it('adopts the existing scan after stop/start without creating a second scan', async () => {
    const pending = deferred<ScanResult>();
    vi.mocked(scanNow).mockReturnValueOnce(pending.promise);
    await startScheduler();
    const scan = executeScan();
    stopScheduler();
    await startScheduler();
    expect(scanNow).toHaveBeenCalledTimes(1);
    expect(vi.getTimerCount()).toBe(0);
    pending.resolve(result);
    await scan;
    expect(appStore.isRunning).toBe(true);
    expect(appStore.nextRunAt).toBe(Date.now() + 300_000);
    expect(vi.getTimerCount()).toBe(1);
  });

  it('does not start without a valid configuration', async () => {
    vi.mocked(getConfig).mockResolvedValueOnce(null);
    await startScheduler();
    expect(appStore.isRunning).toBe(false);
    expect(scanNow).not.toHaveBeenCalled();
    expect(vi.getTimerCount()).toBe(0);
  });

  it('clears finished scheduled progress, including a copy that ended below 100 percent', async () => {
    appStore.progress = { folder: 'scheduled', percentage: 20, copied: 2, total: 10, speed: 1, eta: 8, elapsed: 2, source: 'scheduled' };
    await executeScan();
    expect(appStore.progress).toBeNull();
    expect(appStore.nextRunAt).toBeNull();
  });
});
