<script setup lang="ts">
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import {
  AlertCircle,
  Check,
  ChevronRight,
  Download,
  FileCode2,
  FolderOpen,
  LoaderCircle,
  Package,
  Palette,
  Plus,
  RefreshCw,
  Save,
  Search,
  Settings2,
  ShieldCheck,
  Trash2,
} from 'lucide-vue-next';
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import { useI18n } from 'vue-i18n';

import {
  getConfig,
  notepadExtensionsApi,
  openPathParent,
  type EnhanceAnyLexerConfig,
  type EnhanceAnyLexerRule,
  type EnhanceAnyLexerSection,
  type NotepadInstance,
  type NotepadPluginCatalog,
  type NotepadPluginCatalogEntry,
  type NotepadPluginInstallProgress,
  type NotepadPluginPackage,
} from '@/lib/tauri';

defineOptions({ name: 'NotepadExtensionsPage' });

type PageTab = 'catalog' | 'enhance';

const { t, locale } = useI18n();
const tab = ref<PageTab>('catalog');
const instances = ref<NotepadInstance[]>([]);
const selectedExePath = ref('');
const catalog = ref<NotepadPluginCatalog | null>(null);
const serverUrl = ref('');
const searchKeyword = ref('');
const loadingInstances = ref(false);
const loadingCatalog = ref(false);
const installingPluginId = ref('');
const installPhase = ref('');
const notice = ref<{ kind: 'success' | 'warning' | 'error'; message: string } | null>(null);
const enhanceConfig = ref<EnhanceAnyLexerConfig | null>(null);
const activeSectionIndex = ref(0);
const loadingEnhance = ref(false);
const savingEnhance = ref(false);
const testText = ref('2026-07-17 10:23:01 INFO Service started\n2026-07-17 10:23:08 WARN Connection is slow\n2026-07-17 10:23:12 ERROR Connection Timeout');
let unlistenInstall: UnlistenFn | null = null;

const selectedInstance = computed(() =>
  instances.value.find((item) => item.exe_path === selectedExePath.value) ?? null,
);

const catalogPlugins = computed(() => {
  const keyword = searchKeyword.value.trim().toLowerCase();
  const plugins = catalog.value?.plugins ?? [];
  if (!keyword) return plugins;
  return plugins.filter((plugin) =>
    [plugin.name, plugin.publisher, plugin.description_en, plugin.description_zh]
      .join(' ')
      .toLowerCase()
      .includes(keyword),
  );
});

const activeSection = computed<EnhanceAnyLexerSection | null>(() =>
  enhanceConfig.value?.sections[activeSectionIndex.value] ?? null,
);

const previewLines = computed(() => testText.value.split('\n'));

function pluginDescription(plugin: NotepadPluginCatalogEntry) {
  const isChinese = locale.value.toLowerCase().startsWith('zh');
  return (isChinese ? plugin.description_zh : plugin.description_en)
    || plugin.description_zh
    || plugin.description_en;
}

function latestRelease(plugin: NotepadPluginCatalogEntry) {
  return plugin.releases[0] ?? null;
}

function packageFor(plugin: NotepadPluginCatalogEntry): NotepadPluginPackage | null {
  const architecture = selectedInstance.value?.architecture_key;
  if (!architecture) return null;
  return latestRelease(plugin)?.packages[architecture] ?? null;
}

function isInstalled(plugin: NotepadPluginCatalogEntry) {
  const packageInfo = packageFor(plugin);
  if (!packageInfo || !selectedInstance.value) return false;
  return selectedInstance.value.installed_plugins.some(
    (installed) => installed.name.toLowerCase() === packageInfo.install_dir.toLowerCase(),
  );
}

function installPhaseLabel(phase: string) {
  return t(`notepadExtensions.install.phases.${phase || 'preparing'}`);
}

function friendlyError(error: unknown) {
  const raw = String(error);
  const known = [
    'notepad_executable_not_found',
    'not_notepad_executable',
    'notepad_marker_files_missing',
    'notepad_architecture_unsupported',
    'plugin_server_not_configured',
    'plugin_install_permission_denied',
    'plugin_update_requires_notepad_exit',
    'plugin_architecture_mismatch',
    'plugin_sha256_mismatch',
    'plugin_entry_dll_missing',
  ].find((key) => raw.includes(key));
  return known ? t(`notepadExtensions.errors.${known}`) : raw;
}

