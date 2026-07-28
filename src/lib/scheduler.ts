import { appStore, addLog } from './store';
import { scanNow, getConfig, type ScanResult } from './tauri';
import { i18n } from '../i18n';

// Helper to access translation function outside components
const t = (key: string, args?: any) => {
    return i18n.global.t(key, args);
};

let timer: ReturnType<typeof setInterval> | null = null;

/**
 * How long to wait before retrying a cycle that stood aside for the copy queue.
 * Short enough that postponed candidates do not sit until the next interval tick,
 * long enough that a busy queue is not polled aggressively.
 */
const DEFERRED_SCAN_RETRY_MS = 60 * 1000;
let deferredRetryTimer: ReturnType<typeof setTimeout> | null = null;

function clearDeferredRetry() {
    if (deferredRetryTimer) {
        clearTimeout(deferredRetryTimer);
        deferredRetryTimer = null;
    }
}

/** Re-run a postponed cycle once the copy queue has had time to drain. */
function scheduleDeferredRetry() {
    if (deferredRetryTimer) return;
    deferredRetryTimer = setTimeout(() => {
        deferredRetryTimer = null;
        if (!appStore.isRunning) return;
        void executeScan();
    }, DEFERRED_SCAN_RETRY_MS);
}

export async function executeScan() {
    addLog(t('console.running'), 'info');
    try {
        const result: ScanResult = await scanNow();
        addLog(t('console.scanComplete', { scanned: result.scanned_paths, found: result.found_folders.length, copied: result.copied_folders.length }), 'success');

        if (result.found_folders.length > 0) {
            result.found_folders.forEach(f => addLog(`Checked: ${f}`, 'info'));
        }
        if (result.copied_folders.length > 0) {
            result.copied_folders.forEach(f => addLog(`Copied new files: ${f}`, 'success'));
        }
        if (result.errors.length > 0) {
            result.errors.forEach(e => addLog(`Error: ${e}`, 'error'));
        }
        if (result.deferred_for_copy_queue) {
            addLog(t('console.scanDeferredForQueue'), 'info');
            scheduleDeferredRetry();
        } else {
            clearDeferredRetry();
        }
    } catch (e) {
        const errMsg = String(e);
        if (errMsg.includes('already in progress') || errMsg.includes('queue already in progress')) {
            // Previous scan/deploy still running — this is normal, just skip quietly
            addLog(t('console.scanSkipped'), 'info');
            scheduleDeferredRetry();
        } else {
            addLog(t('console.scanFailed', { error: e }), 'error');
        }
    } finally {
        appStore.progress = null; // Ensure progress is cleared when scan finishes
    }
}

function updateNextRunTime(delayMs: number) {
    const next = new Date(Date.now() + delayMs);
    appStore.nextRunTime = next.toLocaleTimeString();
}

export async function startScheduler(isRestart = false) {
    if (appStore.isRunning && !isRestart) return;
    
    const config = await getConfig();
    if (!config) {
        addLog(t('console.failedLoadConfig', { error: 'Config is null' }), 'error');
        return;
    }

    // Clear existing timer if any
    if (timer) {
        clearInterval(timer);
        timer = null;
    }

    appStore.isRunning = true;
    
    if (!isRestart) {
        const msg = t('console.schedulerStarted', { interval: config.interval_minutes });
        addLog(msg, 'info');
        
        // Execute first scan immediately
        executeScan();
    }
    
    const intervalMs = config.interval_minutes * 60 * 1000;
    updateNextRunTime(intervalMs);
    
    timer = setInterval(() => {
        executeScan();
        updateNextRunTime(intervalMs);
    }, intervalMs);
}

export function stopScheduler() {
    appStore.isRunning = false;
    clearDeferredRetry();
    if (timer) {
        clearInterval(timer);
        timer = null;
    }
    appStore.nextRunTime = '-';
    const msg = t('console.schedulerStopped');
    addLog(msg, 'info');
}

/** Called after saving config while scheduler is running — reloads interval without re-triggering an immediate scan */
export async function restartSchedulerInterval() {
    if (!appStore.isRunning) return;
    await startScheduler(true);
}
