## 3.1.10

### Patch Changes

- `pnpm install` with `pnprServer` now rejects registry-keyed lockfile entries whose resolution uses a non-registry source with `ERR_PNPM_RESOLUTION_SHAPE_MISMATCH`. The `pnpr` lockfile verification and resolution endpoints enforce the same check before accepting or caching a lockfile.

- Installs through a pnpr server now enforce `minimumReleaseAgeStrict` and add fallback versions to `minimumReleaseAgeExclude` when strict mode is off.

- Updated dependencies:
  - @pnpm/deps.graph-hasher@1100.3.14
  - @pnpm/error@1100.2.3
  - @pnpm/lockfile.fs@1100.2.20
  - @pnpm/lockfile.types@1100.1.9
  - @pnpm/network.auth-header@1101.1.20
  - @pnpm/store.cafs@1100.3.12
  - @pnpm/store.controller-types@1101.3.8
  - @pnpm/types@1102.1.5
