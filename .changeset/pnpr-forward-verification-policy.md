---
"@pnpm/installing.deps-installer": patch
"@pnpm/pnpr.client": patch
"pnpm": patch
---

Installs through a pnpr server now apply the project's whole verification policy. `minimumReleaseAgeExclude`, `minimumReleaseAgeIgnoreMissingTime`, `trustPolicy`, `trustPolicyExclude`, `trustPolicyIgnoreAfter`, and `trustLockfile` were ignored, so excluded packages were still held back and a lockfile containing them could be rejected.

`trustPolicy: no-downgrade` no longer fails with `TRUST_POLICY_INCOMPATIBLE_WITH_PNPR` when a pnpr server is configured.

`--frozen-lockfile` and `--no-prefer-frozen-lockfile` are now honored on the pnpr path, instead of resolving and rewriting the lockfile anyway. Since `frozenLockfile` defaults to `true` on CI, a CI install through a pnpr server now fails on an out-of-date lockfile rather than updating it.
