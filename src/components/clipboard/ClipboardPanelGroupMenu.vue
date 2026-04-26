<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from 'vue';
import { Check, ChevronDown, Pencil, Plus, Trash2, X } from 'lucide-vue-next';
import { useI18n } from 'vue-i18n';

import {
  buildClipboardPanelGroupRows,
  resolveClipboardPanelGroupLabel,
} from '@/lib/clipboardPanelGroupsMenu';
import type { ClipboardGroup } from '@/lib/clipboardTypes';

const props = defineProps<{
  groups: ClipboardGroup[];
  selectedGroupId: number | null;
}>();

const emit = defineEmits<{
  select: [groupId: number | null];
  create: [name: string];
  rename: [payload: { id: number; name: string }];
  delete: [group: ClipboardGroup];
}>();

const { t } = useI18n();
const rootRef = ref<HTMLElement | null>(null);
const createInputRef = ref<HTMLInputElement | null>(null);
const renameInputRef = ref<HTMLInputElement | null>(null);
const menuOpen = ref(false);
const creating = ref(false);
const newGroupName = ref('');
const editingGroupId = ref<number | null>(null);
const editingName = ref('');

const labels = computed(() => ({
  defaultGroup: t('clipboard.groups.default'),
  createGroup: t('clipboard.groups.add'),
}));
const rows = computed(() =>
  buildClipboardPanelGroupRows(props.groups, props.selectedGroupId, labels.value),
);
const triggerLabel = computed(() =>
  resolveClipboardPanelGroupLabel(props.groups, props.selectedGroupId, labels.value.defaultGroup),
);

function focusFirstInteractive() {
  const first = rootRef.value?.querySelector<HTMLElement>('[role="menuitem"], input, button');
  first?.focus();
}

function resetEditors() {
  creating.value = false;
  newGroupName.value = '';
  editingGroupId.value = null;
  editingName.value = '';
}

function closeMenu() {
  menuOpen.value = false;
  resetEditors();
}

function toggleMenu() {
  menuOpen.value = !menuOpen.value;
  if (!menuOpen.value) {
    resetEditors();
  } else {
    void nextTick().then(() => {
      focusFirstInteractive();
    });
  }
}

function selectGroup(groupId: number | null) {
  emit('select', groupId);
  closeMenu();
}

async function startCreate() {
  editingGroupId.value = null;
  editingName.value = '';
  creating.value = true;
  newGroupName.value = '';
  await nextTick();
  createInputRef.value?.focus();
}

function submitCreate() {
  const name = newGroupName.value.trim();
  if (!name) return;
  emit('create', name);
  closeMenu();
}

async function startRename(group: ClipboardGroup) {
  creating.value = false;
  newGroupName.value = '';
  editingGroupId.value = group.id;
  editingName.value = group.name;
  await nextTick();
  renameInputRef.value?.focus();
}

function submitRename(group: ClipboardGroup) {
  const name = editingName.value.trim();
  if (!name) return;
  emit('rename', { id: group.id, name });
  closeMenu();
}

function requestDelete(group: ClipboardGroup) {
  emit('delete', group);
  closeMenu();
}

function submitRenameById(id: number | null) {
  const group = groupById(id);
  if (!group) return;
  submitRename(group);
}

function startRenameById(id: number | null) {
  const group = groupById(id);
  if (!group) return;
  void startRename(group);
}

function requestDeleteById(id: number | null) {
  const group = groupById(id);
  if (!group) return;
  requestDelete(group);
}

function handleDocumentMouseDown(event: MouseEvent) {
  if (!menuOpen.value) return;
  if (rootRef.value?.contains(event.target as Node)) return;
  closeMenu();
}

function handleDocumentKeydown(event: KeyboardEvent) {
  if (event.key !== 'Escape' || !menuOpen.value) return;
  event.preventDefault();
  if (creating.value || editingGroupId.value !== null) {
    resetEditors();
    return;
  }
  closeMenu();
}

function groupById(id: number | null): ClipboardGroup | null {
  if (id === null) return null;
  return props.groups.find((group) => group.id === id) ?? null;
}

onMounted(() => {
  document.addEventListener('mousedown', handleDocumentMouseDown);
  document.addEventListener('keydown', handleDocumentKeydown);
});

onBeforeUnmount(() => {
  document.removeEventListener('mousedown', handleDocumentMouseDown);
  document.removeEventListener('keydown', handleDocumentKeydown);
});
</script>

