import { appStore, addLog } from './store';
import { scanNow, addSystemEvent, getConfig, type ScanResult } from './tauri';
import { i18n } from '../i18n';

// Helper to access translation function outside components
const t = (key: string, args?: any) => {
    // @ts-ignore
    return i18n.global.t(key, args);
};

let timer: ReturnType<typeof setInterval> | null = null;

export async function executeScan() {
    if (appStore.isManualDeploying) {
        addLog(t('console.scanFailed', { error: 'Manual deploy in progress' }), 'error');
        return;
    }

    addLog(t('console.running'), 'info'); 
    try {
        const result: ScanResult = await scanNow();
        addLog(t('console.scanComplete', { scanned: result.scanned_paths, found: result.found_folders.length, copied: result.copied_folders.length }), 'success');
        
        if (result.found_folders.length > 0) {
            result.found_folders.forEach(f => addLog(`Found candidate: ${f}`, 'info'));
        }
        if (result.copied_folders.length > 0) {
            result.copied_folders.forEach(f => addLog(`Successfully copied: ${f}`, 'success'));
        }
        if (result.errors.length > 0) {
            result.errors.forEach(e => addLog(`Error: ${e}`, 'error'));
        }
    } catch (e) {
        addLog(t('console.scanFailed', { error: e }), 'error');
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
        addSystemEvent('SCHEDULER_START', msg);
        
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
    if (timer) {
        clearInterval(timer);
        timer = null;
    }
    appStore.nextRunTime = '-';
    const msg = t('console.schedulerStopped');
    addLog(msg, 'info');
    addSystemEvent('SCHEDULER_STOP', msg);
}
