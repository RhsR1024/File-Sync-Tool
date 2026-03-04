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

export type TaskRecordPhase =
    | 'copying'
    | 'paused'
    | 'remote_pushing'
    | 'remote_deploying'
    | 'completed'
    | 'cancelled';

export interface RemoteServerRecord {
    key: string;
    label: string;
    percentage: number;
    completed: boolean;
    speed: number;
}

export interface TaskRecord {
    id: string;
    startTime: string;
    startedAtMs: number;
    finishedAtMs?: number;
    updatedAt: number;
    folder: string;
    localPath: string;
    copyPercentage: number;
    copyCompleted: boolean;
    copyTotal: number;
    hasRemote: boolean;
    remoteServers: RemoteServerRecord[];
    remoteExpanded: boolean;
    deployPercentage: number;
    deployCompleted: boolean;
    speed: number;
    copied: number;
    total: number;
    phase: TaskRecordPhase;
}

export const appStore = reactive({
    logs: [] as LogEntry[],
    taskRecords: [] as TaskRecord[],
    progress: null as ProgressState | null,
    isRunning: false,
    nextRunTime: '-',
    isManualDeploying: false,
    manualDeployMsg: '',
});

function normalizePath(path: string | undefined): string {
    if (!path) return '';
    return path.replace(/\//g, '\\').replace(/\\+$/g, '').toLowerCase();
}

function isTerminalPhase(phase: TaskRecordPhase): boolean {
    return phase === 'completed' || phase === 'cancelled';
}

function pinTaskRecord(record: TaskRecord) {
    const idx = appStore.taskRecords.findIndex(r => r.id === record.id);
    if (idx > 0) {
        appStore.taskRecords.splice(idx, 1);
        appStore.taskRecords.unshift(record);
    }
}

function touchTaskRecord(record: TaskRecord) {
    record.updatedAt = Date.now();
    pinTaskRecord(record);
}

function findLatestActiveRecord(): TaskRecord | undefined {
    return appStore.taskRecords.find(r => !isTerminalPhase(r.phase));
}

function findByFolder(folder: string, onlyActive = true): TaskRecord | undefined {
    return appStore.taskRecords.find(r => r.folder === folder && (!onlyActive || !isTerminalPhase(r.phase)));
}

function pathRelated(aRaw: string | undefined, bRaw: string | undefined): boolean {
    const a = normalizePath(aRaw);
    const b = normalizePath(bRaw);
    if (!a || !b) return false;
    return a === b || a.startsWith(`${b}\\`) || b.startsWith(`${a}\\`);
}

function findByLocalPath(localPath: string, onlyActive = true): TaskRecord | undefined {
    return appStore.taskRecords.find(r => pathRelated(r.localPath, localPath) && (!onlyActive || !isTerminalPhase(r.phase)));
}

function findTargetRecord(folder?: string, localPath?: string): TaskRecord | undefined {
    if (folder) {
        return findByFolder(folder, true)
            || findByFolder(folder, false)
            || (localPath ? findByLocalPath(localPath, true) : undefined)
            || (localPath ? findByLocalPath(localPath, false) : undefined);
    }
    return (localPath ? findByLocalPath(localPath, true) : undefined) || findLatestActiveRecord();
}

function isRecentlyCancelled(folder: string, localPath?: string): boolean {
    const latest = findTargetRecord(folder, localPath);
    if (!latest || latest.phase !== 'cancelled') return false;
    return Date.now() - latest.updatedAt < 10_000;
}

function createTaskRecord(payload: {
    folder: string;
    total_bytes: number;
    copied_bytes: number;
    percentage: number;
    speed: number;
    local_path: string;
    phase: TaskRecordPhase;
}): TaskRecord {
    const now = Date.now();
    return {
        id: `${payload.folder}-${now}`,
        startTime: new Date(now).toLocaleString(),
        startedAtMs: now,
        updatedAt: now,
        folder: payload.folder,
        localPath: payload.local_path,
        copyPercentage: payload.percentage,
        copyCompleted: payload.percentage >= 100,
        copyTotal: payload.total_bytes,
        hasRemote: false,
        remoteServers: [],
        remoteExpanded: false,
        deployPercentage: 0,
        deployCompleted: false,
        speed: payload.speed,
        copied: payload.copied_bytes,
        total: payload.total_bytes,
        phase: payload.phase,
    };
}

function mergeIntoPrimary(primary: TaskRecord, duplicate: TaskRecord) {
    primary.startedAtMs = Math.min(primary.startedAtMs, duplicate.startedAtMs);
    primary.updatedAt = Math.max(primary.updatedAt, duplicate.updatedAt);
    primary.copyTotal = Math.max(primary.copyTotal, duplicate.copyTotal);
    primary.total = Math.max(primary.total, duplicate.total);
    primary.copied = Math.max(primary.copied, duplicate.copied);
    primary.copyPercentage = Math.max(primary.copyPercentage, duplicate.copyPercentage);
    primary.deployPercentage = Math.max(primary.deployPercentage, duplicate.deployPercentage);
    primary.copyCompleted = primary.copyCompleted || duplicate.copyCompleted;
    primary.hasRemote = primary.hasRemote || duplicate.hasRemote;
    primary.deployCompleted = primary.deployCompleted || duplicate.deployCompleted;
    if (!primary.localPath && duplicate.localPath) primary.localPath = duplicate.localPath;

    for (const srv of duplicate.remoteServers) {
        const existing = primary.remoteServers.find(s => s.key === srv.key);
        if (existing) {
            existing.percentage = Math.max(existing.percentage, srv.percentage);
            existing.speed = Math.max(existing.speed, srv.speed);
            existing.completed = existing.completed || srv.completed;
        } else {
            primary.remoteServers.push({ ...srv });
        }
    }

    const rank: Record<TaskRecordPhase, number> = {
        copying: 1,
        paused: 2,
        remote_pushing: 3,
        remote_deploying: 4,
        completed: 5,
        cancelled: 6,
    };
    const allowPromoteToTerminal = isTerminalPhase(primary.phase);
    const duplicateIsTerminal = isTerminalPhase(duplicate.phase);
    if (duplicateIsTerminal && !allowPromoteToTerminal) {
        // Keep active state if primary is still running; terminal duplicate is likely stale.
    } else if (rank[duplicate.phase] > rank[primary.phase]) {
        primary.phase = duplicate.phase;
    }
    if (duplicate.finishedAtMs && (!primary.finishedAtMs || duplicate.finishedAtMs > primary.finishedAtMs)) {
        primary.finishedAtMs = duplicate.finishedAtMs;
    }
}

function mergeDuplicatesByLocalPath(primary: TaskRecord) {
    const duplicates = appStore.taskRecords.filter(
        r => r.id !== primary.id && pathRelated(r.localPath, primary.localPath)
    );
    if (!duplicates.length) return;

    for (const d of duplicates) {
        mergeIntoPrimary(primary, d);
    }

    for (const d of duplicates) {
        const idx = appStore.taskRecords.findIndex(r => r.id === d.id);
        if (idx >= 0) appStore.taskRecords.splice(idx, 1);
    }
}

function finalizeTask(record: TaskRecord) {
    record.copyCompleted = true;
    record.copyPercentage = 100;
    if (record.copyTotal > 0) {
        record.copied = record.copyTotal;
        record.total = record.copyTotal;
    }
    record.deployCompleted = true;
    record.phase = 'completed';
    record.speed = 0;
    record.finishedAtMs = Date.now();
    mergeDuplicatesByLocalPath(record);
    touchTaskRecord(record);
}

export function addLog(msg: string, type: 'info' | 'error' | 'success' = 'info') {
    const time = new Date().toLocaleTimeString();
    appStore.logs.unshift({ time, msg, type });
    if (appStore.logs.length > 1000) appStore.logs.pop();
}

export function upsertTaskRecord(payload: {
    folder: string;
    total_bytes: number;
    copied_bytes: number;
    percentage: number;
    speed: number;
    local_path: string;
    remote_path: string;
}) {
    const isRemoteDeploy = payload.remote_path.startsWith('[');

    if (!isRemoteDeploy) {
        if (isRecentlyCancelled(payload.folder, payload.local_path)) return;

        const existing = findTargetRecord(payload.folder, payload.local_path);
        if (existing && !isTerminalPhase(existing.phase)) {
            existing.localPath = payload.local_path || existing.localPath;
            existing.copyPercentage = Math.max(existing.copyPercentage, payload.percentage);
            existing.copied = Math.max(existing.copied, payload.copied_bytes);
            existing.total = Math.max(existing.total, payload.total_bytes);
            existing.copyTotal = Math.max(existing.copyTotal, payload.total_bytes);
            existing.speed = payload.speed;

            if (payload.percentage >= 100) {
                existing.copyCompleted = true;
                existing.copyPercentage = 100;
            } else if (existing.phase !== 'paused') {
                existing.phase = 'copying';
            }

            touchTaskRecord(existing);
            return;
        }

        const record = createTaskRecord({
            folder: payload.folder,
            total_bytes: payload.total_bytes,
            copied_bytes: payload.copied_bytes,
            percentage: payload.percentage,
            speed: payload.speed,
            local_path: payload.local_path,
            phase: 'copying',
        });

        appStore.taskRecords.unshift(record);
        if (appStore.taskRecords.length > 200) appStore.taskRecords.pop();
        return;
    }

    if (isRecentlyCancelled(payload.folder, payload.local_path)) return;

    let target = findTargetRecord(payload.folder, payload.local_path);
    if (target && isTerminalPhase(target.phase)) {
        // Do not revive historical completed/cancelled rows with late progress events.
        target = findByLocalPath(payload.local_path, true);
    }
    if (!target) {
        target = createTaskRecord({
            folder: payload.folder,
            total_bytes: payload.total_bytes,
            copied_bytes: payload.copied_bytes,
            percentage: 100,
            speed: payload.speed,
            local_path: payload.local_path,
            phase: 'remote_pushing',
        });
        target.copyCompleted = true;
        target.copyPercentage = 100;
        appStore.taskRecords.unshift(target);
        if (appStore.taskRecords.length > 200) appStore.taskRecords.pop();
    }

    target.hasRemote = true;
    target.localPath = payload.local_path || target.localPath;
    target.deployPercentage = payload.percentage;
    target.speed = payload.speed;

    if (!target.copyCompleted) {
        target.copyCompleted = true;
        target.copyPercentage = 100;
    }

    if (target.phase !== 'paused' && target.phase !== 'cancelled' && target.phase !== 'remote_deploying') {
        target.phase = 'remote_pushing';
    }

    const spaceIdx = payload.remote_path.indexOf(' ');
    const serverKey = spaceIdx > 0 ? payload.remote_path.substring(0, spaceIdx) : payload.remote_path;

    const existingServer = target.remoteServers.find(s => s.key === serverKey);
    if (existingServer) {
        existingServer.percentage = payload.percentage;
        existingServer.speed = payload.speed;
        existingServer.completed = payload.percentage >= 100;
    } else {
        target.remoteServers.push({
            key: serverKey,
            label: payload.remote_path,
            percentage: payload.percentage,
            completed: payload.percentage >= 100,
            speed: payload.speed,
        });
    }

    if (payload.percentage >= 100 && target.remoteServers.length > 0) {
        target.deployCompleted = target.remoteServers.every(s => s.completed);
    }

    mergeDuplicatesByLocalPath(target);
    touchTaskRecord(target);
}

export function setTaskRecordPaused(folder: string | undefined, paused: boolean) {
    const target = findTargetRecord(folder, undefined);
    if (!target || isTerminalPhase(target.phase)) return;

    if (paused) {
        target.phase = 'paused';
    } else if (target.hasRemote && !target.deployCompleted && target.copyCompleted) {
        target.phase = target.deployPercentage >= 100 ? 'remote_deploying' : 'remote_pushing';
    } else {
        target.phase = 'copying';
    }

    touchTaskRecord(target);
}

export function markTaskRecordCancelled(folder?: string) {
    const target = findTargetRecord(folder, appStore.progress?.localPath);
    if (!target) return;
    target.phase = 'cancelled';
    target.speed = 0;
    target.finishedAtMs = Date.now();
    touchTaskRecord(target);
}

function extractFolderByPrefix(msg: string, prefix: string): string | undefined {
    if (!msg.startsWith(prefix)) return undefined;
    const folder = msg.slice(prefix.length).trim();
    return folder || undefined;
}

function completeFromServerSuccess(target: TaskRecord, lowerMsg: string) {
    const matched = /^\[(.+?)\]\s+deployment successful$/.exec(lowerMsg);
    if (matched) {
        const key = `[${matched[1]}]`;
        const server = target.remoteServers.find(s => s.key === key || s.key.toLowerCase().startsWith(key.toLowerCase()));
        if (server) server.completed = true;
    }

    if (target.remoteServers.length > 0 && target.remoteServers.every(s => s.completed)) {
        target.deployCompleted = true;
    }

    touchTaskRecord(target);
}

export function syncTaskRecordByLog(msg: string, level: string) {
    const lower = msg.toLowerCase();

    const cancelledFolder = extractFolderByPrefix(msg, 'Copy cancelled:');
    if (
        lower.includes('scan cancelled by user')
        || lower.includes('remaining deployments cancelled')
        || lower.includes('cancelled by user')
        || cancelledFolder
    ) {
        markTaskRecordCancelled(cancelledFolder);
        return;
    }

    const copiedFolder = extractFolderByPrefix(msg, 'Successfully copied:');
    if (copiedFolder) {
        const target = findTargetRecord(copiedFolder);
        if (target) finalizeTask(target);
        return;
    }

    const target = findLatestActiveRecord();
    if (!target || isTerminalPhase(target.phase)) return;

    if (
        lower.includes('starting deployment for')
        || lower.includes('deploying to server')
        || lower.includes('uploading to')
    ) {
        target.hasRemote = true;
        target.phase = 'remote_pushing';
        touchTaskRecord(target);
        return;
    }

    if (
        lower.includes('executing post commands')
        || lower.includes('executing post-deployment commands')
    ) {
        target.hasRemote = true;
        target.phase = 'remote_deploying';
        touchTaskRecord(target);
        return;
    }

    if (lower.includes('deployment successful')) {
        completeFromServerSuccess(target, lower);
        return;
    }

    if (level === 'error' && lower.includes('deployment failed')) {
        touchTaskRecord(target);
    }
}
