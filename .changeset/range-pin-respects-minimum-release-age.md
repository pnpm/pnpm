---
"@pnpm/engine.pm.commands": patch
"pacquet": patch
"pnpm": patch
---

A `devEngines.packageManager` range now records the running pnpm in `pnpm-lock.yaml` only if it meets `minimumReleaseAge`. Otherwise pnpm records the newest version in the range that meets it. If no version in the range does, pnpm still records the running pnpm [#16431](https://github.com/pnpm/pnpm/issues/16431).