<template>
  <div ref="rootRef" class="relative shrink-0" data-no-drag>
    <button
      type="button"
      class="inline-flex h-8 max-w-[132px] items-center gap-1 rounded-lg bg-white px-2.5 text-xs font-medium text-slate-700 shadow-sm ring-1 ring-slate-200 transition hover:bg-slate-50 hover:text-slate-900"
      aria-haspopup="menu"
      :aria-expanded="menuOpen"
      :title="triggerLabel"
      @click="toggleMenu"
    >
      <span class="truncate">{{ triggerLabel }}</span>
      <ChevronDown
        class="h-3.5 w-3.5 shrink-0 transition-transform duration-150"
        :class="menuOpen ? 'rotate-180' : ''"
      />
    </button>

    <transition
      enter-active-class="transition-all duration-150 ease-out"
      enter-from-class="translate-y-1 opacity-0"
      enter-to-class="translate-y-0 opacity-100"
      leave-active-class="transition-all duration-100 ease-in"
      leave-from-class="translate-y-0 opacity-100"
      leave-to-class="translate-y-1 opacity-0"
    >
      <div
        v-if="menuOpen"
        class="absolute bottom-full right-0 z-50 mb-1.5 w-[188px] rounded-xl border border-slate-200 bg-white p-1.5 shadow-[0_18px_40px_rgba(15,23,42,0.18)]"
        role="menu"
        :aria-label="t('clipboard.groups.title')"
      >
        <template v-for="row in rows" :key="row.kind === 'group' ? `group-${row.id ?? 'default'}` : 'create'">
          <div v-if="row.showSeparatorAbove" class="mx-1 my-1 h-px bg-slate-200" />

          <template v-if="row.kind === 'group'">
            <form
              v-if="editingGroupId === row.id && row.id !== null"
              class="space-y-1.5 rounded-lg bg-slate-50 p-1.5"
              @submit.prevent="submitRenameById(row.id)"
            >
              <input
                ref="renameInputRef"
                v-model="editingName"
                type="text"
                class="w-full rounded-md border border-slate-200 bg-white px-2 py-1.5 text-xs text-slate-700 outline-none transition focus:border-slate-300 focus:ring-2 focus:ring-slate-200"
                :placeholder="t('clipboard.groups.newPlaceholder')"
              >
              <div class="flex items-center gap-1">
                <button
                  type="submit"
                  class="inline-flex flex-1 items-center justify-center gap-1 rounded-md bg-slate-900 px-2 py-1.5 text-[11px] font-medium text-white transition hover:bg-slate-700"
                >
                  <Check class="h-3.5 w-3.5" />
                  <span>{{ t('clipboard.groups.save') }}</span>
                </button>
                <button
                  type="button"
                  class="inline-flex h-7 w-7 items-center justify-center rounded-md border border-slate-200 text-slate-500 transition hover:bg-white hover:text-slate-700"
                  @click="resetEditors"
                >
                  <X class="h-3.5 w-3.5" />
                </button>
              </div>
            </form>

            <button
              v-else
              type="button"
              role="menuitem"
              class="group flex w-full items-center gap-2 rounded-lg px-2.5 py-2 text-left text-xs transition"
              :class="row.selected
                ? 'bg-slate-100 text-slate-900'
                : 'text-slate-600 hover:bg-slate-50 hover:text-slate-900'"
              @click="selectGroup(row.id)"
            >
              <span class="min-w-0 flex-1 truncate">{{ row.name }}</span>
              <div
                v-if="!row.isDefault"
                class="flex items-center gap-0.5 opacity-0 transition-opacity group-hover:opacity-100"
              >
                <button
                  type="button"
                  class="inline-flex h-5 w-5 items-center justify-center rounded text-slate-400 transition hover:bg-white hover:text-slate-700"
                  :title="t('clipboard.groups.rename')"
                  @click.stop="startRenameById(row.id)"
                >
                  <Pencil class="h-3 w-3" />
                </button>
                <button
                  type="button"
                  class="inline-flex h-5 w-5 items-center justify-center rounded text-slate-400 transition hover:bg-red-50 hover:text-red-600"
                  :title="t('clipboard.actions.delete')"
                  @click.stop="requestDeleteById(row.id)"
                >
                  <Trash2 class="h-3 w-3" />
                </button>
              </div>
            </button>
          </template>

          <form
            v-else-if="creating"
            class="space-y-1.5 rounded-lg bg-slate-50 p-1.5"
            @submit.prevent="submitCreate"
          >
            <input
              ref="createInputRef"
              v-model="newGroupName"
              type="text"
              class="w-full rounded-md border border-slate-200 bg-white px-2 py-1.5 text-xs text-slate-700 outline-none transition focus:border-slate-300 focus:ring-2 focus:ring-slate-200"
              :placeholder="t('clipboard.groups.newPlaceholder')"
            >
            <div class="flex items-center gap-1">
              <button
                type="submit"
                class="inline-flex flex-1 items-center justify-center gap-1 rounded-md bg-slate-900 px-2 py-1.5 text-[11px] font-medium text-white transition hover:bg-slate-700"
              >
                <Plus class="h-3.5 w-3.5" />
                <span>{{ t('clipboard.groups.add') }}</span>
              </button>
              <button
                type="button"
                class="inline-flex h-7 w-7 items-center justify-center rounded-md border border-slate-200 text-slate-500 transition hover:bg-white hover:text-slate-700"
                @click="resetEditors"
              >
                <X class="h-3.5 w-3.5" />
              </button>
            </div>
          </form>

          <button
            v-else
            type="button"
            role="menuitem"
            class="flex w-full items-center gap-2 rounded-lg px-2.5 py-2 text-left text-xs text-slate-600 transition hover:bg-slate-50 hover:text-slate-900"
            @click="startCreate"
          >
            <Plus class="h-3.5 w-3.5" />
            <span>{{ row.label }}</span>
          </button>
        </template>
      </div>
    </transition>
  </div>
</template>
