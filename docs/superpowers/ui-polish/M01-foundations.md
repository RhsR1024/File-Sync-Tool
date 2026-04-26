# M01 — Design Tokens & Shared Primitives

- **Phase**: 1 (foundation, must run first)
- **Risk**: Low — adds new files / extends existing; does not modify existing pages
- **Estimated changes**: ~5 new files, 1 small extension to `Empty.vue`
- **Hard rule**: Pages do NOT migrate to new primitives in this module. Migration happens in Phase 2-5 modules. M01 only ships the toolkit.

---

## Goal

Lay down the design system primitives the rest of the polish work needs:

1. A central tokens module so every page reads radius/shadow/spacing scales from the same source.
2. A shared `<Toast />` that pages can opt into instead of each maintaining its own status-message timer.
3. A shared `<LoadingSkeleton />` that pages can drop in for any async > 300ms operation.
4. A small extension to `<Empty />` so all empty states share the same shape (icon, title, description, optional action).
5. A reusable `useToast` composable wired to a global toast queue, so pages don't have to manage refs.

This module SHIPS but is NOT YET CONSUMED by existing pages. Adoption happens module by module afterward.

---

## Files

**Create:**
- `src/lib/uiTokens.ts` — design tokens as TypeScript constants (no runtime cost; consumed via Tailwind class strings)
- `src/components/Toast.vue` — single toast item (presentational)
- `src/components/ToastContainer.vue` — fixed bottom-right stack mounted globally
- `src/components/LoadingSkeleton.vue` — generic skeleton (text-line, card, list-row variants)
- `src/composables/useToast.ts` — global queue + `pushToast(message, tone, ttl)` API

**Extend:**
- `src/components/Empty.vue` — add optional `actionLabel` + `@action` slot/prop pair so pages can render "Sync now" / "Retry" / "Open settings" CTAs from the empty state
- `src/App.vue` — mount `<ToastContainer />` once globally (does not affect existing per-page toasts)

**Touch (for i18n):**
- `src/locales/messages.ts` — add `common.toast.dismiss` (zh: `关闭`, en: `Dismiss`) for the toast close button. No other strings.

---

## Token specifications

### `uiTokens.ts` shape

Export plain string constants — the values are Tailwind class fragments so components can compose them. NO CSS variables (Tailwind 4 already handles theming via its config).

```
export const RADIUS = {
  card: 'rounded-2xl',
  hero: 'rounded-[24px]',
  button: 'rounded-xl',
  pill: 'rounded-full',
  input: 'rounded-lg',
} as const;

export const SHADOW = {
  resting: 'shadow-sm',
  card:   'shadow-[0_14px_40px_rgba(15,23,42,0.06)]',
  hero:   'shadow-[0_18px_60px_rgba(15,23,42,0.08)]',
  modal:  'shadow-[0_20px_70px_rgba(15,23,42,0.18)]',
} as const;

export const SURFACE = {
  page:  'bg-slate-50',
  card:  'bg-white/85 backdrop-blur',
  cardOpaque: 'bg-white',
  inset: 'bg-slate-100',
  sidebar: 'bg-[#0b1220]',
  terminal: 'bg-[#0f172a]',
} as const;

export const TEXT = {
  heading:  'text-slate-950 font-semibold',
  body:     'text-slate-700',
  muted:    'text-slate-500',
  caption:  'text-slate-400 text-xs',
  inverted: 'text-white',
} as const;

export const FOCUS_RING = 'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500/50 focus-visible:ring-offset-2 focus-visible:ring-offset-white';

export const TRANSITION = {
  default: 'transition-colors duration-150',
  motion:  'transition-all duration-200 ease-out',
  modal:   'transition-opacity duration-200',
} as const;

export const ICON_SIZE = {
  inline: 'h-3.5 w-3.5',
  sm:     'h-4 w-4',
  md:     'h-5 w-5',
  lg:     'h-6 w-6',
} as const;
```

These are guidelines, not enforcement. Pages cite them when a design choice is non-obvious.

### Toast contract

```
type ToastTone = 'info' | 'success' | 'error' | 'warning';
interface Toast {
  id: string;            // uuid
  message: string;
  tone: ToastTone;
  ttlMs?: number;        // default 3000; 0 means persistent until dismissed
  action?: { label: string; onClick: () => void };
}
```

