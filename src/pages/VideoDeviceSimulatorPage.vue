<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import {
  Activity,
  AlertTriangle,
  BellRing,
  CheckCircle2,
  Clipboard,
  Download,
  FileDown,
  LoaderCircle,
  Plus,
  RadioTower,
  RefreshCw,
  RotateCcw,
  Server,
  ShieldAlert,
  Square,
  Trash2,
  Video,
  XCircle,
} from 'lucide-vue-next';

import { useDeviceSimulator } from '@/composables/useDeviceSimulator';
import { isDeviceSimulatorRuntimeActive, type AlarmJobRequest } from '@/lib/deviceSimulator';

const { t } = useI18n();
const simulator = useDeviceSimulator();
const activeTab = ref<'configuration' | 'runtime' | 'alarms' | 'logs'>('configuration');
const logLevel = ref('all');
const logQuery = ref('');
const copiedValue = ref('');
const continuousAlarm = ref(false);

const alarm = reactive<AlarmJobRequest>({
  target_device_ids: [],
  alarm_profile_id: 'default',
  alarm_type_ids: [],
  mode: 'sequential',
  interval_ms: 1_000,
  send_count: 1,
  recovery_delay_secs: null,
  image_variant: null,
  user_image_id: null,
});
const alarmTypesText = ref('');

const fieldClass = 'min-h-11 w-full rounded-xl border border-slate-300 bg-white px-3 py-2 text-sm text-slate-800 shadow-sm transition-colors placeholder:text-slate-400 focus-visible:border-sky-500 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-500/25 disabled:cursor-not-allowed disabled:bg-slate-100 disabled:text-slate-500';
const buttonFocus = 'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-500/45 focus-visible:ring-offset-2';

const running = computed(() => isDeviceSimulatorRuntimeActive(simulator.status.value.state));
const profilesById = computed(() => new Map(simulator.profiles.value.map((profile) => [profile.id, profile])));
const visibleDevices = computed(() => simulator.preview.value?.devices.slice(0, 100) ?? []);
const allStreamAddresses = computed(() => simulator.preview.value?.devices.flatMap((device) => device.streams) ?? []);
const filteredLogs = computed(() => simulator.logs.value.filter((entry) => {
  if (logLevel.value !== 'all' && entry.level !== logLevel.value) return false;
  const query = logQuery.value.trim().toLowerCase();
  if (!query) return true;
  return [entry.message, entry.component, entry.device_ip, entry.error_code]
    .some((value) => value?.toLowerCase().includes(query));
}));
const assetTone = computed(() => {
  const state = simulator.assets.value?.state ?? 'unknown';
  if (state === 'ready' || state === 'update_available') return 'ready';
  if (state === 'failed') return 'error';
  return 'attention';
});

watch(
  () => simulator.request.groups.map((group) => group.profile_id).join(','),
  () => {
    if (simulator.busyAction.value === null) void simulator.refreshAssets();
  },
);

onMounted(() => simulator.initialize());
onBeforeUnmount(() => simulator.dispose());

function addServer() {
  if (simulator.topologyLocked.value) return;
  simulator.request.platform.servers.push({
    id: `server-${Date.now()}`,
    host: '',
    port: simulator.request.platform.kind === 'vms' ? 80 : 80,
  });
}

function removeServer(id: string) {
  if (simulator.topologyLocked.value) return;
  simulator.request.platform.servers = simulator.request.platform.servers.filter((server) => server.id !== id);
}

function profileLabel(profileId: string) {
  const profile = profilesById.value.get(profileId);
  return profile ? t(profile.display_name_key) : t(`deviceSimulator.profiles.${profileId}`);
}

function statusLabel(state: string) {
  return t(`deviceSimulator.states.${state}`);
}

async function copyText(value: string) {
  await navigator.clipboard.writeText(value);
  copiedValue.value = value;
  window.setTimeout(() => {
    if (copiedValue.value === value) copiedValue.value = '';
  }, 1_600);
}

function downloadJson(filename: string, value: unknown) {
  const blob = new Blob([JSON.stringify(value, null, 2)], { type: 'application/json' });
  const link = document.createElement('a');
  link.href = URL.createObjectURL(blob);
  link.download = filename;
  link.click();
  URL.revokeObjectURL(link.href);
}

function syncAlarmTypes() {
  alarm.alarm_type_ids = alarmTypesText.value
    .split(',')
    .map((value) => value.trim())
    .filter(Boolean);
  if (alarm.target_device_ids.length === 0) {
    alarm.target_device_ids = simulator.preview.value?.devices.map((device) => device.device_id) ?? [];
  }
}

function alarmRequest(): AlarmJobRequest {
  syncAlarmTypes();
  const sendCount = Number(alarm.send_count);
  const recoveryDelay = Number(alarm.recovery_delay_secs);
  return {
    ...alarm,
    target_device_ids: [...alarm.target_device_ids],
    alarm_type_ids: [...alarm.alarm_type_ids],
    interval_ms: Math.max(100, Number(alarm.interval_ms) || 1_000),
    send_count: continuousAlarm.value ? null : Math.max(1, sendCount || 1),
    recovery_delay_secs: Number.isFinite(recoveryDelay) && recoveryDelay >= 0
      ? recoveryDelay
      : null,
  };
}

