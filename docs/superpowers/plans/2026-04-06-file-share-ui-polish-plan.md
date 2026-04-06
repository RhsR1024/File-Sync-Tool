# File Share UI Polish Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Polish the desktop file-share settings page and the share-web browser so they read as one dense, professional subsystem without regressing tree navigation, thumbnails, or runtime controls.

**Architecture:** Keep the current tree-based file-share flow and the existing Rust/session contract intact, then tighten the two Vue surfaces in place. Use `src/pages/FileSharePage.vue` for the desktop information-architecture work, centralize the browser visual tokens in `src/share-web/style.css`, and keep existing i18n key names such as `sharedRootsTitle` and `changePath` so the polish stays low-risk.

**Tech Stack:** Vue 3, TypeScript, vue-i18n, Vite, Tailwind utility classes, scoped CSS

---

## Working Rules

- The worktree is already dirty. Never run `git checkout --`, `git reset --hard`, or revert unrelated hunks.
- Stage files explicitly in every commit step so the in-progress tree-unification work stays untouched.
- Do not rename existing i18n keys in `src/locales/messages.ts`; change visible strings only.
- Do not add Rust/backend work in this plan. If manual QA shows `/api/session` does not expose `session.features.thumbnail_enabled`, stop and open a follow-up backend plan instead of mixing API work into this UI pass.

## File Map

- `src/pages/FileSharePage.vue` - Desktop file-share settings page template and scoped CSS; shared directory rows, left-main/right-side layout, password inputs, compact buttons.
- `src/locales/messages.ts` - Desktop copy for "shared directory list" wording and path-action labels.
- `src/share-web/style.css` - Global share-web visual tokens and shell background treatment.
- `src/share-web/App.vue` - Share-web shell spacing, banners, and panel composition.
- `src/share-web/components/ToolbarActions.vue` - Breadcrumb readability, action grouping, session chip layout.
- `src/share-web/components/SearchBar.vue` - Compact segmented search control and aligned search actions.
- `src/share-web/components/EntryTable.vue` - Dense file rows, stable thumbnail slot, compact action buttons.
- `src/share-web/messages.ts` - Share-web copy that supports the new compact toolbar/search language.
- `src/share-web/types.ts` - Thumbnail and preview gating helpers.
- `src/share-web/types.test.mjs` - Lightweight assertion coverage for thumbnail gating behavior.

### Task 1: Lock Thumbnail Feature-Flag Behavior Before UI Polish

**Files:**
- Modify: `src/share-web/types.test.mjs`
- Modify: `src/share-web/types.ts`

- [ ] **Step 1: Extend the lightweight thumbnail assertions**

Add the following assertions to `src/share-web/types.test.mjs` after the existing thumbnail checks so the browser UI refactor cannot silently loosen the runtime gates:

```js
const noPreviewPermissionEntry = {
  ...imageEntry,
  permissions: {
    ...imageEntry.permissions,
    preview_image: false,
  },
};

assert.equal(
  shareTypes.canRenderEntryThumbnail(enabledSession, noPreviewPermissionEntry),
  false,
  'preview permission should still gate list thumbnails',
);

assert.equal(
  shareTypes.canRenderEntryThumbnail(null, imageEntry),
  false,
  'missing session data should disable thumbnails defensively',
);

assert.equal(
  shareTypes.canRenderEntryThumbnail(enabledSession, {
    ...imageEntry,
    kind: 'directory',
    is_dir: true,
  }),
  false,
  'directories should never render image thumbnails',
);
```

- [ ] **Step 2: Run the thumbnail assertions**

Run: `node src/share-web/types.test.mjs`
Expected: PASS if the current dirty worktree already contains the helper logic, or FAIL on one of the new assertions if the helper drifted.

- [ ] **Step 3: Align the helper implementation only if the new assertions failed**

Keep the helper pair in `src/share-web/types.ts` exactly like this so the UI layer always follows the runtime feature flags:

```ts
export function canPreviewEntry(
  session: FileShareSession | null | undefined,
  entry: FileShareNode,
): boolean {
  return Boolean(
    session?.features.image_preview_enabled
    && !entry.is_dir
    && entry.permissions.preview_image
    && isImageEntry(entry.name),
  );
}

export function canRenderEntryThumbnail(
  session: FileShareSession | null | undefined,
  entry: FileShareNode,
): boolean {
  return Boolean(session?.features.thumbnail_enabled) && canPreviewEntry(session, entry);
}
```

- [ ] **Step 4: Re-run the lightweight thumbnail test**

