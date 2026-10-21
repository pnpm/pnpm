## 3.1.4

### Patch Changes

- Registry revisions are no longer supported. pnpm now fails with `ERR_PNPM_REGISTRY_REVISIONS_UNSUPPORTED` on registry metadata that selects a revision, on a `+rN` version specifier, and on a lockfile entry with a `revision` field. `pnpm update --patches` is removed.

- Updated dependencies:
  - @pnpm/deps.graph-hasher@1100.3.8
  - @pnpm/lockfile.fs@1100.2.14
  - @pnpm/lockfile.types@1100.1.5
  - @pnpm/network.auth-header@1101.1.17
  - @pnpm/store.cafs@1100.3.8
  - @pnpm/store.controller-types@1101.3.4
  - @pnpm/types@1102.1.2
