---
"@pnpm/resolving.resolver-base": minor
"@pnpm/resolving.npm-resolver": minor
"@pnpm/resolving.default-resolver": minor
"@pnpm/installing.client": minor
"@pnpm/store.connection-manager": minor
"@pnpm/testing.temp-store": minor
"@pnpm/installing.deps-installer": minor
"pnpm": patch
---

Added a per-lockfile cache for the post-resolution lockfile verification gate so repeat installs against an unchanged lockfile skip every per-package registry round trip. Stored as JSON Lines at `<cacheDir>/lockfile-verified.jsonl`: a stat-only fast path matches on size, mtime, and inode, falling back to a content hash when those drift (typical after a CI checkout).

`ResolutionVerifier` is now `{ verify, activeVerifier }` — each resolver-side verifier factory declares both the runtime check and a cache slot (key + policy snapshot + `satisfies` comparator). The default resolver chain returns a `ResolutionVerifier[]` (`createResolutionVerifiers`); the install side fans out across the list, and the cache layer records one slot per active verifier. The gate runs in full whenever the lockfile changes, any verifier rejects its cached slot, or no record exists. Future verifiers plug in by returning their own entry — no install-side changes needed [#11687](https://github.com/pnpm/pnpm/issues/11687).
