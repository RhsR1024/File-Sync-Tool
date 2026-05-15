# File Share Web Homepage Redesign

## Summary

This design defines a homepage-only redesign for the file share web interface. The new direction should feel like a premium, minimalist web tool instead of a generic operating-system-style file manager. The work stops at design output for this phase: three HTML homepage options for visual review.

## Confirmed Scope

- In scope:
  - main file browsing homepage
  - visual hierarchy
  - header / breadcrumb / search / actions composition
  - file list presentation
  - empty and flash-message placement as part of homepage structure
- Out of scope for this phase:
  - login dialog redesign
  - upload dialog redesign
  - preview dialog redesign
  - implementation in `src/share-web/`
  - backend or API changes

## Design Direction

### Chosen Foundation

- Visual foundation: `Minimal Explorer`
- Product tone: premium web tool
- Layout mode: `Single Canvas`
- Density mode: `Editorial Balanced`

### What This Means In Practice

- The page should feel calm, intentional, and typographically controlled.
- The interface chrome should be reduced so the file list remains the hero.
- The design should avoid obvious "desktop file manager clone" cues.
- The page should still read as a serious tool, not a marketing landing page.

## Experience Goals

1. Make the file share homepage feel refined enough to represent a deliberate product, not just an internal utility.
2. Preserve fast scanning and low-friction actions for real file work.
3. Keep navigation, search, and upload affordances obvious without overpowering the file content area.
4. Improve visual clarity for mixed file types while keeping the list readable for text-heavy directories.

## Shared Design System For All Three Variants

### Layout

- Single centered canvas on a soft page background
- No side rail in the chosen direction
- One primary content panel containing:
  - top identity area
  - breadcrumb/navigation bar
  - search and action area
  - optional inline feedback banners
  - file list/table

### Visual Language

- Clean monochrome-first palette with one restrained accent family
- Strong typography hierarchy instead of heavy decoration
- Fine borders, soft separation, minimal shadow usage
- Rounded corners, but not overly soft or playful
- Motion should be subtle and short

### Density

- Balanced row height
- Enough whitespace to feel premium
- Still compact enough for real browsing
- Thumbnail use must stay secondary, not dominant

### Tone To Avoid

- Overly technical desktop-shell mimicry
- Glassmorphism-heavy or decorative dashboard aesthetics
- Card-grid gallery direction for the main browsing view
- Excessive gradients, loud color blocks, or startup landing-page treatment

## Variant Strategy

The three HTML options should share one design system while diverging in hierarchy and emphasis.

### Variant A: Editorial Workspace

**Role**
The calmest and most brand-forward version.

**Emphasis**
- Breadcrumb and page identity feel more curated
- Search feels elegant and central
- Actions stay visible but visually softened

**Use Case**
Best when the product should feel polished and elevated first, while still practical.

### Variant B: Action Gallery

**Role**
The operational version with stronger action affordances.

**Emphasis**
- Upload and creation actions are more prominent
- Search/filter tools feel more immediately actionable
- The page still stays minimalist, but with more interaction weight near the top

**Use Case**
Best when frequent file operations matter more than pure calmness.

### Variant C: Content First

**Role**
The most stripped-down version, optimized for content area priority.

**Emphasis**
- The page chrome becomes extremely light
- The table gets maximum visual attention
- Header and controls compress into a thinner layer

**Use Case**
Best when the team wants the highest focus on file browsing itself.

## Structural Requirements For Each HTML Option

Each homepage option must include visible representations of:

- product/title area
- session/account state
- breadcrumbs
- search
- at least one primary action
- file list
- row-level operations

The file list can be represented as realistic static mock data in the design output.

## Content Recommendations

- Use realistic mixed file names instead of lorem ipsum
- Include a few different file types so icon/badge behavior can be assessed
- Keep copy concise and product-like
- Prefer Chinese copy if the mockup includes UI text, since the product context is Chinese-first

## Evaluation Criteria

The user should be able to compare the 3 HTML options based on:

- which one feels most premium
- which one feels easiest to use
- whether actions are prominent enough
- whether the file list remains the visual center
- whether the page still feels like a true file tool

## Output Plan

Deliver three homepage HTML files:

1. `Editorial Workspace`
2. `Action Gallery`
3. `Content First`

These are review artifacts, not production-ready Vue components.

## Self-Review

- Scope is intentionally limited to homepage only.
- The chosen direction stays consistent across all variants.
- The variants differ by hierarchy and emphasis, not by unrelated theme changes.
- No backend, routing, or dialog redesign is implied in this phase.
