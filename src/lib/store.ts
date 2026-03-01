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

export type TaskRecordPhase = 'copying' | 'deploying' | 'completed' | 'cancelled';

export interface TaskRecord {
    id: string;
    startTime: string;   // full datetime string
    folder: string;      // source folder / operation name
    localPath: string;
    // local copy
    copyPercentage: number;
    copyCompleted: boolean;
    copyTotal: number;
    // remote deploy
    hasRemote: boolean;
    remotePath?: string;
    deployPercentage: number;
    deployCompleted: boolean;
    // live metrics (only meaningful while active)
    speed: number;
    copied: number;
    total: number;
    phase: TaskRecordPhase;
}

export const appStore = reactive({
    // Console Logs
    logs: [] as LogEntry[],

    // Persistent task records shown in console
    taskRecords: [] as TaskRecord[],

    // Scan/Copy Progress (for TaskStatusPage live table)
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

/** Update or create a TaskRecord from a copy-progress event payload */
export function upsertTaskRecord(payload: {
    folder: string;
    total_bytes: number;
    copied_bytes: number;
    percentage: number;
    speed: number;
    local_path: string;
    remote_path: string;
}) {
    // During local copy:  remote_path = Windows UNC source path (e.g. \\server\share\...)
    // During SFTP deploy: remote_path = "[ServerName] host:/linux/path"
    // Distinguish by checking if it starts with '[' (deploy display format)
    const isRemoteDeploy = payload.remote_path.startsWith('[');

    if (!isRemoteDeploy) {
        // --- Local copy phase ---
        const existing = appStore.taskRecords.find(
            r => r.folder === payload.folder && r.phase === 'copying'
        );
        if (existing) {
            existing.copyPercentage = payload.percentage;
            existing.copied = payload.copied_bytes;
            existing.total = payload.total_bytes;
            existing.speed = payload.speed;
            existing.copyTotal = payload.total_bytes;
            if (payload.percentage >= 100) {
                existing.copyCompleted = true;
                existing.phase = 'deploying';
                // If no deploy events follow within 8s, mark as completed
                setTimeout(() => {
                    if (existing.phase === 'deploying' && !existing.hasRemote) {
                        existing.phase = 'completed';
                    }
                }, 8000);
            }
        } else {
            // New record
            const record: TaskRecord = {
                id: `${payload.folder}-${Date.now()}`,
                startTime: new Date().toLocaleString(),
                folder: payload.folder,
                localPath: payload.local_path,
                copyPercentage: payload.percentage,
                copyCompleted: payload.percentage >= 100,
                copyTotal: payload.total_bytes,
                hasRemote: false,
                deployPercentage: 0,
                deployCompleted: false,
                speed: payload.speed,
                copied: payload.copied_bytes,
                total: payload.total_bytes,
                phase: payload.percentage >= 100 ? 'deploying' : 'copying',
            };
            appStore.taskRecords.unshift(record);
            if (appStore.taskRecords.length > 50) appStore.taskRecords.pop();
        }
    } else {
        // --- Remote SFTP deploy phase ---
        // Find the most recent record in 'deploying' phase to attach to
        const target = appStore.taskRecords.find(r => r.phase === 'deploying')
            ?? appStore.taskRecords[0];
        if (target) {
            target.hasRemote = true;
            target.phase = 'deploying';
            target.remotePath = payload.remote_path;
            target.deployPercentage = payload.percentage;
            target.speed = payload.speed;
            target.copied = payload.copied_bytes;
            target.total = payload.total_bytes;
            if (payload.percentage >= 100) {
                target.deployCompleted = true;
                target.phase = 'completed';
            }
        }
    }
}
