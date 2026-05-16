---
"@pnpm/installing.deps-installer": minor
"pnpm": patch
---

Added a per-lockfile cache for the post-resolution lockfile verification gate (today: `minimumReleaseAge`; the cache layer itself is policy-neutral so future resolver-side verifiers plug in without changing it). Stored as JSON Lines at `<cacheDir>/lockfile-verified.jsonl`, the cache lets repeat installs against an unchanged lockfile skip every per-package registry round trip: a stat-only fast path matches on size, mtime, and inode, falling back to a content hash when those drift (typical after a CI checkout). Each active verifier owns a slot under `verifiers[key]` and contributes a `satisfies` comparator that decides whether a cached policy snapshot still covers today's policy; the gate runs in full whenever the lockfile changes, any verifier rejects its cached slot, or no record exists [#11687](https://github.com/pnpm/pnpm/issues/11687).
