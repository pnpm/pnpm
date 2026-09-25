---
"@pnpm/installing.linking.hoist": patch
"pnpm": patch
"pacquet": patch
---

Fixed concurrent installs failing when replacing the same stale hoisted dependency link.
