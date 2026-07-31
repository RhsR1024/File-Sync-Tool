<script setup lang="ts">
import {
  Check,
  Download,
  Eye,
  EyeOff,
  Import,
  RotateCcw,
  StickyNote,
} from 'lucide-vue-next';
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import { useI18n } from 'vue-i18n';

import { usePaperTodo } from '@/composables/usePaperTodo';
import type { PaperSkin, PaperTodoSettings } from '@/lib/paperTodo';
import {
  PAPER_TODO_HOTKEY_IDS,
  PAPER_TODO_SETTINGS_TABS,
  PAPER_TODO_TOGGLE_GROUPS,
  type PaperTodoSettingsTabId,
} from '@/lib/paperTodoSettingsUi';

const props = withDefaults(defineProps<{
  todoCount?: number;
  noteCount?: number;
  openCount?: number;
  busy?: boolean;
}>(), {
  todoCount: 0,
  noteCount: 0,
  openCount: 0,
  busy: false,
});

const emit = defineEmits<{
  showAll: [];
  hideAll: [];
  importData: [];
  exportData: [];
  cleanAssets: [];
}>();

const { t } = useI18n();
const store = usePaperTodo();
const settings = computed(() => store.state.value.settings);
const activeTab = ref<PaperTodoSettingsTabId>('launcher');
const tablist = ref<HTMLElement | null>(null);

const TEXT_SIZES = ['small', 'medium', 'large', 'xlarge'] as const;
const TEXT_SIZE_FIELDS = [
  { key: 'titleFontSize', label: 'paperTodo.settings.titleSize' },
  { key: 'todoFontSize', label: 'paperTodo.settings.todoSize' },
  { key: 'noteFontSize', label: 'paperTodo.settings.noteSize' },
] as const satisfies ReadonlyArray<{ key: keyof PaperTodoSettings; label: string }>;
const SKINS: ReadonlyArray<{ id: PaperSkin; label: string; hint: string }> = [
  { id: 'classic', label: 'paperTodo.settings.skins.classic', hint: 'paperTodo.settings.skins.classicHint' },
  { id: 'grain', label: 'paperTodo.settings.skins.grain', hint: 'paperTodo.settings.skins.grainHint' },
  { id: 'quiet', label: 'paperTodo.settings.skins.quiet', hint: 'paperTodo.settings.skins.quietHint' },
  { id: 'desk', label: 'paperTodo.settings.skins.desk', hint: 'paperTodo.settings.skins.deskHint' },
];
const PALETTES = [
  { id: 'warm', color: '#b8791a' },
  { id: 'ink', color: '#52525b' },
  { id: 'forest', color: '#15803d' },
  { id: 'frost', color: '#0369a1' },
] as const;
const DARK_PALETTES = {
  warm: '#d9a441',
  ink: '#a1a1aa',
  forest: '#4ade80',
  frost: '#7dd3fc',
} as const;
const PREVIEW_TEXT_SIZES = { small: '8px', medium: '8.5px', large: '9.5px', xlarge: '10.5px' } as const;
const systemThemeQuery = typeof window !== 'undefined' ? window.matchMedia('(prefers-color-scheme: dark)') : null;
const systemDark = ref(systemThemeQuery?.matches ?? false);

const previewStyle = computed(() => ({
  '--preview-tint': settings.value.theme === 'dark' || (settings.value.theme === 'system' && systemDark.value)
    ? DARK_PALETTES[settings.value.palette]
    : PALETTES.find((item) => item.id === settings.value.palette)?.color ?? '#b8791a',
  '--preview-todo-size': PREVIEW_TEXT_SIZES[settings.value.todoFontSize],
  '--preview-row-weight': settings.value.todoBold ? '700' : '500',
  '--preview-title-weight': settings.value.titleBold ? '700' : '600',
  fontFamily: settings.value.fontFamily,
  fontSize: `${settings.value.interfaceScale}%`,
}));

function setSetting<K extends keyof PaperTodoSettings>(key: K, value: PaperTodoSettings[K]): void {
  store.updateSettings((current) => { current[key] = value; });
}

function setBoolean(key: keyof PaperTodoSettings, event: Event): void {
  setSetting(key, (event.target as HTMLInputElement).checked as never);
}

function setString(key: keyof PaperTodoSettings, event: Event): void {
  setSetting(key, (event.target as HTMLInputElement | HTMLSelectElement).value as never);
}

function setNumber(key: keyof PaperTodoSettings, event: Event): void {
  setSetting(key, Number((event.target as HTMLInputElement).value) as never);
}

function setHotkey(key: keyof PaperTodoSettings['hotkeys'], event: Event): void {
  const value = (event.target as HTMLInputElement).value.trim();
  store.updateSettings((current) => { current.hotkeys[key] = value; });
}

