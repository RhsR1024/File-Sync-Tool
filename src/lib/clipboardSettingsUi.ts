import {
  Database,
  Eye,
  Filter,
  Images,
  Keyboard,
  LayoutPanelTop,
  Settings2,
} from 'lucide-vue-next';

export const CLIPBOARD_SETTINGS_TABS = [
  { id: 'general', labelKey: 'clipboard.settings.tabs.general', icon: Settings2 },
  { id: 'display', labelKey: 'clipboard.settings.tabs.display', icon: LayoutPanelTop },
  { id: 'shortcuts', labelKey: 'clipboard.settings.tabs.shortcuts', icon: Keyboard },
  { id: 'data', labelKey: 'clipboard.settings.tabs.data', icon: Database },
  { id: 'preview', labelKey: 'clipboard.settings.tabs.preview', icon: Eye },
  { id: 'appFilter', labelKey: 'clipboard.settings.tabs.appFilter', icon: Filter },
  { id: 'imageCopy', labelKey: 'clipboard.imageCopy.title', icon: Images },
] as const;

export type ClipboardSettingsTabId =
  typeof CLIPBOARD_SETTINGS_TABS[number]['id'];

export const CLIPBOARD_TOOLBAR_ACTION_IDS = [
  'batch',
  'settings',
  'lock',
] as const;
export type ClipboardToolbarActionId =
  typeof CLIPBOARD_TOOLBAR_ACTION_IDS[number];
