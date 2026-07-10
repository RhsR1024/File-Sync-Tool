import { reactive } from 'vue';

import { buildAppPatch, buildSyncPatch } from './configDomains.ts';
import type { AppConfig, AppDomainConfigPatch, SyncConfigPatch } from './tauri.ts';

export interface ConfigStoreDependencies {
  getConfig(): Promise<AppConfig>;
  updateSyncConfig(patch: SyncConfigPatch): Promise<void>;
  updateAppConfig(patch: AppDomainConfigPatch): Promise<void>;
  restartSchedulerInterval(): Promise<void>;
  addConfigEvent(): void;
  setMaxLogLines(value: number): void;
}

export function createConfigStore(dependencies: ConfigStoreDependencies) {
  let loadPromise: Promise<void> | null = null;

  const store = reactive({
    config: null as AppConfig | null,
    isLoaded: false,
    isLoading: false,
    isSaving: false,

    async refresh() {
      store.isLoading = true;
      try {
        store.config = await dependencies.getConfig();
        store.isLoaded = true;
      } finally {
        store.isLoading = false;
      }
    },

    async ensureLoaded() {
      if (store.isLoaded) return;
      if (!loadPromise) {
        loadPromise = store.refresh().finally(() => {
          loadPromise = null;
        });
      }
      await loadPromise;
    },

    async saveSync() {
      if (store.isSaving) return;
      await store.ensureLoaded();
      if (!store.config) throw new Error('Configuration is not loaded');

      store.isSaving = true;
      try {
        await dependencies.updateSyncConfig(buildSyncPatch(store.config));
        await store.refresh();
        await dependencies.restartSchedulerInterval();
        dependencies.addConfigEvent();
      } finally {
        store.isSaving = false;
      }
    },

    async saveApp() {
      if (store.isSaving) return;
      await store.ensureLoaded();
      if (!store.config) throw new Error('Configuration is not loaded');

      store.isSaving = true;
      try {
        await dependencies.updateAppConfig(buildAppPatch(store.config));
        await store.refresh();
        if (store.config && store.config.max_log_lines > 0) {
          dependencies.setMaxLogLines(store.config.max_log_lines);
        }
        dependencies.addConfigEvent();
      } finally {
        store.isSaving = false;
      }
    },
  });

  return store;
}
