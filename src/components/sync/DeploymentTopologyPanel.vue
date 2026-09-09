<script setup lang="ts">
import { computed, ref } from 'vue';
import { Activity, Loader2, Network, Play, Search, Square, RotateCcw } from 'lucide-vue-next';
import { useI18n } from 'vue-i18n';
import AppConfirmDialog from '@/components/AppConfirmDialog.vue';
import { pushToast } from '@/composables/useToast';
import {
  cancelTaskRun,
  queryDeploymentTopology,
  startDeploymentTopologyTask,
  type DeploymentTopologyMode,
  type DeploymentTopologyRequest,
  type DeploymentTopologyResult,
} from '@/lib/tauri';
import { taskStateStore } from '@/lib/taskStateStore';

const { t } = useI18n();
const mode = ref<DeploymentTopologyMode>('ha_and_replica');
const primaryIp = ref('');
const haReplicaIp = ref('');
const virtualIp = ref('');
const replicaIpsText = ref('');
const frameworkPassword = ref('admin_123');
const busy = ref(false);
const confirmOpen = ref(false);
const result = ref<DeploymentTopologyResult | null>(null);
const activeHandle = ref<{ task_group_id: string; run_id: string } | null>(null);
const trackedTask = computed(() => activeHandle.value
  ? taskStateStore.groups.find(group => group.task_group_id === activeHandle.value?.task_group_id) ?? null
  : null);
const taskRunning = computed(() => trackedTask.value
  ? ['queued', 'copying', 'copy_completed', 'local_executing', 'deploying'].includes(trackedTask.value.summary_status)
  : false);

const includesHa = computed(() => mode.value === 'ha' || mode.value === 'ha_and_replica');
const includesReplica = computed(() => mode.value === 'replica' || mode.value === 'ha_and_replica');
const replicaIps = computed(() => Array.from(new Set(
  replicaIpsText.value.split(/[\s,，、;；]+/).map(value => value.trim()).filter(Boolean),
)));

const request = computed<DeploymentTopologyRequest>(() => ({
  mode: mode.value,
  primaryIp: primaryIp.value.trim(),
  haReplicaIp: includesHa.value ? haReplicaIp.value.trim() : null,
  virtualIp: includesHa.value ? virtualIp.value.trim() : null,
  replicaIps: includesReplica.value ? replicaIps.value : [],
  frameworkPassword: frameworkPassword.value,
  pollIntervalSecs: 30,
  pollAttempts: 20,
}));

const valid = computed(() => request.value.primaryIp.length > 0
  && request.value.frameworkPassword.length > 0
  && (!includesHa.value || Boolean(request.value.haReplicaIp && request.value.virtualIp))
  && (!includesReplica.value || request.value.replicaIps.length > 0));

async function execute(retry = false) {
  confirmOpen.value = false;
  if (!valid.value || busy.value) return;
  busy.value = true;
  result.value = null;
  try {
    activeHandle.value = await startDeploymentTopologyTask(
      request.value,
      retry ? activeHandle.value?.task_group_id ?? null : null,
    );
    await taskStateStore.hydrateTaskState();
    pushToast(t('settings.topologyTaskStarted'), 'success', { ttlMs: 6000 });
  } catch (error) {
    pushToast(String(error), 'error', { ttlMs: 6000 });
  } finally {
    busy.value = false;
  }
}

async function cancelTopology() {
  if (!activeHandle.value || !taskRunning.value) return;
  await cancelTaskRun(activeHandle.value.task_group_id, activeHandle.value.run_id);
  pushToast(t('settings.topologyCancelRequested'), 'warning');
}

