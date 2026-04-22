# ALT+C Panel Group Dropdown Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Align the `Alt+C` clipboard panel group layout with ElegantClipboard by replacing the fixed left sidebar with a bottom filter bar and a bottom-right upward-opening group dropdown.

**Architecture:** Keep the current clipboard store and backend group APIs unchanged. Implement the UI change as a panel-only composition update: add a focused panel-group helper and a dedicated dropdown component, then reflow `ClipboardPanelPage.vue` so search stays near the top while filter chips and group selection move into the bottom bar.

**Tech Stack:** Vue 3 + TypeScript + Tailwind utilities, existing clipboard composables/store, Node `node:test` helper tests.

---

### Task 1: Define panel group menu view logic

**Files:**
- Create: `src/lib/clipboardPanelGroupsMenu.ts`
- Create: `src/lib/clipboardPanelGroupsMenu.test.mjs`

- [ ] **Step 1: Write a failing helper test for the default label and dropdown rows**
- [ ] **Step 2: Run the helper test and confirm it fails for missing module/exports**
- [ ] **Step 3: Implement the minimal helper to build the panel group label and menu rows**
- [ ] **Step 4: Re-run the helper test and confirm it passes**

### Task 2: Build the panel-only dropdown component

**Files:**
- Create: `src/components/clipboard/ClipboardPanelGroupMenu.vue`
- Modify: `src/locales/messages.ts`

- [ ] **Step 1: Render a bottom-right trigger that shows the active group label**
- [ ] **Step 2: Render an upward-opening menu with default, custom groups, and create action**
- [ ] **Step 3: Add hover actions for rename/delete and close-on-outside-click behavior**
- [ ] **Step 4: Add any missing locale strings needed by the panel wording**

### Task 3: Reflow the `Alt+C` panel layout

**Files:**
- Modify: `src/pages/ClipboardPanelPage.vue`

- [ ] **Step 1: Remove the fixed left `ClipboardGroupSidebar` from the panel**
- [ ] **Step 2: Keep search at the top, move filter chips into a bottom bar, and mount the new group dropdown at the right**
- [ ] **Step 3: Preserve existing selection, batch, preview, and group CRUD flows through the clipboard store**

### Task 4: Verify the targeted behavior

**Files:**
- Test: `src/lib/clipboardPanelGroupsMenu.test.mjs`
- Test: `src/lib/clipboardGroupsView.test.mjs` (regression only if needed)

- [ ] **Step 1: Run `node --test src/lib/clipboardPanelGroupsMenu.test.mjs`**
- [ ] **Step 2: Run `node --test src/lib/clipboardGroupsView.test.mjs src/lib/clipboardPanelGroupsMenu.test.mjs`**
- [ ] **Step 3: Run `cmd /c pnpm check` if the local dependency state allows it, otherwise record the blocker**
