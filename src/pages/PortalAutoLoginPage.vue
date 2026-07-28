<script setup lang="ts">
import {
  CheckCircle2,
  CircleAlert,
  Clock3,
  Eye,
  EyeOff,
  KeyRound,
  LoaderCircle,
  LogIn,
  RefreshCw,
  Save,
  ShieldCheck,
  Trash2,
  Wifi,
} from 'lucide-vue-next';
import { computed, onBeforeUnmount, onMounted, reactive, ref } from 'vue';
import { useI18n } from 'vue-i18n';

import { pushToast } from '@/composables/useToast';
import { configStore } from '@/lib/configStore';
import {
  portalLoginCheckStatus,
  portalLoginGetRuntimeStatus,
  portalLoginRun,
  type PortalLoginCheckResult,
  type PortalLoginResult,
  type PortalLoginSettings,
  type PortalLoginStep,
} from '@/lib/tauri';

defineOptions({ name: 'PortalAutoLoginPage' });

const { t, te, locale } = useI18n();

const draft = reactive<PortalLoginSettings>({
  enabled: false,
  host: 'http://1.1.1.3',
  login_url: '/ac_portal/login.php',
  portal_url: '/ac_portal/default/pc.html',
  username: '',
  password: '',
  password_saved: false,
  remember_pwd: true,
  retry_count: 3,
  retry_interval_secs: 5,
  network_wait_secs: 30,
  request_timeout_secs: 15,
});

const isLoading = ref(true);
const isSaving = ref(false);
const isChecking = ref(false);
const isRunning = ref(false);
const loginRequestPending = ref(false);
const showPassword = ref(false);
const lastResult = ref<PortalLoginResult | null>(null);
const checkResult = ref<PortalLoginCheckResult | null>(null);
const formError = ref('');
let statusTimer: ReturnType<typeof setInterval> | null = null;

const busy = computed(() => isLoading.value
  || isSaving.value
  || isChecking.value
  || isRunning.value
  || loginRequestPending.value);
const hasCredentials = computed(() => Boolean(
  draft.username.trim() && (draft.password_saved || draft.password),
));
const numberFields: Array<{
  key: 'retry_count' | 'retry_interval_secs' | 'network_wait_secs' | 'request_timeout_secs';
  labelKey: string;
  min: number;
  max: number;
}> = [
  { key: 'retry_count', labelKey: 'portalAutoLogin.retryCount', min: 1, max: 10 },
  { key: 'retry_interval_secs', labelKey: 'portalAutoLogin.retryInterval', min: 1, max: 300 },
  { key: 'network_wait_secs', labelKey: 'portalAutoLogin.networkWait', min: 0, max: 300 },
  { key: 'request_timeout_secs', labelKey: 'portalAutoLogin.requestTimeout', min: 1, max: 120 },
];

const outcome = computed(() => {
  if (isRunning.value || loginRequestPending.value) return 'running';
  if (lastResult.value) return lastResult.value.outcome;
  if (checkResult.value) return checkResult.value.logged_in ? 'already_logged_in' : 'not_logged_in';
  return 'idle';
});

const outcomeClasses = computed(() => {
  if (outcome.value === 'success' || outcome.value === 'already_logged_in') {
    return 'border-emerald-200 bg-emerald-50 text-emerald-800';
  }
  if (outcome.value === 'failed') return 'border-rose-200 bg-rose-50 text-rose-800';
  if (outcome.value === 'running') return 'border-sky-200 bg-sky-50 text-sky-800';
  if (outcome.value === 'not_logged_in') return 'border-amber-200 bg-amber-50 text-amber-800';
  return 'border-slate-200 bg-slate-50 text-slate-700';
});

function copySettings(settings: PortalLoginSettings) {
  Object.assign(draft, JSON.parse(JSON.stringify(settings)) as PortalLoginSettings);
}

