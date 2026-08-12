---
"@pnpm/installing.deps-installer": patch
"pacquet": patch
"pnpm": patch
---

Removing the last dependency that references a catalog entry via the fast lockfile update no longer leaves the stale catalog entry in `pnpm-lock.yaml`.
