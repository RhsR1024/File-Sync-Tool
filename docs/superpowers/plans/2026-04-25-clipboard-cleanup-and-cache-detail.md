# Clipboard Cleanup And Cache Detail Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove deprecated clipboard settings end-to-end without compatibility fallbacks, make clipboard panel keyboard shortcuts work as shown, and move disk-cache value viewing into an explicit detail dialog.

**Architecture:** Trim the clipboard settings model at the Rust + TypeScript boundary first so removed UI controls do not survive in saved config. Then simplify the settings UI and panel behavior around the reduced model, and finally update disk-cache cleanup to fetch and show full cache values only when the user asks for details.

**Tech Stack:** Vue 3, TypeScript, Tauri 2, Rust, node:test

---

### Task 1: Lock in failing tests for the new clipboard contract

**Files:**
- Modify: `src/lib/clipboardTypes.contract.test.ts`
- Modify: `src/lib/clipboardTypes.test.mjs`
- Modify: `src/composables/clipboardInteractionHelpers.test.mjs`

- [ ] **Step 1: Write the failing contract test for the reduced clipboard settings shape**

```ts
assert.deepEqual(clipboardSettingsContract.display, {
  density: 'standard',
  preview_lines: 3,
  time_format: 'relative',
  show_char_count: true,
  show_byte_size: true,
  show_source_app: 'both',
  image_max_height: 120,
  image_auto_height: true,
  drag_indicator: true,
});

assert.deepEqual(clipboardSettingsContract.shortcuts, {
  paste: 'Enter',
  plain_paste: 'Shift+Enter',
  delete: 'Delete',
  favorite: 'Ctrl+D',
  edit: 'Ctrl+E',
  focus_search: ['Ctrl+F'],
  close: 'Escape',
});
```

- [ ] **Step 2: Run the contract test and verify it fails**

Run: `node --test src/lib/clipboardTypes.contract.test.ts`
Expected: FAIL because the old defaults still expose `show_char_count: false`, `show_source_app: 'name'`, `quick_paste`, and `focus_search: ['Ctrl+F', '/']`.

- [ ] **Step 3: Write the failing normalization test for removed fields**

```js
test('normalizeClipboardSettings ignores removed panel and toolbar settings', () => {
  const normalized = normalizeClipboardSettings({
    panel: {
      follow_cursor: false,
      remember_position: true,
      animate: false,
      use_mica: false,
    },
    toolbar: {
      visible: false,
      items: ['search'],
    },
  });

  assert.equal('panel' in normalized, false);
  assert.equal('toolbar' in normalized, false);
});
```

- [ ] **Step 4: Write the failing quick-paste helper test for the new explicit shortcut labels**

```js
test('resolveQuickPasteTargetId still maps Alt+number to visible row order', () => {
  const items = [{ id: 21 }, { id: 22 }, { id: 23 }];

  assert.equal(resolveQuickPasteTargetId(items, '1', true), 21);
  assert.equal(resolveQuickPasteTargetId(items, '3', true), 23);
  assert.equal(resolveQuickPasteTargetId(items, '3', false), null);
});
```

- [ ] **Step 5: Run the focused test files and verify at least one assertion fails in each changed file**

Run: `node --test src/lib/clipboardTypes.test.mjs src/lib/clipboardTypes.contract.test.ts src/composables/clipboardInteractionHelpers.test.mjs`
Expected: FAIL with contract/default mismatches.

### Task 2: Remove deprecated clipboard settings fields from frontend and backend models

**Files:**
- Modify: `src/lib/clipboardTypes.ts`
- Modify: `src-tauri/src/clipboard/models.rs`
- Modify: `src-tauri/src/config.rs`
- Modify: `src/lib/clipboardTypes.contract.test.ts`
- Modify: `src/lib/clipboardTypes.test.mjs`

- [ ] **Step 1: Remove `ClipboardPanelSettings`, `ClipboardToolbarSettings`, and `quick_paste` from the TypeScript model**