function validate(requireCredentials = false): string {
  try {
    const url = new URL(draft.host.trim());
    if (!['http:', 'https:'].includes(url.protocol)
      || (url.pathname && url.pathname !== '/')
      || url.search
      || url.hash) {
      return t('portalAutoLogin.validation.host');
    }
  } catch {
    return t('portalAutoLogin.validation.host');
  }
  if (!draft.login_url.trim() || !draft.portal_url.trim()) {
    return t('portalAutoLogin.validation.paths');
  }
  if (!Number.isInteger(draft.retry_count) || draft.retry_count < 1 || draft.retry_count > 10) {
    return t('portalAutoLogin.validation.retryCount');
  }
  if (draft.retry_interval_secs < 1 || draft.retry_interval_secs > 300) {
    return t('portalAutoLogin.validation.retryInterval');
  }
  if (draft.network_wait_secs < 0 || draft.network_wait_secs > 300) {
    return t('portalAutoLogin.validation.networkWait');
  }
  if (draft.request_timeout_secs < 1 || draft.request_timeout_secs > 120) {
    return t('portalAutoLogin.validation.requestTimeout');
  }
  if ((draft.enabled || requireCredentials) && !draft.username.trim()) {
    return t('portalAutoLogin.validation.username');
  }
  if ((draft.enabled || requireCredentials) && !draft.password_saved && !draft.password) {
    return t('portalAutoLogin.validation.password');
  }
  return '';
}

function clearSavedPassword() {
  draft.password = '';
  draft.password_saved = false;
  showPassword.value = false;
  formError.value = '';
}

function localizedBackendMessage(value: unknown): string {
  const raw = value instanceof Error ? value.message : String(value);
  const match = raw.match(/portal_login\.([a-z_]+)/);
  if (!match) return raw;
  const key = `portalAutoLogin.backendErrors.${match[1]}`;
  return te(key) ? t(key) : raw;
}

async function persistSettings(requireCredentials = false): Promise<boolean> {
  formError.value = validate(requireCredentials);
  if (formError.value) return false;

  await configStore.ensureLoaded();
  if (!configStore.config) return false;
  configStore.config.portal_login = { ...draft };
  await configStore.saveApp();
  if (configStore.config) copySettings(configStore.config.portal_login);
  return true;
}

async function saveSettings() {
  isSaving.value = true;
  try {
    if (!(await persistSettings())) return;
    pushToast(t('portalAutoLogin.saved'), 'success');
  } catch (error) {
    formError.value = localizedBackendMessage(error);
  } finally {
    isSaving.value = false;
  }
}

async function checkStatus() {
  isChecking.value = true;
  checkResult.value = null;
  try {
    if (!(await persistSettings())) return;
    checkResult.value = await portalLoginCheckStatus();
  } catch (error) {
    formError.value = localizedBackendMessage(error);
  } finally {
    isChecking.value = false;
  }
}

async function loginNow() {
  loginRequestPending.value = true;
  checkResult.value = null;
  try {
    if (!(await persistSettings(true))) return;
    lastResult.value = await portalLoginRun();
    pushToast(
      t(lastResult.value.outcome === 'failed'
        ? 'portalAutoLogin.loginFailedToast'
        : 'portalAutoLogin.loginSuccessToast'),
      lastResult.value.outcome === 'failed' ? 'error' : 'success',
    );
  } catch (error) {
    formError.value = localizedBackendMessage(error);
  } finally {
    loginRequestPending.value = false;
    await refreshRuntime();
  }
}

async function refreshRuntime() {
  try {
    const runtime = await portalLoginGetRuntimeStatus();
    isRunning.value = runtime.running;
    if (runtime.last_result) lastResult.value = runtime.last_result;
  } catch {
    // A stale runtime poll should not replace actionable form feedback.
  }
}

function stepText(step: PortalLoginStep): string {
  const key = `portalAutoLogin.steps.${step.code}`;
  const base = te(key) ? t(key, { detail: step.detail ?? '' }) : step.code;
  if (!step.detail || base.includes(step.detail)) return base;
  return `${base} · ${localizedBackendMessage(step.detail)}`;
}

function formatTime(value: string) {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat(locale.value.startsWith('zh') ? 'zh-CN' : 'en-US', {
    dateStyle: 'medium',
    timeStyle: 'medium',
  }).format(date);
}

