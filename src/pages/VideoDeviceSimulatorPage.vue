<script setup lang="ts">
import { computed, onMounted, onUnmounted, reactive, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import { invoke } from '@tauri-apps/api/core';
import { useRouter } from 'vue-router';
import {
  Activity,
  AlertTriangle,
  BellRing,
  Cable,
  CheckCircle2,
  ChevronDown,
  Clock3,
  Clipboard,
  Download,
  FileDown,
  Globe,
  ImagePlus,
  LoaderCircle,
  List,
  Pencil,
  Plus,
  RadioTower,
  RefreshCw,
  RotateCcw,
  Search,
  Server,
  ShieldAlert,
  ShieldCheck,
  Square,
  Trash2,
  Video,
  XCircle,
} from 'lucide-vue-next';

import { useDeviceSimulator } from '@/composables/useDeviceSimulator';
import {
  alarmErrorHttpStatus,
  alarmErrorMessageKey,
  isAlarmSubscriptionExpired,
  isDeviceSimulatorRuntimeActive,
  type AddressConflictAssessment,
  type AlarmJobRequest,
  type ConflictEvidence,
  type MediaThemeSummary,
  type PlatformAccessMode,
  type PreflightCheck,
} from '@/lib/deviceSimulator';

const { t, te } = useI18n();
const router = useRouter();
const simulator = useDeviceSimulator();
const activeTab = ref<'configuration' | 'runtime' | 'alarms' | 'logs'>('configuration');
const logLevel = ref('all');
const logQuery = ref('');
const copiedValue = ref('');
const continuousAlarm = ref(false);
const now = ref(Date.now());
let subscriptionTicker: number | null = null;
const assetDetailsOpen = ref(false);
const interfaceSelectorOpen = ref(false);
const ipAllocationMode = ref<'continuous' | 'explicit'>('continuous');
const deviceIpText = ref('');

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

const fieldClass = 'min-h-11 w-full rounded-xl border border-slate-300 bg-white px-3 py-2 text-sm text-slate-800 shadow-sm transition-colors placeholder:text-slate-400 focus-visible:border-sky-500 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-500/25 disabled:cursor-not-allowed disabled:bg-slate-100 disabled:text-slate-500';
const buttonFocus = 'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-500/45 focus-visible:ring-offset-2';

const runtimeActive = computed(() => isDeviceSimulatorRuntimeActive(simulator.status.value.state));
const recoveryRequired = computed(() => Boolean(simulator.recoverySessionId.value));
const running = computed(() => simulator.status.value.state === 'running');
const stoppable = computed(() => runtimeActive.value && !recoveryRequired.value);
const cleanupActive = computed(() => new Set([
  'stopping_alarms',
  'stopping_services',
  'removing_firewall',
  'removing_ips',
]).has(simulator.status.value.state));
const cleanupPercent = computed(() => {
  const progress = simulator.cleanupProgress.value;
  if (!progress || progress.total <= 0) return 0;
  return Math.min(100, Math.round((progress.completed / progress.total) * 100));
});
const profilesById = computed(() => new Map(simulator.profiles.value.map((profile) => [profile.id, profile])));
const selectedAlarmProfileId = ref('ipc-custom');
const selectedAlarmTypeId = ref('');
const availableAlarmTypes = computed(() => simulator.alarmTypes.value
  .find((profile) => profile.profile_id === selectedAlarmProfileId.value)?.alarm_types ?? []);
const alarmProfileOptions = computed(() => simulator.request.groups
  .map((group) => group.profile_id)
  .filter((profileId, index, values) => values.indexOf(profileId) === index));
const visibleDevices = computed(() => simulator.preview.value?.devices.slice(0, 100) ?? []);
const allStreamAddresses = computed(() => simulator.preview.value?.devices.flatMap((device) => device.streams) ?? []);
const filteredLogs = computed(() => simulator.logs.value.filter((entry) => {
  if (logLevel.value !== 'all' && entry.level !== logLevel.value) return false;
  const query = logQuery.value.trim().toLowerCase();
  if (!query) return true;
  return [entry.message, entry.component, entry.device_ip, entry.error_code]
    .some((value) => value?.toLowerCase().includes(query));
}));
const displayedError = computed(() => {
  const message = simulator.errorMessage.value;
  if (!message) return '';
  const newline = message.indexOf('\n');
  const messageKey = newline >= 0 ? message.slice(0, newline) : message;
  const details = newline >= 0 ? message.slice(newline + 1) : '';
  const summary = messageKey.startsWith('deviceSimulator.') ? t(messageKey) : messageKey;
  return details ? `${summary}\n${details}` : summary;
});
const assetTone = computed(() => {
  const state = simulator.assets.value?.state ?? 'unknown';
  if (state === 'ready' || state === 'update_available') return 'ready';
  if (state === 'failed') return 'error';
  return 'attention';
});

/**
 * Checks that can only ever report the state of the world, never something the
 * user can act on. They stay in the report — the raw payload still reaches the
 * run log — but repeating them on every start only trains people to ignore the
 * banner that also carries the blocking failures.
 */
const ADVISORY_CHECK_IDS = new Set(['profile-evidence']);
const attentionChecks = computed(() => (simulator.preflight.value?.checks ?? []).filter((check) => {
  if (check.status === 'passed') return false;
  return !(check.status === 'warning' && ADVISORY_CHECK_IDS.has(check.id));
}));
const preflightTone = computed<'blocked' | 'warning' | 'clear' | null>(() => {
  if (!simulator.preflight.value) return null;
  if (attentionChecks.value.some((check) => check.status === 'failed')) return 'blocked';
  if (attentionChecks.value.length > 0) return 'warning';
  // Once the devices are up, the running state in the header is the answer to
  // "did it work"; a standing "check passed" would only add to the stack.
  return runtimeActive.value ? null : 'clear';
});
const preflightBanner = computed(() => {
  if (preflightTone.value === 'blocked') {
    return {
      container: 'border-rose-200 bg-rose-50',
      icon: XCircle,
      icon_color: 'text-rose-600',
      title: 'text-rose-950',
      body: 'text-rose-800',
      badge: 'bg-rose-600 text-white',
      button: 'border-rose-300 bg-white text-rose-800 hover:bg-rose-100',
    };
  }
  if (preflightTone.value === 'warning') {
    return {
      container: 'border-amber-200 bg-amber-50',
      icon: AlertTriangle,
      icon_color: 'text-amber-600',
      title: 'text-amber-950',
      body: 'text-amber-800',
      badge: 'bg-amber-500 text-white',
      button: 'border-amber-300 bg-white text-amber-900 hover:bg-amber-100',
    };
  }
  return {
    container: 'border-emerald-200 bg-emerald-50',
    icon: CheckCircle2,
    icon_color: 'text-emerald-600',
    title: 'text-emerald-950',
    body: 'text-emerald-800',
    badge: 'bg-emerald-600 text-white',
    button: 'border-emerald-300 bg-white text-emerald-800 hover:bg-emerald-100',
  };
});

/**
 * A blocked start reports the generic "fix the failed checks" sentence, which
 * says strictly less than the banner listing those very checks. Show one.
 */
const visibleError = computed(() => (
  simulator.errorMessage.value === 'deviceSimulator.errors.preflightBlocked' && preflightTone.value === 'blocked'
    ? ''
    : displayedError.value
));

const alarmError = computed(() => simulator.alarmStats.value?.last_error ?? null);
/**
 * Explain the failure when the code is one we recognise, and otherwise fall back
 * to the generic sentence. Either way the raw code is rendered separately, so a
 * code we have no phrasing for is still fully visible.
 */
const alarmErrorSummary = computed(() => {
  const error = alarmError.value;
  if (!error) return '';
  const status = alarmErrorHttpStatus(error.code);
  const key = alarmErrorMessageKey(error.code);
  if (key && te(key)) return status ? t(key, { status }) : t(key);
  return t(error.message_key);
});

const subscription = computed(() => simulator.alarmSubscription.value);
const subscriptionExpired = computed(() => {
  const current = subscription.value;
  return current !== null && current.learned && isAlarmSubscriptionExpired(current, now.value);
});
const subscriptionTone = computed(() => {
  const current = subscription.value;
  if (current?.overridden) {
    return {
      container: 'border-slate-200 bg-slate-50',
      icon: Server,
      icon_color: 'text-slate-500',
      title: 'text-slate-900',
      body: 'text-slate-600',
      title_key: 'deviceSimulator.subscription.overriddenTitle',
      description_key: 'deviceSimulator.subscription.overriddenDescription',
    };
  }
  if (!current?.learned) {
    return {
      container: 'border-amber-200 bg-amber-50',
      icon: AlertTriangle,
      icon_color: 'text-amber-600',
      title: 'text-amber-900',
      body: 'text-amber-800',
      title_key: 'deviceSimulator.subscription.waitingTitle',
      description_key: 'deviceSimulator.subscription.waitingDescription',
    };
  }
  if (subscriptionExpired.value) {
    return {
      container: 'border-amber-200 bg-amber-50',
      icon: AlertTriangle,
      icon_color: 'text-amber-600',
      title: 'text-amber-900',
      body: 'text-amber-800',
      title_key: 'deviceSimulator.subscription.expiredTitle',
      description_key: 'deviceSimulator.subscription.expiredDescription',
    };
  }
  return {
    container: 'border-emerald-200 bg-emerald-50',
    icon: CheckCircle2,
    icon_color: 'text-emerald-600',
    title: 'text-emerald-900',
    body: 'text-emerald-800',
    title_key: 'deviceSimulator.subscription.activeTitle',
    description_key: 'deviceSimulator.subscription.activeDescription',
  };
});
const subscriptionLifetime = computed(() => {
  const current = subscription.value;
  if (!current?.learned || current.duration_secs === null) return '';
  if (current.expires_at_ms === null) return t('deviceSimulator.subscription.duration', { seconds: current.duration_secs });
  const remaining = Math.max(0, Math.round((current.expires_at_ms - now.value) / 1_000));
  return t('deviceSimulator.subscription.remaining', { seconds: current.duration_secs, remaining });
});

watch(
  () => simulator.request.groups.map((group) => group.profile_id).join(','),
  () => {
    if (simulator.busyAction.value === null) void simulator.refreshAssets();
  },
);

watch(alarmProfileOptions, (profiles) => {
  if (!profiles.includes(selectedAlarmProfileId.value)) {
    selectedAlarmProfileId.value = profiles[0] ?? 'ipc-custom';
    alarm.alarm_type_ids = [];
  }
}, { immediate: true });

watch(selectedAlarmProfileId, () => {
  selectedAlarmTypeId.value = '';
  alarm.target_device_ids = [];
  if (alarm.mode === 'configured') alarm.mode = 'sequential';
});
const assetDownloadActive = computed(() => {
  // Asset readiness is refreshed independently from progress events. If a
  // terminal event was missed, never let its stale progress contradict the
  // authoritative status shown in the details below.
  if (simulator.assets.value?.state === 'ready') return false;
  return new Set([
    'checking',
    'downloading',
    'verifying',
    'installing',
  ]).has(simulator.assetProgress.value?.state ?? '');
});
const assetPercent = computed(() => {
  const progress = simulator.assetProgress.value;
  if (!progress?.total) return null;
  return Math.min(100, Math.round((progress.downloaded / progress.total) * 100));
});
const assetSummaryLabel = computed(() => {
  if (assetDownloadActive.value) {
    return assetPercent.value !== null
      ? t('deviceSimulator.assets.summary.preparingPercent', { percent: assetPercent.value })
      : t('deviceSimulator.assets.summary.preparing');
  }
  if (assetTone.value === 'ready') return t('deviceSimulator.assets.summary.ready');
  if (assetTone.value === 'error') return t('deviceSimulator.assets.summary.failed');
  return t('deviceSimulator.assets.summary.attention');
});
const assetChipClass = computed(() => {
  if (assetTone.value === 'ready') return 'border-emerald-200 bg-emerald-50 text-emerald-800 hover:bg-emerald-100';
  if (assetTone.value === 'error') return 'border-rose-200 bg-rose-50 text-rose-800 hover:bg-rose-100';
  return 'border-amber-200 bg-amber-50 text-amber-900 hover:bg-amber-100';
});

/** Keep the details out of the way while files are ready, and open them the moment they need attention. */
watch(() => simulator.assets.value?.state ?? 'unknown', (state) => {
  if (state === 'unknown' || state === 'checking') return;
  assetDetailsOpen.value = state !== 'ready';
}, { immediate: true });

const configuredDeviceCount = computed(() => simulator.request.groups
  .reduce((total, group) => total + Math.max(0, Number(group.count) || 0), 0));
// Both figures come off the draft rather than the last preview, so the launch
// panel never lags a keystroke behind what the user just typed.
const configuredChannelCount = computed(() => simulator.request.groups.reduce((total, group) => {
  const count = Math.max(0, Number(group.count) || 0);
  const channels = group.profile_id.startsWith('nvr-')
    ? Math.max(1, Number(group.nvr_channel_count) || 1)
    : 1;
  return total + count * channels;
}, 0));
const plannedAddresses = computed(() => simulator.preview.value?.devices.map((device) => device.ip) ?? []);
const plannedAddressSummary = computed(() => {
  const addresses = plannedAddresses.value;
  if (addresses.length === 0) return '';
  if (ipAllocationMode.value === 'explicit') {
    return addresses.length === 1
      ? addresses[0]
      : t('deviceSimulator.launch.explicitAddresses', { count: addresses.length });
  }
  const last = addresses[addresses.length - 1];
  return addresses[0] === last ? addresses[0] : `${addresses[0]} – ${last}`;
});
const explicitIpCountMismatch = computed(() => ipAllocationMode.value === 'explicit'
  && simulator.request.device_ips.length !== configuredDeviceCount.value);
const addressAssessments = computed(() => simulator.preflight.value?.address_assessments ?? []);
const unresolvedAddressAssessments = computed(() => addressAssessments.value
  .filter((assessment) => assessment.verdict !== 'clear'));
const visibleAddressAssessments = computed(() => unresolvedAddressAssessments.value.slice(0, 12));
const hiddenAddressAssessmentCount = computed(() => Math.max(0, unresolvedAddressAssessments.value.length - visibleAddressAssessments.value.length));
const interfaceSelectionDescription = computed(() => {
  const selection = simulator.interfaceSelection.value;
  if (simulator.manualInterfaceSelection.value) return t('deviceSimulator.networkAdapter.manual');
  if (selection.kind === 'unavailable') return t('deviceSimulator.networkAdapter.unavailable');
  if (selection.kind === 'invalid_target') return t('deviceSimulator.networkAdapter.invalidTarget');
  if (selection.kind === 'fallback') return t('deviceSimulator.networkAdapter.noMatch');
  if (selection.kind === 'ambiguous') {
    return t('deviceSimulator.networkAdapter.ambiguous', { count: selection.matching_interface_ids.length });
  }
  if (selection.target_count > 1) {
    return t('deviceSimulator.networkAdapter.autoMatchedMany', {
      matched: selection.matched_target_count,
      total: selection.target_count,
    });
  }
  return t('deviceSimulator.networkAdapter.autoMatched', {
    ip: selection.target_ip ?? '',
    subnet: selection.matched_network ?? '',
  });
});

// The runtime treats the type list as a filter in every mode, so a chosen type
// survives a mode switch. Only "configured" carries the extra requirement of
// exactly one type, so clearing the type has to drop that mode.
watch(selectedAlarmTypeId, (alarmTypeId) => {
  if (!alarmTypeId && alarm.mode === 'configured') alarm.mode = 'sequential';
});

onMounted(async () => {
  // The subscription lifetime counts down against wall-clock time, so the view
  // needs its own tick; telemetry alone would leave a stale "remaining" value.
  subscriptionTicker = window.setInterval(() => { now.value = Date.now(); }, 1_000);
  await simulator.initialize();
  if (simulator.request.device_ips.length > 0) {
    ipAllocationMode.value = 'explicit';
    deviceIpText.value = simulator.request.device_ips.join('\n');
  }
});

onUnmounted(() => {
  if (subscriptionTicker !== null) window.clearInterval(subscriptionTicker);
  subscriptionTicker = null;
});

function addServer() {
  if (simulator.topologyLocked.value) return;
  simulator.request.platform.servers.push({
    id: `server-${Date.now()}`,
    host: '',
    port: 80,
  });
}

function removeServer(id: string) {
  if (simulator.topologyLocked.value) return;
  simulator.request.platform.servers = simulator.request.platform.servers.filter((server) => server.id !== id);
}

function setPlatformAccessMode(mode: PlatformAccessMode) {
  if (simulator.topologyLocked.value) return;
  simulator.request.platform.access_mode = mode;
}

/** Restricted admission derives its allow list from the server hosts, so an
 * empty list would block the intended platform too. The backend rejects this at
 * start; surface it while the user is still editing. */
const platformAccessNeedsServer = computed(
  () => simulator.request.platform.access_mode === 'configured_servers_only'
    && !simulator.request.platform.servers.some((server) => server.host.trim() !== '' && server.port > 0),
);

function profileLabel(profileId: string) {
  const profile = profilesById.value.get(profileId);
  return profile ? t(profile.display_name_key) : t(`deviceSimulator.profiles.${profileId}`);
}

function mediaThemeLabel(theme: MediaThemeSummary) {
  return te(theme.display_name_key) ? t(theme.display_name_key) : theme.id;
}

function requiredFileLabel(fileId: string) {
  if (fileId === 'protocol-core') return t('deviceSimulator.assets.basicFiles');
  if (fileId.startsWith('media-')) return t('deviceSimulator.assets.liveFiles');
  if (fileId.startsWith('ipc-') || fileId.startsWith('nvr-')) return profileLabel(fileId);
  return t('deviceSimulator.assets.otherFiles');
}

function statusLabel(state: string) {
  return t(`deviceSimulator.states.${state}`);
}

function preflightDetails(check: PreflightCheck) {
  const localConflicts = unresolvedAddressAssessments.value.filter((assessment) => assessment.evidence
    .some((evidence) => evidence.kind === 'local' && evidence.result === 'occupied'));
  if (check.id === 'local-addresses' && localConflicts.length > 0) {
    return t('deviceSimulator.preflight.evidence.localConfirmed', {
      addresses: formatAssessmentAddresses(localConflicts),
    });
  }
  if (check.id === 'address-conflicts') {
    const conflicts = unresolvedAddressAssessments.value.filter((assessment) => assessment.verdict === 'conflict');
    if (conflicts.length > 0) {
      return t('deviceSimulator.preflight.evidence.confirmed', {
        addresses: formatAssessmentAddresses(conflicts),
      });
    }
    if (unresolvedAddressAssessments.value.length > 0) {
      return t('deviceSimulator.preflight.evidence.inconclusive', {
        addresses: formatAssessmentAddresses(unresolvedAddressAssessments.value),
      });
    }
  }
  // The backend detail counts the packs the selection resolves to, which reads
  // like a success line. Say what to do about it, and keep the raw detail: for
  // a catalog or validation fault it carries the only identifying code.
  if (check.id === 'assets' && check.status === 'failed') {
    const guidance = t('deviceSimulator.preflight.evidence.assetsNotPrepared');
    return check.details ? `${guidance} (${check.details})` : guidance;
  }
  if (check.id === 'profile-evidence') {
    return t(check.status === 'failed'
      ? 'deviceSimulator.preflight.evidence.profileUnreviewed'
      : 'deviceSimulator.preflight.evidence.profileUnverified');
  }
  if (check.id === 'platform-connectivity' && check.status === 'warning') {
    return t('deviceSimulator.preflight.evidence.serverConnectivity', {
      servers: simulator.request.platform.servers
        .map((server) => `${server.host}:${server.port}`)
        .join(', '),
    });
  }
  if (check.id === 'firewall' && check.status === 'warning') {
    return t('deviceSimulator.preflight.evidence.firewallManual');
  }
  return check.details ?? '';
}

function formatAssessmentAddresses(assessments: AddressConflictAssessment[]) {
  const shown = assessments.slice(0, 12).map((assessment) => assessment.address).join(', ');
  const remaining = assessments.length - 12;
  return remaining > 0 ? `${shown} (+${remaining})` : shown;
}

function addressVerdictLabel(assessment: AddressConflictAssessment) {
  return t(`deviceSimulator.preflight.evidence.verdict.${assessment.verdict}`);
}

function addressEvidenceText(evidence: ConflictEvidence) {
  if (evidence.kind === 'local') {
    return t('deviceSimulator.preflight.evidence.local', {
      owner: evidence.details || t('deviceSimulator.preflight.evidence.localInterface'),
    });
  }
  if (evidence.kind === 'neighbor' && evidence.result === 'occupied') {
    return t('deviceSimulator.preflight.evidence.neighbor', {
      mac: evidence.details || t('deviceSimulator.preflight.evidence.unknownMac'),
    });
  }
  if (evidence.kind === 'neighbor') {
    return t('deviceSimulator.preflight.evidence.neighborState', {
      state: evidence.details || 'unknown',
    });
  }
  if (evidence.kind === 'probe' && evidence.result === 'occupied') {
    return t('deviceSimulator.preflight.evidence.probe', {
      mac: evidence.details || t('deviceSimulator.preflight.evidence.unknownMac'),
    });
  }
  // ARP reaches the local link only, which is the one case a probe leaves open.
  if (evidence.kind === 'probe') return t('deviceSimulator.preflight.evidence.probeOffLink');
  return t('deviceSimulator.preflight.evidence.notProbed');
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
  alarm.alarm_profile_id = selectedAlarmProfileId.value;
  alarm.alarm_type_ids = selectedAlarmTypeId.value ? [selectedAlarmTypeId.value] : [];
  alarm.target_device_ids = simulator.preview.value?.devices
    .filter((device) => device.profile_id === selectedAlarmProfileId.value)
    .map((device) => device.device_id) ?? [];
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

async function chooseAlarmImage() {
  const imported = await simulator.importAlarmImage();
  if (!imported) return;
  alarm.user_image_id = imported.image_id;
  alarm.image_variant = null;
}

function clearAlarmImage() {
  alarm.user_image_id = null;
  simulator.clearAlarmImageSelection();
}

function formatImageSize(bytes: number) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KiB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MiB`;
}

function setIpAllocationMode(mode: 'continuous' | 'explicit') {
  ipAllocationMode.value = mode;
  if (mode === 'continuous') {
    simulator.request.device_ips = [];
    deviceIpText.value = '';
    return;
  }
  if (!deviceIpText.value.trim()) deviceIpText.value = simulator.request.start_ip;
  updateExplicitIps();
}

function updateExplicitIps() {
  const addresses = deviceIpText.value
    .split(/[\s,;]+/)
    .map((value) => value.trim())
    .filter(Boolean);
  simulator.request.device_ips = addresses;
  const first = addresses[0];
  if (first && /^(?:\d{1,3}\.){3}\d{1,3}$/.test(first)) simulator.request.start_ip = first;
}

function selectNetworkInterface(event: Event) {
  simulator.selectInterfaceManually((event.target as HTMLSelectElement).value);
}

async function openPingScanner() {
  const address = simulator.request.device_ips[0] ?? simulator.request.start_ip;
  const octets = address.split('.');
  if (octets.length === 4) {
    try {
      await invoke('save_kv', {
        key: 'networkTools.pingScanConfig',
        value: { prefix: octets.slice(0, 3).join('.'), start: 1, end: 254, timeoutMs: 1000 },
      });
    } catch {
      // The network tool remains usable even if its suggested range cannot be saved.
    }
  }
  await router.push('/tools/network');
}
</script>

<template>
  <main class="h-full min-w-0 overflow-y-auto bg-slate-50" :aria-busy="simulator.busyAction.value !== null">
    <div class="mx-auto w-full max-w-[1600px] space-y-5 p-4 sm:p-6 lg:p-8">
      <header class="rounded-3xl border border-slate-200 bg-white p-5 shadow-sm sm:p-6">
        <div class="flex flex-col gap-4 lg:flex-row lg:items-center lg:justify-between">
          <div class="flex min-w-0 items-start gap-3">
            <div class="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl bg-gradient-to-br from-sky-600 to-cyan-500 shadow-sm">
              <Video class="h-5 w-5 text-white" aria-hidden="true" />
            </div>
            <div class="min-w-0">
              <p class="text-xs font-bold uppercase tracking-[0.16em] text-sky-700">{{ t('deviceSimulator.eyebrow') }}</p>
              <h1 class="mt-1 text-2xl font-bold text-slate-900">{{ t('deviceSimulator.title') }}</h1>
              <p class="mt-1 max-w-3xl text-sm text-slate-500">{{ t('deviceSimulator.description') }}</p>
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
              v-if="stoppable"
              type="button"
              class="inline-flex min-h-11 cursor-pointer items-center gap-2 rounded-xl bg-rose-700 px-4 py-2 text-sm font-semibold text-white transition-colors hover:bg-rose-800 disabled:cursor-not-allowed disabled:opacity-60"
              :class="buttonFocus"
              :disabled="simulator.busyAction.value !== null"
              @click="simulator.stop"
            >
              <LoaderCircle v-if="simulator.busyAction.value === 'stop'" class="h-4 w-4 animate-spin motion-reduce:animate-none" aria-hidden="true" />
              <Square v-else class="h-4 w-4" aria-hidden="true" />
              {{ t('deviceSimulator.actions.stop') }}
            </button>
            <button
              type="button"
              class="relative inline-flex min-h-11 cursor-pointer items-center gap-2 overflow-hidden rounded-xl border px-3 py-2 text-sm font-semibold transition-colors"
              :class="[buttonFocus, assetChipClass]"
              :aria-expanded="assetDetailsOpen"
              aria-controls="asset-details"
              @click="assetDetailsOpen = !assetDetailsOpen"
            >
              <LoaderCircle v-if="assetDownloadActive" class="h-4 w-4 animate-spin motion-reduce:animate-none" aria-hidden="true" />
              <CheckCircle2 v-else-if="assetTone === 'ready'" class="h-4 w-4" aria-hidden="true" />
              <XCircle v-else-if="assetTone === 'error'" class="h-4 w-4" aria-hidden="true" />
              <AlertTriangle v-else class="h-4 w-4" aria-hidden="true" />
              <span class="tabular-nums">{{ assetSummaryLabel }}</span>
              <ChevronDown class="h-4 w-4 transition-transform" :class="assetDetailsOpen ? 'rotate-180' : ''" aria-hidden="true" />
              <span
                v-if="assetDownloadActive"
                class="absolute inset-x-0 bottom-0 h-0.5 bg-sky-600 transition-[width] duration-200"
                :class="assetPercent === null ? 'w-1/3 animate-pulse motion-reduce:animate-none' : ''"
                :style="assetPercent === null ? undefined : { width: `${assetPercent}%` }"
                aria-hidden="true"
              ></span>
            </button>
            <button
              type="button"
              class="inline-flex min-h-11 cursor-pointer items-center gap-2 rounded-xl border border-slate-300 bg-white px-4 py-2 text-sm font-semibold text-slate-700 transition-colors hover:bg-slate-50 disabled:cursor-not-allowed disabled:opacity-60"
              :class="buttonFocus"
              :disabled="simulator.busyAction.value !== null"
              @click="simulator.refreshEnvironment"
            >
              <RefreshCw class="h-4 w-4" :class="['refresh', 'check-assets'].includes(simulator.busyAction.value ?? '') ? 'animate-spin motion-reduce:animate-none' : ''" aria-hidden="true" />
              {{ t('common.refresh') }}
            </button>
          </div>
        </div>

        <div v-if="assetDetailsOpen" id="asset-details" class="mt-5 border-t border-slate-200 pt-5">
          <div class="flex flex-col gap-4 lg:flex-row lg:items-start lg:justify-between">
            <div class="min-w-0">
              <h2 class="text-sm font-bold text-slate-900">{{ t('deviceSimulator.assets.title') }}</h2>
              <p class="mt-1 text-sm leading-6 text-slate-600" aria-live="polite">
                {{ t(`deviceSimulator.assets.states.${simulator.assets.value?.state ?? 'unknown'}`) }}
              </p>
              <p v-if="assetDownloadActive && simulator.assetProgress.value" class="mt-2 text-xs font-medium text-slate-700" aria-live="polite">
                {{ simulator.assetProgress.value.current_pack_id
                  ? requiredFileLabel(simulator.assetProgress.value.current_pack_id)
                  : t(`deviceSimulator.assets.states.${simulator.assetProgress.value.state}`) }}
                <template v-if="simulator.assetProgress.value.total">
                  · {{ formatImageSize(simulator.assetProgress.value.downloaded) }} /
                  {{ formatImageSize(simulator.assetProgress.value.total) }}
                </template>
                <template v-if="simulator.assetProgress.value.speed_bps > 0">
                  · {{ formatImageSize(simulator.assetProgress.value.speed_bps) }}/s
                </template>
              </p>
              <div v-if="assetDownloadActive" class="mt-2 h-2 w-full max-w-xl overflow-hidden rounded-full bg-slate-200" role="progressbar" :aria-label="t('deviceSimulator.assets.progressLabel')" :aria-valuenow="assetPercent ?? undefined" aria-valuemin="0" aria-valuemax="100">
                <div v-if="assetPercent !== null" class="h-full rounded-full bg-sky-600 transition-[width] duration-200" :style="{ width: `${assetPercent}%` }" />
                <div v-else class="h-full w-1/3 animate-pulse rounded-full bg-sky-600 motion-reduce:animate-none" />
              </div>
              <ul v-if="simulator.assets.value?.packs.length" class="mt-3 space-y-1 text-xs text-slate-600">
                <li v-for="pack in simulator.assets.value.packs" :key="pack.id" class="flex flex-wrap gap-x-2">
                  <span class="font-semibold text-slate-700">{{ requiredFileLabel(pack.id) }}</span>
                  <span>{{ t('deviceSimulator.assets.version') }} {{ pack.installed_version ?? '—' }} / {{ pack.required_version }}</span>
                  <span v-if="pack.error_code" class="text-rose-700">{{ t('deviceSimulator.assets.fileError') }}</span>
                </li>
              </ul>
              <p v-if="assetTone === 'ready'" class="mt-3 rounded-lg border border-amber-300 bg-amber-50 px-3 py-2 text-xs font-semibold leading-5 text-amber-900">
                {{ t('deviceSimulator.assets.staticReviewWarning') }}
              </p>
            </div>
            <div class="flex shrink-0 flex-wrap gap-2">
              <button type="button" class="inline-flex min-h-11 cursor-pointer items-center gap-2 rounded-xl border border-slate-300 bg-white px-4 py-2 text-sm font-semibold text-slate-700 hover:bg-slate-50 disabled:cursor-not-allowed disabled:opacity-60" :class="buttonFocus" :disabled="simulator.busyAction.value !== null" @click="simulator.refreshAssets">
                <RefreshCw class="h-4 w-4" :class="simulator.busyAction.value === 'check-assets' ? 'animate-spin motion-reduce:animate-none' : ''" aria-hidden="true" />{{ t('deviceSimulator.actions.checkAssets') }}
              </button>
              <button type="button" class="inline-flex min-h-11 cursor-pointer items-center gap-2 rounded-xl bg-sky-600 px-4 py-2 text-sm font-semibold text-white shadow-sm hover:bg-sky-700 disabled:cursor-not-allowed disabled:opacity-60" :class="buttonFocus" :disabled="simulator.busyAction.value !== null" @click="simulator.prepareAssets">
                <Download class="h-4 w-4" aria-hidden="true" />{{ t('deviceSimulator.actions.downloadAssets') }}
              </button>
              <button v-if="assetDownloadActive" type="button" class="inline-flex min-h-11 cursor-pointer items-center gap-2 rounded-xl border border-rose-300 bg-white px-4 py-2 text-sm font-semibold text-rose-700 hover:bg-rose-50" :class="buttonFocus" @click="simulator.cancelAssetDownload">
                <Square class="h-4 w-4" aria-hidden="true" />{{ t('common.cancel') }}
              </button>
            </div>
          </div>
        </div>
      </header>

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
            <LoaderCircle v-if="simulator.busyAction.value === 'recover'" class="h-4 w-4 animate-spin motion-reduce:animate-none" aria-hidden="true" />
            <RotateCcw v-else class="h-4 w-4" aria-hidden="true" />
            {{ t(simulator.busyAction.value === 'recover' ? 'deviceSimulator.actions.recovering' : 'deviceSimulator.actions.recover') }}
          </button>
        </div>
      </section>

      <section v-if="cleanupActive && simulator.cleanupProgress.value" class="rounded-2xl border border-sky-200 bg-sky-50 p-5" aria-labelledby="cleanup-title" aria-live="polite">
        <div class="flex items-start gap-3">
          <Activity class="mt-0.5 h-5 w-5 shrink-0 text-sky-700" aria-hidden="true" />
          <div class="min-w-0 flex-1">
            <div class="flex flex-wrap items-center justify-between gap-2">
              <h2 id="cleanup-title" class="font-bold text-sky-950">{{ t('deviceSimulator.cleanup.title') }}</h2>
              <span class="text-sm font-semibold text-sky-800">{{ simulator.cleanupProgress.value.completed }} / {{ simulator.cleanupProgress.value.total }}</span>
            </div>
            <p class="mt-1 text-sm leading-6 text-sky-900">{{ t(simulator.cleanupProgress.value.message_key) }}</p>
            <div class="mt-3 h-2 overflow-hidden rounded-full bg-sky-100" role="progressbar" :aria-label="t('deviceSimulator.cleanup.progressLabel')" :aria-valuenow="cleanupPercent" aria-valuemin="0" aria-valuemax="100">
              <div class="h-full rounded-full bg-sky-600 transition-[width] duration-200" :style="{ width: `${cleanupPercent}%` }" />
            </div>
          </div>
        </div>
      </section>

      <div v-if="visibleError" role="alert" class="flex items-start gap-3 rounded-2xl border border-rose-200 bg-rose-50 p-4 text-sm text-rose-800">
        <XCircle class="mt-0.5 h-5 w-5 shrink-0" aria-hidden="true" />
        <div class="min-w-0"><strong>{{ t('deviceSimulator.errors.title') }}</strong><p class="mt-1 whitespace-pre-wrap break-words">{{ visibleError }}</p></div>
      </div>

      <section
        v-if="preflightTone"
        :role="preflightTone === 'blocked' ? 'alert' : 'status'"
        class="rounded-2xl border p-4 sm:p-5"
        :class="preflightBanner.container"
        aria-labelledby="preflight-banner-title"
      >
        <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
          <div class="flex min-w-0 items-start gap-3">
            <component :is="preflightBanner.icon" class="mt-0.5 h-5 w-5 shrink-0" :class="preflightBanner.icon_color" aria-hidden="true" />
            <div class="min-w-0">
              <div class="flex flex-wrap items-center gap-2">
                <h2 id="preflight-banner-title" class="font-bold" :class="preflightBanner.title">
                  {{ t(`deviceSimulator.preflight.banner.${preflightTone}`) }}
                </h2>
                <span
                  v-if="attentionChecks.length > 0"
                  class="inline-flex h-5 min-w-5 items-center justify-center rounded-full px-1.5 text-xs font-bold tabular-nums"
                  :class="preflightBanner.badge"
                  :aria-label="t('deviceSimulator.preflight.banner.countLabel')"
                >{{ attentionChecks.length }}</span>
              </div>
              <p class="mt-1 text-sm leading-6" :class="preflightBanner.body">
                {{ t(`deviceSimulator.preflight.banner.${preflightTone}Hint`) }}
              </p>
            </div>
          </div>
          <button
            type="button"
            class="inline-flex min-h-11 shrink-0 cursor-pointer items-center justify-center gap-2 rounded-xl border px-4 py-2 text-sm font-semibold transition-colors disabled:cursor-not-allowed disabled:opacity-60"
            :class="[buttonFocus, preflightBanner.button]"
            :disabled="simulator.busyAction.value !== null"
            @click="simulator.runPreflight"
          >
            <LoaderCircle v-if="simulator.busyAction.value === 'preflight'" class="h-4 w-4 animate-spin motion-reduce:animate-none" aria-hidden="true" />
            <RefreshCw v-else class="h-4 w-4" aria-hidden="true" />
            {{ t(simulator.busyAction.value === 'preflight' ? 'deviceSimulator.preflight.banner.checking' : 'deviceSimulator.actions.recheck') }}
          </button>
        </div>

        <ul v-if="attentionChecks.length > 0" class="mt-4 space-y-2">
          <li
            v-for="check in attentionChecks"
            :key="check.id"
            class="rounded-xl border bg-white/70 p-3 text-sm"
            :class="check.status === 'failed' ? 'border-rose-200' : 'border-amber-200'"
          >
            <div class="flex flex-wrap items-center gap-2">
              <span
                class="rounded-md px-2 py-0.5 text-xs font-bold"
                :class="check.status === 'failed' ? 'bg-rose-100 text-rose-800' : 'bg-amber-100 text-amber-900'"
              >{{ t(`deviceSimulator.preflight.banner.${check.status === 'failed' ? 'failedBadge' : 'warningBadge'}`) }}</span>
              <strong class="text-slate-900">{{ t(check.message_key) }}</strong>
            </div>
            <p v-if="preflightDetails(check)" class="mt-1.5 break-words leading-6 text-slate-700">{{ preflightDetails(check) }}</p>
            <div v-if="check.id === 'address-conflicts' && visibleAddressAssessments.length > 0" class="mt-2 space-y-1 text-xs leading-5 text-slate-600">
              <div v-for="assessment in visibleAddressAssessments" :key="assessment.address" class="font-mono">
                <span class="font-semibold text-slate-800">{{ assessment.address }}</span><span class="ml-2 font-sans">{{ addressVerdictLabel(assessment) }}</span>
                <p v-for="evidence in assessment.evidence.filter((item) => item.result !== 'available')" :key="`${assessment.address}-${evidence.kind}-${evidence.result}`" class="pl-2 font-sans">{{ addressEvidenceText(evidence) }}</p>
              </div>
              <p v-if="hiddenAddressAssessmentCount > 0" class="font-sans">{{ t('deviceSimulator.preflight.evidence.more', { count: hiddenAddressAssessmentCount }) }}</p>
            </div>
          </li>
        </ul>
      </section>

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
                  <div :class="[fieldClass, 'mt-2', 'flex items-center bg-slate-100 font-semibold']" aria-readonly="true">UMS</div>
                </label>
                <label class="block text-sm font-semibold text-slate-700">{{ t('deviceSimulator.fields.alarmReceiverPort') }}
                  <input v-model.number="simulator.request.platform.alarm_receiver_port" :class="[fieldClass, 'mt-2']" type="number" min="1" max="65535" inputmode="numeric" />
                  <span class="mt-1 block text-xs font-normal leading-5 text-slate-500">{{ t('deviceSimulator.fields.alarmReceiverPortHint') }}</span>
                </label>
              </div>
              <div class="mt-4 space-y-3">
                <p class="text-xs leading-5 text-slate-500">{{ t('deviceSimulator.configuration.serversHint') }}</p>
                <div v-for="serverItem in simulator.request.platform.servers" :key="serverItem.id" class="grid gap-3 rounded-xl border border-slate-200 bg-slate-50 p-3 sm:grid-cols-[1fr_8rem_2.75rem]">
                  <label class="text-xs font-semibold text-slate-600">{{ t('deviceSimulator.fields.serverHost') }}<input v-model="serverItem.host" :class="[fieldClass, 'mt-1']" type="text" /></label>
                  <label class="text-xs font-semibold text-slate-600">{{ t('deviceSimulator.fields.port') }}<input v-model.number="serverItem.port" :class="[fieldClass, 'mt-1']" type="number" min="1" max="65535" inputmode="numeric" /></label>
                  <button type="button" class="mt-5 inline-flex min-h-11 cursor-pointer items-center justify-center rounded-xl text-rose-700 hover:bg-rose-100" :class="buttonFocus" :aria-label="t('deviceSimulator.actions.removeServer')" @click="removeServer(serverItem.id)"><Trash2 class="h-5 w-5" aria-hidden="true" /></button>
                </div>
                <button type="button" class="inline-flex min-h-11 cursor-pointer items-center gap-2 rounded-xl border border-dashed border-slate-300 px-4 py-2 text-sm font-semibold text-slate-700 hover:border-sky-400 hover:bg-sky-50" :class="buttonFocus" @click="addServer"><Plus class="h-4 w-4" aria-hidden="true" />{{ t('deviceSimulator.actions.addServer') }}</button>
              </div>

              <div class="mt-5 border-t border-slate-200 pt-4">
                <p class="text-sm font-semibold text-slate-700">{{ t('deviceSimulator.fields.accessMode') }}</p>
                <div class="mt-2 inline-flex rounded-lg border border-slate-300 bg-slate-100 p-1" role="group" :aria-label="t('deviceSimulator.fields.accessMode')">
                  <button v-for="mode in (['open', 'configured_servers_only'] as const)" :key="mode" type="button" class="inline-flex min-h-10 cursor-pointer items-center gap-2 rounded-md px-3 py-2 text-sm font-semibold transition-colors" :class="simulator.request.platform.access_mode === mode ? 'bg-white text-slate-900 shadow-sm' : 'text-slate-600 hover:text-slate-900'" :aria-pressed="simulator.request.platform.access_mode === mode" @click="setPlatformAccessMode(mode)"><ShieldCheck v-if="mode === 'configured_servers_only'" class="h-4 w-4" aria-hidden="true" /><Globe v-else class="h-4 w-4" aria-hidden="true" />{{ t(`deviceSimulator.fields.accessModes.${mode}`) }}</button>
                </div>
                <p class="mt-2 text-xs leading-5 text-slate-500">{{ t(`deviceSimulator.fields.accessModeHints.${simulator.request.platform.access_mode}`) }}</p>
                <p v-if="platformAccessNeedsServer" class="mt-2 flex items-start gap-2 rounded-lg bg-amber-50 px-3 py-2 text-xs leading-5 text-amber-800"><AlertTriangle class="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />{{ t('deviceSimulator.fields.accessModeServerRequired') }}</p>
              </div>

              <div v-if="subscription" class="mt-5 rounded-xl border p-4" :class="subscriptionTone.container">
                <div class="flex items-start gap-3">
                  <component :is="subscriptionTone.icon" class="mt-0.5 h-5 w-5 shrink-0" :class="subscriptionTone.icon_color" aria-hidden="true" />
                  <div class="min-w-0 flex-1">
                    <p class="text-sm font-bold" :class="subscriptionTone.title">{{ t(subscriptionTone.title_key) }}</p>
                    <p class="mt-1 text-xs leading-5" :class="subscriptionTone.body">{{ t(subscriptionTone.description_key) }}</p>
                    <dl class="mt-3 space-y-1.5 text-xs">
                      <div class="flex flex-wrap gap-x-2">
                        <dt class="font-semibold" :class="subscriptionTone.body">{{ t('deviceSimulator.subscription.destination') }}</dt>
                        <dd class="min-w-0 break-all font-mono" :class="subscriptionTone.body">{{ subscription.destinations.join('、') || '—' }}</dd>
                      </div>
                      <div v-if="subscription.learned" class="flex flex-wrap gap-x-2">
                        <dt class="font-semibold" :class="subscriptionTone.body">{{ t('deviceSimulator.subscription.advertised') }}</dt>
                        <dd class="min-w-0 break-all font-mono" :class="subscriptionTone.body">{{ [subscription.host, subscription.port].filter(Boolean).join(':') }}</dd>
                      </div>
                      <div v-if="subscriptionLifetime" class="flex flex-wrap gap-x-2">
                        <dt class="font-semibold" :class="subscriptionTone.body">{{ t('deviceSimulator.subscription.lifetime') }}</dt>
                        <dd class="min-w-0" :class="subscriptionTone.body">{{ subscriptionLifetime }}</dd>
                      </div>
                    </dl>
                  </div>
                </div>
              </div>

              <details class="mt-5 rounded-xl border border-slate-200 bg-slate-50">
                <summary class="min-h-11 cursor-pointer list-none px-4 py-3 text-sm font-semibold text-slate-700">
                  {{ t('deviceSimulator.configuration.advanced') }}
                </summary>
                <div class="grid gap-4 border-t border-slate-200 p-4">
                  <label class="block text-sm font-semibold text-slate-700">{{ t('deviceSimulator.fields.alarmReceiver') }}
                    <input v-model="simulator.request.platform.alarm_receiver_url" :class="[fieldClass, 'mt-2']" type="url" placeholder="http://192.168.1.10/alarm" />
                    <span class="mt-1 block text-xs font-normal leading-5 text-slate-500">{{ t('deviceSimulator.fields.alarmReceiverHint') }}</span>
                  </label>
                  <label class="block text-sm font-semibold text-slate-700">{{ t('deviceSimulator.fields.assetServer') }}
                    <input v-model="simulator.settings.value.asset_server_url_override" :class="[fieldClass, 'mt-2']" type="url" placeholder="http://127.0.0.1:3000/virtual-device-assets" />
                    <span class="mt-1 block text-xs font-normal leading-5 text-slate-500">{{ t('deviceSimulator.fields.assetServerHint') }}</span>
                  </label>
                </div>
              </details>
            </section>

            <section class="rounded-2xl border border-slate-200 bg-white p-5 shadow-sm" aria-labelledby="network-title">
              <div class="flex flex-wrap items-center justify-between gap-3">
                <div class="flex items-center gap-3"><RadioTower class="h-5 w-5 text-sky-700" aria-hidden="true" /><h2 id="network-title" class="font-bold text-slate-900">{{ t('deviceSimulator.configuration.network') }}</h2></div>
                <button type="button" class="inline-flex min-h-11 cursor-pointer items-center gap-2 rounded-lg border border-slate-300 bg-white px-3 py-2 text-sm font-semibold text-slate-700 transition-colors hover:bg-slate-50" :class="buttonFocus" @click="openPingScanner"><Search class="h-4 w-4" aria-hidden="true" />{{ t('deviceSimulator.actions.pingScan') }}</button>
              </div>
              <div class="mt-5 inline-flex rounded-lg border border-slate-300 bg-slate-100 p-1" role="group" :aria-label="t('deviceSimulator.fields.ipAllocationMode')">
                <button v-for="mode in ['continuous', 'explicit'] as const" :key="mode" type="button" class="inline-flex min-h-10 cursor-pointer items-center gap-2 rounded-md px-3 py-2 text-sm font-semibold transition-colors" :class="ipAllocationMode === mode ? 'bg-white text-slate-900 shadow-sm' : 'text-slate-600 hover:text-slate-900'" :aria-pressed="ipAllocationMode === mode" @click="setIpAllocationMode(mode)"><List v-if="mode === 'explicit'" class="h-4 w-4" aria-hidden="true" /><RadioTower v-else class="h-4 w-4" aria-hidden="true" />{{ t(`deviceSimulator.fields.ipModes.${mode}`) }}</button>
              </div>
              <div class="mt-5 border-y border-slate-200 py-4">
                <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
                  <div class="flex min-w-0 items-start gap-3">
                    <Cable class="mt-0.5 h-5 w-5 shrink-0 text-sky-700" aria-hidden="true" />
                    <div class="min-w-0">
                      <p class="text-xs font-semibold text-slate-500">{{ t('deviceSimulator.networkAdapter.label') }}</p>
                      <p class="mt-1 break-words text-sm font-bold text-slate-900">
                        {{ simulator.selectedInterface.value
                          ? `${simulator.selectedInterface.value.name} · ${simulator.selectedInterface.value.description}`
                          : t('deviceSimulator.fields.selectInterface') }}
                      </p>
                      <p v-if="simulator.selectedInterface.value?.ipv4_addresses.length" class="mt-1 break-words font-mono text-xs text-slate-600">
                        {{ simulator.selectedInterface.value.ipv4_addresses.join(', ') }}
                      </p>
                      <p class="mt-2 text-xs leading-5" :class="simulator.interfaceSelection.value.kind === 'fallback' || simulator.interfaceSelection.value.kind === 'ambiguous' ? 'text-amber-800' : 'text-slate-600'">
                        {{ interfaceSelectionDescription }}
                      </p>
                    </div>
                  </div>
                  <button type="button" class="inline-flex min-h-10 shrink-0 cursor-pointer items-center justify-center gap-2 rounded-lg border border-slate-300 bg-white px-3 py-2 text-sm font-semibold text-slate-700 hover:bg-slate-50 disabled:cursor-not-allowed disabled:opacity-60" :class="buttonFocus" :disabled="simulator.interfaces.value.length === 0" :aria-expanded="interfaceSelectorOpen" @click="interfaceSelectorOpen = !interfaceSelectorOpen">
                    <Pencil class="h-4 w-4" aria-hidden="true" />{{ t(interfaceSelectorOpen ? 'deviceSimulator.networkAdapter.done' : 'deviceSimulator.networkAdapter.change') }}
                  </button>
                </div>
                <div v-if="interfaceSelectorOpen" class="mt-4 flex flex-col gap-2 sm:flex-row sm:items-end">
                  <label class="min-w-0 flex-1 text-sm font-semibold text-slate-700">{{ t('deviceSimulator.fields.interface') }}
                    <select :value="simulator.request.interface_id" :class="[fieldClass, 'mt-2']" @change="selectNetworkInterface"><option value="">{{ t('deviceSimulator.fields.selectInterface') }}</option><option v-for="item in simulator.interfaces.value" :key="item.id" :value="item.id">{{ item.name }} · {{ item.description }}</option></select>
                  </label>
                  <button v-if="simulator.manualInterfaceSelection.value" type="button" class="inline-flex min-h-11 cursor-pointer items-center justify-center gap-2 rounded-lg border border-sky-300 bg-sky-50 px-3 py-2 text-sm font-semibold text-sky-800 hover:bg-sky-100" :class="buttonFocus" @click="simulator.applyAutomaticInterfaceSelection">{{ t('deviceSimulator.networkAdapter.useAutomatic') }}</button>
                </div>
              </div>
              <div class="mt-5 grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
                <label v-if="ipAllocationMode === 'continuous'" class="block text-sm font-semibold text-slate-700">{{ t('deviceSimulator.fields.startIp') }}<input v-model="simulator.request.start_ip" :class="[fieldClass, 'mt-2']" type="text" inputmode="decimal" /></label>
                <label v-else class="block text-sm font-semibold text-slate-700 sm:col-span-2 lg:col-span-3">{{ t('deviceSimulator.fields.explicitIps') }}<textarea v-model="deviceIpText" :class="[fieldClass, 'mt-2', 'min-h-28 resize-y font-mono']" rows="4" :placeholder="t('deviceSimulator.fields.explicitIpsPlaceholder')" @input="updateExplicitIps" /><span class="mt-1 block text-xs font-normal leading-5" :class="explicitIpCountMismatch ? 'text-rose-700' : 'text-slate-500'">{{ t('deviceSimulator.fields.explicitIpsHint', { addresses: simulator.request.device_ips.length, devices: configuredDeviceCount }) }}</span></label>
                <label class="block text-sm font-semibold text-slate-700">{{ t('deviceSimulator.fields.prefix') }}<input v-model.number="simulator.request.subnet_prefix" :class="[fieldClass, 'mt-2']" type="number" min="1" max="30" inputmode="numeric" /></label>
                <label class="block text-sm font-semibold text-slate-700">{{ t('deviceSimulator.fields.httpPort') }}<input v-model.number="simulator.request.device_http_port" :class="[fieldClass, 'mt-2']" type="number" min="1" max="65535" inputmode="numeric" /></label>
                <label v-for="stream in (['main', 'sub', 'third'] as const)" :key="stream" class="block text-sm font-semibold text-slate-700">{{ t(`deviceSimulator.fields.rtsp.${stream}`) }}<input v-model.number="simulator.request.rtsp_ports[stream]" :class="[fieldClass, 'mt-2']" type="number" min="1" max="65535" inputmode="numeric" /></label>
              </div>
            </section>

            <section class="rounded-2xl border border-slate-200 bg-white p-5 shadow-sm" aria-labelledby="media-theme-title">
              <div class="flex items-start gap-3">
                <Video class="mt-0.5 h-5 w-5 shrink-0 text-sky-700" aria-hidden="true" />
                <div>
                  <h2 id="media-theme-title" class="font-bold text-slate-900">{{ t('deviceSimulator.mediaThemes.title') }}</h2>
                  <p class="mt-1 text-sm leading-6 text-slate-600">{{ t('deviceSimulator.mediaThemes.description') }}</p>
                </div>
              </div>
              <div class="mt-4 grid gap-3 sm:grid-cols-2" role="radiogroup" :aria-label="t('deviceSimulator.mediaThemes.title')">
                <label
                  v-for="theme in simulator.mediaThemes.value"
                  :key="theme.id"
                  class="relative flex min-h-20 cursor-pointer items-start gap-3 rounded-xl border p-4 transition-colors duration-200 motion-reduce:transition-none"
                  :class="simulator.request.media_theme_id === theme.id
                    ? 'border-sky-500 bg-sky-50 text-sky-950 shadow-sm'
                    : 'border-slate-200 bg-slate-50 text-slate-800 hover:border-sky-300 hover:bg-sky-50/60'"
                >
                  <input v-model="simulator.request.media_theme_id" class="peer sr-only" type="radio" name="device-simulator-media-theme" :value="theme.id" />
                  <span class="mt-0.5 flex h-9 w-9 shrink-0 items-center justify-center rounded-lg" :class="simulator.request.media_theme_id === theme.id ? 'bg-sky-600 text-white' : 'bg-white text-slate-500'" aria-hidden="true">
                    <CheckCircle2 v-if="simulator.request.media_theme_id === theme.id" class="h-5 w-5" />
                    <Video v-else class="h-5 w-5" />
                  </span>
                  <span class="min-w-0">
                    <span class="block text-sm font-semibold">{{ mediaThemeLabel(theme) }}</span>
                    <span v-if="theme.is_default" class="mt-1 block text-xs font-medium text-sky-700">{{ t('deviceSimulator.mediaThemes.defaultLabel') }}</span>
                  </span>
                  <span class="pointer-events-none absolute inset-0 rounded-xl peer-focus-visible:ring-2 peer-focus-visible:ring-sky-500 peer-focus-visible:ring-offset-2" aria-hidden="true" />
                </label>
              </div>
              <label class="mt-4 flex min-h-16 cursor-pointer items-center gap-3 rounded-xl border border-slate-200 bg-slate-50 p-4 transition-colors duration-200 hover:border-sky-300 hover:bg-sky-50/60 has-[:disabled]:cursor-not-allowed has-[:disabled]:opacity-60 motion-reduce:transition-none">
                <input v-model="simulator.request.stream.time_watermark_enabled" class="peer sr-only" type="checkbox" role="switch" :disabled="simulator.topologyLocked.value" />
                <span class="relative h-7 w-12 shrink-0 rounded-full bg-slate-300 transition-colors duration-200 after:absolute after:left-1 after:top-1 after:h-5 after:w-5 after:rounded-full after:bg-white after:shadow-sm after:transition-transform after:duration-200 after:content-[''] peer-checked:bg-sky-600 peer-checked:after:translate-x-5 peer-focus-visible:ring-2 peer-focus-visible:ring-sky-500 peer-focus-visible:ring-offset-2 motion-reduce:transition-none motion-reduce:after:transition-none" aria-hidden="true" />
                <Clock3 class="h-5 w-5 shrink-0 text-sky-700" aria-hidden="true" />
                <span class="min-w-0">
                  <span class="block text-sm font-semibold text-slate-900">{{ t('deviceSimulator.mediaThemes.timeWatermark') }}</span>
                  <span class="mt-1 block text-xs font-normal leading-5 text-slate-600">{{ t('deviceSimulator.mediaThemes.timeWatermarkHint') }}</span>
                </span>
              </label>
              <p class="mt-3 flex items-start gap-2 text-xs leading-5 text-slate-500">
                <AlertTriangle class="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />
                {{ t('deviceSimulator.mediaThemes.restartHint') }}
              </p>
            </section>

            <section class="rounded-2xl border border-slate-200 bg-white p-5 shadow-sm" aria-labelledby="groups-title">
              <div class="flex flex-wrap items-center justify-between gap-3"><div class="flex items-center gap-3"><Video class="h-5 w-5 text-sky-700" aria-hidden="true" /><h2 id="groups-title" class="font-bold text-slate-900">{{ t('deviceSimulator.configuration.groups') }}</h2></div><button type="button" class="inline-flex min-h-11 cursor-pointer items-center gap-2 rounded-xl border border-slate-300 px-4 py-2 text-sm font-semibold text-slate-700 hover:bg-slate-50" :class="buttonFocus" @click="simulator.addGroup()"><Plus class="h-4 w-4" aria-hidden="true" />{{ t('deviceSimulator.actions.addGroup') }}</button></div>
              <div class="mt-4 space-y-3">
                <article v-for="group in simulator.request.groups" :key="group.id" class="grid gap-3 rounded-xl border border-slate-200 bg-slate-50 p-4 md:grid-cols-[minmax(12rem,1fr)_8rem_9rem_2.75rem]">
                  <label class="text-xs font-semibold text-slate-600">{{ t('deviceSimulator.fields.profile') }}<select :value="group.profile_id" :class="[fieldClass, 'mt-1']" @change="simulator.updateGroupProfile(group, ($event.target as HTMLSelectElement).value)"><option v-for="profile in simulator.profiles.value" :key="profile.id" :value="profile.id">{{ profileLabel(profile.id) }}</option><template v-if="simulator.profiles.value.length === 0"><option v-for="id in ['ipc-custom', 'ipc-smart', 'ipc-structured', 'ipc-face-access', 'nvr-common', 'nvr-vehicle']" :key="id" :value="id">{{ profileLabel(id) }}</option></template></select></label>
                  <label class="text-xs font-semibold text-slate-600">{{ t('deviceSimulator.fields.count') }}<input v-model.number="group.count" :class="[fieldClass, 'mt-1']" type="number" min="1" max="500" inputmode="numeric" /></label>
                  <label v-if="group.profile_id.startsWith('nvr-')" class="text-xs font-semibold text-slate-600">{{ t('deviceSimulator.fields.channels') }}<input v-model.number="group.nvr_channel_count" :class="[fieldClass, 'mt-1']" type="number" min="1" max="128" inputmode="numeric" /></label>
                  <button type="button" class="mt-5 inline-flex min-h-11 cursor-pointer items-center justify-center rounded-xl text-rose-700 hover:bg-rose-100 disabled:cursor-not-allowed disabled:opacity-40" :class="buttonFocus" :disabled="simulator.request.groups.length <= 1" :aria-label="t('deviceSimulator.actions.removeGroup')" @click="simulator.removeGroup(group.id)"><Trash2 class="h-5 w-5" aria-hidden="true" /></button>
                </article>
              </div>
            </section>
          </div>

          <div class="space-y-5">
            <section class="rounded-2xl border border-slate-200 bg-white p-5 shadow-sm xl:sticky xl:top-5" aria-labelledby="launch-title">
              <h2 id="launch-title" class="text-xs font-bold uppercase tracking-[0.16em] text-slate-500">
                {{ t(simulator.topologyLocked.value ? 'deviceSimulator.launch.runningTitle' : 'deviceSimulator.launch.title') }}
              </h2>
              <dl class="mt-4 divide-y divide-slate-100 border-y border-slate-200 text-sm">
                <div class="flex items-baseline justify-between gap-3 py-2.5">
                  <dt class="text-slate-500">{{ t('deviceSimulator.launch.devices') }}</dt>
                  <dd class="text-2xl font-bold leading-none tabular-nums text-slate-900">{{ configuredDeviceCount }}</dd>
                </div>
                <div class="flex items-baseline justify-between gap-3 py-2.5">
                  <dt class="text-slate-500">{{ t('deviceSimulator.launch.channels') }}</dt>
                  <dd class="text-2xl font-bold leading-none tabular-nums text-slate-900">{{ configuredChannelCount }}</dd>
                </div>
                <div class="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-1 py-2.5">
                  <dt class="text-slate-500">{{ t('deviceSimulator.launch.addresses') }}</dt>
                  <dd class="min-w-0 break-all text-right font-mono text-xs font-semibold text-slate-800">{{ plannedAddressSummary || '—' }}</dd>
                </div>
                <div class="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-1 py-2.5">
                  <dt class="text-slate-500">{{ t('deviceSimulator.launch.adapter') }}</dt>
                  <dd class="min-w-0 break-words text-right text-xs font-semibold text-slate-800">{{ simulator.selectedInterface.value?.name || '—' }}</dd>
                </div>
              </dl>
              <div class="mt-5 grid gap-2">
                <button v-if="!stoppable && !recoveryRequired" type="button" class="inline-flex min-h-12 w-full cursor-pointer items-center justify-center gap-2 rounded-xl bg-emerald-600 px-4 py-2 text-sm font-bold text-white shadow-sm transition-colors hover:bg-emerald-700 disabled:cursor-not-allowed disabled:opacity-60" :class="buttonFocus" :disabled="simulator.busyAction.value !== null" @click="simulator.start"><LoaderCircle v-if="simulator.busyAction.value === 'start'" class="h-4 w-4 animate-spin motion-reduce:animate-none" aria-hidden="true" /><RadioTower v-else class="h-4 w-4" aria-hidden="true" />{{ t('deviceSimulator.actions.start') }}</button>
                <button type="button" class="inline-flex min-h-11 w-full cursor-pointer items-center justify-center gap-2 rounded-xl border border-slate-300 px-4 py-2 text-sm font-semibold text-slate-700 transition-colors hover:bg-slate-50 disabled:cursor-not-allowed disabled:opacity-60" :class="buttonFocus" :disabled="simulator.busyAction.value !== null || simulator.topologyLocked.value" @click="simulator.saveSettings">{{ t('common.save') }}</button>
              </div>
              <p v-if="!simulator.topologyLocked.value" class="mt-3 flex items-start gap-2 text-xs leading-5 text-slate-500">
                <ShieldCheck class="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />
                {{ t('deviceSimulator.preflight.startHint') }}
              </p>
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
          ]" :key="metric[0]" class="rounded-2xl border border-slate-200 bg-white p-5 shadow-sm transition-colors hover:border-sky-200"><p class="text-xs font-bold uppercase tracking-wider text-slate-500">{{ t(`deviceSimulator.metrics.${metric[0]}`) }}</p><p class="mt-2 text-2xl font-bold tabular-nums text-slate-900">{{ metric[1] }}</p></div>
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
          <section class="rounded-2xl border border-slate-200 bg-white p-5 shadow-sm" aria-labelledby="alarm-title">
            <div class="flex items-center gap-3">
              <BellRing class="h-5 w-5 text-amber-600" aria-hidden="true" />
              <h2 id="alarm-title" class="font-bold text-slate-900">{{ t('deviceSimulator.alarms.title') }}</h2>
            </div>
            <p class="mt-2 text-sm leading-6 text-slate-600">{{ t('deviceSimulator.alarms.description') }}</p>
            <div class="mt-5 grid gap-4 sm:grid-cols-2">
              <label class="text-sm font-semibold text-slate-700">
                {{ t('deviceSimulator.fields.alarmProfile') }}
                <select v-model="selectedAlarmProfileId" :class="[fieldClass, 'mt-2']">
                  <option v-for="profileId in alarmProfileOptions" :key="profileId" :value="profileId">{{ profileLabel(profileId) }}</option>
                </select>
              </label>
              <label class="text-sm font-semibold text-slate-700">
                {{ t('deviceSimulator.fields.alarmTypes') }}
                <select v-model="selectedAlarmTypeId" :class="[fieldClass, 'mt-2']" :disabled="availableAlarmTypes.length === 0">
                  <option value="">{{ t('deviceSimulator.alarms.allTypes') }}</option>
                  <option v-for="alarmType in availableAlarmTypes" :key="alarmType.id" :value="alarmType.id">{{ alarmType.display_name }}</option>
                </select>
                <span v-if="availableAlarmTypes.length === 0" class="mt-1 block text-xs font-normal text-slate-500">{{ t('deviceSimulator.alarms.typesUnavailable') }}</span>
              </label>
              <label class="text-sm font-semibold text-slate-700">
                {{ t('deviceSimulator.fields.dispatchMode') }}
                <select v-model="alarm.mode" :class="[fieldClass, 'mt-2']">
                  <option value="sequential">{{ t('deviceSimulator.alarms.sequential') }}</option>
                  <option value="random">{{ t('deviceSimulator.alarms.random') }}</option>
                  <option value="configured" :disabled="selectedAlarmTypeId === ''">{{ t('deviceSimulator.alarms.configured') }}</option>
                </select>
              </label>
              <label class="text-sm font-semibold text-slate-700">
                {{ t('deviceSimulator.fields.interval') }}
                <input v-model.number="alarm.interval_ms" :class="[fieldClass, 'mt-2']" type="number" min="100" max="3600000" inputmode="numeric">
              </label>
              <label class="text-sm font-semibold text-slate-700">
                {{ t('deviceSimulator.fields.sendCount') }}
                <input v-model.number="alarm.send_count" :class="[fieldClass, 'mt-2']" type="number" min="1" max="100000" inputmode="numeric" :disabled="continuousAlarm">
              </label>
              <label class="text-sm font-semibold text-slate-700">
                {{ t('deviceSimulator.fields.recoveryDelay') }}
                <input v-model.number="alarm.recovery_delay_secs" :class="[fieldClass, 'mt-2']" type="number" min="0" max="86400" inputmode="numeric">
              </label>
              <label class="text-sm font-semibold text-slate-700">
                {{ t('deviceSimulator.fields.imageVariant') }}
                <select
                  v-model="alarm.image_variant"
                  :class="[fieldClass, 'mt-2']"
                  :disabled="alarm.user_image_id !== null"
                  :aria-describedby="alarm.user_image_id ? 'alarm-image-source-hint' : undefined"
                >
                  <option :value="null">{{ t('deviceSimulator.alarms.imageDefault') }}</option>
                  <option value="small">{{ t('deviceSimulator.alarms.imageSmall') }}</option>
                  <option value="big">{{ t('deviceSimulator.alarms.imageBig') }}</option>
                </select>
              </label>
              <label class="flex min-h-11 cursor-pointer items-center gap-3 rounded-xl border border-slate-200 bg-slate-50 px-3 py-2 text-sm font-semibold text-slate-700">
                <input v-model="continuousAlarm" type="checkbox" class="h-4 w-4 rounded border-slate-300 text-sky-600 focus-visible:ring-2 focus-visible:ring-sky-500/45">
                <span>
                  <span class="block">{{ t('deviceSimulator.fields.continuous') }}</span>
                  <span class="block text-xs font-normal text-slate-500">{{ t('deviceSimulator.alarms.continuousHint') }}</span>
                </span>
              </label>
              <div class="sm:col-span-2 rounded-xl border border-slate-200 bg-slate-50 p-4" aria-labelledby="alarm-user-image-title">
                <div class="flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between">
                  <div class="min-w-0">
                    <h3 id="alarm-user-image-title" class="text-sm font-semibold text-slate-800">{{ t('deviceSimulator.fields.customImage') }}</h3>
                    <p id="alarm-image-source-hint" class="mt-1 text-xs leading-5 text-slate-500">{{ t('deviceSimulator.alarms.customImageHint') }}</p>
                    <template v-if="simulator.importedAlarmImage.value && alarm.user_image_id">
                      <p class="mt-2 truncate text-sm font-medium text-slate-800" :title="simulator.importedAlarmImage.value.file_name">
                        {{ simulator.importedAlarmImage.value.file_name }} · {{ formatImageSize(simulator.importedAlarmImage.value.size) }}
                      </p>
                    </template>
                  </div>
                  <div class="flex shrink-0 flex-wrap gap-2">
                    <button
                      type="button"
                      class="inline-flex min-h-11 cursor-pointer items-center gap-2 rounded-xl border border-sky-300 bg-white px-4 py-2 text-sm font-semibold text-sky-800 hover:bg-sky-50 disabled:cursor-not-allowed disabled:opacity-60"
                      :class="buttonFocus"
                      :disabled="simulator.busyAction.value !== null"
                      @click="chooseAlarmImage"
                    >
                      <ImagePlus class="h-4 w-4" aria-hidden="true" />
                      {{ t(alarm.user_image_id ? 'deviceSimulator.actions.replaceImage' : 'deviceSimulator.actions.chooseImage') }}
                    </button>
                    <button
                      v-if="alarm.user_image_id"
                      type="button"
                      class="inline-flex min-h-11 cursor-pointer items-center gap-2 rounded-xl border border-slate-300 bg-white px-4 py-2 text-sm font-semibold text-slate-700 hover:bg-slate-100"
                      :class="buttonFocus"
                      @click="clearAlarmImage"
                    >
                      <XCircle class="h-4 w-4" aria-hidden="true" />
                      {{ t('deviceSimulator.actions.clearImage') }}
                    </button>
                  </div>
                </div>
              </div>
              <p v-if="!running" class="sm:col-span-2 flex items-start gap-2 text-xs leading-5 text-slate-500"><AlertTriangle class="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />{{ t('deviceSimulator.alarms.requiresRunning') }}</p>
            </div>
            <div class="mt-5 flex flex-wrap gap-2">
              <button type="button" class="inline-flex min-h-11 cursor-pointer items-center gap-2 rounded-xl border border-amber-300 bg-amber-50 px-4 py-2 text-sm font-semibold text-amber-900 hover:bg-amber-100 disabled:cursor-not-allowed disabled:opacity-60" :class="buttonFocus" :disabled="simulator.busyAction.value !== null || !running" @click="triggerAlarm"><BellRing class="h-4 w-4" aria-hidden="true" />{{ t('deviceSimulator.actions.triggerOnce') }}</button>
              <button type="button" class="inline-flex min-h-11 cursor-pointer items-center gap-2 rounded-xl bg-sky-600 px-4 py-2 text-sm font-semibold text-white hover:bg-sky-700 disabled:cursor-not-allowed disabled:opacity-60" :class="buttonFocus" :disabled="simulator.busyAction.value !== null || !running || simulator.activeAlarmJobId.value !== null || simulator.alarmStopPending.value" @click="startAlarm"><Activity class="h-4 w-4" aria-hidden="true" />{{ t('deviceSimulator.actions.startAlarm') }}</button>
              <button type="button" class="inline-flex min-h-11 cursor-pointer items-center gap-2 rounded-xl bg-rose-700 px-4 py-2 text-sm font-semibold text-white hover:bg-rose-800 disabled:cursor-not-allowed disabled:opacity-60" :class="buttonFocus" :disabled="simulator.busyAction.value !== null || !simulator.activeAlarmJobId.value || simulator.alarmStopPending.value" @click="simulator.stopAlarm"><Square class="h-4 w-4" aria-hidden="true" />{{ t('deviceSimulator.actions.stopAlarm') }}</button>
            </div>
          </section>
          <aside class="rounded-2xl border border-slate-200 bg-white p-5 shadow-sm" aria-labelledby="alarm-stats-title">
            <h2 id="alarm-stats-title" class="font-bold text-slate-900">{{ t('deviceSimulator.alarms.stats') }}</h2>
            <dl class="mt-4 grid grid-cols-2 gap-3">
              <div v-for="key in ['attempted', 'succeeded', 'failed', 'unverified', 'in_flight']" :key="key" class="rounded-xl bg-slate-100 p-3">
                <dt class="text-xs font-semibold text-slate-500">{{ t(`deviceSimulator.alarms.${key}`) }}</dt>
                <dd class="mt-1 text-xl font-bold tabular-nums text-slate-900">{{ simulator.alarmStats.value?.[key as keyof typeof simulator.alarmStats.value] ?? simulator.lastAlarmResult.value?.[key as keyof typeof simulator.lastAlarmResult.value] ?? 0 }}</dd>
              </div>
              <div class="rounded-xl bg-slate-100 p-3">
                <dt class="text-xs font-semibold text-slate-500">{{ t('deviceSimulator.alarms.lastHttpStatus') }}</dt>
                <dd class="mt-1 text-xl font-bold tabular-nums text-slate-900">{{ simulator.alarmStats.value?.last_http_status ?? '—' }}</dd>
              </div>
            </dl>
            <p class="mt-4 rounded-xl border border-amber-200 bg-amber-50 p-3 text-xs leading-5 text-amber-900">{{ t('deviceSimulator.alarms.unverifiedHint') }}</p>
            <div v-if="alarmError" class="mt-4 rounded-xl border border-rose-200 bg-rose-50 p-3 text-rose-800">
              <p class="text-sm font-semibold">{{ alarmErrorSummary }}</p>
              <p v-if="alarmError.details" class="mt-1.5 break-all font-mono text-xs leading-5 text-rose-700">{{ alarmError.details }}</p>
              <p class="mt-1.5 break-all font-mono text-[11px] leading-4 text-rose-600">{{ alarmError.code }}</p>
            </div>
            <div v-if="subscription" class="mt-4 rounded-xl border p-3 text-xs leading-5" :class="subscriptionTone.container">
              <p class="font-semibold" :class="subscriptionTone.title">{{ t(subscriptionTone.title_key) }}</p>
              <p class="mt-1 break-all font-mono" :class="subscriptionTone.body">{{ subscription.destinations.join('、') || '—' }}</p>
            </div>
          </aside>
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