Run: `node src/share-web/types.test.mjs`
Expected: PASS with the final line `share-web types tests PASSED`

- [ ] **Step 5: Commit the guardrail**

```bash
git add src/share-web/types.ts src/share-web/types.test.mjs
git commit -m "test: lock file share thumbnail gating"
```

### Task 2: Reshape the Desktop Settings Page Into a Dense Shared Directory Workbench

**Files:**
- Modify: `src/pages/FileSharePage.vue`
- Modify: `src/locales/messages.ts`

- [ ] **Step 1: Update the desktop copy without renaming translation keys**

Change only the visible strings under `tools.fileShare` in `src/locales/messages.ts`:

```ts
sharedRootsTitle: 'Shared Directory List',
sharedRootsDescription: 'The service mounts every saved directory in this list when it starts.',
changePath: 'Change Path',
```

```ts
sharedRootsTitle: '共享目录列表',
sharedRootsDescription: '启动共享时会挂载这里保存的全部共享目录。',
changePath: '更换路径',
```

- [ ] **Step 2: Replace the large shared-root cards with compact directory rows**

In `src/pages/FileSharePage.vue`, keep the existing `draft.roots` data flow and `addRoot(root)` handler, but swap the current root-card markup for this denser structure:

```vue
<div class="grid grid-cols-1 gap-5 xl:grid-cols-[minmax(0,1.7fr)_minmax(320px,380px)]">
  <div class="space-y-4">
    <div class="fs-card">
      <div class="mb-4 flex items-center justify-between gap-3">
        <div>
          <p class="fs-label-sm">{{ t('tools.fileShare.sharedRootsTitle') }}</p>
          <p class="text-sm text-slate-500">{{ t('tools.fileShare.sharedRootsDescription') }}</p>
        </div>
        <button type="button" :disabled="formDisabled" @click="addRoot()" class="fs-btn fs-btn-soft">
          <Plus class="h-4 w-4" />{{ t('tools.fileShare.addDir') }}
        </button>
      </div>

      <div v-if="draft.roots.length === 0" class="rounded-xl border border-dashed border-slate-200 bg-slate-50 px-4 py-8 text-center text-sm text-slate-500">
        {{ t('tools.fileShare.noDirs') }}
      </div>

      <div v-else class="fs-root-list">
        <div v-for="(root, index) in draft.roots" :key="root.id" class="fs-root-row">
          <div class="fs-root-row-top">
            <div class="min-w-0">
              <label class="fs-label">{{ t('tools.fileShare.aliasLabel') }}</label>
              <input v-model="root.alias" :disabled="formDisabled" class="fs-input w-full" />
            </div>

            <div class="fs-root-actions">
              <label class="fs-inline-toggle">
                <span class="fs-toggle">
                  <input v-model="root.enabled" type="checkbox" :disabled="formDisabled" class="sr-only">
                  <span class="fs-toggle-track" :class="root.enabled ? 'bg-teal-600' : 'bg-slate-300'"><span class="fs-toggle-thumb" :class="root.enabled ? 'translate-x-4' : 'translate-x-0'"></span></span>
                </span>
                <span>{{ t('tools.fileShare.enabledLabel') }}</span>
              </label>

              <button type="button" :disabled="formDisabled" @click="addRoot(root)" class="fs-btn fs-btn-plain fs-btn-compact">
                {{ t('tools.fileShare.changePath') }}
              </button>

              <button type="button" :disabled="formDisabled" @click="draft.roots.splice(index, 1)" class="fs-btn fs-btn-danger fs-btn-icon">
                <Trash2 class="h-4 w-4" />
              </button>
            </div>
          </div>

          <div class="fs-root-path" :title="root.path">{{ root.path }}</div>
        </div>
      </div>
    </div>
  </div>

  <div class="space-y-4 xl:sticky xl:top-6">
```

- [ ] **Step 3: Tighten the desktop visual system and fix password-input overlap**

Update the scoped CSS in `src/pages/FileSharePage.vue` so the dense layout and password fields are stable on desktop and narrow widths:

```css
.fs-card,.fs-stat{border:1px solid rgb(226 232 240 / .9);border-radius:.875rem;background:#fff;box-shadow:0 8px 24px rgb(15 23 42 / .05)}
.fs-card{padding:1rem}.fs-stat{padding:.9rem}
.fs-root-list{display:flex;flex-direction:column;gap:.75rem}
.fs-root-row{border:1px solid rgb(226 232 240 / .9);border-radius:.875rem;background:linear-gradient(180deg,#fff 0%,rgb(248 250 252) 100%);padding:.9rem}
.fs-root-row-top{display:grid;grid-template-columns:minmax(0,1fr) auto;gap:.75rem;align-items:end}
.fs-root-actions{display:flex;flex-wrap:wrap;justify-content:flex-end;align-items:center;gap:.5rem}
.fs-inline-toggle{display:inline-flex;align-items:center;gap:.6rem;min-height:2.5rem;padding:0 .75rem;border:1px solid rgb(226 232 240 / .85);border-radius:.75rem;background:#fff;font-size:.8125rem;font-weight:600;color:rgb(51 65 85);white-space:nowrap}
.fs-root-path{margin-top:.75rem;min-height:2.5rem;display:flex;align-items:center;padding:.7rem .85rem;border:1px solid rgb(226 232 240 / .8);border-radius:.75rem;background:rgb(248 250 252);font-size:.8125rem;color:rgb(71 85 105);white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.fs-input-with-icon{padding-left:3rem}
.fs-btn{white-space:nowrap}
.fs-btn-compact{min-height:2.5rem;padding:.65rem .9rem}
.fs-btn-icon{min-width:2.5rem;min-height:2.5rem;padding:0}
@media (max-width: 900px){.fs-root-row-top{grid-template-columns:1fr}.fs-root-actions{justify-content:flex-start}}
```

- [ ] **Step 4: Run static verification for the desktop page**

Run: `pnpm check`
Expected: PASS with no new TypeScript errors from `FileSharePage.vue` or the locale file

- [ ] **Step 5: Boot the desktop shell, verify the workbench, and commit**

Run: `pnpm tauri dev`
Expected:
- The page title still shows the file-share header and runtime badge.
- The former shared-root section now reads like a dense shared-directory list.
- Each directory row shows `Enable / Change Path / Delete` in one action cluster.
- Password inputs no longer overlap the `KeyRound` icon.

Then commit only the desktop files:

```bash
git add src/pages/FileSharePage.vue src/locales/messages.ts
git commit -m "feat: polish file share settings layout"
```

### Task 3: Unify the Share-Web Shell, Breadcrumbs, and Search Controls

**Files:**
- Modify: `src/share-web/style.css`
- Modify: `src/share-web/App.vue`
- Modify: `src/share-web/components/ToolbarActions.vue`
- Modify: `src/share-web/components/SearchBar.vue`
- Modify: `src/share-web/messages.ts`

- [ ] **Step 1: Add share-web visual tokens and tighten the shell surface**

Introduce reusable tokens in `src/share-web/style.css` and consume them in `src/share-web/App.vue` so the browser page looks related to the desktop surface without losing the dark browsing canvas:

```css
:root {
  --fs-shell-bg: #0b1626;
  --fs-panel: rgba(8, 14, 24, 0.78);
  --fs-panel-border: rgba(148, 163, 184, 0.16);
  --fs-surface: rgba(15, 23, 42, 0.52);
  --fs-surface-strong: rgba(15, 23, 42, 0.82);
  --fs-text: #edf6ff;
  --fs-muted: #8aa2ba;
  --fs-accent: #20c7b8;
  --fs-accent-2: #39bdf8;
  --fs-danger: #f87171;
}
```

```css
.page { position: relative; max-width: 1280px; margin: 0 auto; padding: 24px 18px 40px; }
.panel { display: flex; flex-direction: column; gap: 16px; padding: 20px; border-radius: 24px; border: 1px solid var(--fs-panel-border); background: var(--fs-panel); backdrop-filter: blur(16px); box-shadow: 0 24px 72px rgba(0, 0, 0, 0.28); }
.flash-banner,.error-banner { margin: 0; border-radius: 16px; padding: 12px 14px; }
.flash-banner { border: 1px solid rgba(34, 197, 94, 0.2); background: rgba(34, 197, 94, 0.1); color: #c9f8da; }
.error-banner { border: 1px solid rgba(248, 113, 113, 0.24); background: rgba(239, 68, 68, 0.12); color: #fecaca; }
```

- [ ] **Step 2: Rework breadcrumbs into a readable path strip and cluster actions on the right**

Keep the current `navigate` event contract in `src/share-web/components/ToolbarActions.vue`, but restyle the markup so the breadcrumbs read like a path instead of a row of detached chips:

```vue
<div class="toolbar-row">
  <div class="breadcrumbs-shell">
    <div class="breadcrumbs" role="navigation" aria-label="Breadcrumb">
      <template v-for="(crumb, index) in breadcrumbs" :key="crumb.node_id ?? `__home__-${index}`">
        <button
          type="button"
          class="crumb"
          :class="{ current: isCurrentCrumb(index) }"
          :disabled="busy || isCurrentCrumb(index)"
          @click="emit('navigate', crumb.node_id)"
        >
          {{ crumb.label }}
        </button>
        <span v-if="index < breadcrumbs.length - 1" class="crumb-separator" aria-hidden="true">/</span>
      </template>
    </div>
  </div>

  <div class="toolbar-actions">
    <button type="button" class="ghost-button" :disabled="busy" @click="emit('refresh')">
      {{ t('toolbar.refresh') }}
    </button>
    <button v-if="canUploadFiles()" type="button" class="primary-button" :disabled="busy" @click="emit('upload-files')">
      {{ t('toolbar.uploadFiles') }}
    </button>
    <button v-if="canUploadDirectory()" type="button" class="ghost-button" :disabled="busy" @click="emit('upload-directory')">
      {{ t('toolbar.uploadDirectory') }}
    </button>
    <button v-if="canCreateDirectory()" type="button" class="ghost-button" :disabled="busy" @click="emit('create-directory')">
      {{ t('toolbar.createDirectory') }}
    </button>
    <button v-if="canCreateText()" type="button" class="ghost-button" :disabled="busy" @click="emit('create-text')">
      {{ t('toolbar.createText') }}
    </button>
    <div v-if="sessionText" class="session-group">
      <div class="session-chip" :class="{ guest: sessionIsGuest }">{{ sessionText }}</div>
      <button v-if="sessionActionLabel" type="button" class="ghost-button" :disabled="busy" @click="emit('session-action')">
        {{ sessionActionLabel }}
      </button>
    </div>
  </div>
</div>
```

```css
.breadcrumbs-shell{min-width:0;display:flex;align-items:center;padding:10px 12px;border:1px solid rgba(148,163,184,.14);border-radius:18px;background:rgba(15,23,42,.42)}
.breadcrumbs{min-width:0;display:flex;align-items:center;gap:8px;overflow-x:auto;scrollbar-width:none}
.crumb{border:none;padding:0;background:transparent;color:var(--fs-muted);font-size:14px;white-space:nowrap}
.crumb.current{color:var(--fs-text);font-weight:700}
.crumb-separator{color:rgba(138,162,186,.55)}
.toolbar-actions{display:flex;flex-wrap:wrap;justify-content:flex-end;gap:8px}
.primary-button,.ghost-button,.session-chip{min-height:38px;border-radius:999px;padding:0 14px}
.primary-button{border:none;background:linear-gradient(135deg,var(--fs-accent-2),var(--fs-accent));color:#04111b;font-weight:700}
.ghost-button{border:1px solid rgba(148,163,184,.16);background:rgba(15,23,42,.42);color:var(--fs-text)}
.session-chip{border:1px solid rgba(148,163,184,.16);background:rgba(15,23,42,.52);color:var(--fs-text)}
.session-chip.guest{border-color:rgba(32,199,184,.18);background:rgba(32,199,184,.12)}
```

- [ ] **Step 3: Convert the search bar into a compact segmented control**

Update `src/share-web/components/SearchBar.vue` and the matching copy in `src/share-web/messages.ts`:

```ts
search: {
  scopeLabel: 'Search scope',
  current: 'Current Directory',
  global: 'All Shared Directories',
  placeholder: 'Search by file name',
  submit: 'Search',
  clear: 'Clear',
},
```

```ts
search: {
  scopeLabel: '搜索范围',
  current: '当前目录',
  global: '全部共享目录',
  placeholder: '输入文件名关键字',
  submit: '搜索',
  clear: '清空',
},
```

```vue
<div class="search-shell">
  <div class="scope-toggle" role="tablist" :aria-label="t('search.scopeLabel')">
    <button v-if="canSearchCurrent" type="button" class="scope-button" :class="{ active: scope === 'current' }" @click="emit('update:scope', 'current')">
      {{ t('search.current') }}
    </button>
    <button v-if="canSearchGlobal" type="button" class="scope-button" :class="{ active: scope === 'global' }" @click="emit('update:scope', 'global')">
      {{ t('search.global') }}
    </button>
  </div>

  <div class="search-box">
    <input
      :value="keyword"
      type="search"
      :placeholder="t('search.placeholder')"
      :disabled="busy"
      @input="emit('update:keyword', ($event.target as HTMLInputElement).value)"
      @keyup.enter="emit('search')"
    />
    <button type="button" class="search-button" :disabled="busy" @click="emit('search')">{{ t('search.submit') }}</button>
    <button type="button" class="clear-button" :disabled="busy" @click="emit('clear')">{{ t('search.clear') }}</button>
  </div>
</div>
```