function recordHotkey(key: keyof PaperTodoSettings['hotkeys'], event: KeyboardEvent): void {
  if (event.key === 'Tab') return;
  event.preventDefault();
  if (event.key === 'Backspace' || event.key === 'Delete' || event.key === 'Escape') {
    store.updateSettings((current) => { current.hotkeys[key] = ''; });
    return;
  }
  if (['Control', 'Shift', 'Alt', 'Meta'].includes(event.key)) return;
  const parts: string[] = [];
  if (event.ctrlKey) parts.push('Ctrl');
  if (event.altKey) parts.push('Alt');
  if (event.shiftKey) parts.push('Shift');
  if (event.metaKey) parts.push('Super');
  const normalizedKey = event.key === ' ' ? 'Space' : event.key.length === 1 ? event.key.toUpperCase() : event.key;
  parts.push(normalizedKey);
  store.updateSettings((current) => { current.hotkeys[key] = parts.join('+'); });
}

function focusTab(index: number): void {
  tablist.value?.querySelector<HTMLButtonElement>(`[data-tab-index="${index}"]`)?.focus();
}

function onTabKeydown(event: KeyboardEvent, index: number): void {
  const last = PAPER_TODO_SETTINGS_TABS.length - 1;
  const targets: Record<string, number> = {
    ArrowDown: index === last ? 0 : index + 1,
    ArrowUp: index === 0 ? last : index - 1,
    Home: 0,
    End: last,
  };
  const next = targets[event.key];
  if (next === undefined) return;
  event.preventDefault();
  activeTab.value = PAPER_TODO_SETTINGS_TABS[next].id;
  focusTab(next);
}

function onSystemThemeChange(event: MediaQueryListEvent): void {
  systemDark.value = event.matches;
}

onMounted(() => systemThemeQuery?.addEventListener('change', onSystemThemeChange));
onBeforeUnmount(() => systemThemeQuery?.removeEventListener('change', onSystemThemeChange));
</script>

