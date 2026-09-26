---
"@pnpm/installing.deps-installer": patch
"pacquet": patch
"pnpm": patch
---

`pnpm remove` no longer re-resolves the dependency graph. The removed dependency's entries are dropped from `pnpm-lock.yaml` and anything they made unreachable is pruned, without registry access. The install still falls back to a full resolution when a surviving package resolves a peer dependency through the removed one.
