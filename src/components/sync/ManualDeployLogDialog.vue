<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onDeactivated, ref, watch } from 'vue';
import { CheckCircle2, ChevronDown, Clock3, LoaderCircle, Pencil, Server, X, XCircle } from 'lucide-vue-next';
import { useI18n } from 'vue-i18n';
import type { DeployAttempt, DeployServer, TaskGroup, TaskLogEntry } from '@/lib/tauri';
import type { ManualDeploySession } from '@/lib/taskStateStore';

const props = defineProps<{
  open: boolean;
  session: ManualDeploySession | null;
  group: TaskGroup | null;
  logs: TaskLogEntry[];
  servers: DeployServer[];
}>();

const emit = defineEmits<{
  close: [];
  editServer: [serverId: string];
}>();

const { t, locale } = useI18n();
const dialogRef = ref<HTMLElement | null>(null);
const closeButtonRef = ref<HTMLButtonElement | null>(null);
const logScrollerRef = ref<HTMLElement | null>(null);
const selectedServerId = ref<string | null>(null);
const followingTail = ref(true);
let previousFocus: HTMLElement | null = null;

const run = computed(() => props.group?.runs.find(candidate => candidate.run_id === props.session?.run_id) ?? null);
const attempts = computed(() => run.value?.deploy_attempts ?? []);
const attemptByServer = computed(() => new Map(attempts.value.map(attempt => [attempt.server_id, attempt])));

type DisplayStatus = 'waiting' | 'connecting' | 'uploading' | 'commands' | 'success' | 'failed' | 'cancelled' | 'interrupted';

interface ServerDisplayItem {
  id: string;
  name: string;
  host: string;
  attempt: DeployAttempt | null;
  status: DisplayStatus;
}

function attemptDisplayStatus(attempt: DeployAttempt | null): DisplayStatus {
  if (!attempt) return 'waiting';
  if (attempt.status === 'success') return 'success';
  if (attempt.status === 'failed') return 'failed';
  if (attempt.status === 'cancelled') return 'cancelled';
  if (attempt.status === 'interrupted') return 'interrupted';
  if (attempt.stage === 'connecting') return 'connecting';
  if (attempt.stage === 'uploading') return 'uploading';
  if (attempt.stage === 'executing_commands') return 'commands';
  return 'waiting';
}

const serverItems = computed<ServerDisplayItem[]>(() => {
  const configured = new Map(props.servers.map(server => [server.id, server]));
  return (props.session?.server_ids ?? []).map(id => {
    const server = configured.get(id);
    const attempt = attemptByServer.value.get(id) ?? null;
    return {
      id,
      name: server?.name || attempt?.server_name || server?.host || id,
      host: server?.host || attempt?.server_host || '',
      attempt,
      status: attemptDisplayStatus(attempt),
    };
  });
});

const filteredLogs = computed(() => {
  if (!props.session) return [];
  return props.logs.filter(log => (
    log.task_group_id === props.session!.task_group_id
    && log.run_id === props.session!.run_id
    && (!selectedServerId.value || log.server_id === selectedServerId.value)
  ));
});

const visibleLogs = computed(() => filteredLogs.value.slice(-2_000));
const successCount = computed(() => serverItems.value.filter(item => item.status === 'success').length);
const failedCount = computed(() => serverItems.value.filter(item => item.status === 'failed').length);
const activeCount = computed(() => serverItems.value.filter(item => ['connecting', 'uploading', 'commands'].includes(item.status)).length);
const waitingCount = computed(() => serverItems.value.filter(item => item.status === 'waiting').length);
const isFinished = computed(() => Boolean(run.value?.finished_at));

function statusLabel(status: DisplayStatus) {
  return t(`settings.manualDeployLog.status.${status}`);
}