```ts
export interface ClipboardShortcutsSettings {
  paste: string;
  plain_paste: string;
  delete: string;
  favorite: string;
  edit: string;
  focus_search: string[];
  close: string;
}

export interface ClipboardSettings {
  // ...
  shortcuts: ClipboardShortcutsSettings;
  navigation: ClipboardNavigationSettings;
  data: ClipboardDataSettings;
  app_filter: ClipboardAppFilterSettings;
}
```

- [ ] **Step 2: Update the TypeScript defaults to the new UX defaults**

```ts
display: {
  density: 'standard',
  preview_lines: 3,
  time_format: 'relative',
  show_char_count: true,
  show_byte_size: true,
  show_source_app: 'both',
  image_max_height: 120,
  image_auto_height: true,
  drag_indicator: true,
},
shortcuts: {
  paste: 'Enter',
  plain_paste: 'Shift+Enter',
  delete: 'Delete',
  favorite: 'Ctrl+D',
  edit: 'Ctrl+E',
  focus_search: ['Ctrl+F'],
  close: 'Escape',
},
```

- [ ] **Step 3: Remove the same deprecated structs and fields from Rust**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ClipboardShortcutsSettings {
    pub paste: String,
    pub plain_paste: String,
    pub delete: String,
    pub favorite: String,
    pub edit: String,
    pub focus_search: Vec<String>,
    pub close: String,
}
```

- [ ] **Step 4: Remove old `panel` and `toolbar` expectations from Rust config/model tests**

```rust
assert!(settings.navigation.enabled);
assert_eq!(settings.shortcuts.focus_search, vec!["Ctrl+F".to_string()]);
assert!(settings.display.show_char_count);
assert_eq!(settings.display.show_source_app, ClipboardSourceAppDisplay::Both);
```

- [ ] **Step 5: Run the focused tests and verify they pass**

Run: `node --test src/lib/clipboardTypes.test.mjs src/lib/clipboardTypes.contract.test.ts`
Expected: PASS

### Task 3: Simplify the clipboard settings UI around the reduced model

**Files:**
- Modify: `src/lib/clipboardSettingsUi.ts`
- Modify: `src/components/clipboard/ClipboardSettingsPanel.vue`
- Modify: `src/components/clipboard-settings/GeneralTab.vue`
- Modify: `src/components/clipboard-settings/DisplayTab.vue`
- Modify: `src/components/clipboard-settings/ShortcutsTab.vue`
- Delete: `src/components/clipboard-settings/AboutTab.vue`
- Modify: `src/locales/messages.ts`

- [ ] **Step 1: Remove the `about` tab and toolbar-order helpers from the settings UI metadata**

```ts
export const CLIPBOARD_SETTINGS_TABS = [
  { id: 'general', labelKey: 'clipboard.settings.tabs.general', icon: Settings2 },
  { id: 'display', labelKey: 'clipboard.settings.tabs.display', icon: LayoutPanelTop },
  { id: 'shortcuts', labelKey: 'clipboard.settings.tabs.shortcuts', icon: Keyboard },
  { id: 'data', labelKey: 'clipboard.settings.tabs.data', icon: Database },
  { id: 'preview', labelKey: 'clipboard.settings.tabs.preview', icon: Eye },
  { id: 'appFilter', labelKey: 'clipboard.settings.tabs.appFilter', icon: Filter },
] as const;
```

- [ ] **Step 2: Remove `panel` / `toolbar` merge logic from the settings panel**

```ts
const next = normalizeClipboardSettings({
  ...model,
  ...patch,
  display: { ...model.display, ...(patch.display ?? {}) },
  preview: { ...model.preview, ...(patch.preview ?? {}) },
  shortcuts: {
    ...model.shortcuts,
    ...(patch.shortcuts ?? {}),
    focus_search: patch.shortcuts?.focus_search
      ? [...patch.shortcuts.focus_search]
      : [...model.shortcuts.focus_search],
  },
  navigation: { ...model.navigation, ...(patch.navigation ?? {}) },
  data: { ...model.data, ...(patch.data ?? {}) },
  app_filter: {
    ...model.app_filter,
    ...(patch.app_filter ?? {}),
    patterns: patch.app_filter?.patterns
      ? [...patch.app_filter.patterns]
      : [...model.app_filter.patterns],
  },
});
```

- [ ] **Step 3: Strip the deprecated controls from the tab components**

```vue
<!-- GeneralTab.vue -->
<div class="mt-4 space-y-3">
  <label class="flex items-start justify-between gap-4">
    <div>
      <div class="text-sm text-slate-700">
        {{ t('clipboard.settings.general.reinsertOnSelfCopy') }}
      </div>
      <div class="mt-1 text-xs leading-5 text-slate-500">
        {{ t('clipboard.settings.general.reinsertOnSelfCopyHint') }}
      </div>
    </div>
    <input
      type="checkbox"
      :checked="props.settings.reinsert_on_self_copy"
      @change="patch({ reinsert_on_self_copy: ($event.target as HTMLInputElement).checked })"
    >
  </label>
