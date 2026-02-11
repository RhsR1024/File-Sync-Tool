import { reactive } from 'vue';

export interface LogEntry {
    time: string;
    msg: string;
    type: 'info' | 'error' | 'success';
}

export interface ProgressState {
    folder: string;
    percentage: number;
    copied: number;
    total: number;
    speed: number;
    eta: number;
    elapsed: number;
    localPath?: string;
    remotePath?: string;
}

export const appStore = reactive({
    // Console Logs
    logs: [] as LogEntry[],
    
    // Scan/Copy Progress
    progress: null as ProgressState | null,
    
    // Scheduler Status
    isRunning: false,
    nextRunTime: '-',
    
    // Manual Deploy State
    isManualDeploying: false,
    manualDeployMsg: '',
});

export function addLog(msg: string, type: 'info' | 'error' | 'success' = 'info') {
    const time = new Date().toLocaleTimeString();
    appStore.logs.unshift({ time, msg, type });
    if (appStore.logs.length > 1000) appStore.logs.pop();
}