function statusClass(status: DisplayStatus) {
  const classes: Record<DisplayStatus, string> = {
    waiting: 'border-slate-200 bg-slate-50 text-slate-600',
    connecting: 'border-blue-200 bg-blue-50 text-blue-700',
    uploading: 'border-indigo-200 bg-indigo-50 text-indigo-700',
    commands: 'border-violet-200 bg-violet-50 text-violet-700',
    success: 'border-emerald-200 bg-emerald-50 text-emerald-700',
    failed: 'border-rose-200 bg-rose-50 text-rose-700',
    cancelled: 'border-amber-200 bg-amber-50 text-amber-700',
    interrupted: 'border-orange-200 bg-orange-50 text-orange-700',
  };
  return classes[status];
}

function logLevelClass(level: string) {
  if (level === 'error') return 'text-rose-300';
  if (level === 'success') return 'text-emerald-300';
  if (level === 'warn') return 'text-amber-300';
  if (level === 'command') return 'text-cyan-300';
  return 'text-slate-200';
}

function formatTime(timestamp: string) {
  try {
    return new Intl.DateTimeFormat(locale.value, {
      hour: '2-digit', minute: '2-digit', second: '2-digit', hour12: false,
    }).format(new Date(timestamp));
  } catch {
    return timestamp;
  }
}

function close() {
  emit('close');
}

function handleKeydown(event: KeyboardEvent) {
  if (event.key === 'Escape') {
    event.preventDefault();
    close();
    return;
  }
  if (event.key !== 'Tab' || !dialogRef.value) return;
  const focusable = dialogRef.value.querySelectorAll<HTMLElement>(
    'button:not([disabled]), input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])',
  );
  if (!focusable.length) return;
  const first = focusable[0];
  const last = focusable[focusable.length - 1];
  if (event.shiftKey && document.activeElement === first) {
    event.preventDefault();
    last.focus();
  } else if (!event.shiftKey && document.activeElement === last) {
    event.preventDefault();
    first.focus();
  }
}

function handleLogScroll() {
  const element = logScrollerRef.value;
  if (!element) return;
  followingTail.value = element.scrollHeight - element.scrollTop - element.clientHeight < 48;
}

async function scrollToLatest() {
  followingTail.value = true;
  await nextTick();
  logScrollerRef.value?.scrollTo({ top: logScrollerRef.value.scrollHeight, behavior: 'smooth' });
}

watch(() => props.open, async open => {
  if (open) {
    previousFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    selectedServerId.value = null;
    followingTail.value = true;
    await nextTick();
    closeButtonRef.value?.focus();
    logScrollerRef.value?.scrollTo({ top: logScrollerRef.value.scrollHeight });
  } else {
    await nextTick();
    previousFocus?.focus?.();
    previousFocus = null;
  }
});

watch(() => visibleLogs.value.length, async () => {
  if (!props.open || !followingTail.value) return;
  await nextTick();
  logScrollerRef.value?.scrollTo({ top: logScrollerRef.value.scrollHeight });
});

onDeactivated(close);
onBeforeUnmount(() => previousFocus?.focus?.());
</script>

