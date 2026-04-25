import { listen } from '@tauri-apps/api/event';
import { ref } from 'vue';

import type {
  DownloadCompletePayload,
  DownloadProgress,
  UpdateState,
} from '@/lib/tauri';
import { updaterApi } from '@/lib/tauri';

export type UpdaterDialogState =
  | 'closed'
  | 'found'
  | 'downloading'
  | 'ready'
  | 'resume'
  | 'verify_failed'
  | 'network_error';

const state = ref<UpdateState | null>(null);
const progress = ref<DownloadProgress | null>(null);
const dialogOpen = ref(false);
const dialogState = ref<UpdaterDialogState>('closed');
const dialogError = ref<string | null>(null);

let initialized = false;
let initPromise: Promise<void> | null = null;

function applyInitialDialogState() {
  if (state.value?.pending_update) {
    dialogState.value = 'resume';
    dialogOpen.value = true;
    dialogError.value = null;
  }
}

async function init() {
  state.value = await updaterApi.getState();
  applyInitialDialogState();

  await listen<UpdateState>('update-state-changed', (event) => {
    const previousPending = state.value?.pending_update?.temp_path ?? null;
    state.value = event.payload;
    if (!previousPending && state.value?.pending_update && dialogState.value === 'closed') {
      dialogState.value = 'resume';
      dialogOpen.value = true;
      dialogError.value = null;
    }
  });

  await listen<DownloadProgress>('update-download-progress', (event) => {
    progress.value = event.payload;
    dialogState.value = 'downloading';
    dialogOpen.value = true;
  });

  await listen<DownloadCompletePayload>('update-download-complete', (event) => {
    progress.value = null;
    if (event.payload.sha256_ok) {
      dialogState.value = 'ready';
      dialogError.value = null;
      dialogOpen.value = true;
      return;
    }

    const error = event.payload.error ?? 'unknown';
    if (error === 'cancelled') {
      dialogState.value = 'closed';
      dialogError.value = null;
      dialogOpen.value = false;
      return;
    }

    dialogError.value = error;
    dialogState.value = error.includes('verify_failed') ? 'verify_failed' : 'network_error';
    dialogOpen.value = true;
  });

  await listen(EVENT_OPEN_DIALOG, () => {
    if (state.value?.has_update) {
      dialogState.value = 'found';
      dialogError.value = null;
      dialogOpen.value = true;
    }
  });
}

const EVENT_OPEN_DIALOG = 'open-update-dialog';

export function ensureUpdaterInitialized() {
  if (initialized) {
    return initPromise ?? Promise.resolve();
  }

  initialized = true;
  initPromise = init().catch((error) => {
    initialized = false;
    initPromise = null;
    throw error;
  });
  return initPromise;
}

export function useUpdater() {
  return {
    state,
    progress,
    dialogOpen,
    dialogState,
    dialogError,
  };
}
