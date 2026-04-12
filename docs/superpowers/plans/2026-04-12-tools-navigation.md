# Tools Navigation Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the left sidebar-first navigation with a sticky top navigation bar and tools mega panel so users can jump directly to any tool from any page.

**Architecture:** Extract tool navigation metadata into a shared module, then build a `TopNavigationBar` shell that reads from that metadata and current route state. Keep `/tools` as the browse-all page, but make the primary tool entry point a click-to-open mega panel rendered from the shared definitions and runtime status.

**Tech Stack:** Vue 3, Vue Router, Vue I18n, lucide-vue-next, Tailwind CSS, Node `assert` tests via `.test.mjs`

---

## File Structure

- Modify: `src/App.vue`
  Responsibility: replace the sidebar-based shell with the sticky top-nav layout and keepalive router outlet.
- Create: `src/lib/toolsNavigation.ts`
  Responsibility: define shared app sections, tool definitions, route helpers, and runtime mapping in one place.
- Create: `src/lib/toolsNavigation.test.mjs`
  Responsibility: lock down tool ordering, route matching, and runtime key behavior before UI wiring.
- Create: `src/components/TopNavigationBar.vue`
  Responsibility: render the sticky top navigation, handle the tools trigger, route highlighting, and keyboard/escape behavior.
- Create: `src/components/ToolsMegaPanel.vue`
  Responsibility: render the tools panel grid, current-tool highlighting, runtime indicators, and “view all tools” action.
- Modify: `src/pages/ToolsHubPage.vue`
  Responsibility: reuse shared tool definitions instead of duplicating tool card metadata and align the page with the new shell.
- Modify: `src/locales/messages.ts`
  Responsibility: add top-nav and mega-panel strings while preserving the user’s version bump changes.
- Leave in place but unused: `src/components/Sidebar.vue`
  Responsibility: keep file history intact for now; remove only after the new shell is verified and the team asks for cleanup.

## Task 1: Lock Shared Navigation Rules With Tests

**Files:**
- Create: `src/lib/toolsNavigation.ts`
- Create: `src/lib/toolsNavigation.test.mjs`

- [ ] **Step 1: Write the failing test**

```js
import assert from 'node:assert/strict';

import {
  appSections,
  isPathInSection,
  resolveActiveToolPath,
  toolEntries,
} from './toolsNavigation.ts';

assert.equal(appSections[4].kind, 'tools');
assert.equal(toolEntries[1].path, '/tools/appliance-ssh');
assert.equal(resolveActiveToolPath('/tools/appliance-ssh/details'), '/tools/appliance-ssh');
assert.equal(resolveActiveToolPath('/history'), null);
assert.equal(isPathInSection('/tools/file-share', '/tools'), true);
assert.equal(isPathInSection('/settings/profile', '/settings'), true);
assert.equal(isPathInSection('/history', '/tasks'), false);

console.log('toolsNavigation tests PASSED');
```

- [ ] **Step 2: Run test to verify it fails**

Run: `node src/lib/toolsNavigation.test.mjs`

Expected: FAIL with a module-not-found error for `./toolsNavigation.ts`

- [ ] **Step 3: Write the minimal implementation**