async function refreshInstances(preferredPath?: string) {
  loadingInstances.value = true;
  try {
    instances.value = await notepadExtensionsApi.detectInstances();
    const target = preferredPath
      ?? (instances.value.some((item) => item.exe_path === selectedExePath.value)
        ? selectedExePath.value
        : instances.value[0]?.exe_path);
    selectedExePath.value = target ?? '';
  } catch (error) {
    notice.value = { kind: 'error', message: friendlyError(error) };
  } finally {
    loadingInstances.value = false;
  }
}

async function choosePortable() {
  const path = await notepadExtensionsApi.pickExecutable();
  if (!path) return;
  try {
    const instance = await notepadExtensionsApi.validateInstance(path);
    const existingIndex = instances.value.findIndex((item) => item.exe_path === instance.exe_path);
    if (existingIndex >= 0) instances.value.splice(existingIndex, 1, instance);
    else instances.value.push(instance);
    selectedExePath.value = instance.exe_path;
    notice.value = { kind: 'success', message: t('notepadExtensions.instance.valid') };
  } catch (error) {
    notice.value = { kind: 'error', message: friendlyError(error) };
  }
}

async function refreshCatalog() {
  if (!serverUrl.value.trim()) {
    catalog.value = null;
    return;
  }
  loadingCatalog.value = true;
  try {
    catalog.value = await notepadExtensionsApi.fetchCatalog(serverUrl.value);
  } catch (error) {
    catalog.value = null;
    notice.value = {
      kind: 'warning',
      message: `${t('notepadExtensions.catalog.unavailable')} ${friendlyError(error)}`,
    };
  } finally {
    loadingCatalog.value = false;
  }
}

async function installPlugin(plugin: NotepadPluginCatalogEntry) {
  const instance = selectedInstance.value;
  const packageInfo = packageFor(plugin);
  if (!instance || !packageInfo) return;
  installingPluginId.value = plugin.id;
  installPhase.value = 'preparing';
  notice.value = null;
  try {
    const result = await notepadExtensionsApi.installPlugin(
      serverUrl.value,
      instance.exe_path,
      plugin.id,
      packageInfo,
    );
    await refreshInstances(instance.exe_path);
    notice.value = {
      kind: result.restart_required ? 'warning' : 'success',
      message: result.restart_required
        ? t('notepadExtensions.install.restartRequired')
        : t('notepadExtensions.install.success', { name: plugin.name }),
    };
  } catch (error) {
    notice.value = { kind: 'error', message: friendlyError(error) };
  } finally {
    installingPluginId.value = '';
    installPhase.value = '';
  }
}

async function openEnhanceConfiguration() {
  if (!selectedInstance.value) return;
  tab.value = 'enhance';
  loadingEnhance.value = true;
  notice.value = null;
  try {
    enhanceConfig.value = await notepadExtensionsApi.readEnhanceConfig(
      selectedInstance.value.exe_path,
    );
    activeSectionIndex.value = 0;
  } catch (error) {
    notice.value = { kind: 'error', message: friendlyError(error) };
  } finally {
    loadingEnhance.value = false;
  }
}

function addSection() {
  if (!enhanceConfig.value) return;
  enhanceConfig.value.sections.push({ lexer: 'normal text', excluded_styles: [], rules: [] });
  activeSectionIndex.value = enhanceConfig.value.sections.length - 1;
}

function removeSection(index: number) {
  if (!enhanceConfig.value) return;
  enhanceConfig.value.sections.splice(index, 1);
  activeSectionIndex.value = Math.max(0, Math.min(activeSectionIndex.value, enhanceConfig.value.sections.length - 1));
}

function newRule(): EnhanceAnyLexerRule {
  return {
    id: crypto.randomUUID(),
    name: t('notepadExtensions.enhance.newRule'),
    enabled: true,
    color: '#8B5CF6',
    pattern: '\\bKEYWORD\\b',
    whitelist_styles: [],
  };
}

function addRule() {
  activeSection.value?.rules.push(newRule());
}

function removeRule(index: number) {
  activeSection.value?.rules.splice(index, 1);
}

