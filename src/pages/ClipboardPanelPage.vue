<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { useI18n } from 'vue-i18n';

import { useClipboardStore } from '@/composables/useClipboardStore';
import { useClipboardHotkey } from '@/composables/useClipboardHotkey';
import { useHoverPreview } from '@/composables/useHoverPreview';
import ClipboardList from '@/components/clipboard/ClipboardList.vue';
import ClipboardHoverPreview from '@/components/clipboard/ClipboardHoverPreview.vue';
import { clipboardApi } from '@/lib/tauri';
import type { ClipboardFilter } from '@/lib/clipboardTypes';

defineOptions({ name: 'ClipboardPanelPage' });

const { t } = useI18n();
const store = useClipboardStore();

const selectedIndex = ref(0);
const searchInput = ref<HTMLInputElement | null>(null);

const filters: ClipboardFilter[] = ['all', 'text', 'image', 'file', 'favorite'];

const selectedId = computed<number | null>(
  () => store.items.value[selectedIndex.value]?.id ?? null,
);

async function paste(id: number, plain: boolean) {
  try {
    if (plain) await clipboardApi.pastePlain(id);
    else await clipboardApi.paste(id);
  } catch (e) {
    console.error('[clipboard] paste failed:', e);
    store.error.value = `${t('clipboard.errors.pasteFailed')} — ${e}`;
  }
}

async function onReorder(ids: number[]) {
  try {
    await clipboardApi.reorderFavorites(ids);
    await store.reload();
  } catch (e) {
    console.error('[clipboard] reorder failed:', e);
    store.error.value = `${t('clipboard.errors.saveFailed')} — ${e}`;
  }
}

function close() {
  void getCurrentWindow().hide();
}

// Explicit drag handler. `data-tauri-drag-region` alone is unreliable on
// transparent undecorated windows in Tauri 2.10, so we also start dragging
// directly on left-button mousedown in the header (skipping interactive
// children like the close button).
function onHeaderMouseDown(e: MouseEvent) {
  if (e.button !== 0) return;
  const target = e.target as HTMLElement | null;
  if (target && target.closest('button, input, a, [data-no-drag]')) return;
  void getCurrentWindow().startDragging();
}

function changeFilter(dir: 1 | -1) {
  const cur = store.filter.value;
  const curIdx = filters.indexOf(cur);
  const next = filters[(curIdx + dir + filters.length) % filters.length];
  store.filter.value = next;
  selectedIndex.value = 0;
  void store.reload();
}

function setFilter(f: ClipboardFilter) {
  store.filter.value = f;
  selectedIndex.value = 0;
  void store.reload();
}

function selectById(id: number) {
  const idx = store.items.value.findIndex((it) => it.id === id);
  if (idx >= 0) selectedIndex.value = idx;
}

const preview = useHoverPreview();

function onListSelect(id: number) {
  selectById(id);
  const item = store.items.value.find((it) => it.id === id);
  if (item && item.kind === 'image') preview.onEnter(item);
}

useClipboardHotkey({
  items: store.items,
  selectedIndex,
  filter: store.filter,
  searchValue: store.search,
  onPaste: paste,
  onDelete: (id) => store.remove(id),
  onFavorite: (id) => store.toggleFavorite(id),
  onClose: close,
  onFocusSearch: () => searchInput.value?.focus(),
  onFilterChange: changeFilter,
});

// Keep selection in-bounds when the list shrinks.
watch(
  () => store.items.value.length,
  (len) => {
    if (selectedIndex.value >= len) {
      selectedIndex.value = Math.max(0, len - 1);
    }
  },
);

let unlistenShown: UnlistenFn | null = null;
let unlistenItemAdded: UnlistenFn | null = null;
// Bump whenever the panel becomes visible so DynamicScroller remounts and
// measures its container in a visible window. The virtual scroller doesn't
// compute visible rows while the window is hidden, which produced an empty
// "All" view on first open.
const showCounter = ref(0);
const listKey = computed(() => `${store.filter.value}-${showCounter.value}`);

onMounted(async () => {
  unlistenShown = await listen('clipboard-panel-shown', async () => {
    store.search.value = '';
    selectedIndex.value = 0;
    await store.reload();
    // Wait two frames so the webview finishes its first paint after show()
    // before we force-remount the list, otherwise the virtual scroller can
    // still measure a zero-height container.
    await nextTick();
    await new Promise<void>((resolve) => {
      requestAnimationFrame(() => requestAnimationFrame(() => resolve()));
    });
    showCounter.value += 1;
    searchInput.value?.focus();
  });
  unlistenItemAdded = await store.startListening();
  // Defer the first reload until the panel is actually shown. Mounting the
  // virtual scroller while the window is hidden leaves it stuck at 0 visible
  // rows on first open.
});

onBeforeUnmount(() => {
  unlistenShown?.();
  unlistenItemAdded?.();
});
</script>

<template>
  <div class="flex h-screen w-screen flex-col overflow-hidden bg-white">
    <header
      class="flex select-none items-center justify-between border-b border-slate-200 px-4 py-3"
      data-tauri-drag-region
      @mousedown="onHeaderMouseDown"
    >
      <span class="pointer-events-none text-sm font-semibold text-slate-700">{{ t('clipboard.tool.title') }}</span>
      <button
        type="button"
        data-no-drag
        class="text-xs text-slate-400 transition-colors hover:text-slate-700"
        :title="t('clipboard.actions.close')"
        @click="close"
      >
        ✕
      </button>
    </header>

    <div class="px-3 pt-3 pb-2">
      <input
        ref="searchInput"
        v-model="store.search.value"
        type="search"
        :placeholder="t('clipboard.search.placeholder')"
        class="w-full rounded-lg border border-slate-200/70 bg-white/60 px-3 py-1.5 text-sm outline-none focus:border-slate-400"
        @input="store.reload()"
      />
    </div>

    <div class="flex flex-wrap gap-1 px-3 pb-2">
      <button
        v-for="f in filters"
        :key="f"
        type="button"
        class="rounded-full px-2.5 py-0.5 text-xs transition-colors"
        :class="store.filter.value === f
          ? 'bg-slate-900 text-white shadow-sm'
          : 'bg-slate-200/60 text-slate-600 hover:bg-slate-200'"
        @click="setFilter(f)"
      >
        {{ t(`clipboard.filter.${f}`) }}
      </button>
    </div>

    <div
      class="flex-1 overflow-hidden px-2 pb-2"
      @mouseleave="preview.onLeave()"
      @wheel="preview.onWheelZoom($event)"
    >
      <div v-if="store.items.value.length === 0" class="flex h-full items-center justify-center p-6 text-center text-sm text-slate-400">
        {{ store.search.value ? t('clipboard.panel.noMatch') : t('clipboard.panel.empty') }}
      </div>
      <ClipboardList
        v-else
        :key="listKey"
        :items="store.items.value"
        :selected-id="selectedId"
        :compact="true"
        :draggable="store.filter.value === 'favorite'"
        @select="onListSelect"
        @activate="(id) => paste(id, false)"
        @reorder="onReorder"
      />
    </div>
  </div>

  <ClipboardHoverPreview
    v-if="preview.activeItem.value"
    :item="preview.activeItem.value"
    :scale="preview.scale.value"
  />
</template>
