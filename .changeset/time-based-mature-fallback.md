---
"@pnpm/installing.deps-resolver": patch
"@pnpm/resolving.npm-resolver": patch
"@pnpm/resolving.resolver-base": patch
"@pnpm/store.controller-types": patch
"pnpm": patch
"pacquet": patch
---

With `resolutionMode: time-based`, a transitive dependency that has no matching version published before the time-based cutoff now resolves to the lowest matching version allowed by `minimumReleaseAge`. pnpm picks a version younger than `minimumReleaseAge` only if no older version matches [#16298](https://github.com/pnpm/pnpm/issues/16298).
