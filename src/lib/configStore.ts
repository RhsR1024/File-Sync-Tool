import { createConfigStore } from './configStoreCore.ts';
import { restartSchedulerInterval } from './scheduler.ts';
import { appStore } from './store.ts';
import { getConfig, updateAppConfig, updateSyncConfig } from './tauri.ts';

export { createConfigStore } from './configStoreCore.ts';
export type { ConfigStoreDependencies } from './configStoreCore.ts';

export const configStore = createConfigStore({
  getConfig,
  updateSyncConfig,
  updateAppConfig,
  restartSchedulerInterval,
  setMaxLogLines: (value) => {
    appStore.maxLogLines = value;
  },
});
