# Settings Directory Picker Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add one reusable directory-path input component and use it for Settings local storage, manual deploy local path, and task-level local path selection.

**Architecture:** Extract a focused `DirectoryPathInput` component that wraps a text input with a folder-picker action using the existing `openDirectory()` Tauri command. Keep task-level fallback semantics unchanged by introducing a tiny helper module for nullable task paths, then wire the component into `SettingsPage.vue` and update i18n copy.

**Tech Stack:** Vue 3 `script setup`, TypeScript, lucide-vue-next, existing Tauri invoke wrapper, Node assert-based `.test.mjs` unit tests

---

### Task 1: Lock In Task Path Fallback Logic

**Files:**
- Create: `src/lib/settingsDirectoryPathState.ts`
- Create: `src/lib/settingsDirectoryPathState.test.mjs`

- [ ] **Step 1: Write the failing test**

```js
import assert from 'node:assert/strict';

import {
  getDirectoryInputValue,
  getTaskLocalPathHint,
  getTaskLocalPathPlaceholder,
  toOptionalDirectoryValue,
} from './settingsDirectoryPathState.ts';

assert.equal(getDirectoryInputValue(null), '');
assert.equal(getDirectoryInputValue('D:\\Builds'), 'D:\\Builds');

assert.equal(toOptionalDirectoryValue(''), null);
assert.equal(toOptionalDirectoryValue('   '), null);
assert.equal(toOptionalDirectoryValue(' D:\\Builds '), 'D:\\Builds');

assert.equal(getTaskLocalPathPlaceholder('D:\\GlobalTarget'), 'D:\\GlobalTarget');
assert.equal(getTaskLocalPathPlaceholder(''), '');

assert.equal(
  getTaskLocalPathHint('Use Local Storage target directory'),
  'Use Local Storage target directory',
);

console.log('settingsDirectoryPathState tests PASSED');
```

- [ ] **Step 2: Run test to verify it fails**

Run: `node src/lib/settingsDirectoryPathState.test.mjs`
Expected: FAIL with module-not-found or missing export because `settingsDirectoryPathState.ts` does not exist yet

- [ ] **Step 3: Write minimal implementation**

```ts
export function getDirectoryInputValue(value: string | null | undefined): string {
  return value ?? '';
}

export function toOptionalDirectoryValue(value: string): string | null {
  const trimmed = value.trim();
  return trimmed ? trimmed : null;
}

export function getTaskLocalPathPlaceholder(globalLocalPath: string): string {
  return globalLocalPath.trim();
}

export function getTaskLocalPathHint(message: string): string {
  return message;
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `node src/lib/settingsDirectoryPathState.test.mjs`
Expected: PASS and prints `settingsDirectoryPathState tests PASSED`

### Task 2: Add Reusable Directory Picker Input

**Files:**
- Create: `src/components/settings/DirectoryPathInput.vue`
- Modify: `src/pages/SettingsPage.vue`

- [ ] **Step 1: Create the reusable component**

```vue
<script setup lang="ts">
import { FolderOpen } from 'lucide-vue-next';
import { computed } from 'vue';

interface Props {
  modelValue: string;
  placeholder?: string;
  title?: string;
  disabled?: boolean;
  picking?: boolean;
}

const props = withDefaults(defineProps<Props>(), {
  placeholder: '',
  title: '',
  disabled: false,
  picking: false,
});

const emit = defineEmits<{
  'update:modelValue': [value: string];
  browse: [];
}>();

const isDisabled = computed(() => props.disabled || props.picking);
</script>

<template>
  <div class="flex gap-2">
    <input
      :value="modelValue"
      type="text"
      :placeholder="placeholder"
      :disabled="disabled"
      class="flex-1 p-2 border border-slate-300 rounded-lg focus:ring-2 focus:ring-blue-500 focus:border-blue-500 outline-none transition-all disabled:bg-slate-100 disabled:cursor-not-allowed"
      @input="emit('update:modelValue', ($event.target as HTMLInputElement).value)"
    />
    <button
      type="button"
      :disabled="isDisabled"
      :title="title"
      class="p-2 text-slate-400 hover:text-blue-600 hover:bg-blue-50 rounded-lg transition-colors border border-slate-300 bg-white disabled:opacity-60 disabled:cursor-not-allowed"
      @click="emit('browse')"
    >
      <FolderOpen class="w-4 h-4" />
    </button>
  </div>
</template>
```

- [ ] **Step 2: Replace the global local-path input**

Use `DirectoryPathInput` for `config.local_path` in the Local Storage section and wire its `browse` event to a shared folder-pick handler.

- [ ] **Step 3: Replace the manual deploy local-path input**

Use `DirectoryPathInput` for `manualLocalPath` in the Manual Deployment section and wire it to the same folder-pick flow.

- [ ] **Step 4: Keep cancel behavior safe**

When `openDirectory()` returns `null`, leave the current value untouched and do not show an error toast.

### Task 3: Update Task Editor Copy and Nullable Binding

**Files:**
- Modify: `src/pages/SettingsPage.vue`
- Modify: `src/locales/messages.ts`

- [ ] **Step 1: Add a nullable computed binding for task local path**

Map `taskForm.local_path` to a string via the helper module so the component always receives a string while task state still stores `null` for “use global default”.

- [ ] **Step 2: Replace the task local-path input**

Use `DirectoryPathInput` in the task modal, rename the label to “Local Path”, and set the placeholder to the current global local target directory.

- [ ] **Step 3: Add the default-path hint copy**

Add a small hint below the task-level field:
- Chinese: `留空时使用“本地存储”中的本地目标目录`
- English: `Leave empty to use the Local Storage target directory`

- [ ] **Step 4: Update i18n keys**

Update `src/locales/messages.ts` so:
- task field label no longer says “override”
- browse button titles remain consistent
- new task hint text exists in both English and Chinese

- [ ] **Step 5: Run verification**

Run:
- `node src/lib/settingsDirectoryPathState.test.mjs`
- `pnpm check`

Expected:
- helper test passes
- type-check exits successfully