async function triggerAlarm() {
  await simulator.triggerAlarm(alarmRequest());
}

async function startAlarm() {
  await simulator.startAlarm(alarmRequest());
}
</script>

<template>
  <main class="h-full min-w-0 overflow-y-auto bg-slate-50" :aria-busy="simulator.busyAction.value !== null">
    <div class="mx-auto w-full max-w-[1600px] space-y-5 p-4 sm:p-6 lg:p-8">
      <header class="rounded-3xl border border-slate-200 bg-white p-5 shadow-sm sm:p-6">
        <div class="flex flex-col gap-4 lg:flex-row lg:items-center lg:justify-between">
          <div class="flex min-w-0 items-start gap-4">
            <div class="flex h-12 w-12 shrink-0 items-center justify-center rounded-2xl bg-gradient-to-br from-sky-600 to-cyan-500 text-white shadow-lg shadow-sky-500/20">
              <Video class="h-6 w-6" aria-hidden="true" />
            </div>
            <div class="min-w-0">
              <p class="text-xs font-bold uppercase tracking-[0.16em] text-sky-700">{{ t('deviceSimulator.eyebrow') }}</p>
              <h1 class="mt-1 text-2xl font-bold tracking-tight text-slate-900">{{ t('deviceSimulator.title') }}</h1>
              <p class="mt-1 max-w-3xl text-sm leading-6 text-slate-600">{{ t('deviceSimulator.description') }}</p>
            </div>
          </div>
          <div class="flex flex-wrap items-center gap-2">
            <span
              class="inline-flex min-h-11 items-center gap-2 rounded-xl border px-3 py-2 text-sm font-semibold"
              :class="running ? 'border-emerald-200 bg-emerald-50 text-emerald-800' : 'border-slate-200 bg-slate-50 text-slate-700'"
            >
              <span class="h-2.5 w-2.5 rounded-full" :class="running ? 'bg-emerald-500' : 'bg-slate-400'" aria-hidden="true"></span>
              {{ statusLabel(simulator.status.value.state) }}
            </span>
            <button
              type="button"
              class="inline-flex min-h-11 cursor-pointer items-center gap-2 rounded-xl border border-slate-300 bg-white px-4 py-2 text-sm font-semibold text-slate-700 transition-colors hover:bg-slate-50 disabled:cursor-not-allowed disabled:opacity-60"
              :class="buttonFocus"
              :disabled="simulator.busyAction.value !== null"
              @click="simulator.initialize"
            >
              <RefreshCw class="h-4 w-4" :class="simulator.busyAction.value === 'initialize' ? 'animate-spin motion-reduce:animate-none' : ''" aria-hidden="true" />
              {{ t('common.refresh') }}
            </button>
          </div>
        </div>
      </header>

      <section
        class="rounded-2xl border p-4 sm:p-5"
        :class="assetTone === 'ready'
          ? 'border-emerald-200 bg-emerald-50/80'
          : assetTone === 'error'
            ? 'border-rose-200 bg-rose-50/80'
            : 'border-amber-200 bg-amber-50/80'"
        aria-labelledby="asset-banner-title"
      >
        <div class="flex flex-col gap-4 lg:flex-row lg:items-center lg:justify-between">
          <div class="flex min-w-0 items-start gap-3">
            <CheckCircle2 v-if="assetTone === 'ready'" class="mt-0.5 h-5 w-5 shrink-0 text-emerald-700" aria-hidden="true" />
            <XCircle v-else-if="assetTone === 'error'" class="mt-0.5 h-5 w-5 shrink-0 text-rose-700" aria-hidden="true" />
            <AlertTriangle v-else class="mt-0.5 h-5 w-5 shrink-0 text-amber-700" aria-hidden="true" />
            <div>
              <h2 id="asset-banner-title" class="font-bold text-slate-900">{{ t('deviceSimulator.assets.title') }}</h2>
              <p class="mt-1 text-sm leading-6 text-slate-700">
                {{ t(`deviceSimulator.assets.states.${simulator.assets.value?.state ?? 'unknown'}`) }}
              </p>
              <p v-if="simulator.assetProgress.value" class="mt-1 text-xs text-slate-600">
                {{ simulator.assetProgress.value.current_pack_id ?? t('deviceSimulator.assets.catalog') }} ·
                {{ simulator.assetProgress.value.downloaded.toLocaleString() }} /
                {{ simulator.assetProgress.value.total?.toLocaleString() ?? '—' }} B
              </p>
              <ul v-if="simulator.assets.value?.packs.length" class="mt-2 space-y-1 text-xs text-slate-600">
                <li v-for="pack in simulator.assets.value.packs" :key="pack.id" class="flex flex-wrap gap-x-2">
                  <span class="font-semibold text-slate-700">{{ pack.id }}</span>
                  <span>{{ pack.installed_version ?? '—' }} / {{ pack.required_version }}</span>
                  <span v-if="pack.error_code" class="font-mono text-rose-700">{{ pack.error_code }}</span>
                </li>
              </ul>
            </div>
          </div>
          <div class="flex flex-wrap gap-2">
            <button type="button" class="inline-flex min-h-11 cursor-pointer items-center gap-2 rounded-xl border border-slate-300 bg-white px-4 py-2 text-sm font-semibold text-slate-700 hover:bg-slate-50 disabled:cursor-not-allowed disabled:opacity-60" :class="buttonFocus" :disabled="simulator.busyAction.value !== null" @click="simulator.refreshAssets">
              <RefreshCw class="h-4 w-4" aria-hidden="true" />{{ t('deviceSimulator.actions.checkAssets') }}
            </button>
            <button type="button" class="inline-flex min-h-11 cursor-pointer items-center gap-2 rounded-xl bg-sky-600 px-4 py-2 text-sm font-semibold text-white shadow-sm hover:bg-sky-700 disabled:cursor-not-allowed disabled:opacity-60" :class="buttonFocus" :disabled="simulator.busyAction.value !== null" @click="simulator.prepareAssets">
              <Download class="h-4 w-4" aria-hidden="true" />{{ t('deviceSimulator.actions.downloadAssets') }}
            </button>
            <button v-if="simulator.assetProgress.value?.state === 'downloading'" type="button" class="inline-flex min-h-11 cursor-pointer items-center gap-2 rounded-xl border border-rose-300 bg-white px-4 py-2 text-sm font-semibold text-rose-700 hover:bg-rose-50" :class="buttonFocus" @click="simulator.cancelAssetDownload">
              <Square class="h-4 w-4" aria-hidden="true" />{{ t('common.cancel') }}
            </button>
          </div>
        </div>
      </section>

      <section v-if="simulator.recoverySessionId.value" class="rounded-2xl border border-rose-300 bg-rose-50 p-5" aria-labelledby="recovery-title">
        <div class="flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between">
          <div class="flex items-start gap-3">
            <ShieldAlert class="mt-0.5 h-6 w-6 shrink-0 text-rose-700" aria-hidden="true" />
            <div>
              <h2 id="recovery-title" class="font-bold text-rose-950">{{ t('deviceSimulator.recovery.title') }}</h2>
              <p class="mt-1 text-sm leading-6 text-rose-800">{{ t('deviceSimulator.recovery.description', { id: simulator.recoverySessionId.value }) }}</p>
            </div>
          </div>
          <button type="button" class="inline-flex min-h-11 cursor-pointer items-center justify-center gap-2 rounded-xl bg-rose-700 px-4 py-2 text-sm font-semibold text-white hover:bg-rose-800 disabled:cursor-not-allowed disabled:opacity-60" :class="buttonFocus" :disabled="simulator.busyAction.value !== null" @click="simulator.recover">
            <RotateCcw class="h-4 w-4" aria-hidden="true" />{{ t('deviceSimulator.actions.recover') }}
          </button>
        </div>
      </section>

      <div v-if="simulator.errorMessage.value" role="alert" class="flex items-start gap-3 rounded-2xl border border-rose-200 bg-rose-50 p-4 text-sm text-rose-800">
        <XCircle class="mt-0.5 h-5 w-5 shrink-0" aria-hidden="true" />
        <div class="min-w-0"><strong>{{ t('deviceSimulator.errors.title') }}</strong><p class="mt-1 break-words">{{ simulator.errorMessage.value }}</p></div>
      </div>

      <nav class="flex gap-1 overflow-x-auto rounded-2xl border border-slate-200 bg-white p-1.5 shadow-sm" :aria-label="t('deviceSimulator.tabs.label')">
        <button
          v-for="tab in ['configuration', 'runtime', 'alarms', 'logs'] as const"
          :key="tab"
          type="button"
          class="min-h-11 shrink-0 cursor-pointer rounded-xl px-4 py-2 text-sm font-semibold transition-colors"
          :class="[buttonFocus, activeTab === tab ? 'bg-slate-900 text-white' : 'text-slate-600 hover:bg-slate-100 hover:text-slate-900']"
          :aria-current="activeTab === tab ? 'page' : undefined"
          @click="activeTab = tab"
        >
          {{ t(`deviceSimulator.tabs.${tab}`) }}
        </button>
      </nav>

      <template v-if="activeTab === 'configuration'">
        <fieldset :disabled="simulator.topologyLocked.value" class="grid gap-5 xl:grid-cols-[minmax(0,1.05fr)_minmax(24rem,.95fr)]">
          <div class="space-y-5">
            <section class="rounded-2xl border border-slate-200 bg-white p-5 shadow-sm" aria-labelledby="platform-title">
              <div class="flex items-center gap-3"><Server class="h-5 w-5 text-sky-700" aria-hidden="true" /><h2 id="platform-title" class="font-bold text-slate-900">{{ t('deviceSimulator.configuration.platform') }}</h2></div>
              <div class="mt-5 grid gap-4 md:grid-cols-2">
                <label class="block text-sm font-semibold text-slate-700">{{ t('deviceSimulator.fields.platform') }}
                  <select v-model="simulator.request.platform.kind" :class="[fieldClass, 'mt-2']"><option value="vms">VMS</option><option value="ums">UMS</option></select>
                </label>
                <label class="block text-sm font-semibold text-slate-700">{{ t('deviceSimulator.fields.alarmReceiver') }}
                  <input v-model="simulator.request.platform.alarm_receiver_url" :class="[fieldClass, 'mt-2']" type="url" placeholder="http://192.168.1.10/alarm" />
                </label>
              </div>
              <div class="mt-4 space-y-3">
                <div v-for="serverItem in simulator.request.platform.servers" :key="serverItem.id" class="grid gap-3 rounded-xl border border-slate-200 bg-slate-50 p-3 sm:grid-cols-[1fr_8rem_2.75rem]">
                  <label class="text-xs font-semibold text-slate-600">{{ t('deviceSimulator.fields.serverHost') }}<input v-model="serverItem.host" :class="[fieldClass, 'mt-1']" type="text" /></label>
                  <label class="text-xs font-semibold text-slate-600">{{ t('deviceSimulator.fields.port') }}<input v-model.number="serverItem.port" :class="[fieldClass, 'mt-1']" type="number" min="1" max="65535" inputmode="numeric" /></label>
                  <button type="button" class="mt-5 inline-flex min-h-11 cursor-pointer items-center justify-center rounded-xl text-rose-700 hover:bg-rose-100" :class="buttonFocus" :aria-label="t('deviceSimulator.actions.removeServer')" @click="removeServer(serverItem.id)"><Trash2 class="h-5 w-5" aria-hidden="true" /></button>
                </div>
                <button type="button" class="inline-flex min-h-11 cursor-pointer items-center gap-2 rounded-xl border border-dashed border-slate-300 px-4 py-2 text-sm font-semibold text-slate-700 hover:border-sky-400 hover:bg-sky-50" :class="buttonFocus" @click="addServer"><Plus class="h-4 w-4" aria-hidden="true" />{{ t('deviceSimulator.actions.addServer') }}</button>
              </div>
            </section>

            <section class="rounded-2xl border border-slate-200 bg-white p-5 shadow-sm" aria-labelledby="network-title">
              <div class="flex items-center gap-3"><RadioTower class="h-5 w-5 text-sky-700" aria-hidden="true" /><h2 id="network-title" class="font-bold text-slate-900">{{ t('deviceSimulator.configuration.network') }}</h2></div>
              <div class="mt-5 grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
                <label class="block text-sm font-semibold text-slate-700 sm:col-span-2 lg:col-span-3">{{ t('deviceSimulator.fields.interface') }}
                  <select v-model="simulator.request.interface_id" :class="[fieldClass, 'mt-2']"><option value="">{{ t('deviceSimulator.fields.selectInterface') }}</option><option v-for="item in simulator.interfaces.value" :key="item.id" :value="item.id">{{ item.name }} · {{ item.description }}</option></select>
                </label>
                <label class="block text-sm font-semibold text-slate-700">{{ t('deviceSimulator.fields.startIp') }}<input v-model="simulator.request.start_ip" :class="[fieldClass, 'mt-2']" type="text" inputmode="decimal" /></label>
                <label class="block text-sm font-semibold text-slate-700">{{ t('deviceSimulator.fields.prefix') }}<input v-model.number="simulator.request.subnet_prefix" :class="[fieldClass, 'mt-2']" type="number" min="1" max="30" inputmode="numeric" /></label>
                <label class="block text-sm font-semibold text-slate-700">{{ t('deviceSimulator.fields.httpPort') }}<input v-model.number="simulator.request.device_http_port" :class="[fieldClass, 'mt-2']" type="number" min="1" max="65535" inputmode="numeric" /></label>
                <label v-for="stream in ['main', 'sub', 'third'] as const" :key="stream" class="block text-sm font-semibold text-slate-700">{{ t(`deviceSimulator.fields.rtsp.${stream}`) }}<input v-model.number="simulator.request.rtsp_ports[stream]" :class="[fieldClass, 'mt-2']" type="number" min="1" max="65535" inputmode="numeric" /></label>
              </div>
            </section>

            <section class="rounded-2xl border border-slate-200 bg-white p-5 shadow-sm" aria-labelledby="groups-title">
              <div class="flex flex-wrap items-center justify-between gap-3"><div class="flex items-center gap-3"><Video class="h-5 w-5 text-sky-700" aria-hidden="true" /><h2 id="groups-title" class="font-bold text-slate-900">{{ t('deviceSimulator.configuration.groups') }}</h2></div><button type="button" class="inline-flex min-h-11 cursor-pointer items-center gap-2 rounded-xl border border-slate-300 px-4 py-2 text-sm font-semibold text-slate-700 hover:bg-slate-50" :class="buttonFocus" @click="simulator.addGroup()"><Plus class="h-4 w-4" aria-hidden="true" />{{ t('deviceSimulator.actions.addGroup') }}</button></div>
              <div class="mt-4 space-y-3">
                <article v-for="group in simulator.request.groups" :key="group.id" class="grid gap-3 rounded-xl border border-slate-200 bg-slate-50 p-4 md:grid-cols-[minmax(12rem,1fr)_8rem_9rem_2.75rem]">
                  <label class="text-xs font-semibold text-slate-600">{{ t('deviceSimulator.fields.profile') }}<select :value="group.profile_id" :class="[fieldClass, 'mt-1']" @change="simulator.updateGroupProfile(group, ($event.target as HTMLSelectElement).value)"><option v-for="profile in simulator.profiles.value" :key="profile.id" :value="profile.id">{{ profileLabel(profile.id) }}</option><template v-if="simulator.profiles.value.length === 0"><option v-for="id in ['ipc-custom', 'ipc-smart', 'nvr-common', 'nvr-vehicle']" :key="id" :value="id">{{ profileLabel(id) }}</option></template></select></label>
                  <label class="text-xs font-semibold text-slate-600">{{ t('deviceSimulator.fields.count') }}<input v-model.number="group.count" :class="[fieldClass, 'mt-1']" type="number" min="1" max="500" inputmode="numeric" /></label>
                  <label class="text-xs font-semibold text-slate-600">{{ t('deviceSimulator.fields.channels') }}<input v-model.number="group.nvr_channel_count" :class="[fieldClass, 'mt-1']" type="number" min="1" max="128" inputmode="numeric" :disabled="!group.profile_id.startsWith('nvr-')" /></label>
                  <button type="button" class="mt-5 inline-flex min-h-11 cursor-pointer items-center justify-center rounded-xl text-rose-700 hover:bg-rose-100 disabled:cursor-not-allowed disabled:opacity-40" :class="buttonFocus" :disabled="simulator.request.groups.length <= 1" :aria-label="t('deviceSimulator.actions.removeGroup')" @click="simulator.removeGroup(group.id)"><Trash2 class="h-5 w-5" aria-hidden="true" /></button>
                </article>
              </div>
            </section>
          </div>

          <div class="space-y-5">
            <section class="rounded-2xl border border-slate-200 bg-white p-5 shadow-sm xl:sticky xl:top-5" aria-labelledby="preflight-title">
              <h2 id="preflight-title" class="font-bold text-slate-900">{{ t('deviceSimulator.preflight.title') }}</h2>
              <p class="mt-1 text-sm leading-6 text-slate-600">{{ t('deviceSimulator.preflight.description') }}</p>
              <div class="mt-4 flex flex-wrap gap-2">
                <button type="button" class="inline-flex min-h-11 cursor-pointer items-center gap-2 rounded-xl border border-slate-300 px-4 py-2 text-sm font-semibold text-slate-700 hover:bg-slate-50 disabled:cursor-not-allowed disabled:opacity-60" :class="buttonFocus" :disabled="simulator.busyAction.value !== null" @click="simulator.previewDevices"><Activity class="h-4 w-4" aria-hidden="true" />{{ t('deviceSimulator.actions.preview') }}</button>
                <button type="button" class="inline-flex min-h-11 cursor-pointer items-center gap-2 rounded-xl bg-sky-600 px-4 py-2 text-sm font-semibold text-white hover:bg-sky-700 disabled:cursor-not-allowed disabled:opacity-60" :class="buttonFocus" :disabled="simulator.busyAction.value !== null" @click="simulator.runPreflight"><ShieldAlert class="h-4 w-4" aria-hidden="true" />{{ t('deviceSimulator.actions.preflight') }}</button>
              </div>
              <ul v-if="simulator.preflight.value" class="mt-4 space-y-2">
                <li v-for="check in simulator.preflight.value.checks" :key="check.id" class="flex items-start gap-2 rounded-xl border p-3 text-sm" :class="check.status === 'failed' ? 'border-rose-200 bg-rose-50 text-rose-800' : check.status === 'warning' ? 'border-amber-200 bg-amber-50 text-amber-800' : 'border-emerald-200 bg-emerald-50 text-emerald-800'">
                  <XCircle v-if="check.status === 'failed'" class="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" /><AlertTriangle v-else-if="check.status === 'warning'" class="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" /><CheckCircle2 v-else class="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />
                  <div><strong>{{ t(check.message_key) }}</strong><p v-if="check.details" class="mt-1 break-words">{{ check.details }}</p></div>
                </li>
              </ul>
              <div v-if="simulator.preview.value" class="mt-5 grid grid-cols-2 gap-3">
                <div class="rounded-xl bg-slate-100 p-3"><p class="text-xs font-semibold text-slate-500">{{ t('deviceSimulator.metrics.devices') }}</p><p class="mt-1 text-2xl font-bold text-slate-900">{{ simulator.preview.value.total_devices }}</p></div>
                <div class="rounded-xl bg-slate-100 p-3"><p class="text-xs font-semibold text-slate-500">{{ t('deviceSimulator.metrics.channels') }}</p><p class="mt-1 text-2xl font-bold text-slate-900">{{ simulator.preview.value.total_channels }}</p></div>
              </div>
              <div class="mt-5 grid gap-2 sm:grid-cols-2 xl:grid-cols-1 2xl:grid-cols-2">
                <button v-if="!running" type="button" class="inline-flex min-h-12 cursor-pointer items-center justify-center gap-2 rounded-xl bg-emerald-600 px-4 py-2 text-sm font-bold text-white shadow-sm hover:bg-emerald-700 disabled:cursor-not-allowed disabled:opacity-60" :class="buttonFocus" :disabled="simulator.busyAction.value !== null || simulator.blockingPreflight.value" @click="simulator.start"><LoaderCircle v-if="simulator.busyAction.value === 'start'" class="h-4 w-4 animate-spin motion-reduce:animate-none" aria-hidden="true" /><RadioTower v-else class="h-4 w-4" aria-hidden="true" />{{ t('deviceSimulator.actions.start') }}</button>
                <button v-else type="button" class="inline-flex min-h-12 cursor-pointer items-center justify-center gap-2 rounded-xl bg-rose-700 px-4 py-2 text-sm font-bold text-white hover:bg-rose-800 disabled:cursor-not-allowed disabled:opacity-60" :class="buttonFocus" :disabled="simulator.busyAction.value !== null" @click="simulator.stop"><Square class="h-4 w-4" aria-hidden="true" />{{ t('deviceSimulator.actions.stop') }}</button>
                <button type="button" class="inline-flex min-h-12 cursor-pointer items-center justify-center gap-2 rounded-xl border border-slate-300 px-4 py-2 text-sm font-semibold text-slate-700 hover:bg-slate-50 disabled:cursor-not-allowed disabled:opacity-60" :class="buttonFocus" :disabled="simulator.busyAction.value !== null || simulator.topologyLocked.value" @click="simulator.saveSettings">{{ t('common.save') }}</button>
              </div>
              <p v-if="simulator.topologyLocked.value" class="mt-3 flex items-start gap-2 text-xs leading-5 text-amber-800"><AlertTriangle class="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />{{ t('deviceSimulator.configuration.locked') }}</p>
            </section>
          </div>
        </fieldset>
      </template>

      <template v-else-if="activeTab === 'runtime'">
        <section class="grid gap-4 sm:grid-cols-2 xl:grid-cols-4" aria-label="Runtime metrics">
          <div v-for="metric in [
            ['devices', `${simulator.status.value.metrics.online_devices}/${simulator.status.value.metrics.total_devices}`],
            ['channels', simulator.status.value.metrics.total_channels],
            ['clients', simulator.rtspStats.value?.active_clients ?? simulator.status.value.metrics.active_rtsp_clients],
            ['bitrate', `${simulator.rtspStats.value?.bitrate_kbps ?? simulator.status.value.metrics.outbound_bitrate_kbps} kbps`],
          ]" :key="metric[0]" class="rounded-2xl border border-slate-200 bg-white p-5 shadow-sm"><p class="text-xs font-bold uppercase tracking-wider text-slate-500">{{ t(`deviceSimulator.metrics.${metric[0]}`) }}</p><p class="mt-2 text-2xl font-bold text-slate-900">{{ metric[1] }}</p></div>
        </section>
        <section class="mt-5 rounded-2xl border border-slate-200 bg-white shadow-sm" aria-labelledby="streams-title">
          <div class="flex flex-wrap items-center justify-between gap-3 border-b border-slate-200 p-5"><div><h2 id="streams-title" class="font-bold text-slate-900">{{ t('deviceSimulator.runtime.streams') }}</h2><p class="mt-1 text-sm text-slate-600">{{ t('deviceSimulator.runtime.streamsDescription') }}</p></div><button type="button" class="inline-flex min-h-11 cursor-pointer items-center gap-2 rounded-xl border border-slate-300 px-4 py-2 text-sm font-semibold text-slate-700 hover:bg-slate-50 disabled:opacity-50" :class="buttonFocus" :disabled="allStreamAddresses.length === 0" @click="downloadJson('device-simulator-streams.json', allStreamAddresses)"><FileDown class="h-4 w-4" aria-hidden="true" />{{ t('common.export') }}</button></div>
          <div class="max-h-[32rem] overflow-auto">
            <table class="min-w-full text-left text-sm"><thead class="sticky top-0 bg-slate-50 text-xs uppercase tracking-wide text-slate-500"><tr><th class="px-5 py-3">{{ t('deviceSimulator.fields.device') }}</th><th class="px-5 py-3">{{ t('deviceSimulator.fields.stream') }}</th><th class="px-5 py-3">URL</th><th class="px-5 py-3"><span class="sr-only">{{ t('common.actions') }}</span></th></tr></thead><tbody class="divide-y divide-slate-100"><tr v-for="stream in allStreamAddresses" :key="`${stream.device_id}-${stream.channel_id}-${stream.stream}`"><td class="whitespace-nowrap px-5 py-3 font-medium text-slate-800">{{ stream.device_id }}<span v-if="stream.channel_id" class="ml-1 text-slate-500">/ {{ stream.channel_id }}</span></td><td class="px-5 py-3 text-slate-600">{{ stream.stream }}</td><td class="max-w-xl truncate px-5 py-3 font-mono text-xs text-slate-600" :title="stream.url">{{ stream.url }}</td><td class="px-5 py-3"><button type="button" class="inline-flex min-h-11 min-w-11 cursor-pointer items-center justify-center rounded-xl text-sky-700 hover:bg-sky-50" :class="buttonFocus" :aria-label="t('deviceSimulator.actions.copyUrl')" @click="copyText(stream.url)"><CheckCircle2 v-if="copiedValue === stream.url" class="h-4 w-4 text-emerald-600" aria-hidden="true" /><Clipboard v-else class="h-4 w-4" aria-hidden="true" /></button></td></tr><tr v-if="allStreamAddresses.length === 0"><td colspan="4" class="px-5 py-12 text-center text-slate-500">{{ t('deviceSimulator.runtime.noStreams') }}</td></tr></tbody></table>
          </div>
        </section>
        <section class="mt-5 rounded-2xl border border-slate-200 bg-white shadow-sm" aria-labelledby="identity-title"><div class="flex items-center justify-between border-b border-slate-200 p-5"><h2 id="identity-title" class="font-bold text-slate-900">{{ t('deviceSimulator.runtime.identities') }}</h2><button type="button" class="inline-flex min-h-11 cursor-pointer items-center gap-2 rounded-xl border border-slate-300 px-4 py-2 text-sm font-semibold text-slate-700 hover:bg-slate-50 disabled:opacity-50" :class="buttonFocus" :disabled="!simulator.preview.value" @click="downloadJson('device-simulator-identities.json', simulator.preview.value)"><FileDown class="h-4 w-4" aria-hidden="true" />{{ t('common.export') }}</button></div><div class="max-h-96 overflow-auto"><table class="min-w-full text-left text-sm"><thead class="sticky top-0 bg-slate-50 text-xs uppercase text-slate-500"><tr><th class="px-5 py-3">ID</th><th class="px-5 py-3">IP</th><th class="px-5 py-3">MAC</th><th class="px-5 py-3">{{ t('deviceSimulator.fields.profile') }}</th></tr></thead><tbody class="divide-y divide-slate-100"><tr v-for="device in visibleDevices" :key="device.device_id"><td class="px-5 py-3 font-medium text-slate-800">{{ device.device_id }}</td><td class="px-5 py-3 font-mono text-xs text-slate-600">{{ device.ip }}</td><td class="px-5 py-3 font-mono text-xs text-slate-600">{{ device.mac }}</td><td class="px-5 py-3 text-slate-600">{{ profileLabel(device.profile_id) }}</td></tr></tbody></table></div></section>
      </template>

      <template v-else-if="activeTab === 'alarms'">
        <div class="grid gap-5 xl:grid-cols-[minmax(0,1fr)_24rem]">
          <section class="rounded-2xl border border-slate-200 bg-white p-5 shadow-sm" aria-labelledby="alarm-title"><div class="flex items-center gap-3"><BellRing class="h-5 w-5 text-amber-600" aria-hidden="true" /><h2 id="alarm-title" class="font-bold text-slate-900">{{ t('deviceSimulator.alarms.title') }}</h2></div><p class="mt-2 text-sm leading-6 text-slate-600">{{ t('deviceSimulator.alarms.description') }}</p><div class="mt-5 grid gap-4 sm:grid-cols-2"><label class="text-sm font-semibold text-slate-700">{{ t('deviceSimulator.fields.alarmProfile') }}<input v-model="alarm.alarm_profile_id" :class="[fieldClass, 'mt-2']" type="text" /></label><label class="text-sm font-semibold text-slate-700">{{ t('deviceSimulator.fields.alarmTypes') }}<input v-model="alarmTypesText" :class="[fieldClass, 'mt-2']" type="text" :placeholder="t('deviceSimulator.alarms.typesPlaceholder')" /></label><label class="text-sm font-semibold text-slate-700">{{ t('deviceSimulator.fields.dispatchMode') }}<select v-model="alarm.mode" :class="[fieldClass, 'mt-2']"><option value="sequential">{{ t('deviceSimulator.alarms.sequential') }}</option><option value="random">{{ t('deviceSimulator.alarms.random') }}</option><option value="configured">{{ t('deviceSimulator.alarms.configured') }}</option></select></label><label class="text-sm font-semibold text-slate-700">{{ t('deviceSimulator.fields.interval') }}<input v-model.number="alarm.interval_ms" :class="[fieldClass, 'mt-2']" type="number" min="100" max="3600000" inputmode="numeric" /></label><label class="text-sm font-semibold text-slate-700">{{ t('deviceSimulator.fields.sendCount') }}<input v-model.number="alarm.send_count" :class="[fieldClass, 'mt-2']" type="number" min="1" max="100000" inputmode="numeric" :disabled="continuousAlarm" /></label><label class="text-sm font-semibold text-slate-700">{{ t('deviceSimulator.fields.recoveryDelay') }}<input v-model.number="alarm.recovery_delay_secs" :class="[fieldClass, 'mt-2']" type="number" min="0" max="86400" inputmode="numeric" /></label><label class="flex min-h-11 cursor-pointer items-center gap-3 rounded-xl border border-slate-200 bg-slate-50 px-3 py-2 text-sm font-semibold text-slate-700"><input v-model="continuousAlarm" type="checkbox" class="h-4 w-4 rounded border-slate-300 text-sky-600 focus-visible:ring-2 focus-visible:ring-sky-500/45" /><span><span class="block">{{ t('deviceSimulator.fields.continuous') }}</span><span class="block text-xs font-normal text-slate-500">{{ t('deviceSimulator.alarms.continuousHint') }}</span></span></label></div><div class="mt-5 flex flex-wrap gap-2"><button type="button" class="inline-flex min-h-11 cursor-pointer items-center gap-2 rounded-xl border border-amber-300 bg-amber-50 px-4 py-2 text-sm font-semibold text-amber-900 hover:bg-amber-100 disabled:cursor-not-allowed disabled:opacity-60" :class="buttonFocus" :disabled="simulator.busyAction.value !== null || !running" @click="triggerAlarm"><BellRing class="h-4 w-4" aria-hidden="true" />{{ t('deviceSimulator.actions.triggerOnce') }}</button><button type="button" class="inline-flex min-h-11 cursor-pointer items-center gap-2 rounded-xl bg-sky-600 px-4 py-2 text-sm font-semibold text-white hover:bg-sky-700 disabled:cursor-not-allowed disabled:opacity-60" :class="buttonFocus" :disabled="simulator.busyAction.value !== null || !running" @click="startAlarm"><Activity class="h-4 w-4" aria-hidden="true" />{{ t('deviceSimulator.actions.startAlarm') }}</button><button type="button" class="inline-flex min-h-11 cursor-pointer items-center gap-2 rounded-xl bg-rose-700 px-4 py-2 text-sm font-semibold text-white hover:bg-rose-800 disabled:cursor-not-allowed disabled:opacity-60" :class="buttonFocus" :disabled="simulator.busyAction.value !== null || !simulator.alarmStats.value" @click="simulator.stopAlarm"><Square class="h-4 w-4" aria-hidden="true" />{{ t('deviceSimulator.actions.stopAlarm') }}</button></div></section>
          <aside class="rounded-2xl border border-slate-200 bg-white p-5 shadow-sm" aria-labelledby="alarm-stats-title"><h2 id="alarm-stats-title" class="font-bold text-slate-900">{{ t('deviceSimulator.alarms.stats') }}</h2><dl class="mt-4 grid grid-cols-2 gap-3"><div v-for="key in ['attempted', 'succeeded', 'failed', 'in_flight']" :key="key" class="rounded-xl bg-slate-100 p-3"><dt class="text-xs font-semibold text-slate-500">{{ t(`deviceSimulator.alarms.${key}`) }}</dt><dd class="mt-1 text-xl font-bold text-slate-900">{{ simulator.alarmStats.value?.[key as keyof typeof simulator.alarmStats.value] ?? simulator.lastAlarmResult.value?.[key as keyof typeof simulator.lastAlarmResult.value] ?? 0 }}</dd></div></dl><p v-if="simulator.alarmStats.value?.last_error" class="mt-4 rounded-xl border border-rose-200 bg-rose-50 p-3 text-sm text-rose-800">{{ simulator.alarmStats.value.last_error.code }}</p></aside>
        </div>
      </template>

      <template v-else>
        <section class="rounded-2xl border border-slate-200 bg-white shadow-sm" aria-labelledby="logs-title"><div class="flex flex-col gap-3 border-b border-slate-200 p-5 lg:flex-row lg:items-center lg:justify-between"><div><h2 id="logs-title" class="font-bold text-slate-900">{{ t('deviceSimulator.logs.title') }}</h2><p class="mt-1 text-sm text-slate-600">{{ t('deviceSimulator.logs.description') }}</p></div><div class="flex flex-col gap-2 sm:flex-row"><label class="sr-only" for="simulator-log-level">{{ t('deviceSimulator.logs.level') }}</label><select id="simulator-log-level" v-model="logLevel" :class="[fieldClass, 'sm:w-40']"><option value="all">{{ t('common.all') }}</option><option v-for="level in ['trace', 'debug', 'info', 'warning', 'error']" :key="level" :value="level">{{ level }}</option></select><label class="sr-only" for="simulator-log-search">{{ t('common.search') }}</label><input id="simulator-log-search" v-model="logQuery" :class="[fieldClass, 'sm:w-64']" type="search" :placeholder="t('deviceSimulator.logs.search')" /><button type="button" class="inline-flex min-h-11 cursor-pointer items-center justify-center gap-2 rounded-xl border border-slate-300 px-4 py-2 text-sm font-semibold text-slate-700 hover:bg-slate-50 disabled:opacity-50" :class="buttonFocus" :disabled="filteredLogs.length === 0" @click="downloadJson('device-simulator-logs.json', filteredLogs)"><FileDown class="h-4 w-4" aria-hidden="true" />{{ t('common.export') }}</button></div></div><ol class="max-h-[42rem] divide-y divide-slate-100 overflow-auto font-mono text-xs"><li v-for="(entry, index) in filteredLogs" :key="`${entry.timestamp}-${index}`" class="grid gap-1 px-5 py-3 lg:grid-cols-[11rem_5rem_10rem_1fr]"><time class="text-slate-500">{{ entry.timestamp }}</time><span class="font-bold uppercase" :class="entry.level === 'error' ? 'text-rose-700' : entry.level === 'warning' ? 'text-amber-700' : 'text-sky-700'">{{ entry.level }}</span><span class="truncate text-slate-500" :title="entry.component">{{ entry.component }}</span><span class="break-words text-slate-800">{{ entry.message }}<span v-if="entry.error_code" class="ml-2 rounded bg-rose-100 px-1.5 py-0.5 text-rose-700">{{ entry.error_code }}</span></span></li><li v-if="filteredLogs.length === 0" class="px-5 py-12 text-center font-sans text-sm text-slate-500">{{ t('deviceSimulator.logs.empty') }}</li></ol></section>
      </template>
    </div>
  </main>
</template>

<style scoped>
@media (prefers-reduced-motion: reduce) {
  *,
  *::before,
  *::after {
    scroll-behavior: auto !important;
    transition-duration: 0.01ms !important;
    animation-duration: 0.01ms !important;
    animation-iteration-count: 1 !important;
  }
}
</style>
