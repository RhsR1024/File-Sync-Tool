# UI workspace consistency polish implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` or `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver a wider, label-safe sidebar; full-width sync and remote-package workspaces; and the requested Appliance Access whitelist-source ordering without changing existing business contracts.

**Architecture:** Make presentation-only changes in the existing Vue owners and lock them with the repository's source-level `node:test` convention. Preserve stores, routes, Tauri invocations, request mappings, modal widths, and backend behavior.

**Tech Stack:** Vue 3, TypeScript, Tailwind CSS 4, Vue I18n, Lucide Vue, Node `node:test`, ESLint, Vue TypeScript compiler, Vite.

## Global Constraints

- User-facing responses remain Chinese; project planning and code documentation remain English.
- Do not change route names, translation keys, Tauri commands, configuration persistence, or backend code.
- Do not add a UI library, tooltip framework, remote font, or shared layout component.
- Preserve the user's `src-tauri/Cargo.toml` change and untracked `.codegraph/` directory.
- Keep Trellis 0.6.6 update files separate from product UI staging and review.
- Follow test-first red-green-refactor for every production change.

---

### Task 1: Sidebar responsive width and full-label hover fallback

**Files:**
- Create: `src/components/SidebarLayout.test.mjs`
- Modify: `src/components/Sidebar.vue:85`
- Modify: `src/components/Sidebar.vue:121-138`

**Interfaces:**
- Consumes: `item.label: string` from the existing translated sidebar-section computed value.
- Produces: responsive `w-72 xl:w-80` shell and native `title` fallback; no new exported API.

- [ ] **Step 1: Write the failing source-level layout test**

```js
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const sidebarSource = readFileSync(join(__dirname, 'Sidebar.vue'), 'utf8');

test('sidebar uses responsive desktop widths', () => {
  assert.match(sidebarSource, /h-screen w-72[^\"]*xl:w-80/);
  assert.doesNotMatch(sidebarSource, /h-screen w-64/);
});

test('sidebar keeps defensive truncation and exposes the full translated label', () => {
  assert.match(sidebarSource, /:title="item\.label"/);
  assert.match(sidebarSource, /truncate[^\"]*">\s*\{\{ item\.label \}\}/);
});
```

- [ ] **Step 2: Run the test and verify RED**

Run:

```powershell
node --test src/components/SidebarLayout.test.mjs
```

Expected: FAIL because `Sidebar.vue` still contains `w-64` and no `:title="item.label"`.

- [ ] **Step 3: Apply the minimal sidebar implementation**

Change the shell and link opening tags to the following exact contracts:

```vue
<div class="flex h-screen w-72 flex-col border-r border-slate-800 bg-[#0b1220] text-white shadow-[14px_0_40px_rgba(2,6,23,0.34)] z-10 xl:w-80">
```

```vue
<router-link
  :to="item.path"
  class="group flex min-h-11 items-center gap-3 rounded-xl border px-3 py-2.5 transition-[transform,box-shadow,background-color,border-color,color] duration-200 hover:-translate-y-0.5 hover:shadow-[0_10px_24px_rgba(2,6,23,0.32)] motion-reduce:hover:translate-y-0 motion-reduce:hover:shadow-none motion-reduce:transition-none focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-400/60 focus-visible:ring-offset-2 focus-visible:ring-offset-[#0b1220]"
  :class="item.active
    ? 'border-sky-500/25 bg-sky-500/10 text-slate-50 shadow-[0_10px_22px_rgba(14,165,233,0.10)] forced-colors:outline forced-colors:outline-1 forced-colors:outline-sky-300'
    : 'border-transparent text-slate-400 hover:border-slate-700/70 hover:bg-slate-800/80 hover:text-slate-100'"
  :aria-current="item.active ? 'page' : undefined"
  :title="item.label"
>
```

Keep the visible `<span class="min-w-0 flex-1 truncate text-sm font-medium leading-5 tracking-[0.01em]">` unchanged.

- [ ] **Step 4: Run the sidebar test and verify GREEN**

```powershell
node --test src/components/SidebarLayout.test.mjs
```

Expected: PASS, 2 tests, 0 failures.

---

### Task 2: Sync-console configuration tabs use the full workspace

**Files:**
- Modify: `src/pages/sync/SyncConsoleLayout.test.mjs`
- Modify: `src/components/sync/SyncConfigurationEditor.vue:736-737`
- Modify: `src/components/sync/SyncConfigurationEditor.vue:1044-1154`

