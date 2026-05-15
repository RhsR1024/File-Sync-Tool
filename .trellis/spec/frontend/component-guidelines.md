# Component Guidelines

> How components are built in this project.

---

## Overview

<!--
Document your project's component conventions here.

Questions to answer:
- What component patterns do you use?
- How are props defined?
- How do you handle composition?
- What accessibility standards apply?
-->

(To be filled by the team)

---

## Component Structure

<!-- Standard structure of a component file -->

(To be filled by the team)

---

## Props Conventions

<!-- How props should be defined and typed -->

(To be filled by the team)

---

## Styling Patterns

<!-- How styles are applied (CSS modules, styled-components, Tailwind, etc.) -->

### Clipboard List Layout

The Alt+C clipboard panel renders the same `ClipboardItem` cards in normal history,
favorite drag lists, and the pinned section. Item height must be calculated through
`src/lib/clipboardListLayout.ts` instead of hard-coded per wrapper.

```typescript
// Good: wrappers ask the shared layout model for the visible section height.
resolveClipboardPinnedSectionHeight(items, displaySettings, { compact: true });

// Good: row renderers ask the same model for each item height.
resolveClipboardItemHeight(item, displaySettings, { compact: true });
```

Do not use independent constants such as `compact ? 98 : 124` for pinned rows.
Pinning should only move an item into the pinned section; it must not change the
card's image preview height or text row height.

### Clipboard Activation and Preview Lifecycle

`ClipboardList` row activation emits `select` before `activate`, and
`ClipboardPanelPage.onListSelect()` can schedule a delayed hover preview. Any
activation path that pastes through Tauri must clear the preview lifecycle at the
paste boundary before invoking backend paste commands.

```typescript
// Good: clears visible previews and pending show timers before the backend hides
// the panel and simulates paste.
async function paste(id: number, plain: boolean) {
  preview.hideNow();
  if (plain) await clipboardApi.pastePlain(id);
  else await clipboardApi.paste(id);
}
```

Do not rely only on `@pointerdown.capture` or the Rust `finish_paste()` hide call:
the frontend can schedule a new preview between pointerdown and activation. Keep
a regression assertion in `src/pages/ClipboardPanelPage.test.mjs` that
`paste()` calls `preview.hideNow()` before `clipboardApi.paste`.

---

## Accessibility

<!-- A11y requirements and patterns -->

(To be filled by the team)

---

## Common Mistakes

<!-- Component-related mistakes your team has made -->

(To be filled by the team)
