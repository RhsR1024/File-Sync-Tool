<script setup lang="ts">
import { computed, ref } from 'vue';
import { useI18n } from 'vue-i18n';

import { usePaperTodo } from '@/composables/usePaperTodo';
import type { PaperTodoSettings } from '@/lib/paperTodo';
import {
  PAPER_TODO_HOTKEY_IDS,
  PAPER_TODO_SETTINGS_TABS,
  PAPER_TODO_TOGGLE_GROUPS,
  type PaperTodoSettingsTabId,
} from '@/lib/paperTodoSettingsUi';

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
    ArrowRight: index === last ? 0 : index + 1,
    ArrowLeft: index === 0 ? last : index - 1,
    Home: 0,
    End: last,
  };
  const next = targets[event.key];
  if (next === undefined) return;
  event.preventDefault();
  activeTab.value = PAPER_TODO_SETTINGS_TABS[next].id;
  focusTab(next);
}
</script>

<template>
  <section class="py-6">
    <nav
      ref="tablist"
      class="flex max-w-full items-center gap-1 overflow-x-auto rounded-full border border-slate-200/70 bg-slate-100/80 p-1"
      role="tablist"
      :aria-label="t('paperTodo.settingsTitle')"
    >
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
        class="group flex shrink-0 items-center gap-2 rounded-full px-4 py-2 text-sm font-semibold transition-colors duration-200 focus:outline-none focus-visible:ring-2 focus-visible:ring-slate-900/30"
        :class="activeTab === tab.id
          ? 'bg-white text-slate-900 shadow-[0_1px_2px_rgba(15,23,42,0.08)] ring-1 ring-slate-200/60'
          : 'text-slate-500 hover:text-slate-700'"
        @click="activeTab = tab.id"
        @keydown="onTabKeydown($event, index)"
      >
        <component
          :is="tab.icon"
          class="h-4 w-4"
          :class="activeTab === tab.id ? 'text-slate-900' : 'text-slate-400 group-hover:text-slate-500'"
          :stroke-width="activeTab === tab.id ? 2.25 : 2"
        />
        <span class="whitespace-nowrap">{{ t(tab.labelKey) }}</span>
      </button>
    </nav>

    <div
      :id="`paper-todo-settings-panel-${activeTab}`"
      class="mt-5"
      role="tabpanel"
      :aria-labelledby="`paper-todo-settings-tab-${activeTab}`"
    >
      <p class="text-xs leading-5 text-slate-500">{{ t(`paperTodo.settings.tabHints.${activeTab}`) }}</p>

      <div v-if="activeTab === 'launcher'" class="mt-4 grid grid-cols-1 gap-x-8 gap-y-4 md:grid-cols-2">
        <label class="paper-toggle-row md:col-span-2">
          <span class="min-w-0 flex-1">
            <span class="paper-toggle-label">{{ t('paperTodo.settings.launcherEnabled') }}</span>
            <span class="paper-toggle-hint">{{ t('paperTodo.settings.launcherEnabledHint') }}</span>
          </span>
          <input class="paper-switch" type="checkbox" :checked="settings.launcherEnabled" @change="setBoolean('launcherEnabled', $event)">
        </label>
        <label class="paper-setting-field">
          <span>{{ t('paperTodo.settings.launcherEdge') }}</span>
          <select :value="settings.launcherEdge" :disabled="!settings.launcherEnabled" @change="setString('launcherEdge', $event)">
            <option value="left">{{ t('paperTodo.settings.leftEdge') }}</option>
            <option value="right">{{ t('paperTodo.settings.rightEdge') }}</option>
          </select>
        </label>
        <label class="paper-setting-field">
          <span>{{ t('paperTodo.settings.launcherOffset') }} · {{ settings.launcherOffset }}%</span>
          <input type="range" min="0" max="100" step="5" :value="settings.launcherOffset" :disabled="!settings.launcherEnabled" @input="setNumber('launcherOffset', $event)">
        </label>
      </div>

      <div v-else-if="activeTab === 'appearance'">
        <div class="grid grid-cols-1 gap-x-8 gap-y-4 md:grid-cols-2">
          <label class="paper-setting-field">
            <span>{{ t('paperTodo.settings.theme') }}</span>
            <select :value="settings.theme" @change="setString('theme', $event)">
              <option value="system">{{ t('paperTodo.settings.system') }}</option>
              <option value="light">{{ t('paperTodo.settings.light') }}</option>
              <option value="dark">{{ t('paperTodo.settings.dark') }}</option>
            </select>
          </label>
          <label class="paper-setting-field">
            <span>{{ t('paperTodo.settings.palette') }}</span>
            <select :value="settings.palette" @change="setString('palette', $event)">
              <option value="warm">{{ t('paperTodo.settings.warm') }}</option>
              <option value="ink">{{ t('paperTodo.settings.ink') }}</option>
              <option value="forest">{{ t('paperTodo.settings.forest') }}</option>
              <option value="frost">{{ t('paperTodo.settings.frost') }}</option>
            </select>
          </label>
          <label class="paper-setting-field">
            <span>{{ t('paperTodo.settings.font') }}</span>
            <select :value="settings.fontFamily" @change="setString('fontFamily', $event)">
              <option value="system-ui">{{ t('paperTodo.settings.systemFont') }}</option>
              <option value="Microsoft YaHei">Microsoft YaHei</option>
              <option value="DengXian">DengXian</option>
              <option value="Segoe UI">Segoe UI</option>
            </select>
          </label>
          <label class="paper-setting-field">
            <span>{{ t('paperTodo.settings.textRendering') }}</span>
            <select :value="settings.textRendering" @change="setString('textRendering', $event)">
              <option value="standard">{{ t('paperTodo.settings.standard') }}</option>
              <option value="soft">{{ t('paperTodo.settings.soft') }}</option>
              <option value="sharp">{{ t('paperTodo.settings.sharp') }}</option>
            </select>
          </label>
          <label class="paper-setting-field">
            <span>{{ t('paperTodo.settings.interfaceScale') }} · {{ settings.interfaceScale }}%</span>
            <input type="range" min="80" max="120" step="5" :value="settings.interfaceScale" @input="setNumber('interfaceScale', $event)">
          </label>
          <label class="paper-setting-field">
            <span>{{ t('paperTodo.settings.titleLength') }} · {{ settings.titleMaxLength }}</span>
            <input type="range" min="2" max="20" step="1" :value="settings.titleMaxLength" @input="setNumber('titleMaxLength', $event)">
          </label>
        </div>

        <h3 class="paper-settings-subhead">{{ t('paperTodo.settings.textSizes') }}</h3>
        <div class="grid grid-cols-1 gap-x-8 gap-y-4 sm:grid-cols-2 lg:grid-cols-3">
          <label v-for="field in TEXT_SIZE_FIELDS" :key="field.key" class="paper-setting-field">
            <span>{{ t(field.label) }}</span>
            <select :value="settings[field.key]" @change="setString(field.key, $event)">
              <option v-for="size in TEXT_SIZES" :key="size" :value="size">{{ t(`paperTodo.settings.${size}`) }}</option>
            </select>
          </label>
          <label class="paper-setting-field">
            <span>{{ t('paperTodo.settings.imageMarkers') }}</span>
            <select :value="settings.imageMarkerVisibility" @change="setString('imageMarkerVisibility', $event)">
              <option value="always">{{ t('paperTodo.settings.always') }}</option>
              <option value="editing">{{ t('paperTodo.settings.onlyEditing') }}</option>
              <option value="hidden">{{ t('paperTodo.settings.hidden') }}</option>
            </select>
          </label>
        </div>

        <div class="mt-5 divide-y divide-slate-100 border-t border-slate-100">
          <label v-for="field in PAPER_TODO_TOGGLE_GROUPS.appearance" :key="field.key" class="paper-toggle-row">
            <span class="min-w-0 flex-1">
              <span class="paper-toggle-label">{{ t(field.label) }}</span>
              <span class="paper-toggle-hint">{{ t(field.description) }}</span>
            </span>
            <input class="paper-switch" type="checkbox" :checked="Boolean(settings[field.key])" @change="setBoolean(field.key, $event)">
          </label>
        </div>
      </div>

      <div v-else-if="activeTab === 'capsule'">
        <label class="paper-setting-field mt-4 max-w-xs">
          <span>{{ t('paperTodo.settings.capsuleSize') }}</span>
          <select :value="settings.capsuleFontSize" @change="setString('capsuleFontSize', $event)">
            <option v-for="size in TEXT_SIZES" :key="size" :value="size">{{ t(`paperTodo.settings.${size}`) }}</option>
          </select>
        </label>
        <div class="mt-5 divide-y divide-slate-100 border-t border-slate-100">
          <label v-for="field in PAPER_TODO_TOGGLE_GROUPS.capsule" :key="field.key" class="paper-toggle-row">
            <span class="min-w-0 flex-1">
              <span class="paper-toggle-label">{{ t(field.label) }}</span>
              <span class="paper-toggle-hint">{{ t(field.description) }}</span>
            </span>
            <input
              class="paper-switch"
              type="checkbox"
              :checked="Boolean(settings[field.key])"
              :disabled="field.key === 'autoHideDockedCapsules' && !settings.autoDockCapsules"
              @change="setBoolean(field.key, $event)"
            >
          </label>
        </div>
      </div>

      <div v-else-if="activeTab === 'papers'">
        <div class="mt-4 divide-y divide-slate-100 border-t border-slate-100">
          <label v-for="field in PAPER_TODO_TOGGLE_GROUPS.papers" :key="field.key" class="paper-toggle-row">
            <span class="min-w-0 flex-1">
              <span class="paper-toggle-label">{{ t(field.label) }}</span>
              <span class="paper-toggle-hint">{{ t(field.description) }}</span>
            </span>
            <input class="paper-switch" type="checkbox" :checked="Boolean(settings[field.key])" @change="setBoolean(field.key, $event)">
          </label>
        </div>
        <label class="paper-setting-field mt-5 max-w-xs">
          <span>{{ t('paperTodo.settings.externalExtension') }}</span>
          <input :value="settings.externalExtension" maxlength="11" placeholder=".md" @change="setString('externalExtension', $event)">
        </label>
      </div>

      <div v-else class="mt-4 grid grid-cols-1 gap-4 md:grid-cols-2">
        <label v-for="key in PAPER_TODO_HOTKEY_IDS" :key="key" class="paper-setting-field">
          <span>{{ t(`paperTodo.settings.hotkey.${key}`) }}</span>
          <input
            :value="settings.hotkeys[key]"
            placeholder="Ctrl+Shift+Space"
            @keydown="recordHotkey(key, $event)"
            @change="setHotkey(key, $event)"
          >
        </label>
      </div>
    </div>
  </section>