```ts
import {
  Activity,
  BarChart3,
  Globe,
  History,
  KeyRound,
  type LucideIcon,
  ListChecks,
  MonitorUp,
  Server,
  Settings,
  Shield,
  Share2,
} from 'lucide-vue-next';

export type ToolRuntimeKey = 'screenShare' | 'fileShare' | null;

export interface ToolEntry {
  key: string;
  path: string;
  titleKey: string;
  descriptionKey: string;
  chipKey: string;
  icon: LucideIcon;
  iconClasses: string;
  runtimeKey: ToolRuntimeKey;
}

export interface AppSection {
  key: string;
  path: string;
  titleKey: string;
  icon: LucideIcon;
  kind: 'route' | 'tools';
}

export const appSections: AppSection[] = [
  { key: 'tasks', path: '/tasks', titleKey: 'sidebar.tasks', icon: ListChecks, kind: 'route' },
  { key: 'console', path: '/', titleKey: 'sidebar.console', icon: Activity, kind: 'route' },
  { key: 'history', path: '/history', titleKey: 'sidebar.history', icon: History, kind: 'route' },
  { key: 'settings', path: '/settings', titleKey: 'sidebar.settings', icon: Settings, kind: 'route' },
  { key: 'tools', path: '/tools', titleKey: 'sidebar.tools', icon: Server, kind: 'tools' },
];

export const toolEntries: ToolEntry[] = [
  {
    key: 'framework-password',
    path: '/tools/framework-password',
    titleKey: 'sidebar.frameworkPassword',
    descriptionKey: 'tools.frameworkPassword.description',
    chipKey: 'toolsHub.cards.frameworkPassword.chip',
    icon: KeyRound,
    iconClasses: 'from-amber-500 to-orange-600 shadow-amber-500/20',
    runtimeKey: null,
  },
  {
    key: 'appliance-ssh',
    path: '/tools/appliance-ssh',
    titleKey: 'sidebar.applianceSsh',
    descriptionKey: 'tools.applianceSsh.description',
    chipKey: 'toolsHub.cards.applianceSsh.chip',
    icon: Shield,
    iconClasses: 'from-sky-500 to-indigo-600 shadow-sky-500/20',
    runtimeKey: null,
  },
  {
    key: 'code-statistics',
    path: '/tools/code-statistics',
    titleKey: 'sidebar.codeStatistics',
    descriptionKey: 'codeStatistics.description',
    chipKey: 'toolsHub.cards.codeStatistics.chip',
    icon: BarChart3,
    iconClasses: 'from-emerald-500 to-teal-600 shadow-emerald-500/20',
    runtimeKey: null,
  },
  {
    key: 'network-tools',
    path: '/tools/network',
    titleKey: 'sidebar.networkTools',
    descriptionKey: 'toolsHub.cards.networkTools.description',
    chipKey: 'toolsHub.cards.networkTools.chip',
    icon: Globe,
    iconClasses: 'from-violet-500 to-fuchsia-600 shadow-violet-500/20',
    runtimeKey: null,
  },
  {
    key: 'screen-share',
    path: '/tools/screen-share',
    titleKey: 'sidebar.screenShare',
    descriptionKey: 'toolsHub.cards.screenShare.description',
    chipKey: 'toolsHub.cards.screenShare.chip',
    icon: MonitorUp,
    iconClasses: 'from-purple-500 to-indigo-600 shadow-purple-500/20',
    runtimeKey: 'screenShare',
  },
  {
    key: 'file-share',
    path: '/tools/file-share',
    titleKey: 'sidebar.fileShare',
    descriptionKey: 'toolsHub.cards.fileShare.description',
    chipKey: 'toolsHub.cards.fileShare.chip',
    icon: Share2,
    iconClasses: 'from-cyan-500 to-teal-600 shadow-cyan-500/20',
    runtimeKey: 'fileShare',
  },
];

export function isPathInSection(pathname: string, sectionPath: string): boolean {
  if (sectionPath === '/') {
    return pathname === '/';
  }

  return pathname === sectionPath || pathname.startsWith(`${sectionPath}/`);
}

export function resolveActiveToolPath(pathname: string): string | null {
  const match = toolEntries.find((entry) => isPathInSection(pathname, entry.path));
  return match?.path ?? null;
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `node src/lib/toolsNavigation.test.mjs`

Expected: PASS with `toolsNavigation tests PASSED`

- [ ] **Step 5: Commit**

```bash
git add src/lib/toolsNavigation.ts src/lib/toolsNavigation.test.mjs
git commit -m "test(ui): add shared tools navigation coverage"
```

## Task 2: Build The Top Navigation Bar And Mega Panel

**Files:**
- Create: `src/components/TopNavigationBar.vue`
- Create: `src/components/ToolsMegaPanel.vue`
- Modify: `src/locales/messages.ts`

- [ ] **Step 1: Write the failing test for any new helper logic before component code**

```js
import assert from 'node:assert/strict';

