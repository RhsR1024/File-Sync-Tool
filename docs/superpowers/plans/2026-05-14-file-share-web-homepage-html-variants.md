# File Share Web Homepage HTML Variants Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Produce three review-ready HTML homepage variants for the file share web interface, all aligned to the approved premium single-canvas design direction.

**Architecture:** Create a small standalone design-preview package under the project workspace that shares one design system and one mock dataset, then render three homepage variants that differ by hierarchy and emphasis rather than unrelated theme changes. Add a lightweight index page so the variants can be compared side-by-side in a browser before any Vue implementation work begins.

**Tech Stack:** HTML, CSS, vanilla JavaScript (only if needed), static preview files

---

### Task 1: Set Up the Review Artifact Workspace

**Files:**
- Create: `docs/design/file-share-web-homepage/README.md`
- Create: `docs/design/file-share-web-homepage/shared.css`
- Create: `docs/design/file-share-web-homepage/mock-data.js`
- Create: `docs/design/file-share-web-homepage/index.html`

- [ ] Create the artifact folder `docs/design/file-share-web-homepage/` so the mockups stay separate from production Vue files.
- [ ] Write `README.md` that explains the chosen direction, the 3 variants, and how to open `index.html`.
- [ ] Add `shared.css` with the common design system:
  - page background
  - canvas container
  - typography scale
  - monochrome-first palette with restrained accent
  - balanced table density tokens
  - shared buttons, chips, banners, and table primitives
- [ ] Add `mock-data.js` with one shared realistic dataset:
  - product title
  - session/account label
  - breadcrumbs
  - search placeholder
  - action labels
  - mixed file rows with realistic Chinese-first names and several file types
- [ ] Create `index.html` as the review hub linking to the 3 variants and briefly describing the difference between them.

### Task 2: Build Variant A - Editorial Workspace

**Files:**
- Create: `docs/design/file-share-web-homepage/editorial-workspace.html`
- Modify: `docs/design/file-share-web-homepage/shared.css`
- Modify: `docs/design/file-share-web-homepage/mock-data.js`

- [ ] Build the calmest, most brand-forward homepage variant.
- [ ] Make the top identity area more expressive than the other variants while keeping the page serious and tool-like.
- [ ] Give breadcrumbs and search the cleanest visual treatment of the 3 versions.
- [ ] Keep action buttons visible but visually softened so they do not dominate the canvas.
- [ ] Ensure the file table still reads as the primary content region.

### Task 3: Build Variant B - Action Gallery

**Files:**
- Create: `docs/design/file-share-web-homepage/action-gallery.html`
- Modify: `docs/design/file-share-web-homepage/shared.css`

- [ ] Build the operations-forward homepage variant.
- [ ] Increase the emphasis of upload / creation / refresh actions without breaking the minimalist system.
- [ ] Make the search and controls cluster feel more immediately actionable than Variant A.
- [ ] Keep the same density mode and file-table skeleton so the comparison stays fair.
- [ ] Preserve the premium tone; this must not regress into a generic admin dashboard.

### Task 4: Build Variant C - Content First

**Files:**
- Create: `docs/design/file-share-web-homepage/content-first.html`
- Modify: `docs/design/file-share-web-homepage/shared.css`

- [ ] Build the most stripped-down homepage variant.
- [ ] Compress the top chrome more aggressively than the other variants.
- [ ] Give the file list the highest percentage of visible space.
- [ ] Keep the controls obvious enough to remain usable even though the UI chrome is lighter.
- [ ] Make sure this version still looks deliberate and premium rather than unfinished.

### Task 5: Add Comparison Support and Review Notes

**Files:**
- Modify: `docs/design/file-share-web-homepage/index.html`
- Modify: `docs/design/file-share-web-homepage/README.md`

- [ ] Add clear navigation on the index page to open each variant quickly.
- [ ] Include a short comparison block for each option:
  - what it emphasizes
  - when it is strongest
  - what trade-off it makes
- [ ] Add a note in `README.md` describing how each variant maps back to the approved design spec:
  - same design language
  - same single-canvas layout
  - same balanced-density principle
  - different hierarchy only

### Task 6: Review the Output Before Implementation

**Files:**
- Review: `docs/design/file-share-web-homepage/index.html`
- Review: `docs/design/file-share-web-homepage/editorial-workspace.html`
- Review: `docs/design/file-share-web-homepage/action-gallery.html`
- Review: `docs/design/file-share-web-homepage/content-first.html`

- [ ] Open the review hub in a browser and verify all 3 variants load without broken relative paths.
- [ ] Verify the 3 variants are meaningfully different in structure and emphasis, not just spacing tweaks.
- [ ] Check that Chinese text, long breadcrumbs, and file names still look intentional.
- [ ] Check that the file list remains the visual center in all 3 variants.
- [ ] Record any remaining gaps before moving from mockup artifacts to Vue implementation.

## Self-Review

- Spec coverage:
  - homepage-only scope is covered
  - 3 HTML variants are covered
  - shared design system is covered
  - variant differentiation is covered
  - review-ready output is covered
- Placeholder scan:
  - no TODO/TBD placeholders remain
  - file paths are explicit
  - task responsibilities are concrete
- Consistency:
  - all variants inherit the same approved direction
  - the plan keeps layout constant and changes hierarchy, matching the design spec

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-05-14-file-share-web-homepage-html-variants.md`. Two execution options:

**1. Subagent-Driven (recommended)** - I dispatch a fresh subagent per task, review between tasks, fast iteration

**2. Inline Execution** - Execute tasks in this session using executing-plans, batch execution with checkpoints

Which approach?
