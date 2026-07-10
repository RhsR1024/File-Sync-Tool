import { i18n } from '../i18n.ts';
import { createConfigStore } from './configStoreCore.ts';
import { restartSchedulerInterval } from './scheduler.ts';
import { appStore } from './store.ts';
import {
  addSystemEvent,
  getConfig,
  updateAppConfig,
  updateSyncConfig,
} from './tauri.ts';

export { createConfigStore } from './configStoreCore.ts';
export type { ConfigStoreDependencies } from './configStoreCore.ts';

export const configStore = createConfigStore({
  getConfig,
  updateSyncConfig,
  updateAppConfig,
  restartSchedulerInterval,
  addConfigEvent: () => {
    addSystemEvent('CONFIG_CHANGE', i18n.global.t('settings.saved'));
  },
  setMaxLogLines: (value) => {
    appStore.maxLogLines = value;
  },
});