import { appSections, resolveActiveToolPath } from './toolsNavigation.ts';

assert.equal(appSections.some((section) => section.key === 'tools'), true);
assert.equal(resolveActiveToolPath('/tools/screen-share/session'), '/tools/screen-share');

console.log('top navigation helper coverage PASSED');
```

- [ ] **Step 2: Run the helper test to verify it still fails until helper code exists**

Run: `node src/lib/toolsNavigation.test.mjs`

Expected: FAIL if Task 1 has not been completed; otherwise PASS and unlock component work.

- [ ] **Step 3: Implement the components and strings**

```vue
<!-- TopNavigationBar.vue -->
<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { useI18n } from 'vue-i18n';
import { ChevronDown, Server } from 'lucide-vue-next';

import { appSections, isPathInSection, resolveActiveToolPath, toolEntries } from '@/lib/toolsNavigation';
import ToolsMegaPanel from '@/components/ToolsMegaPanel.vue';

const route = useRoute();
const router = useRouter();
const { t } = useI18n();
const isToolsOpen = ref(false);
const rootRef = ref<HTMLElement | null>(null);

const activeToolPath = computed(() => resolveActiveToolPath(route.path));

function closeTools() {
  isToolsOpen.value = false;
}

function toggleTools() {
  isToolsOpen.value = !isToolsOpen.value;
}

function onPointerDown(event: PointerEvent) {
  if (!isToolsOpen.value) return;
  if (rootRef.value?.contains(event.target as Node)) return;
  closeTools();
}

function onKeydown(event: KeyboardEvent) {
  if (event.key === 'Escape') {
    closeTools();
  }
}

onMounted(() => {
  document.addEventListener('pointerdown', onPointerDown);
  document.addEventListener('keydown', onKeydown);
});

onBeforeUnmount(() => {
  document.removeEventListener('pointerdown', onPointerDown);
  document.removeEventListener('keydown', onKeydown);
});
</script>
```

```ts
// messages.ts additions
topNav: {
  toolsMenuLabel: 'Open tools',
  viewAllTools: 'View all tools',
  toolsPanelTitle: 'Toolbox',
  toolsPanelDescription: 'Jump straight to a tool from anywhere.',
}
```

- [ ] **Step 4: Run lint/type checks against the new components**

Run: `pnpm lint src/components/TopNavigationBar.vue src/components/ToolsMegaPanel.vue src/locales/messages.ts`

Expected: PASS with no Vue/TypeScript lint errors

- [ ] **Step 5: Commit**

```bash
git add src/components/TopNavigationBar.vue src/components/ToolsMegaPanel.vue src/locales/messages.ts
git commit -m "feat(ui): add top navigation tools mega panel"
```

## Task 3: Replace The App Shell

**Files:**
- Modify: `src/App.vue`

- [ ] **Step 1: Write the failing verification target**

```text
Goal: `App.vue` no longer imports or renders `Sidebar.vue`, and renders `TopNavigationBar.vue` above the router outlet.
```

- [ ] **Step 2: Verify the current shell still uses the sidebar**

Run: `rg -n "Sidebar|TopNavigationBar" src/App.vue`

Expected: shows `Sidebar` import and no `TopNavigationBar`

- [ ] **Step 3: Implement the new shell**

```vue
<script setup lang="ts">
import TopNavigationBar from '@/components/TopNavigationBar.vue';
// keep the existing event hydration logic unchanged
</script>

