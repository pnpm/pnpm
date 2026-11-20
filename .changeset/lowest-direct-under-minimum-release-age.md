---
"@pnpm/resolving.npm-resolver": patch
"pacquet": patch
"pnpm": patch
---

`resolutionMode` is no longer ignored when `minimumReleaseAge` is in effect. `lowest-direct` and `time-based` pick the lowest satisfying version of a direct dependency again; previously any active release-age cutoff — including the built-in default — silently forced the highest, so `resolutionMode` only worked when `minimumReleaseAge: 0` was set explicitly [#13752](https://github.com/pnpm/pnpm/issues/13752).