async function query() {
  if (!valid.value || busy.value) return;
  busy.value = true;
  try {
    result.value = await queryDeploymentTopology(request.value);
  } catch (error) {
    pushToast(String(error), 'error', { ttlMs: 6000 });
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <section class="mx-6 mb-6 rounded-xl border border-slate-200 bg-white p-5 shadow-sm">
    <div class="flex flex-wrap items-start justify-between gap-3">
      <div class="flex items-start gap-3">
        <span class="flex h-10 w-10 items-center justify-center rounded-xl bg-blue-50 text-blue-700">
          <Network class="h-5 w-5" />
        </span>
        <div>
          <h2 class="text-base font-semibold text-slate-900">{{ t('settings.topologyTitle') }}</h2>
          <p class="mt-1 text-sm text-slate-500">{{ t('settings.topologyDescription') }}</p>
        </div>
      </div>
      <span class="rounded-full bg-amber-50 px-3 py-1 text-xs font-medium text-amber-800">{{ t('settings.topologyWaitHint') }}</span>
    </div>

    <div class="mt-5 grid gap-4 md:grid-cols-2 xl:grid-cols-3">
      <label class="space-y-1.5 text-sm text-slate-700">
        <span class="font-medium">{{ t('settings.topologyMode') }}</span>
        <select v-model="mode" :disabled="busy" class="w-full rounded-lg border border-slate-300 bg-white px-3 py-2 outline-none focus:ring-2 focus:ring-blue-500/30">
          <option value="ha">{{ t('settings.topologyModeHa') }}</option>
          <option value="replica">{{ t('settings.topologyModeReplica') }}</option>
          <option value="ha_and_replica">{{ t('settings.topologyModeCombined') }}</option>
        </select>
      </label>
      <label class="space-y-1.5 text-sm text-slate-700">
        <span class="font-medium">{{ t('settings.topologyPrimaryIp') }}</span>
        <input v-model="primaryIp" :disabled="busy" class="w-full rounded-lg border border-slate-300 px-3 py-2 font-mono outline-none focus:ring-2 focus:ring-blue-500/30" placeholder="192.115.1.17" />
      </label>
      <label v-if="includesHa" class="space-y-1.5 text-sm text-slate-700">
        <span class="font-medium">{{ t('settings.topologyHaReplicaIp') }}</span>
        <input v-model="haReplicaIp" :disabled="busy" class="w-full rounded-lg border border-slate-300 px-3 py-2 font-mono outline-none focus:ring-2 focus:ring-blue-500/30" placeholder="192.115.1.55" />
      </label>
      <label v-if="includesHa" class="space-y-1.5 text-sm text-slate-700">
        <span class="font-medium">{{ t('settings.topologyVirtualIp') }}</span>
        <input v-model="virtualIp" :disabled="busy" class="w-full rounded-lg border border-slate-300 px-3 py-2 font-mono outline-none focus:ring-2 focus:ring-blue-500/30" placeholder="192.115.1.128" />
      </label>
      <label v-if="includesReplica" class="space-y-1.5 text-sm text-slate-700 md:col-span-2">
        <span class="font-medium">{{ t('settings.topologyReplicaIps') }}</span>
        <textarea v-model="replicaIpsText" :disabled="busy" rows="2" class="w-full resize-y rounded-lg border border-slate-300 px-3 py-2 font-mono outline-none focus:ring-2 focus:ring-blue-500/30" placeholder="192.115.1.18, 192.115.1.19"></textarea>
        <span class="text-xs text-slate-500">{{ t('settings.topologyReplicaCount', { count: replicaIps.length }) }}</span>
      </label>
      <label class="space-y-1.5 text-sm text-slate-700">
        <span class="font-medium">{{ t('settings.topologyFrameworkPassword') }}</span>
        <input v-model="frameworkPassword" type="password" autocomplete="new-password" :disabled="busy" class="w-full rounded-lg border border-slate-300 px-3 py-2 outline-none focus:ring-2 focus:ring-blue-500/30" />
      </label>
    </div>

    <div class="mt-5 flex flex-wrap items-center gap-3 border-t border-slate-100 pt-4">
      <button type="button" :disabled="!valid || busy || taskRunning" class="inline-flex min-h-11 cursor-pointer items-center gap-2 rounded-lg bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700 disabled:cursor-not-allowed disabled:opacity-50" @click="confirmOpen = true">
        <Loader2 v-if="busy || taskRunning" class="h-4 w-4 animate-spin motion-reduce:animate-none" />
        <Play v-else class="h-4 w-4" />
        {{ busy || taskRunning ? t('settings.topologyRunning') : t('settings.topologyExecute') }}
      </button>
      <button type="button" :disabled="!valid || busy || taskRunning" class="inline-flex min-h-11 cursor-pointer items-center gap-2 rounded-lg border border-slate-300 px-4 py-2 text-sm font-medium text-slate-700 hover:bg-slate-50 disabled:opacity-50" @click="query">
        <Search class="h-4 w-4" /> {{ t('settings.topologyQuery') }}
      </button>
      <button v-if="taskRunning" type="button" class="inline-flex min-h-11 cursor-pointer items-center gap-2 rounded-lg border border-rose-200 bg-rose-50 px-4 py-2 text-sm font-medium text-rose-700 hover:bg-rose-100" @click="cancelTopology">
        <Square class="h-4 w-4" /> {{ t('common.cancel') }}
      </button>
      <button v-else-if="trackedTask?.had_failures" type="button" :disabled="!valid || busy" class="inline-flex min-h-11 cursor-pointer items-center gap-2 rounded-lg border border-amber-200 bg-amber-50 px-4 py-2 text-sm font-medium text-amber-700 hover:bg-amber-100" @click="execute(true)">
        <RotateCcw class="h-4 w-4" /> {{ t('settings.topologyRetry') }}
      </button>
      <p class="text-xs text-slate-500">{{ t('settings.topologyDisconnectHint') }}</p>
    </div>

    <div v-if="trackedTask" class="mt-3 rounded-lg border border-indigo-100 bg-indigo-50/70 px-4 py-3 text-sm text-indigo-900" role="status" aria-live="polite">
      <div class="font-semibold">{{ t('settings.topologyTaskRecord') }}</div>
      <div class="mt-1 font-mono text-xs">{{ trackedTask.task_group_id }} · {{ trackedTask.summary_status }}</div>
    </div>

    <div v-if="result" class="mt-4 rounded-lg border p-4" :class="result.status === 'success' ? 'border-emerald-200 bg-emerald-50' : result.status === 'unconfirmed' ? 'border-amber-200 bg-amber-50' : 'border-rose-200 bg-rose-50'">
      <div class="flex items-center gap-2 text-sm font-medium text-slate-900">
        <Activity class="h-4 w-4" />
        {{ result.message }}
      </div>
      <p class="mt-1 text-xs text-slate-600">{{ t('settings.topologyObserved', { observed: result.observedServerCount, expected: result.expectedServerCount }) }}</p>
      <div v-if="result.servers.length" class="mt-3 grid gap-2 md:grid-cols-2 xl:grid-cols-3">
        <div v-for="server in result.servers" :key="server.serverIp" class="rounded-lg border border-white/80 bg-white/80 p-3 text-xs text-slate-600">
          <div class="font-mono font-semibold text-slate-900">{{ server.serverIp }}</div>
          <div class="mt-1">{{ server.serverName }} · haType={{ server.haType }}</div>
        </div>
      </div>
    </div>

    <AppConfirmDialog
      :open="confirmOpen"
      :title="t('settings.topologyConfirmTitle')"
      :description="t('settings.topologyConfirmDescription')"
      :confirm-label="t('settings.topologyExecute')"
      :cancel-label="t('common.cancel')"
      tone="warning"
      :busy="busy"
      @confirm="execute(false)"
      @cancel="confirmOpen = false"
    />
  </section>
</template>
