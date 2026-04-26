<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { ChevronRight, FileCode2, Folder, FolderOpen } from 'lucide-vue-next';
import type { CodeCountScopeTreeNode } from '@/lib/tauri';

defineOptions({ name: 'CodeStatisticsScopeTreeNode' });

const props = withDefaults(
  defineProps<{
    node: CodeCountScopeTreeNode;
    selectedKeySet: Set<string>;
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
const rowRef = ref<HTMLElement | null>(null);

const collectLeafKeys = (node: CodeCountScopeTreeNode): string[] => {
  if (node.kind === 'file') {
    return [node.key];
  }

  return node.children.flatMap(collectLeafKeys);
};

const isDirectory = computed(() => props.node.kind === 'directory');
const leafKeys = computed(() => collectLeafKeys(props.node));
const selectedLeafCount = computed(() => {
  let count = 0;
  for (const key of leafKeys.value) {
    if (props.selectedKeySet.has(key)) count += 1;
  }
  return count;
});
const isEmptyDirectory = computed(() => isDirectory.value && leafKeys.value.length === 0);
const isChecked = computed(() => {
  if (isEmptyDirectory.value) {
    return props.selectedKeySet.has(props.node.key);
  }
  return leafKeys.value.length > 0 && selectedLeafCount.value === leafKeys.value.length;
});
const isPartial = computed(
  () => selectedLeafCount.value > 0 && selectedLeafCount.value < leafKeys.value.length,
);
const isExpanded = computed(
  () => !isDirectory.value || props.expandedKeys.includes(props.node.key),
);
// ARIA tree levels are 1-based; level prop is 0-based.
const ariaLevel = computed(() => props.level + 1);
// Tri-state checkbox status — partial directories report 'mixed' so screen
// readers announce the indeterminate state. Using `aria-checked` instead of
// `aria-selected` because the latter does not accept the 'mixed' token.
const ariaChecked = computed<'true' | 'false' | 'mixed'>(() => {
  if (isPartial.value) return 'mixed';
  return isChecked.value ? 'true' : 'false';
});

watch(
  [isPartial, isChecked],
  () => {
    if (!checkboxRef.value) return;
    checkboxRef.value.indeterminate = isPartial.value;
  },
  { immediate: true },
);

const handleToggleSelection = () => {
  emit('toggle-selection', props.node);
};

const handleToggleExpand = () => {
  if (!isDirectory.value) return;
  emit('toggle-expand', props.node.key);
};

// ── Keyboard navigation (WAI-ARIA tree pattern) ─────────────────
//
// We rely on the parent component to wrap the rendered tree with
// `role="tree"`. Within that scope we walk the flat list of currently
// rendered `[role="treeitem"]` elements via DOM queries — this avoids
// threading focus state through every recursive prop.

const getTreeItems = (el: HTMLElement): HTMLElement[] => {
  const tree = el.closest('[role="tree"]');
  if (!tree) return [];
  return Array.from(tree.querySelectorAll('[role="treeitem"]')) as HTMLElement[];
};

const focusByOffset = (offset: 1 | -1) => {
  const el = rowRef.value;
  if (!el) return;
  const items = getTreeItems(el);
  const idx = items.indexOf(el);
  if (idx === -1) return;
  const target = items[idx + offset];
  if (target) target.focus();
};

const focusParent = () => {
  const el = rowRef.value;
  if (!el) return;
  const items = getTreeItems(el);
  const idx = items.indexOf(el);
  if (idx <= 0) return;
  const myLevel = Number.parseInt(el.getAttribute('aria-level') ?? '1', 10);
  for (let i = idx - 1; i >= 0; i--) {
    const lvl = Number.parseInt(items[i].getAttribute('aria-level') ?? '1', 10);
    if (lvl < myLevel) {
      items[i].focus();
      return;
    }
  }
};

const handleKeydown = (event: KeyboardEvent) => {
  // Only react when the row itself is the keyboard target. Lets the inner
  // checkbox / buttons keep their native semantics for mouse / screen reader
  // users without us swallowing their keystrokes.
  if (event.target !== rowRef.value) return;

  switch (event.key) {
    case 'ArrowRight':
      event.preventDefault();
      if (isDirectory.value) {
        if (!isExpanded.value) {
          emit('toggle-expand', props.node.key);
        } else {
          focusByOffset(1);
        }
      }
      break;
    case 'ArrowLeft':
      event.preventDefault();
      if (isDirectory.value && isExpanded.value) {
        emit('toggle-expand', props.node.key);
      } else {
        focusParent();
      }
      break;
    case 'ArrowDown':
      event.preventDefault();
      focusByOffset(1);
      break;
    case 'ArrowUp':
      event.preventDefault();
      focusByOffset(-1);
      break;
    case ' ':
    case 'Spacebar':
      event.preventDefault();
      handleToggleSelection();
      break;
    case 'Enter':
      event.preventDefault();
      if (isDirectory.value) {
        emit('toggle-expand', props.node.key);
      } else {
        handleToggleSelection();
      }
      break;
  }
};
</script>

<template>
  <div class="space-y-1">
    <div
      ref="rowRef"
      role="treeitem"
      :aria-level="ariaLevel"
      :aria-expanded="isDirectory ? isExpanded : undefined"
      :aria-checked="ariaChecked"
      tabindex="0"
      class="flex items-center gap-2 rounded-xl px-2 py-1.5 hover:bg-white/80 transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-blue-500/50 focus-visible:ring-offset-1 focus-visible:ring-offset-white"
      :style="{ paddingLeft: `${level * 18 + 8}px` }"
      @keydown="handleKeydown"
    >
      <button
        v-if="isDirectory"
        type="button"
        tabindex="-1"
        class="flex h-6 w-6 items-center justify-center rounded-md text-slate-500 hover:bg-slate-100 hover:text-slate-700 transition-colors"
        @click="handleToggleExpand"
      >
        <ChevronRight
          class="h-4 w-4 transition-transform motion-reduce:transition-none"
          :class="isExpanded ? 'rotate-90' : ''"
        />
      </button>
      <span v-else class="block h-6 w-6 shrink-0"></span>

      <input
        ref="checkboxRef"
        type="checkbox"
        tabindex="-1"
        class="h-4 w-4 rounded border-slate-300 text-blue-600 focus:ring-blue-500"
        :checked="isChecked"
        @change="handleToggleSelection"
      />

      <button
        type="button"
        tabindex="-1"
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
        class="shrink-0 rounded-full bg-slate-100 px-2 py-0.5 text-[10px] text-slate-500 tabular-nums"
      >
        {{ leafKeys.length }}
      </span>
    </div>

    <div v-if="isDirectory && isExpanded" role="group" class="space-y-1">
      <CodeStatisticsScopeTreeNode
        v-for="child in node.children"
        :key="child.key"
        :node="child"
        :selected-key-set="selectedKeySet"
        :expanded-keys="expandedKeys"
        :level="level + 1"
        @toggle-selection="emit('toggle-selection', $event)"
        @toggle-expand="emit('toggle-expand', $event)"
      />
    </div>
  </div>
</template>
