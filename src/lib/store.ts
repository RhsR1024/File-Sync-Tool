import { reactive } from 'vue';
import { deriveTaskRecordElapsedSeconds } from './taskRecordElapsed';

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

export type TaskRecordPhase =
    | 'queued'
    | 'copying'
    | 'paused'
    | 'remote_pushing'
    | 'remote_deploying'
    | 'completed'
    | 'failed'
    | 'cancelled'
    | 'interrupted';

export interface RemoteServerRecord {
    key: string;
    label: string;
    percentage: number;
    completed: boolean;
    failed: boolean;
    speed: number;
}

export interface TaskRecord {
    id: string;
    startTime: string;
    startedAtMs: number;
    finishedAtMs?: number;
    updatedAt: number;
    elapsedSeconds: number;
    folder: string;
    sourcePath: string;
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
    source: 'manual' | 'scheduled';
    ignored?: boolean;
    /** Filter rules applied to this task (for display in the task table). */
    filterExtensions: string[];
    filterKeywords: string[];
    /** Expected number of servers to deploy to (prevents premature finalization). */
    expectedServerCount?: number;
    /** Name of the server currently being deployed to. */
    currentServerName?: string;
}

export const appStore = reactive({
    logs: [] as LogEntry[],
    taskRecords: [] as TaskRecord[],
    progress: null as ProgressState | null,
    isRunning: false,
    nextRunTime: '-',
    isManualDeploying: false,
    manualDeployMsg: '',
    maxLogLines: 200,
    maxTaskRecords: 100,
});