function styleListText(values: number[]) {
  return values.join(',');
}

function updateStyleList(target: number[], value: string) {
  const next = value
    .split(',')
    .map((part) => Number.parseInt(part.trim(), 10))
    .filter((item) => Number.isFinite(item));
  target.splice(0, target.length, ...next);
}

function ruleMatches(rule: EnhanceAnyLexerRule, line: string) {
  if (!rule.enabled || !rule.pattern) return false;
  try {
    return new RegExp(rule.pattern, 'i').test(line);
  } catch {
    return false;
  }
}

function previewSegments(line: string) {
  const section = activeSection.value;
  if (!section) return [{ text: line, color: '' }];
  for (const rule of section.rules) {
    if (!ruleMatches(rule, line)) continue;
    try {
      const expression = new RegExp(rule.pattern, 'gi');
      const segments: Array<{ text: string; color: string }> = [];
      let cursor = 0;
      for (const match of line.matchAll(expression)) {
        const start = match.index ?? 0;
        if (start > cursor) segments.push({ text: line.slice(cursor, start), color: '' });
        const matched = match[0];
        if (!matched) break;
        segments.push({ text: matched, color: rule.color });
        cursor = start + matched.length;
      }
      if (cursor < line.length) segments.push({ text: line.slice(cursor), color: '' });
      return segments.length ? segments : [{ text: line, color: '' }];
    } catch {
      return [{ text: line, color: '' }];
    }
  }
  return [{ text: line, color: '' }];
}

async function saveEnhance() {
  if (!selectedInstance.value || !enhanceConfig.value) return;
  savingEnhance.value = true;
  notice.value = null;
  try {
    const result = await notepadExtensionsApi.saveEnhanceConfig(
      selectedInstance.value.exe_path,
      enhanceConfig.value,
    );
    notice.value = {
      kind: result.restart_required ? 'warning' : 'success',
      message: result.restart_required
        ? t('notepadExtensions.enhance.savedRestart')
        : t('notepadExtensions.enhance.saved'),
    };
    await refreshInstances(selectedInstance.value.exe_path);
  } catch (error) {
    notice.value = { kind: 'error', message: friendlyError(error) };
  } finally {
    savingEnhance.value = false;
  }
}

async function revealPath(path: string) {
  try {
    await openPathParent(path);
  } catch (error) {
    notice.value = { kind: 'error', message: friendlyError(error) };
  }
}

onMounted(async () => {
  unlistenInstall = await listen<NotepadPluginInstallProgress>(
    'notepad-plugin-install-progress',
    (event) => {
      if (event.payload.plugin_id === installingPluginId.value) {
        installPhase.value = event.payload.phase;
      }
    },
  );
  try {
    serverUrl.value = (await getConfig()).update_server_url;
  } catch {
    serverUrl.value = '';
  }
  await Promise.all([refreshInstances(), refreshCatalog()]);
});

onBeforeUnmount(() => unlistenInstall?.());
</script>

