---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

Installation no longer fails with `Cannot convert undefined or null to object` when a linked local dependency provides a peer dependency that is also provided by one of its ancestors. This was reachable via `pnpm deploy --legacy`.