</div>
```

- [ ] **Step 4: Update the display and shortcut copy to match the new defaults**

```vue
{
  label: t('clipboard.settings.shortcuts.focusSearch'),
  value: props.settings.shortcuts.focus_search.join(' / '),
},
{
  label: t('clipboard.settings.shortcuts.quickPaste'),
  value: 'Alt+1 - Alt+9',
},
```

- [ ] **Step 5: Run the settings-related unit tests and type checks that cover these files**

Run: `node --test src/lib/clipboardSettingsUi.test.mjs src/lib/clipboardTypes.test.mjs`
Expected: PASS

### Task 4: Make clipboard panel keyboard behavior match the shown shortcuts

**Files:**
- Modify: `src/pages/ClipboardPanelPage.vue`
- Modify: `src/composables/useClipboardHotkey.ts`
- Modify: `src-tauri/src/clipboard/commands.rs`
- Modify: `src/locales/messages.ts`

- [ ] **Step 1: Write the failing panel-shell regression test for the new keyboard-first open behavior**

```js
test('clipboard panel does not auto-focus the search box on show', () => {
  assert.doesNotMatch(
    pageSource,
    /searchInput\.value\?\.focus\(\)/,
  );
});
```

- [ ] **Step 2: Run the panel regression test and verify it fails**

Run: `node --test src/pages/ClipboardPanelPage.test.mjs`
Expected: FAIL because the page still auto-focuses the search input on show.

- [ ] **Step 3: Stop auto-focusing search on open and allow Delete / Alt+1..9 to work from the selected row**

```ts
unlistenShown = await listen('clipboard-panel-shown', async () => {
  preview.hideNow();
  await refreshPreviewSettings();
  store.search.value = '';
  selectedIndex.value = 0;
  resetBatchSelection();
  await store.reload();
  await nextTick();
  showCounter.value += 1;
});
```

- [ ] **Step 4: Focus the panel window when it opens so window-level key handlers can receive input**

```rust
fn show_panel(panel: &WebviewWindow) -> Result<(), String> {
    panel.show().map_err(|e| e.to_string())?;
    panel.set_focus().map_err(|e| e.to_string())?;
    Ok(())
}
```

- [ ] **Step 5: Reduce the shown focus-search shortcuts to `Ctrl+F` only**

```ts
focus_search: ['Ctrl+F'],
```

- [ ] **Step 6: Re-run the panel and helper tests**

Run: `node --test src/pages/ClipboardPanelPage.test.mjs src/composables/clipboardInteractionHelpers.test.mjs`
Expected: PASS

### Task 5: Move disk-cache values behind a detail dialog and rename IPSAN usage to purpose

**Files:**
- Modify: `src/lib/tauri.ts`
- Modify: `src-tauri/src/disk_cleanup.rs`
- Modify: `src/pages/DiskCacheCleanupPage.vue`
- Modify: `src/locales/messages.ts`

- [ ] **Step 1: Extend the backend cache-content payload to include the full normalized value**

```rust
pub struct CacheKeyContentEntry {
    pub key: String,
    pub value_type: String,
    pub preview: String,
    pub full_value: String,
    pub truncated: bool,
}
```

- [ ] **Step 2: Mirror the new payload shape in TypeScript**

```ts
export interface CacheKeyContentEntry {
  key: string;
  value_type: string;
  preview: string;
  full_value: string;
  truncated: boolean;
}
```

- [ ] **Step 3: Replace inline cache-value rendering with a detail button + modal state**

```vue
<button
  type="button"
  class="inline-flex items-center rounded-lg border border-slate-200 bg-white px-2.5 py-1.5 text-xs font-semibold text-slate-700 transition hover:bg-slate-50"
  @click="openCacheDetail(ipsanCacheContentEntry(ipsanCacheKey(item.IPSANId)))"
