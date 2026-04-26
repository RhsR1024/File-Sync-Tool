<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import { FolderOpen, TriangleAlert } from 'lucide-vue-next';

import type { ClipboardItem, FilePathStatus } from '@/lib/clipboardTypes';

const props = defineProps<{
  open: boolean;
  item: ClipboardItem | null;
  statuses: FilePathStatus[] | null;
  busy?: boolean;
}>();

const emit = defineEmits<{
  close: [];
  openPath: [path: string];
}>();

const { t } = useI18n();
const dialogRef = ref<HTMLElement | null>(null);

const rows = computed(() => {
  const paths = props.item?.file_paths ?? [];
  const statusMap = new Map((props.statuses ?? []).map((status) => [status.path, status]));
  return paths.map((path) => {
    const status = statusMap.get(path);
    return {
      path,
      exists: status?.exists ?? false,
      size: status?.size ?? null,
    };
  });
});

function formatSize(size: number | null): string {
  if (size == null) return '-';
  if (size < 1024) return `${size} B`;
  if (size < 1024 * 1024) return `${(size / 1024).toFixed(1)} KB`;
  return `${(size / (1024 * 1024)).toFixed(2)} MB`;
}

function focusFirstElement() {
  const first = dialogRef.value?.querySelector<HTMLElement>('button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])');
  first?.focus();
}

function onWindowKeydown(event: KeyboardEvent) {
  if (!props.open) return;
  if (event.key === 'Escape') {
    event.preventDefault();
    emit('close');
    return;
  }
  if (event.key !== 'Tab' || !dialogRef.value) return;

  const focusable = Array.from(
    dialogRef.value.querySelectorAll<HTMLElement>('button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])'),
  ).filter((node) => !node.hasAttribute('disabled'));
  if (focusable.length === 0) return;

  const first = focusable[0];
  const last = focusable[focusable.length - 1];
  const active = document.activeElement as HTMLElement | null;
  if (event.shiftKey && active === first) {
    event.preventDefault();
    last.focus();
  } else if (!event.shiftKey && active === last) {
    event.preventDefault();
    first.focus();
  }
}

watch(
  () => props.open,
  async (open) => {
    window.removeEventListener('keydown', onWindowKeydown);
    if (!open) return;
    await nextTick();
    focusFirstElement();
    window.addEventListener('keydown', onWindowKeydown);
  },
  { immediate: true },
);

onBeforeUnmount(() => {
  window.removeEventListener('keydown', onWindowKeydown);
});
</script>

<template>
  <div
    v-if="props.open"
    class="fixed inset-0 z-[75] flex items-center justify-center bg-slate-950/30 px-4"
    @click.self="emit('close')"
  >
    <div
      ref="dialogRef"
      class="w-full max-w-2xl rounded-2xl bg-white p-5 shadow-2xl"
      role="dialog"
      aria-modal="true"
      :aria-label="t('clipboard.fileDetails.title')"
    >
      <div class="flex items-start justify-between gap-4">
        <div>
          <h3 class="text-base font-semibold text-slate-900">
            {{ t('clipboard.fileDetails.title') }}
          </h3>
          <p class="mt-1 text-sm text-slate-500">
            {{ t('clipboard.fileDetails.subtitle', { n: rows.length }) }}
          </p>
        </div>
        <button
          type="button"
          class="rounded-lg px-2 py-1 text-sm text-slate-500 transition-colors hover:bg-slate-100 hover:text-slate-700"
          :aria-label="t('clipboard.actions.close')"
          @click="emit('close')"
        >
          {{ t('clipboard.actions.close') }}
        </button>
      </div>

      <div class="mt-4 max-h-[420px] space-y-2 overflow-y-auto pr-1">
        <div
          v-if="props.busy"
          class="rounded-xl border border-slate-200 bg-slate-50 px-4 py-3 text-sm text-slate-500"
        >
          {{ t('clipboard.loading') }}
        </div>

        <div
          v-for="row in rows"
          :key="row.path"
          class="rounded-xl border px-4 py-3"
          :class="row.exists
            ? 'border-slate-200 bg-white'
            : 'border-red-200 bg-red-50/60'"
        >
          <div class="flex items-start justify-between gap-3">
            <div class="min-w-0 flex-1">
              <div class="break-all text-sm font-medium text-slate-800">
                {{ row.path }}
              </div>
              <div class="mt-1 flex flex-wrap items-center gap-2 text-xs text-slate-500">
                <span
                  class="rounded-full px-2 py-0.5 font-medium"
                  :class="row.exists
                    ? 'bg-emerald-100 text-emerald-700'
                    : 'bg-red-100 text-red-700'"
                >
                  {{ row.exists ? t('clipboard.fileDetails.exists') : t('clipboard.fileDetails.missing') }}
                </span>
                <span>{{ t('clipboard.fileDetails.size', { size: formatSize(row.size) }) }}</span>
              </div>
            </div>
            <button
              type="button"
              class="inline-flex shrink-0 items-center gap-1 rounded-lg border border-slate-200 px-2.5 py-1.5 text-xs font-medium text-slate-700 transition-colors hover:bg-slate-50 disabled:cursor-not-allowed disabled:opacity-40"
              :disabled="!row.exists"
              @click="emit('openPath', row.path)"
            >
              <FolderOpen class="h-3.5 w-3.5" />
              {{ t('clipboard.actions.openInExplorer') }}
            </button>
          </div>

          <div
            v-if="!row.exists"
            class="mt-2 flex items-center gap-1.5 text-xs text-red-600"
          >
            <TriangleAlert class="h-3.5 w-3.5" />
            {{ t('clipboard.fileDetails.invalidHint') }}
          </div>
        </div>

        <div
          v-if="!props.busy && rows.length === 0"
          class="rounded-xl border border-slate-200 bg-slate-50 px-4 py-3 text-sm text-slate-500"
        >
          {{ t('clipboard.fileDetails.empty') }}
        </div>
      </div>
    </div>
  </div>
</template>