<template>
  <section class="paper-settings-layout">
    <aside class="paper-settings-nav">
      <div class="paper-settings-nav-title">{{ t('paperTodo.settings.navTitle') }}</div>
      <nav ref="tablist" class="paper-settings-tablist" role="tablist" :aria-label="t('paperTodo.settingsTitle')" aria-orientation="vertical">
        <button
          v-for="(tab, index) in PAPER_TODO_SETTINGS_TABS"
          :id="`paper-todo-settings-tab-${tab.id}`"
          :key="tab.id"
          type="button"
          role="tab"
          :data-tab-index="index"
          :aria-controls="`paper-todo-settings-panel-${tab.id}`"
          :aria-selected="activeTab === tab.id"
          :tabindex="activeTab === tab.id ? 0 : -1"
          :class="activeTab === tab.id && 'is-active'"
          @click="activeTab = tab.id"
          @keydown="onTabKeydown($event, index)"
        >
          <component :is="tab.icon" class="h-4 w-4" />
          <span>{{ t(tab.labelKey) }}</span>
        </button>
      </nav>
      <div class="paper-settings-stats">
        <span>{{ t('paperTodo.desktopStatus') }}</span>
        <strong>{{ t('paperTodo.desktopStatusSummary', { todos: props.todoCount, notes: props.noteCount, open: props.openCount }) }}</strong>
      </div>
    </aside>

    <main
      :id="`paper-todo-settings-panel-${activeTab}`"
      class="paper-settings-content"
      role="tabpanel"
      :aria-labelledby="`paper-todo-settings-tab-${activeTab}`"
    >
      <header class="paper-settings-section-head">
        <h2>{{ t(PAPER_TODO_SETTINGS_TABS.find((tab) => tab.id === activeTab)?.labelKey ?? '') }}</h2>
        <p>{{ t(`paperTodo.settings.tabHints.${activeTab}`) }}</p>
      </header>

      <template v-if="activeTab === 'launcher'">
        <div class="settings-card">
          <label class="settings-row">
            <span><strong>{{ t('paperTodo.settings.launcherEnabled') }}</strong><small>{{ t('paperTodo.settings.launcherEnabledHint') }}</small></span>
            <input class="paper-switch" type="checkbox" :checked="settings.launcherEnabled" @change="setBoolean('launcherEnabled', $event)">
          </label>
          <label class="settings-row">
            <span><strong>{{ t('paperTodo.settings.autoCollapseLauncher') }}</strong><small>{{ t('paperTodo.settings.autoCollapseLauncherHint') }}</small></span>
            <input class="paper-switch" type="checkbox" :checked="settings.autoCollapseLauncher" :disabled="!settings.launcherEnabled" @change="setBoolean('autoCollapseLauncher', $event)">
          </label>
          <div class="settings-row">
            <span><strong>{{ t('paperTodo.settings.launcherEdge') }}</strong><small>{{ t('paperTodo.settings.launcherEdgeHint') }}</small></span>
            <div class="settings-segmented">
              <button type="button" :class="settings.launcherEdge === 'left' && 'is-active'" :disabled="!settings.launcherEnabled" @click="setSetting('launcherEdge', 'left')">{{ t('paperTodo.settings.leftEdge') }}</button>
              <button type="button" :class="settings.launcherEdge === 'right' && 'is-active'" :disabled="!settings.launcherEnabled" @click="setSetting('launcherEdge', 'right')">{{ t('paperTodo.settings.rightEdge') }}</button>
            </div>
          </div>
          <label class="settings-row">
            <span><strong>{{ t('paperTodo.settings.launcherOffset') }}</strong><small>{{ settings.launcherOffset }}%</small></span>
            <input class="settings-range" type="range" min="0" max="100" step="5" :value="settings.launcherOffset" :disabled="!settings.launcherEnabled" @input="setNumber('launcherOffset', $event)">
          </label>
        </div>
        <div class="desktop-paper-card">
          <StickyNote class="h-4 w-4" />
          <span><strong>{{ t('paperTodo.desktopStatus') }}</strong><small>{{ t('paperTodo.desktopStatusSummary', { todos: props.todoCount, notes: props.noteCount, open: props.openCount }) }}</small></span>
          <button type="button" @click="emit('showAll')"><Eye class="h-3.5 w-3.5" />{{ t('paperTodo.showAllShort') }}</button>
          <button type="button" @click="emit('hideAll')"><EyeOff class="h-3.5 w-3.5" />{{ t('paperTodo.hideAllShort') }}</button>
        </div>
      </template>

      <template v-else-if="activeTab === 'appearance'">
        <div class="settings-card skin-card">
          <div class="settings-card-title">
            <strong>{{ t('paperTodo.settings.skin') }}</strong>
            <span>{{ t('paperTodo.settings.skinHint') }}</span>
          </div>
          <div class="skin-grid">
            <button
              v-for="skin in SKINS"
              :key="skin.id"
              type="button"
              class="skin-option"
              :class="settings.paperSkin === skin.id && 'is-active'"
              :aria-pressed="settings.paperSkin === skin.id"
              @click="setSetting('paperSkin', skin.id)"
            >
              <span class="skin-option-preview" :class="`is-${skin.id}`">
                <i></i><b></b><b></b><b></b><em></em>
              </span>
              <span><strong>{{ t(skin.label) }}</strong><small>{{ t(skin.hint) }}</small></span>
              <Check v-if="settings.paperSkin === skin.id" class="skin-check h-3.5 w-3.5" />
            </button>
          </div>
        </div>

        <div class="settings-card settings-grid-card">
          <label class="paper-setting-field"><span>{{ t('paperTodo.settings.theme') }}</span><select :value="settings.theme" @change="setString('theme', $event)"><option value="system">{{ t('paperTodo.settings.system') }}</option><option value="light">{{ t('paperTodo.settings.light') }}</option><option value="dark">{{ t('paperTodo.settings.dark') }}</option></select></label>
          <label class="paper-setting-field"><span>{{ t('paperTodo.settings.palette') }}</span><select :value="settings.palette" @change="setString('palette', $event)"><option v-for="palette in PALETTES" :key="palette.id" :value="palette.id">{{ t(`paperTodo.settings.${palette.id}`) }}</option></select></label>
          <label class="paper-setting-field"><span>{{ t('paperTodo.settings.font') }}</span><select :value="settings.fontFamily" @change="setString('fontFamily', $event)"><option value="system-ui">{{ t('paperTodo.settings.systemFont') }}</option><option value="Microsoft YaHei">Microsoft YaHei</option><option value="DengXian">DengXian</option><option value="Segoe UI">Segoe UI</option></select></label>
          <label class="paper-setting-field"><span>{{ t('paperTodo.settings.textRendering') }}</span><select :value="settings.textRendering" @change="setString('textRendering', $event)"><option value="standard">{{ t('paperTodo.settings.standard') }}</option><option value="soft">{{ t('paperTodo.settings.soft') }}</option><option value="sharp">{{ t('paperTodo.settings.sharp') }}</option></select></label>
          <label class="paper-setting-field"><span>{{ t('paperTodo.settings.interfaceScale') }} · {{ settings.interfaceScale }}%</span><input type="range" min="80" max="120" step="5" :value="settings.interfaceScale" @input="setNumber('interfaceScale', $event)"></label>
          <label class="paper-setting-field"><span>{{ t('paperTodo.settings.titleLength') }} · {{ settings.titleMaxLength }}</span><input type="range" min="2" max="20" step="1" :value="settings.titleMaxLength" @input="setNumber('titleMaxLength', $event)"></label>
          <label v-for="field in TEXT_SIZE_FIELDS" :key="field.key" class="paper-setting-field"><span>{{ t(field.label) }}</span><select :value="settings[field.key]" @change="setString(field.key, $event)"><option v-for="size in TEXT_SIZES" :key="size" :value="size">{{ t(`paperTodo.settings.${size}`) }}</option></select></label>
          <label class="paper-setting-field"><span>{{ t('paperTodo.settings.imageMarkers') }}</span><select :value="settings.imageMarkerVisibility" @change="setString('imageMarkerVisibility', $event)"><option value="always">{{ t('paperTodo.settings.always') }}</option><option value="editing">{{ t('paperTodo.settings.onlyEditing') }}</option><option value="hidden">{{ t('paperTodo.settings.hidden') }}</option></select></label>
        </div>
        <div class="settings-card">
          <label v-for="field in PAPER_TODO_TOGGLE_GROUPS.appearance" :key="field.key" class="settings-row">
            <span><strong>{{ t(field.label) }}</strong><small>{{ t(field.description) }}</small></span>
            <input class="paper-switch" type="checkbox" :checked="Boolean(settings[field.key])" @change="setBoolean(field.key, $event)">
          </label>
        </div>
      </template>

      <template v-else-if="activeTab === 'papers'">
        <div class="settings-card">
          <label v-for="field in PAPER_TODO_TOGGLE_GROUPS.papers" :key="field.key" class="settings-row">
            <span><strong>{{ t(field.label) }}</strong><small>{{ t(field.description) }}</small></span>
            <input class="paper-switch" type="checkbox" :checked="Boolean(settings[field.key])" @change="setBoolean(field.key, $event)">
          </label>
          <label class="settings-row"><span><strong>{{ t('paperTodo.settings.externalExtension') }}</strong></span><input class="settings-input" :value="settings.externalExtension" maxlength="11" placeholder=".md" @change="setString('externalExtension', $event)"></label>
        </div>
      </template>

      <template v-else-if="activeTab === 'shortcuts'">
        <div class="settings-card shortcut-grid">
          <label v-for="key in PAPER_TODO_HOTKEY_IDS" :key="key" class="paper-setting-field"><span>{{ t(`paperTodo.settings.hotkey.${key}`) }}</span><input :value="settings.hotkeys[key]" placeholder="Ctrl+Shift+Space" @keydown="recordHotkey(key, $event)" @change="setHotkey(key, $event)"></label>
        </div>
      </template>

      <template v-else>
        <div class="settings-card data-card">
          <div class="settings-card-title"><strong>{{ t('paperTodo.dataManagement') }}</strong><span>{{ t('paperTodo.dataManagementHint') }}</span></div>
          <div class="data-actions">
            <button type="button" :disabled="props.busy" @click="emit('importData')"><Import class="h-4 w-4" />{{ t('paperTodo.importData') }}</button>
            <button type="button" :disabled="props.busy" @click="emit('exportData')"><Download class="h-4 w-4" />{{ t('paperTodo.exportData') }}</button>
            <button type="button" :disabled="props.busy" @click="emit('cleanAssets')"><RotateCcw class="h-4 w-4" />{{ t('paperTodo.cleanAssets') }}</button>
          </div>
        </div>
      </template>
    </main>

    <aside class="paper-settings-preview">
      <div class="preview-heading"><strong>{{ t('paperTodo.settings.livePreview') }}</strong><span>{{ t('paperTodo.settings.livePreviewHint') }}</span></div>
      <div class="preview-stage">
        <div class="preview-paper" :class="[`is-${settings.paperSkin}`, `theme-${settings.theme}`]" :style="previewStyle">
          <div class="preview-paper-head"><span></span><strong>{{ t('paperTodo.previewTitle') }}</strong><small>1/3</small><i></i></div>
          <div class="preview-paper-row is-high"><b></b><i></i><span>{{ t('paperTodo.previewTodoOne') }}</span><em>{{ t('paperTodo.previewDueToday') }}</em></div>
          <div class="preview-paper-row is-medium"><b></b><i></i><span>{{ t('paperTodo.previewTodoTwo') }}</span></div>
          <div class="preview-completed-label">{{ t('paperTodo.completedGroup', { count: 1 }) }}</div>
          <div class="preview-paper-row is-done"><i></i><span>{{ t('paperTodo.previewTodoDone') }}</span></div>
          <div class="preview-paper-progress"><span></span></div>
        </div>
        <div class="preview-launcher" :class="settings.launcherEdge === 'left' ? 'is-left' : 'is-right'"><i></i><span>{{ t('paperTodo.launcher.collapsedCount', { count: props.todoCount + props.noteCount }) }}</span></div>
      </div>

      <div class="preview-control">
        <span>{{ t('paperTodo.settings.palette') }}</span>
        <div class="palette-options">
          <button v-for="palette in PALETTES" :key="palette.id" type="button" :class="settings.palette === palette.id && 'is-active'" :style="{ background: palette.color }" :aria-label="t(`paperTodo.settings.${palette.id}`)" :aria-pressed="settings.palette === palette.id" @click="setSetting('palette', palette.id)"></button>
        </div>
      </div>
      <div class="preview-control">
        <span>{{ t('paperTodo.settings.theme') }}</span>
        <div class="settings-segmented is-full"><button v-for="theme in ['system', 'light', 'dark'] as const" :key="theme" type="button" :class="settings.theme === theme && 'is-active'" @click="setSetting('theme', theme)">{{ t(`paperTodo.settings.${theme}`) }}</button></div>
      </div>
      <div class="preview-control">
        <span>{{ t('paperTodo.settings.todoSize') }}</span>
        <div class="settings-segmented is-full"><button v-for="size in TEXT_SIZES" :key="size" type="button" :class="settings.todoFontSize === size && 'is-active'" @click="setSetting('todoFontSize', size)">{{ t(`paperTodo.settings.${size}`) }}</button></div>
      </div>
      <div class="preview-data-actions">
        <div>
          <button type="button" :disabled="props.busy" @click="emit('importData')"><Import class="h-3.5 w-3.5" />{{ t('paperTodo.importDataShort') }}</button>
          <button type="button" :disabled="props.busy" @click="emit('exportData')"><Download class="h-3.5 w-3.5" />{{ t('paperTodo.exportDataShort') }}</button>
        </div>
        <button type="button" :disabled="props.busy" @click="emit('cleanAssets')"><RotateCcw class="h-3.5 w-3.5" />{{ t('paperTodo.cleanAssets') }}</button>
      </div>
    </aside>
  </section>