**Interfaces:**
- Consumes: existing `section` prop and `shows()` routing filter.
- Produces: the same `sync-console-workspace` layout marker used by the console shell and overview; no behavior or type changes.

- [ ] **Step 1: Extend the existing layout test with the configuration editor contract**

```js
const configurationSource = readFileSync(
  join(__dirname, '..', '..', 'components', 'sync', 'SyncConfigurationEditor.vue'),
  'utf8',
);

test('sync configuration tabs use the full-width console workspace', () => {
  assert.match(configurationSource, /sync-console-workspace/);
  assert.doesNotMatch(configurationSource, /max-w-4xl/);
  assert.doesNotMatch(configurationSource, /min-h-full[^\"]*mx-auto/);
  assert.match(configurationSource, /xl:grid-cols-2/);
});
```

- [ ] **Step 2: Run the test and verify RED**

```powershell
node --test src/pages/sync/SyncConsoleLayout.test.mjs
```

Expected: the new test FAILS because the editor still has `max-w-4xl mx-auto` and no wide scan-timing grid.

- [ ] **Step 3: Remove the root width cap**

```vue
<div v-if="config" class="sync-console-workspace min-h-full w-full space-y-6 p-6 pb-24">
```

Keep the outer scroll container unchanged.

- [ ] **Step 4: Group scan timing controls responsively**

Use a wide-screen two-column content grid while preserving every control, ID, validation binding, help message, and event handler. Replace the scan-card body opening `p-6 space-y-5` wrapper with:

```vue
<div class="grid grid-cols-1 gap-6 p-6 xl:grid-cols-2 xl:gap-x-8">
```

Apply these exact wrapper classes to the four current body groups, in order:

```text
scan interval:       space-y-3
stability section:   space-y-4 xl:border-l xl:border-slate-100 xl:pl-8
copy-buffer section: space-y-3 border-t border-slate-100 pt-5
time-range section:  space-y-4 border-t border-slate-100 pt-5
```

Inside the stability section, change its inner field wrapper from `space-y-4` to `grid grid-cols-1 gap-5 2xl:grid-cols-2`. Remove only `pt-5 border-t` from the stability wrapper because the wide divider moves to `xl:border-l`. Do not change the Local Storage card or File Filters grid.

- [ ] **Step 5: Run the sync layout test and type-check**

```powershell
node --test src/pages/sync/SyncConsoleLayout.test.mjs
pnpm check
```

Expected: layout tests pass and Vue type checking exits 0.

---

### Task 3: Remote Package Replacement full-width workspace and unified panels

**Files:**
- Create: `src/pages/RemotePackagePatchLayout.test.mjs`
- Modify: `src/pages/RemotePackagePatchPage.vue:456-790`
- Modify: `src/pages/RemotePackagePatchPage.vue:806-820`

**Interfaces:**
- Consumes: all existing refs, computed values, component events, and `rpp-*` style helpers.
- Produces: full-width workspace; wide steps 3/4; responsive target-selection split; no new exported API.

- [ ] **Step 1: Write the failing remote-package layout test**

```js
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const pageSource = readFileSync(join(__dirname, 'RemotePackagePatchPage.vue'), 'utf8');

test('remote package patch uses the full tool workspace', () => {
  assert.doesNotMatch(pageSource, /max-w-7xl/);
  assert.doesNotMatch(pageSource, /mx-auto flex w-full/);
  assert.match(pageSource, /flex w-full flex-col gap-6 px-6 py-6/);
});

test('complex remote package steps span the desktop grid', () => {
  assert.match(pageSource, /xl:col-span-2[^\"]*">[\s\S]*?stepBadgeClass\(3\)/);
  assert.match(pageSource, /xl:col-span-2[^\"]*">[\s\S]*?stepBadgeClass\(4\)/);
  assert.match(pageSource, /2xl:grid-cols-\[minmax\(320px,0\.8fr\)_minmax\(0,1\.2fr\)\]/);
});

test('remote package panels use the shared card hierarchy', () => {
  assert.match(pageSource, /rounded-xl border border-slate-200 bg-white shadow-sm/);
  assert.match(pageSource, /border-b border-slate-200 bg-slate-50 px-5 py-4/);
});
```

- [ ] **Step 2: Run the test and verify RED**

```powershell
node --test src/pages/RemotePackagePatchLayout.test.mjs
```

Expected: FAIL on the maximum-width container, step spans, target grid, and panel hierarchy.

- [ ] **Step 3: Replace the workspace and step-grid classes**