function stepDotClass(level: PortalLoginStep['level']) {
  if (level === 'success') return 'bg-emerald-500';
  if (level === 'warn') return 'bg-amber-500';
  if (level === 'error') return 'bg-rose-500';
  return 'bg-sky-500';
}

onMounted(async () => {
  try {
    await configStore.ensureLoaded();
    if (configStore.config) copySettings(configStore.config.portal_login);
    await refreshRuntime();
    statusTimer = setInterval(() => void refreshRuntime(), 1500);
  } catch (error) {
    formError.value = localizedBackendMessage(error);
  } finally {
    isLoading.value = false;
  }
});

onBeforeUnmount(() => {
  if (statusTimer) clearInterval(statusTimer);
});
</script>

<template>
  <div class="flex-1 overflow-y-auto bg-[radial-gradient(circle_at_top_left,_rgba(16,185,129,0.14),_transparent_30%),linear-gradient(180deg,_#f8fbff_0%,_#eef6f4_45%,_#f8fafc_100%)]">
    <main class="mx-auto flex w-full max-w-6xl flex-col gap-6 px-6 py-6 pb-10">
      <section class="relative overflow-hidden rounded-[28px] border border-white/80 bg-white/85 px-6 py-6 shadow-[0_18px_60px_rgba(15,23,42,0.08)] backdrop-blur">
        <div class="pointer-events-none absolute -right-12 -top-16 h-40 w-40 rounded-full bg-emerald-100/80 blur-3xl" aria-hidden="true"></div>
        <div class="relative flex flex-col justify-between gap-5 sm:flex-row sm:items-center">
          <div class="flex items-start gap-4">
            <div class="flex h-12 w-12 shrink-0 items-center justify-center rounded-2xl bg-gradient-to-br from-emerald-500 to-cyan-600 shadow-lg shadow-emerald-500/20">
              <LogIn class="h-6 w-6 text-white" aria-hidden="true" />
            </div>
            <div>
              <p class="text-[11px] font-bold uppercase tracking-[0.16em] text-emerald-700">{{ t('portalAutoLogin.eyebrow') }}</p>
              <h1 class="mt-1 text-2xl font-bold tracking-tight text-slate-950">{{ t('portalAutoLogin.title') }}</h1>
              <p class="mt-2 max-w-3xl text-sm leading-6 text-slate-600">{{ t('portalAutoLogin.description') }}</p>
            </div>
          </div>
          <div class="flex shrink-0 gap-2">
            <button
              type="button"
              class="inline-flex min-h-11 cursor-pointer items-center justify-center gap-2 rounded-xl border border-slate-200 bg-white px-4 text-sm font-semibold text-slate-700 transition-colors hover:bg-slate-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-emerald-500/40 disabled:cursor-not-allowed disabled:opacity-50 motion-reduce:transition-none"
              :disabled="busy"
              @click="checkStatus"
            >
              <RefreshCw class="h-4 w-4" :class="isChecking ? 'animate-spin motion-reduce:animate-none' : ''" aria-hidden="true" />
              {{ t('portalAutoLogin.checkStatus') }}
            </button>
            <button
              type="button"
              class="inline-flex min-h-11 cursor-pointer items-center justify-center gap-2 rounded-xl bg-slate-950 px-4 text-sm font-semibold text-white shadow-lg shadow-slate-900/15 transition-colors hover:bg-slate-800 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-slate-500/50 disabled:cursor-not-allowed disabled:opacity-50 motion-reduce:transition-none"
              :disabled="busy || !hasCredentials"
              @click="loginNow"
            >
              <LoaderCircle v-if="isRunning || loginRequestPending" class="h-4 w-4 animate-spin motion-reduce:animate-none" aria-hidden="true" />
              <LogIn v-else class="h-4 w-4" aria-hidden="true" />
              {{ isRunning || loginRequestPending ? t('portalAutoLogin.loggingIn') : t('portalAutoLogin.loginNow') }}
            </button>
          </div>
        </div>
      </section>

      <div v-if="formError" role="alert" class="flex items-start gap-3 rounded-2xl border border-rose-200 bg-rose-50 px-4 py-3 text-sm text-rose-800">
        <CircleAlert class="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />
        <span>{{ formError }}</span>
      </div>

      <section class="grid gap-6 lg:grid-cols-[minmax(0,1.35fr)_minmax(320px,0.65fr)]">
        <form class="space-y-6 rounded-[24px] border border-slate-200/80 bg-white/95 p-6 shadow-[0_14px_40px_rgba(15,23,42,0.06)]" @submit.prevent="saveSettings">
          <div class="flex items-center justify-between gap-4 border-b border-slate-100 pb-5">
            <div>
              <h2 class="text-lg font-bold text-slate-950">{{ t('portalAutoLogin.configuration') }}</h2>
              <p class="mt-1 text-sm text-slate-500">{{ t('portalAutoLogin.configurationHint') }}</p>
            </div>
            <label class="flex min-h-11 shrink-0 cursor-pointer items-center gap-3 whitespace-nowrap rounded-xl border border-slate-200 bg-slate-50 px-3 text-sm font-semibold text-slate-700">
              <input v-model="draft.enabled" type="checkbox" class="h-4 w-4 rounded border-slate-300 text-emerald-600 focus:ring-emerald-500" />
              {{ t('portalAutoLogin.autoLogin') }}
            </label>
          </div>

          <fieldset class="grid gap-4 md:grid-cols-2" :disabled="busy">
            <legend class="sr-only">{{ t('portalAutoLogin.portalSection') }}</legend>
            <label class="md:col-span-2">
              <span class="mb-1.5 block text-sm font-semibold text-slate-700">{{ t('portalAutoLogin.host') }}</span>
              <input v-model.trim="draft.host" type="url" class="min-h-11 w-full rounded-xl border border-slate-200 bg-white px-3 text-sm text-slate-900 outline-none transition focus:border-emerald-400 focus:ring-2 focus:ring-emerald-500/15" placeholder="http://1.1.1.3" />
            </label>
            <label>
              <span class="mb-1.5 block text-sm font-semibold text-slate-700">{{ t('portalAutoLogin.loginUrl') }}</span>
              <input v-model.trim="draft.login_url" type="text" class="min-h-11 w-full rounded-xl border border-slate-200 bg-white px-3 font-mono text-sm text-slate-900 outline-none transition focus:border-emerald-400 focus:ring-2 focus:ring-emerald-500/15" />
            </label>
            <label>
              <span class="mb-1.5 block text-sm font-semibold text-slate-700">{{ t('portalAutoLogin.portalUrl') }}</span>
              <input v-model.trim="draft.portal_url" type="text" class="min-h-11 w-full rounded-xl border border-slate-200 bg-white px-3 font-mono text-sm text-slate-900 outline-none transition focus:border-emerald-400 focus:ring-2 focus:ring-emerald-500/15" />
            </label>
          </fieldset>

          <div class="grid gap-4 border-t border-slate-100 pt-5 md:grid-cols-2">
            <label>
              <span class="mb-1.5 block text-sm font-semibold text-slate-700">{{ t('portalAutoLogin.username') }}</span>
              <input v-model.trim="draft.username" autocomplete="username" type="text" class="min-h-11 w-full rounded-xl border border-slate-200 bg-white px-3 text-sm text-slate-900 outline-none transition focus:border-emerald-400 focus:ring-2 focus:ring-emerald-500/15" />
            </label>
            <div>
              <label for="portal-domain-password" class="mb-1.5 block text-sm font-semibold text-slate-700">{{ t('portalAutoLogin.password') }}</label>
              <div v-if="draft.password_saved" class="flex min-h-11 items-center justify-between gap-3 rounded-xl border border-emerald-200 bg-emerald-50 px-3" aria-live="polite">
                <span class="flex min-w-0 items-center gap-2 text-sm font-semibold text-emerald-800">
                  <ShieldCheck class="h-4 w-4 shrink-0" aria-hidden="true" />
                  <span class="truncate">{{ t('portalAutoLogin.passwordStored') }}</span>
                </span>
                <button type="button" class="inline-flex min-h-9 shrink-0 cursor-pointer items-center gap-1.5 whitespace-nowrap rounded-lg px-2.5 text-xs font-semibold text-emerald-800 transition-colors hover:bg-emerald-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-emerald-500/40" :disabled="busy" @click="clearSavedPassword">
                  <Trash2 class="h-3.5 w-3.5" aria-hidden="true" />
                  {{ t('portalAutoLogin.clearPassword') }}
                </button>
              </div>
              <span v-else class="relative block">
                <input id="portal-domain-password" v-model="draft.password" autocomplete="new-password" :type="showPassword ? 'text' : 'password'" class="min-h-11 w-full rounded-xl border border-slate-200 bg-white px-3 pr-12 text-sm text-slate-900 outline-none transition focus:border-emerald-400 focus:ring-2 focus:ring-emerald-500/15" :placeholder="t('portalAutoLogin.passwordPlaceholder')" />
                <button type="button" class="absolute right-1 top-1 inline-flex h-9 w-9 cursor-pointer items-center justify-center rounded-lg text-slate-500 hover:bg-slate-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-emerald-500/40" :aria-label="t(showPassword ? 'portalAutoLogin.hidePassword' : 'portalAutoLogin.showPassword')" @click="showPassword = !showPassword">
                  <EyeOff v-if="showPassword" class="h-4 w-4" aria-hidden="true" />
                  <Eye v-else class="h-4 w-4" aria-hidden="true" />
                </button>
              </span>
              <p class="mt-1.5 text-xs text-slate-500">{{ t(draft.password_saved ? 'portalAutoLogin.passwordStoredHint' : 'portalAutoLogin.passwordEntryHint') }}</p>
            </div>
            <label class="flex min-h-11 cursor-pointer items-center gap-3 rounded-xl border border-slate-200 bg-slate-50 px-3 text-sm text-slate-700 md:col-span-2">
              <input v-model="draft.remember_pwd" type="checkbox" class="h-4 w-4 rounded border-slate-300 text-emerald-600 focus:ring-emerald-500" />
              <span><strong>{{ t('portalAutoLogin.rememberPwd') }}</strong><span class="ml-2 text-slate-500">{{ t('portalAutoLogin.rememberPwdHint') }}</span></span>
            </label>
          </div>

          <div class="grid gap-4 border-t border-slate-100 pt-5 sm:grid-cols-2 xl:grid-cols-4">
            <label v-for="field in numberFields" :key="field.key">
              <span class="mb-1.5 block text-sm font-semibold text-slate-700">{{ t(field.labelKey) }}</span>
              <input v-model.number="draft[field.key]" type="number" :min="field.min" :max="field.max" class="min-h-11 w-full rounded-xl border border-slate-200 bg-white px-3 text-sm text-slate-900 outline-none transition focus:border-emerald-400 focus:ring-2 focus:ring-emerald-500/15" />
            </label>
          </div>

          <div class="flex flex-col gap-3 border-t border-slate-100 pt-5 sm:flex-row sm:items-center sm:justify-between">
            <div class="flex max-w-2xl items-start gap-2 text-xs leading-5 text-amber-700">
              <KeyRound class="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />
              <span>{{ t('portalAutoLogin.passwordWarning') }}</span>
            </div>
            <button type="submit" class="inline-flex min-h-11 shrink-0 cursor-pointer items-center justify-center gap-2 whitespace-nowrap rounded-xl bg-emerald-600 px-5 text-sm font-semibold text-white transition-colors hover:bg-emerald-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-emerald-500/50 disabled:cursor-not-allowed disabled:opacity-50" :disabled="busy">
              <LoaderCircle v-if="isSaving" class="h-4 w-4 animate-spin motion-reduce:animate-none" aria-hidden="true" />
              <Save v-else class="h-4 w-4" aria-hidden="true" />
              {{ t('portalAutoLogin.save') }}
            </button>
          </div>
        </form>

        <aside class="space-y-5">
          <section class="rounded-[24px] border bg-white/95 p-5 shadow-[0_14px_40px_rgba(15,23,42,0.06)]" :class="outcomeClasses">
            <div class="flex items-start gap-3">
              <LoaderCircle v-if="outcome === 'running'" class="mt-0.5 h-5 w-5 shrink-0 animate-spin motion-reduce:animate-none" aria-hidden="true" />
              <CheckCircle2 v-else-if="outcome === 'success' || outcome === 'already_logged_in'" class="mt-0.5 h-5 w-5 shrink-0" aria-hidden="true" />
              <CircleAlert v-else-if="outcome === 'failed' || outcome === 'not_logged_in'" class="mt-0.5 h-5 w-5 shrink-0" aria-hidden="true" />
              <Wifi v-else class="mt-0.5 h-5 w-5 shrink-0" aria-hidden="true" />
              <div class="min-w-0">
                <p class="text-xs font-bold uppercase tracking-[0.12em] opacity-70">{{ t('portalAutoLogin.currentStatus') }}</p>
                <h2 class="mt-1 text-lg font-bold">{{ t(`portalAutoLogin.outcomes.${outcome}`) }}</h2>
                <p v-if="lastResult?.account || checkResult?.account" class="mt-1 truncate text-sm">{{ t('portalAutoLogin.account', { account: lastResult?.account || checkResult?.account }) }}</p>
                <p v-if="lastResult?.checked_at || checkResult?.checked_at" class="mt-2 flex items-center gap-1.5 text-xs opacity-70">
                  <Clock3 class="h-3.5 w-3.5" aria-hidden="true" />
                  {{ formatTime(lastResult?.checked_at || checkResult?.checked_at || '') }}
                </p>
              </div>
            </div>
          </section>

          <section class="rounded-[24px] border border-slate-200/80 bg-white/95 p-5 shadow-[0_14px_40px_rgba(15,23,42,0.06)]">
            <div class="flex items-center gap-2">
              <ShieldCheck class="h-5 w-5 text-emerald-600" aria-hidden="true" />
              <h2 class="font-bold text-slate-950">{{ t('portalAutoLogin.flowTitle') }}</h2>
            </div>
            <ol class="mt-4 space-y-3 text-sm leading-6 text-slate-600">
              <li v-for="index in 4" :key="index" class="flex gap-3">
                <span class="flex h-6 w-6 shrink-0 items-center justify-center rounded-full bg-slate-100 text-xs font-bold text-slate-600">{{ index }}</span>
                <span>{{ t(`portalAutoLogin.flow.${index}`) }}</span>
              </li>
            </ol>
          </section>
        </aside>
      </section>

      <section class="rounded-[24px] border border-slate-200/80 bg-white/95 p-6 shadow-[0_14px_40px_rgba(15,23,42,0.06)]">
        <div class="flex items-center justify-between gap-4">
          <div>
            <h2 class="text-lg font-bold text-slate-950">{{ t('portalAutoLogin.runLog') }}</h2>
            <p class="mt-1 text-sm text-slate-500">{{ t('portalAutoLogin.runLogHint') }}</p>
          </div>
          <span v-if="lastResult" class="rounded-full bg-slate-100 px-3 py-1 text-xs font-semibold text-slate-600">{{ t('portalAutoLogin.attempts', { count: lastResult.attempts }) }}</span>
        </div>
        <div v-if="lastResult?.steps.length" class="mt-5 grid gap-3 md:grid-cols-2">
          <div v-for="(step, index) in lastResult.steps" :key="`${index}-${step.code}`" class="flex min-h-12 items-start gap-3 rounded-xl border border-slate-100 bg-slate-50/80 px-3 py-3 text-sm text-slate-700">
            <span class="mt-1.5 h-2 w-2 shrink-0 rounded-full" :class="stepDotClass(step.level)" aria-hidden="true"></span>
            <span class="leading-5">{{ stepText(step) }}</span>
          </div>
        </div>
        <div v-else class="mt-5 flex min-h-28 flex-col items-center justify-center rounded-2xl border border-dashed border-slate-200 bg-slate-50/60 px-5 text-center">
          <Wifi class="h-6 w-6 text-slate-400" aria-hidden="true" />
          <p class="mt-2 text-sm font-semibold text-slate-600">{{ t('portalAutoLogin.noRunLog') }}</p>
        </div>
      </section>
    </main>
  </div>
</template>
