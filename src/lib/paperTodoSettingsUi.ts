import {
  Anchor,
  DatabaseBackup,
  Keyboard,
  Palette,
  StickyNote,
} from 'lucide-vue-next';

import type { PaperTodoSettings } from '@/lib/paperTodo';

export const PAPER_TODO_SETTINGS_TABS = [
  { id: 'launcher', labelKey: 'paperTodo.settings.tabs.launcher', icon: Anchor },
  { id: 'appearance', labelKey: 'paperTodo.settings.tabs.appearance', icon: Palette },
  { id: 'papers', labelKey: 'paperTodo.settings.tabs.papers', icon: StickyNote },
  { id: 'shortcuts', labelKey: 'paperTodo.settings.tabs.shortcuts', icon: Keyboard },
  { id: 'data', labelKey: 'paperTodo.settings.tabs.data', icon: DatabaseBackup },
] as const;

export type PaperTodoSettingsTabId = typeof PAPER_TODO_SETTINGS_TABS[number]['id'];

export interface PaperTodoToggleField {
  key: keyof PaperTodoSettings;
  label: string;
  description: string;
}

/**
 * Toggle rows grouped by the tab that owns them. Splitting one long list into
 * these groups is the whole point of the tabbed layout: each screen answers a
 * single question instead of presenting every switch at once.
 */
export const PAPER_TODO_TOGGLE_GROUPS: Record<
  Exclude<PaperTodoSettingsTabId, 'launcher' | 'shortcuts' | 'data'>,
  PaperTodoToggleField[]
> = {
  appearance: [
    { key: 'animations', label: 'paperTodo.settings.animations', description: 'paperTodo.settings.animationsHint' },
    { key: 'hoverTips', label: 'paperTodo.settings.hoverTips', description: 'paperTodo.settings.hoverTipsHint' },
    { key: 'titleBold', label: 'paperTodo.settings.titleBold', description: 'paperTodo.settings.titleBoldHint' },
    { key: 'todoBold', label: 'paperTodo.settings.todoBold', description: 'paperTodo.settings.todoBoldHint' },
    { key: 'noteBold', label: 'paperTodo.settings.noteBold', description: 'paperTodo.settings.noteBoldHint' },
  ],
  papers: [
    { key: 'autoClearCompleted', label: 'paperTodo.settings.autoClear', description: 'paperTodo.settings.autoClearHint' },
    { key: 'showLinkedNoteTitle', label: 'paperTodo.settings.linkedTitle', description: 'paperTodo.settings.linkedTitleHint' },
    { key: 'showNewTodoButton', label: 'paperTodo.settings.showNewTodo', description: 'paperTodo.settings.showNewTodoHint' },
    { key: 'showNewNoteButton', label: 'paperTodo.settings.showNewNote', description: 'paperTodo.settings.showNewNoteHint' },
    { key: 'showExternalOpenButton', label: 'paperTodo.settings.showExternal', description: 'paperTodo.settings.showExternalHint' },
    { key: 'autoCompressImages', label: 'paperTodo.settings.compressImages', description: 'paperTodo.settings.compressImagesHint' },
    { key: 'preferPowerShell7', label: 'paperTodo.settings.preferPwsh', description: 'paperTodo.settings.preferPwshHint' },
    { key: 'hideScriptWindow', label: 'paperTodo.settings.hideScript', description: 'paperTodo.settings.hideScriptHint' },
    { key: 'hideFromTaskbar', label: 'paperTodo.settings.hideTaskbar', description: 'paperTodo.settings.hideTaskbarHint' },
    { key: 'avoidFullscreen', label: 'paperTodo.settings.avoidFullscreen', description: 'paperTodo.settings.avoidFullscreenHint' },
  ],
};

export const PAPER_TODO_HOTKEY_IDS = [
  'showAll',
  'hideAll',
  'toggleAll',
  'newTodo',
  'newNote',
] as const;
