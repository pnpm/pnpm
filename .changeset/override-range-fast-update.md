---
"@pnpm/installing.deps-installer": patch
"pacquet": patch
"pnpm": patch
---

Changing a `pnpm.overrides` entry to a version range now updates the lockfile in place when a version the lockfile already holds satisfies the range, instead of re-resolving the whole dependency graph. Only exact versions were handled before [#13696](https://github.com/pnpm/pnpm/issues/13696).
