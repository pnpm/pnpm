---
"pacquet": patch
---

Fixed `pnpm deploy --legacy` to exclude dependencies that are only reachable from unselected workspace projects after `pnpm fetch`.
