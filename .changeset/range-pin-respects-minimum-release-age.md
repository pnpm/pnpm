---
"@pnpm/engine.pm.commands": patch
"pacquet": patch
"pnpm": patch
---

A `devEngines.packageManager` range now records a pnpm version in `pnpm-lock.yaml` that meets `minimumReleaseAge`. If the running pnpm is newer than the cutoff, pnpm records the newest version in the range that is old enough [#16431](https://github.com/pnpm/pnpm/issues/16431).
