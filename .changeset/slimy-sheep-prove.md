---
"@pnpm/prune-lockfile": patch
---

Dev dependencies are not marked as prod dependencies if they are used as peer dependencies of prod dependencies.
