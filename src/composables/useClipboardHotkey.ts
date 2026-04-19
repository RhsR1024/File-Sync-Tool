import { onBeforeUnmount, onMounted, type Ref } from 'vue';

import type { ClipboardFilter, ClipboardItem } from '@/lib/clipboardTypes';

export interface ClipboardHotkeyOptions {
  items: Ref<ClipboardItem[]>;
  selectedIndex: Ref<number>;
  filter: Ref<ClipboardFilter>;
  searchValue: Ref<string>;
  onPaste: (id: number, plain: boolean) => void;
  onDelete: (id: number) => void;
  onFavorite: (id: number) => void;
  onClose: () => void;
  onFocusSearch: () => void;
  onFilterChange: (dir: 1 | -1) => void;
}

export function useClipboardHotkey(opts: ClipboardHotkeyOptions): void {
  function isEditable(el: EventTarget | null): boolean {
    if (!(el instanceof HTMLElement)) return false;
    const tag = el.tagName;
    return tag === 'INPUT' || tag === 'TEXTAREA' || el.isContentEditable;
  }

  function handler(e: KeyboardEvent) {
    const inEditable = isEditable(e.target);
    const list = opts.items.value;
    const idx = opts.selectedIndex.value;

    switch (e.key) {
      case 'ArrowDown':
        if (inEditable) return;
        if (list.length === 0) return;
        opts.selectedIndex.value = (idx + 1) % list.length;
        e.preventDefault();
        break;
      case 'ArrowUp':
        if (inEditable) return;
        if (list.length === 0) return;
        opts.selectedIndex.value = (idx - 1 + list.length) % list.length;
        e.preventDefault();
        break;
      case 'ArrowLeft':
        if (inEditable) return;
        opts.onFilterChange(-1);
        e.preventDefault();
        break;
      case 'ArrowRight':
        if (inEditable) return;
        opts.onFilterChange(1);
        e.preventDefault();
        break;
      case 'Enter': {
        const current = list[idx];
        if (current) {
          opts.onPaste(current.id, e.shiftKey);
          e.preventDefault();
        }
        break;
      }
      case 'Delete': {
        if (inEditable) return;
        const current = list[idx];
        if (current) {
          opts.onDelete(current.id);
          e.preventDefault();
        }
        break;
      }
      case 'd':
      case 'D':
        if (e.ctrlKey) {
          const current = list[idx];
          if (current) {
            opts.onFavorite(current.id);
            e.preventDefault();
          }
        }
        break;
      case 'f':
      case 'F':
        if (e.ctrlKey) {
          opts.onFocusSearch();
          e.preventDefault();
        }
        break;
      case '/':
        if (!inEditable) {
          opts.onFocusSearch();
          e.preventDefault();
        }
        break;
      case 'Escape':
        if (opts.searchValue.value) {
          opts.searchValue.value = '';
          e.preventDefault();
        } else {
          opts.onClose();
          e.preventDefault();
        }
        break;
    }
  }

  onMounted(() => window.addEventListener('keydown', handler));
  onBeforeUnmount(() => window.removeEventListener('keydown', handler));
}
