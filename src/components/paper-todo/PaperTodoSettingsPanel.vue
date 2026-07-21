<script setup lang="ts">
import { computed } from 'vue';
import { useI18n } from 'vue-i18n';

import { usePaperTodo } from '@/composables/usePaperTodo';
import type { PaperTodoSettings } from '@/lib/paperTodo';

const { t } = useI18n();
const store = usePaperTodo();
const settings = computed(() => store.state.value.settings);

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

const toggleFields: Array<{ key: keyof PaperTodoSettings; label: string; description: string }> = [
  { key: 'animations', label: 'paperTodo.settings.animations', description: 'paperTodo.settings.animationsHint' },
  { key: 'hoverTips', label: 'paperTodo.settings.hoverTips', description: 'paperTodo.settings.hoverTipsHint' },
  { key: 'autoClearCompleted', label: 'paperTodo.settings.autoClear', description: 'paperTodo.settings.autoClearHint' },
  { key: 'showLinkedNoteTitle', label: 'paperTodo.settings.linkedTitle', description: 'paperTodo.settings.linkedTitleHint' },
  { key: 'hideLinkedNoteCapsules', label: 'paperTodo.settings.hideLinkedCapsule', description: 'paperTodo.settings.hideLinkedCapsuleHint' },
  { key: 'capsuleMode', label: 'paperTodo.settings.capsuleMode', description: 'paperTodo.settings.capsuleModeHint' },
  { key: 'autoDockCapsules', label: 'paperTodo.settings.autoDock', description: 'paperTodo.settings.autoDockHint' },
  { key: 'rememberExpandedPosition', label: 'paperTodo.settings.rememberPosition', description: 'paperTodo.settings.rememberPositionHint' },
  { key: 'hideFromTaskbar', label: 'paperTodo.settings.hideTaskbar', description: 'paperTodo.settings.hideTaskbarHint' },
  { key: 'avoidFullscreen', label: 'paperTodo.settings.avoidFullscreen', description: 'paperTodo.settings.avoidFullscreenHint' },
  { key: 'autoCompressImages', label: 'paperTodo.settings.compressImages', description: 'paperTodo.settings.compressImagesHint' },
  { key: 'preferPowerShell7', label: 'paperTodo.settings.preferPwsh', description: 'paperTodo.settings.preferPwshHint' },
  { key: 'hideScriptWindow', label: 'paperTodo.settings.hideScript', description: 'paperTodo.settings.hideScriptHint' },
  { key: 'showNewTodoButton', label: 'paperTodo.settings.showNewTodo', description: 'paperTodo.settings.showNewTodoHint' },
  { key: 'showNewNoteButton', label: 'paperTodo.settings.showNewNote', description: 'paperTodo.settings.showNewNoteHint' },
  { key: 'showExternalOpenButton', label: 'paperTodo.settings.showExternal', description: 'paperTodo.settings.showExternalHint' },
  { key: 'todoBold', label: 'paperTodo.settings.todoBold', description: 'paperTodo.settings.todoBoldHint' },
  { key: 'noteBold', label: 'paperTodo.settings.noteBold', description: 'paperTodo.settings.noteBoldHint' },
  { key: 'titleBold', label: 'paperTodo.settings.titleBold', description: 'paperTodo.settings.titleBoldHint' },
  { key: 'capsuleBold', label: 'paperTodo.settings.capsuleBold', description: 'paperTodo.settings.capsuleBoldHint' },
];
</script>