>
  {{ t('diskCacheCleanup.actions.viewDetails') }}
</button>
```

- [ ] **Step 4: Add the shared modal content**

```vue
<div v-if="cacheDetailEntry" class="fixed inset-0 z-50 flex items-center justify-center bg-slate-950/45 px-4">
  <div class="w-full max-w-3xl rounded-2xl bg-white p-5 shadow-2xl">
    <div class="text-sm font-semibold text-slate-900">{{ cacheDetailEntry.key }}</div>
    <pre class="mt-4 max-h-[60vh] overflow-auto rounded-xl bg-slate-950 p-4 font-mono text-xs leading-5 text-slate-100">{{ cacheDetailEntry.full_value || t('diskCacheCleanup.cache.emptyContent') }}</pre>
  </div>
</div>
```

- [ ] **Step 5: Rename IPSAN `usage` labels to `purpose` in both locales**

```ts
ipsan: {
  columns: {
    purpose: '用途',
  },
},
```

- [ ] **Step 6: Run the focused tests and the app checks that cover the changed modules**

Run: `node --test src/lib/diskCacheCleanupPresentation.test.mjs`
Expected: PASS

### Task 6: Verification and cross-layer review

**Files:**
- Review: `src/lib/clipboardTypes.ts`
- Review: `src/components/clipboard/ClipboardSettingsPanel.vue`
- Review: `src/pages/ClipboardPanelPage.vue`
- Review: `src/pages/DiskCacheCleanupPage.vue`
- Review: `src-tauri/src/clipboard/models.rs`
- Review: `src-tauri/src/clipboard/commands.rs`
- Review: `src-tauri/src/disk_cleanup.rs`

- [ ] **Step 1: Run the full targeted frontend test set**

Run: `node --test src/lib/clipboardTypes.test.mjs src/lib/clipboardTypes.contract.test.ts src/lib/clipboardSettingsUi.test.mjs src/composables/clipboardInteractionHelpers.test.mjs src/pages/ClipboardPanelPage.test.mjs src/lib/diskCacheCleanupPresentation.test.mjs`
Expected: PASS with 0 failures.

- [ ] **Step 2: Run the Rust clipboard and disk-cleanup tests**

Run: `cargo test clipboard::models::tests --manifest-path src-tauri/Cargo.toml`
Expected: PASS

- [ ] **Step 3: Run the Rust disk-cleanup tests**

Run: `cargo test disk_cleanup --manifest-path src-tauri/Cargo.toml`
Expected: PASS

- [ ] **Step 4: Run project quality checks for the touched frontend files**

Run: `cmd /c pnpm check`
Expected: PASS

- [ ] **Step 5: Run a final cross-layer grep to ensure removed fields no longer exist in source**

Run: `rg -n "follow_cursor|remember_position|use_mica|toolbarOrder|toolbarVisible|quick_paste|tabs\\.about" src src-tauri`
Expected: no matches in active source files beyond intentional test or plan artifacts.
