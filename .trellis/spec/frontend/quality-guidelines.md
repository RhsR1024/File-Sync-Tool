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

---

## Code Review Checklist

<!-- What reviewers should check -->

(To be filled by the team)
