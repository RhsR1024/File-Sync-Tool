# State Management

> Concrete state ownership rules used by the Vue frontend.

## State categories

| Category | Owner | Examples |
| --- | --- | --- |
| Component-local UI state | `ref` / `computed` in the component | open dialogs, draft rows, validation messages, active preview |
| Cross-route runtime state | reactive module store under `src/lib/` | `appStore`, `taskStateStore`, `configStore` |
| Backend-owned persisted state | Tauri command plus explicit refresh | `AppConfig`, task records, updater state |
| Navigation state | Vue Router | sync-console tab path, tool routes |

Do not add Pinia or another state framework for a single feature. Existing stores are small reactive modules with explicit command dependencies.

## Shared configuration

`src/lib/configStore.ts` is the only long-lived `AppConfig` source for `SettingsPage` and sync-console configuration pages.

```ts
configStore.config       // AppConfig | null
configStore.ensureLoaded()
configStore.refresh()
configStore.saveSync()
configStore.saveApp()
```

Rules:

- Call `ensureLoaded()` from component lifecycle hooks; concurrent callers are deduplicated.
- Bind forms to `configStore.config`; do not copy the full object into a page-local ref.
- Use `saveSync()` only for the 13 sync-owned fields and `saveApp()` only for the 12 app-owned fields. The exact cross-layer field contract is in `../backend/config-domain-persistence.md`.
- After a save, accept the refreshed backend value as canonical because normalization may change submitted values.
- Keep transient form state (which modal is open, unsaved row input, validation text) local to the owning editor.

## Runtime stores

- `appStore` owns process-wide logs, scheduler/runtime flags, and tool runtime indicators.
- `taskStateStore` owns task groups and manual task/deploy actions shared by overview and delivery surfaces.
- Components may derive display state with `computed`, but should call store methods or Tauri wrappers for mutations.

## Common mistakes

- Holding two full `AppConfig` refs in different keep-alive pages and saving either object wholesale.
- Treating a successful invoke as the canonical value without refreshing backend state.
- Moving modal-only draft state into a global store, which leaks unfinished edits across unrelated pages.
- Using a local route-tab ref when the URL already represents the active tab.