```css
.scope-toggle{display:inline-flex;gap:4px;padding:4px;border:1px solid rgba(148,163,184,.14);border-radius:16px;background:rgba(15,23,42,.44)}
.scope-button{border:none;border-radius:12px;background:transparent;color:var(--fs-muted);padding:8px 12px;font-size:13px;font-weight:600}
.scope-button.active{background:linear-gradient(135deg,rgba(57,189,248,.22),rgba(32,199,184,.18));color:var(--fs-text)}
.search-box{display:grid;grid-template-columns:minmax(0,1fr) auto auto;gap:8px;flex:1;min-width:min(100%,340px)}
.search-box input{min-height:40px;border:1px solid rgba(148,163,184,.16);border-radius:16px;background:rgba(7,13,21,.78);color:var(--fs-text);padding:0 14px}
.search-button,.clear-button{min-height:40px;border:none;border-radius:14px;padding:0 14px}
.search-button{background:linear-gradient(135deg,var(--fs-accent-2),var(--fs-accent));color:#04111b;font-weight:700}
.clear-button{background:rgba(148,163,184,.12);color:var(--fs-text)}
```

- [ ] **Step 4: Run share-web static verification**

Run:
- `pnpm check`
- `pnpm build:file-share-web`

Expected: both PASS with no new type or build errors from `App.vue`, `ToolbarActions.vue`, `SearchBar.vue`, or `messages.ts`

- [ ] **Step 5: Commit the shell and toolbar polish**

```bash
git add src/share-web/style.css src/share-web/App.vue src/share-web/components/ToolbarActions.vue src/share-web/components/SearchBar.vue src/share-web/messages.ts
git commit -m "feat: tighten file share browser shell"
```

### Task 4: Make the Share-Web Table Denser Without Losing Thumbnail Stability

**Files:**
- Modify: `src/share-web/components/EntryTable.vue`

- [ ] **Step 1: Stabilize the thumbnail/icon slot and stack file name metadata**

Reshape the name cell in `src/share-web/components/EntryTable.vue` so thumbnails, file icons, and path hints all occupy predictable space:

```vue
<div class="entry-name">
  <button type="button" class="name-button" @click="emit('open', entry)">
    <span v-if="canShowThumbnail(entry)" class="entry-visual entry-visual--thumb">
      <img
        :src="fileShareApi.previewUrl(entry.node_id)"
        alt=""
        class="entry-thumb"
        loading="lazy"
        @error="markThumbnailFailed(entry.node_id)"
      >
    </span>

    <span v-else class="entry-visual entry-visual--icon" :class="{ folder: entry.is_dir }" aria-hidden="true">
      <svg v-if="entry.is_dir" viewBox="0 0 24 24">
        <path
          d="M3.5 6.5h6l2 2H20a1.5 1.5 0 0 1 1.5 1.5v7.5A2 2 0 0 1 19.5 19h-15A2 2 0 0 1 2.5 17V8.5a2 2 0 0 1 2-2Z"
          fill="currentColor"
        />
      </svg>
      <svg v-else viewBox="0 0 24 24">
        <path
          d="M7 3.5h7.5L19.5 8v12A1.5 1.5 0 0 1 18 21.5H7A2.5 2.5 0 0 1 4.5 19V6A2.5 2.5 0 0 1 7 3.5Z"
          fill="currentColor"
        />
      </svg>
    </span>

    <span class="entry-copy">
      <span class="name-text">{{ entry.name }}</span>
      <span v-if="searchActive && entry.display_path !== entry.name" class="entry-hint">{{ entry.display_path }}</span>
    </span>
  </button>
</div>
```

- [ ] **Step 2: Reduce row height and replace the loud action colors with compact semantic buttons**

Tighten the table CSS in `src/share-web/components/EntryTable.vue`:

