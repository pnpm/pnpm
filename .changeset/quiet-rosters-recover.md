---
"@pnpm/pnpr": patch
---

pnpr now refuses stale authorization when a team roster, managed access rule, or SCIM directory read exceeds five seconds. A stalled roster or rule read no longer delays SCIM refreshes. [pnpm/pnpm#17836](https://github.com/pnpm/pnpm/issues/17836).