<template>
  <div class="mx-auto w-full max-w-5xl pb-10">
    <section class="border-b border-slate-200 py-6">
      <h2 class="text-base font-semibold text-slate-900">{{ t('paperTodo.settings.edgeLauncher') }}</h2>
      <div class="mt-4 grid grid-cols-1 gap-x-8 gap-y-4 md:grid-cols-2">
        <label class="flex min-h-14 cursor-pointer items-center gap-4 rounded-md border border-slate-200 bg-white px-4 py-3">
          <span class="min-w-0 flex-1">
            <span class="block text-sm font-medium text-slate-800">{{ t('paperTodo.settings.launcherEnabled') }}</span>
            <span class="mt-0.5 block text-xs leading-5 text-slate-500">{{ t('paperTodo.settings.launcherEnabledHint') }}</span>
          </span>
          <input class="paper-toggle" type="checkbox" :checked="settings.launcherEnabled" @change="setBoolean('launcherEnabled', $event)">
        </label>
        <label class="paper-setting-field">
          <span>{{ t('paperTodo.settings.launcherEdge') }}</span>
          <select :value="settings.launcherEdge" :disabled="!settings.launcherEnabled" @change="setString('launcherEdge', $event)">
            <option value="left">{{ t('paperTodo.settings.leftEdge') }}</option>
            <option value="right">{{ t('paperTodo.settings.rightEdge') }}</option>
          </select>
        </label>
        <label class="paper-setting-field md:col-span-2">
          <span>{{ t('paperTodo.settings.launcherOffset') }} · {{ settings.launcherOffset }}%</span>
          <input class="max-w-xl" type="range" min="10" max="80" step="5" :value="settings.launcherOffset" :disabled="!settings.launcherEnabled" @input="setNumber('launcherOffset', $event)">
        </label>
      </div>
    </section>

    <section class="border-b border-slate-200 py-6">
      <h2 class="text-base font-semibold text-slate-900">{{ t('paperTodo.settings.appearance') }}</h2>
      <div class="mt-4 grid grid-cols-1 gap-x-8 gap-y-4 md:grid-cols-2">
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
          <span>{{ t('paperTodo.settings.interfaceScale') }} · {{ settings.interfaceScale }}%</span>
          <input type="range" min="80" max="120" step="5" :value="settings.interfaceScale" @input="setNumber('interfaceScale', $event)">
        </label>
        <label class="paper-setting-field">
          <span>{{ t('paperTodo.settings.titleLength') }} · {{ settings.titleMaxLength }}</span>
          <input type="range" min="2" max="20" step="1" :value="settings.titleMaxLength" @input="setNumber('titleMaxLength', $event)">
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
          <span>{{ t('paperTodo.settings.todoSize') }}</span>
          <select :value="settings.todoFontSize" @change="setString('todoFontSize', $event)">
            <option value="small">{{ t('paperTodo.settings.small') }}</option><option value="medium">{{ t('paperTodo.settings.medium') }}</option><option value="large">{{ t('paperTodo.settings.large') }}</option><option value="xlarge">{{ t('paperTodo.settings.xlarge') }}</option>
          </select>
        </label>
        <label class="paper-setting-field">
          <span>{{ t('paperTodo.settings.noteSize') }}</span>
          <select :value="settings.noteFontSize" @change="setString('noteFontSize', $event)">
            <option value="small">{{ t('paperTodo.settings.small') }}</option><option value="medium">{{ t('paperTodo.settings.medium') }}</option><option value="large">{{ t('paperTodo.settings.large') }}</option><option value="xlarge">{{ t('paperTodo.settings.xlarge') }}</option>
          </select>
        </label>
        <label class="paper-setting-field">
          <span>{{ t('paperTodo.settings.titleSize') }}</span>
          <select :value="settings.titleFontSize" @change="setString('titleFontSize', $event)">
            <option value="small">{{ t('paperTodo.settings.small') }}</option><option value="medium">{{ t('paperTodo.settings.medium') }}</option><option value="large">{{ t('paperTodo.settings.large') }}</option><option value="xlarge">{{ t('paperTodo.settings.xlarge') }}</option>
          </select>
        </label>
        <label class="paper-setting-field">
          <span>{{ t('paperTodo.settings.capsuleSize') }}</span>
          <select :value="settings.capsuleFontSize" @change="setString('capsuleFontSize', $event)">
            <option value="small">{{ t('paperTodo.settings.small') }}</option><option value="medium">{{ t('paperTodo.settings.medium') }}</option><option value="large">{{ t('paperTodo.settings.large') }}</option><option value="xlarge">{{ t('paperTodo.settings.xlarge') }}</option>
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
    </section>

    <section class="border-b border-slate-200 py-6">
      <h2 class="text-base font-semibold text-slate-900">{{ t('paperTodo.settings.behavior') }}</h2>
      <div class="mt-3 divide-y divide-slate-100">
        <label v-for="field in toggleFields" :key="field.key" class="flex min-h-14 cursor-pointer items-center gap-4 py-3">
          <span class="min-w-0 flex-1">
            <span class="block text-sm font-medium text-slate-800">{{ t(field.label) }}</span>
            <span class="mt-0.5 block text-xs leading-5 text-slate-500">{{ t(field.description) }}</span>
          </span>
          <input class="paper-toggle" type="checkbox" :checked="Boolean(settings[field.key])" @change="setBoolean(field.key, $event)">
        </label>
      </div>
      <label class="paper-setting-field mt-4 max-w-sm">
        <span>{{ t('paperTodo.settings.externalExtension') }}</span>
        <input :value="settings.externalExtension" maxlength="11" placeholder=".md" @change="setString('externalExtension', $event)">
      </label>
    </section>

    <section class="py-6">
      <h2 class="text-base font-semibold text-slate-900">{{ t('paperTodo.settings.hotkeys') }}</h2>
      <p class="mt-1 text-xs leading-5 text-slate-500">{{ t('paperTodo.settings.hotkeysHint') }}</p>
      <div class="mt-4 grid grid-cols-1 gap-4 md:grid-cols-2">
        <label v-for="key in (['showAll', 'hideAll', 'toggleAll', 'newTodo', 'newNote'] as const)" :key="key" class="paper-setting-field">
          <span>{{ t(`paperTodo.settings.hotkey.${key}`) }}</span>
          <input :value="settings.hotkeys[key]" placeholder="Ctrl+Shift+Space" @keydown="recordHotkey(key, $event)" @change="setHotkey(key, $event)">
        </label>
      </div>
    </section>
  </div>
</template>

<style scoped>
.paper-setting-field { display: flex; min-width: 0; flex-direction: column; gap: 0.4rem; font-size: 0.8rem; font-weight: 600; color: rgb(51 65 85); }
.paper-setting-field select,
.paper-setting-field input:not([type='range']) { min-height: 2.5rem; border: 1px solid rgb(203 213 225); border-radius: 6px; background: white; padding: 0 0.75rem; outline: none; font-size: 0.875rem; font-weight: 400; color: rgb(15 23 42); }
.paper-setting-field select:focus,
.paper-setting-field input:focus { border-color: rgb(14 165 233); box-shadow: 0 0 0 3px rgb(14 165 233 / 0.12); }
.paper-setting-field input[type='range'] { accent-color: rgb(2 132 199); }
.paper-toggle { width: 2.5rem; height: 1.35rem; cursor: pointer; accent-color: rgb(2 132 199); }
</style>