```css
.entry-row{display:grid;grid-template-columns:minmax(0,2fr) 96px 144px minmax(112px,auto);align-items:center;gap:12px;padding:12px 16px;border-radius:16px;background:rgba(8,14,24,.62);border:1px solid rgba(148,163,184,.12)}
.entry-head{padding:10px 16px;background:rgba(255,255,255,.035);color:var(--fs-muted);font-size:12px}
.name-button{display:flex;align-items:center;gap:12px;width:100%;min-width:0;border:none;padding:0;background:transparent;color:var(--fs-text);text-align:left}
.entry-visual{display:inline-flex;align-items:center;justify-content:center;width:36px;height:36px;flex-shrink:0;border-radius:12px;background:rgba(148,163,184,.08)}
.entry-visual--icon{color:#eef4fb}
.entry-visual--icon.folder{color:#f7c85b}
.entry-thumb{width:36px;height:36px;border-radius:10px;object-fit:cover;border:1px solid rgba(148,163,184,.18);background:rgba(15,23,42,.3)}
.entry-copy{min-width:0;display:flex;flex-direction:column}
.entry-hint{margin-top:4px;color:var(--fs-muted);font-size:12px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.entry-actions{display:flex;justify-content:flex-end;gap:6px}
.icon-button{display:inline-flex;align-items:center;justify-content:center;width:34px;height:34px;border:1px solid transparent;border-radius:10px;background:rgba(15,23,42,.5);color:var(--fs-text)}
.icon-button.download{border-color:rgba(74,222,128,.18);background:rgba(34,197,94,.12);color:#dcfce7}
.icon-button.preview{border-color:rgba(56,189,248,.18);background:rgba(56,189,248,.12);color:#dbeafe}
.icon-button.rename{border-color:rgba(250,204,21,.18);background:rgba(245,158,11,.12);color:#fef3c7}
.icon-button.delete{border-color:rgba(248,113,113,.18);background:rgba(239,68,68,.12);color:#fee2e2}
```

- [ ] **Step 3: Keep the compact layout responsive instead of horizontally overflowing**

Finish the responsive cleanup in the same file:

```css
@media (max-width: 880px) {
  .entry-head { display: none; }
  .entry-row { grid-template-columns: 1fr; justify-items: start; gap: 10px; }
  .entry-actions { justify-content: flex-start; }
}
```

- [ ] **Step 4: Run share-web verification again**

Run:
- `pnpm check`
- `pnpm build:file-share-web`

Expected: both PASS with no new type or build errors from `EntryTable.vue`

- [ ] **Step 5: Commit the denser table**

```bash
git add src/share-web/components/EntryTable.vue
git commit -m "feat: compact file share entry table"
```

### Task 5: Run End-to-End Verification for Both Surfaces

**Files:**
- Verify: `src/pages/FileSharePage.vue`
- Verify: `src/share-web/App.vue`
- Verify: `src/share-web/components/ToolbarActions.vue`
- Verify: `src/share-web/components/SearchBar.vue`
- Verify: `src/share-web/components/EntryTable.vue`
- Verify: `src/share-web/types.ts`
- Verify: `src/share-web/types.test.mjs`

- [ ] **Step 1: Re-run the thumbnail gate test after all UI changes**

Run: `node src/share-web/types.test.mjs`
Expected: PASS with the final line `share-web types tests PASSED`

- [ ] **Step 2: Run the full frontend type check**

Run: `pnpm check`
Expected: PASS

- [ ] **Step 3: Run the production builds**

Run: `pnpm build`
Expected: PASS, including the embedded `pnpm build:file-share-web` step

- [ ] **Step 4: Manually verify the acceptance checklist in the running app and browser**

Run: `pnpm tauri dev`
Expected:
- The desktop page shows `Shared Directory List` / `共享目录列表` instead of `Shared Roots` / `共享根目录`.
- Each directory row keeps `Enable`, `Change Path`, and `Delete` in the same action area.
- The desktop layout still reads as left-main settings plus right-side runtime controls.
- Password fields no longer overlap the leading key icon.
- Breadcrumbs in share-web read like a path strip instead of detached chips.
- Search scope buttons feel like a compact segmented control.
- With `image_preview_enabled = true` and `thumbnail_enabled = true`, image rows show thumbnails.
- After turning `thumbnail_enabled` off in the desktop page and restarting the share service, thumbnails disappear but the rest of the table stays aligned.
- After turning `image_preview_enabled` off and restarting the share service, both thumbnails and preview actions disappear together.
- At a narrow browser width (around 880px or below), the table stacks without horizontal overflow.

- [ ] **Step 5: Confirm the branch is at a clean verification checkpoint**

Run:
- `git status --short`
- `git log --oneline -n 4`

Expected:
- No new unstaged changes beyond any unrelated pre-existing worktree files.
- The latest history already includes the task commits from this plan, so no empty "verification only" commit is needed.
