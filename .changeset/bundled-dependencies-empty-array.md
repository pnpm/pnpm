---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

Normalize empty `bundledDependencies` and `bundleDependencies` arrays to prevent them from being serialized in the lockfile, resolving non-deterministic lockfile changes.
