---
"@pnpm/lockfile.utils": patch
"@pnpm/lockfile.verification": patch
"pnpm": patch
"pacquet": patch
---

Update injected workspace dependencies when `shared-workspace-lockfile` is `false` and the dependency's own dependencies have changed.
