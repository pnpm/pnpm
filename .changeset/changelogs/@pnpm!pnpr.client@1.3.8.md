## 1.3.8

### Patch Changes

- Installs through a pnpr server now apply the project's whole verification policy. `minimumReleaseAgeExclude`, `minimumReleaseAgeIgnoreMissingTime`, `trustPolicy`, `trustPolicyExclude`, `trustPolicyIgnoreAfter`, and `trustLockfile` were ignored, so excluded packages were still held back and a lockfile containing them could be rejected.

  `trustPolicy: no-downgrade` no longer fails with `TRUST_POLICY_INCOMPATIBLE_WITH_PNPR` when a pnpr server is configured.

  `--frozen-lockfile` and `--no-prefer-frozen-lockfile` are now honored on the pnpr path, instead of resolving and rewriting the lockfile anyway. Since `frozenLockfile` defaults to `true` on CI, a CI install through a pnpr server now fails on an out-of-date lockfile rather than updating it.

- Updated dependencies:
  - @pnpm/lockfile.fs@1100.1.15
  - @pnpm/lockfile.types@1100.0.17
  - @pnpm/types@1101.7.0
