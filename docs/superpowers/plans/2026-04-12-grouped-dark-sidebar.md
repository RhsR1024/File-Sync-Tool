# Grouped Dark Sidebar Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Refine the existing dark sidebar into grouped section blocks so users can jump directly to tool pages without the extra "Other Tools" click.

**Architecture:** Keep the current app shell and dark sidebar layout, but replace the flat single list with shared sidebar section metadata and grouped card-like containers. Add a tiny pure-data helper for grouping and route matching so the structure can be tested without mounting Vue components.

**Tech Stack:** Vue 3, TypeScript, Vue Router, Vue I18n, Tailwind CSS, Node-based `.mjs` assertion tests

---

### Task 1: Define and test grouped sidebar navigation metadata

**Files:**
- Create: `src/lib/sidebarNavigation.test.mjs`
- Create: `src/lib/sidebarNavigation.ts`

- [ ] **Step 1: Write the failing test**

```js
import assert from 'node:assert/strict';
import { SIDEBAR_NAV_SECTIONS, isSidebarItemActive } from './sidebarNavigation.ts';

assert.equal(SIDEBAR_NAV_SECTIONS.length, 3);
assert.deepEqual(
  SIDEBAR_NAV_SECTIONS.map((section) => section.labelKey),
  ['sidebar.commonGroup', 'sidebar.tools', 'sidebar.systemGroup'],
);

const toolPaths = SIDEBAR_NAV_SECTIONS[1].items.map((item) => item.path);
assert.deepEqual(toolPaths, [
  '/tools',
  '/tools/appliance-ssh',
  '/tools/framework-password',
  '/tools/code-statistics',
  '/tools/network',
  '/tools/screen-share',
  '/tools/file-share',
]);

assert.equal(isSidebarItemActive('/tools', SIDEBAR_NAV_SECTIONS[1].items[0]), true);
assert.equal(isSidebarItemActive('/tools/appliance-ssh', SIDEBAR_NAV_SECTIONS[1].items[0]), false);
assert.equal(isSidebarItemActive('/tools/appliance-ssh/details', SIDEBAR_NAV_SECTIONS[1].items[1]), true);
```

- [ ] **Step 2: Run test to verify it fails**

Run: `node src/lib/sidebarNavigation.test.mjs`
Expected: FAIL with a module resolution error because `src/lib/sidebarNavigation.ts` does not exist yet.

- [ ] **Step 3: Write minimal implementation**

```ts
export const SIDEBAR_NAV_SECTIONS = [
  {
    key: 'common',
    labelKey: 'sidebar.commonGroup',
    items: [
      { key: 'tasks', labelKey: 'sidebar.tasks', path: '/tasks', iconKey: 'tasks', matchMode: 'prefix' },
      { key: 'console', labelKey: 'sidebar.console', path: '/', iconKey: 'console', matchMode: 'exact' },
      { key: 'history', labelKey: 'sidebar.history', path: '/history', iconKey: 'history', matchMode: 'prefix' },
    ],
  },
  {
    key: 'tools',
    labelKey: 'sidebar.tools',
    items: [
      { key: 'tools-overview', labelKey: 'sidebar.toolsOverview', path: '/tools', iconKey: 'toolsOverview', matchMode: 'exact' },
      { key: 'appliance-ssh', labelKey: 'sidebar.applianceSsh', path: '/tools/appliance-ssh', iconKey: 'applianceSsh', matchMode: 'prefix' },
      { key: 'framework-password', labelKey: 'sidebar.frameworkPassword', path: '/tools/framework-password', iconKey: 'frameworkPassword', matchMode: 'prefix' },
      { key: 'code-statistics', labelKey: 'sidebar.codeStatistics', path: '/tools/code-statistics', iconKey: 'codeStatistics', matchMode: 'prefix' },
      { key: 'network-tools', labelKey: 'sidebar.networkTools', path: '/tools/network', iconKey: 'networkTools', matchMode: 'prefix' },
      { key: 'screen-share', labelKey: 'sidebar.screenShare', path: '/tools/screen-share', iconKey: 'screenShare', matchMode: 'prefix', runtimeKey: 'screenShare' },
      { key: 'file-share', labelKey: 'sidebar.fileShare', path: '/tools/file-share', iconKey: 'fileShare', matchMode: 'prefix', runtimeKey: 'fileShare' },
    ],
  },
  {
    key: 'system',
    labelKey: 'sidebar.systemGroup',
    items: [
      { key: 'settings', labelKey: 'sidebar.settings', path: '/settings', iconKey: 'settings', matchMode: 'prefix' },
    ],
  },
] as const;

export function isSidebarItemActive(currentPath: string, item: { path: string; matchMode?: 'exact' | 'prefix' }) {
  if (item.matchMode === 'exact' || item.path === '/') {
    return currentPath === item.path;
  }
  return currentPath === item.path || currentPath.startsWith(`${item.path}/`);
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `node src/lib/sidebarNavigation.test.mjs`
Expected: PASS with `sidebarNavigation tests PASSED`

### Task 2: Apply grouped block styling in the existing dark sidebar

**Files:**
- Modify: `src/components/Sidebar.vue`
- Modify: `src/locales/messages.ts`

- [ ] **Step 1: Update the sidebar component to use shared sections**

```ts
import { SIDEBAR_NAV_SECTIONS, isSidebarItemActive } from '@/lib/sidebarNavigation';

