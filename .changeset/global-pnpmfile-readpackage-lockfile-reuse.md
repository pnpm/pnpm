---
"@pnpm/installing.deps-installer": patch
"pacquet": patch
"pnpm": patch
---

A `readPackage` hook in the global pnpmfile no longer goes stale when `pnpm install` reuses the lockfile. The global pnpmfile is excluded from `pnpmfileChecksum`, so editing its `readPackage` hook used to leave the previously resolved subtrees untouched. Now, while a global pnpmfile exports `readPackage`, every install re-resolves the dependency subtrees instead of reusing the recorded ones [pnpm/pnpm#15136](https://github.com/pnpm/pnpm/issues/15136).
