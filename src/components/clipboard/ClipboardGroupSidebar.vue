<script setup lang="ts">
import { computed, ref } from 'vue';
import { Check, FolderTree, Pencil, Plus, Trash2, X } from 'lucide-vue-next';
import { useI18n } from 'vue-i18n';

import type { ClipboardGroup } from '@/lib/clipboardTypes';

const props = withDefaults(defineProps<{
  groups: ClipboardGroup[];
  selectedGroupId: number | null;
  compact?: boolean;
}>(), {
  compact: false,
});

const emit = defineEmits<{
  select: [groupId: number | null];
  create: [name: string];
  rename: [payload: { id: number; name: string }];
  delete: [group: ClipboardGroup];
}>();

const { t } = useI18n();
const newGroupName = ref('');
const editingGroupId = ref<number | null>(null);
const editingName = ref('');
const canCreate = computed(() => newGroupName.value.trim().length > 0);

function submitCreate() {
  const name = newGroupName.value.trim();
  if (!name) return;
  emit('create', name);
  newGroupName.value = '';
}

function beginRename(group: ClipboardGroup) {
  editingGroupId.value = group.id;
  editingName.value = group.name;
}

function cancelRename() {
  editingGroupId.value = null;
  editingName.value = '';
}

function submitRename(group: ClipboardGroup) {
  const name = editingName.value.trim();
  if (!name) return;
  emit('rename', { id: group.id, name });
  cancelRename();
}
</script>

<template>
  <aside
    class="flex h-full min-h-0 flex-col border-slate-200 bg-slate-50/80"
    :class="props.compact ? 'w-28 border-r px-2 py-2.5' : 'rounded-2xl border px-3 py-3.5'"
  >
    <div class="mb-3 flex items-center gap-2 px-1">
      <FolderTree class="h-4 w-4 text-slate-500" />
      <span class="text-xs font-semibold uppercase tracking-[0.18em] text-slate-500">
        {{ t('clipboard.groups.title') }}
      </span>
    </div>

    <form class="mb-3 space-y-2" @submit.prevent="submitCreate">
      <input
        v-model="newGroupName"
        type="text"
        class="w-full rounded-lg border border-slate-200 bg-white px-2.5 py-2 text-sm text-slate-700 outline-none transition focus:border-slate-300 focus:ring-2 focus:ring-slate-200"
        :placeholder="t('clipboard.groups.newPlaceholder')"
      />
      <button
        type="submit"
        class="inline-flex w-full items-center justify-center gap-1.5 rounded-lg bg-slate-900 px-2.5 py-2 text-sm font-medium text-white transition hover:bg-slate-700 disabled:cursor-not-allowed disabled:opacity-50"
        :disabled="!canCreate"
      >
        <Plus class="h-4 w-4" />
        <span>{{ t('clipboard.groups.add') }}</span>
      </button>
    </form>

    <button
      type="button"
      class="mb-2 flex w-full items-center gap-2 rounded-xl px-2.5 py-2 text-left text-sm transition"
      :class="props.selectedGroupId === null
        ? 'bg-white text-slate-900 shadow-sm ring-1 ring-slate-200'
        : 'text-slate-600 hover:bg-white hover:text-slate-900'"
      @click="emit('select', null)"
    >
      <span class="truncate">{{ t('clipboard.groups.all') }}</span>
    </button>

    <div class="min-h-0 flex-1 overflow-y-auto pr-1">
      <div v-if="!props.groups.length" class="rounded-xl border border-dashed border-slate-200 px-3 py-4 text-xs leading-5 text-slate-400">
        {{ t('clipboard.groups.empty') }}
      </div>

      <div v-else class="space-y-2">
        <div
          v-for="group in props.groups"
          :key="group.id"
          class="rounded-xl border border-transparent bg-white/70 p-2 transition"
          :class="props.selectedGroupId === group.id && 'border-slate-200 shadow-sm'"
        >
          <template v-if="editingGroupId === group.id">
            <form class="space-y-2" @submit.prevent="submitRename(group)">
              <input
                v-model="editingName"
                type="text"
                class="w-full rounded-lg border border-slate-200 bg-white px-2 py-1.5 text-sm text-slate-700 outline-none transition focus:border-slate-300 focus:ring-2 focus:ring-slate-200"
              />
              <div class="flex items-center gap-1">
                <button
                  type="submit"
                  class="inline-flex flex-1 items-center justify-center gap-1 rounded-lg bg-slate-900 px-2 py-1.5 text-xs font-medium text-white transition hover:bg-slate-700"
                >
                  <Check class="h-3.5 w-3.5" />
                  <span>{{ t('clipboard.groups.save') }}</span>
                </button>
                <button
                  type="button"
                  class="inline-flex items-center justify-center rounded-lg border border-slate-200 px-2 py-1.5 text-slate-500 transition hover:bg-slate-100"
                  @click="cancelRename"
                >
                  <X class="h-3.5 w-3.5" />
                </button>
              </div>
            </form>
          </template>

          <template v-else>
            <button
              type="button"
              class="flex w-full items-center justify-between gap-2 rounded-lg px-2 py-1.5 text-left text-sm transition"
              :class="props.selectedGroupId === group.id
                ? 'bg-slate-900 text-white'
                : 'text-slate-700 hover:bg-slate-100'"
              @click="emit('select', group.id)"
            >
              <span class="truncate">{{ group.name }}</span>
            </button>
            <div class="mt-2 flex items-center gap-1">
              <button
                type="button"
                class="inline-flex flex-1 items-center justify-center gap-1 rounded-lg border border-slate-200 px-2 py-1.5 text-xs text-slate-600 transition hover:bg-slate-100"
                @click="beginRename(group)"
              >
                <Pencil class="h-3.5 w-3.5" />
                <span>{{ t('clipboard.groups.rename') }}</span>
              </button>
              <button
                type="button"
                class="inline-flex items-center justify-center rounded-lg border border-red-200 px-2 py-1.5 text-red-500 transition hover:bg-red-50"
                @click="emit('delete', group)"
              >
                <Trash2 class="h-3.5 w-3.5" />
              </button>
            </div>
          </template>
        </div>
      </div>
    </div>
  </aside>
</template>