const sections = computed(() =>
  SIDEBAR_NAV_SECTIONS.map((section) => ({
    ...section,
    items: section.items.map((item) => ({
      ...item,
      label: t(item.labelKey),
      active: isSidebarItemActive(route.path, item),
      runtimeActive: item.runtimeKey ? appStore.toolRuntime[item.runtimeKey] : false,
    })),
  })),
);
```

- [ ] **Step 2: Replace the flat list with section blocks**

```vue
<section v-for="section in sections" :key="section.key" class="space-y-2">
  <div class="px-2 text-[11px] font-semibold uppercase tracking-[0.18em] text-slate-500">
    {{ t(section.labelKey) }}
  </div>
  <div class="rounded-2xl border border-slate-800/80 bg-slate-900/70 p-2 shadow-[inset_0_1px_0_rgba(255,255,255,0.03)]">
    <router-link v-for="item in section.items" :key="item.path" :to="item.path">
      ...
    </router-link>
  </div>
</section>
```

- [ ] **Step 3: Add the new group labels**

```ts
sidebar: {
  commonGroup: 'Quick Access',
  toolsOverview: 'Tools Overview',
  systemGroup: 'System',
}
```

For Chinese strings use escaped Unicode literals to avoid encoding drift:

```ts
commonGroup: '\u5e38\u7528\u529f\u80fd',
toolsOverview: '\u5de5\u5177\u603b\u89c8',
systemGroup: '\u7cfb\u7edf\u8bbe\u7f6e',
```

- [ ] **Step 4: Verify the sidebar remains visually compact**

Check manually in the browser:
- The dark sidebar still feels like the current version, not a new navigation system
- The "Other Tools" block is always visible and every tool is directly clickable
- Active route highlight is clear without over-brightening the whole sidebar

### Task 3: Verification

**Files:**
- Modify: `src/components/Sidebar.vue`
- Modify: `src/lib/sidebarNavigation.ts`
- Modify: `src/lib/sidebarNavigation.test.mjs`
- Modify: `src/locales/messages.ts`

- [ ] **Step 1: Run focused navigation test**

Run: `node src/lib/sidebarNavigation.test.mjs`
Expected: PASS with `sidebarNavigation tests PASSED`

- [ ] **Step 2: Run TypeScript validation**

Run: `pnpm check`
Expected: PASS with no TypeScript errors

- [ ] **Step 3: Run production build**

Run: `pnpm build`
Expected: PASS with Vite and file-share web bundle build succeeding
