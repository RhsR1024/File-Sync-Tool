# Quality Guidelines

> Code quality standards for frontend development.

---

## Overview

<!--
Document your project's quality standards here.

Questions to answer:
- What patterns are forbidden?
- What linting rules do you enforce?
- What are your testing requirements?
- What code review standards apply?
-->

(To be filled by the team)

---

## Forbidden Patterns

<!-- Patterns that should never be used and why -->

### Browser API Compatibility

Do not use `URLSearchParams.size` to decide whether a request URL needs a query
string. Some client browsers and embedded WebViews do not expose this property;
when it is missing, `query.size > 0` evaluates to false even after parameters
were added.

```typescript
// Bad: drops the query string when URLSearchParams.size is unavailable.
const query = new URLSearchParams();
query.set('node_id', nodeId);
const suffix = query.size > 0 ? `?${query.toString()}` : '';

// Good: works anywhere URLSearchParams.toString() is available.
const queryString = query.toString();
const suffix = queryString ? `?${queryString}` : '';
```

---

## Required Patterns

<!-- Patterns that must always be used -->

(To be filled by the team)

---

## Testing Requirements

<!-- What level of testing is expected -->

When request builders depend on browser API compatibility, add a regression test
that removes or stubs the risky API surface and asserts the exact URL passed to
`fetch`.

### Pure page logic lives in `src/lib` with `node --test` coverage

When a page needs non-trivial pure logic (input parsing, target composition,
serialization, de-duplication), extract it into a `src/lib/<name>.ts` module and
cover it with a sibling `<name>.test.mjs` run via `node --test`. The test file
imports the `.ts` source directly (Node type stripping), so the lib module must
not pull in runtime-only dependencies — import shared types from
`src/lib/tauri.ts` with `import type` only (e.g. `applianceSshGroups.ts`, which
keeps the Tauri request contract out of the test runtime).

### UI renames must not leak into backend contracts

When a feature renames a concept in the UI (e.g. "jump host" → "master"), change
only i18n copy and component-level naming. Request/response field names in
`src/lib/tauri.ts` mirror Rust structs and stay untouched unless the backend
changes in the same task.

---

## Code Review Checklist

<!-- What reviewers should check -->

(To be filled by the team)