`useToast()` exposes:
- `pushToast(message, tone, opts?) => string` — returns id
- `dismissToast(id)`
- `clearToasts()`
- `toasts` — readonly ref array

Internals: a module-scoped reactive array, no Pinia / no Vuex. Auto-dismissal handled by per-toast `setTimeout`. Container watches the array.

### Toast UX

- Position: fixed bottom-right, 24px from edges
- Stack: newest on top; max 4 visible, older ones fall off
- Width: 320px-420px responsive
- Animation: slide-in from right (160ms ease-out), fade-out on dismiss (120ms)
- Tone styling:
  - `success`: emerald-500 left border + CheckCircle2 icon
  - `error`: rose-500 left border + AlertCircle icon
  - `warning`: amber-500 left border + AlertTriangle icon
  - `info`: indigo-500 left border + Info icon
- ARIA: `role="status"` for info/success, `role="alert"` for error/warning
- Reduced motion: respect `prefers-reduced-motion`, drop slide animation
- Close button has `aria-label` from `common.toast.dismiss`
- ESC dismisses the topmost toast

### LoadingSkeleton variants

```
<LoadingSkeleton variant="text-line" :lines="3" />        // 3 stacked text-line shimmers
<LoadingSkeleton variant="card" />                        // card-sized rectangular shimmer
<LoadingSkeleton variant="list-row" :count="5" />         // 5 list-row shimmers
<LoadingSkeleton variant="custom" class="h-32 w-full" />  // free-size; consumer supplies class
```

Implementation: a single component using Tailwind animate-pulse on a slate-200/50 background. No third-party shimmer library.

Reduced-motion: drop the pulse, keep static gray background so the shape still indicates "something coming".

### Empty.vue extension

Existing props: `icon`, `title`, `description`, `dashed` — keep all.

Add:
- `actionLabel?: string` — when provided, render a primary button below description
- `actionTone?: 'primary' | 'subtle'` — default `primary`
- emit `@action` when button clicked
- `aria-live="polite"` on the wrapper so screen readers announce the empty state

If `actionLabel` is omitted, render exactly as today (no visual difference).

---

## Issues addressed (not pages — toolkit)

This module fixes the *cause* of three patterns that keep showing up across the inventory:

1. **Toast/status timing inconsistency** — `TaskStatusPage` 2.4s, `AboutPage` 3.2s, `SettingsPage` 3s. Fix: shared 3s default, configurable.
2. **Empty states drift** — some pages use `Empty.vue` with dashed border, others without. Fix: standardize the prop set.
3. **No skeleton loaders** — pages either show blank or a spinner. Fix: ship skeletons; pages adopt them in their own modules.

---

## Out of scope

- DO NOT modify any existing page to consume the new primitives. That happens in M03+.
- DO NOT delete the existing per-page `showToast` / `showStatus` / `showStatusMsg` helpers. They stay until pages migrate.
- DO NOT change Tailwind config. Use existing classes.
- DO NOT introduce a state management library (Pinia, Vuex). The composable uses a module-scoped reactive ref.
- DO NOT add a third-party toast or skeleton library.

---

## Verification

After implementation:

1. `pnpm check` — clean.
2. `node --test src/composables/useToast.test.mjs` — write 3-5 tests:
   - `pushToast` returns id and adds to queue
   - auto-dismiss after `ttlMs`
   - `dismissToast(id)` removes the right one
   - `clearToasts` empties the queue
   - `pushToast` with `ttlMs=0` does NOT auto-dismiss
3. Manual smoke: in `App.vue`, temporarily call `pushToast('Hello', 'success')` from `onMounted`, confirm it appears bottom-right and disappears in 3s. **Remove the test call before the diff is shown to user.**
4. Visual: confirm `<Empty />` with no `actionLabel` renders identically to before (use a screenshot or compare with `git diff`).

---

## Reporting back

Subagent should report:
- The 5 new files created (paths only)
- The 2 extended files (paths + 1-line summary)
- Test results
- Any deviation from this spec, with reason

Total report under 250 words.
