import { appStore, addLog } from './store';
import { scanNow, getConfig } from './tauri';
import { i18n } from '../i18n';

const t = (key: string, args?: Record<string, unknown>) => i18n.global.t(key, args ?? {});
const DEFERRED_SCAN_RETRY_MS = 60 * 1000;
let timer: ReturnType<typeof setTimeout> | null = null;
let scanInFlight: Promise<void> | null = null;
let intervalMs = DEFERRED_SCAN_RETRY_MS;
let startRevision = 0;
let startPending = false;

function clearTimer() {
    if (timer !== null) clearTimeout(timer);
    timer = null;
    appStore.nextRunAt = null;
}

function scheduleNextScan(delayMs: number) {
    clearTimer();
    if (!appStore.isRunning || scanInFlight) return;
    appStore.nextRunAt = Date.now() + delayMs;
    timer = setTimeout(() => {
        timer = null;
        void executeScan();
    }, delayMs);
}

/** Scheduled, deferred and manual scans share one in-flight operation. */
export function executeScan(): Promise<void> {
    if (scanInFlight) return scanInFlight;
    clearTimer();
    appStore.scanInProgress = true;
    appStore.scanStartedAt = Date.now();
    appStore.scanWaitingForQueue = false;
    appStore.lastScanError = '';
    addLog(t('console.scanning'), 'info');

    let retryDeferred = false;
    let completedScan = false;
    // The microtask also ensures the guard is installed before scanNow can throw.
    scanInFlight = Promise.resolve().then(async () => {
        try {
            const result = await scanNow();
            completedScan = true;
            addLog(t('console.scanComplete', { scanned: result.scanned_paths, found: result.found_folders.length, copied: result.copied_folders.length }), 'success');
            result.found_folders.forEach(f => addLog(`Checked: ${f}`, 'info'));
            result.copied_folders.forEach(f => addLog(`Copied new files: ${f}`, 'success'));
            result.errors.forEach(e => addLog(`Error: ${e}`, 'error'));
            appStore.lastScanError = result.errors.join('\n');
            retryDeferred = result.deferred_for_copy_queue;
            if (retryDeferred) addLog(t('console.scanDeferredForQueue'), 'info');
        } catch (error) {
            const message = String(error);
            if (message.includes('already in progress')) {
                addLog(t('console.scanSkipped'), 'info');
                retryDeferred = true;
            } else {
                appStore.lastScanError = message;
                addLog(t('console.scanFailed', { error: message }), 'error');
            }
        } finally {
            scanInFlight = null;
            appStore.scanInProgress = false;
            appStore.scanStartedAt = null;
            appStore.scanWaitingForQueue = retryDeferred;
            // Progress belongs to copy events; a busy scan must not erase the
            // progress of a manual copy that currently owns the executor.
            if (completedScan && appStore.progress?.source === 'scheduled') {
                appStore.progress = null;
            }
            scheduleNextScan(retryDeferred ? DEFERRED_SCAN_RETRY_MS : intervalMs);
        }
    });
    return scanInFlight;
}

export async function startScheduler(isRestart = false) {
    if (!isRestart && (appStore.isRunning || startPending)) return;
    const revision = ++startRevision;
    startPending = true;
    try {
        const config = await getConfig();
        // Stop/restart during the configuration request invalidates this start.
        if (revision !== startRevision) return;
        if (!config) throw new Error('Config is null');
        const minutes = Number(config.interval_minutes);
        intervalMs = Number.isFinite(minutes) && minutes > 0
            ? Math.min(minutes * 60 * 1000, 2_147_483_647)
            : DEFERRED_SCAN_RETRY_MS;
        appStore.isRunning = true;
        clearTimer();
        if (!isRestart) {
            addLog(t('console.schedulerStarted', { interval: intervalMs / 60_000 }), 'info');
            // Restarting while a scan is still running adopts that operation;
            // it must finish before the new interval can begin.
            void executeScan();
        } else {
            scheduleNextScan(appStore.scanWaitingForQueue ? DEFERRED_SCAN_RETRY_MS : intervalMs);
        }
    } catch (error) {
        if (revision === startRevision) {
            addLog(t('console.failedLoadConfig', { error: String(error) }), 'error');
        }
    } finally {
        if (revision === startRevision) startPending = false;
    }
}

export function stopScheduler() {
    ++startRevision;
    startPending = false;
    appStore.isRunning = false;
    clearTimer();
    addLog(t('console.schedulerStopped'), 'info');
}

/** Reload the interval without starting another scan. */
export async function restartSchedulerInterval() {
    if (appStore.isRunning) await startScheduler(true);
}