<template>
  <div class="flex-1 overflow-y-auto bg-gradient-to-b from-violet-50/70 via-slate-50 to-white">
    <div class="mx-auto flex w-full max-w-7xl flex-col gap-5 px-6 py-6 pb-12">
      <header class="flex flex-wrap items-start justify-between gap-4">
        <div>
          <div class="mb-2 inline-flex items-center gap-2 rounded-full border border-violet-200 bg-white px-3 py-1 text-xs font-semibold text-violet-700 shadow-sm">
            <Package class="h-3.5 w-3.5" aria-hidden="true" />
            {{ t('notepadExtensions.eyebrow') }}
          </div>
          <h1 class="text-2xl font-bold tracking-tight text-slate-950">{{ t('notepadExtensions.title') }}</h1>
          <p class="mt-1 max-w-3xl text-sm leading-6 text-slate-600">{{ t('notepadExtensions.description') }}</p>
        </div>
        <button
          type="button"
          class="inline-flex min-h-11 cursor-pointer items-center gap-2 rounded-xl border border-slate-200 bg-white px-4 text-sm font-semibold text-slate-700 shadow-sm transition-colors hover:border-violet-300 hover:text-violet-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-violet-500"
          :disabled="loadingInstances"
          @click="refreshInstances()"
        >
          <RefreshCw class="h-4 w-4" :class="loadingInstances ? 'animate-spin' : ''" />
          {{ t('notepadExtensions.instance.rescan') }}
        </button>
      </header>

      <section class="rounded-2xl border border-slate-200 bg-white p-4 shadow-[0_18px_50px_-38px_rgba(15,23,42,0.6)]">
        <div class="flex flex-wrap items-end gap-3">
          <label class="min-w-[260px] flex-1 space-y-1.5">
            <span class="text-xs font-semibold uppercase tracking-wide text-slate-500">{{ t('notepadExtensions.instance.label') }}</span>
            <select
              v-model="selectedExePath"
              class="min-h-11 w-full rounded-xl border border-slate-300 bg-white px-3 text-sm text-slate-800 outline-none transition focus:border-violet-500 focus:ring-2 focus:ring-violet-100"
            >
              <option value="" disabled>{{ t('notepadExtensions.instance.empty') }}</option>
              <option v-for="instance in instances" :key="instance.exe_path" :value="instance.exe_path">
                {{ instance.architecture_key.toUpperCase() }} · {{ instance.exe_path }}
              </option>
            </select>
          </label>
          <button
            type="button"
            class="inline-flex min-h-11 cursor-pointer items-center gap-2 rounded-xl bg-slate-900 px-4 text-sm font-semibold text-white transition-colors hover:bg-violet-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-violet-500 focus-visible:ring-offset-2"
            @click="choosePortable"
          >
            <FolderOpen class="h-4 w-4" />
            {{ t('notepadExtensions.instance.choosePortable') }}
          </button>
        </div>

        <div v-if="selectedInstance" class="mt-4 grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
          <div class="rounded-xl bg-slate-50 p-3">
            <p class="text-xs font-medium text-slate-500">{{ t('notepadExtensions.instance.architecture') }}</p>
            <p class="mt-1 font-mono text-sm font-semibold text-slate-800">{{ selectedInstance.architecture_key.toUpperCase() }}</p>
          </div>
          <div class="rounded-xl bg-slate-50 p-3">
            <p class="text-xs font-medium text-slate-500">{{ t('notepadExtensions.instance.mode') }}</p>
            <p class="mt-1 text-sm font-semibold text-slate-800">{{ selectedInstance.portable ? t('notepadExtensions.instance.portable') : t('notepadExtensions.instance.installed') }}</p>
          </div>
          <div class="rounded-xl bg-slate-50 p-3">
            <p class="text-xs font-medium text-slate-500">{{ t('notepadExtensions.instance.process') }}</p>
            <p class="mt-1 text-sm font-semibold" :class="selectedInstance.running ? 'text-emerald-700' : 'text-slate-700'">
              {{ selectedInstance.running ? t('notepadExtensions.instance.running') : t('notepadExtensions.instance.stopped') }}
            </p>
          </div>
          <div class="rounded-xl bg-slate-50 p-3">
            <p class="text-xs font-medium text-slate-500">{{ t('notepadExtensions.instance.plugins') }}</p>
            <p class="mt-1 text-sm font-semibold text-slate-800">{{ selectedInstance.installed_plugins.length }}</p>
          </div>
        </div>
        <div
          v-if="selectedInstance?.requires_elevation"
          class="mt-3 flex items-start gap-2 rounded-xl border border-amber-200 bg-amber-50 px-3 py-2.5 text-xs leading-5 text-amber-900"
        >
          <AlertCircle class="mt-0.5 h-4 w-4 shrink-0 text-amber-600" />
          {{ t('notepadExtensions.instance.requiresAdmin') }}
        </div>
      </section>

      <div
        v-if="notice"
        class="flex items-start gap-3 rounded-xl border px-4 py-3 text-sm"
        :class="{
          'border-emerald-200 bg-emerald-50 text-emerald-800': notice.kind === 'success',
          'border-amber-200 bg-amber-50 text-amber-900': notice.kind === 'warning',
          'border-rose-200 bg-rose-50 text-rose-800': notice.kind === 'error',
        }"
        role="status"
      >
        <Check v-if="notice.kind === 'success'" class="mt-0.5 h-4 w-4 shrink-0" />
        <AlertCircle v-else class="mt-0.5 h-4 w-4 shrink-0" />
        <span>{{ notice.message }}</span>
      </div>

      <nav class="flex gap-1 rounded-xl border border-slate-200 bg-white p-1" :aria-label="t('notepadExtensions.tabs.label')">
        <button
          v-for="item in ([['catalog', 'notepadExtensions.tabs.catalog'], ['enhance', 'notepadExtensions.tabs.enhance']] as const)"
          :key="item[0]"
          type="button"
          class="min-h-10 flex-1 cursor-pointer rounded-lg px-4 text-sm font-semibold transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-violet-500"
          :class="tab === item[0] ? 'bg-violet-600 text-white shadow-sm' : 'text-slate-600 hover:bg-slate-50 hover:text-slate-900'"
          @click="item[0] === 'enhance' ? openEnhanceConfiguration() : tab = 'catalog'"
        >
          {{ t(item[1]) }}
        </button>
      </nav>

      <section v-if="tab === 'catalog'" class="space-y-4">
        <div class="flex flex-wrap gap-3">
          <label class="relative min-w-[260px] flex-1">
            <Search class="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-slate-400" />
            <input
              v-model.trim="searchKeyword"
              type="search"
              class="min-h-11 w-full rounded-xl border border-slate-300 bg-white pl-10 pr-3 text-sm outline-none transition focus:border-violet-500 focus:ring-2 focus:ring-violet-100"
              :placeholder="t('notepadExtensions.catalog.search')"
            />
          </label>
          <button
            type="button"
            class="inline-flex min-h-11 cursor-pointer items-center gap-2 rounded-xl border border-slate-200 bg-white px-4 text-sm font-semibold text-slate-700 transition-colors hover:border-violet-300 hover:text-violet-700"
            :disabled="loadingCatalog"
            @click="refreshCatalog"
          >
            <RefreshCw class="h-4 w-4" :class="loadingCatalog ? 'animate-spin' : ''" />
            {{ t('notepadExtensions.catalog.refresh') }}
          </button>
        </div>

        <div v-if="loadingCatalog" class="flex min-h-48 items-center justify-center rounded-2xl border border-slate-200 bg-white">
          <LoaderCircle class="h-6 w-6 animate-spin text-violet-600" />
        </div>
        <div v-else-if="catalogPlugins.length" class="grid gap-4 lg:grid-cols-2">
          <article
            v-for="plugin in catalogPlugins"
            :key="plugin.id"
            class="flex flex-col rounded-2xl border border-slate-200 bg-white p-5 shadow-[0_18px_45px_-38px_rgba(15,23,42,0.8)] transition-colors hover:border-violet-300"
          >
            <div class="flex items-start gap-4">
              <div class="flex h-11 w-11 shrink-0 items-center justify-center rounded-xl bg-gradient-to-br from-violet-500 to-indigo-600 text-white shadow-lg shadow-violet-200">
                <Palette v-if="plugin.adapter === 'enhance-any-lexer'" class="h-5 w-5" />
                <Package v-else class="h-5 w-5" />
              </div>
              <div class="min-w-0 flex-1">
                <div class="flex flex-wrap items-center gap-2">
                  <h2 class="font-bold text-slate-900">{{ plugin.name }}</h2>
                  <span v-if="isInstalled(plugin)" class="rounded-full bg-emerald-100 px-2 py-0.5 text-xs font-semibold text-emerald-700">{{ t('notepadExtensions.catalog.installed') }}</span>
                  <span v-if="plugin.adapter" class="rounded-full bg-violet-100 px-2 py-0.5 text-xs font-semibold text-violet-700">{{ t('notepadExtensions.catalog.visualConfig') }}</span>
                </div>
                <p class="mt-1 text-xs text-slate-500">{{ plugin.publisher }} · {{ latestRelease(plugin)?.version }} · {{ plugin.license }}</p>
              </div>
            </div>
            <p class="mt-4 flex-1 text-sm leading-6 text-slate-600">{{ pluginDescription(plugin) }}</p>
            <div class="mt-5 flex flex-wrap items-center justify-between gap-3 border-t border-slate-100 pt-4">
              <span class="text-xs text-slate-500">
                {{ packageFor(plugin) ? `${selectedInstance?.architecture_key.toUpperCase()} · ${latestRelease(plugin)?.notepad_compatible || 'Notepad++'}` : t('notepadExtensions.catalog.incompatible') }}
              </span>
              <div class="flex gap-2">
                <button
                  v-if="plugin.adapter === 'enhance-any-lexer' && isInstalled(plugin)"
                  type="button"
                  class="inline-flex min-h-10 cursor-pointer items-center gap-2 rounded-lg border border-slate-200 px-3 text-sm font-semibold text-slate-700 hover:border-violet-300 hover:text-violet-700"
                  @click="openEnhanceConfiguration"
                >
                  <Settings2 class="h-4 w-4" />
                  {{ t('notepadExtensions.catalog.configure') }}
                </button>
                <button
                  type="button"
                  class="inline-flex min-h-10 cursor-pointer items-center gap-2 rounded-lg bg-violet-600 px-4 text-sm font-semibold text-white transition-colors hover:bg-violet-700 disabled:cursor-not-allowed disabled:bg-slate-300"
                  :disabled="!selectedInstance || !packageFor(plugin) || installingPluginId !== ''"
                  @click="installPlugin(plugin)"
                >
                  <LoaderCircle v-if="installingPluginId === plugin.id" class="h-4 w-4 animate-spin" />
                  <Download v-else class="h-4 w-4" />
                  {{ installingPluginId === plugin.id ? installPhaseLabel(installPhase) : isInstalled(plugin) ? t('notepadExtensions.catalog.reinstall') : t('notepadExtensions.catalog.install') }}
                </button>
              </div>
            </div>
          </article>
        </div>
        <div v-else class="rounded-2xl border border-dashed border-slate-300 bg-white px-6 py-12 text-center">
          <Package class="mx-auto h-8 w-8 text-slate-300" />
          <h2 class="mt-3 font-semibold text-slate-800">{{ t('notepadExtensions.catalog.emptyTitle') }}</h2>
          <p class="mt-1 text-sm text-slate-500">{{ t('notepadExtensions.catalog.emptyDescription') }}</p>
        </div>
      </section>

      <section v-else class="space-y-4">
        <div v-if="!selectedInstance?.enhance_any_lexer.installed" class="rounded-2xl border border-amber-200 bg-amber-50 p-6 text-center">
          <AlertCircle class="mx-auto h-7 w-7 text-amber-600" />
          <h2 class="mt-3 font-bold text-amber-950">{{ t('notepadExtensions.enhance.notInstalledTitle') }}</h2>
          <p class="mt-1 text-sm text-amber-800">{{ t('notepadExtensions.enhance.notInstalledDescription') }}</p>
          <button type="button" class="mt-4 min-h-10 cursor-pointer rounded-lg bg-amber-900 px-4 text-sm font-semibold text-white" @click="tab = 'catalog'">
            {{ t('notepadExtensions.enhance.backToCatalog') }}
          </button>
        </div>
        <div v-else-if="loadingEnhance" class="flex min-h-56 items-center justify-center rounded-2xl border border-slate-200 bg-white">
          <LoaderCircle class="h-7 w-7 animate-spin text-violet-600" />
        </div>
        <template v-else-if="enhanceConfig">
          <div class="grid gap-4 xl:grid-cols-[260px_minmax(0,1fr)]">
            <aside class="rounded-2xl border border-slate-200 bg-white p-4">
              <div class="flex items-center justify-between gap-2">
                <div>
                  <h2 class="font-bold text-slate-900">{{ t('notepadExtensions.enhance.lexers') }}</h2>
                  <p class="text-xs text-slate-500">{{ t('notepadExtensions.enhance.lexersHint') }}</p>
                </div>
                <button type="button" class="flex h-10 w-10 cursor-pointer items-center justify-center rounded-lg border border-slate-200 text-slate-600 hover:border-violet-300 hover:text-violet-700" :aria-label="t('notepadExtensions.enhance.addLexer')" @click="addSection">
                  <Plus class="h-4 w-4" />
                </button>
              </div>
              <div class="mt-4 space-y-2">
                <button
                  v-for="(section, index) in enhanceConfig.sections"
                  :key="`${section.lexer}-${index}`"
                  type="button"
                  class="flex min-h-11 w-full cursor-pointer items-center gap-2 rounded-xl border px-3 text-left text-sm transition-colors"
                  :class="activeSectionIndex === index ? 'border-violet-300 bg-violet-50 font-semibold text-violet-800' : 'border-transparent bg-slate-50 text-slate-700 hover:border-slate-200'"
                  @click="activeSectionIndex = index"
                >
                  <FileCode2 class="h-4 w-4 shrink-0" />
                  <span class="min-w-0 flex-1 truncate">{{ section.lexer }}</span>
                  <span class="text-xs opacity-60">{{ section.rules.length }}</span>
                  <ChevronRight class="h-4 w-4 opacity-50" />
                </button>
              </div>
            </aside>

            <div v-if="activeSection" class="space-y-4">
              <section class="rounded-2xl border border-slate-200 bg-white p-5">
                <div class="flex flex-wrap items-end gap-3">
                  <label class="min-w-[220px] flex-1 space-y-1.5">
                    <span class="text-xs font-semibold text-slate-600">{{ t('notepadExtensions.enhance.lexerName') }}</span>
                    <input v-model.trim="activeSection.lexer" class="min-h-11 w-full rounded-xl border border-slate-300 px-3 text-sm outline-none focus:border-violet-500 focus:ring-2 focus:ring-violet-100" />
                  </label>
                  <label class="min-w-[240px] flex-1 space-y-1.5">
                    <span class="text-xs font-semibold text-slate-600">{{ t('notepadExtensions.enhance.excludedStyles') }}</span>
                    <input :value="styleListText(activeSection.excluded_styles)" class="min-h-11 w-full rounded-xl border border-slate-300 px-3 font-mono text-sm outline-none focus:border-violet-500 focus:ring-2 focus:ring-violet-100" placeholder="1,3,4,6" @change="updateStyleList(activeSection.excluded_styles, ($event.target as HTMLInputElement).value)" />
                  </label>
                  <button type="button" class="flex min-h-11 cursor-pointer items-center gap-2 rounded-xl border border-rose-200 px-3 text-sm font-semibold text-rose-700 hover:bg-rose-50" @click="removeSection(activeSectionIndex)">
                    <Trash2 class="h-4 w-4" />
                    {{ t('notepadExtensions.enhance.deleteLexer') }}
                  </button>
                </div>
              </section>

              <section class="rounded-2xl border border-slate-200 bg-white p-5">
                <div class="flex items-center justify-between gap-3">
                  <div>
                    <h2 class="font-bold text-slate-900">{{ t('notepadExtensions.enhance.rules') }}</h2>
                    <p class="text-xs text-slate-500">{{ t('notepadExtensions.enhance.rulesHint') }}</p>
                  </div>
                  <button type="button" class="inline-flex min-h-10 cursor-pointer items-center gap-2 rounded-lg bg-violet-600 px-3 text-sm font-semibold text-white hover:bg-violet-700" @click="addRule">
                    <Plus class="h-4 w-4" />
                    {{ t('notepadExtensions.enhance.addRule') }}
                  </button>
                </div>

                <div class="mt-4 space-y-3">
                  <article v-for="(rule, index) in activeSection.rules" :key="rule.id" class="rounded-xl border border-slate-200 bg-slate-50/70 p-4">
                    <div class="grid gap-3 lg:grid-cols-[auto_minmax(140px,0.7fr)_120px_minmax(240px,1.5fr)_auto] lg:items-end">
                      <label class="flex min-h-11 cursor-pointer items-center gap-2 rounded-lg px-1 text-sm font-medium text-slate-700">
                        <input v-model="rule.enabled" type="checkbox" class="h-4 w-4 rounded border-slate-300 text-violet-600 focus:ring-violet-500" />
                        {{ t('notepadExtensions.enhance.enabled') }}
                      </label>
                      <label class="space-y-1">
                        <span class="text-xs font-semibold text-slate-600">{{ t('notepadExtensions.enhance.ruleName') }}</span>
                        <input v-model.trim="rule.name" class="min-h-10 w-full rounded-lg border border-slate-300 bg-white px-3 text-sm outline-none focus:border-violet-500" />
                      </label>
                      <label class="space-y-1">
                        <span class="text-xs font-semibold text-slate-600">{{ t('notepadExtensions.enhance.color') }}</span>
                        <div class="flex min-h-10 items-center gap-2 rounded-lg border border-slate-300 bg-white px-2">
                          <input v-model="rule.color" type="color" class="h-7 w-8 cursor-pointer border-0 bg-transparent p-0" />
                          <span class="font-mono text-xs text-slate-600">{{ rule.color }}</span>
                        </div>
                      </label>
                      <label class="space-y-1">
                        <span class="text-xs font-semibold text-slate-600">{{ t('notepadExtensions.enhance.pattern') }}</span>
                        <input v-model="rule.pattern" class="min-h-10 w-full rounded-lg border border-slate-300 bg-white px-3 font-mono text-sm outline-none focus:border-violet-500" />
                      </label>
                      <button type="button" class="flex h-10 w-10 cursor-pointer items-center justify-center rounded-lg text-slate-400 hover:bg-rose-50 hover:text-rose-600" :aria-label="t('notepadExtensions.enhance.deleteRule')" @click="removeRule(index)">
                        <Trash2 class="h-4 w-4" />
                      </button>
                    </div>
                  </article>
                  <div v-if="activeSection.rules.length === 0" class="rounded-xl border border-dashed border-slate-300 px-5 py-8 text-center text-sm text-slate-500">
                    {{ t('notepadExtensions.enhance.emptyRules') }}
                  </div>
                </div>
              </section>

              <section class="grid gap-4 lg:grid-cols-2">
                <div class="rounded-2xl border border-slate-200 bg-white p-5">
                  <h2 class="font-bold text-slate-900">{{ t('notepadExtensions.enhance.testText') }}</h2>
                  <textarea v-model="testText" rows="7" class="mt-3 w-full resize-y rounded-xl border border-slate-300 p-3 font-mono text-sm leading-6 outline-none focus:border-violet-500 focus:ring-2 focus:ring-violet-100" />
                </div>
                <div class="rounded-2xl border border-slate-800 bg-slate-950 p-5 text-slate-300 shadow-xl">
                  <div class="flex items-center justify-between gap-3">
                    <h2 class="font-bold text-white">{{ t('notepadExtensions.enhance.preview') }}</h2>
                    <span class="rounded-full bg-slate-800 px-2 py-1 text-[11px] text-slate-400">{{ t('notepadExtensions.enhance.previewHint') }}</span>
                  </div>
                  <pre class="mt-3 overflow-x-auto whitespace-pre-wrap font-mono text-sm leading-6"><span v-for="(line, lineIndex) in previewLines" :key="lineIndex"><span v-for="(segment, segmentIndex) in previewSegments(line)" :key="segmentIndex" :style="segment.color ? { color: segment.color, fontWeight: 700 } : undefined">{{ segment.text }}</span><br v-if="lineIndex < previewLines.length - 1" /></span></pre>
                </div>
              </section>

              <div class="sticky bottom-4 flex flex-wrap items-center justify-between gap-3 rounded-2xl border border-violet-200 bg-white/95 p-4 shadow-xl backdrop-blur">
                <button type="button" class="inline-flex min-h-10 cursor-pointer items-center gap-2 text-sm font-semibold text-slate-600 hover:text-violet-700" @click="revealPath(selectedInstance.enhance_any_lexer.config_path)">
                  <FolderOpen class="h-4 w-4" />
                  {{ t('notepadExtensions.enhance.openConfigFolder') }}
                </button>
                <button type="button" class="inline-flex min-h-11 cursor-pointer items-center gap-2 rounded-xl bg-violet-600 px-5 text-sm font-bold text-white transition-colors hover:bg-violet-700 disabled:cursor-not-allowed disabled:bg-slate-300" :disabled="savingEnhance" @click="saveEnhance">
                  <LoaderCircle v-if="savingEnhance" class="h-4 w-4 animate-spin" />
                  <Save v-else class="h-4 w-4" />
                  {{ savingEnhance ? t('notepadExtensions.enhance.saving') : t('notepadExtensions.enhance.save') }}
                </button>
              </div>
            </div>
          </div>
        </template>
      </section>

      <footer class="flex items-center gap-2 text-xs text-slate-500">
        <ShieldCheck class="h-4 w-4 text-emerald-600" />
        {{ t('notepadExtensions.securityNote') }}
      </footer>
    </div>
  </div>
</template>