</template>

<style scoped>
.paper-settings-subhead {
  margin: 1.5rem 0 0.75rem;
  color: rgb(100 116 139);
  font-size: 0.7rem;
  font-weight: 700;
  letter-spacing: 0.08em;
  text-transform: uppercase;
}
.paper-setting-field {
  display: flex;
  min-width: 0;
  flex-direction: column;
  gap: 0.4rem;
  font-size: 0.8rem;
  font-weight: 600;
  color: rgb(51 65 85);
}
.paper-setting-field select,
.paper-setting-field input:not([type='range']) {
  min-height: 2.5rem;
  border: 1px solid rgb(203 213 225);
  border-radius: 6px;
  background: white;
  padding: 0 0.75rem;
  outline: none;
  font-size: 0.875rem;
  font-weight: 400;
  color: rgb(15 23 42);
}
.paper-setting-field select:focus,
.paper-setting-field input:focus {
  border-color: rgb(14 165 233);
  box-shadow: 0 0 0 3px rgb(14 165 233 / 0.12);
}
.paper-setting-field select:disabled,
.paper-setting-field input:disabled { background: rgb(248 250 252); color: rgb(148 163 184); }
.paper-setting-field input[type='range'] { accent-color: rgb(2 132 199); }
.paper-toggle-row {
  display: flex;
  min-height: 3.5rem;
  cursor: pointer;
  align-items: center;
  gap: 1rem;
  padding: 0.75rem 0;
}
.paper-toggle-label {
  display: block;
  color: rgb(30 41 59);
  font-size: 0.875rem;
  font-weight: 500;
}
.paper-toggle-hint {
  display: block;
  margin-top: 0.125rem;
  color: rgb(100 116 139);
  font-size: 0.75rem;
  line-height: 1.25rem;
}
/* A real switch instead of an accent-tinted checkbox: the settings list is long
   enough that the on/off state has to be readable at a glance while scanning. */
.paper-switch {
  position: relative;
  width: 2.5rem;
  height: 1.4rem;
  flex: 0 0 2.5rem;
  cursor: pointer;
  appearance: none;
  border-radius: 999px;
  background: rgb(203 213 225);
  transition: background-color 180ms ease;
}
.paper-switch::after {
  position: absolute;
  top: 0.15rem;
  left: 0.15rem;
  width: 1.1rem;
  height: 1.1rem;
  border-radius: 999px;
  background: white;
  box-shadow: 0 1px 2px rgb(15 23 42 / 0.25);
  content: '';
  transition: transform 180ms cubic-bezier(0.22, 1, 0.36, 1);
}
.paper-switch:checked { background: rgb(2 132 199); }
.paper-switch:checked::after { transform: translateX(1.1rem); }
.paper-switch:focus-visible { outline: 2px solid rgb(14 165 233 / 0.7); outline-offset: 2px; }
.paper-switch:disabled { cursor: default; opacity: 0.45; }
@media (prefers-reduced-motion: reduce) {
  .paper-switch,
  .paper-switch::after { transition: none; }
}
</style>