function normalizePath(path: string | undefined): string {
    if (!path) return '';
    return path.replace(/\//g, '\\').replace(/\\+$/g, '').toLowerCase();
}

function samePath(aRaw: string | undefined, bRaw: string | undefined): boolean {
    const a = normalizePath(aRaw);
    const b = normalizePath(bRaw);
    return !!a && a === b;
}

function isTerminalPhase(phase: TaskRecordPhase): boolean {
    return phase === 'completed' || phase === 'failed' || phase === 'cancelled' || phase === 'interrupted';
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
    record.elapsedSeconds = deriveTaskRecordElapsedSeconds(record);
    pinTaskRecord(record);
}

function findLatestActiveRecord(): TaskRecord | undefined {
    return appStore.taskRecords.find(r => !isTerminalPhase(r.phase) && r.phase !== 'queued')
        || appStore.taskRecords.find(r => !isTerminalPhase(r.phase));
}

function findByFolder(folder: string, onlyActive = true): TaskRecord | undefined {
    return appStore.taskRecords.find(r => r.folder === folder && (!onlyActive || !isTerminalPhase(r.phase)));
}

function findByLocalPath(localPath: string, onlyActive = true): TaskRecord | undefined {
    return appStore.taskRecords.find(r => samePath(r.localPath, localPath) && (!onlyActive || !isTerminalPhase(r.phase)));
}

function findManualByPaths(sourcePath: string, localPath: string, onlyActive = true): TaskRecord | undefined {
    return appStore.taskRecords.find(r =>
        r.source === 'manual'
        && samePath(r.sourcePath, sourcePath)
        && samePath(r.localPath, localPath)
        && (!onlyActive || !isTerminalPhase(r.phase))
    );
}

function findTargetRecord(folder?: string, localPath?: string): TaskRecord | undefined {
    if (localPath) {
        return findByLocalPath(localPath, true)
            || (folder ? findByFolder(folder, true) : undefined)
            || findByLocalPath(localPath, false)
            || (folder ? findByFolder(folder, false) : undefined);
    }

    if (folder) {
        return findByFolder(folder, true)
            || findByFolder(folder, false);
    }
    return findLatestActiveRecord();
}

function isRecentlyCancelled(folder: string, localPath?: string): boolean {
    const latest = findTargetRecord(folder, localPath);
    if (!latest || latest.phase !== 'cancelled' || latest.ignored) return false;
    return Date.now() - latest.updatedAt < 10_000;
}

function createTaskRecord(payload: {
    folder: string;
    total_bytes: number;
    copied_bytes: number;
    percentage: number;
    speed: number;
    local_path: string;
    source_path: string;
    phase: TaskRecordPhase;
    source: 'manual' | 'scheduled';
}): TaskRecord {
    const now = Date.now();
    return {
        id: `${payload.folder}-${now}`,
        startTime: new Date(now).toLocaleString(),
        startedAtMs: now,
        updatedAt: now,
        elapsedSeconds: 0,
        folder: payload.folder,
        sourcePath: payload.source_path,
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
        source: payload.source,
        ignored: false,
        filterExtensions: [],
        filterKeywords: [],
    };
}

function mergeIntoPrimary(primary: TaskRecord, duplicate: TaskRecord) {
    primary.startedAtMs = Math.min(primary.startedAtMs, duplicate.startedAtMs);
    primary.updatedAt = Math.max(primary.updatedAt, duplicate.updatedAt);
    primary.elapsedSeconds = Math.max(primary.elapsedSeconds, duplicate.elapsedSeconds ?? 0);
    primary.copyTotal = Math.max(primary.copyTotal, duplicate.copyTotal);
    primary.total = Math.max(primary.total, duplicate.total);
    primary.copied = Math.max(primary.copied, duplicate.copied);
    primary.copyPercentage = Math.max(primary.copyPercentage, duplicate.copyPercentage);
    primary.deployPercentage = Math.max(primary.deployPercentage, duplicate.deployPercentage);
    primary.copyCompleted = primary.copyCompleted || duplicate.copyCompleted;
    primary.hasRemote = primary.hasRemote || duplicate.hasRemote;
    primary.deployCompleted = primary.deployCompleted || duplicate.deployCompleted;
    if (!primary.localPath && duplicate.localPath) primary.localPath = duplicate.localPath;
    if (!primary.sourcePath && duplicate.sourcePath) primary.sourcePath = duplicate.sourcePath;

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
        queued: 0,
        copying: 1,
        paused: 2,
        remote_pushing: 3,
        remote_deploying: 4,
        completed: 5,
        failed: 6,
        cancelled: 7,
        interrupted: 8,
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
        r => r.id !== primary.id && samePath(r.localPath, primary.localPath)
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
    record.ignored = false;
    record.speed = 0;
    record.finishedAtMs = Date.now();
    record.elapsedSeconds = deriveTaskRecordElapsedSeconds(record);
    mergeDuplicatesByLocalPath(record);
    touchTaskRecord(record);
}

export function addLog(msg: string, type: 'info' | 'error' | 'success' | 'command' = 'info') {
    const time = new Date().toLocaleTimeString();
    appStore.logs.push({ time, msg, type });
    while (appStore.logs.length > appStore.maxLogLines) {
        appStore.logs.shift();
    }
}

export function upsertTaskRecord(payload: {
    folder: string;
    total_bytes: number;
    copied_bytes: number;
    percentage: number;
    speed: number;
    elapsed_seconds?: number;
    local_path: string;
    remote_path: string;
    source?: string;
}) {
    const isRemoteDeploy = payload.remote_path.startsWith('[');

    if (!isRemoteDeploy) {
        if (isRecentlyCancelled(payload.folder, payload.local_path)) return;

        const existing = findTargetRecord(payload.folder, payload.local_path);
        if (existing && !isTerminalPhase(existing.phase)) {
            existing.localPath = payload.local_path || existing.localPath;
            if (payload.remote_path && !existing.sourcePath) {
                existing.sourcePath = payload.remote_path;
            }
            existing.copyPercentage = Math.max(existing.copyPercentage, payload.percentage);
            existing.copied = Math.max(existing.copied, payload.copied_bytes);
            existing.total = Math.max(existing.total, payload.total_bytes);
            existing.copyTotal = Math.max(existing.copyTotal, payload.total_bytes);
            existing.speed = payload.speed;
            existing.elapsedSeconds = deriveTaskRecordElapsedSeconds(existing, payload.elapsed_seconds);

            if (payload.percentage >= 100) {
                existing.copyCompleted = true;
                existing.copyPercentage = 100;
                if (existing.phase === 'queued') {
                    existing.phase = 'copying';
                }
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
            source_path: payload.remote_path || '',
            phase: 'copying',
            source: (payload.source === 'manual' ? 'manual' : 'scheduled'),
        });
        record.elapsedSeconds = deriveTaskRecordElapsedSeconds(record, payload.elapsed_seconds);

        appStore.taskRecords.unshift(record);
        if (appStore.taskRecords.length > appStore.maxTaskRecords) appStore.taskRecords.pop();
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
            source_path: '',
            phase: 'remote_pushing',
            source: (payload.source === 'manual' ? 'manual' : 'scheduled'),
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
    target.elapsedSeconds = deriveTaskRecordElapsedSeconds(target, payload.elapsed_seconds);

    if (!target.copyCompleted) {
        target.copyCompleted = true;
        target.copyPercentage = 100;
    }

    if (target.phase !== 'paused' && target.phase !== 'cancelled' && target.phase !== 'remote_deploying') {
        target.phase = 'remote_pushing';
    }

    const serverLabel = payload.remote_path.trim();
    // Extract current server name from "[ServerName] /path" format
    const serverNameMatch = /^\[(.+?)\]/.exec(serverLabel);
    if (serverNameMatch) {
        target.currentServerName = serverNameMatch[1];
    }
    const existingServer = target.remoteServers.find(s => s.label === serverLabel);
    if (existingServer) {
        existingServer.percentage = payload.percentage;
        existingServer.speed = payload.speed;
        existingServer.completed = payload.percentage >= 100;
    } else {
        target.remoteServers.push({
            key: serverLabel,
            label: serverLabel,
            percentage: payload.percentage,
            completed: payload.percentage >= 100,
            failed: false,
            speed: payload.speed,
        });
    }

    if (payload.percentage >= 100 && target.remoteServers.length > 0) {
        const allDone = target.remoteServers.every(s => s.completed || s.failed);
        const expected = target.expectedServerCount ?? 0;
        // Only mark deploy complete when all expected servers have reported
        if (expected > 0) {
            const finishedCount = target.remoteServers.filter(s => s.completed || s.failed).length;
            target.deployCompleted = allDone && finishedCount >= expected;
        } else {
            target.deployCompleted = allDone;
        }
    }

    mergeDuplicatesByLocalPath(target);
    touchTaskRecord(target);
}

export function enqueueManualCopyTaskRecord(payload: {
    folder: string;
    sourcePath: string;
    localPath: string;
    filterExtensions?: string[];
    filterKeywords?: string[];
}) {
    const existing = findManualByPaths(payload.sourcePath, payload.localPath, true);
    if (existing) {
        if (existing.phase === 'completed') return;
        touchTaskRecord(existing);
        return;
    }

    const record = createTaskRecord({
        folder: payload.folder,
        total_bytes: 0,
        copied_bytes: 0,
        percentage: 0,
        speed: 0,
        local_path: payload.localPath,
        source_path: payload.sourcePath,
        phase: 'queued',
        source: 'manual',
    });
    record.filterExtensions = payload.filterExtensions ?? [];
    record.filterKeywords = payload.filterKeywords ?? [];

    appStore.taskRecords.unshift(record);
    if (appStore.taskRecords.length > 200) appStore.taskRecords.pop();
}

export function updateManualCopyTaskState(payload: {
    folder: string;
    sourcePath: string;
    localPath: string;
    state: 'started' | 'completed' | 'failed' | 'cancelled';
}) {
    const target = findManualByPaths(payload.sourcePath, payload.localPath, false)
        || findTargetRecord(payload.folder, payload.localPath);
    if (!target) return;

    if (payload.state === 'started') {
        if (!isTerminalPhase(target.phase)) {
            target.phase = 'copying';
            touchTaskRecord(target);
        }
        return;
    }

    if (payload.state === 'completed') {
        finalizeTask(target);
        return;
    }

    target.phase = payload.state === 'failed' ? 'failed' : 'cancelled';
    target.ignored = false;
    target.speed = 0;
    target.finishedAtMs = Date.now();
    target.elapsedSeconds = deriveTaskRecordElapsedSeconds(target);
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
    target.ignored = false;
    target.speed = 0;
    target.finishedAtMs = Date.now();
    target.elapsedSeconds = deriveTaskRecordElapsedSeconds(target);
    touchTaskRecord(target);
}

export function markTaskRecordSkipped(folder?: string) {
    const target = findTargetRecord(folder, appStore.progress?.localPath);
    if (!target) return;
    target.phase = 'cancelled';
    target.ignored = false;
    target.speed = 0;
    target.finishedAtMs = Date.now();
    target.elapsedSeconds = deriveTaskRecordElapsedSeconds(target);
    touchTaskRecord(target);
}

export function markTaskRecordIgnored(id: string) {
    const target = appStore.taskRecords.find(r => r.id === id);
    if (!target) return;
    target.phase = 'cancelled';
    target.ignored = true;
    target.speed = 0;
    target.finishedAtMs = Date.now();
    target.elapsedSeconds = deriveTaskRecordElapsedSeconds(target);
    touchTaskRecord(target);
}

export function prepareTaskRecordForRetry(id: string) {
    const target = appStore.taskRecords.find(r => r.id === id);
    if (!target) return;
    const now = Date.now();
    target.phase = 'queued';
    target.ignored = false;
    target.startedAtMs = now;
    target.startTime = new Date(now).toLocaleString();
    target.updatedAt = now;
    target.finishedAtMs = undefined;
    target.elapsedSeconds = 0;
    target.copyPercentage = 0;
    target.copyCompleted = false;
    target.deployPercentage = 0;
    target.deployCompleted = false;
    target.speed = 0;
    target.copied = 0;
    target.total = 0;
    target.copyTotal = 0;
    target.hasRemote = false;
    target.remoteExpanded = false;
    target.remoteServers = [];
    target.expectedServerCount = undefined;
    target.currentServerName = undefined;
    pinTaskRecord(target);
}

export function removeTaskRecord(id: string) {
    const idx = appStore.taskRecords.findIndex(r => r.id === id);
    if (idx >= 0) {
        appStore.taskRecords.splice(idx, 1);
    }
}

function extractFolderByPrefix(msg: string, prefix: string): string | undefined {
    if (!msg.startsWith(prefix)) return undefined;
    const folder = msg.slice(prefix.length).trim();
    return folder || undefined;
}

function extractFolderByUpToDate(msg: string): string | undefined {
    const matched = /^'(.+)' is up to date/i.exec(msg);
    return matched?.[1]?.trim() || undefined;
}

function extractServerKey(lowerMsg: string): string | undefined {
    const matched = /^\[(.+?)\]/.exec(lowerMsg);
    return matched ? matched[1].toLowerCase() : undefined;
}

function matchServerRecord(server: RemoteServerRecord, key: string): boolean {
    const labelLower = server.label.toLowerCase();
    const bracketKey = `[${key}]`;
    return labelLower.startsWith(`${bracketKey} `) || labelLower === bracketKey || server.key.toLowerCase() === bracketKey;
}

function completeFromServerSuccess(target: TaskRecord, lowerMsg: string) {
    const key = extractServerKey(lowerMsg);
    if (key) {
        for (const server of target.remoteServers) {
            if (matchServerRecord(server, key)) {
                server.completed = true;
                server.failed = false;
                server.percentage = Math.max(server.percentage, 100);
            }
        }
    }

    // Check if all expected servers are done
    const expectedCount = target.expectedServerCount ?? 0;
    const completedCount = target.remoteServers.filter(s => s.completed).length;
    const failedCount = target.remoteServers.filter(s => s.failed).length;
    const finishedCount = completedCount + failedCount;

    if (target.remoteServers.length === 0 && expectedCount <= 0) {
        // Manual deploy or single-server deploy without server tracking — mark complete directly
        target.deployCompleted = true;
    } else if (expectedCount > 0 && finishedCount >= expectedCount) {
        target.deployCompleted = true;
    } else if (expectedCount <= 0 && target.remoteServers.length > 0 && target.remoteServers.every(s => s.completed || s.failed)) {
        target.deployCompleted = true;
    }

    touchTaskRecord(target);
}

function markServerFailed(target: TaskRecord, lowerMsg: string) {
    const key = extractServerKey(lowerMsg);
    let matchedAny = false;
    if (key) {
        for (const server of target.remoteServers) {
            if (matchServerRecord(server, key)) {
                server.failed = true;
                server.completed = false;
                matchedAny = true;
            }
        }
    }

    // Check if all expected servers are done (including failures)
    const expectedCount = target.expectedServerCount ?? 0;
    const finishedCount = target.remoteServers.filter(s => s.completed || s.failed).length;

    if (expectedCount > 0 && finishedCount >= expectedCount) {
        target.deployCompleted = true;
        // If any server failed, mark as failed
        target.phase = target.remoteServers.some(s => s.failed) ? 'failed' : 'completed';
        target.finishedAtMs = Date.now();
        target.speed = 0;
        target.elapsedSeconds = deriveTaskRecordElapsedSeconds(target);
    } else if (expectedCount <= 0 && target.remoteServers.length > 0 && target.remoteServers.every(s => s.completed || s.failed)) {
        target.deployCompleted = true;
        target.phase = target.remoteServers.some(s => s.failed) ? 'failed' : 'completed';
        target.finishedAtMs = Date.now();
        target.speed = 0;
        target.elapsedSeconds = deriveTaskRecordElapsedSeconds(target);
    } else if (!matchedAny) {
        // Failure occurred before any upload progress was reported (e.g. connection timeout).
        // No server records exist to match, so mark the task failed directly.
        target.deployCompleted = true;
        target.phase = 'failed';
        target.finishedAtMs = Date.now();
        target.speed = 0;
        target.elapsedSeconds = deriveTaskRecordElapsedSeconds(target);
    }

    touchTaskRecord(target);
}

export function syncTaskRecordByLog(msg: string, level: string) {
    const lower = msg.toLowerCase();

    const upToDateFolder = extractFolderByUpToDate(msg);
    if (upToDateFolder) {
        const target = findTargetRecord(upToDateFolder);
        if (target) finalizeTask(target);
        return;
    }

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

    const skippedFolder = extractFolderByPrefix(msg, 'Copy skipped:');
    if (skippedFolder) {
        markTaskRecordSkipped(skippedFolder);
        return;
    }

    const copiedFolder = extractFolderByPrefix(msg, 'Successfully copied:');
    if (copiedFolder) {
        const target = findTargetRecord(copiedFolder);
        if (target) {
            // If the task has remote deployment that hasn't completed yet, don't finalize.
            // The deploy success/failure events will handle finalization.
            if (target.hasRemote && !target.deployCompleted) {
                target.copyCompleted = true;
                target.copyPercentage = 100;
                touchTaskRecord(target);
            } else {
                finalizeTask(target);
            }
        }
        return;
    }

    const target = findLatestActiveRecord();
    if (!target || isTerminalPhase(target.phase)) return;

    // "Starting deployment for N server(s)..." — extract expected count
    const deployCountMatch = /starting deployment for (\d+) server/i.exec(msg);
    if (deployCountMatch) {
        target.hasRemote = true;
        target.expectedServerCount = parseInt(deployCountMatch[1], 10);
        target.phase = 'remote_pushing';
        touchTaskRecord(target);
        return;
    }

    // "Starting manual deployment: ... -> [ServerName] host:path"
    if (lower.includes('starting manual deployment')) {
        target.hasRemote = true;
        target.phase = 'remote_pushing';
        touchTaskRecord(target);
        return;
    }

    // "Deploying to server N/M [ServerName]" — extract current server name
    const deployingToMatch = /deploying to server \d+\/\d+ \[(.+?)\]/i.exec(msg);
    if (deployingToMatch) {
        target.hasRemote = true;
        target.currentServerName = deployingToMatch[1];
        target.phase = 'remote_pushing';
        touchTaskRecord(target);
        return;
    }

    if (lower.includes('uploading to')) {
        target.hasRemote = true;
        target.phase = 'remote_pushing';
        touchTaskRecord(target);
        return;
    }

    // "[ServerName] Executing post commands..." or "Executing post-deployment commands..."
    if (
        lower.includes('executing post commands')
        || lower.includes('executing post-deployment commands')
    ) {
        target.hasRemote = true;
        target.phase = 'remote_deploying';
        // Extract server name from "[ServerName] Executing..."
        const postCmdServerMatch = /^\[(.+?)\]/.exec(msg);
        if (postCmdServerMatch) {
            target.currentServerName = postCmdServerMatch[1];
        }
        touchTaskRecord(target);
        return;
    }

    if (lower.includes('deployment successful')) {
        completeFromServerSuccess(target, lower);
        if (target.deployCompleted) {
            // All servers done — determine final phase
            const anyFailed = target.remoteServers.some(s => s.failed);
            if (anyFailed) {
                target.phase = 'failed';
                target.speed = 0;
                target.finishedAtMs = Date.now();
            } else {
                finalizeTask(target);
            }
        }
        return;
    }

    if (level === 'error' && lower.includes('deployment failed')) {
        markServerFailed(target, lower);
    }
}

export function markStaleTasksInterrupted() {
    for (const record of appStore.taskRecords) {
        if (record.phase === 'copying' || record.phase === 'paused' || record.phase === 'queued'
            || record.phase === 'remote_pushing' || record.phase === 'remote_deploying') {
            record.phase = 'interrupted';
            record.speed = 0;
            record.finishedAtMs = record.updatedAt;
            record.elapsedSeconds = deriveTaskRecordElapsedSeconds(record);
        }
    }
}

/**
 * Pre-register a manual deploy task record so that subsequent deploy events
 * are associated with the same record and it won't finalize prematurely.
 */
export function preRegisterManualDeploy(folder: string, localPath: string, serverCount: number): TaskRecord {
    // Look for an existing active record for this folder/path
    let target = findTargetRecord(folder, localPath);
    if (target && !isTerminalPhase(target.phase)) {
        target.hasRemote = true;
        target.expectedServerCount = serverCount;
        target.phase = 'remote_pushing';
        touchTaskRecord(target);
        return target;
    }

    // Create a new record
    const record = createTaskRecord({
        folder,
        total_bytes: 0,
        copied_bytes: 0,
        percentage: 0,
        speed: 0,
        local_path: localPath,
        source_path: '',
        phase: 'remote_pushing',
        source: 'manual',
    });
    record.hasRemote = true;
    record.expectedServerCount = serverCount;
    record.copyCompleted = true;
    record.copyPercentage = 100;
    record.elapsedSeconds = 0;
    appStore.taskRecords.unshift(record);
    if (appStore.taskRecords.length > appStore.maxTaskRecords) appStore.taskRecords.pop();
    return record;
}

export function setManualDeployCurrentServer(taskId: string, serverName: string) {
    const record = appStore.taskRecords.find(r => r.id === taskId);
    if (record && !isTerminalPhase(record.phase)) {
        record.currentServerName = serverName;
        touchTaskRecord(record);
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
