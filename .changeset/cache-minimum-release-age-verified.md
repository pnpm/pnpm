---
"@pnpm/resolving.resolver-base": minor
"@pnpm/resolving.npm-resolver": minor
"@pnpm/resolving.default-resolver": minor
"@pnpm/installing.deps-installer": minor
"pnpm": patch
---

Added a per-lockfile cache for the post-resolution lockfile verification gate so repeat installs against an unchanged lockfile skip every per-package registry round trip. Stored as JSON Lines at `<cacheDir>/lockfile-verified.jsonl`: a stat-only fast path matches on size, mtime, and inode, falling back to a content hash when those drift (typical after a CI checkout). The cache is policy-neutral — each resolver-side verifier now declares its own `ActiveVerifier` slot (key + policy snapshot + `satisfies` comparator) on the returned `ResolutionVerifier`. The combinator in `@pnpm/resolving.default-resolver` flattens slots across sub-verifiers, and the install side persists one slot per active verifier; the gate runs in full whenever the lockfile changes, any verifier rejects its cached slot, or no record exists. Future verifiers plug in by attaching their slot to their own factory's return — no changes needed on the install side [#11687](https://github.com/pnpm/pnpm/issues/11687).
