---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

Projects with a pnpmfile now use the fast lockfile update paths: an unchanged pnpmfile (proven by the recorded `pnpmfileChecksum`) no longer forces a full re-resolution for removals, dependency group moves, compatible range changes, and the other in-place lockfile rewrites [#13696](https://github.com/pnpm/pnpm/issues/13696).
