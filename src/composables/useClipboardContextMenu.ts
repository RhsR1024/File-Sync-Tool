import { computed, ref, type Ref } from 'vue';

import { clipboardApi, openDirectory } from '../lib/tauri';
import type { ClipboardItem, FilePathStatus } from '../lib/clipboardTypes';
import {
  buildClipboardMenuItems,
  buildImageSaveTargetPath,
  decodeMergeSeparatorInput,
  getPreferredExplorerPath,
  type ClipboardContextActionId,
} from './clipboardContextMenuHelpers';

export interface ClipboardContextMenuPosition {
  x: number;
  y: number;
}

interface UseClipboardContextMenuOptions {
  selectedIds: Ref<Set<number>>;
  onPaste: (id: number, plain: boolean) => Promise<void>;
  onCopy: (id: number) => Promise<void>;
  onDelete: (id: number) => Promise<void>;
  onToggleFavorite: (id: number) => Promise<void>;
  onError: (error: unknown, action: ClipboardContextActionId) => void;
  onMergeSuccess?: () => void | Promise<void>;
}

async function loadFileStatusesForItem(item: ClipboardItem): Promise<FilePathStatus[] | null> {
  if (item.kind !== 'file') return null;
  if (!item.file_paths?.length) return [];
  return clipboardApi.checkFilePaths([item.id]);
}

export function useClipboardContextMenu(options: UseClipboardContextMenuOptions) {
  const activeItem = ref<ClipboardItem | null>(null);
  const menuPosition = ref<ClipboardContextMenuPosition>({ x: 0, y: 0 });
  const menuOpen = ref(false);
  const fileStatuses = ref<FilePathStatus[] | null>(null);
  const fileStatusLoading = ref(false);
  const fileDetailsItem = ref<ClipboardItem | null>(null);
  const fileDetailsStatuses = ref<FilePathStatus[] | null>(null);
  const fileDetailsOpen = ref(false);
  const mergeDialogOpen = ref(false);
  const mergePending = ref(false);
  const mergeSeparatorInput = ref('\\n');

  async function refreshFileStatuses(item: ClipboardItem): Promise<FilePathStatus[] | null> {
    fileStatusLoading.value = true;
    try {
      const next = await loadFileStatusesForItem(item);
      fileStatuses.value = next;
      return next;
    } finally {
      fileStatusLoading.value = false;
    }
  }

  function closeMenu(): void {
    menuOpen.value = false;
  }

  function openMenu(item: ClipboardItem, position: ClipboardContextMenuPosition): void {
    activeItem.value = item;
    menuPosition.value = position;
    menuOpen.value = true;
    fileStatuses.value = item.kind === 'file' ? fileStatuses.value : null;

    if (item.kind === 'file') {
      void refreshFileStatuses(item).catch((error) => {
        options.onError(error, 'showFileDetails');
      });
    }
  }

  async function runAction(action: ClipboardContextActionId): Promise<void> {
    const item = activeItem.value;
    if (!item) return;

    try {
      switch (action) {
        case 'paste':
          await options.onPaste(item.id, false);
          break;
        case 'pastePlain':
          await options.onPaste(item.id, true);
          break;
        case 'copy':
          await options.onCopy(item.id);
          break;
        case 'pasteAsFiles':
          await clipboardApi.pasteAsFiles(item.id);
          break;
        case 'pasteAsPath':
          await clipboardApi.pasteAsPath(item.id);
          break;
        case 'showFileDetails':
          fileDetailsStatuses.value = await refreshFileStatuses(item);
          fileDetailsItem.value = item;
          fileDetailsOpen.value = true;
          break;
        case 'openInExplorer': {
          const statuses = item.kind === 'file' ? await refreshFileStatuses(item) : fileStatuses.value;
          const path = getPreferredExplorerPath(item, statuses);
          if (!path) return;
          await clipboardApi.openInExplorer(path);
          break;
        }
        case 'saveImageAs': {
          const directory = await openDirectory();
          if (!directory) return;
          await clipboardApi.saveImageAs(item.id, buildImageSaveTargetPath(directory, item));
          break;
        }
        case 'toggleFavorite':
          await options.onToggleFavorite(item.id);
          break;
        case 'delete':
          await options.onDelete(item.id);
          break;
      }
      closeMenu();
    } catch (error) {
      options.onError(error, action);
    }
  }

  function openMergeDialog(): void {
    mergeDialogOpen.value = true;
  }

  function closeMergeDialog(): void {
    mergeDialogOpen.value = false;
  }

  async function confirmMergePaste(): Promise<void> {
    if (options.selectedIds.value.size < 2) return;

    mergePending.value = true;
    try {
      await clipboardApi.mergePaste(
        Array.from(options.selectedIds.value),
        decodeMergeSeparatorInput(mergeSeparatorInput.value),
      );
      mergeDialogOpen.value = false;
      await options.onMergeSuccess?.();
    } catch (error) {
      options.onError(error, 'paste');
    } finally {
      mergePending.value = false;
    }
  }

  const menuItems = computed(() => {
    if (!activeItem.value) return [];
    return buildClipboardMenuItems({
      item: activeItem.value,
      fileStatuses: fileStatuses.value,
    });
  });

  const canMergeSelection = computed(() => options.selectedIds.value.size >= 2);

  return {
    activeItem,
    canMergeSelection,
    closeMenu,
    closeMergeDialog,
    confirmMergePaste,
    fileDetailsItem,
    fileDetailsOpen,
    fileDetailsStatuses,
    fileStatusLoading,
    fileStatuses,
    menuItems,
    menuOpen,
    menuPosition,
    mergeDialogOpen,
    mergePending,
    mergeSeparatorInput,
    openMenu,
    openMergeDialog,
    runAction,
  };
}