<template>
  <div class="flex h-screen flex-col overflow-hidden font-sans text-slate-900 bg-[radial-gradient(circle_at_top_left,_rgba(59,130,246,0.16),_transparent_40%),linear-gradient(135deg,_#f4f8fc_0%,_#e2e8f0_100%)]">
    <TopNavigationBar />
    <main class="flex min-h-0 flex-1 flex-col overflow-hidden">
      <router-view v-slot="{ Component }">
        <keep-alive include="MainConsole,CodeStatisticsPage,NetworkToolsPage,SettingsPage">
          <component :is="Component" />
        </keep-alive>
      </router-view>
    </main>
  </div>
</template>
```

- [ ] **Step 4: Verify the new shell**

Run: `rg -n "Sidebar|TopNavigationBar" src/App.vue`

Expected: `TopNavigationBar` present, `Sidebar` absent

- [ ] **Step 5: Commit**

```bash
git add src/App.vue
git commit -m "feat(ui): replace sidebar shell with top navigation"
```

## Task 4: Rewire The Tools Hub To Shared Data

**Files:**
- Modify: `src/pages/ToolsHubPage.vue`
- Modify: `src/lib/toolsNavigation.ts`

- [ ] **Step 1: Write the failing regression test for tool ordering**

```js
import assert from 'node:assert/strict';

import { toolEntries } from './toolsNavigation.ts';

assert.deepEqual(
  toolEntries.map((entry) => entry.key),
  ['framework-password', 'appliance-ssh', 'code-statistics', 'network-tools', 'screen-share', 'file-share'],
);

console.log('tools order regression PASSED');
```

- [ ] **Step 2: Run the test and watch it fail if the shared order is wrong**

Run: `node src/lib/toolsNavigation.test.mjs`

Expected: FAIL if the order is broken; PASS once the shared source matches the desired card order

- [ ] **Step 3: Replace the page-local card list with the shared data**

```ts
import { toolEntries } from '@/lib/toolsNavigation';

const toolCards = computed(() => toolEntries);
```

```vue
<article v-for="card in toolCards" :key="card.key">
  <component :is="card.icon" class="h-6 w-6" />
  <h2>{{ t(card.titleKey) }}</h2>
  <p>{{ t(card.descriptionKey) }}</p>
</article>
```

- [ ] **Step 4: Run lint/type checks for the page**

Run: `pnpm lint src/pages/ToolsHubPage.vue src/lib/toolsNavigation.ts`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/pages/ToolsHubPage.vue src/lib/toolsNavigation.ts
git commit -m "refactor(ui): reuse shared tools metadata in hub"
```

## Task 5: Full Verification

**Files:**
- Verify only: `src/App.vue`, `src/components/TopNavigationBar.vue`, `src/components/ToolsMegaPanel.vue`, `src/lib/toolsNavigation.ts`, `src/lib/toolsNavigation.test.mjs`, `src/pages/ToolsHubPage.vue`, `src/locales/messages.ts`

- [ ] **Step 1: Run focused tests**

Run: `node src/lib/toolsNavigation.test.mjs`

Expected: PASS with `toolsNavigation tests PASSED`

- [ ] **Step 2: Run type checks**

Run: `pnpm check`

Expected: PASS

- [ ] **Step 3: Run lint**

Run: `pnpm lint`

Expected: PASS

- [ ] **Step 4: Manual verification checklist**

```text
1. Open `/`.
2. Confirm the left sidebar is gone and the top navigation is visible.
3. Click `Tools` and confirm the mega panel opens.
4. Click outside the panel and press `Esc`; both should close it.
5. Enter `/tools/appliance-ssh` and confirm `Tools` stays highlighted and the panel marks the SSH tool as current.
6. Click “View all tools” and confirm `/tools` opens with the existing card grid.
7. Check running-state dots for screen share and file share if either runtime flag is true.
```

- [ ] **Step 5: Commit**

```bash
git add src/App.vue src/components/TopNavigationBar.vue src/components/ToolsMegaPanel.vue src/lib/toolsNavigation.ts src/lib/toolsNavigation.test.mjs src/pages/ToolsHubPage.vue src/locales/messages.ts
git commit -m "feat(ui): complete top navigation redesign"
```