<template>
  <Teleport to="body">
    <Transition name="manual-deploy-dialog">
      <div
        v-if="open && session"
        class="fixed inset-0 z-[75] flex items-center justify-center bg-slate-950/55 p-4"
        @click.self="close"
      >
        <section
          ref="dialogRef"
          role="dialog"
          aria-modal="true"
          aria-labelledby="manual-deploy-log-title"
          aria-describedby="manual-deploy-log-description"
          class="flex max-h-[88vh] w-full max-w-6xl flex-col overflow-hidden rounded-2xl border border-slate-200 bg-white shadow-[0_24px_80px_rgba(15,23,42,0.28)]"
          @keydown="handleKeydown"
        >
          <header class="flex items-start justify-between gap-4 border-b border-slate-200 px-5 py-4">
            <div class="min-w-0">
              <div class="flex items-center gap-2">
                <span class="relative flex h-2.5 w-2.5" aria-hidden="true">
                  <span v-if="!isFinished" class="absolute inline-flex h-full w-full animate-ping rounded-full bg-indigo-400 opacity-70 motion-reduce:animate-none"></span>
                  <span class="relative inline-flex h-2.5 w-2.5 rounded-full" :class="isFinished ? 'bg-slate-400' : 'bg-indigo-500'"></span>
                </span>
                <h2 id="manual-deploy-log-title" class="truncate text-lg font-semibold text-slate-950">
                  {{ t('settings.manualDeployLog.title', { name: session.display_name }) }}
                </h2>
              </div>
              <p id="manual-deploy-log-description" class="mt-1 text-sm text-slate-600">
                {{ t('settings.manualDeployLog.description') }}
              </p>
            </div>
            <button
              ref="closeButtonRef"
              type="button"
              class="inline-flex h-11 w-11 shrink-0 cursor-pointer items-center justify-center rounded-xl text-slate-500 transition-colors duration-200 hover:bg-slate-100 hover:text-slate-800 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500/50"
              :aria-label="t('settings.manualDeployLog.close')"
              :title="t('settings.manualDeployLog.close')"
              @click="close"
            >
              <X class="h-5 w-5" aria-hidden="true" />
            </button>
          </header>

          <div class="grid grid-cols-2 gap-2 border-b border-slate-200 bg-slate-50 px-5 py-3 sm:grid-cols-4" aria-live="polite">
            <div class="rounded-lg border border-emerald-200 bg-white px-3 py-2 text-sm text-slate-700">
              <span class="font-semibold text-emerald-700">{{ successCount }}</span> {{ t('settings.manualDeployLog.summary.success') }}
            </div>
            <div class="rounded-lg border border-blue-200 bg-white px-3 py-2 text-sm text-slate-700">
              <span class="font-semibold text-blue-700">{{ activeCount }}</span> {{ t('settings.manualDeployLog.summary.active') }}
            </div>
            <div class="rounded-lg border border-slate-200 bg-white px-3 py-2 text-sm text-slate-700">
              <span class="font-semibold text-slate-700">{{ waitingCount }}</span> {{ t('settings.manualDeployLog.summary.waiting') }}
            </div>
            <div class="rounded-lg border border-rose-200 bg-white px-3 py-2 text-sm text-slate-700">
              <span class="font-semibold text-rose-700">{{ failedCount }}</span> {{ t('settings.manualDeployLog.summary.failed') }}
            </div>
          </div>

          <div class="grid min-h-0 flex-1 grid-cols-1 lg:grid-cols-[minmax(240px,0.34fr)_minmax(0,1fr)]">
            <aside class="max-h-52 overflow-y-auto border-b border-slate-200 bg-white p-3 lg:max-h-none lg:border-b-0 lg:border-r">
              <button
                type="button"
                class="mb-2 flex min-h-11 w-full cursor-pointer items-center gap-2 rounded-xl border px-3 py-2 text-left text-sm transition-colors duration-200 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500/50"
                :class="selectedServerId === null ? 'border-indigo-200 bg-indigo-50 text-indigo-700' : 'border-slate-200 bg-white text-slate-700 hover:bg-slate-50'"
                @click="selectedServerId = null"
              >
                <Server class="h-4 w-4" aria-hidden="true" />
                <span class="font-medium">{{ t('settings.manualDeployLog.allServers') }}</span>
              </button>

              <div class="space-y-2">
                <div
                  v-for="item in serverItems"
                  :key="item.id"
                  class="flex min-h-11 w-full items-start gap-1 rounded-xl border p-1 transition-colors duration-200"
                  :class="[selectedServerId === item.id ? 'ring-2 ring-indigo-400/40' : '', statusClass(item.status)]"
                >
                  <button
                    type="button"
                    class="flex min-w-0 flex-1 cursor-pointer items-start gap-2 rounded-lg px-2 py-1 text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500/50"
                    @click="selectedServerId = item.id"
                  >
                    <CheckCircle2 v-if="item.status === 'success'" class="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />
                    <XCircle v-else-if="item.status === 'failed'" class="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />
                    <LoaderCircle v-else-if="['connecting', 'uploading', 'commands'].includes(item.status)" class="mt-0.5 h-4 w-4 shrink-0 animate-spin motion-reduce:animate-none" aria-hidden="true" />
                    <Clock3 v-else class="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />
                    <span class="min-w-0 flex-1">
                      <span class="block truncate text-sm font-semibold">{{ item.name }}</span>
                      <span class="block truncate font-mono text-[11px] opacity-75">{{ item.host || item.id }}</span>
                      <span class="mt-0.5 block text-[11px] font-medium">{{ statusLabel(item.status) }}</span>
                    </span>
                  </button>
                  <button
                    v-if="item.status === 'failed'"
                    type="button"
                    class="inline-flex h-8 w-8 shrink-0 items-center justify-center rounded-lg hover:bg-white/70 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-rose-500/50"
                    :aria-label="t('settings.manualDeployLog.editServer', { name: item.name })"
                    :title="t('settings.manualDeployLog.editServer', { name: item.name })"
                    @click="emit('editServer', item.id)"
                  >
                    <Pencil class="h-4 w-4" aria-hidden="true" />
                  </button>
                </div>
              </div>
            </aside>

            <div class="relative flex min-h-0 flex-col bg-slate-950">
              <div class="flex items-center justify-between border-b border-slate-800 px-4 py-2.5 font-mono text-xs text-slate-400">
                <span>{{ selectedServerId ? t('settings.manualDeployLog.filteredLog') : t('settings.manualDeployLog.allLog') }}</span>
                <span>{{ t('settings.manualDeployLog.lineCount', { count: filteredLogs.length }) }}</span>
              </div>
              <div
                ref="logScrollerRef"
                class="manual-deploy-log-scroll min-h-[300px] flex-1 overflow-y-auto p-4 font-mono text-xs leading-5 lg:min-h-[420px]"
                @scroll="handleLogScroll"
              >
                <div v-if="visibleLogs.length === 0" class="flex min-h-56 items-center justify-center text-slate-500">
                  {{ t('settings.manualDeployLog.waitingForLogs') }}
                </div>
                <div v-for="(log, index) in visibleLogs" v-else :key="`${log.timestamp}-${index}`" class="flex gap-2">
                  <span class="shrink-0 tabular-nums text-slate-500">{{ formatTime(log.timestamp) }}</span>
                  <span v-if="log.server_name" class="shrink-0 text-indigo-300">[{{ log.server_name }}]</span>
                  <span class="whitespace-pre-wrap break-words" :class="logLevelClass(log.level)">{{ log.message }}</span>
                </div>
              </div>
              <button
                v-if="!followingTail"
                type="button"
                class="absolute bottom-4 right-4 inline-flex min-h-11 cursor-pointer items-center gap-2 rounded-xl border border-slate-600 bg-slate-800 px-3 py-2 text-xs font-medium text-white shadow-lg transition-colors duration-200 hover:bg-slate-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-400"
                @click="scrollToLatest"
              >
                <ChevronDown class="h-4 w-4" aria-hidden="true" />
                {{ t('settings.manualDeployLog.latest') }}
              </button>
            </div>
          </div>
        </section>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.manual-deploy-dialog-enter-active,
.manual-deploy-dialog-leave-active { transition: opacity 180ms ease; }
.manual-deploy-dialog-enter-from,
.manual-deploy-dialog-leave-to { opacity: 0; }
.manual-deploy-log-scroll::-webkit-scrollbar { width: 8px; }
.manual-deploy-log-scroll::-webkit-scrollbar-track { background: #020617; }
.manual-deploy-log-scroll::-webkit-scrollbar-thumb { background: #334155; border: 2px solid #020617; border-radius: 999px; }
.manual-deploy-log-scroll::-webkit-scrollbar-thumb:hover { background: #475569; }
@media (prefers-reduced-motion: reduce) {
  .manual-deploy-dialog-enter-active,
  .manual-deploy-dialog-leave-active { transition: none; }
}
</style>