```vue
<div class="flex-1 overflow-y-auto bg-slate-50">
  <div class="flex w-full flex-col gap-6 px-6 py-6">
```

```vue
<div class="grid grid-cols-1 gap-6 xl:grid-cols-[400px_minmax(0,1fr)] xl:items-start">
```

Steps 1 and 2 use:

```vue
<section class="overflow-hidden rounded-xl border border-slate-200 bg-white shadow-sm">
```

Steps 3 and 4 use:

```vue
<section class="overflow-hidden rounded-xl border border-slate-200 bg-white shadow-sm xl:col-span-2">
```

- [ ] **Step 4: Give each step a consistent header and body**

For steps 1-3, change the heading row class to:

```vue
<div class="flex items-center gap-2 border-b border-slate-200 bg-slate-50 px-5 py-4 text-sm font-semibold text-slate-800">
```

For step 4, change the heading row class to:

```vue
<div class="flex flex-col gap-3 border-b border-slate-200 bg-slate-50 px-5 py-4 md:flex-row md:items-center md:justify-between">
```

After each heading row closes, insert `<div class="p-5">`; close that body wrapper immediately before the section's `</section>`. Do not change handlers or conditional content.

- [ ] **Step 5: Split target selection into two responsive columns**

Inside step 3's `p-5` body, insert an outer `<div class="grid grid-cols-1 gap-5 2xl:grid-cols-[minmax(320px,0.8fr)_minmax(0,1.2fr)]">` with two `<div class="space-y-3">` children. Move the range from the `pickReplacement` button through `scanError` into the first child. Move the complete `template v-if="inventory"` and its following `v-else` scan-hint block into the second child. Do not edit nodes inside those ranges.

- [ ] **Step 6: Align the existing `rpp-*` helpers**

```css
.rpp-input {
  @apply rounded-lg border border-slate-300 px-3 py-2 text-sm outline-none transition-colors focus:border-blue-500 focus:ring-2 focus:ring-blue-100 disabled:cursor-not-allowed disabled:bg-slate-50 disabled:text-slate-400;
}

.rpp-primary {
  @apply inline-flex cursor-pointer items-center justify-center gap-2 rounded-lg bg-blue-600 px-3.5 py-2 text-sm font-semibold text-white transition-colors hover:bg-blue-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500/50 focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:bg-slate-300;
}

.rpp-secondary {
  @apply inline-flex cursor-pointer items-center justify-center gap-2 rounded-lg border border-slate-200 bg-white px-3.5 py-2 text-sm font-semibold text-slate-700 transition-colors hover:border-blue-200 hover:bg-blue-50 hover:text-blue-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500/50 focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50;
}
```

Replace the segment helper while preserving `rpp-segment-active`:

```css
.rpp-segment {
  @apply cursor-pointer rounded-lg border border-slate-200 bg-white px-3 py-2 text-sm font-semibold text-slate-600 transition-colors hover:bg-slate-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500/50 focus-visible:ring-offset-2;
}
```

- [ ] **Step 7: Run remote-package tests and type-check**

```powershell
node --test src/lib/remotePackagePatch.test.mjs src/pages/RemotePackagePatchLayout.test.mjs
pnpm check
```

Expected: all tests pass and Vue type checking exits 0.

---

### Task 4: Appliance Access whitelist source ordering and default guard

**Files:**
- Create: `src/pages/EnableApplianceSshPage.test.mjs`
- Modify: `src/pages/EnableApplianceSshPage.vue:57-60`
- Modify: `src/pages/EnableApplianceSshPage.vue:770-795`

**Interfaces:**
- Consumes: existing `whitelistSourceMode: Ref<'local' | 'all'>` and `WHITELIST_ALL_CIDR`.
- Produces: Allow all radio before Local IP; existing request contract remains unchanged.

- [ ] **Step 1: Write the failing option-order and default-contract test**

```js
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const pageSource = readFileSync(join(__dirname, 'EnableApplianceSshPage.vue'), 'utf8');

test('allow all is the first whitelist source and remains the default', () => {
  assert.match(pageSource, /whitelistSourceMode = ref<'local' \| 'all'>\('all'\)/);
  const allOption = pageSource.indexOf("t('tools.applianceSsh.whitelistSourceAll')");
  const localOption = pageSource.indexOf("t('tools.applianceSsh.whitelistSourceLocal')");
  assert.ok(allOption >= 0);
  assert.ok(localOption >= 0);
  assert.ok(allOption < localOption);
});

test('whitelist source modes keep their existing request mapping', () => {
  assert.match(pageSource, /const WHITELIST_ALL_CIDR = '0\.0\.0\.0\/0'/);
  assert.match(
    pageSource,
    /whitelistSourceMode\.value === 'all'[\s\S]*?WHITELIST_ALL_CIDR[\s\S]*?: undefined/,
  );
});
```

