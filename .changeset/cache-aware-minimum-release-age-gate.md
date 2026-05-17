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

Restructured the `minimumReleaseAge` lockfile revalidation gate around a generic `ResolutionVerifier` interface. Each resolver may now export a sibling verifier factory (today: `createNpmResolutionVerifier`) that re-checks an already-resolved lockfile entry against its policies; the resolver chain returns the verifier list as `resolutionVerifiers` and the install side fans out across it. A `ResolutionVerifier` carries `verify` plus `policy` and `canTrustPastCheck` — the cache contract that lets repeat installs against an unchanged lockfile skip the per-package registry round trip entirely.

Verification results are memoized in JSON Lines at `<cacheDir>/lockfile-verified.jsonl`: a stat-only fast path matches on lockfile size, mtime, and inode, falling back to a content hash when those drift (typical after a CI checkout). Every active verifier's policy contribution is merged into a single `policy` bag on the record; the gate runs in full whenever the lockfile changes, any verifier rejects the cached policy, or no record exists [#11687](https://github.com/pnpm/pnpm/issues/11687).
