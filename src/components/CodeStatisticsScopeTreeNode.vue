<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { ChevronRight, FileCode2, Folder, FolderOpen } from 'lucide-vue-next';
import type { CodeCountScopeTreeNode } from '@/lib/tauri';

defineOptions({ name: 'CodeStatisticsScopeTreeNode' });

const props = withDefaults(
  defineProps<{
    node: CodeCountScopeTreeNode;
    selectedLeafKeys: string[];
    expandedKeys: string[];
    level?: number;
  }>(),
  {
    level: 0,
  },
);

const emit = defineEmits<{
  (e: 'toggle-selection', node: CodeCountScopeTreeNode): void;
  (e: 'toggle-expand', key: string): void;
}>();

const checkboxRef = ref<HTMLInputElement | null>(null);

const collectLeafKeys = (node: CodeCountScopeTreeNode): string[] => {
  if (node.kind === 'file') {
    return [node.key];
  }

  return node.children.flatMap(collectLeafKeys);
};

const isDirectory = computed(() => props.node.kind === 'directory');
const leafKeys = computed(() => collectLeafKeys(props.node));
const hasSelectableLeaves = computed(() => leafKeys.value.length > 0);
const selectedKeySet = computed(() => new Set(props.selectedLeafKeys));
const selectedLeafCount = computed(() =>
  leafKeys.value.filter((key) => selectedKeySet.value.has(key)).length,
);
const isChecked = computed(
  () => leafKeys.value.length > 0 && selectedLeafCount.value === leafKeys.value.length,
);
const isPartial = computed(
  () => selectedLeafCount.value > 0 && selectedLeafCount.value < leafKeys.value.length,
);
const isExpanded = computed(
  () => !isDirectory.value || props.expandedKeys.includes(props.node.key),
);

watch(
  [isPartial, isChecked],
  () => {
    if (!checkboxRef.value) return;
    checkboxRef.value.indeterminate = isPartial.value;
  },
  { immediate: true },
);

const handleToggleSelection = () => {
  if (!hasSelectableLeaves.value) return;
  emit('toggle-selection', props.node);
};

const handleToggleExpand = () => {
  if (!isDirectory.value) return;
  emit('toggle-expand', props.node.key);
};
</script>

<template>
  <div class="space-y-1">
    <div
      class="flex items-center gap-2 rounded-xl px-2 py-1.5 hover:bg-white/80 transition-colors"
      :style="{ paddingLeft: `${level * 18 + 8}px` }"
    >
      <button
        v-if="isDirectory"
        type="button"
        class="flex h-6 w-6 items-center justify-center rounded-md text-slate-500 hover:bg-slate-100 hover:text-slate-700 transition-colors"
        @click="handleToggleExpand"
      >
        <ChevronRight
          class="h-4 w-4 transition-transform"
          :class="isExpanded ? 'rotate-90' : ''"
        />
      </button>
      <span v-else class="block h-6 w-6 shrink-0"></span>

      <input
        ref="checkboxRef"
        type="checkbox"
        class="h-4 w-4 rounded border-slate-300 text-blue-600 focus:ring-blue-500"
        :checked="isChecked"
        :disabled="!hasSelectableLeaves"
        @change="handleToggleSelection"
      />

      <button
        type="button"
        class="flex min-w-0 flex-1 items-center gap-2 text-left"
        @click="isDirectory ? handleToggleExpand() : handleToggleSelection()"
      >
        <FolderOpen
          v-if="isDirectory && isExpanded"
          class="h-4 w-4 shrink-0 text-amber-500"
        />
        <Folder
          v-else-if="isDirectory"
          class="h-4 w-4 shrink-0 text-amber-500"
        />
        <FileCode2 v-else class="h-4 w-4 shrink-0 text-sky-500" />
        <span class="truncate text-sm font-medium text-slate-800">{{ node.label }}</span>
      </button>

      <span
        v-if="isDirectory"
        class="shrink-0 rounded-full bg-slate-100 px-2 py-0.5 text-[10px] text-slate-500"
      >
        {{ leafKeys.length }}
      </span>
    </div>

    <div v-if="isDirectory && isExpanded" class="space-y-1">
      <CodeStatisticsScopeTreeNode
        v-for="child in node.children"
        :key="child.key"
        :node="child"
        :selected-leaf-keys="selectedLeafKeys"
        :expanded-keys="expandedKeys"
        :level="level + 1"
        @toggle-selection="emit('toggle-selection', $event)"
        @toggle-expand="emit('toggle-expand', $event)"
      />
    </div>
  </div>
</template>