- [ ] **Step 2: Run the test and verify RED for ordering only**

```powershell
node --test src/pages/EnableApplianceSshPage.test.mjs
```

Expected: default and CIDR mapping assertions pass; option-order assertion fails because Local IP appears first.

- [ ] **Step 3: Move Allow all before Local IP**

```vue
<label class="inline-flex items-center gap-2 text-xs text-slate-700 cursor-pointer">
  <input
    v-model="whitelistSourceMode"
    type="radio"
    value="all"
    :disabled="isLoading"
    class="text-blue-600 focus:ring-blue-500 disabled:opacity-50 disabled:cursor-not-allowed"
  />
  <span>{{ t('tools.applianceSsh.whitelistSourceAll') }}</span>
</label>
<label class="inline-flex items-center gap-2 text-xs text-slate-700 cursor-pointer">
  <input
    v-model="whitelistSourceMode"
    type="radio"
    value="local"
    :disabled="isLoading"
    class="text-blue-600 focus:ring-blue-500 disabled:opacity-50 disabled:cursor-not-allowed"
  />
  <span>{{ t('tools.applianceSsh.whitelistSourceLocal') }}</span>
</label>
```

Do not modify the default ref, hint, CIDR constant, or request construction.

- [ ] **Step 4: Run Appliance Access tests and verify GREEN**

```powershell
node --test src/lib/applianceSshPresentation.test.mjs src/lib/applianceSshGroups.test.mjs src/pages/EnableApplianceSshPage.test.mjs
```

Expected: all tests pass with 0 failures.

---

### Task 5: Full quality and visual verification

**Files:**
- Verify all files changed in Tasks 1-4.

**Interfaces:**
- Consumes: completed UI/test changes from Tasks 1-4.
- Produces: verification evidence for task completion.

- [ ] **Step 1: Run all targeted tests together**

```powershell
node --test src/components/SidebarLayout.test.mjs src/pages/sync/SyncConsoleLayout.test.mjs src/lib/remotePackagePatch.test.mjs src/pages/RemotePackagePatchLayout.test.mjs src/lib/applianceSshPresentation.test.mjs src/lib/applianceSshGroups.test.mjs src/pages/EnableApplianceSshPage.test.mjs
```

Expected: 0 failures.

- [ ] **Step 2: Run type checking and lint**

```powershell
pnpm check
pnpm lint
```

Expected: both commands exit 0 with no errors.

- [ ] **Step 3: Run the production frontend build**

```powershell
pnpm build
```

Expected: Vue/Vite and file-share-web builds exit 0.

- [ ] **Step 4: Inspect whitespace and the complete product diff**

```powershell
git diff --check -- src/components/Sidebar.vue src/components/SidebarLayout.test.mjs src/components/sync/SyncConfigurationEditor.vue src/pages/sync/SyncConsoleLayout.test.mjs src/pages/RemotePackagePatchPage.vue src/pages/RemotePackagePatchLayout.test.mjs src/pages/EnableApplianceSshPage.vue src/pages/EnableApplianceSshPage.test.mjs
git diff --stat -- src
```

Expected: no whitespace errors and only approved UI/test files.

- [ ] **Step 5: Perform responsive visual checks**

```text
- 1024 px: sidebar is 288 px; all tool content stacks without horizontal scroll.
- 1440/1920 px: sidebar is 320 px; Sync tabs share gutters; Remote steps 3/4 span full width.
- English navigation: long labels are readable or expose the full title on hover.
- Keyboard: focus indicators remain visible across navigation, remote controls, and Appliance Access radios.
- Appliance Access: Allow all appears first and is selected on a fresh page load.
```

- [ ] **Step 6: Run the Trellis quality gate before completion**

Load `trellis-check`, verify spec compliance, and record any remaining manual-only checks. Do not stage the user's `src-tauri/Cargo.toml`, `.codegraph/`, visual-companion artifacts, or unrelated Trellis update files with product UI changes.

## Rollback points

1. Sidebar width/title can be reverted independently with its test.
2. Sync configuration layout can be reverted without touching config-store behavior.
3. Remote Package Replacement markup/style can be reverted without data migration.
4. Appliance Access option order can be reverted independently; backend/request contracts remain untouched.
