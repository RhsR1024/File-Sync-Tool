# File Share Web Homepage Redesign

## Goal

Redesign the file share web homepage so it feels more like a premium web product and less like a plain system file manager, while preserving the current core file-sharing workflow.

## Requirements

- Redesign only the homepage/main browsing screen in this phase.
- Produce 3 HTML homepage options for visual review.
- Keep the current core capabilities visible in the design direction:
  - breadcrumb navigation
  - search
  - upload entry points
  - preview/download/rename/delete affordances
- Follow the confirmed visual direction:
  - minimalist foundation
  - premium web-tool feel, not OS file manager feel
  - single-canvas layout
  - balanced density, not ultra-dense and not thumbnail-heavy
- Keep the experience suitable for both file-heavy and mixed-content directories.

## Acceptance Criteria

- [ ] A written design spec captures the confirmed direction and scope.
- [ ] Three homepage HTML options are produced under one shared design system.
- [ ] The 3 options are meaningfully different in hierarchy and emphasis, not just color swaps.
- [ ] Each option remains recognizably aligned with the chosen direction.
- [ ] The output is ready for user review before implementation.

## Confirmed Design Decisions

- Visual direction: `C. Minimal Explorer`
- Product feel: closer to a premium web tool than a system file manager
- Layout: `L1. Single Canvas`
- Density: `D2. Editorial Balanced`
- Deliverable scope: homepage only
- Variation strategy: 3 homepage options within one shared design language

## Planned Homepage Variants

- `A. Editorial Workspace`
- `B. Action Gallery`
- `C. Content First`

## Technical Notes

- Existing implementation lives in `src/share-web/`.
- Current homepage structure is centered in `src/share-web/App.vue` with shared subcomponents:
  - `components/ToolbarActions.vue`
  - `components/SearchBar.vue`
  - `components/EntryTable.vue`
- This phase is design/output focused, not production implementation yet.