</template>

<style scoped>
.paper-settings-layout { display: grid; min-height: 650px; grid-template-columns: 224px minmax(360px, 1fr) 300px; overflow: hidden; border: 1px solid #e2e8f0; border-radius: 12px; background: #f8fafc; box-shadow: 0 14px 30px rgb(15 23 42 / 0.08); }
.paper-settings-nav { display: flex; min-width: 0; flex-direction: column; border-right: 1px solid #e2e8f0; background: #fff; padding: 18px 12px; }
.paper-settings-nav-title { padding: 0 10px 12px; color: #94a3b8; font-size: 11px; font-weight: 700; letter-spacing: .1em; text-transform: uppercase; }
.paper-settings-tablist { display: flex; flex-direction: column; gap: 4px; }
.paper-settings-tablist button { display: flex; min-height: 36px; cursor: pointer; align-items: center; gap: 9px; border-radius: 7px; padding: 0 10px; color: #475569; font-size: 13px; font-weight: 500; transition: background-color 180ms ease, color 180ms ease; }
.paper-settings-tablist button:hover { background: #f1f5f9; color: #334155; }
.paper-settings-tablist button.is-active { background: #0f172a; color: #fff; }
.paper-settings-tablist button:focus-visible { outline: 2px solid #0ea5e9; outline-offset: 2px; }
.paper-settings-stats { margin-top: auto; padding: 14px 10px 2px; }
.paper-settings-stats > span { display: block; color: #94a3b8; font-size: 10px; font-weight: 700; letter-spacing: .08em; text-transform: uppercase; }
.paper-settings-stats > strong { display: block; margin-top: 5px; color: #475569; font-size: 11px; font-weight: 500; line-height: 1.55; }
.paper-settings-content { min-width: 0; overflow-y: auto; padding: 22px 24px; }
.paper-settings-section-head { margin-bottom: 15px; }
.paper-settings-section-head h2 { color: #0f172a; font-size: 16px; font-weight: 700; }
.paper-settings-section-head p { margin-top: 3px; color: #64748b; font-size: 12px; line-height: 1.55; }
.settings-card { overflow: hidden; border-radius: 10px; background: #fff; box-shadow: 0 0 0 1px #e6ebf1; }
.settings-card + .settings-card { margin-top: 14px; }
.desktop-paper-card { display: flex; min-height: 58px; align-items: center; gap: 10px; margin-top: 14px; border-radius: 10px; background: #fff; padding: 10px 14px; color: #b8791a; box-shadow: 0 1px 2px rgb(15 23 42 / .06),0 0 0 1px #e6ebf1; }
.desktop-paper-card > span { min-width: 0; flex: 1; color: #1e293b; }
.desktop-paper-card strong { display: block; font-size: 13px; font-weight: 600; }
.desktop-paper-card small { display: block; overflow: hidden; margin-top: 1px; text-overflow: ellipsis; white-space: nowrap; color: #64748b; font-size: 11.5px; }
.desktop-paper-card button { display: inline-flex; min-height: 32px; cursor: pointer; align-items: center; gap: 5px; border: 1px solid #cbd5e1; border-radius: 7px; padding: 0 10px; color: #334155; font-size: 11.5px; font-weight: 600; }
.desktop-paper-card button:hover { background: #f8fafc; }
.settings-row { display: flex; min-height: 62px; cursor: pointer; align-items: center; gap: 18px; padding: 10px 14px; }
.settings-row + .settings-row { border-top: 1px solid #eef2f6; }
.settings-row > span { min-width: 0; flex: 1; }
.settings-row strong { display: block; color: #1e293b; font-size: 13.5px; font-weight: 600; }
.settings-row small { display: block; margin-top: 2px; color: #64748b; font-size: 12px; line-height: 1.45; }
.settings-card-title { padding: 14px; border-bottom: 1px solid #eef2f6; }
.settings-card-title strong { display: block; color: #1e293b; font-size: 13.5px; }
.settings-card-title span { display: block; margin-top: 3px; color: #64748b; font-size: 12px; }
.settings-segmented { display: inline-flex; flex: 0 0 auto; border: 1px solid #cbd5e1; border-radius: 7px; background: #f8fafc; padding: 2px; }
.settings-segmented button { min-height: 28px; cursor: pointer; border-radius: 5px; padding: 0 10px; color: #64748b; font-size: 11px; font-weight: 600; }
.settings-segmented button.is-active { background: #0f172a; color: #fff; box-shadow: 0 1px 2px rgb(15 23 42 / .18); }
.settings-segmented button:focus-visible { outline: 2px solid #0ea5e9; outline-offset: 1px; }
.settings-segmented button:disabled { cursor: default; opacity: .4; }
.settings-segmented.is-full { display: flex; width: 100%; }
.settings-segmented.is-full button { min-width: 0; flex: 1; padding-inline: 4px; }
.settings-range { width: min(190px, 40%); accent-color: #0284c7; }
.settings-select,.settings-input { min-height: 36px; border: 1px solid #cbd5e1; border-radius: 6px; background: #fff; padding: 0 10px; color: #0f172a; font-size: 13px; }
.skin-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 10px; padding: 12px; }
.skin-option { position: relative; display: grid; min-width: 0; cursor: pointer; grid-template-columns: 66px minmax(0, 1fr); align-items: center; gap: 9px; border: 1px solid #dbe3ec; border-radius: 9px; padding: 8px; text-align: left; transition: border-color 180ms ease, box-shadow 180ms ease; }
.skin-option:hover { border-color: #94a3b8; }
.skin-option.is-active { border: 2px solid #0284c7; padding: 7px; box-shadow: 0 0 0 2px rgb(2 132 199 / .1); }
.skin-option:focus-visible { outline: 2px solid #0ea5e9; outline-offset: 2px; }
.skin-option > span:nth-child(2) { min-width: 0; }
.skin-option strong { display: block; color: #1e293b; font-size: 12px; }
.skin-option small { display: -webkit-box; overflow: hidden; margin-top: 2px; -webkit-box-orient: vertical; -webkit-line-clamp: 2; color: #64748b; font-size: 10.5px; line-height: 1.35; }
.skin-check { position: absolute; top: 5px; right: 5px; border-radius: 999px; background: #0284c7; padding: 2px; color: #fff; box-sizing: content-box; }
.skin-option-preview { position: relative; display: block; height: 52px; overflow: hidden; border: 1px solid rgb(15 23 42 / .11); border-radius: 7px; background: #fffdf3; box-shadow: 0 4px 8px rgb(15 23 42 / .08); }
.skin-option-preview i { display: block; height: 11px; border-bottom: 1px solid rgb(15 23 42 / .09); }
.skin-option-preview b { display: block; width: 76%; height: 4px; margin: 5px 7px 0; border-radius: 3px; background: #cbd5e1; }
.skin-option-preview b:nth-of-type(2) { width: 60%; }
.skin-option-preview em { position: absolute; right: 6px; bottom: 5px; width: 14px; height: 3px; border-radius: 2px; background: #b8791a; }
.skin-option-preview.is-grain { border-radius: 8px; background: repeating-linear-gradient(#fdf8e7 0,#fdf8e7 10px,#eadfca 11px); }
.skin-option-preview.is-grain i { background: #f4e3bb; }
.skin-option-preview.is-quiet { border-radius: 10px; background: #fcfcf9; }
.skin-option-preview.is-quiet i { border: 0; background: transparent; }
.skin-option-preview.is-desk { border-radius: 5px; border-left: 4px solid #b8791a; background: #fffdf6; }
.settings-grid-card,.shortcut-grid { display: grid; grid-template-columns: repeat(2,minmax(0,1fr)); gap: 14px 18px; padding: 14px; }
.paper-setting-field { display: flex; min-width: 0; flex-direction: column; gap: 6px; color: #334155; font-size: 12px; font-weight: 600; }
.paper-setting-field select,.paper-setting-field input:not([type='range']) { min-height: 38px; border: 1px solid #cbd5e1; border-radius: 6px; background: #fff; padding: 0 10px; outline: none; color: #0f172a; font-size: 13px; font-weight: 400; }
.paper-setting-field input[type='range'] { accent-color: #0284c7; }
.paper-setting-field select:focus,.paper-setting-field input:focus { border-color: #0ea5e9; box-shadow: 0 0 0 3px rgb(14 165 233 / .12); }
.paper-switch { position: relative; width: 40px; height: 22px; flex: 0 0 40px; cursor: pointer; appearance: none; border-radius: 999px; background: #cbd5e1; transition: background-color 180ms ease; }
.paper-switch::after { position: absolute; top: 3px; left: 3px; width: 16px; height: 16px; border-radius: 999px; background: #fff; box-shadow: 0 1px 2px rgb(15 23 42 / .25); content: ''; transition: transform 180ms ease; }
.paper-switch:checked { background: #0284c7; }
.paper-switch:checked::after { transform: translateX(18px); }
.paper-switch:focus-visible { outline: 2px solid rgb(14 165 233 / .7); outline-offset: 2px; }
.paper-switch:disabled { cursor: default; opacity: .45; }
.data-card { padding-bottom: 14px; }
.data-actions { display: flex; flex-wrap: wrap; gap: 8px; padding: 14px; }
.data-actions button { display: inline-flex; min-height: 38px; cursor: pointer; align-items: center; gap: 7px; border: 1px solid #cbd5e1; border-radius: 6px; padding: 0 12px; color: #334155; font-size: 12px; font-weight: 600; }
.data-actions button:hover { background: #f8fafc; }
.data-actions button:disabled { cursor: default; opacity: .4; }
.paper-settings-preview { display: flex; min-width: 0; flex-direction: column; border-left: 1px solid #e2e8f0; background: #fff; padding: 22px 20px; }
.preview-heading strong { display: block; color: #1e293b; font-size: 13px; }
.preview-heading span { display: block; margin-top: 2px; color: #64748b; font-size: 11px; }
.preview-stage { position: relative; display: flex; min-height: 270px; align-items: center; justify-content: center; margin-top: 12px; overflow: hidden; border: 1px solid #e2e8f0; border-radius: 10px; background: linear-gradient(135deg,#f8fafc,#eef2f7); padding: 18px; }
.preview-paper { --preview-base:#fffdf3; position: relative; width: 194px; height: 226px; overflow: hidden; border-radius: 8px; background: color-mix(in srgb,var(--preview-tint) 4%,var(--preview-base)); box-shadow: 0 10px 24px rgb(15 23 42 / .16); color: #1e293b; }
.preview-paper.theme-dark { --preview-base:#1f242b; color:#e5e7eb; }
@media (prefers-color-scheme: dark) { .preview-paper.theme-system { --preview-base:#1f242b; color:#e5e7eb; } }
.preview-paper-head { display: flex; height: 32px; align-items: center; gap: 6px; border-bottom: 1px solid rgb(15 23 42 / .1); padding: 0 8px; }
.preview-paper-head > span { width: 9px; height: 9px; border-radius: 3px; background: var(--preview-tint); }
.preview-paper-head strong { min-width: 0; flex: 1; font-size: 10px; font-weight: var(--preview-title-weight); }
.preview-paper-head small { font-size: 8px; opacity: .55; }
.preview-paper-head i { width: 13px; height: 3px; border-radius: 3px; background: currentColor; opacity: .3; }
.preview-paper-row { display: flex; height: 31px; align-items: center; gap: 7px; margin: 0 9px; font-size: var(--preview-todo-size); font-weight: var(--preview-row-weight); }
.preview-paper-row > b { display: none; width: 3px; height: 15px; flex: 0 0 3px; border-radius: 2px; background: #d8a13c; }
.preview-paper-row.is-high > b { background: #c2410c; }
.preview-paper-row i { width: 11px; height: 11px; border: 1px solid currentColor; border-radius: 4px; opacity: .35; }
.preview-paper-row span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.preview-paper-row em { display: none; flex: 0 0 auto; border: 1px solid currentColor; border-radius: 4px; padding: 1px 4px; font-size: 7px; font-style: normal; font-weight: 600; opacity: .48; }
.preview-paper-row.is-done { text-decoration: line-through; opacity: .42; }
.preview-completed-label { display: none; color: currentColor; font-size: 7.5px; font-weight: 700; letter-spacing: .08em; opacity: .42; text-transform: uppercase; }
.preview-paper-progress { position: absolute; right: 0; bottom: 0; left: 0; height: 3px; background: color-mix(in srgb,var(--preview-tint) 18%,transparent); }
.preview-paper-progress span { display: block; width: 34%; height: 100%; background: var(--preview-tint); }
.preview-paper.is-grain { border-radius: 9px; background-image: repeating-linear-gradient(transparent 0,transparent 30px,color-mix(in srgb,var(--preview-tint) 14%,transparent) 30px,color-mix(in srgb,var(--preview-tint) 14%,transparent) 31px); }
.preview-paper.is-grain .preview-paper-head { background: color-mix(in srgb,var(--preview-tint) 13%,var(--preview-base)); }
.preview-paper.is-quiet { border-radius: 12px; }
.preview-paper.is-quiet .preview-paper-head { height: 45px; border: 0; padding-inline: 14px; }
.preview-paper.is-quiet .preview-paper-row { height: 37px; margin-inline: 14px; }
.preview-paper.is-quiet .preview-paper-row i { border-radius: 999px; }
.preview-paper.is-quiet .preview-completed-label { display: block; margin: 7px 14px 0; }
.preview-paper.is-desk { border-radius: 6px; border-left: 4px solid var(--preview-tint); }
.preview-paper.is-desk .preview-paper-head { height: 28px; }
.preview-paper.is-desk .preview-paper-row { height: 25px; border-bottom: 1px solid rgb(15 23 42 / .05); }
.preview-paper.is-desk .preview-paper-row > b,
.preview-paper.is-desk .preview-paper-row em { display: block; }
.preview-paper.is-desk .preview-completed-label { display: block; height: 20px; padding: 5px 9px 0; }
.preview-launcher { position: absolute; top: 50%; display: flex; height: 30px; transform: translateY(-50%); align-items: center; gap: 5px; border: 1px solid #d6c28e; border-radius: 999px; background: #fff9e8; padding: 0 10px 0 8px; color: #8a5a10; font-size: 9px; box-shadow: 0 2px 6px rgb(15 23 42 / .12); }
.preview-launcher.is-right { right: -5px; }
.preview-launcher.is-left { left: -5px; flex-direction: row-reverse; }
.preview-launcher i { width: 3px; height: 18px; border-radius: 3px; background: var(--preview-tint,#b8791a); }
.preview-control { margin-top: 15px; }
.preview-control > span { display: block; margin-bottom: 7px; color: #475569; font-size: 11px; font-weight: 700; }
.palette-options { display: flex; gap: 9px; }
.palette-options button { width: 28px; height: 28px; cursor: pointer; border: 3px solid #fff; border-radius: 999px; box-shadow: 0 0 0 1px #cbd5e1; }
.palette-options button.is-active { box-shadow: 0 0 0 2px #0284c7; }
.palette-options button:focus-visible { outline: 2px solid #0ea5e9; outline-offset: 3px; }
.preview-data-actions { display: flex; flex-direction: column; gap: 8px; margin-top: auto; padding-top: 18px; }
.preview-data-actions > div { display: flex; gap: 8px; }
.preview-data-actions button { display: inline-flex; min-height: 32px; flex: 1; cursor: pointer; align-items: center; justify-content: center; gap: 5px; border: 1px solid #cbd5e1; border-radius: 7px; color: #475569; font-size: 11.5px; font-weight: 600; }
.preview-data-actions > button { border-color: transparent; }
.preview-data-actions button:hover { background: #f8fafc; color: #0f172a; }
@media (max-width: 1180px) { .paper-settings-layout { grid-template-columns: 190px minmax(340px,1fr) 260px; } .skin-grid { grid-template-columns: 1fr; } .paper-settings-content { padding-inline: 18px; } }
@media (max-width: 900px) { .paper-settings-layout { grid-template-columns: 180px minmax(0,1fr); } .paper-settings-preview { grid-column: 1 / -1; border-top: 1px solid #e2e8f0; border-left: 0; } }
@media (max-width: 720px) { .paper-settings-layout { grid-template-columns: minmax(0,1fr); overflow: visible; } .paper-settings-nav { border-right: 0; border-bottom: 1px solid #e2e8f0; } .paper-settings-tablist { display: grid; grid-template-columns: repeat(2,minmax(0,1fr)); } .paper-settings-stats { margin-top: 10px; } .paper-settings-content { padding: 18px 14px; } .paper-settings-preview { grid-column: 1; } .settings-grid-card,.shortcut-grid { grid-template-columns: 1fr; } .desktop-paper-card { flex-wrap: wrap; } }
@media (prefers-reduced-motion: reduce) { .paper-settings-tablist button,.skin-option,.paper-switch,.paper-switch::after { transition: none; } }
</style>
