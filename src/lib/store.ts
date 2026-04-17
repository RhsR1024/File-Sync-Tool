import { reactive } from 'vue';

export interface ManualCopyFormState {
    sourcePath: string;
    targetRootPath: string;
}

export interface LogEntry {
    time: string;
    msg: string;
    type: 'info' | 'error' | 'success' | 'command';
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
    source?: 'manual' | 'scheduled';
}

export interface ToolRuntimeState {
    screenShare: boolean;
    fileShare: boolean;
}

export const appStore = reactive({
    logs: [] as LogEntry[],
    progress: null as ProgressState | null,
    isRunning: false,
    nextRunTime: '-',
    isManualDeploying: false,
    manualDeployMsg: '',
    maxLogLines: 200,
    nowTick: Date.now(),
    toolRuntime: {
        screenShare: false,
        fileShare: false,
    } as ToolRuntimeState,
});

let liveTickTimer: ReturnType<typeof setInterval> | null = null;

export function startLiveTicker() {
    if (liveTickTimer) return;
    appStore.nowTick = Date.now();
    liveTickTimer = setInterval(() => {
        appStore.nowTick = Date.now();
    }, 1000);
}

export function stopLiveTicker() {
    if (liveTickTimer) {
        clearInterval(liveTickTimer);
        liveTickTimer = null;
    }
}

export function setToolRuntime<K extends keyof ToolRuntimeState>(tool: K, active: boolean) {
    appStore.toolRuntime[tool] = active;
}

export function addLog(msg: string, type: 'info' | 'error' | 'success' | 'command' = 'info') {
    const time = new Date().toLocaleTimeString();
    appStore.logs.push({ time, msg, type });
    while (appStore.logs.length > appStore.maxLogLines) {
        appStore.logs.shift();
    }
}

const MANUAL_COPY_STORAGE_KEY = 'manualCopy_form_state';

function loadManualCopyFormFromStorage(): ManualCopyFormState {
    try {
        const stored = localStorage.getItem(MANUAL_COPY_STORAGE_KEY);
        if (stored) {
            return JSON.parse(stored);
        }
    } catch {
        // Ignore parse errors and use default state
    }
    return {
        sourcePath: '',
        targetRootPath: '',
    };
}

export const manualCopyFormState = reactive<ManualCopyFormState>(loadManualCopyFormFromStorage());

export function updateManualCopyForm(state: Partial<ManualCopyFormState>): void {
    Object.assign(manualCopyFormState, state);
    localStorage.setItem(MANUAL_COPY_STORAGE_KEY, JSON.stringify(manualCopyFormState));
}

export function getManualCopyForm(): ManualCopyFormState {
    return {
        sourcePath: manualCopyFormState.sourcePath,
        targetRootPath: manualCopyFormState.targetRootPath,
    };
}

export function clearManualCopyForm(): void {
    manualCopyFormState.sourcePath = '';
    manualCopyFormState.targetRootPath = '';
    localStorage.removeItem(MANUAL_COPY_STORAGE_KEY);
}
