---
"@pnpm/installing.deps-restorer": patch
"pnpm": patch
---

An install that skips resolution because `pnpm-lock.yaml` is already up to date now reacts fully to packages the lockfile removed — for example after pulling a lockfile in which a dependency was deleted. The hoist layer is recomputed, so a package that became hoistable when a direct dependency was removed is hoisted, and `pendingBuilds` entries for removed packages are dropped instead of staying pending forever.
