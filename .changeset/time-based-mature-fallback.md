---
"@pnpm/installing.deps-resolver": patch
"@pnpm/resolving.npm-resolver": patch
"@pnpm/resolving.resolver-base": patch
"@pnpm/store.controller-types": patch
"pnpm": patch
"pacquet": patch
---

When time-based resolution has no matching version before its cutoff, prefer a version allowed by `minimumReleaseAge` before falling back to an immature version. This avoids unnecessary install failures when a mature matching version is available. Fixes https://github.com/pnpm/pnpm/issues/16298.
