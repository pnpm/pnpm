---
"@pnpm/installing.deps-installer": minor
"pnpm": patch
---

Added a per-lockfile cache for the `minimumReleaseAge` lockfile revalidation gate. Stored as JSON Lines at `<cacheDir>/minimum-release-age-verified.jsonl`, the cache lets repeat installs against an unchanged lockfile skip every per-package registry round trip: a stat-only fast path matches on size, mtime, and inode, falling back to a content hash when those drift (typical after a CI checkout). The gate still runs in full whenever the lockfile changes, the policy is tightened, or no record exists, preserving the supply-chain protection from [#11583](https://github.com/pnpm/pnpm/pull/11583) [#11687](https://github.com/pnpm/pnpm/issues/11687).
